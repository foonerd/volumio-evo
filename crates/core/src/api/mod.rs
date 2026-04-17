//! HTTP and Socket.IO API.

mod http;
mod network_ui;
mod playback_clock;
mod pushstate_log;
mod socketio;
mod sources_ui;
mod v1;

use crate::alsa::AlsaSettings;
use crate::config::Config;
use crate::mpd::{self, MpdConfig, VolumioState};
use crate::network_mounts::NetworkMounts;
use crate::playback_options::PlaybackOptions;
use std::sync::Arc;
use std::time::Duration;

/// Shared state: config + channel to trigger album-art cache-clear broadcast + last browse for getLastPushedBrowseLibrary.
/// Node `volumecontrol.js`: while muted, UI still shows **premute** level (`Volume.vol`), not 0.
#[derive(Debug, Clone)]
pub struct VolumeUiMuteState {
    pub muted: bool,
    pub premute_percent: u8,
}

impl Default for VolumeUiMuteState {
    fn default() -> Self {
        Self {
            muted: false,
            premute_percent: 80,
        }
    }
}

pub struct RouterState {
    pub config: Arc<Config>,
    /// NAS/SMB/NFS mounts (`settings/mounts/shares.toml`, `/mnt/NAS/...`).
    pub network_mounts: Arc<NetworkMounts>,
    /// Persisted ALSA output selection (Playback Options); full pipeline apply is future work.
    pub alsa: Arc<tokio::sync::RwLock<AlsaSettings>>,
    /// MPD / playback options (stock UI sections: buffer, DSD, volume, resampling).
    pub playback: Arc<tokio::sync::RwLock<PlaybackOptions>>,
    albumart_clear_tx: tokio::sync::mpsc::UnboundedSender<()>,
    /// Last pushBrowseLibrary payload (for getLastPushedBrowseLibrary).
    pub last_browse: Arc<tokio::sync::RwLock<Option<serde_json::Value>>>,
    /// Serializes ALSA + MPD volume so startup (multi-control Hardware path) cannot race UI/REST `setvol`.
    pub volume_apply: tokio::sync::Mutex<()>,
    /// RAM clock: MPD `seek` + wall time between sparse `pushState` (Node `currentSeek` pattern).
    pub playback_clock: Arc<tokio::sync::RwLock<playback_clock::PlaybackClock>>,
    /// Wakes the broadcast loop for an immediate `pushState` (MPD idle + playback commands).
    /// Unbounded so wakeups are never coalesced away (unlike [`tokio::sync::Notify`]).
    pub push_state_wake_tx: tokio::sync::mpsc::UnboundedSender<()>,
    /// Wakes the broadcast loop for an immediate `pushQueue` to all Socket.IO clients (queue edits).
    pub push_queue_wake_tx: tokio::sync::mpsc::UnboundedSender<()>,
    /// Landing-page mute: logical level preserved for `pushState.volume` while output is silenced.
    pub volume_ui_mute: Arc<tokio::sync::RwLock<VolumeUiMuteState>>,
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

    /// Reseed RAM clock from MPD (broadcast resync, Socket/REST `getState`).
    pub async fn store_mpd_snapshot(&self, s: &VolumioState) {
        self.playback_clock.write().await.sync_from_mpd(s);
    }

    #[inline]
    pub fn notify_push_state(&self) {
        let _ = self.push_state_wake_tx.send(());
    }

    /// After queue mutations, notify all clients with an immediate `pushQueue` (Node `volumioPushQueue`).
    #[inline]
    pub fn notify_push_queue(&self) {
        let _ = self.push_queue_wake_tx.send(());
    }
}

/// Raw 0–100 from MPD + ALSA (ignores [`VolumeUiMuteState`] — use before applying a new mute).
pub async fn resolve_live_volume_percent(state: &AppState) -> u8 {
    let config = MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    };
    let master = read_master_volume_percent(state).await;
    match mpd::get_state_connected(&config, &state.config.music_sources.music_root, master).await {
        Ok(s) => s.volume.unwrap_or(0),
        Err(_) => master.unwrap_or(0),
    }
}

/// Stock UI `pushState`: `mute` + logical `volume` when Evo has silenced output (`volumecontrol.js`).
pub async fn apply_volume_mute_overlay(state: &AppState, s: &mut VolumioState) {
    let pb = state.playback.read().await;
    s.disable_volume_control = pb.mixer_type == "None";
    drop(pb);
    let vm = state.volume_ui_mute.read().await;
    if vm.muted {
        s.mute = true;
        s.volume = Some(vm.premute_percent);
    } else {
        s.mute = false;
    }
}

pub type AppState = Arc<RouterState>;

/// **Master fader** level 0–100 from ALSA (same control as [`crate::alsa::set_system_volume_percent`]),
/// or `None` when mixer type is **None**, **Software** without a **`SoftMaster`** control on the card,
/// or `amixer get` failed — [`mpd::get_state_connected`] then uses MPD.
pub async fn read_master_volume_percent(state: &AppState) -> Option<u8> {
    let alsa = state.alsa.read().await.clone();
    let pb = state.playback.read().await.clone();
    if pb.mixer_type == "None" {
        return None;
    }
    // Software maps to ALSA **`SoftMaster`** only when present; else callers use MPD (see doc).
    if pb.mixer_type == "Software" && !crate::alsa::alsa_softmaster_control_present(&alsa) {
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
            tracing::debug!("{} read master volume task: {}", crate::log_tags::EVO_VOLUME, e);
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
        tracing::info!(
            "{} skipping default startup volume (VOLUMIO_EVO_SKIP_STARTUP_VOLUME=1)",
            crate::log_tags::EVO_VOLUME
        );
        return;
    }

    tokio::time::sleep(Duration::from_secs(3)).await;

    let alsa = state.alsa.read().await.clone();
    let pb = state.playback.read().await.clone();
    let Some(vol) = pb.startup_volume_percent_for_mpd() else {
        tracing::debug!(
            volumestart = %pb.volumestart,
            mixer_type = %pb.mixer_type,
            "{} startup volume: not applicable or disabled",
            crate::log_tags::EVO_VOLUME
        );
        return;
    };

    let log_curve = pb
        .volumecurvemode
        .trim()
        .eq_ignore_ascii_case("logarithmic");

    let _vol_apply = state.volume_apply.lock().await;

    let softmaster = crate::alsa::alsa_softmaster_control_present(&alsa);

    match pb.mixer_type.as_str() {
        "Software" => {
            if !softmaster {
                let alsa_open = alsa.clone();
                let log_open = log_curve;
                match tokio::task::spawn_blocking(move || {
                    crate::alsa::open_alsa_playback_line_unity_before_mpd_volume(&alsa_open, log_open)
                })
                .await
                {
                    Ok(()) => tracing::info!(
                        "{} startup volume (Software): ALSA playback line to unity, then MPD setvol",
                        crate::log_tags::EVO_VOLUME
                    ),
                    Err(e) => tracing::debug!(
                        "{} startup volume (Software): open playback line: {}",
                        crate::log_tags::EVO_VOLUME,
                        e
                    ),
                }
            } else {
                let alsa_c = alsa.clone();
                let mn = pb.mixer.clone();
                let v = vol;
                let lc = log_curve;
                match tokio::task::spawn_blocking(move || {
                    crate::alsa::set_system_volume_percent(&alsa_c, "Software", &mn, lc, v)
                })
                .await
                {
                    Ok(Ok(())) => tracing::info!(
                        vol = v,
                        "{} startup volume (Software + SoftMaster): ALSA then MPD setvol",
                        crate::log_tags::EVO_VOLUME
                    ),
                    Ok(Err(e)) => tracing::warn!(
                        "{} startup volume (Software): ALSA SoftMaster: {}",
                        crate::log_tags::EVO_VOLUME,
                        e
                    ),
                    Err(e) => tracing::warn!(
                        "{} startup volume (Software): ALSA task: {}",
                        crate::log_tags::EVO_VOLUME,
                        e
                    ),
                }
            }
        }
        "Hardware" => {
            let alsa_c = alsa.clone();
            let mn = pb.mixer.clone();
            let v = vol;
            let lc = log_curve;
            match tokio::task::spawn_blocking(move || {
                crate::alsa::apply_startup_volume_hardware_mixer(&alsa_c, &mn, lc, v)
            })
            .await
            {
                Ok(Ok(())) => tracing::info!(
                    vol = v,
                    "{} startup volume (Hardware): ALSA apply (sibling faders unity, primary to volumestart)",
                    crate::log_tags::EVO_VOLUME
                ),
                Ok(Err(e)) => tracing::warn!(
                    "{} startup volume (Hardware): ALSA: {}",
                    crate::log_tags::EVO_VOLUME,
                    e
                ),
                Err(e) => tracing::warn!(
                    "{} startup volume (Hardware): ALSA task: {}",
                    crate::log_tags::EVO_VOLUME,
                    e
                ),
            }
        }
        "None" => {
            tracing::debug!(
                "{} startup volume: internal inconsistency (mixer None with applicable volumestart)",
                crate::log_tags::EVO_VOLUME
            );
            return;
        }
        other => {
            tracing::warn!(
                mixer_type = %other,
                "{} startup volume: unknown mixer_type; MPD setvol only",
                crate::log_tags::EVO_VOLUME
            );
        }
    }

    if pb.mixer_type == "Hardware" && pb.mpd_shares_alsa_hardware_mixer(&alsa) {
        tracing::info!(
            vol,
            "{} applied default startup volume (ALSA only; MPD shares hardware mixer — skip setvol)",
            crate::log_tags::EVO_VOLUME
        );
        return;
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
                    "{} applied default startup volume (MPD setvol; Playback options volumestart)",
                    crate::log_tags::EVO_VOLUME
                );
                return;
            }
            Err(e) => {
                tracing::debug!(
                    attempt,
                    err = %e,
                    "{} startup volume: waiting for MPD",
                    crate::log_tags::EVO_VOLUME
                );
                tokio::time::sleep(Duration::from_millis(700)).await;
            }
        }
    }

    tracing::warn!(
        vol,
        "{} could not apply default startup volume: MPD not accepting setvol after retries",
        crate::log_tags::EVO_VOLUME
    );
}

/// After boot, apply persisted [`crate::network_config::NetworkIntent`] so NM matches **intent** without
/// opening the UI. Set `VOLUMIO_EVO_SKIP_NETWORK_INTENT_APPLY=1` to disable (debug / installers).
pub async fn run_startup_network_intent_apply(state: AppState) {
    if std::env::var("VOLUMIO_EVO_SKIP_NETWORK_INTENT_APPLY")
        .ok()
        .as_deref()
        == Some("1")
    {
        tracing::info!(
            "{} skipping startup network intent apply (VOLUMIO_EVO_SKIP_NETWORK_INTENT_APPLY=1)",
            crate::log_tags::EVO_NET
        );
        return;
    }

    // Brief delay so NetworkManager is up after systemd (same idea as startup volume).
    tokio::time::sleep(Duration::from_secs(2)).await;

    let intent = crate::network_config::NetworkIntent::load();
    if matches!(
        intent.wifi.role,
        crate::network_config::WifiRole::Sta
    ) {
        crate::nm_network::ensure_wifi_client_hw_ready().await;
    }

    let cfg = state.config.as_ref();
    let report = crate::nm_network::apply_network_intent_exclusive(&intent, cfg).await;
    if report.ok {
        tracing::info!(
            "{} applied persisted network intent at startup ({} step(s))",
            crate::log_tags::EVO_NET,
            report.steps.len()
        );
    } else {
        tracing::warn!(
            "{} startup network intent apply failed or partial: {}",
            crate::log_tags::EVO_NET,
            report
                .steps
                .last()
                .map(|s| s.as_str())
                .unwrap_or("unknown error")
        );
    }
}
