//! Generate `smb.conf` for the Evo SMB server (guest-friendly; optional dialect floor).

use crate::config::MusicSourcesConfig;
use crate::paths::{SMB_SHARE_ALLOWED_ROOTS, SMB_SHARE_DENIED_PREFIXES};
use crate::samba_settings::SambaSettings;

use std::path::{Path, PathBuf};

/// Sanitize **netbios name** (max 15 alnum / hyphen; default `Volumio`).
pub fn netbios_name_from_device(device_name: &str) -> String {
    let raw = device_name.trim();
    let compact: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(15)
        .collect();
    let mut s = if compact.is_empty() {
        "Volumio".to_string()
    } else {
        compact
    };
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        let mut p = String::from("V");
        p.push_str(&s);
        s = p.chars().take(15).collect();
    }
    s
}

/// Validate a user-defined share path against the allow/deny prefix policy (`docs/SAMBA.md`).
pub fn validate_extra_share_path(policy_path: &str) -> Result<(), String> {
    let trimmed = policy_path.trim();
    if trimmed.is_empty() || !trimmed.starts_with('/') {
        return Err("share path must be a non-empty absolute path".into());
    }
    let normalized = trimmed.trim_end_matches('/');
    for deny in SMB_SHARE_DENIED_PREFIXES {
        if normalized.starts_with(deny) {
            return Err(format!("path is under a denied prefix ({deny})"));
        }
    }
    if !SMB_SHARE_ALLOWED_ROOTS
        .iter()
        .any(|root| normalized.starts_with(root))
    {
        return Err("path must be under an allowed root (see docs/SAMBA.md)".into());
    }
    Ok(())
}

fn resolve_internal(ms: &MusicSourcesConfig) -> PathBuf {
    ms.local
        .clone()
        .unwrap_or_else(|| ms.music_root.join("INTERNAL"))
}

fn resolve_usb(ms: &MusicSourcesConfig) -> PathBuf {
    ms.usb
        .clone()
        .unwrap_or_else(|| ms.music_root.join("USB"))
}

fn resolve_nas(ms: &MusicSourcesConfig) -> PathBuf {
    ms.nas
        .clone()
        .unwrap_or_else(|| ms.music_root.join("NAS"))
}

fn path_for_smb(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn sanitize_share_section_name(name: &str) -> String {
    let t = name.trim();
    if t.is_empty() {
        return "ExtraShare".to_string();
    }
    let s: String = t
        .chars()
        .map(|c| {
            if c.is_control() || c == '[' || c == ']' {
                '_'
            } else {
                c
            }
        })
        .take(80)
        .collect();
    if s.trim().is_empty() {
        "ExtraShare".into()
    } else {
        s.trim().to_string()
    }
}

/// Build full `smb.conf` body.
pub fn render_smb_conf(
    settings: &SambaSettings,
    ms: &MusicSourcesConfig,
    netbios_name: &str,
    guest_unix_user: &str,
) -> String {
    let internal = path_for_smb(&resolve_internal(ms));
    let usb = path_for_smb(&resolve_usb(ms));
    let nas = path_for_smb(&resolve_nas(ms));

    let mp = crate::samba_settings::normalize_min_protocol(settings.min_protocol.as_str());

    let mut global = String::new();
    global.push_str("[global]\n");
    global.push_str("workgroup = WORKGROUP\n");
    global.push_str(&format!("netbios name = {netbios_name}\n"));
    global.push_str("server string = Volumio Evo\n");
    global.push_str("security = user\n");
    global.push_str("map to guest = Bad User\n");
    global.push_str(&format!("guest account = {guest_unix_user}\n"));
    global.push_str("encrypt passwords = yes\n");
    global.push_str("wins support = yes\n");
    global.push_str("local master = no\n");
    global.push_str("preferred master = no\n");
    global.push_str("os level = 30\n");
    if mp != "default" {
        global.push_str(&format!("server min protocol = {mp}\n"));
    }

    let mut out = global;
    out.push_str("\n");

    out.push_str("[Internal Storage]\n");
    out.push_str("\tcomment = Internal Music Folder\n");
    out.push_str(&format!("\tpath = {internal}\n"));
    out.push_str("\tread only = no\n");
    out.push_str("\tguest ok = yes\n\n");

    out.push_str("[USB]\n");
    out.push_str("\tcomment = USB Music Folder\n");
    out.push_str(&format!("\tpath = {usb}\n"));
    out.push_str("\tread only = no\n");
    out.push_str("\tguest ok = yes\n\n");

    out.push_str("[NAS]\n");
    out.push_str("\tcomment = NAS Music Folder\n");
    out.push_str(&format!("\tpath = {nas}\n"));
    out.push_str("\tread only = no\n");
    out.push_str("\tguest ok = yes\n\n");

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for es in &settings.extra_shares {
        if let Err(_) = validate_extra_share_path(es.path.as_str()) {
            continue;
        }
        let section = sanitize_share_section_name(es.name.as_str());
        let key = section.to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let p = es.path.trim();
        out.push_str(&format!("[{section}]\n"));
        out.push_str("\tcomment = User-defined share\n");
        out.push_str(&format!("\tpath = {}\n", path_for_smb(Path::new(p))));
        out.push_str("\tread only = no\n");
        out.push_str("\tguest ok = yes\n\n");
    }

    out
}
