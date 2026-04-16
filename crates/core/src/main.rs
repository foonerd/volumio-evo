//! Volumio Evo: Rust backend + WASM plugins.

use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod alsa;
mod alsa_cards;
mod paths;
mod playback_options;
mod albumart;
mod i2s;
mod api;
mod artist_normalize;
mod config;
mod metavolumio;
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

    let config = Arc::new(config);
    let (app, io, state) = api::router(config);
    tokio::spawn(api::push_state_queue_loop(state.clone(), io));
    tokio::spawn(api::run_startup_volume_bootstrap(state.clone()));
    let listener = tokio::net::TcpListener::bind(&state.config.bind).await?;
    tracing::info!("listening on {}", state.config.bind);

    axum::serve(listener, app).await?;
    Ok(())
}
