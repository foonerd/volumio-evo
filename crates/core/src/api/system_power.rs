//! Graceful shutdown / reboot (shared by Socket.IO and sleep timer).
//!
//! Non-root service user: **only** `sudo -n` with paths from the environment (drop-in from
//! bootstrap) — see `docs/OS_PRIVILEGE_MODEL.md` and `volumio-evo-power` sudoers.

use super::AppState;
use crate::mpd::{self, MpdConfig};
use std::time::Duration;

use crate::playback_options::evo_non_root_service;

fn mpd_config(state: &AppState) -> MpdConfig {
    tracing::debug!(
        "{} system_power::mpd_config {}:{}",
        crate::log_tags::EVO_UI,
        state.config.mpd_host,
        state.config.mpd_port
    );
    MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    }
}

fn systemctl_bin() -> String {
    std::env::var("VOLUMIO_EVO_SYSTEMCTL").unwrap_or_else(|_| "/usr/bin/systemctl".to_string())
}

fn reboot_bin() -> String {
    std::env::var("VOLUMIO_EVO_REBOOT_BIN").unwrap_or_else(|_| "/sbin/reboot".to_string())
}

fn shutdown_bin() -> String {
    std::env::var("VOLUMIO_EVO_SHUTDOWN_BIN").unwrap_or_else(|_| "/sbin/shutdown".to_string())
}

/// Run `systemctl …` as root, or `sudo -n systemctl …` when Evo runs as the non-root service user.
async fn run_systemctl(args: &[&str]) -> std::io::Result<std::process::Output> {
    let systemctl = systemctl_bin();
    if evo_non_root_service() {
        tokio::process::Command::new("/usr/bin/sudo")
            .arg("-n")
            .arg(&systemctl)
            .args(args)
            .output()
            .await
    } else {
        tokio::process::Command::new(&systemctl).args(args).output().await
    }
}

/// Stop `smbd` / `nmbd` with two invocations so sudoers can use the same lines as
/// `volumio-evo-samba` (`systemctl stop smbd` / `stop nmbd` — not one `stop smbd nmbd` line).
async fn try_stop_samba_units() {
    let _ = run_systemctl(&["stop", "smbd"]).await;
    let _ = run_systemctl(&["stop", "nmbd"]).await;
}

/// Stop playback, release Samba daemons if present, unmount NAS shares, sync, then `systemctl` (with
/// `/sbin/shutdown` or `reboot` fallback after 3s, matching volumio3-backend `platformSpecific.js`).
pub(crate) async fn graceful_power_transition(state: AppState, reboot: bool) {
    tracing::debug!(
        "{} graceful_power_transition enter reboot={}",
        crate::log_tags::EVO_UI,
        reboot
    );
    let tag = crate::log_tags::EVO_UI;
    let config = mpd_config(&state);
    if let Err(e) = mpd::run_command_connected(&config, "stop", None, None, None, None).await {
        tracing::warn!("{} pre-power: MPD stop: {}", tag, e);
    }

    try_stop_samba_units().await;

    if let Err(e) = state.network_mounts.umount_all_shares().await {
        tracing::warn!("{} pre-power: list shares / umount: {}", tag, e);
    }

    let sync_ok = tokio::task::spawn_blocking(|| {
        std::process::Command::new("/bin/sync")
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false);
    if !sync_ok {
        tracing::warn!("{} pre-power: sync did not report success", tag);
    }

    let action = if reboot { "reboot" } else { "poweroff" };
    match run_systemctl(&[action]).await {
        Ok(o) if o.status.success() => {
            tracing::info!("{} systemctl {} started", tag, action);
        }
        Ok(o) => {
            tracing::warn!(
                "{} systemctl {}: {}",
                tag,
                action,
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Err(e) => tracing::warn!("{} systemctl {}: {}", tag, action, e),
    }

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let fallback_result = if reboot {
            let b = reboot_bin();
            if evo_non_root_service() {
                tokio::process::Command::new("/usr/bin/sudo")
                    .arg("-n")
                    .arg(&b)
                    .output()
                    .await
            } else {
                tokio::process::Command::new(&b).output().await
            }
        } else {
            let s = shutdown_bin();
            if evo_non_root_service() {
                tokio::process::Command::new("/usr/bin/sudo")
                    .arg("-n")
                    .arg(&s)
                    .args(["-h", "now"])
                    .output()
                    .await
            } else {
                tokio::process::Command::new(&s)
                    .args(["-h", "now"])
                    .output()
                    .await
            }
        };
        if let Err(e) = fallback_result {
            tracing::warn!("{} fallback power command: {}", tag, e);
        }
    });
}
