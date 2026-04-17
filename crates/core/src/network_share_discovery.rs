//! LAN SMB discovery for `getNetworkSharesDiscovery` → `pushNetworkSharesDiscovery`.
//! Replaces Node `volumiodiscovery` + `smbclient -L`: mDNS via `avahi-browse`, then guest listing per host.

use std::io::ErrorKind;
use std::time::Duration;

use regex::Regex;
use serde::Serialize;
use tokio::process::Command;

/// `avahi-browse` does not exit by itself; coreutils **`timeout(1)`** kills it after this many seconds
/// (exit 124 is normal). Stdout still lists services seen up to that point.
const AVAHI_BROWSE_KILL_AFTER_SECS: u64 = 18;
/// Safety cap on the whole `timeout avahi-browse ...` invocation.
const AVAHI_WRAPPER_MAX_WAIT: Duration = Duration::from_secs(25);
/// Per-host `smbclient -L` budget.
const SMBCLIENT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Serialize)]
struct ShareRow {
    sharename: String,
    path: String,
}

#[derive(Debug, Serialize)]
struct NasDevice {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ip: Option<String>,
    shares: Vec<ShareRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

#[derive(Debug, Serialize)]
struct DiscoveryPayload {
    nas: Vec<NasDevice>,
}

/// Run mDNS + smbclient discovery (async). Returns `{ "nas": [ ... ] }` for Socket.IO.
pub async fn discover_network_shares() -> serde_json::Value {
    let hosts = match discover_smb_hosts_avahi().await {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!(
                "{} SMB mDNS browse failed: {}",
                crate::log_tags::EVO_UI,
                e
            );
            return serde_json::to_value(DiscoveryPayload { nas: vec![] })
                .unwrap_or_else(|_| serde_json::json!({ "nas": [] }));
        }
    };

    if hosts.is_empty() {
        tracing::debug!(
            "{} SMB mDNS: no _smb._tcp services found",
            crate::log_tags::EVO_UI
        );
        return serde_json::to_value(DiscoveryPayload { nas: vec![] })
            .unwrap_or_else(|_| serde_json::json!({ "nas": [] }));
    }

    tracing::info!(
        "{} SMB discovery: {} host(s) from mDNS",
        crate::log_tags::EVO_UI,
        hosts.len()
    );

    let mut nas = Vec::with_capacity(hosts.len());
    for (name, ip) in hosts {
        match list_shares_on_host(&name, &ip).await {
            Ok(dev) => nas.push(dev),
            Err(e) => tracing::debug!(
                "{} smbclient -L failed for {} ({}): {}",
                crate::log_tags::EVO_UI,
                name,
                ip,
                e
            ),
        }
    }

    serde_json::to_value(DiscoveryPayload { nas }).unwrap_or_else(|_| {
        serde_json::json!({ "nas": [] })
    })
}

/// Parseable `avahi-browse -p -r` lines (service type may be `_smb._tcp` or e.g. `Microsoft Windows Network`):
/// `=;iface;IPv4;NAME;<type>;domain;host;IP;port;...`
async fn discover_smb_hosts_avahi() -> Result<Vec<(String, String)>, String> {
    let kill_secs = AVAHI_BROWSE_KILL_AFTER_SECS.to_string();
    let out = match tokio::time::timeout(
        AVAHI_WRAPPER_MAX_WAIT,
        Command::new("timeout")
            .arg(&kill_secs)
            .arg("avahi-browse")
            .args(["-p", "-r", "_smb._tcp"])
            .output(),
    )
    .await
    {
        Err(_) => return Err(
            "timeout/avahi-browse: outer wait exceeded (bug or hung subprocess)".into(),
        ),
        Ok(Err(e)) if e.kind() == ErrorKind::NotFound => {
            return Err(
                "`timeout` or `avahi-browse` missing (install coreutils and avahi-utils)".into(),
            );
        }
        Ok(Err(e)) => return Err(format!("timeout avahi-browse: {e}")),
        Ok(Ok(o)) => o,
    };

    let text = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let code = out.status.code().unwrap_or(-1);

    // `timeout` returns 124 when the child was killed — expected; we still parse any stdout.
    if text.trim().is_empty() {
        if stderr.contains("Failed to create client object")
            || stderr.contains("Daemon not responding")
            || stderr.contains("No such entity")
        {
            return Err(
                "avahi-daemon not usable; try: sudo systemctl enable --now avahi-daemon".into(),
            );
        }
        return Err(format!(
            "avahi-browse produced no output (exit {code}); stderr={}",
            stderr.trim().chars().take(200).collect::<String>()
        ));
    }
    let mut seen_ip: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut hosts: Vec<(String, String)> = Vec::new();

    for line in text.lines() {
        if !line.starts_with('=') {
            continue;
        }
        let parts: Vec<&str> = line.split(';').collect();
        if parts.len() <= 7 {
            continue;
        }
        let name = parts[3].trim().to_string();
        let ip = parts[7].trim();
        if ip.parse::<std::net::Ipv4Addr>().is_err() {
            continue;
        }
        if seen_ip.insert(ip.to_string()) {
            hosts.push((name, ip.to_string()));
        }
    }

    Ok(hosts)
}

async fn list_shares_on_host(display_name: &str, ip: &str) -> Result<NasDevice, String> {
    // Match Node: guest list; -m SMB3_11 for dialect negotiation; debuglevel for stderr version parse.
    let out = tokio::time::timeout(
        SMBCLIENT_TIMEOUT,
        Command::new("/usr/bin/smbclient")
            .args([
                "--debuglevel=4",
                "-N",
                "-L",
                ip,
                "-m",
                "SMB3_11",
            ])
            .output(),
    )
    .await
    .map_err(|_| "smbclient timed out".to_string())?
    .map_err(|e| e.to_string())?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let shares = parse_smbclient_disk_list(&stdout);
    if shares.is_empty() && !out.status.success() {
        return Err(
            stderr
                .trim()
                .chars()
                .take(400)
                .collect::<String>(),
        );
    }
    let version = parse_smb_negotiated_version(&stderr);

    Ok(NasDevice {
        name: display_name.to_string(),
        ip: Some(ip.to_string()),
        shares,
        version,
    })
}

/// Same idea as Node `parseSmbClientResult`: lines containing `Disk`.
fn parse_smbclient_disk_list(stdout: &str) -> Vec<ShareRow> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        if !line.contains("Disk") {
            continue;
        }
        let Some(prefix) = line.split("Disk").next() else {
            continue;
        };
        let sharename = prefix.trim().to_string();
        if sharename.is_empty() {
            continue;
        }
        out.push(ShareRow {
            path: sharename.clone(),
            sharename,
        });
    }
    out
}

fn parse_smb_negotiated_version(stderr: &str) -> Option<String> {
    let re = Regex::new(r"(?i)negotiated dialect\[(SMB[0-9_]+)\]").ok()?;
    if let Some(c) = re.captures(stderr) {
        return c.get(1).map(|m| m.as_str().to_string());
    }
    const KNOWN: &[&str] = &[
        "SMB3_11", "SMB3_10", "SMB3_02", "SMB3", "SMB2_24", "SMB2_22", "SMB2_10", "SMB2_02",
        "SMB1", "NT1",
    ];
    for line in stderr.lines() {
        for d in KNOWN {
            if line.contains(d) {
                return Some((*d).to_string());
            }
        }
    }
    None
}
