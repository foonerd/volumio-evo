//! Axum router, REST v1, Socket.IO layer, and album-art placeholder routes.

use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use socketioxide::SocketIo;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use super::v1;

/// Minimal 1x1 transparent PNG used when no default image exists under albumart_root.
const FALLBACK_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

const CACHE_MAX_AGE: &str = "public, max-age=2628000"; // 30d

/// Try to load default image from albumart_root (default.jpg then default.png); else return embedded placeholder.
fn default_album_art_bytes(state: &super::AppState) -> (Vec<u8>, &'static str) {
    let root = &state.albumart_root;
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

/// GET /albumart - serve default/placeholder from albumart_root or embedded fallback.
async fn album_art(State(state): State<super::AppState>) -> impl IntoResponse {
    let (data, ct) = default_album_art_bytes(&state);
    image_response(data, ct)
}

/// GET /albumartd - direct album art; same default/placeholder for now.
async fn album_art_direct(State(state): State<super::AppState>) -> impl IntoResponse {
    let (data, ct) = default_album_art_bytes(&state);
    image_response(data, ct)
}

/// GET /tinyart/* - tiny art variant; same default/placeholder for now.
async fn album_art_tiny(State(state): State<super::AppState>) -> impl IntoResponse {
    let (data, ct) = default_album_art_bytes(&state);
    image_response(data, ct)
}

/// Returns the router and the SocketIo handle so the caller can broadcast (e.g. pushState/pushQueue).
pub fn router(state: super::AppState) -> (Router, SocketIo) {
    let (socket_layer, io) = SocketIo::builder()
        .with_state(state.clone())
        .max_payload(1_000_000)
        .build_layer();
    super::socketio::register_handlers(&io);

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
        .with_state(state.clone());

    let app = Router::new()
        .route("/", get(health))
        .route("/api/health", get(health))
        .route("/albumart", get(album_art))
        .route("/albumartd", get(album_art_direct))
        .route("/tinyart/*path", get(album_art_tiny))
        .nest("/api/v1", v1_routes)
        .layer(socket_layer)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    (app, io)
}

async fn health() -> &'static str {
    "ok"
}
