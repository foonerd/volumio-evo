//! Layout under `/var/lib/volumio-evo/`: `music/`, `albumart/`, `settings/` (Evo-controlled state).

use std::path::{Path, PathBuf};

/// All persisted Evo settings (ALSA output, MPD playback options, …).
pub const DEFAULT_SETTINGS_DIR: &str = "/var/lib/volumio-evo/settings";

/// Override base directory for settings. Default: [`DEFAULT_SETTINGS_DIR`].
pub fn settings_dir() -> PathBuf {
    std::env::var("VOLUMIO_EVO_SETTINGS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(DEFAULT_SETTINGS_DIR).to_path_buf())
}

/// Default ALSA output / I2S state when `VOLUMIO_EVO_ALSA_STATE` is unset (`settings/alsa/state.toml`).
pub fn default_alsa_state_path() -> PathBuf {
    settings_dir().join("alsa").join("state.toml")
}

/// Default `playback.toml` (MPD / Playback Options) when `VOLUMIO_EVO_PLAYBACK_STATE` is unset.
pub fn default_mpd_playback_path() -> PathBuf {
    settings_dir().join("mpd").join("playback.toml")
}

/// **Settings → System** persisted state when `VOLUMIO_EVO_SYSTEM_STATE` is unset.
pub fn default_system_state_path() -> PathBuf {
    settings_dir().join("system").join("state.toml")
}

/// Alarm clock + sleep timer when `VOLUMIO_EVO_ALARM_STATE` is unset (`settings/alarm/state.toml`).
pub fn default_alarm_clock_state_path() -> PathBuf {
    std::env::var("VOLUMIO_EVO_ALARM_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| settings_dir().join("alarm").join("state.toml"))
}

/// Wallpaper files + **`state.toml`** (unity with other **`settings/*/`** trees).
/// Env **`VOLUMIO_EVO_BACKGROUNDS_DIR`** overrides; default **`settings/backgrounds/`** under
/// [`settings_dir`].
pub fn backgrounds_data_dir() -> PathBuf {
    std::env::var("VOLUMIO_EVO_BACKGROUNDS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| settings_dir().join("backgrounds"))
}

/// **`settings/backgrounds/state.toml`**: current color vs image selection.
pub fn backgrounds_state_path() -> PathBuf {
    backgrounds_data_dir().join("state.toml")
}

/// **`settings/ui/active_layout`**: one-line mirror of **`[ui] active_layout`** (written when layout is saved;
/// fallback when **`/etc`** could not be updated).
pub fn ui_active_layout_overlay_path() -> PathBuf {
    settings_dir().join("ui").join("active_layout")
}

/// **`settings/ui/config.toml.pending`**: merged copy of **`/etc/volumio-evo/config.toml`** with **`[ui] active_layout`**
/// updated — installed via **`sudo install`** (**`volumio-evo-config-install-ui`** sudoers). Separate from Network’s
/// **`settings/network/config.toml.pending`**.
pub fn ui_config_toml_pending_path() -> PathBuf {
    settings_dir().join("ui").join("config.toml.pending")
}

/// Root of the **`volumio-evo`** tree (contains `layer/plymouth/`, `scripts/`). Used by boot-branding install.
/// Default when unset: **`/usr/share/volumio-evo/repo`** (packaged layout); development sets **`VOLUMIO_EVO_REPO_DIR`**.
pub fn evo_repo_dir() -> PathBuf {
    std::env::var("VOLUMIO_EVO_REPO_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new("/usr/share/volumio-evo/repo").to_path_buf())
}

/// Optional override for **`scripts/run-boot-branding.sh`** (must be executable).
/// When unset: **`$EVO_REPO_DIR/scripts/run-boot-branding.sh`** so **`sudo`** may use a stable path in sudoers.
pub fn boot_branding_run_script_path() -> PathBuf {
    std::env::var("VOLUMIO_EVO_BOOT_BRANDING_SCRIPT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| evo_repo_dir().join("scripts").join("run-boot-branding.sh"))
}

/// Root install script invoked by [`boot_branding_run_script_path`] (packages ship both under **`repo/scripts/`**).
pub fn boot_branding_install_script_path() -> PathBuf {
    evo_repo_dir().join("scripts").join("volumio-boot-branding.sh")
}

/// Live HLS output directory for LAN browser playback (`GET /hls/...`).
/// Override with **`VOLUMIO_EVO_HLS_DIR`** (full path to the directory that contains **`live/`**).
pub fn video_hls_root() -> PathBuf {
    std::env::var("VOLUMIO_EVO_HLS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new("/run/volumio-evo/hls").to_path_buf())
}

/// Segment + playlist directory (`…/hls/live/index.m3u8`).
pub fn video_hls_live_dir() -> PathBuf {
    video_hls_root().join("live")
}
