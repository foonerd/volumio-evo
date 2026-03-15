//! Axum router, REST v1, and Socket.IO layer.

use axum::{routing::{get, post}, Router};
use socketioxide::SocketIo;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use super::v1;

pub fn router(state: super::AppState) -> Router {
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

    Router::new()
        .route("/", get(health))
        .route("/api/health", get(health))
        .nest("/api/v1", v1_routes)
        .layer(socket_layer)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
}

async fn health() -> &'static str {
    "ok"
}
