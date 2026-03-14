//! Volumio Evo: Rust backend + WASM plugins.

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod api;
mod config;
mod plugins;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = config::load()?;
    tracing::info!("config loaded: bind={}", config.bind);

    let app = api::router();
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    tracing::info!("listening on {}", config.bind);

    axum::serve(listener, app).await?;
    Ok(())
}
