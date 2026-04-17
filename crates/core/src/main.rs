//! Volumio Evo: Rust backend + WASM plugins.

use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod log_tags;
mod evo_log_fmt;
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
    let mut config = config::load()?;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::try_new(config.log_level.env_filter_directive())
            .unwrap_or_else(|_| EnvFilter::new("info"))
    });

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(crate::evo_log_fmt::EvoPrefixedStderr)
                .event_format(tracing_subscriber::fmt::format::Format::default()),
        )
        .init();

    config::finalize_loaded_config(&mut config);

    tracing::info!(
        "{} config loaded: log_level={:?}, bind={}, mpd={}:{}",
        crate::log_tags::EVO_BOOT,
        config.log_level,
        config.bind,
        config.mpd_host,
        config.mpd_port
    );

    let config = Arc::new(config);
    let (app, io, state, push_wake_rx) = api::router(config);
    let idle_cfg = crate::mpd::MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    };
    tokio::spawn(crate::mpd::idle_push_state_wake_loop(
        idle_cfg,
        state.push_state_wake_tx.clone(),
    ));
    tokio::spawn(api::push_state_queue_loop(state.clone(), io, push_wake_rx));
    tokio::spawn(api::run_startup_volume_bootstrap(state.clone()));
    let listener = tokio::net::TcpListener::bind(&state.config.bind).await?;
    tracing::info!("{} listening on {}", crate::log_tags::EVO_BOOT, state.config.bind);

    axum::serve(listener, app).await?;
    Ok(())
}
