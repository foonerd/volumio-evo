//! `iw` helpers for **per-PHY capability detection** and **virtual AP interface** lifecycle.
//!
//! Background and product policy: [`docs/NETWORK_NM.md`](../../../docs/NETWORK_NM.md).
//!
//! Canonical single-PHY AP+STA recipe (see Ezurio FAQ, Nathan Lewis Pi 5 guide, RaspAP docs,
//! Raspberry Pi forums / kernel wiki cited in `NETWORK_NM.md`):
//!
//! 1. Detect a `valid interface combinations` rule containing **`managed`** and **`AP`** on one `phy`.
//! 2. Create a secondary AP vif on that phy: `iw dev <sta_if> interface add <ap_if> type __ap`.
//! 3. Bind STA NM profile to `sta_if` and AP NM profile to `ap_if` (`802-11-wireless.mode ap`,
//!    `ipv4.method shared`). NM then keeps **both** active with no single-device collision.

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
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

/// Path to **`iw`**. Must match **`/etc/sudoers.d/volumio-evo-iw`** (bootstrap) when Evo runs non-root.
/// Override: **`VOLUMIO_EVO_IW`** (same pattern as **`VOLUMIO_EVO_NMCLI`**, **`VOLUMIO_EVO_RFKILL`**).
pub fn iw_bin() -> String {
    std::env::var("VOLUMIO_EVO_IW")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/sbin/iw".to_string())
}

async fn iw_spawn_output(args: &[&str]) -> Result<std::process::Output> {
    let bin = iw_bin();
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

async fn iw_output(args: &[&str]) -> Result<String> {
    let out = iw_spawn_output(args).await?;
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        tracing::debug!(
            "{} iw {:?} failed (exit {}): {} (stdout: {})",
            crate::log_tags::EVO_NET,
            args,
            code,
            stderr.trim(),
            stdout.trim()
        );
        return Err(anyhow!(
            "iw {} failed (exit {}): {}\n{}",
            args.join(" "),
            code,
            stderr.trim(),
            stdout.trim()
        ));
    }
    Ok(stdout)
}

/// Per-`phy` capability summary.
#[derive(Debug, Clone, Default)]
pub struct PhyCapability {
    /// `phy` name (e.g. `phy0`); kept for diagnostics + `Debug` logs.
    #[allow(dead_code)]
    pub phy: String,
    /// True iff any **`valid interface combinations`** line allows **`managed`** and **`AP`** together.
    pub supports_managed_plus_ap: bool,
    /// Best `#channels <= N` observed across combinations (≥ 1 means AP must share STA channel).
    #[allow(dead_code)]
    pub max_channels: u32,
    /// `Supported interface modes` list (informational).
    #[allow(dead_code)]
    pub interface_modes: Vec<String>,
}

/// Parse `iw phy <phy> info` for interface combinations; return capability flags.
pub async fn phy_capability(phy: &str) -> Result<PhyCapability> {
    let phy_norm = phy.trim().trim_start_matches("phy");
    let phy_arg = format!("phy{}", phy_norm);
    let out = iw_output(&[&phy_arg, "info"]).await?;
    Ok(parse_phy_info(&phy_arg, &out))
}

fn parse_phy_info(phy_name: &str, raw: &str) -> PhyCapability {
    let mut cap = PhyCapability {
        phy: phy_name.to_string(),
        ..Default::default()
    };

    let mut in_modes = false;
    for line in raw.lines() {
        let lt = line.trim();
        if lt == "Supported interface modes:" {
            in_modes = true;
            continue;
        }
        if in_modes {
            if let Some(rest) = lt.strip_prefix('*') {
                cap.interface_modes.push(rest.trim().to_string());
            } else if !lt.is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
                in_modes = false;
            }
        }
    }

    let combos = extract_interface_combinations(raw);
    for combo in &combos {
        if combo_has_managed(combo) && combo_has_ap(combo) {
            cap.supports_managed_plus_ap = true;
        }
        if let Some(n) = parse_channel_count(combo) {
            if n > cap.max_channels {
                cap.max_channels = n;
            }
        }
    }

    cap
}

/// Extract each `* …` combination line under `valid interface combinations:`, joining wrapped lines.
fn extract_interface_combinations(raw: &str) -> Vec<String> {
    let mut combos: Vec<String> = Vec::new();
    let mut in_combos = false;
    let mut current = String::new();
    for line in raw.lines() {
        let lt = line.trim_end();
        if lt.trim() == "valid interface combinations:" {
            in_combos = true;
            continue;
        }
        if !in_combos {
            continue;
        }
        let is_indented = line.starts_with(' ') || line.starts_with('\t');
        if !is_indented && !lt.is_empty() {
            if !current.is_empty() {
                combos.push(current.clone());
                current.clear();
            }
            in_combos = false;
            continue;
        }
        let trimmed = lt.trim();
        if let Some(rest) = trimmed.strip_prefix("* ") {
            if !current.is_empty() {
                combos.push(current.clone());
                current.clear();
            }
            current.push_str(rest);
        } else if !trimmed.is_empty() {
            current.push(' ');
            current.push_str(trimmed);
        }
    }
    if !current.is_empty() {
        combos.push(current);
    }
    combos
}

fn combo_has_managed(combo: &str) -> bool {
    // Look for the `managed` mode token; groups are rendered like `{ managed }` or `{ IBSS, managed, AP }`.
    // Avoid matching substrings inside other words by requiring boundary chars around it.
    has_mode_token(combo, "managed")
}

fn combo_has_ap(combo: &str) -> bool {
    // `AP` as a bracketed mode (not `AP/VLAN` — that also implies AP so fine either way; not `P2P-GO`).
    has_mode_token(combo, "AP")
}

fn has_mode_token(combo: &str, token: &str) -> bool {
    let bytes = combo.as_bytes();
    let tb = token.as_bytes();
    let mut i = 0;
    while i + tb.len() <= bytes.len() {
        if &bytes[i..i + tb.len()] == tb {
            let before_ok = i == 0
                || matches!(
                    bytes[i - 1] as char,
                    ' ' | ',' | '{' | '(' | '/'
                );
            let after_idx = i + tb.len();
            let after_ok = after_idx >= bytes.len()
                || matches!(
                    bytes[after_idx] as char,
                    ' ' | ',' | '}' | ')' | '/'
                );
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn parse_channel_count(combo: &str) -> Option<u32> {
    let needle = "#channels <=";
    let idx = combo.find(needle)?;
    let rest = &combo[idx + needle.len()..];
    let s: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    s.parse().ok()
}

/// `iw dev` → mapping **`ifname → phy`** (e.g. `wlan0` → `phy0`).
pub async fn wifi_iface_to_phy_map() -> Result<HashMap<String, String>> {
    let out = iw_output(&["dev"]).await?;
    let mut map = HashMap::new();
    let mut cur_phy: Option<String> = None;
    for line in out.lines() {
        let lt = line.trim();
        if let Some(rest) = lt.strip_prefix("phy#") {
            cur_phy = Some(format!("phy{}", rest.trim()));
            continue;
        }
        if let Some(rest) = lt.strip_prefix("Interface ") {
            if let Some(phy) = cur_phy.as_ref() {
                map.insert(rest.trim().to_string(), phy.clone());
            }
        }
    }
    Ok(map)
}

pub async fn phy_for_ifname(ifname: &str) -> Result<Option<String>> {
    let map = wifi_iface_to_phy_map().await?;
    Ok(map.get(ifname.trim()).cloned())
}

/// Per-interface Wi-Fi device info from `iw dev` (ifname, phy, NL80211 `type`).
#[derive(Debug, Clone)]
pub struct WifiDev {
    pub ifname: String,
    pub phy: String,
    /// `iw dev … type` value: **`managed`** (STA), **`AP`**, **`IBSS`**, **`monitor`**, etc.
    pub iftype: String,
}

/// Parse full `iw dev` output; return one row per `Interface <name>` block with its phy and `type`.
pub async fn list_wifi_devices() -> Result<Vec<WifiDev>> {
    let out = iw_output(&["dev"]).await?;
    let mut rows: Vec<WifiDev> = Vec::new();
    let mut cur_phy: Option<String> = None;
    let mut cur_iface: Option<String> = None;
    let mut cur_type: Option<String> = None;
    for line in out.lines() {
        let lt = line.trim();
        if let Some(rest) = lt.strip_prefix("phy#") {
            flush_wifi_dev(&mut rows, &mut cur_phy, &mut cur_iface, &mut cur_type);
            cur_phy = Some(format!("phy{}", rest.trim()));
            continue;
        }
        if let Some(rest) = lt.strip_prefix("Interface ") {
            flush_wifi_dev_iface(&mut rows, &cur_phy, &mut cur_iface, &mut cur_type);
            cur_iface = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = lt.strip_prefix("type ") {
            cur_type = Some(rest.trim().to_string());
        }
    }
    flush_wifi_dev(&mut rows, &mut cur_phy, &mut cur_iface, &mut cur_type);
    Ok(rows)
}

fn flush_wifi_dev(
    rows: &mut Vec<WifiDev>,
    phy: &mut Option<String>,
    iface: &mut Option<String>,
    iftype: &mut Option<String>,
) {
    if let (Some(ifn), Some(ty)) = (iface.take(), iftype.take()) {
        if let Some(p) = phy.clone() {
            rows.push(WifiDev {
                ifname: ifn,
                phy: p,
                iftype: ty,
            });
        }
    }
}

fn flush_wifi_dev_iface(
    rows: &mut Vec<WifiDev>,
    phy: &Option<String>,
    iface: &mut Option<String>,
    iftype: &mut Option<String>,
) {
    if let (Some(ifn), Some(ty)) = (iface.take(), iftype.take()) {
        if let Some(p) = phy.clone() {
            rows.push(WifiDev {
                ifname: ifn,
                phy: p,
                iftype: ty,
            });
        }
    }
}

/// **STA-capable** := iw reports the interface type **`managed`** (client) and not **`AP`**.
/// Used by the UI / REST to filter out virtual AP vifs (`ap0` via `iw … type __ap`) when
/// presenting **Preferred Wi-Fi interface** choices or enumerating devices for client operations.
pub async fn is_sta_capable(ifname: &str) -> bool {
    let name = ifname.trim();
    if name.is_empty() {
        return false;
    }
    let Ok(devs) = list_wifi_devices().await else {
        return false;
    };
    let Some(dev) = devs.iter().find(|d| d.ifname == name) else {
        return false;
    };
    let ty_lc = dev.iftype.to_ascii_lowercase();
    if ty_lc == "ap" || ty_lc.contains("__ap") {
        return false;
    }
    if ty_lc == "managed" {
        return true;
    }
    // Not currently managed: fall back to phy capability list (e.g. brand-new iface not yet configured).
    match phy_capability(&dev.phy).await {
        Ok(cap) => cap
            .interface_modes
            .iter()
            .any(|m| m.eq_ignore_ascii_case("managed")),
        Err(_) => false,
    }
}

pub async fn iw_dev_exists(ifname: &str) -> bool {
    let name = ifname.trim();
    if name.is_empty() {
        return false;
    }
    match wifi_iface_to_phy_map().await {
        Ok(map) => map.contains_key(name),
        Err(_) => false,
    }
}

/// Create the AP vif on the same phy as **`sta_if`** iff it doesn't already exist.
/// Brings it up with `ip link set dev <ap> up` via `iw` sibling commands is **not** in scope here —
/// NetworkManager will take the device up when the AP profile is activated.
pub async fn ensure_ap_vif_present(sta_if: &str, ap_if: &str) -> Result<()> {
    let sta = sta_if.trim();
    let ap = ap_if.trim();
    if sta.is_empty() || ap.is_empty() || sta == ap {
        return Ok(());
    }
    if iw_dev_exists(ap).await {
        tracing::debug!(
            "{} iw: ap vif {} already present on phy; skipping add",
            crate::log_tags::EVO_NET,
            ap
        );
        return Ok(());
    }
    tracing::info!(
        "{} iw: creating ap vif {} on phy of {} (type __ap)",
        crate::log_tags::EVO_NET,
        ap,
        sta
    );
    iw_output(&["dev", sta, "interface", "add", ap, "type", "__ap"]).await?;
    Ok(())
}

pub async fn ensure_ap_vif_absent(ap_if: &str) -> Result<()> {
    let ap = ap_if.trim();
    if ap.is_empty() {
        return Ok(());
    }
    if !iw_dev_exists(ap).await {
        return Ok(());
    }
    tracing::info!(
        "{} iw: removing ap vif {}",
        crate::log_tags::EVO_NET,
        ap
    );
    iw_output(&["dev", ap, "del"]).await?;
    Ok(())
}

/// Best-effort STA link info parsed from `iw dev <sta> link`.
#[derive(Debug, Clone, Default)]
pub struct StaLinkInfo {
    pub connected: bool,
    pub freq_mhz: Option<u32>,
    pub channel: Option<u32>,
    /// NM **`802-11-wireless.band`** value: `bg` (2.4 GHz), `a` (5 GHz), `6GHz`.
    pub band: Option<String>,
}

pub async fn sta_link_info(sta_if: &str) -> StaLinkInfo {
    let sta = sta_if.trim();
    if sta.is_empty() {
        return StaLinkInfo::default();
    }
    let out = match iw_output(&["dev", sta, "link"]).await {
        Ok(s) => s,
        Err(_) => return StaLinkInfo::default(),
    };
    let mut info = StaLinkInfo::default();
    for line in out.lines() {
        let lt = line.trim();
        if lt.starts_with("Not connected") {
            info.connected = false;
            break;
        }
        if lt.starts_with("Connected to") {
            info.connected = true;
        }
        if let Some(rest) = lt.strip_prefix("freq:") {
            if let Ok(v) = rest.trim().parse::<u32>() {
                info.freq_mhz = Some(v);
                info.channel = freq_to_channel(v);
                info.band = freq_to_band(v);
            }
        }
    }
    info
}

pub fn freq_to_channel(mhz: u32) -> Option<u32> {
    match mhz {
        2412..=2472 => Some((mhz - 2407) / 5),
        2484 => Some(14),
        5000..=5895 => Some((mhz - 5000) / 5),
        5955..=7115 => Some((mhz - 5950) / 5),
        _ => None,
    }
}

pub fn freq_to_band(mhz: u32) -> Option<String> {
    if mhz < 3000 {
        Some("bg".into())
    } else if (3000..5900).contains(&mhz) {
        Some("a".into())
    } else if mhz >= 5900 {
        Some("6GHz".into())
    } else {
        None
    }
}

/// Convenience: does the phy backing **`sta_if`** support concurrent **managed + AP**?
/// Logs the probed phy, detected capability, and channel limit at `debug`.
pub async fn sta_phy_supports_concurrent_sta_ap(sta_if: &str) -> bool {
    let Ok(Some(phy)) = phy_for_ifname(sta_if).await else {
        tracing::debug!(
            "{} wifi_phy: no phy resolved for {} (iw dev failed or ifname missing)",
            crate::log_tags::EVO_NET,
            sta_if
        );
        return false;
    };
    match phy_capability(&phy).await {
        Ok(cap) => {
            tracing::debug!(
                "{} wifi_phy: sta_if={} phy={} supports_managed_plus_ap={} max_channels={} modes={:?}",
                crate::log_tags::EVO_NET,
                sta_if,
                cap.phy,
                cap.supports_managed_plus_ap,
                cap.max_channels,
                cap.interface_modes
            );
            cap.supports_managed_plus_ap
        }
        Err(e) => {
            tracing::debug!(
                "{} wifi_phy: phy_capability({}) failed: {}",
                crate::log_tags::EVO_NET,
                phy,
                e
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PI5_PHY_INFO: &str = r#"
Wiphy phy0
	Supported interface modes:
		 * IBSS
		 * managed
		 * AP
		 * P2P-client
		 * P2P-GO
		 * P2P-device
	valid interface combinations:
		 * #{ managed } <= 2, #{ P2P-device } <= 1, #{ P2P-client, P2P-GO } <= 1,
		   total <= 3, #channels <= 2
		 * #{ managed } <= 1, #{ AP } <= 1, #{ P2P-client } <= 1, #{ P2P-device } <= 1,
		   total <= 4, #channels <= 1
	Device supports SAE with AUTHENTICATE command.
"#;

    const STA_ONLY_PHY_INFO: &str = r#"
Wiphy phy1
	Supported interface modes:
		 * managed
		 * monitor
	valid interface combinations:
		 * #{ managed } <= 1, #{ P2P-client, P2P-GO } <= 1,
		   total <= 2, #channels <= 1
"#;

    #[test]
    fn pi5_phy_supports_managed_plus_ap() {
        let cap = parse_phy_info("phy0", PI5_PHY_INFO);
        assert!(cap.supports_managed_plus_ap, "combos: {:?}", extract_interface_combinations(PI5_PHY_INFO));
        assert!(cap.interface_modes.iter().any(|m| m == "managed"));
        assert!(cap.interface_modes.iter().any(|m| m == "AP"));
        assert_eq!(cap.max_channels, 2);
    }

    #[test]
    fn sta_only_phy_does_not_support_concurrent_ap() {
        let cap = parse_phy_info("phy1", STA_ONLY_PHY_INFO);
        assert!(!cap.supports_managed_plus_ap);
        assert_eq!(cap.max_channels, 1);
    }

    #[test]
    fn freq_to_channel_basics() {
        assert_eq!(freq_to_channel(2412), Some(1));
        assert_eq!(freq_to_channel(2437), Some(6));
        assert_eq!(freq_to_channel(2484), Some(14));
        assert_eq!(freq_to_channel(5180), Some(36));
        assert_eq!(freq_to_channel(5955), Some(1));
    }

    #[test]
    fn freq_to_band_basics() {
        assert_eq!(freq_to_band(2412).as_deref(), Some("bg"));
        assert_eq!(freq_to_band(5180).as_deref(), Some("a"));
        assert_eq!(freq_to_band(5955).as_deref(), Some("6GHz"));
    }
}
