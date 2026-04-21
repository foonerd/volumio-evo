//! **Wayland kiosk control plane** (layer uses labwc + GTK/WebKit shell, not WPE).
//!
//! This module is the minimum Rust wiring that turns the existing
//! Settings -> System kiosk toggle (see `crate::system_settings::SystemSettings`
//! fields `kiosk_enabled`, `primary_display`, `kiosk_rotation`, `kiosk_auto_rotate`,
//! `kiosk_osk`, `kiosk_cursor`) into actual side effects on the layer component at
//! `layer/kiosk-wpe/`.
//!
//! Responsibilities:
//!  - Write per-key overlay files under `settings/kiosk/` so the shell launcher
//!    picks up user choices without restarting the backend.
//!  - Start / stop `volumio-evo-kiosk.service` and
//!    `volumio-evo-kiosk-autorotate.service` via `sudo -n systemctl ...`
//!    (see `/etc/sudoers.d/volumio-evo-kiosk-control` installed by bootstrap).
//!  - Probe DRM presence so we refuse to start on headless hosts.
//!  - Expose a `kiosk_status_json(...)` snapshot consumed by
//!    `GET /api/v1/kiosk/status`.
//!
//! Environment (published by `scripts/bootstrap-volumio-evo-player.sh` in the
//! `10-runtime-user.conf` systemd drop-in):
//!   - `VOLUMIO_EVO_KIOSK_SYSTEMCTL` - path to `systemctl` that matches the
//!     kiosk-control sudoers drop-in. Falls back to `/usr/bin/systemctl`.
//!
//! Log tag: `crate::log_tags::EVO_KIOSK` on every line. See
//! `docs/OBSERVABILITY.md` for the tag convention.

use std::path::PathBuf;

use serde_json::json;

use crate::log_tags::EVO_KIOSK;
use crate::paths;

/// Unit names used by the layer component.
pub const KIOSK_UNIT: &str = "volumio-evo-kiosk.service";
pub const KIOSK_AUTOROTATE_UNIT: &str = "volumio-evo-kiosk-autorotate.service";

#[cfg(unix)]
fn effective_uid_is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
fn effective_uid_is_root() -> bool {
    false
}

/// Resolved path to `systemctl`. Matches the sudoers drop-in
/// `/etc/sudoers.d/volumio-evo-kiosk-control` when running as a non-root
/// service user. Override with `VOLUMIO_EVO_KIOSK_SYSTEMCTL`.
pub fn kiosk_systemctl_bin() -> String {
    std::env::var("VOLUMIO_EVO_KIOSK_SYSTEMCTL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("VOLUMIO_EVO_SYSTEMCTL")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "/usr/bin/systemctl".to_string())
}

/// Service user name for logging / UI hints. The resolution in bootstrap's
/// `configure_evo_runtime_user` publishes `VOLUMIO_EVO_RUNTIME_USER`.
pub fn service_user_hint() -> Option<String> {
    std::env::var("VOLUMIO_EVO_RUNTIME_USER")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// True if at least one DRM card exists. Cheap, synchronous; used on every
/// save to refuse enabling the kiosk on a headless host.
pub fn drm_device_present() -> bool {
    for i in 0..8 {
        let p = format!("/dev/dri/card{i}");
        if std::path::Path::new(&p).exists() {
            return true;
        }
    }
    false
}

/// Atomic overlay write (tmp + rename). Returns `Ok(true)` when the value
/// changed on disk, `Ok(false)` when it already matched.
fn write_overlay(path: PathBuf, value: &str) -> std::io::Result<bool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let trimmed = value.trim();
    let current = std::fs::read_to_string(&path).ok();
    if current.as_deref().map(|s| s.trim()) == Some(trimmed) {
        return Ok(false);
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, format!("{trimmed}\n"))?;
    std::fs::rename(&tmp, &path)?;
    Ok(true)
}

/// Write `primary_display` overlay (`auto` | `hdmi` | `dsi` | `wayland-default`).
pub fn apply_primary_display(value: &str) -> std::io::Result<bool> {
    let v = normalize_primary_display(value);
    let changed = write_overlay(paths::kiosk_primary_display_overlay_path(), &v)?;
    if changed {
        tracing::info!("{} primary_display overlay -> {}", EVO_KIOSK, v);
    }
    Ok(changed)
}

/// Normalize to one of the UI-exposed values; fall back to "auto".
pub fn normalize_primary_display(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => "auto".to_string(),
        "hdmi" => "hdmi".to_string(),
        "dsi" => "dsi".to_string(),
        "wayland-default" | "wayland_default" => "wayland-default".to_string(),
        _ => "auto".to_string(),
    }
}

/// Normalize rotation to 0 | 90 | 180 | 270.
pub fn normalize_rotation(deg: u16) -> u16 {
    match deg {
        0 | 90 | 180 | 270 => deg,
        _ => 0,
    }
}

/// Write rotation overlay as the plain degree integer.
pub fn apply_rotation(deg: u16) -> std::io::Result<bool> {
    let d = normalize_rotation(deg);
    let changed = write_overlay(paths::kiosk_rotation_overlay_path(), &d.to_string())?;
    if changed {
        tracing::info!("{} rotation overlay -> {}", EVO_KIOSK, d);
    }
    Ok(changed)
}

/// Normalize to `squeekboard` | `wvkbd` | `none`.
pub fn normalize_osk(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "wvkbd" => "wvkbd".to_string(),
        "none" | "off" | "disabled" => "none".to_string(),
        _ => "squeekboard".to_string(),
    }
}

pub fn apply_osk(value: &str) -> std::io::Result<bool> {
    let v = normalize_osk(value);
    let changed = write_overlay(paths::kiosk_osk_overlay_path(), &v)?;
    if changed {
        tracing::info!("{} osk overlay -> {}", EVO_KIOSK, v);
    }
    Ok(changed)
}

/// Normalize to `auto` | `hide` | `show`.
pub fn normalize_cursor(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "hide" => "hide".to_string(),
        "show" => "show".to_string(),
        _ => "auto".to_string(),
    }
}

pub fn apply_cursor(value: &str) -> std::io::Result<bool> {
    let v = normalize_cursor(value);
    let changed = write_overlay(paths::kiosk_cursor_overlay_path(), &v)?;
    if changed {
        tracing::info!("{} cursor overlay -> {}", EVO_KIOSK, v);
    }
    Ok(changed)
}

/// Write auto_rotate overlay. Does NOT flip the autorotate unit - that is
/// the job of `apply_kiosk_settings` which coordinates the unit state.
pub fn apply_auto_rotate_overlay(enabled: bool) -> std::io::Result<bool> {
    let v = if enabled { "true" } else { "false" };
    let changed = write_overlay(paths::kiosk_auto_rotate_overlay_path(), v)?;
    if changed {
        tracing::info!("{} auto_rotate overlay -> {}", EVO_KIOSK, v);
    }
    Ok(changed)
}

/// Run `systemctl <verb> <unit>`, using `sudo -n` when the service runs as a
/// non-root user. Returns `Ok(())` on exit code 0.
fn systemctl_verb(verb: &str, unit: &str) -> std::io::Result<()> {
    let bin = kiosk_systemctl_bin();
    let status = if effective_uid_is_root() {
        std::process::Command::new(&bin).args([verb, unit]).status()?
    } else {
        std::process::Command::new("sudo")
            .args(["-n", &bin, verb, unit])
            .status()?
    };
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("systemctl {verb} {unit} failed (exit {:?})", status.code()),
        ))
    }
}

pub fn start_kiosk_unit() -> std::io::Result<()> {
    systemctl_verb("start", KIOSK_UNIT)
}

pub fn stop_kiosk_unit() -> std::io::Result<()> {
    systemctl_verb("stop", KIOSK_UNIT)
}

pub fn restart_kiosk_unit() -> std::io::Result<()> {
    systemctl_verb("restart", KIOSK_UNIT)
}

pub fn start_autorotate_unit() -> std::io::Result<()> {
    systemctl_verb("start", KIOSK_AUTOROTATE_UNIT)
}

pub fn stop_autorotate_unit() -> std::io::Result<()> {
    systemctl_verb("stop", KIOSK_AUTOROTATE_UNIT)
}

/// `systemctl is-active --quiet <unit>` -> bool. Never fails; returns false
/// on any error.
pub fn unit_is_active(unit: &str) -> bool {
    let bin = kiosk_systemctl_bin();
    let mut cmd = std::process::Command::new(&bin);
    cmd.args(["is-active", "--quiet", unit]);
    match cmd.status() {
        Ok(s) => s.success(),
        Err(_) => false,
    }
}

/// Outcome enum used by the caller (Socket.IO saveKioskSettings branch) so
/// toasts can be targeted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// Everything applied; the kiosk unit is active.
    Running,
    /// Everything applied; the kiosk unit is stopped (user disabled).
    Stopped,
    /// Kiosk requested but no DRM card exists.
    NoDrm,
    /// Kiosk requested but the service runs as root (policy: refuse).
    RootUserRefused,
    /// Overlays written but the unit action (start/stop/restart) failed; see log.
    PartialFailure,
}

/// Apply the full set of kiosk-related settings from `SystemSettings` to the
/// layer. Called from the Socket.IO `saveKioskSettings` branch AND from
/// `crate::api::run_startup_kiosk_apply` on boot.
///
/// The call is idempotent: overlays only change when values differ, and
/// systemctl verbs are only run when the current unit state disagrees with
/// the requested state (or when a live rotation / output change requires a
/// debounced restart).
pub async fn apply_kiosk_settings(
    state: &crate::api::AppState,
) -> ApplyOutcome {
    let sys = state.system_settings.read().await.clone();

    // Overlay writes first - harmless even if the unit is inactive. We
    // capture the "changed" bool from each apply_* so the running
    // kiosk can be restarted when any one of them differs from disk
    // (same mechanism the UI toggles depend on to take effect).
    let primary_display_changed = match apply_primary_display(&sys.primary_display) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("{} primary_display overlay: {}", EVO_KIOSK, e);
            false
        }
    };
    let rotation_changed = match apply_rotation(sys.kiosk_rotation) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("{} rotation overlay: {}", EVO_KIOSK, e);
            false
        }
    };
    let osk_changed = match apply_osk(&sys.kiosk_osk) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("{} osk overlay: {}", EVO_KIOSK, e);
            false
        }
    };
    let cursor_changed = match apply_cursor(&sys.kiosk_cursor) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("{} cursor overlay: {}", EVO_KIOSK, e);
            false
        }
    };
    let auto_rotate_changed = match apply_auto_rotate_overlay(sys.kiosk_auto_rotate) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("{} auto_rotate overlay: {}", EVO_KIOSK, e);
            false
        }
    };
    let any_overlay_changed = primary_display_changed
        || rotation_changed
        || osk_changed
        || cursor_changed
        || auto_rotate_changed;

    // Desired state:
    //   kiosk_enabled=true  + DRM present + non-root user -> unit active
    //   kiosk_enabled=false                                -> unit inactive
    //   kiosk_enabled=true  + no DRM                       -> refuse, revert toggle is caller's job
    //   kiosk_enabled=true  + root service user            -> refuse by default
    if sys.kiosk_enabled {
        if !drm_device_present() {
            tracing::warn!(
                "{} kiosk_enabled=true but no /dev/dri/card* present; refusing to start",
                EVO_KIOSK
            );
            return ApplyOutcome::NoDrm;
        }
        let allow_root = std::env::var("KIOSK_ALLOW_ROOT")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if effective_uid_is_root() && !allow_root {
            tracing::warn!(
                "{} kiosk_enabled=true but service runs as root and KIOSK_ALLOW_ROOT!=1; refusing",
                EVO_KIOSK
            );
            return ApplyOutcome::RootUserRefused;
        }

        let active = unit_is_active(KIOSK_UNIT);
        let op_ok = if active {
            if any_overlay_changed {
                // Running kiosk reads overlays at launcher start; any
                // change requires a unit restart so the running browser
                // picks up the new values (OSK selection, rotation,
                // cursor policy, primary display, auto-rotate).
                tracing::info!(
                    "{} live kiosk settings change (rotation={} osk={} cursor={} primary_display={} auto_rotate={}); restarting kiosk unit",
                    EVO_KIOSK,
                    rotation_changed,
                    osk_changed,
                    cursor_changed,
                    primary_display_changed,
                    auto_rotate_changed,
                );
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                restart_kiosk_unit().map_err(|e| {
                    tracing::warn!("{} restart_kiosk_unit: {}", EVO_KIOSK, e);
                    e
                })
            } else {
                Ok(())
            }
        } else {
            tracing::info!("{} starting {}", EVO_KIOSK, KIOSK_UNIT);
            start_kiosk_unit().map_err(|e| {
                tracing::warn!("{} start_kiosk_unit: {}", EVO_KIOSK, e);
                e
            })
        };

        // Autorotate unit follows auto_rotate bool, but only when kiosk itself
        // is enabled (BindsTo= already handles teardown, but we keep it tidy).
        let ar_current = unit_is_active(KIOSK_AUTOROTATE_UNIT);
        let ar_op = match (sys.kiosk_auto_rotate, ar_current) {
            (true, false) => {
                tracing::info!("{} starting {}", EVO_KIOSK, KIOSK_AUTOROTATE_UNIT);
                start_autorotate_unit()
            }
            (false, true) => {
                tracing::info!("{} stopping {}", EVO_KIOSK, KIOSK_AUTOROTATE_UNIT);
                stop_autorotate_unit()
            }
            _ => Ok(()),
        };
        if let Err(e) = ar_op {
            tracing::warn!("{} autorotate unit: {}", EVO_KIOSK, e);
        }

        match op_ok {
            Ok(()) => ApplyOutcome::Running,
            Err(_) => ApplyOutcome::PartialFailure,
        }
    } else {
        // Disabled: autorotate first (BindsTo is belt-and-braces, do it
        // explicitly to get a clean shutdown ordering).
        if unit_is_active(KIOSK_AUTOROTATE_UNIT) {
            if let Err(e) = stop_autorotate_unit() {
                tracing::warn!("{} stop_autorotate_unit: {}", EVO_KIOSK, e);
            }
        }
        if unit_is_active(KIOSK_UNIT) {
            tracing::info!("{} stopping {}", EVO_KIOSK, KIOSK_UNIT);
            if let Err(e) = stop_kiosk_unit() {
                tracing::warn!("{} stop_kiosk_unit: {}", EVO_KIOSK, e);
                return ApplyOutcome::PartialFailure;
            }
        }
        ApplyOutcome::Stopped
    }
}

/// Snapshot used by `GET /api/v1/kiosk/status`. Reads persisted settings +
/// live unit state + DRM probe. No side effects.
pub async fn kiosk_status_json(state: &crate::api::AppState) -> serde_json::Value {
    let sys = state.system_settings.read().await.clone();
    json!({
        "kiosk_enabled":     sys.kiosk_enabled,
        "primary_display":   sys.primary_display,
        "rotation":          normalize_rotation(sys.kiosk_rotation),
        "auto_rotate":       sys.kiosk_auto_rotate,
        "osk":               normalize_osk(&sys.kiosk_osk),
        "cursor":            normalize_cursor(&sys.kiosk_cursor),
        "unit_active":       unit_is_active(KIOSK_UNIT),
        "autorotate_active": unit_is_active(KIOSK_AUTOROTATE_UNIT),
        "drm_present":       drm_device_present(),
        "service_user":      service_user_hint(),
        "unit_name":         KIOSK_UNIT,
        "autorotate_unit":   KIOSK_AUTOROTATE_UNIT,
    })
}
