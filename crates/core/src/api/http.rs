//! Axum router, REST v1, Socket.IO layer, and album-art placeholder routes.

use axum::{
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
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

/// Plugin dirs for icon/sectionimage/sourceicon. Prefer **runtime** paths on device (`/usr/share/…`)
/// before the compile-time `CARGO_MANIFEST_DIR` tree (often absent on a deployed binary).
fn albumart_plugin_dirs(plugin_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let bundled =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/bundled-plugins");
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    for p in [
        std::path::PathBuf::from("/usr/share/volumio-evo/plugins"),
        plugin_dir.to_path_buf(),
        std::path::PathBuf::from("/data/plugins"),
        bundled,
    ] {
        if !out.iter().any(|e| e == &p) {
            out.push(p);
        }
    }
    out
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

/// Port from `bind` (e.g. `0.0.0.0:3000` → 3000).
fn http_listen_port(bind: &str) -> u16 {
    bind.rsplit(':')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000)
}

fn trim_host_port(h: &str) -> String {
    let h = h.trim();
    if h.starts_with('[') {
        if let Some(end) = h.find(']') {
            return h[1..end].to_string();
        }
    }
    if h.matches(':').count() == 1 {
        if let Some((host, port)) = h.rsplit_once(':') {
            if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
                return host.to_string();
            }
        }
    }
    h.to_string()
}

/// Non-loopback, non-APIPA IPv4s. Sorted: ethernet-style first, then Wi‑Fi, then others.
fn collect_ipv4_candidates() -> Vec<(String, std::net::Ipv4Addr)> {
    use if_addrs::{get_if_addrs, IfAddr};
    let mut v: Vec<(String, std::net::Ipv4Addr)> = Vec::new();
    let Ok(ifaces) = get_if_addrs() else {
        return v;
    };
    for iface in ifaces {
        if iface.name == "lo" {
            continue;
        }
        let IfAddr::V4(v4) = iface.addr else {
            continue;
        };
        let ip = v4.ip;
        if ip.is_loopback() {
            continue;
        }
        let o = ip.octets();
        if o[0] == 169 && o[1] == 254 {
            continue;
        }
        v.push((iface.name, ip));
    }
    v.sort_by(|a, b| {
        let score = |name: &str| -> u8 {
            if name.starts_with("eth") || name == "end0" {
                0
            } else if name.starts_with("wl") || name.starts_with("wlan") {
                1
            } else {
                2
            }
        };
        (score(&a.0), a.1).cmp(&(score(&b.0), b.1))
    });
    v
}

#[derive(Serialize)]
struct ApiHostResponse {
    host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    host2: Option<String>,
}

/// GET /api/host — matches volumio3-backend: JSON base URL(s) for Socket.IO + REST.
/// Uses live interface addresses (not a static file) so WiFi/LAN changes apply after reload.
/// If the HTTP `Host` header is a local IPv4, that address is preferred (same subnet as the client).
async fn api_host(State(state): State<AppState>, headers: HeaderMap) -> Json<ApiHostResponse> {
    use std::net::Ipv4Addr;
    let port = http_listen_port(&state.config.bind);
    let candidates = collect_ipv4_candidates();
    let host_hdr = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|s| trim_host_port(s));

    let mut preferred: Option<Ipv4Addr> = None;
    if let Some(ref h) = host_hdr {
        if let Ok(ip) = h.parse::<Ipv4Addr>() {
            if candidates.iter().any(|(_, cand)| *cand == ip) {
                preferred = Some(ip);
            }
        }
    }

    let primary = preferred
        .or_else(|| candidates.first().map(|(_, ip)| *ip))
        .unwrap_or(Ipv4Addr::new(127, 0, 0, 1));

    let host = format!("http://{}:{}", primary, port);
    let host2 = if candidates.len() > 1 {
        candidates
            .iter()
            .map(|(_, ip)| *ip)
            .find(|ip| *ip != primary)
            .map(|ip| format!("http://{}:{}", ip, port))
    } else {
        None
    };

    Json(ApiHostResponse { host, host2 })
}

/// Returns the router, SocketIo handle, app state, and wake receivers for [`super::push_state_queue_loop`].
pub fn router(
    state: Arc<Config>,
) -> (
    Router,
    SocketIo,
    AppState,
    tokio::sync::mpsc::UnboundedReceiver<()>,
    tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    let (push_wake_tx, push_wake_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    let (push_queue_wake_tx, push_queue_wake_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    let network_mounts = Arc::new(crate::network_mounts::NetworkMounts::new());
    let router_state = Arc::new(RouterState {
        config: state.clone(),
        network_mounts: network_mounts.clone(),
        alsa: Arc::new(tokio::sync::RwLock::new(crate::alsa::AlsaSettings::load())),
        playback: Arc::new(tokio::sync::RwLock::new(crate::playback_options::PlaybackOptions::load())),
        albumart_clear_tx: tx,
        last_browse: Arc::new(tokio::sync::RwLock::new(None)),
        volume_apply: tokio::sync::Mutex::new(()),
        playback_clock: Arc::new(tokio::sync::RwLock::new(crate::api::playback_clock::PlaybackClock::default())),
        push_state_wake_tx: push_wake_tx,
        push_queue_wake_tx,
        volume_ui_mute: Arc::new(tokio::sync::RwLock::new(crate::api::VolumeUiMuteState::default())),
    });

    let cfg_nas = state.clone();
    let nm_boot = network_mounts.clone();
    tokio::spawn(async move {
        nm_boot.mount_all_at_boot(cfg_nas).await;
    });

    let music_root = state.music_sources.music_root.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = crate::network_mounts::ensure_music_library_nas_symlink(&music_root) {
            tracing::warn!(
                "{} startup: music_root/NAS symlink: {}",
                crate::log_tags::EVO_UI,
                e
            );
        }
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
        .route("/getActiveUi", get(v1::get_active_ui))
        .route("/network/nm/status", get(v1::network_nm_status))
        .route("/network/nm/wifi-devices", get(v1::network_nm_wifi_devices))
        .route(
            "/network/nm/intent",
            get(v1::network_nm_intent_get).put(v1::network_nm_intent_put),
        )
        .route("/replaceAndPlay", post(v1::replace_and_play))
        .route("/pluginEndpoint", post(v1::plugin_endpoint))
        .with_state(router_state.clone());

    let app = Router::new()
        .route("/", get(health))
        .route("/api/health", get(health))
        .route("/api/host", get(api_host))
        .route("/status", get(status))
        .route("/albumart", get(album_art))
        .route("/albumartd", get(album_art_direct))
        .route("/tinyart/*path", get(album_art_tiny))
        .route("/albumart-upload", post(album_art_upload))
        .nest("/api/v1", v1_routes)
        .layer(socket_layer)
        .layer(TraceLayer::new_for_http())
        // `permissive()` uses `Allow-Origin: *`, which browsers reject for credentialed
        // cross-origin requests (Socket.IO polling with cookies / `withCredentials`).
        .layer(CorsLayer::very_permissive())
        .with_state(router_state.clone());

    (app, io, router_state, push_wake_rx, push_queue_wake_rx)
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
        tracing::warn!(
            "{} albumart-upload mkdir {:?}: {}",
            crate::log_tags::EVO_ALBUMART,
            dir,
            e
        );
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
        tracing::warn!(
            "{} albumart-upload write {:?}: {}",
            crate::log_tags::EVO_ALBUMART,
            path,
            e
        );
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
