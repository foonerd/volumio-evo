//! Graceful shutdown / reboot (shared by Socket.IO and sleep timer).

use super::AppState;
use std::time::Duration;

/// Stop playback, release Samba daemons if present, unmount NAS shares, sync, then `systemctl` (with
/// `/sbin/shutdown` or `/sbin/reboot` fallback after 3s, matching volumio3-backend `platformSpecific.js`).
pub(crate) async fn graceful_power_transition(state: AppState, reboot: bool) {
    tracing::debug!(
        "{} graceful_power_transition enter reboot={}",
        crate::log_tags::EVO_UI,
        reboot
    );
    let tag = crate::log_tags::EVO_UI;
    if let Err(e) = crate::playback_router::run_command_connected_with_video(
        &state,
        "stop",
        None,
        None,
        None,
        None,
    )
    .await
    {
        tracing::warn!("{} pre-power: playback stop: {}", tag, e);
    }

    let _ = tokio::process::Command::new("sudo")
        .args(["/usr/bin/systemctl", "stop", "smbd", "nmbd"])
        .output()
        .await;

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
    match tokio::process::Command::new("sudo")
        .args(["/usr/bin/systemctl", action])
        .output()
        .await
    {
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
        let fallback = if reboot {
            tokio::process::Command::new("sudo")
                .arg("/sbin/reboot")
                .output()
                .await
        } else {
            tokio::process::Command::new("sudo")
                .args(["/sbin/shutdown", "-h", "now"])
                .output()
                .await
        };
        if let Err(e) = fallback {
            tracing::warn!("{} fallback power command: {}", tag, e);
        }
    });
}
