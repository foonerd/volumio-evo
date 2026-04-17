//! Wi‑Fi **rfkill** soft block handling. NetworkManager cannot scan or use radios that are
//! soft-blocked; Evo unblocks **`wifi`** via **`rfkill`** (root) or **`sudo -n`** (see bootstrap sudoers).

use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;

#[cfg(unix)]
fn effective_uid_is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
fn effective_uid_is_root() -> bool {
    false
}

/// Path to **`rfkill`**; must match **`/etc/sudoers.d/volumio-evo-rfkill`** when non-root.
/// Bootstrap sets **`Environment=VOLUMIO_EVO_RFKILL=...`** in `10-runtime-user.conf`.
pub fn rfkill_bin() -> String {
    std::env::var("VOLUMIO_EVO_RFKILL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/sbin/rfkill".to_string())
}

/// True if any **`wlan`** rfkill reports soft block (`/sys/class/rfkill/*/soft` == 1).
pub fn wlan_soft_blocked() -> bool {
    let Ok(dir) = std::fs::read_dir("/sys/class/rfkill") else {
        return false;
    };
    for ent in dir.flatten() {
        let p = ent.path();
        let Ok(typ) = std::fs::read_to_string(p.join("type")) else {
            continue;
        };
        if typ.trim() != "wlan" {
            continue;
        }
        let Ok(soft) = std::fs::read_to_string(p.join("soft")) else {
            continue;
        };
        if soft.trim() == "1" {
            return true;
        }
    }
    false
}

/// If Wi‑Fi is soft-blocked, run **`rfkill unblock wifi`** (root) or **`sudo -n <rfkill> unblock wifi`**.
pub async fn ensure_wifi_unblocked_for_nm() {
    if !wlan_soft_blocked() {
        return;
    }
    let bin = rfkill_bin();
    if !Path::new(&bin).is_file() {
        tracing::warn!(
            "{} rfkill not found at {} — cannot unblock Wi‑Fi (install rfkill package or set VOLUMIO_EVO_RFKILL)",
            crate::log_tags::EVO_NET,
            bin
        );
        return;
    }

    let is_root = effective_uid_is_root();

    let result = if is_root {
        Command::new(&bin)
            .args(["unblock", "wifi"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await
    } else {
        Command::new("sudo")
            .args(["-n", &bin, "unblock", "wifi"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await
    };

    match result {
        Ok(out) if out.status.success() => {
            tracing::info!(
                "{} Wi‑Fi was soft-blocked (rfkill); ran {} unblock wifi",
                crate::log_tags::EVO_NET,
                if is_root { &bin } else { "sudo -n" }
            );
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            tracing::warn!(
                "{} could not rfkill unblock wifi ({}): {} — non-root needs /etc/sudoers.d/volumio-evo-rfkill (re-run bootstrap)",
                crate::log_tags::EVO_NET,
                bin,
                err.trim()
            );
        }
        Err(e) => {
            tracing::warn!(
                "{} spawn rfkill unblock: {} — check VOLUMIO_EVO_RFKILL and sudoers",
                crate::log_tags::EVO_NET,
                e
            );
        }
    }
}
