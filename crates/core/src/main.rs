//! Volumio Evo: Rust backend + WASM plugins.

use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod albumart;
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
    let (app, io) = api::router(state.clone());
    tokio::spawn(api::push_state_queue_loop(state.clone(), io));
    let listener = tokio::net::TcpListener::bind(&state.bind).await?;
    tracing::info!("listening on {}", state.bind);

    axum::serve(listener, app).await?;
    Ok(())
}
