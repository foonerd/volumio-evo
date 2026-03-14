//! Axum router and handlers.

use axum::{
    routing::get,
    Router,
};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

pub fn router() -> Router {
    Router::new()
        .route("/", get(health))
        .route("/api/health", get(health))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
}

async fn health() -> &'static str {
    "ok"
}
