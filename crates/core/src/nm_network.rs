//! NetworkManager integration via **`nmcli`** (non-interactive).
//! See **[docs/NETWORK_NM.md](../../../docs/NETWORK_NM.md)** for architecture and rollout phases.
//!
//! Phase 1: device listing, Wi‑Fi scan → stock UI shape `{ available: [...] }`, REST diagnostic.
//! Phase 2: apply persisted [`crate::network_config::NetworkIntent`] (DHCP/static, Wi‑Fi STA/AP, hotspot profile).

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::OnceLock;
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

/// Effective Wi‑Fi interface for NM scans and diagnostics.
///
/// 1. Non-empty [`Config::wifi_iface`] or **`VOLUMIO_EVO_WIFI_IFACE`** (applied at load) wins.
/// 2. Otherwise, pick the first `wifi` device from **`nmcli -t device`**, **preferring** one whose
///    state does not look like **`unavailable`** when several radios exist (e.g. USB `wlan1` vs dead SoC `wlan0`).
/// 3. Fallback: [`DEFAULT_WIFI_IFACE`] (`wlan0`).
pub async fn resolve_effective_wifi_iface(config: &Config) -> String {
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

async fn nmcli_output_lossy(args: &[&str]) -> Result<String> {
    nmcli_output(args).await
}

async fn nm_connection_exists(name: &str) -> bool {
    let Ok(out) = nmcli_spawn_output(&["connection", "show", name]).await else {
        return false;
    };
    out.status.success()
}

fn first_ethernet_device(devices: &[NmDeviceRow]) -> Option<String> {
    devices
        .iter()
        .find(|d| d.kind.eq_ignore_ascii_case("ethernet") && d.device != "lo")
        .map(|d| d.device.clone())
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
    let table = nm_device_table().await?;
    let ifname = if eth.device.trim().is_empty() {
        first_ethernet_device(&table).ok_or_else(|| anyhow!("no ethernet device found"))?
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

async fn ensure_wifi_sta(
    steps: &mut Vec<String>,
    wifi_ifname: &str,
    wifi: &WifiIntent,
    sta_psk: Option<&str>,
) -> Result<()> {
    let ifname = wifi_ifname.trim();
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
            seq.extend(
                [
                    "wifi-sec.key-mgmt".into(),
                    "wpa-psk".into(),
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

    nmcli_output_lossy(&["connection", "up", NM_CON_WIFI_STA])
        .await
        .context("nmcli connection up wifi STA")?;
    steps.push(format!("brought up {}", NM_CON_WIFI_STA));
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
        seq.extend(["ipv4.method".into(), "shared".into()]);
        if let Some(p) = psk {
            seq.extend([
                "wifi-sec.key-mgmt".into(),
                "wpa-psk".into(),
                "wifi-sec.psk".into(),
                p.to_string(),
            ]);
        } else {
            seq.extend(["wifi-sec.key-mgmt".into(), "none".into()]);
            steps.push("warning: AP with no passphrase (open hotspot)".into());
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
        args.extend(["ipv4.method".into(), "shared".into()]);
        if let Some(p) = psk {
            args.extend([
                "wifi-sec.key-mgmt".into(),
                "wpa-psk".into(),
                "wifi-sec.psk".into(),
                p.to_string(),
            ]);
        } else {
            args.extend(["wifi-sec.key-mgmt".into(), "none".into()]);
        }
        let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        nmcli_output_lossy(&args_ref).await?;
        steps.push(format!("added AP profile {}", con_name));
    }

    nmcli_output_lossy(&["connection", "up", con_name])
        .await
        .context("nmcli connection up wifi AP")?;
    steps.push(format!("brought up {}", con_name));
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
        seq.extend(["ipv4.method".into(), "shared".into()]);
        if let Some(p) = psk {
            seq.extend([
                "wifi-sec.key-mgmt".into(),
                "wpa-psk".into(),
                "wifi-sec.psk".into(),
                p.to_string(),
            ]);
        } else {
            seq.extend(["wifi-sec.key-mgmt".into(), "none".into()]);
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
        args.extend(["ipv4.method".into(), "shared".into()]);
        if let Some(p) = psk {
            args.extend([
                "wifi-sec.key-mgmt".into(),
                "wpa-psk".into(),
                "wifi-sec.psk".into(),
                p.to_string(),
            ]);
        } else {
            args.extend(["wifi-sec.key-mgmt".into(), "none".into()]);
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
    let hotspot_ifname = crate::network_config::effective_hotspot_ifname(intent, Some(config));
    if sta_ifname != hotspot_ifname {
        steps.push(format!(
            "STA iface {} ≠ hotspot iface {} (split-radio / STA-only USB)",
            sta_ifname, hotspot_ifname
        ));
    }

    let res = async {
        ensure_ethernet(&mut steps, &intent.ethernet).await?;

        let hs = hotspot_connection_name(&intent.fallback);
        match intent.wifi.role {
            WifiRole::Disabled => {
                connection_down_lossy(NM_CON_WIFI_STA).await;
                connection_down_lossy(hs).await;
                steps.push("wifi role disabled; brought down STA and hotspot (best effort)".into());
            }
            WifiRole::Sta => {
                connection_down_lossy(hs).await;
                ensure_wifi_sta(&mut steps, &sta_ifname, &intent.wifi, sta_psk_ref).await?;
            }
            WifiRole::Ap => {
                connection_down_lossy(NM_CON_WIFI_STA).await;
                ensure_wifi_ap(&mut steps, &hotspot_ifname, &intent.wifi, ap_psk_ref, hs).await?;
            }
        }

        if matches!(intent.wifi.role, WifiRole::Sta) {
            ensure_hotspot_profile(
                &mut steps,
                &hotspot_ifname,
                &intent.wifi,
                ap_psk_ref,
                &intent.fallback,
            )
            .await?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    match res {
        Ok(()) => NetworkApplyReport { ok: true, steps },
        Err(e) => {
            steps.push(format!("error: {}", e));
            NetworkApplyReport {
                ok: false,
                steps,
            }
        }
    }
}
