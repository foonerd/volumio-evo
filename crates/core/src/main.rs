//! Volumio Evo: Rust backend + WASM plugins.

use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod api;
mod config;
mod mpd;
mod plugins;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::load()?;
    tracing::info!(
        "config loaded: bind={}, mpd={}:{}",
        config.bind,
        config.mpd_host,
        config.mpd_port
    );

    let state = Arc::new(config);
    let app = api::router(state.clone());
    let listener = tokio::net::TcpListener::bind(&state.bind).await?;
    tracing::info!("listening on {}", state.bind);

    axum::serve(listener, app).await?;
    Ok(())
}
