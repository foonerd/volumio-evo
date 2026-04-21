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

/// SMB server persisted state (**`settings/samba/state.toml`**) when `VOLUMIO_EVO_SAMBA_STATE` is unset.
/// Policy: repository `docs/SAMBA.md`.
pub fn default_samba_state_path() -> PathBuf {
    std::env::var("VOLUMIO_EVO_SAMBA_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| settings_dir().join("samba").join("state.toml"))
}

/// Generated **`smb.conf`** written by Evo before **`sudo install`** → **`/etc/samba/smb.conf`** (non-root path in sudoers).
pub fn default_samba_generated_smb_conf_path() -> PathBuf {
    settings_dir().join("samba").join("smb.conf.generated")
}

/// Root of the **`volumio-evo`** tree (contains `layer/plymouth/`, `layer/install/`). Used by boot-branding install.
/// Default when unset: **`/usr/share/volumio-evo/repo`** (packaged layout); development sets **`VOLUMIO_EVO_REPO_DIR`**.
pub fn evo_repo_dir() -> PathBuf {
    std::env::var("VOLUMIO_EVO_REPO_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new("/usr/share/volumio-evo/repo").to_path_buf())
}

/// Optional override for **`layer/install/run-boot-branding.sh`** (must be executable).
/// When unset: **`$EVO_REPO_DIR/layer/install/run-boot-branding.sh`** so **`sudo`** may use a stable path in sudoers.
pub fn boot_branding_run_script_path() -> PathBuf {
    std::env::var("VOLUMIO_EVO_BOOT_BRANDING_SCRIPT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| evo_repo_dir().join("layer").join("install").join("run-boot-branding.sh"))
}

/// Repo script run as **`sudo -n … --apply-ui-only`** so nginx **`root`** tracks **`[ui] active_layout`**.
/// **`VOLUMIO_EVO_BOOTSTRAP_SCRIPT`** overrides **`$EVO_REPO_DIR/scripts/bootstrap-volumio-evo-player.sh`** and must match **`/etc/sudoers.d/volumio-evo-ui-bootstrap`** on non-root installs.
pub fn bootstrap_player_script_path() -> PathBuf {
    if let Ok(p) = std::env::var("VOLUMIO_EVO_BOOTSTRAP_SCRIPT") {
        let pb = PathBuf::from(p);
        if !pb.as_os_str().is_empty() {
            return pb;
        }
    }
    evo_repo_dir().join("scripts").join("bootstrap-volumio-evo-player.sh")
}

/// Root install script invoked by [`boot_branding_run_script_path`] (ships under **`repo/layer/install/`**).
pub fn boot_branding_install_script_path() -> PathBuf {
    evo_repo_dir()
        .join("layer")
        .join("install")
        .join("volumio-boot-branding.sh")
}

/// Wrapper invoked as **`sudo -n …`** from **`crate::api::kiosk_install`** so the service user can
/// refresh the Wayland kiosk layer (same entry point as bootstrap **`--with-kiosk=wpe`**).
/// Override with **`VOLUMIO_EVO_KIOSK_INSTALL_SCRIPT`** for development.
pub fn kiosk_wpe_install_run_script_path() -> PathBuf {
    std::env::var("VOLUMIO_EVO_KIOSK_INSTALL_SCRIPT")
        .ok()
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            evo_repo_dir()
                .join("layer")
                .join("install")
                .join("run-kiosk-wpe-install.sh")
        })
}

// --- SMB server (user-defined share paths): moderation -------------------------------------------

/// Absolute path prefixes allowed as targets for **user-defined** SMB shares (string prefix policy; see `docs/SAMBA.md`).
pub const SMB_SHARE_ALLOWED_ROOTS: &[&str] = &[
    "/var/lib/volumio-evo",
    "/mnt/NAS",
    "/mnt/USB",
];

/// Prefixes that are **never** exported, even if nested under [`SMB_SHARE_ALLOWED_ROOTS`].
pub const SMB_SHARE_DENIED_PREFIXES: &[&str] = &["/var/lib/volumio-evo/settings"];

// --- WPE kiosk overlays --------------------------------------------------------------------------
// Single-line value files consumed by /usr/local/bin/volumio-evo-kiosk-launch
// and crates/core/src/kiosk.rs. Overlay values WIN over /etc/volumio-evo/kiosk.toml.
// See layer/kiosk-wpe/README.md for the key reference and precedence rules.

/// Root of WPE kiosk overlay files (per-key one-line files).
pub fn kiosk_state_dir() -> PathBuf {
    settings_dir().join("kiosk")
}

/// `settings/kiosk/primary_display`: `auto` | `hdmi` | `dsi` | `wayland-default`.
pub fn kiosk_primary_display_overlay_path() -> PathBuf {
    kiosk_state_dir().join("primary_display")
}

/// `settings/kiosk/rotation`: degrees `0` | `90` | `180` | `270`.
pub fn kiosk_rotation_overlay_path() -> PathBuf {
    kiosk_state_dir().join("rotation")
}

/// `settings/kiosk/osk`: `squeekboard` | `wvkbd` | `none`.
pub fn kiosk_osk_overlay_path() -> PathBuf {
    kiosk_state_dir().join("osk")
}

/// `settings/kiosk/cursor`: `auto` | `hide` | `show`.
pub fn kiosk_cursor_overlay_path() -> PathBuf {
    kiosk_state_dir().join("cursor")
}

/// `settings/kiosk/auto_rotate`: `true` | `false`.
pub fn kiosk_auto_rotate_overlay_path() -> PathBuf {
    kiosk_state_dir().join("auto_rotate")
}

/// `settings/kiosk/zoom`: WebKit page zoom level, float as string (e.g. `"1.2"`).
/// Consumed by `/usr/local/bin/volumio-evo-kiosk-browser` via `KIOSK_ZOOM` env
/// which the launcher exports from this overlay. Matches the Node kiosk
/// `display_zoom` select (60 % - 170 %, default 1.2) so Bootstrap breakpoint
/// behaviour on the kiosk UI is identical across Node and Evo.
pub fn kiosk_zoom_overlay_path() -> PathBuf {
    kiosk_state_dir().join("zoom")
}

/// `settings/kiosk/scale`: Wayland output scale, either `auto` or a float as
/// string (e.g. `"1.5"`). Applied by the session script via `wlr-randr
/// --output <name> --scale <f>` once labwc has opened the Wayland socket.
pub fn kiosk_scale_overlay_path() -> PathBuf {
    kiosk_state_dir().join("scale")
}

/// `settings/kiosk/osk_layout`: resolved XKB layout for the on-screen
/// keyboard (e.g. `us`, `gb`, `de`). The backend normalises the UI value
/// (`auto` or a specific code) and writes the resolved XKB code here; the
/// session script reads it and runs `gsettings set
/// org.gnome.desktop.input-sources sources "[('xkb', '<code>')]"` before
/// launching squeekboard so the OSK matches the system locale.
pub fn kiosk_osk_layout_overlay_path() -> PathBuf {
    kiosk_state_dir().join("osk_layout")
}
