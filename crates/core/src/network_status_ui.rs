//! Socket.IO **`pushInfoNetwork`** payload for **Settings → Network** (`network-status` core plugin).
//! Matches volumio3-backend `ControllerNetwork.prototype.parseInfoNetworkResults` shape.
//! After NM apply, Socket.IO schedules **broadcast** **`pushInfoNetwork`** at **5 s / 10 s** and
//! **`pushInfoNetworkReload`** at **10 s** (Node `onNetworkingRestart`). Stock UI uses one-time
//! bindings in the status template; **`pushInfoNetworkReload`** reloads the page on the network plugin
//! so addresses update (see **`socketio::schedule_push_info_network_refresh`**).
//!
//! **STA rows:** Only interfaces with a **global IPv4** appear in the first pass (`get_if_addrs`).
//! When **`wlan0`** loses DHCP / disconnects at L3, the Wireless row **disappears** — that is the
//! payload reflecting reality, not Socket.IO “disconnecting” the widget.
//!
//! **Concurrent AP+STA (one PHY):** **`ap0`** is **not** a second physical NIC — it is the kernel
//! **`iw dev … add … type __ap`** virtual interface on the **same** PHY as **`wlan0`**. It exists
//! only when we run STA (`wlan0`) + hotspot concurrently on that radio; NM binds the hotspot profile to
//! **`ap*`**. We classify **`ap*`** here so **`10.42.0.1`** appears in status; labels distinguish it from
//! a standalone AP radio (see **`ssid`** for hotspot-on-**`ap`** rows).

use if_addrs::{get_if_addrs, IfAddr};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::process::Stdio;
use tokio::process::Command;

use crate::network_config::NM_CON_WIFI_STA;

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
    let mut wifi_ifaces_emitted: HashSet<String> = HashSet::new();
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
            wifi_ifaces_emitted.insert(name.clone());
            let hotspot = is_hotspot_address(&ip_s);
            let ssid = if hotspot {
                if name.starts_with("ap") {
                    format!("Hotspot (virtual {name}, STA+AP same PHY)")
                } else {
                    "Hotspot".to_string()
                }
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
    append_sta_placeholder_no_ipv4(&mut out, &wifi_ifaces_emitted).await;
    out
}

/// **`wlan0`** has no row yet, but NM still has **`volumio-evo-wifi-sta`** bound (DHCP pending or
/// reconnecting): show Wireless with empty IP so the fragment does not “blink” only Wired.
async fn append_sta_placeholder_no_ipv4(out: &mut Vec<Value>, wifi_with_ip: &HashSet<String>) {
    let Ok(devs) = crate::nm_network::nm_device_table().await else {
        return;
    };
    for d in devs {
        if !d.kind.eq_ignore_ascii_case("wifi") {
            continue;
        }
        let dev = d.device.trim();
        if dev.starts_with("ap") || dev.starts_with("p2p-dev") {
            continue;
        }
        if !dev.starts_with("wlan") && !dev.starts_with("wl") {
            continue;
        }
        if wifi_with_ip.contains(dev) {
            continue;
        }
        if d.connection.trim() != NM_CON_WIFI_STA {
            continue;
        }
        let state_l = d.state.to_lowercase();
        let status = if state_l.contains("connect") {
            "connecting"
        } else {
            "connected"
        };
        let ssid = active_wifi_ssid_nm(dev).await.unwrap_or_else(|| "Wireless".to_string());
        out.push(json!({
            "type": "Wireless",
            "ip": "",
            "ssid": ssid,
            "signal": 0u8,
            "status": status,
            "speed": "",
        }));
    }
}

fn iface_sort_key(name: &str) -> u8 {
    if name.starts_with("eth") || name == "end0" {
        0
    } else if name.starts_with("wl") || name.starts_with("wlan") {
        1
    } else if name.starts_with("ap") {
        2
    } else {
        3
    }
}

/// Station / P2P / **`__ap`** hotspot vifs. **`ap*`** entries are virtual (same PHY as matching
/// **`wlan*`**); they are **not** an extra RF front-end — only concurrent STA+AP creates them.
fn is_wifi_iface(name: &str) -> bool {
    name.starts_with("wl")
        || name.starts_with("wlan")
        || name.starts_with("ap")
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
