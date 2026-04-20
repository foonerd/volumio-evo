//! Persisted SMB server settings (`settings/samba/state.toml`).
//! Policy: repository `docs/SAMBA.md`.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::paths::default_samba_state_path;

/// Named SMB user stored on disk (**never** passwords).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SambaUserRecord {
    #[serde(default)]
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SambaExtraShare {
    #[serde(default)]
    pub name: String,
    /// Absolute path (`docs/SAMBA.md` allowlist).
    #[serde(default)]
    pub path: String,
}

/// Values persisted for Settings → Network → SMB (`state.toml`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SambaSettings {
    #[serde(default)]
    pub enabled: bool,
    /// `default` — omit `server min protocol`; otherwise a Samba dialect keyword (e.g. `SMB2_02`).
    #[serde(default)]
    pub min_protocol: String,
    /// Extra exported directories (validated against [`crate::paths::SMB_SHARE_ALLOWED_ROOTS`]).
    #[serde(default)]
    pub extra_shares: Vec<SambaExtraShare>,
    /// SMB login names (**passwords never stored here**; Samba passdb + [`crate::samba_apply::SMB_USER_SYNC_SCRIPT`]).
    #[serde(default)]
    pub smb_users: Vec<SambaUserRecord>,
}

impl Default for SambaSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            min_protocol: "default".to_string(),
            extra_shares: Vec::new(),
            smb_users: Vec::new(),
        }
    }
}

impl SambaSettings {
    pub fn load() -> Self {
        let path = default_samba_state_path();
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
        let mut s: SambaSettings = toml::from_str(&raw).unwrap_or_default();
        normalize_loaded(&mut s);
        s
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = default_samba_state_path();
        if let Some(parent) = Path::new(&path).parent() {
            fs::create_dir_all(parent)?;
        }
        let body = toml::to_string_pretty(self)?;
        fs::write(&path, body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o644));
        }
        Ok(())
    }

    pub fn smb_users_without_passwords_json(&self) -> Vec<serde_json::Value> {
        self.smb_users
            .iter()
            .map(|u| serde_json::json!({ "username": u.username }))
            .collect()
    }

    pub fn extra_shares_json(&self) -> Vec<serde_json::Value> {
        self.extra_shares
            .iter()
            .map(|x| serde_json::json!({ "name": x.name, "path": x.path }))
            .collect()
    }
}

/// Normalizes names, dedupes, and dialect keyword (call after programmatic edits).
pub(crate) fn normalize_samba_settings(s: &mut SambaSettings) {
    normalize_loaded(s);
}

fn normalize_loaded(s: &mut SambaSettings) {
    s.min_protocol = normalize_min_protocol(&s.min_protocol);
    for es in &mut s.extra_shares {
        es.name = es.name.trim().to_string();
        es.path = es.path.trim().to_string();
    }
    // Dedupe share names (first occurrence wins).
    let mut seen_names = HashSet::<String>::new();
    s.extra_shares.retain(|x| {
        if x.name.is_empty() || x.path.is_empty() {
            return false;
        }
        seen_names.insert(x.name.clone())
    });
    for u in &mut s.smb_users {
        u.username = u.username.trim().to_ascii_lowercase();
    }
    let mut seen_users = HashSet::<String>::new();
    s.smb_users.retain(|u| {
        if u.username.is_empty() {
            return false;
        }
        seen_users.insert(u.username.clone())
    });
}

/// Normalize persisted / form dialect keyword.
pub fn normalize_min_protocol(s: &str) -> String {
    match s.trim() {
        "" | "default" => "default".to_string(),
        "SMB2_02" => "SMB2_02".to_string(),
        "SMB3_02" => "SMB3_02".to_string(),
        _ => "default".to_string(),
    }
}
