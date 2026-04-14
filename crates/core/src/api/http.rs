//! Axum router, REST v1, Socket.IO layer, and album-art placeholder routes.

use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use socketioxide::SocketIo;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::albumart;
use crate::config::Config;
use crate::mpd::MpdConfig;
use super::v1;
use super::{AppState, RouterState};

const ALBUMART_UPLOAD_MAX_BYTES: usize = 1_000_000; // 1MB, match Volumio

#[derive(Debug, Deserialize, Default)]
pub struct AlbumArtQuery {
    pub web: Option<String>,
    pub path: Option<String>,
    pub metadata: Option<String>,
    pub icon: Option<String>,
    #[serde(rename = "sourceicon")]
    pub source_icon: Option<String>,
    #[serde(rename = "sectionimage")]
    pub section_image: Option<String>,
}

/// Minimal 1x1 transparent PNG used when no default image exists under albumart_root.
const FALLBACK_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

const CACHE_MAX_AGE: &str = "public, max-age=2628000"; // 30d

/// Plugin dirs to search for icon/sectionimage/sourceicon (Volumio-compatible order).
/// Bundled tree ships stock `music_service/mpd/*icon.png` so `/albumart?sourceicon=...` works
/// without copying Node's plugin tree into `/usr/share` (dev and minimal installs).
fn albumart_plugin_dirs(plugin_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let bundled =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/bundled-plugins");
    vec![
        bundled,
        plugin_dir.to_path_buf(),
        std::path::PathBuf::from("/data/plugins"),
        std::path::PathBuf::from("/usr/share/volumio-evo/plugins"),
    ]
}

/// Try to serve icon/sectionimage/sourceicon from plugin dirs before default. Returns (body, content_type).
fn try_icon_fallback(state: &AppState, q: &AlbumArtQuery) -> Option<(Vec<u8>, &'static str)> {
    let dirs = albumart_plugin_dirs(&state.config.plugin_dir);
    if let Some(ref icon) = q.icon {
        let name = format!("{}.svg", icon);
        for base in &dirs {
            let path = base.join("icons").join(&name);
            if path.exists() {
                if let Ok(data) = std::fs::read(&path) {
                    return Some((data, "image/svg+xml"));
                }
            }
        }
    }
    if let Some(ref section) = q.section_image {
        for base in &dirs {
            let path = base.join(section);
            if path.exists() {
                if let Ok(data) = std::fs::read(&path) {
                    let ct = if section.ends_with(".svg") {
                        "image/svg+xml"
                    } else if section.ends_with(".png") {
                        "image/png"
                    } else {
                        "image/jpeg"
                    };
                    return Some((data, ct));
                }
            }
        }
    }
    if let Some(ref src) = q.source_icon {
        for base in &dirs {
            let path = base.join(src);
            if path.exists() {
                if let Ok(data) = std::fs::read(&path) {
                    let ct = if src.ends_with(".svg") {
                        "image/svg+xml"
                    } else if src.ends_with(".png") {
                        "image/png"
                    } else {
                        "image/jpeg"
                    };
                    return Some((data, ct));
                }
            }
        }
    }
    None
}

/// Resize image to fit inside max_dim x max_dim, encode as JPEG. Returns None if decode fails.
fn resize_image_to_jpeg(data: &[u8], max_dim: u32) -> Option<Vec<u8>> {
    use std::io::Cursor;
    let img = image::load_from_memory(data).ok()?;
    let thumb = img.thumbnail(max_dim, max_dim);
    let mut out = Vec::new();
    thumb
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .ok()?;
    if out.is_empty() {
        return None;
    }
    Some(out)
}

/// Try to load default image from albumart_root (default.jpg then default.png); else return embedded placeholder.
fn default_album_art_bytes(state: &AppState) -> (Vec<u8>, &'static str) {
    let root = &state.config.albumart_root;
    for name in ["default.jpg", "default.png", "default.jpeg"] {
        let path = root.join(name);
        if path.exists() {
            if let Ok(data) = std::fs::read(&path) {
                let ct = if name.ends_with(".png") {
                    "image/png"
                } else {
                    "image/jpeg"
                };
                return (data, ct);
            }
        }
    }
    (FALLBACK_PNG.to_vec(), "image/png")
}

fn image_response(data: Vec<u8>, content_type: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, CACHE_MAX_AGE)
        .body(Body::from(data))
        .unwrap()
}

/// GET /albumart - resolve from path/web/online providers then icon fallbacks then default/placeholder.
async fn album_art(
    State(state): State<AppState>,
    Query(q): Query<AlbumArtQuery>,
) -> impl IntoResponse {
    let c = &state.config;
    let music_root = &c.music_sources.music_root;
    let metadata = q.metadata.as_deref() == Some("true");
    let mpd_cfg = MpdConfig {
        host: c.mpd_host.clone(),
        port: c.mpd_port,
    };
    if let Some((file_path, ct)) = albumart::resolve_async(
        &c.albumart_root,
        music_root,
        q.path.as_deref(),
        q.web.as_deref(),
        metadata,
        &c.albumart_providers,
        &c.exiftool_path,
        Some(&mpd_cfg),
    )
    .await
    {
        if let Ok(data) = std::fs::read(&file_path) {
            return image_response(data, ct);
        }
    }
    if let Some((data, ct)) = try_icon_fallback(&state, &q) {
        return image_response(data, ct);
    }
    let (data, ct) = default_album_art_bytes(&state);
    image_response(data, ct)
}

/// GET /albumartd - direct album art; same resolution as /albumart, resized to max 500px.
async fn album_art_direct(
    State(state): State<AppState>,
    Query(q): Query<AlbumArtQuery>,
) -> impl IntoResponse {
    let c = &state.config;
    let music_root = &c.music_sources.music_root;
    let metadata = q.metadata.as_deref() == Some("true");
    let mpd_cfg = MpdConfig {
        host: c.mpd_host.clone(),
        port: c.mpd_port,
    };
    if let Some((file_path, _ct)) = albumart::resolve_async(
        &c.albumart_root,
        music_root,
        q.path.as_deref(),
        q.web.as_deref(),
        metadata,
        &c.albumart_providers,
        &c.exiftool_path,
        Some(&mpd_cfg),
    )
    .await
    {
        if let Ok(data) = std::fs::read(&file_path) {
            if let Some(resized) = resize_image_to_jpeg(&data, 500) {
                return image_response(resized, "image/jpeg");
            }
            return image_response(data, "image/jpeg");
        }
    }
    if let Some((data, ct)) = try_icon_fallback(&state, &q) {
        return image_response(data, ct);
    }
    let (data, ct) = default_album_art_bytes(&state);
    image_response(data, ct)
}

/// GET /tinyart/*path - path is artist/album/resolution; resized to max 250px.
async fn album_art_tiny(
    State(state): State<AppState>,
    Path((path_from_url,)): Path<(String,)>,
    Query(q): Query<AlbumArtQuery>,
) -> impl IntoResponse {
    let c = &state.config;
    let music_root = &c.music_sources.music_root;
    let metadata = q.metadata.as_deref() == Some("true");
    let web_param = q
        .web
        .as_deref()
        .or_else(|| Some(path_from_url.as_str()))
        .filter(|s| !s.is_empty());
    let mpd_cfg = MpdConfig {
        host: c.mpd_host.clone(),
        port: c.mpd_port,
    };
    if let Some((file_path, _ct)) = albumart::resolve_async(
        &c.albumart_root,
        music_root,
        q.path.as_deref(),
        web_param,
        metadata,
        &c.albumart_providers,
        &c.exiftool_path,
        Some(&mpd_cfg),
    )
    .await
    {
        if let Ok(data) = std::fs::read(&file_path) {
            if let Some(resized) = resize_image_to_jpeg(&data, 250) {
                return image_response(resized, "image/jpeg");
            }
            return image_response(data, "image/jpeg");
        }
    }
    if let Some((data, ct)) = try_icon_fallback(&state, &q) {
        return image_response(data, ct);
    }
    let (data, ct) = default_album_art_bytes(&state);
    image_response(data, ct)
}

/// Returns the router, SocketIo handle, and app state (for push_state_queue_loop).
pub fn router(state: Arc<Config>) -> (Router, SocketIo, AppState) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    let router_state = Arc::new(RouterState {
        config: state.clone(),
        albumart_clear_tx: tx,
        last_browse: Arc::new(tokio::sync::RwLock::new(None)),
    });

    let (socket_layer, io) = SocketIo::builder()
        .with_state(router_state.clone())
        .max_payload(1_000_000)
        .build_layer();
    super::socketio::register_handlers(&io);

    let io_for_broadcast = io.clone();
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            let payload = serde_json::json!({
                "endpoint": "miscellanea/albumart",
                "method": "clearAlbumartCache",
                "data": ""
            });
            if io_for_broadcast.emit("callMethod", &payload).await.is_err() {
                break;
            }
        }
    });

    let v1_routes = Router::new()
        .route("/getState", get(v1::get_state))
        .route("/commands", get(v1::commands))
        .route("/getQueue", get(v1::get_queue))
        .route("/getInstalledPlugins", get(v1::get_installed_plugins))
        .route("/browse", get(v1::browse))
        .route("/ping", get(v1::ping))
        .route("/getSystemVersion", get(v1::get_system_version))
        .route("/getSystemInfo", get(v1::get_system_info))
        .route("/listplaylists", get(v1::list_playlists))
        .route("/search", get(v1::search))
        .route("/superSearch", get(v1::super_search))
        .route("/collectionstats", get(v1::collection_stats))
        .route("/getzones", get(v1::get_zones))
        .route("/replaceAndPlay", post(v1::replace_and_play))
        .with_state(router_state.clone());

    let app = Router::new()
        .route("/", get(health))
        .route("/api/health", get(health))
        .route("/status", get(status))
        .route("/albumart", get(album_art))
        .route("/albumartd", get(album_art_direct))
        .route("/tinyart/*path", get(album_art_tiny))
        .route("/albumart-upload", post(album_art_upload))
        .nest("/api/v1", v1_routes)
        .layer(socket_layer)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(router_state.clone());

    (app, io, router_state)
}

async fn health() -> &'static str {
    "ok"
}

/// GET /status - return system status string (Volumio uses process.env.VOLUMIO_SYSTEM_STATUS).
async fn status() -> impl IntoResponse {
    let s = std::env::var("VOLUMIO_SYSTEM_STATUS").unwrap_or_else(|_| "ready".to_string());
    (StatusCode::OK, s)
}

/// Sanitize a path component for albumart personal dir (no / \ NUL).
fn sanitize_albumart_component(s: &str) -> String {
    s.chars()
        .map(|c| if c == '/' || c == '\\' || c == '\0' { '_' } else { c })
        .collect()
}

/// POST /albumart-upload - multipart: artist, album (optional), file. Saves to albumart_root/personal/album/artist/album/ or personal/artist/artist/.
async fn album_art_upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut artist: Option<String> = None;
    let mut album: Option<String> = None;
    let mut file_data: Option<Vec<u8>> = None;
    let mut file_ext: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "artist" {
            if let Ok(bytes) = field.bytes().await {
                artist = Some(String::from_utf8_lossy(&bytes).trim().to_string());
            }
        } else if name == "album" {
            if let Ok(bytes) = field.bytes().await {
                let s = String::from_utf8_lossy(&bytes).trim().to_string();
                if !s.is_empty() {
                    album = Some(s);
                }
            }
        } else if name == "filePath" {
            // Volumio sends filePath for path-based upload; we only support artist/album here
            let _ = field.bytes().await;
        } else {
            // File field (any name with a filename)
            let file_name = field.file_name().map(|s| s.to_string());
            if let Some(name) = file_name {
                if let Ok(data) = field.bytes().await {
                    if data.len() > ALBUMART_UPLOAD_MAX_BYTES {
                        return (
                            StatusCode::PAYLOAD_TOO_LARGE,
                            Body::from("Album art upload exceeds 1MB"),
                        )
                            .into_response();
                    }
                    let ext = std::path::Path::new(&name)
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_lowercase());
                    if let Some(ext) = ext {
                        if matches!(ext.as_str(), "jpg" | "jpeg" | "png") {
                            file_data = Some(data.to_vec());
                            file_ext = Some(ext);
                        }
                    }
                }
            }
        }
    }

    let artist = match artist {
        Some(a) if !a.is_empty() => a,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Body::from("Missing or empty artist"),
            )
                .into_response();
        }
    };
    let (file_data, file_ext) = match (file_data, file_ext) {
        (Some(d), Some(e)) => (d, e),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Body::from("Missing or invalid image file (jpg, jpeg, png)"),
            )
                .into_response();
        }
    };

    let root = &state.config.albumart_root;
    let artist_esc = sanitize_albumart_component(&artist);
    let dir = if let Some(ref album_name) = album {
        let album_esc = sanitize_albumart_component(album_name);
        root.join("personal").join("album").join(&artist_esc).join(&album_esc)
    } else {
        root.join("personal").join("artist").join(&artist_esc)
    };

    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("albumart-upload mkdir {:?}: {}", dir, e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Body::from("Failed to create directory"),
        )
            .into_response();
    }

    // Replace existing cover (Volumio clears dir then writes one file)
    let filename = format!("cover.{}", file_ext);
    let path = dir.join(&filename);
    if let Err(e) = std::fs::write(&path, &file_data) {
        tracing::warn!("albumart-upload write {:?}: {}", path, e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Body::from("Failed to save image"),
        )
            .into_response();
    }

    state.send_clear_albumart_cache();

    let return_path = if let Some(ref a) = album {
        format!(
            "/albumart?web={}/{}/extralarge",
            urlencoding::encode(&artist),
            urlencoding::encode(a)
        )
    } else {
        format!("/albumart?web={}%2F%2Fextralarge", urlencoding::encode(&artist))
    };
    let json = serde_json::json!({ "path": return_path });
    (
        StatusCode::CREATED,
        [(header::CONTENT_TYPE, "application/json")],
        Body::from(json.to_string()),
    )
        .into_response()
}
