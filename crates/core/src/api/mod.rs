//! HTTP and Socket.IO API.

mod http;
mod socketio;
mod v1;

use crate::alsa::AlsaSettings;
use crate::config::Config;
use crate::mpd::{self, MpdConfig};
use crate::playback_options::PlaybackOptions;
use std::sync::Arc;
use std::time::Duration;

/// Shared state: config + channel to trigger album-art cache-clear broadcast + last browse for getLastPushedBrowseLibrary.
pub struct RouterState {
    pub config: Arc<Config>,
    /// Persisted ALSA output selection (Playback Options); full pipeline apply is future work.
    pub alsa: Arc<tokio::sync::RwLock<AlsaSettings>>,
    /// MPD / playback options (stock UI sections: buffer, DSD, volume, resampling).
    pub playback: Arc<tokio::sync::RwLock<PlaybackOptions>>,
    albumart_clear_tx: tokio::sync::mpsc::UnboundedSender<()>,
    /// Last pushBrowseLibrary payload (for getLastPushedBrowseLibrary).
    pub last_browse: Arc<tokio::sync::RwLock<Option<serde_json::Value>>>,
}

impl RouterState {
    /// Trigger broadcast of clearAlbumartCache to all Socket.IO clients (no-op if tx closed).
    pub fn send_clear_albumart_cache(&self) {
        let _ = self.albumart_clear_tx.send(());
    }

    /// Store last browse response for getLastPushedBrowseLibrary.
    pub async fn set_last_browse(&self, value: serde_json::Value) {
        *self.last_browse.write().await = Some(value);
    }

    /// Read last browse response (clone).
    pub async fn get_last_browse(&self) -> Option<serde_json::Value> {
        self.last_browse.read().await.clone()
    }
}

pub type AppState = Arc<RouterState>;

/// **Master fader** level 0–100 from ALSA (same control as [`crate::alsa::set_system_volume_percent`]),
/// or `None` when mixer type is **None** or `amixer get` failed — [`mpd::get_state_connected`] then uses MPD.
pub async fn read_master_volume_percent(state: &AppState) -> Option<u8> {
    let alsa = state.alsa.read().await.clone();
    let pb = state.playback.read().await.clone();
    if pb.mixer_type == "None" {
        return None;
    }
    let log = pb
        .volumecurvemode
        .trim()
        .eq_ignore_ascii_case("logarithmic");
    let alsa_c = alsa.clone();
    let mt = pb.mixer_type.clone();
    let mn = pb.mixer.clone();
    match tokio::task::spawn_blocking(move || {
        crate::alsa::get_system_volume_percent(&alsa_c, &mt, &mn, log)
    })
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!("read master volume task: {}", e);
            None
        }
    }
}

pub use http::router;
pub use socketio::push_state_queue_loop;

/// After plugins would be up on Node, apply **Default startup volume** via MPD (`volumecontrol.setStartupVolume`).
/// Set `VOLUMIO_EVO_SKIP_STARTUP_VOLUME=1` to disable. Retries while MPD is still starting.
pub async fn run_startup_volume_bootstrap(state: AppState) {
    if std::env::var("VOLUMIO_EVO_SKIP_STARTUP_VOLUME")
        .ok()
        .as_deref()
        == Some("1")
    {
        tracing::info!("skipping default startup volume (VOLUMIO_EVO_SKIP_STARTUP_VOLUME=1)");
        return;
    }

    tokio::time::sleep(Duration::from_secs(3)).await;

    let alsa = state.alsa.read().await.clone();
    let pb = state.playback.read().await.clone();
    let Some(vol) = pb.startup_volume_percent_for_mpd() else {
        tracing::debug!(
            volumestart = %pb.volumestart,
            mixer_type = %pb.mixer_type,
            "startup volume: not applicable or disabled"
        );
        return;
    };

    let log_curve = pb
        .volumecurvemode
        .trim()
        .eq_ignore_ascii_case("logarithmic");
    let alsa_c = alsa.clone();
    let mt = pb.mixer_type.clone();
    let mn = pb.mixer.clone();
    match tokio::task::spawn_blocking(move || {
        crate::alsa::set_system_volume_percent(&alsa_c, &mt, &mn, log_curve, vol)
    })
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!("startup volume ALSA (amixer): {}", e),
        Err(e) => tracing::warn!("startup volume ALSA task: {}", e),
    }

    let mpd = MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    };

    for attempt in 0u32..30 {
        match mpd::run_command_connected(&mpd, "volume", Some(vol), None, None, None).await {
            Ok(()) => {
                tracing::info!(
                    vol,
                    attempt,
                    "applied default startup volume (MPD setvol; Playback options volumestart)"
                );
                return;
            }
            Err(e) => {
                tracing::debug!(attempt, err = %e, "startup volume: waiting for MPD");
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
        }
    }

    tracing::warn!(
        vol,
        "could not apply default startup volume: MPD not accepting setvol after retries"
    );
}
