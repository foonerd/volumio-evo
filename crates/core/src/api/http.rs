//! Axum router and handlers.

use axum::{routing::get, Router};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use super::v1;

pub fn router(state: super::AppState) -> Router {
    let v1_routes = Router::new()
        .route("/getState", get(v1::get_state))
        .route("/commands", get(v1::commands))
        .route("/getQueue", get(v1::get_queue))
        .route("/getInstalledPlugins", get(v1::get_installed_plugins))
        .route("/browse", get(v1::browse))
        .with_state(state.clone());

    Router::new()
        .route("/", get(health))
        .route("/api/health", get(health))
        .nest("/api/v1", v1_routes)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
}

async fn health() -> &'static str {
    "ok"
}
