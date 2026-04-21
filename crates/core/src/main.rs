//! Volumio Evo: Rust backend + WASM plugins.

use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod version;
mod system_updates;
mod system_settings;
mod log_tags;
mod evo_log_fmt;
mod network_mounts;
mod network_share_discovery;
mod alsa;
mod alsa_cards;
mod paths;
mod alarm_clock;
mod playback_options;
mod appearance;
mod backgrounds;
mod ui_bootstrap;
mod albumart;
mod i2s;
mod api;
mod artist_normalize;
mod config;
mod cue_normalize;
mod kiosk;
mod metavolumio;
mod mpd;
mod samba_settings;
mod samba_conf;
mod samba_apply;
mod network_config;
mod network_status_ui;
mod nm_network;
mod wifi_phy;
mod rfkill_mgmt;
mod rtc_wake;
mod playlist_library;
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
    let (app, io, state, push_wake_rx, push_queue_wake_rx) = api::router(config);
    let idle_cfg = crate::mpd::MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    };
    tokio::spawn(crate::mpd::idle_push_state_wake_loop(
        idle_cfg,
        state.push_state_wake_tx.clone(),
    ));
    tokio::spawn(api::push_state_queue_loop(
        state.clone(),
        io,
        push_wake_rx,
        push_queue_wake_rx,
    ));
    tokio::spawn(api::run_startup_volume_bootstrap(state.clone()));
    tokio::spawn(api::run_startup_network_intent_apply(state.clone()));
    tokio::spawn(api::run_startup_samba_apply(state.clone()));
    tokio::spawn(api::run_startup_system_locale_apply(state.clone()));
    tokio::spawn(api::run_startup_alarm_schedule(state.clone()));
    // WPE kiosk: reassert persisted state.toml -> systemctl on boot.
    tokio::spawn(api::run_startup_kiosk_apply(state.clone()));
    if !std::env::var("VOLUMIO_EVO_SKIP_ALSA_HOTPLUG")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        tokio::spawn(api::run_alsa_sound_hotplug_loop(state.clone()));
    }
    if std::env::var("VOLUMIO_EVO_PROBE_RTC_WAKE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        crate::rtc_wake::log_startup_probe();
    }
    let listener = tokio::net::TcpListener::bind(&state.config.bind).await?;
    tracing::info!("{} listening on {}", crate::log_tags::EVO_BOOT, state.config.bind);

    axum::serve(listener, app).await?;
    Ok(())
}
