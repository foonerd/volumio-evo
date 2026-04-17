//! Socket.IO **`pushInfoNetwork`** payload for **Settings → Network** (`network-status` core plugin).
//! Matches volumio3-backend `ControllerNetwork.prototype.parseInfoNetworkResults` shape.

use if_addrs::{get_if_addrs, IfAddr};
use serde_json::{json, Value};
use std::process::Stdio;
use tokio::process::Command;

/// Build the array the Angular **`network-status-plugin`** assigns to `networkInfos`.
pub async fn push_info_network_array() -> Vec<Value> {
    let mut rows: Vec<(String, std::net::Ipv4Addr)> = Vec::new();
    let Ok(ifaces) = get_if_addrs() else {
        return Vec::new();
    };
    for iface in ifaces {
        if iface.name == "lo" {
            continue;
        }
        let IfAddr::V4(v4) = iface.addr else {
            continue;
        };
        let ip = v4.ip;
        if ip.is_loopback() {
            continue;
        }
        let o = ip.octets();
        if o[0] == 169 && o[1] == 254 {
            continue;
        }
        rows.push((iface.name, ip));
    }
    rows.sort_by(|a, b| iface_sort_key(&a.0).cmp(&iface_sort_key(&b.0)));

    let mut out: Vec<Value> = Vec::new();
    for (name, ip) in rows {
        let ip_s = ip.to_string();
        if is_ethernet_iface(&name) {
            let speed = ethtool_speed_mbps(&name).await;
            let speed_s = speed.map(|s| format!("{s} Mb/s")).unwrap_or_default();
            out.push(json!({
                "type": "Wired",
                "ip": ip_s,
                "status": "connected",
                "speed": speed_s,
            }));
        } else if is_wifi_iface(&name) {
            let hotspot = is_hotspot_address(&ip_s);
            let ssid = if hotspot {
                "Hotspot".to_string()
            } else {
                active_wifi_ssid_nm(&name)
                    .await
                    .unwrap_or_else(|| "Wireless".to_string())
            };
            let signal = if hotspot {
                5u8
            } else {
                wifi_signal_bars_nm(&name).await.unwrap_or(4)
            };
            out.push(json!({
                "type": "Wireless",
                "ip": ip_s,
                "ssid": ssid,
                "signal": signal,
                "status": "connected",
                "speed": "",
            }));
        }
    }
    out
}

fn iface_sort_key(name: &str) -> u8 {
    if name.starts_with("eth") || name == "end0" {
        0
    } else if name.starts_with("wl") || name.starts_with("wlan") {
        1
    } else {
        2
    }
}

fn is_wifi_iface(name: &str) -> bool {
    name.starts_with("wl") || name.starts_with("wlan")
}

fn is_ethernet_iface(name: &str) -> bool {
    !is_wifi_iface(name)
        && (name.starts_with("eth")
            || name.starts_with("en")
            || name == "end0")
}

/// Classic Volumio hotspot uses this address; NM **shared** often uses `10.42.0.1`.
fn is_hotspot_address(ip: &str) -> bool {
    ip == "192.168.211.1" || ip == "10.42.0.1"
}

async fn ethtool_speed_mbps(iface: &str) -> Option<u32> {
    let out = Command::new("ethtool")
        .arg(iface)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    for line in s.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("speed:") {
            for part in line.split_whitespace() {
                if part.ends_with("Mb/s") {
                    let num = part.trim_end_matches("Mb/s").trim();
                    return num.parse().ok();
                }
            }
        }
    }
    None
}

async fn active_wifi_ssid_nm(iface: &str) -> Option<String> {
    let out = Command::new("nmcli")
        .args(["-t", "-f", "SSID,ACTIVE", "dev", "wifi", "list", "ifname", iface])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let mut p = line.split(':');
        let ssid = p.next().unwrap_or("").trim();
        let active = p.next().unwrap_or("").trim();
        if active.eq_ignore_ascii_case("yes") && !ssid.is_empty() {
            return Some(ssid.to_string());
        }
    }
    None
}

async fn wifi_signal_bars_nm(iface: &str) -> Option<u8> {
    let out = Command::new("nmcli")
        .args(["-t", "-f", "SSID,SIGNAL,ACTIVE", "dev", "wifi", "list", "ifname", iface])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let mut parts = line.split(':');
        let _ssid = parts.next()?;
        let sig = parts.next()?.trim().parse::<u8>().ok()?;
        let active = parts.next().unwrap_or("").trim();
        if active.eq_ignore_ascii_case("yes") {
            return Some(crate::nm_network::signal_bars_from_pct(Some(sig)));
        }
    }
    None
}
