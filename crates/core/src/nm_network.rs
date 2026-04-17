//! NetworkManager integration via **`nmcli`** (non-interactive).
//! See **[docs/NETWORK_NM.md](../../../docs/NETWORK_NM.md)** for architecture and rollout phases.
//!
//! Phase 1: device listing, Wi‑Fi scan → stock UI shape `{ available: [...] }`, REST diagnostic.
//! Phase 2: apply persisted [`crate::network_config::NetworkIntent`] (DHCP/static, Wi‑Fi STA/AP, hotspot profile).

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::process::Command;

use crate::config::Config;
use crate::network_config::{
    FallbackIntent, NetworkIntent, WifiIntent, WifiRole, NM_CON_ETHERNET, NM_CON_HOTSPOT,
    NM_CON_WIFI_STA,
};
use crate::network_config::{EthernetIntent, Ipv4Mode};

/// Default Wi‑Fi interface when not specified (see [`crate::network_config::DEFAULT_WIFI_IFACE`]).
pub use crate::network_config::DEFAULT_WIFI_IFACE;

#[cfg(unix)]
fn effective_uid_is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
fn effective_uid_is_root() -> bool {
    false
}

/// Path to **`nmcli`**. Must match **`/etc/sudoers.d/volumio-evo-nmcli`** (bootstrap) when Evo runs non-root.
/// Override: **`VOLUMIO_EVO_NMCLI`** (same pattern as **`VOLUMIO_EVO_RFKILL`**).
pub fn nmcli_bin() -> String {
    std::env::var("VOLUMIO_EVO_NMCLI")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/bin/nmcli".to_string())
}

async fn nmcli_spawn_output(args: &[&str]) -> Result<std::process::Output> {
    let bin = nmcli_bin();
    let mut cmd = if effective_uid_is_root() {
        Command::new(&bin)
    } else {
        let mut c = Command::new("sudo");
        c.arg("-n").arg(&bin);
        c
    };
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .with_context(|| format!("spawn {} (or sudo -n)", bin))
}

async fn nmcli_output(args: &[&str]) -> Result<String> {
    let out = nmcli_spawn_output(args).await?;
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        let detail = format!("{} {}", stderr.trim(), stdout.trim()).trim().to_string();
        tracing::warn!(
            "{} nmcli failed (exit {}): {} — non-root needs NOPASSWD for {} (see bootstrap volumio-evo-nmcli / VOLUMIO_EVO_NMCLI)",
            crate::log_tags::EVO_NET,
            code,
            detail,
            nmcli_bin()
        );
        return Err(anyhow!(
            "nmcli failed (exit {}): {}\n{}",
            code,
            stderr.trim(),
            stdout.trim()
        ));
    }
    Ok(stdout)
}

/// `nmcli -t -f DEVICE,TYPE,STATE,CONNECTION device` as rows.
#[derive(Debug, Clone, Serialize)]
pub struct NmDeviceRow {
    pub device: String,
    pub kind: String,
    pub state: String,
    pub connection: String,
}

pub async fn nm_device_table() -> Result<Vec<NmDeviceRow>> {
    let raw = nmcli_output(&["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device"]).await?;
    let mut rows = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split(':');
        let device = parts.next().unwrap_or("").to_string();
        let kind = parts.next().unwrap_or("").to_string();
        let state = parts.next().unwrap_or("").to_string();
        let connection = parts.next().unwrap_or("").to_string();
        rows.push(NmDeviceRow {
            device,
            kind,
            state,
            connection,
        });
    }
    Ok(rows)
}

/// Active connection **names** bound to **`ifname`** (`nmcli -t -f NAME,DEVICE connection show --active`).
/// When the stack supports **concurrent STA+AP**, both profiles may appear for the same `wlan*`.
async fn nm_active_connection_names_on_device(ifname: &str) -> Vec<String> {
    let want = ifname.trim();
    if want.is_empty() {
        return Vec::new();
    }
    let raw = match nmcli_output(&[
        "-t",
        "-f",
        "NAME,DEVICE",
        "connection",
        "show",
        "--active",
    ])
    .await
    {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut names = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split(':');
        let name = parts.next().unwrap_or("").trim();
        let dev = parts.next().unwrap_or("").trim();
        if dev == want && !name.is_empty() {
            names.push(name.to_string());
        }
    }
    names
}

/// One row from `nmcli dev wifi list` (`-t -f SSID,SIGNAL,SECURITY,ACTIVE`).
#[derive(Debug, Clone, Serialize)]
pub struct NmWifiAp {
    pub ssid: String,
    /// 0–100 as reported by NM (may be empty for hidden).
    pub signal_pct: Option<u8>,
    pub security: String,
    pub active: bool,
}

/// Best-effort: turn NM Wi‑Fi radio on (helps when the device shows `unavailable` / `sw disabled`).
async fn nmcli_radio_wifi_on_best_effort() {
    let _ = nmcli_spawn_output(&["radio", "wifi", "on"]).await;
}

/// Best-effort rescan before listing APs (`nmcli device wifi rescan [ifname …]`).
async fn nmcli_dev_wifi_rescan_best_effort(ifname: Option<&str>) {
    let mut v: Vec<String> = vec!["device".into(), "wifi".into(), "rescan".into()];
    if let Some(i) = ifname {
        if !i.is_empty() {
            v.push("ifname".into());
            v.push(i.to_string());
        }
    }
    let args_ref: Vec<&str> = v.iter().map(|s| s.as_str()).collect();
    let _ = nmcli_output(&args_ref).await;
}

/// Persist UI-chosen STA iface: `settings/network/wifi_iface_preferred` + merge into `/etc/volumio-evo/config.toml` (best-effort `sudo install` when not root; see bootstrap **`volumio-evo-config-install`**).
pub async fn persist_user_wifi_iface_preference(iface: &str) -> Result<()> {
    let iface = iface.trim();
    if iface.is_empty() {
        anyhow::bail!("wifi interface name is empty");
    }
    crate::network_config::write_wifi_iface_preferred(iface)?;
    let etc = Path::new("/etc/volumio-evo/config.toml");
    let base = std::fs::read_to_string(etc).unwrap_or_default();
    let merged = crate::network_config::merge_toml_wifi_iface(&base, iface)?;
    let pending = crate::network_config::config_toml_pending_path();
    if let Some(parent) = pending.parent() {
        std::fs::create_dir_all(parent).context("create pending config parent")?;
    }
    std::fs::write(&pending, merged.as_bytes()).with_context(|| format!("write {}", pending.display()))?;
    install_pending_system_config(&pending).await?;
    Ok(())
}

async fn install_pending_system_config(pending: &Path) -> Result<()> {
    let dest = Path::new("/etc/volumio-evo/config.toml");
    if effective_uid_is_root() {
        std::fs::copy(pending, dest).with_context(|| format!("copy to {}", dest.display()))?;
        return Ok(());
    }
    let st = Command::new("sudo")
        .arg("-n")
        .arg("/usr/bin/install")
        .arg("-o")
        .arg("root")
        .arg("-g")
        .arg("root")
        .arg("-m")
        .arg("644")
        .arg(pending.as_os_str())
        .arg(dest.as_os_str())
        .status()
        .await
        .context("sudo install config.toml")?;
    if !st.success() {
        tracing::warn!(
            "{} could not copy merged config to {} (sudo install failed; preferred iface file still applies). Install bootstrap sudoers **volumio-evo-config-install** or run Evo as root.",
            crate::log_tags::EVO_NET,
            dest.display()
        );
    }
    Ok(())
}

/// Effective Wi‑Fi interface for NM scans and diagnostics.
///
/// 1. **`VOLUMIO_EVO_WIFI_IFACE`** environment variable (admin override).
/// 2. Persisted **`settings/network/wifi_iface_preferred`** (Network UI).
/// 3. Non-empty [`Config::wifi_iface`] from `/etc` / startup (includes env merged at load).
/// 4. Otherwise, pick the first `wifi` device from **`nmcli -t device`**, **preferring** one whose
///    state does not look like **`unavailable`** when several radios exist (e.g. USB `wlan1` vs dead SoC `wlan0`).
/// 5. Fallback: [`DEFAULT_WIFI_IFACE`] (`wlan0`).
pub async fn resolve_effective_wifi_iface(config: &Config) -> String {
    if let Ok(v) = std::env::var("VOLUMIO_EVO_WIFI_IFACE") {
        let t = v.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if let Some(s) = crate::network_config::read_wifi_iface_preferred() {
        let t = s.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if let Some(ref s) = config.wifi_iface {
        let t = s.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    match nm_device_table().await {
        Ok(rows) => {
            let wifi: Vec<&NmDeviceRow> = rows
                .iter()
                .filter(|r| r.kind.eq_ignore_ascii_case("wifi") && !r.device.trim().is_empty())
                .collect();
            if wifi.is_empty() {
                return DEFAULT_WIFI_IFACE.to_string();
            }
            if let Some(r) = wifi.iter().find(|r| {
                let st = r.state.to_ascii_lowercase();
                !st.contains("unavailable")
            }) {
                return r.device.trim().to_string();
            }
            wifi[0].device.trim().to_string()
        }
        Err(_) => DEFAULT_WIFI_IFACE.to_string(),
    }
}

/// Unblock rfkill and enable the NM Wi‑Fi radio so **client (STA) mode** can scan and connect.
/// Call when applying intent after the user turns **Wireless Networking** on.
pub async fn ensure_wifi_client_hw_ready() {
    tracing::debug!(
        "{} wifi_client_hw_ready begin (rfkill unblock + nmcli radio on)",
        crate::log_tags::EVO_NET
    );
    crate::rfkill_mgmt::ensure_wifi_unblocked_for_nm().await;
    nmcli_radio_wifi_on_best_effort().await;
}

/// Raw scan rows (tab-separated).
pub async fn wifi_scan_rows(ifname: Option<&str>) -> Result<Vec<NmWifiAp>> {
    crate::rfkill_mgmt::ensure_wifi_unblocked_for_nm().await;
    nmcli_radio_wifi_on_best_effort().await;
    nmcli_dev_wifi_rescan_best_effort(ifname).await;
    let mut args: Vec<String> = vec![
        "-t".into(),
        "-f".into(),
        "SSID,SIGNAL,SECURITY,ACTIVE".into(),
        "dev".into(),
        "wifi".into(),
        "list".into(),
    ];
    if let Some(iface) = ifname {
        args.push("ifname".into());
        args.push(iface.to_string());
    }
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let raw = nmcli_output(&args_ref).await?;
    let mut out = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.split(':');
        let ssid = parts.next().unwrap_or("").trim().to_string();
        let signal_s = parts.next().unwrap_or("").trim();
        let security = parts.next().unwrap_or("").trim().to_string();
        let active_s = parts.next().unwrap_or("").trim();
        let signal_pct = signal_s.parse::<u8>().ok();
        let active = matches!(active_s.to_ascii_lowercase().as_str(), "yes" | "y");
        if ssid.is_empty() {
            continue;
        }
        out.push(NmWifiAp {
            ssid,
            signal_pct,
            security,
            active,
        });
    }
    Ok(out)
}

/// Map NM signal 0–100 to UI bars 1–5 (same idea as volumio3-backend `processWirelessNetworksArray`).
pub(crate) fn signal_bars_from_pct(pct: Option<u8>) -> u8 {
    let Some(p) = pct else {
        return 0;
    };
    if p >= 80 {
        5
    } else if p >= 60 {
        4
    } else if p >= 40 {
        3
    } else if p >= 20 {
        2
    } else if p > 0 {
        1
    } else {
        0
    }
}

/// Map NM `SECURITY` column to stock-style `security` string (`open` / `wpa2` / …).
fn security_label(nm_sec: &str) -> &'static str {
    let s = nm_sec.to_ascii_lowercase();
    if s.is_empty() || s.contains("--") {
        return "open";
    }
    if s.contains("wpa3") {
        return "wpa3";
    }
    if s.contains("wpa2") || s.contains("wpa") {
        return "wpa2";
    }
    if s.contains("wep") {
        return "wep";
    }
    "open"
}

/// Stock UI shape: `{ available: [ { ssid, signal, security }, ... ] }`.
#[derive(Debug, Clone, Serialize)]
pub struct WirelessNetworkUi {
    pub ssid: String,
    /// 1–5 bars (0 if unknown).
    pub signal: u8,
    pub security: String,
}

pub async fn wifi_scan_ui_list(ifname: Option<&str>) -> Result<Vec<WirelessNetworkUi>> {
    let rows = wifi_scan_rows(ifname).await?;
    let mut seen = std::collections::HashSet::<String>::new();
    let mut out = Vec::new();
    for r in rows {
        if !seen.insert(r.ssid.clone()) {
            continue;
        }
        out.push(WirelessNetworkUi {
            ssid: r.ssid,
            signal: signal_bars_from_pct(r.signal_pct),
            security: security_label(&r.security).to_string(),
        });
    }
    Ok(out)
}

/// JSON for `pushWirelessNetworks` (Node-compatible top-level key `available`).
pub async fn wifi_scan_push_wireless_networks_value(ifname: Option<&str>) -> Value {
    match wifi_scan_ui_list(ifname).await {
        Ok(v) => {
            if v.is_empty() {
                let hint = ifname.unwrap_or("(any)");
                tracing::info!(
                    "{} Wi‑Fi scan returned no SSIDs (ifname={}). If `nmcli` shows the device as unavailable or sw disabled, check: rfkill, `nmcli radio wifi on`, regulatory domain, and driver.",
                    crate::log_tags::EVO_NET,
                    hint
                );
            }
            json!({ "available": v })
        }
        Err(e) => {
            tracing::warn!("{} nmcli wifi scan: {}", crate::log_tags::EVO_NET, e);
            json!({ "available": [], "error": format!("{}", e) })
        }
    }
}

#[derive(Debug, Serialize)]
pub struct NmDiagnostic {
    pub nmcli_available: bool,
    pub general_status: Option<String>,
    pub devices: Vec<NmDeviceRow>,
    /// Interface used for the diagnostic Wi‑Fi scan (matches Evo’s effective iface for scans).
    pub scan_ifname: String,
    pub wifi_scan_error: Option<String>,
}

/// Aggregated snapshot for `GET /api/v1/network/nm/status` and logs.
pub async fn diagnostic_snapshot(wifi_ifname: Option<&str>) -> NmDiagnostic {
    let which = Command::new("which")
        .arg("nmcli")
        .output()
        .await
        .ok()
        .filter(|o| o.status.success());
    let nmcli_available = which.is_some();

    let general = if nmcli_available {
        nmcli_output(&["general", "status"])
            .await
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    };

    let devices = nm_device_table().await.unwrap_or_default();

    let iface = wifi_ifname.unwrap_or(DEFAULT_WIFI_IFACE);
    let wifi_scan_error = match wifi_scan_rows(Some(iface)).await {
        Ok(_) => None,
        Err(e) => Some(format!("{}", e)),
    };

    NmDiagnostic {
        nmcli_available,
        general_status: general,
        devices,
        scan_ifname: iface.to_string(),
        wifi_scan_error,
    }
}

// --- Phase 2: apply intent -------------------------------------------------

/// Human-readable log lines from [`apply_network_intent`].
#[derive(Debug, Serialize)]
pub struct NetworkApplyReport {
    pub ok: bool,
    pub steps: Vec<String>,
}

/// One **info** summary line plus each nmcli apply **step** at **debug** level.
/// Enable detail with **`log_level`** `verbose` / `debug` in config, or **`RUST_LOG`** (see [`crate::config::LogLevel`]).
pub fn log_network_apply_result(context: &str, report: &NetworkApplyReport) {
    tracing::info!(
        "{} network_apply context={} ok={} step_count={}",
        crate::log_tags::EVO_NET,
        context,
        report.ok,
        report.steps.len()
    );
    for line in &report.steps {
        tracing::debug!(
            "{} network_apply context={} step: {}",
            crate::log_tags::EVO_NET,
            context,
            line
        );
    }
    if !report.ok {
        if let Some(last) = report.steps.last() {
            tracing::warn!(
                "{} network_apply context={} error: {}",
                crate::log_tags::EVO_NET,
                context,
                last
            );
        }
    }
}

fn debug_log_network_intent_snapshot(
    intent: &NetworkIntent,
    sta_if: &str,
    hs_if: &str,
    sta_psk_present: bool,
    ap_psk_present: bool,
) {
    let eth = &intent.ethernet;
    let ipv4 = match eth.ipv4_mode {
        Ipv4Mode::Dhcp => "dhcp",
        Ipv4Mode::Static => "static",
    };
    let wifi_if_raw = intent.wifi.ifname.trim();
    let wifi_if_disp = if wifi_if_raw.is_empty() {
        "(from config/env)"
    } else {
        wifi_if_raw
    };
    let hs_if_raw = intent.fallback.hotspot_ifname.trim();
    let hs_if_src = if hs_if_raw.is_empty() {
        "same_as_sta"
    } else {
        "intent"
    };
    tracing::debug!(
        "{} intent_snapshot wifi.role={:?} wifi.ifname={} sta_if_effective={} hotspot_if_effective={} hotspot_ifname.source={} ethernet.enabled={} ethernet.ipv4={} ethernet.device={} fallback.hotspot_enabled={} fallback.hotspot_fallback={} hotspot_connection={} sta_ssid={:?} ap_ssid={:?} ap_channel={} sta_psk_sidecar={} ap_psk_sidecar={}",
        crate::log_tags::EVO_NET,
        intent.wifi.role,
        wifi_if_disp,
        sta_if,
        hs_if,
        hs_if_src,
        eth.enabled,
        ipv4,
        if eth.device.trim().is_empty() {
            "(auto)"
        } else {
            eth.device.trim()
        },
        intent.fallback.hotspot_enabled,
        intent.fallback.hotspot_fallback,
        intent.fallback.hotspot_connection_name.trim(),
        intent.wifi.sta_ssid,
        intent.wifi.ap_ssid,
        intent.wifi.ap_channel,
        sta_psk_present,
        ap_psk_present,
    );
}

async fn nmcli_output_lossy(args: &[&str]) -> Result<String> {
    nmcli_output(args).await
}

async fn nm_connection_exists(name: &str) -> bool {
    let Ok(out) = nmcli_spawn_output(&["connection", "show", name]).await else {
        return false;
    };
    out.status.success()
}

/// **Open** hotspot (no user passphrase): **omit** the `802-11-wireless-security` setting entirely.
///
/// Do **not** use **`wpa-psk`** with an **empty** `psk`. NetworkManager documents WPA-PSK as 8–63 ASCII (or
/// 64 hex) characters; an empty string is not a valid open network — clients still see WPA and prompt for a
/// password. Hostapd likewise uses an open BSS only when WPA options are absent, not when the passphrase is
/// empty.
///
/// For an **existing** profile that had WPA, clear security with `nmcli connection modify … remove
/// 802-11-wireless-security` (implemented in `push_nm_ap_remove_wireless_security`).
fn push_nm_ap_remove_wireless_security(seq: &mut Vec<String>) {
    seq.push("remove".into());
    seq.push("802-11-wireless-security".into());
}

fn first_ethernet_device(devices: &[NmDeviceRow]) -> Option<String> {
    devices
        .iter()
        .find(|d| d.kind.eq_ignore_ascii_case("ethernet") && d.device != "lo")
        .map(|d| d.device.clone())
}

/// **`/sys/class/net/<iface>/carrier`**: `0` or missing → treat as **no Ethernet link** (cable gone).
fn sysfs_ethernet_no_carrier(ifname: &str) -> bool {
    let path = format!("/sys/class/net/{}/carrier", ifname.trim());
    match std::fs::read_to_string(&path) {
        Ok(s) => s.trim() == "0",
        Err(_) => true,
    }
}

async fn resolved_ethernet_ifname(eth: &EthernetIntent) -> Option<String> {
    if !eth.enabled {
        return None;
    }
    let ifname = eth.device.trim();
    if !ifname.is_empty() {
        return Some(ifname.to_string());
    }
    let table = nm_device_table().await.ok()?;
    first_ethernet_device(&table)
}

/// Ethernet “no LAN” per **`NETWORK_NM.md`**: Ethernet **enabled** in intent, iface resolved, **no carrier**.
async fn ethernet_intent_has_no_carrier(eth: &EthernetIntent) -> bool {
    if !eth.enabled {
        return false;
    }
    let Some(iface) = resolved_ethernet_ifname(eth).await else {
        return false;
    };
    sysfs_ethernet_no_carrier(&iface)
}

const HOTSPOT_BRINGUP_ATTEMPTS: u32 = 4;
const HOTSPOT_BRINGUP_DELAY_MS: u64 = 400;

/// After `connection up hotspot` on a **shared iface**, the STA profile may briefly drop off
/// **`nmcli --active`** while the radio reconfigures for **concurrent STA+AP** (e.g. Pi 5 brcmfmac).
/// We sample a few times so we don't mistake that window for "AP displaced STA".
const STA_AFTER_HOTSPOT_SETTLE_ATTEMPTS: u32 = 15;
const STA_AFTER_HOTSPOT_SETTLE_DELAY_MS: u64 = 200;

/// Returns **`true`** if **`nmcli connection up`** succeeded on any attempt (intermittent NM/driver bring-up).
async fn connection_up_hotspot_with_retries(con_name: &str, steps: &mut Vec<String>) -> bool {
    if con_name.trim().is_empty() {
        return false;
    }
    tracing::debug!(
        "{} nm hotspot connection up begin con={} max_attempts={}",
        crate::log_tags::EVO_NET,
        con_name,
        HOTSPOT_BRINGUP_ATTEMPTS
    );
    for attempt in 1..=HOTSPOT_BRINGUP_ATTEMPTS {
        tracing::debug!(
            "{} nm hotspot connection up attempt {}/{} con={}",
            crate::log_tags::EVO_NET,
            attempt,
            HOTSPOT_BRINGUP_ATTEMPTS,
            con_name
        );
        match nmcli_spawn_output(&["connection", "up", con_name]).await {
            Ok(out) if out.status.success() => {
                if attempt > 1 {
                    steps.push(format!(
                        "brought up {} (attempt {} of {})",
                        con_name, attempt, HOTSPOT_BRINGUP_ATTEMPTS
                    ));
                } else {
                    steps.push(format!("brought up {}", con_name));
                }
                return true;
            }
            Ok(out) => {
                let code = out.status.code().unwrap_or(-1);
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::warn!(
                    "{} connection up {} attempt {}/{} (exit {}): {}",
                    crate::log_tags::EVO_NET,
                    con_name,
                    attempt,
                    HOTSPOT_BRINGUP_ATTEMPTS,
                    code,
                    stderr.trim()
                );
            }
            Err(e) => {
                tracing::warn!(
                    "{} connection up {} attempt {} spawn: {}",
                    crate::log_tags::EVO_NET,
                    con_name,
                    attempt,
                    e
                );
            }
        }
        if attempt < HOTSPOT_BRINGUP_ATTEMPTS {
            tokio::time::sleep(Duration::from_millis(HOTSPOT_BRINGUP_DELAY_MS)).await;
        }
    }
    steps.push(format!(
        "warning: {} failed connection up after {} attempts",
        con_name, HOTSPOT_BRINGUP_ATTEMPTS
    ));
    false
}

async fn nm_connection_remove_wifi_security_lossy(con_name: &str) {
    let _ = nmcli_output_lossy(&[
        "connection",
        "modify",
        con_name,
        "remove",
        "802-11-wireless-security",
    ])
    .await;
}

/// **`NETWORK_NM.md`**: AP enabled but AP **state** failed after retries, **no LAN carrier** → **open** hotspot.
/// Returns **`true`** if the retry after stripping security succeeds.
async fn try_critical_open_hotspot_recovery(
    eth: &EthernetIntent,
    hotspot_enabled: bool,
    hs_name: &str,
    steps: &mut Vec<String>,
) -> bool {
    if !hotspot_enabled || hs_name.trim().is_empty() {
        tracing::debug!(
            "{} critical_open_hotspot_recovery skip (hotspot not enabled or no profile name)",
            crate::log_tags::EVO_NET
        );
        return false;
    }
    if !ethernet_intent_has_no_carrier(eth).await {
        tracing::debug!(
            "{} critical_open_hotspot_recovery skip (LAN intent has carrier or ethernet disabled / no iface — not 'no LAN' case)",
            crate::log_tags::EVO_NET
        );
        return false;
    }
    tracing::warn!(
        "{} critical recovery: Ethernet no carrier; forcing open AP profile {}",
        crate::log_tags::EVO_NET,
        hs_name
    );
    nm_connection_remove_wifi_security_lossy(hs_name).await;
    steps.push(format!(
        "critical: Ethernet no carrier + hotspot failed; forced open AP on {}",
        hs_name
    ));
    connection_up_hotspot_with_retries(hs_name, steps).await
}

/// When **STA** and **hotspot** share one **`wlan*`**, some stacks **replace** STA with AP (single active
/// profile). Others report **both** connections active — **concurrent STA+AP** (see **`NETWORK_NM.md`**).
/// We only release the hotspot and bring STA back when NM shows **hotspot without STA** on the iface.
///
/// When Ethernet is **enabled** but **no carrier**, we **keep** the hotspot active (do not run this)
/// so the device stays reachable for provisioning.
async fn restore_sta_after_hotspot_on_shared_radio(
    eth: &EthernetIntent,
    sta_ifname: &str,
    hotspot_con: &str,
    steps: &mut Vec<String>,
) {
    let ifname = sta_ifname.trim();
    if ifname.is_empty() || hotspot_con.trim().is_empty() {
        return;
    }
    let hs = hotspot_con.trim();

    // Prod concurrent-capable stacks (e.g. Pi 5 brcmfmac): re-raise STA **nonfatally** after AP.
    // On single-mode radios this will fail or race; we then rely on the settle poll below to decide.
    match nmcli_spawn_output(&["connection", "up", NM_CON_WIFI_STA]).await {
        Ok(out) if out.status.success() => {
            tracing::debug!(
                "{} shared iface {}: nonfatal re-prod of {} after hotspot succeeded",
                crate::log_tags::EVO_NET,
                ifname,
                NM_CON_WIFI_STA
            );
        }
        Ok(out) => {
            tracing::debug!(
                "{} shared iface {}: nonfatal re-prod of {} after hotspot non-zero (exit {}): {}",
                crate::log_tags::EVO_NET,
                ifname,
                NM_CON_WIFI_STA,
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Err(e) => {
            tracing::debug!(
                "{} shared iface {}: nonfatal re-prod of {} after hotspot spawn: {}",
                crate::log_tags::EVO_NET,
                ifname,
                NM_CON_WIFI_STA,
                e
            );
        }
    }

    // Settle poll: on brcmfmac / Pi 5 NM may show STA missing for up to ~1–2 s while switching
    // the phy into **STA+AP** operation. Sample a bounded window before concluding.
    let mut has_sta = false;
    let mut has_hs = false;
    for attempt in 1..=STA_AFTER_HOTSPOT_SETTLE_ATTEMPTS {
        let active = nm_active_connection_names_on_device(ifname).await;
        has_sta = active.iter().any(|n| n.trim() == NM_CON_WIFI_STA);
        has_hs = active.iter().any(|n| n.trim() == hs);
        tracing::debug!(
            "{} shared iface {}: settle {}/{} has_sta={} has_hs={}",
            crate::log_tags::EVO_NET,
            ifname,
            attempt,
            STA_AFTER_HOTSPOT_SETTLE_ATTEMPTS,
            has_sta,
            has_hs
        );
        if has_sta && has_hs {
            break; // concurrent STA+AP converged
        }
        if attempt < STA_AFTER_HOTSPOT_SETTLE_ATTEMPTS {
            tokio::time::sleep(Duration::from_millis(STA_AFTER_HOTSPOT_SETTLE_DELAY_MS)).await;
        }
    }

    if has_sta && has_hs {
        tracing::debug!(
            "{} shared iface {}: concurrent STA+AP ({} + {}) — leaving both up",
            crate::log_tags::EVO_NET,
            ifname,
            NM_CON_WIFI_STA,
            hs
        );
        steps.push(format!(
            "shared iface: {} + {} both active (concurrent STA+AP)",
            NM_CON_WIFI_STA, hs
        ));
        return;
    }

    if has_sta && !has_hs {
        tracing::debug!(
            "{} shared iface {}: STA active, hotspot not on iface — skip post-hotspot restore",
            crate::log_tags::EVO_NET,
            ifname
        );
        return;
    }

    if !has_hs {
        tracing::debug!(
            "{} shared iface {}: hotspot not active on device — skip STA restore",
            crate::log_tags::EVO_NET,
            ifname
        );
        return;
    }

    if ethernet_intent_has_no_carrier(eth).await {
        tracing::info!(
            "{} shared radio: keep hotspot active on {} (Ethernet enabled but no carrier — STA restore would strand the unit)",
            crate::log_tags::EVO_NET,
            ifname
        );
        steps.push(
            "shared iface: hotspot left up (no LAN carrier); STA cannot share the radio with AP here"
                .into(),
        );
        return;
    }

    tracing::info!(
        "{} shared radio: releasing hotspot {:?} on {} then restoring {} (last connection up wins on single phy)",
        crate::log_tags::EVO_NET,
        hotspot_con,
        ifname,
        NM_CON_WIFI_STA
    );

    connection_down_lossy(hotspot_con).await;

    let mut restored = false;
    for attempt in 1..=HOTSPOT_BRINGUP_ATTEMPTS {
        match nmcli_spawn_output(&["connection", "up", NM_CON_WIFI_STA]).await {
            Ok(out) if out.status.success() => {
                restored = true;
                steps.push(if attempt > 1 {
                    format!(
                        "shared iface: restored {} on {} after hotspot (attempt {})",
                        NM_CON_WIFI_STA, ifname, attempt
                    )
                } else {
                    format!(
                        "shared iface: restored {} on {} — single radio cannot keep STA+AP; STA preferred when LAN has carrier",
                        NM_CON_WIFI_STA, ifname
                    )
                });
                break;
            }
            Ok(out) => {
                let code = out.status.code().unwrap_or(-1);
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::warn!(
                    "{} restore STA after hotspot attempt {}/{} (exit {}): {}",
                    crate::log_tags::EVO_NET,
                    attempt,
                    HOTSPOT_BRINGUP_ATTEMPTS,
                    code,
                    stderr.trim()
                );
            }
            Err(e) => {
                tracing::warn!(
                    "{} restore STA after hotspot attempt {} spawn: {}",
                    crate::log_tags::EVO_NET,
                    attempt,
                    e
                );
            }
        }
        if attempt < HOTSPOT_BRINGUP_ATTEMPTS {
            tokio::time::sleep(Duration::from_millis(HOTSPOT_BRINGUP_DELAY_MS)).await;
        }
    }

    if !restored {
        steps.push(format!(
            "warning: could not restore {} on {} after releasing hotspot",
            NM_CON_WIFI_STA, ifname
        ));
    }
}

/// Build `nmcli connection modify` args for IPv4 (ethernet or wifi STA).
fn nm_ipv4_modify_args(
    mode: &Ipv4Mode,
    address: &str,
    gateway: &str,
    dns: &[String],
) -> Vec<String> {
    match mode {
        Ipv4Mode::Dhcp => vec!["ipv4.method".into(), "auto".into()],
        Ipv4Mode::Static => {
            let dns_s = dns
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            vec![
                "ipv4.method".into(),
                "manual".into(),
                "ipv4.addresses".into(),
                address.trim().to_string(),
                "ipv4.gateway".into(),
                gateway.trim().to_string(),
                "ipv4.dns".into(),
                dns_s,
            ]
        }
    }
}

async fn nm_modify_connection(name: &str, prop_vals: &[String]) -> Result<()> {
    if prop_vals.is_empty() {
        return Ok(());
    }
    let mut args: Vec<String> = vec!["connection".into(), "modify".into(), name.into()];
    args.extend(prop_vals.iter().cloned());
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    nmcli_output_lossy(&args_ref).await?;
    Ok(())
}

async fn ensure_ethernet(steps: &mut Vec<String>, eth: &EthernetIntent) -> Result<()> {
    tracing::debug!(
        "{} nm ensure_ethernet enabled={} device={}",
        crate::log_tags::EVO_NET,
        eth.enabled,
        if eth.device.trim().is_empty() {
            "(auto)"
        } else {
            eth.device.trim()
        }
    );
    if !eth.enabled {
        steps.push("skipped ethernet (disabled in intent)".into());
        return Ok(());
    }

    let table = nm_device_table().await?;
    let ifname = if eth.device.trim().is_empty() {
        match first_ethernet_device(&table) {
            Some(i) => i,
            None => {
                steps.push(
                    "warning: ethernet enabled in intent but no ethernet device found; skipping"
                        .into(),
                );
                return Ok(());
            }
        }
    } else {
        eth.device.trim().to_string()
    };

    if matches!(eth.ipv4_mode, Ipv4Mode::Static) && eth.ipv4_address.trim().is_empty() {
        anyhow::bail!("ethernet static IPv4 requires ipv4_address (CIDR)");
    }

    let props = nm_ipv4_modify_args(
        &eth.ipv4_mode,
        eth.ipv4_address.trim(),
        eth.ipv4_gateway.trim(),
        &eth.ipv4_dns,
    );

    if nm_connection_exists(NM_CON_ETHERNET).await {
        nm_modify_connection(NM_CON_ETHERNET, &props).await?;
        steps.push(format!(
            "modified connection {} (ethernet {})",
            NM_CON_ETHERNET, ifname
        ));
    } else {
        let mut args: Vec<String> = vec![
            "connection".into(),
            "add".into(),
            "type".into(),
            "ethernet".into(),
            "con-name".into(),
            NM_CON_ETHERNET.into(),
            "ifname".into(),
            ifname.clone(),
        ];
        args.extend(props);
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        nmcli_output_lossy(&args_ref).await?;
        steps.push(format!(
            "added connection {} (ethernet {})",
            NM_CON_ETHERNET, ifname
        ));
    }

    nmcli_output_lossy(&["connection", "up", NM_CON_ETHERNET])
        .await
        .context("nmcli connection up ethernet")?;
    steps.push(format!("brought up {}", NM_CON_ETHERNET));
    Ok(())
}

fn hotspot_connection_name(fb: &FallbackIntent) -> &str {
    let s = fb.hotspot_connection_name.trim();
    if s.is_empty() {
        NM_CON_HOTSPOT
    } else {
        s
    }
}

/// Bring connection down if active; ignore **exit 10** (not active / already down) without `WARN` spam.
async fn connection_down_lossy(name: &str) {
    if name.trim().is_empty() {
        return;
    }
    match nmcli_spawn_output(&["connection", "down", name]).await {
        Ok(out) if out.status.success() => {}
        Ok(out) => {
            let code = out.status.code().unwrap_or(-1);
            if code != 10 {
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::debug!(
                    "{} connection down {} (exit {}): {}",
                    crate::log_tags::EVO_NET,
                    name,
                    code,
                    stderr.trim()
                );
            }
        }
        Err(e) => {
            tracing::debug!(
                "{} connection down {} spawn: {}",
                crate::log_tags::EVO_NET,
                name,
                e
            );
        }
    }
}

/// When **`sta_connection_up_nonfatal`** is true (**STA and hotspot share one iface** and **Enable
/// Hotspot** is on), STA `connection up` failure does not abort apply so we can still bring up the AP.
async fn ensure_wifi_sta(
    steps: &mut Vec<String>,
    wifi_ifname: &str,
    wifi: &WifiIntent,
    sta_psk: Option<&str>,
    sta_connection_up_nonfatal: bool,
) -> Result<()> {
    let ifname = wifi_ifname.trim();
    tracing::debug!(
        "{} nm ensure_wifi_sta ifname={} sta_open={} sta_connection_up_nonfatal={} ssid_len={}",
        crate::log_tags::EVO_NET,
        ifname,
        wifi.sta_open,
        sta_connection_up_nonfatal,
        wifi.sta_ssid.trim().chars().count()
    );
    if ifname.is_empty() {
        anyhow::bail!("wifi interface name is empty");
    }
    if wifi.sta_ssid.trim().is_empty() {
        anyhow::bail!("wifi.sta_ssid is required for STA mode");
    }
    if matches!(wifi.sta_ipv4_mode, Ipv4Mode::Static) && wifi.sta_ipv4_address.trim().is_empty() {
        anyhow::bail!("wifi static IPv4 requires sta_ipv4_address (CIDR)");
    }

    let ssid = wifi.sta_ssid.trim();
    let psk = sta_psk.map(|s| s.trim()).filter(|s| !s.is_empty());

    let ipv4_props = nm_ipv4_modify_args(
        &wifi.sta_ipv4_mode,
        wifi.sta_ipv4_address.trim(),
        wifi.sta_ipv4_gateway.trim(),
        &wifi.sta_ipv4_dns,
    );

    if nm_connection_exists(NM_CON_WIFI_STA).await {
        // Some NetworkManager builds do not reliably flip **agent-owned** secrets to **system-stored**
        // when `psk-flags` and `psk` arrive in one `modify` line. Force `psk-flags 0` in its **own**
        // `nmcli` invocation before we write SSID/security/PSK — otherwise `connection up` fails with
        //   "password for '802-11-wireless-security.psk' not given in 'passwd-file'".
        // Documented: **docs/NETWORK_NM.md** (section *STA WPA-PSK: psk-flags + connection down*).
        if !wifi.sta_open && psk.is_some() {
            nmcli_output_lossy(&[
                "connection",
                "modify",
                NM_CON_WIFI_STA,
                "wifi-sec.psk-flags",
                "0",
            ])
            .await?;
            steps.push(format!(
                "{}: wifi-sec.psk-flags 0 (preflight, own nmcli call)",
                NM_CON_WIFI_STA
            ));
        }
        let mut seq = vec![
            "connection".into(),
            "modify".into(),
            NM_CON_WIFI_STA.into(),
            "connection.interface-name".into(),
            ifname.to_string(),
            "802-11-wireless.ssid".into(),
            ssid.to_string(),
        ];
        if wifi.sta_open {
            seq.extend(
                ["wifi-sec.key-mgmt".into(), "none".into()]
                    .iter()
                    .cloned(),
            );
        } else if let Some(p) = psk {
            // Order matters: `nmcli` applies properties **left-to-right**. Set
            // `wifi-sec.psk-flags 0` (system-stored) FIRST, then `wifi-sec.psk <val>`; otherwise
            // if the existing profile has `psk-flags=1` (agent-owned) the psk write is treated as
            // agent-owned and **never persisted** to the keyfile → next `connection up` fails with
            //   "password for '802-11-wireless-security.psk' not given in 'passwd-file'".
            seq.extend(
                [
                    "wifi-sec.key-mgmt".into(),
                    "wpa-psk".into(),
                    "wifi-sec.psk-flags".into(),
                    "0".into(),
                    "wifi-sec.psk".into(),
                    p.to_string(),
                ]
                .iter()
                .cloned(),
            );
        } else {
            seq.extend(
                ["wifi-sec.key-mgmt".into(), "none".into()]
                    .iter()
                    .cloned(),
            );
            steps.push(
                "warning: no wifi-sta.psk and sta_open=false; using open key-mgmt (may fail)"
                    .into(),
            );
        }
        seq.extend(ipv4_props);
        let args_ref: Vec<&str> = seq.iter().map(|s| s.as_str()).collect();
        nmcli_output_lossy(&args_ref).await?;
        steps.push(format!("modified {}", NM_CON_WIFI_STA));
    } else {
        let mut args: Vec<String> = vec![
            "connection".into(),
            "add".into(),
            "type".into(),
            "wifi".into(),
            "con-name".into(),
            NM_CON_WIFI_STA.into(),
            "ifname".into(),
            ifname.to_string(),
            "ssid".into(),
            ssid.to_string(),
        ];
        if wifi.sta_open {
            args.extend(["wifi-sec.key-mgmt".into(), "none".into()]);
        } else if let Some(p) = psk {
            args.extend([
                "wifi-sec.key-mgmt".into(),
                "wpa-psk".into(),
                "wifi-sec.psk-flags".into(),
                "0".into(),
                "wifi-sec.psk".into(),
                p.to_string(),
            ]);
        } else {
            args.extend(["wifi-sec.key-mgmt".into(), "none".into()]);
            steps.push(
                "warning: no wifi-sta.psk and sta_open=false; using open key-mgmt (may fail)"
                    .into(),
            );
        }
        args.extend(ipv4_props);
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        nmcli_output_lossy(&args_ref).await?;
        steps.push(format!("added {}", NM_CON_WIFI_STA));
    }

    if sta_connection_up_nonfatal {
        match nmcli_spawn_output(&["connection", "up", NM_CON_WIFI_STA]).await {
            Ok(out) if out.status.success() => {
                steps.push(format!("brought up {}", NM_CON_WIFI_STA));
            }
            Ok(out) => {
                let code = out.status.code().unwrap_or(-1);
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::warn!(
                    "{} nmcli connection up {} (exit {}, nonfatal): {}",
                    crate::log_tags::EVO_NET,
                    NM_CON_WIFI_STA,
                    code,
                    stderr.trim()
                );
                steps.push(format!(
                    "warning: nmcli connection up {} failed (exit {}): {}",
                    NM_CON_WIFI_STA,
                    code,
                    stderr.trim()
                ));
            }
            Err(e) => {
                tracing::warn!(
                    "{} nmcli connection up {} spawn (nonfatal): {}",
                    crate::log_tags::EVO_NET,
                    NM_CON_WIFI_STA,
                    e
                );
                steps.push(format!(
                    "warning: nmcli connection up {} spawn: {}",
                    NM_CON_WIFI_STA,
                    e
                ));
            }
        }
    } else {
        nmcli_output_lossy(&["connection", "up", NM_CON_WIFI_STA])
            .await
            .context("nmcli connection up wifi STA")?;
        steps.push(format!("brought up {}", NM_CON_WIFI_STA));
    }
    Ok(())
}

/// NM requires **`802-11-wireless.band`** when setting **`802-11-wireless.channel`**.
/// Wi‑Fi 4/5/6/7 are handled by the kernel + wpa_supplicant; Evo only sets band/channel for the AP profile.
fn normalize_nm_ap_band(raw: &str) -> Option<&'static str> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    if t.eq_ignore_ascii_case("bg") {
        return Some("bg");
    }
    if t.eq_ignore_ascii_case("a") {
        return Some("a");
    }
    if t.eq_ignore_ascii_case("6ghz") {
        return Some("6GHz");
    }
    None
}

fn push_nm_ap_channel(seq: &mut Vec<String>, wifi: &WifiIntent) {
    let ch = wifi.ap_channel;
    if ch == 0 {
        return;
    }
    let band = if let Some(b) = normalize_nm_ap_band(&wifi.ap_band) {
        b.to_string()
    } else if !wifi.ap_band.trim().is_empty() {
        tracing::warn!(
            "{} wifi.ap_band {:?} invalid; use bg, a, or 6GHz — skipping channel",
            crate::log_tags::EVO_NET,
            wifi.ap_band.trim()
        );
        return;
    } else {
        match ch {
            1..=14 => "bg".to_string(),
            36..=177 => "a".to_string(),
            _ => {
                tracing::debug!(
                    "{} ap_channel {} has no inferred band; set wifi.ap_band (bg|a|6GHz) in intent",
                    crate::log_tags::EVO_NET,
                    ch
                );
                return;
            }
        }
    };
    seq.push("802-11-wireless.band".into());
    seq.push(band);
    seq.push("802-11-wireless.channel".into());
    seq.push(ch.to_string());
}

async fn ensure_wifi_ap(
    steps: &mut Vec<String>,
    wifi_ifname: &str,
    wifi: &WifiIntent,
    ap_psk: Option<&str>,
    con_name: &str,
) -> Result<()> {
    let ifname = wifi_ifname.trim();
    if ifname.is_empty() {
        anyhow::bail!("wifi interface name is empty");
    }
    let ssid = wifi.ap_ssid.trim();
    if ssid.is_empty() {
        anyhow::bail!("wifi.ap_ssid is empty");
    }
    tracing::debug!(
        "{} nm ensure_wifi_ap ifname={} con_name={} ssid_len={} ap_psk_configured={}",
        crate::log_tags::EVO_NET,
        ifname,
        con_name,
        ssid.chars().count(),
        ap_psk.map(|s| !s.trim().is_empty()).unwrap_or(false)
    );
    let psk = ap_psk.map(|s| s.trim()).filter(|s| !s.is_empty());

    if nm_connection_exists(con_name).await {
        let mut seq = vec![
            "connection".into(),
            "modify".into(),
            con_name.into(),
            "connection.interface-name".into(),
            ifname.to_string(),
            "802-11-wireless.mode".into(),
            "ap".into(),
            "802-11-wireless.ssid".into(),
            ssid.to_string(),
        ];
        push_nm_ap_channel(&mut seq, wifi);
        seq.extend([
            "ipv4.method".into(),
            "shared".into(),
            "ipv6.method".into(),
            "ignore".into(),
        ]);
        if let Some(p) = psk {
            // psk-flags BEFORE psk — see STA note above.
            seq.extend([
                "wifi-sec.key-mgmt".into(),
                "wpa-psk".into(),
                "wifi-sec.psk-flags".into(),
                "0".into(),
                "wifi-sec.psk".into(),
                p.to_string(),
            ]);
        } else {
            push_nm_ap_remove_wireless_security(&mut seq);
            steps.push("warning: AP with no passphrase (open hotspot; no 802.11 wireless security)".into());
        }
        let args_ref: Vec<&str> = seq.iter().map(|s| s.as_str()).collect();
        nmcli_output_lossy(&args_ref).await?;
        steps.push(format!("modified AP profile {}", con_name));
    } else {
        let mut args: Vec<String> = vec![
            "connection".into(),
            "add".into(),
            "type".into(),
            "wifi".into(),
            "con-name".into(),
            con_name.into(),
            "ifname".into(),
            ifname.to_string(),
            "autoconnect".into(),
            "no".into(),
            "wifi.mode".into(),
            "ap".into(),
            "ssid".into(),
            ssid.to_string(),
        ];
        push_nm_ap_channel(&mut args, wifi);
        args.extend([
            "ipv4.method".into(),
            "shared".into(),
            "ipv6.method".into(),
            "ignore".into(),
        ]);
        if let Some(p) = psk {
            args.extend([
                "wifi-sec.key-mgmt".into(),
                "wpa-psk".into(),
                "wifi-sec.psk-flags".into(),
                "0".into(),
                "wifi-sec.psk".into(),
                p.to_string(),
            ]);
        }
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        nmcli_output_lossy(&args_ref).await?;
        steps.push(format!("added AP profile {}", con_name));
    }

    if !connection_up_hotspot_with_retries(con_name, steps).await {
        anyhow::bail!("nmcli connection up wifi AP failed after retries");
    }
    Ok(())
}

/// Ensure a dormant hotspot profile exists for [`FallbackIntent`] (autoconnect off). Does not
/// activate it unless we are already in AP role (caller handles).
async fn ensure_hotspot_profile(
    steps: &mut Vec<String>,
    wifi_ifname: &str,
    wifi: &WifiIntent,
    ap_psk: Option<&str>,
    fb: &FallbackIntent,
) -> Result<()> {
    if !fb.hotspot_enabled {
        tracing::debug!(
            "{} nm ensure_hotspot_profile skip (fallback.hotspot_enabled=false)",
            crate::log_tags::EVO_NET
        );
        return Ok(());
    }
    let name = hotspot_connection_name(fb);
    let ifname = wifi_ifname.trim();
    if ifname.is_empty() {
        return Ok(());
    }
    let ssid = wifi.ap_ssid.trim();
    if ssid.is_empty() {
        steps.push(
            "warning: fallback hotspot enabled but wifi.ap_ssid empty; skipping profile"
                .into(),
        );
        return Ok(());
    }
    let psk = ap_psk.map(|s| s.trim()).filter(|s| !s.is_empty());

    if nm_connection_exists(name).await {
        let mut seq = vec![
            "connection".into(),
            "modify".into(),
            name.into(),
            "connection.interface-name".into(),
            ifname.to_string(),
            "connection.autoconnect".into(),
            "no".into(),
            "802-11-wireless.mode".into(),
            "ap".into(),
            "802-11-wireless.ssid".into(),
            ssid.to_string(),
        ];
        push_nm_ap_channel(&mut seq, wifi);
        seq.extend([
            "ipv4.method".into(),
            "shared".into(),
            "ipv6.method".into(),
            "ignore".into(),
        ]);
        if let Some(p) = psk {
            seq.extend([
                "wifi-sec.key-mgmt".into(),
                "wpa-psk".into(),
                "wifi-sec.psk-flags".into(),
                "0".into(),
                "wifi-sec.psk".into(),
                p.to_string(),
            ]);
        } else {
            push_nm_ap_remove_wireless_security(&mut seq);
        }
        let args_ref: Vec<&str> = seq.iter().map(|s| s.as_str()).collect();
        nmcli_output_lossy(&args_ref).await?;
        steps.push(format!("updated fallback hotspot profile {}", name));
    } else {
        let mut args: Vec<String> = vec![
            "connection".into(),
            "add".into(),
            "type".into(),
            "wifi".into(),
            "con-name".into(),
            name.into(),
            "ifname".into(),
            ifname.to_string(),
            "autoconnect".into(),
            "no".into(),
            "wifi.mode".into(),
            "ap".into(),
            "ssid".into(),
            ssid.to_string(),
        ];
        push_nm_ap_channel(&mut args, wifi);
        args.extend([
            "ipv4.method".into(),
            "shared".into(),
            "ipv6.method".into(),
            "ignore".into(),
        ]);
        if let Some(p) = psk {
            args.extend([
                "wifi-sec.key-mgmt".into(),
                "wpa-psk".into(),
                "wifi-sec.psk-flags".into(),
                "0".into(),
                "wifi-sec.psk".into(),
                p.to_string(),
            ]);
        }
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        nmcli_output_lossy(&args_ref).await?;
        steps.push(format!("created fallback hotspot profile {}", name));
    }
    Ok(())
}

static NM_APPLY_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

/// Serialize concurrent `nmcli` applies (REST / future watchdog).
pub async fn apply_network_intent_exclusive(intent: &NetworkIntent, config: &Config) -> NetworkApplyReport {
    let lock = NM_APPLY_LOCK.get_or_init(|| tokio::sync::Mutex::new(()));
    let _guard = lock.lock().await;
    tracing::debug!(
        "{} apply_network_intent_exclusive lock acquired (serializing nmcli apply)",
        crate::log_tags::EVO_NET
    );
    apply_network_intent(intent, config).await
}

/// Apply [`NetworkIntent`] using `nmcli` (ethernet + Wi‑Fi). Pass PSKs from sidecar files when present.
pub async fn apply_network_intent(intent: &NetworkIntent, config: &Config) -> NetworkApplyReport {
    let mut steps: Vec<String> = Vec::new();
    let sta_psk = crate::network_config::read_secret_file(&crate::network_config::wifi_sta_psk_path());
    let ap_psk = crate::network_config::read_secret_file(&crate::network_config::wifi_ap_psk_path());
    let sta_psk_ref = sta_psk.as_deref();
    let ap_psk_ref = ap_psk.as_deref();
    let sta_ifname = crate::network_config::effective_wifi_ifname(&intent.wifi, Some(config));
    let hotspot_ifname_intent = crate::network_config::effective_hotspot_ifname(intent, Some(config));
    // Resolve **effective AP interface** per `NETWORK_NM.md`:
    //   * explicit `fallback.hotspot_ifname` (or config `effective_hotspot_ifname`) wins
    //   * else: probe STA's phy — if it supports **concurrent managed+AP**, auto-pick a virtual
    //     `ap0` (env override `VOLUMIO_EVO_AP_IFNAME`); else fall back to the STA ifname (single-mode)
    let intent_hotspot_if_is_explicit =
        !intent.fallback.hotspot_ifname.trim().is_empty() && hotspot_ifname_intent != sta_ifname;
    let phy_supports_concurrent =
        crate::wifi_phy::sta_phy_supports_concurrent_sta_ap(&sta_ifname).await;
    let resolved_ap_ifname = if intent_hotspot_if_is_explicit {
        hotspot_ifname_intent.clone()
    } else if phy_supports_concurrent {
        std::env::var("VOLUMIO_EVO_AP_IFNAME")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "ap0".to_string())
    } else {
        sta_ifname.clone()
    };
    debug_log_network_intent_snapshot(
        intent,
        sta_ifname.as_str(),
        resolved_ap_ifname.as_str(),
        sta_psk.is_some(),
        ap_psk.is_some(),
    );
    tracing::debug!(
        "{} phy_capability sta_if={} phy_supports_concurrent_managed_ap={} resolved_ap_if={} explicit_intent_hotspot_if={}",
        crate::log_tags::EVO_NET,
        sta_ifname,
        phy_supports_concurrent,
        resolved_ap_ifname,
        intent_hotspot_if_is_explicit
    );
    if sta_ifname != resolved_ap_ifname {
        steps.push(format!(
            "STA iface {} ≠ hotspot iface {} ({})",
            sta_ifname,
            resolved_ap_ifname,
            if intent_hotspot_if_is_explicit {
                "split-radio / STA-only USB per intent"
            } else if phy_supports_concurrent {
                "auto: concurrent STA+AP vif on same phy"
            } else {
                "single-mode fallback"
            }
        ));
    }

    let res = async {
        ensure_ethernet(&mut steps, &intent.ethernet).await?;
        tracing::debug!(
            "{} nm phase: ethernet done wifi.role={:?}",
            crate::log_tags::EVO_NET,
            intent.wifi.role
        );

        let hs = hotspot_connection_name(&intent.fallback);
        match intent.wifi.role {
            WifiRole::Disabled => {
                connection_down_lossy(NM_CON_WIFI_STA).await;
                connection_down_lossy(hs).await;
                if sta_ifname != resolved_ap_ifname {
                    if let Err(e) =
                        crate::wifi_phy::ensure_ap_vif_absent(&resolved_ap_ifname).await
                    {
                        tracing::debug!(
                            "{} ensure_ap_vif_absent({}) nonfatal: {}",
                            crate::log_tags::EVO_NET,
                            resolved_ap_ifname,
                            e
                        );
                    }
                }
                steps.push("wifi role disabled; brought down STA and hotspot (best effort)".into());
            }
            WifiRole::Sta => {
                let same_iface = sta_ifname == resolved_ap_ifname;
                let concurrent_vif =
                    !same_iface && !intent_hotspot_if_is_explicit && phy_supports_concurrent;
                // Always bring the hotspot profile **down** before STA association/DHCP: leaving
                // the AP active on a brcmfmac-class PHY can pin the radio at the AP channel and
                // cause `IP configuration could not be reserved (… timeout …)` on the STA profile.
                connection_down_lossy(hs).await;
                // Concurrent-vif mode: tear the `ap0` vif down entirely so the phy is free for STA
                // scan/associate. We recreate it **after** STA is up (canonical Ezurio / Pi 5 recipe).
                if concurrent_vif {
                    if let Err(e) =
                        crate::wifi_phy::ensure_ap_vif_absent(&resolved_ap_ifname).await
                    {
                        tracing::debug!(
                            "{} pre-STA ensure_ap_vif_absent({}) nonfatal: {}",
                            crate::log_tags::EVO_NET,
                            resolved_ap_ifname,
                            e
                        );
                    } else {
                        steps.push(format!(
                            "pre-STA: removed AP vif {} to free phy for STA association",
                            resolved_ap_ifname
                        ));
                    }
                }
                // Tear down any in‑progress STA activation before rewriting credentials; avoids NM
                // holding agent-owned / stale secret state while we push a system-stored PSK.
                connection_down_lossy(NM_CON_WIFI_STA).await;
                steps.push(
                    "pre-STA: nmcli connection down STA profile (best effort before modify/up)"
                        .into(),
                );
                // Product rule from `NETWORK_NM.md` §Automatic hotspot: when **Enable Hotspot** is on,
                // a failed STA `connection up` must NOT abort apply — the stack still brings up the AP
                // so the device remains reachable for provisioning. This is true for single-mode shared
                // iface, split-radio, and virtual-vif concurrent mode alike.
                let sta_up_nonfatal = intent.fallback.hotspot_enabled;
                ensure_wifi_sta(
                    &mut steps,
                    &sta_ifname,
                    &intent.wifi,
                    sta_psk_ref,
                    sta_up_nonfatal,
                )
                .await?;
            }
            WifiRole::Ap => {
                connection_down_lossy(NM_CON_WIFI_STA).await;
                ensure_wifi_ap(
                    &mut steps,
                    &resolved_ap_ifname,
                    &intent.wifi,
                    ap_psk_ref,
                    hs,
                )
                .await?;
            }
        }
        tracing::debug!(
            "{} nm phase: wifi role branch done (role={:?})",
            crate::log_tags::EVO_NET,
            intent.wifi.role
        );

        if matches!(intent.wifi.role, WifiRole::Sta) {
            // Single-PHY concurrent STA+AP: create the `__ap` vif on the STA's phy so NM can keep
            // both profiles active on distinct ifnames (canonical `iw dev <sta> interface add <ap>
            // type __ap` — see NETWORK_NM.md). No-op on split-radio or true single-mode.
            if sta_ifname != resolved_ap_ifname && intent.fallback.hotspot_enabled {
                if !intent_hotspot_if_is_explicit {
                    if let Err(e) =
                        crate::wifi_phy::ensure_ap_vif_present(&sta_ifname, &resolved_ap_ifname)
                            .await
                    {
                        tracing::warn!(
                            "{} ensure_ap_vif_present({} on phy of {}): {}",
                            crate::log_tags::EVO_NET,
                            resolved_ap_ifname,
                            sta_ifname,
                            e
                        );
                        steps.push(format!(
                            "warning: could not add AP vif {}: {}",
                            resolved_ap_ifname, e
                        ));
                    } else {
                        steps.push(format!(
                            "created AP vif {} on phy of {} (type __ap)",
                            resolved_ap_ifname, sta_ifname
                        ));
                    }
                }
            }

            // When AP shares a phy with STA (concurrent mode), the AP **must** follow the STA
            // channel/band — brcmfmac, Ezurio and kernel `valid interface combinations` all say so.
            // Derive overrides from `iw dev <sta> link`; fall back to user intent if not associated.
            let mut wifi_for_ap = intent.wifi.clone();
            if sta_ifname != resolved_ap_ifname && !intent_hotspot_if_is_explicit {
                let link = crate::wifi_phy::sta_link_info(&sta_ifname).await;
                if link.connected {
                    if let (Some(ch), Some(band)) = (link.channel, link.band.as_deref()) {
                        if wifi_for_ap.ap_channel != ch
                            || !wifi_for_ap.ap_band.eq_ignore_ascii_case(band)
                        {
                            steps.push(format!(
                                "AP follows STA: band={} channel={} (was band={:?} channel={})",
                                band, ch, wifi_for_ap.ap_band, wifi_for_ap.ap_channel
                            ));
                        }
                        wifi_for_ap.ap_channel = ch;
                        wifi_for_ap.ap_band = band.to_string();
                    }
                } else {
                    tracing::debug!(
                        "{} AP channel follow-STA skipped (STA not associated yet on {})",
                        crate::log_tags::EVO_NET,
                        sta_ifname
                    );
                }
            }

            ensure_hotspot_profile(
                &mut steps,
                &resolved_ap_ifname,
                &wifi_for_ap,
                ap_psk_ref,
                &intent.fallback,
            )
            .await?;

            let hs_name = hotspot_connection_name(&intent.fallback);
            if intent.fallback.hotspot_enabled && !hs_name.trim().is_empty() {
                let same_iface = sta_ifname == resolved_ap_ifname;
                let ok = connection_up_hotspot_with_retries(hs_name, &mut steps).await;
                let recovered = if !ok {
                    try_critical_open_hotspot_recovery(
                        &intent.ethernet,
                        intent.fallback.hotspot_enabled,
                        hs_name,
                        &mut steps,
                    )
                    .await
                } else {
                    false
                };
                if same_iface {
                    // True single-mode (no vif). Settle + restore STA per NETWORK_NM.md.
                    restore_sta_after_hotspot_on_shared_radio(
                        &intent.ethernet,
                        sta_ifname.trim(),
                        hs_name,
                        &mut steps,
                    )
                    .await;
                } else {
                    steps.push(format!(
                        "intent: hotspot on {}, STA on {}",
                        resolved_ap_ifname, sta_ifname
                    ));
                }
                if !ok && !recovered {
                    steps.push(
                        "warning: hotspot did not activate after retries (and critical open recovery if applicable)"
                            .into(),
                    );
                }
            }
        }
        tracing::debug!(
            "{} nm phase: intent apply inner completed",
            crate::log_tags::EVO_NET
        );
        Ok::<(), anyhow::Error>(())
    }
    .await;

    let report = match res {
        Ok(()) => NetworkApplyReport { ok: true, steps },
        Err(e) => {
            tracing::debug!(
                "{} apply_network_intent inner error before report: {}",
                crate::log_tags::EVO_NET,
                e
            );
            steps.push(format!("error: {}", e));
            NetworkApplyReport {
                ok: false,
                steps,
            }
        }
    };
    tracing::debug!(
        "{} apply_network_intent finished ok={} steps={}",
        crate::log_tags::EVO_NET,
        report.ok,
        report.steps.len()
    );
    report
}
