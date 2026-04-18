//! RTC wake via **`rtcwake`** from **util-linux** (`/usr/sbin/rtcwake`).
//!
//! Used to schedule **wake-from-suspend** so an alarm can resume playback after **`mem`** / **`freeze`**
//! / similar. Actual suspend is usually **`systemctl suspend`** or **`rtcwake -m mem`**; this module
//! focuses on **programming** the RTC (`-m no`) and **clear**/**show** — see **[ALARM_WAKE.md](../../../docs/ALARM_WAKE.md)**.

use std::path::Path;
use std::process::Command;

#[cfg(unix)]
fn effective_uid_is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
fn effective_uid_is_root() -> bool {
    false
}

/// Path to **`rtcwake`**. Must match **`/etc/sudoers.d/volumio-evo-rtcwake`** when non-root.
/// Bootstrap sets **`Environment=VOLUMIO_EVO_RTCWAKE=...`** in **`10-runtime-user.conf`**.
pub fn rtcwake_bin() -> String {
    std::env::var("VOLUMIO_EVO_RTCWAKE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/sbin/rtcwake".to_string())
}

/// Optional RTC device name for **`-d`** (e.g. **`rtc0`**). Unset = rtcwake default.
pub fn rtcwake_device() -> Option<String> {
    std::env::var("VOLUMIO_EVO_RTC_DEVICE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn base_rtc_command() -> (std::process::Command, String) {
    let bin = rtcwake_bin();
    let mut cmd = if effective_uid_is_root() {
        Command::new(&bin)
    } else {
        let mut c = Command::new("sudo");
        c.arg("-n").arg(&bin);
        c
    };
    if let Some(ref dev) = rtcwake_device() {
        cmd.args(["-d", dev]);
    }
    (cmd, bin)
}

/// Program the RTC wake alarm for **UTC** **`time_t`** (**`-t`**). Does **not** suspend (**`-m no`**).
///
/// Caller must convert “alarm at local wall time” → Unix epoch **UTC** (e.g. **chrono** + timezone).
#[allow(dead_code)]
pub fn program_wake_utc_epoch(epoch_secs: i64) -> Result<(), std::io::Error> {
    let (mut cmd, bin) = base_rtc_command();
    cmd.args(["-m", "no", "-t"]).arg(epoch_secs.to_string());
    let st = cmd.status()?;
    if st.success() {
        tracing::info!(
            "{} rtcwake -m no -t {} ({} )",
            crate::log_tags::EVO_RTC,
            epoch_secs,
            if effective_uid_is_root() { &bin } else { "sudo -n" }
        );
        Ok(())
    } else {
        tracing::warn!(
            "{} rtcwake program wake failed (exit {:?}) — non-root needs {} + bootstrap sudoers ({})",
            crate::log_tags::EVO_RTC,
            st.code(),
            bin,
            "/etc/sudoers.d/volumio-evo-rtcwake"
        );
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "rtcwake -m no failed",
        ))
    }
}

/// Clear a previously programmed RTC alarm (**`rtcwake -m disable`**).
#[allow(dead_code)]
pub fn clear_wake() -> Result<(), std::io::Error> {
    let (mut cmd, bin) = base_rtc_command();
    cmd.args(["-m", "disable"]);
    let st = cmd.status()?;
    if st.success() {
        tracing::info!(
            "{} rtcwake -m disable ok ({})",
            crate::log_tags::EVO_RTC,
            if effective_uid_is_root() { &bin } else { "sudo -n" }
        );
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "rtcwake -m disable failed",
        ))
    }
}

/// One-shot diagnostic: log **`rtcwake`** path, optional **`-d`**, and **`rtcwake -m show`** when permitted.
pub fn log_startup_probe() {
    let bin = rtcwake_bin();
    if !Path::new(&bin).is_file() {
        tracing::debug!(
            "{} rtcwake not found at {} — install util-linux; alarm wake-from-suspend unavailable",
            crate::log_tags::EVO_RTC,
            bin
        );
        return;
    }
    tracing::info!(
        "{} rtcwake: {}",
        crate::log_tags::EVO_RTC,
        bin
    );
    if let Some(ref d) = rtcwake_device() {
        tracing::info!("{} RTC device (-d): {}", crate::log_tags::EVO_RTC, d);
    }
    match wake_show_text() {
        Some(line) => tracing::info!("{} {}", crate::log_tags::EVO_RTC, line),
        None => tracing::debug!(
            "{} rtcwake -m show unavailable (often needs bootstrap sudoers or root)",
            crate::log_tags::EVO_RTC
        ),
    }
}

/// Human-readable alarm line from **`rtcwake -m show`** (empty if unavailable).
pub fn wake_show_text() -> Option<String> {
    let bin = rtcwake_bin();
    if !Path::new(&bin).is_file() {
        return None;
    }
    let (mut cmd, _) = base_rtc_command();
    cmd.args(["-m", "show"]);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
