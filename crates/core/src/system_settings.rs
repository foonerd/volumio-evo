//! Persisted **Settings → System** state: device name, locale (language, country → regulatory domain),
//! timezone, privacy / update placeholders, WPE kiosk hints.
//!
//! Stored at **`settings/system/state.toml`** under [`crate::paths::settings_dir`].
//! On daemon startup, [`crate::api::run_startup_system_locale_apply`] reapplies timezone, **`iw reg set`**, and
//! **`hostnamectl`** so the OS matches persisted values after reboot (same as saving the locale section).

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::paths;

fn state_path() -> PathBuf {
    std::env::var("VOLUMIO_EVO_SYSTEM_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| paths::default_system_state_path())
}

/// ISO 3166-1 alpha-2 upper-case; drives `iw reg set` when applying regulatory domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSettings {
    /// Network / Zeroconf hostname (`hostnamectl` when permitted).
    #[serde(default = "default_device_name")]
    pub device_name: String,
    #[serde(default = "default_language")]
    pub language_code: String,
    #[serde(default = "default_country")]
    pub country_code: String,
    /// IANA timezone (`timedatectl set-timezone`).
    #[serde(default = "default_timezone")]
    pub timezone: String,

    #[serde(default)]
    pub allow_ui_statistics: bool,

    /// Reserved for automatic OTA window (placeholder).
    #[serde(default = "default_true")]
    pub automatic_updates: bool,
    #[serde(default)]
    pub automatic_updates_start_hour: u8,
    #[serde(default = "default_23")]
    pub automatic_updates_stop_hour: u8,

    /// WPE / on-device shell (placeholder until kiosk layer lands).
    #[serde(default)]
    pub kiosk_enabled: bool,
    #[serde(default = "default_primary_display")]
    pub primary_display: String,
}

fn default_device_name() -> String {
    "volumio-evo".to_string()
}
fn default_language() -> String {
    "en".to_string()
}
fn default_country() -> String {
    "US".to_string()
}
fn default_timezone() -> String {
    "UTC".to_string()
}
fn default_true() -> bool {
    true
}
fn default_23() -> u8 {
    23
}
fn default_primary_display() -> String {
    "auto".to_string()
}

impl Default for SystemSettings {
    fn default() -> Self {
        Self {
            device_name: default_device_name(),
            language_code: default_language(),
            country_code: default_country(),
            timezone: default_timezone(),
            allow_ui_statistics: true,
            automatic_updates: true,
            automatic_updates_start_hour: 0,
            automatic_updates_stop_hour: 23,
            kiosk_enabled: false,
            primary_display: default_primary_display(),
        }
    }
}

impl SystemSettings {
    pub fn load() -> Self {
        let path = state_path();
        if !path.exists() {
            let mut s = Self::default();
            if let Some(h) = read_hostname_hint() {
                if !h.is_empty() && h != "localhost" {
                    s.device_name = h;
                }
            }
            return s;
        }
        match std::fs::read_to_string(&path) {
            Ok(t) => match toml::from_str::<Self>(&t) {
                Ok(mut s) => {
                    s.normalize_in_place();
                    s
                }
                Err(e) => {
                    tracing::warn!(
                        "{} failed to parse {:?}: {}; using defaults",
                        crate::log_tags::EVO_UI,
                        path,
                        e
                    );
                    Self::default()
                }
            },
            Err(e) => {
                tracing::warn!(
                    "{} failed to read {:?}: {}; using defaults",
                    crate::log_tags::EVO_UI,
                    path,
                    e
                );
                Self::default()
            }
        }
    }

    fn normalize_in_place(&mut self) {
        self.country_code = self.country_code.trim().to_uppercase();
        if self.country_code.len() != 2 {
            self.country_code = default_country();
        }
        self.language_code = self.language_code.trim().to_lowercase();
        if self.language_code.is_empty() {
            self.language_code = default_language();
        }
        self.device_name = self.device_name.trim().to_string();
        if self.device_name.is_empty() {
            self.device_name = default_device_name();
        }
        self.automatic_updates_start_hour = self.automatic_updates_start_hour.min(23);
        self.automatic_updates_stop_hour = self.automatic_updates_stop_hour.min(23);
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = state_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let t = toml::to_string_pretty(self).context("serialize system state")?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, t).with_context(|| format!("write {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
        Ok(())
    }

    pub fn merge_general_payload(&mut self, data: &Value) -> bool {
        let mut changed = false;
        if let Some(v) = extract_text(data, "player_name") {
            let t = v.trim();
            if !t.is_empty() && t != self.device_name {
                self.device_name = t.to_string();
                changed = true;
            }
        }
        changed
    }

    pub fn merge_locale_payload(&mut self, data: &Value) -> bool {
        let mut changed = false;
        if let Some(lang) = extract_select_value(data, "language") {
            let l = lang.trim().to_lowercase();
            if !l.is_empty() && l != self.language_code {
                self.language_code = l;
                changed = true;
            }
        }
        if let Some(c) = extract_select_value(data, "country") {
            let u = c.trim().to_uppercase();
            if u.len() == 2 && u != self.country_code {
                self.country_code = u;
                changed = true;
            }
        }
        if let Some(tz) = extract_select_value(data, "timezone") {
            let t = tz.trim();
            if !t.is_empty() && t != self.timezone {
                self.timezone = t.to_string();
                changed = true;
            }
        }
        changed
    }

    pub fn merge_update_payload(&mut self, data: &Value) -> bool {
        let mut changed = false;
        if let Some(v) = data.get("automatic_updates").and_then(|x| x.as_bool()) {
            if v != self.automatic_updates {
                self.automatic_updates = v;
                changed = true;
            }
        }
        if let Some(h) = extract_hour_select(data, "automatic_updates_start_time") {
            if h != self.automatic_updates_start_hour {
                self.automatic_updates_start_hour = h;
                changed = true;
            }
        }
        if let Some(h) = extract_hour_select(data, "automatic_updates_stop_time") {
            if h != self.automatic_updates_stop_hour {
                self.automatic_updates_stop_hour = h;
                changed = true;
            }
        }
        changed
    }

    pub fn merge_privacy_payload(&mut self, data: &Value) -> bool {
        let mut changed = false;
        if let Some(v) = data.get("allow_ui_statistics").and_then(|x| x.as_bool()) {
            if v != self.allow_ui_statistics {
                self.allow_ui_statistics = v;
                changed = true;
            }
        }
        changed
    }

    pub fn merge_kiosk_payload(&mut self, data: &Value) -> bool {
        let mut changed = false;
        if let Some(v) = data.get("kiosk_enabled").and_then(|x| x.as_bool()) {
            if v != self.kiosk_enabled {
                self.kiosk_enabled = v;
                changed = true;
            }
        }
        if let Some(pd) = extract_select_value(data, "primary_display") {
            let t = pd.trim();
            if !t.is_empty() && t != self.primary_display {
                self.primary_display = t.to_string();
                changed = true;
            }
        }
        changed
    }
}

fn extract_text(data: &Value, key: &str) -> Option<String> {
    data.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn extract_select_value(data: &Value, key: &str) -> Option<String> {
    let v = data.get(key)?;
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    v.get("value")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| v.get("value").and_then(|x| x.as_i64()).map(|n| n.to_string()))
}

fn extract_hour_select(data: &Value, key: &str) -> Option<u8> {
    let v = data.get(key)?;
    let n = if let Some(i) = v.as_i64() {
        i as i32
    } else if let Some(o) = v.as_object() {
        o.get("value")?.as_i64()? as i32
    } else if let Some(s) = v.as_str() {
        s.parse().ok()?
    } else {
        return None;
    };
    if (0..=23).contains(&n) {
        Some(n as u8)
    } else {
        None
    }
}

/// Run `timedatectl list-timezones` once and cache (best effort).
pub fn list_timezones_cached() -> &'static [String] {
    static CACHE: OnceLock<Vec<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let out = std::process::Command::new("timedatectl")
                .args(["list-timezones"])
                .output();
            match out {
                Ok(o) if o.status.success() => {
                    let s = String::from_utf8_lossy(&o.stdout);
                    let mut v: Vec<String> = s
                        .lines()
                        .map(|l| l.trim())
                        .filter(|l| !l.is_empty())
                        .map(String::from)
                        .collect();
                    v.sort();
                    if v.is_empty() {
                        vec!["UTC".into()]
                    } else {
                        v
                    }
                }
                _ => vec!["UTC".into()],
            }
        })
        .as_slice()
}

#[cfg(unix)]
fn effective_uid_is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
fn effective_uid_is_root() -> bool {
    false
}

/// Path to **`hostnamectl`**. Must match **`/etc/sudoers.d/volumio-evo-hostname-timedate`** when non-root.
/// Bootstrap sets **`Environment=VOLUMIO_EVO_HOSTNAMECTL=...`** in **`10-runtime-user.conf`**.
pub fn hostnamectl_bin() -> String {
    std::env::var("VOLUMIO_EVO_HOSTNAMECTL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/bin/hostnamectl".to_string())
}

/// Path to **`timedatectl`**. Must match bootstrap sudoers when non-root.
/// Bootstrap sets **`Environment=VOLUMIO_EVO_TIMEDATECTL=...`** in **`10-runtime-user.conf`**.
pub fn timedatectl_bin() -> String {
    std::env::var("VOLUMIO_EVO_TIMEDATECTL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/bin/timedatectl".to_string())
}

/// Best-effort read of OS timezone (may differ from persisted file before apply).
pub fn read_os_timezone() -> Option<String> {
    let out = std::process::Command::new(timedatectl_bin())
        .args(["show", "--property=Timezone", "--value"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() || s.eq_ignore_ascii_case("n/a") {
        None
    } else {
        Some(s)
    }
}

pub fn apply_timezone(tz: &str) -> Result<(), std::io::Error> {
    let bin = timedatectl_bin();
    let status = if effective_uid_is_root() {
        std::process::Command::new(&bin)
            .args(["set-timezone", tz.trim()])
            .status()?
    } else {
        std::process::Command::new("sudo")
            .args(["-n", &bin, "set-timezone", tz.trim()])
            .status()?
    };
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "timedatectl set-timezone failed",
        ))
    }
}

/// `iw reg set <CC>` — country must be ISO 3166-1 alpha-2.
pub fn apply_reg_domain(country_alpha2: &str) -> Result<(), std::io::Error> {
    let cc = country_alpha2.trim().to_uppercase();
    if cc.len() != 2 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "country must be ISO 3166-1 alpha-2",
        ));
    }
    let bin = crate::wifi_phy::iw_bin();
    let status = if effective_uid_is_root() {
        std::process::Command::new(&bin)
            .args(["reg", "set", &cc])
            .status()?
    } else {
        std::process::Command::new("sudo")
            .args(["-n", &bin, "reg", "set", &cc])
            .status()?
    };
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "iw reg set failed",
        ))
    }
}

pub fn apply_hostname(name: &str) -> Result<(), std::io::Error> {
    let n = name.trim();
    if n.is_empty() {
        return Ok(());
    }
    let bin = hostnamectl_bin();
    let status = if effective_uid_is_root() {
        std::process::Command::new(&bin)
            .args(["set-hostname", n])
            .status()?
    } else {
        std::process::Command::new("sudo")
            .args(["-n", &bin, "set-hostname", n])
            .status()?
    };
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "hostnamectl set-hostname failed",
        ))
    }
}

fn read_hostname_hint() -> Option<String> {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
