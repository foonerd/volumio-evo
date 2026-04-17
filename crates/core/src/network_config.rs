//! Persisted NetworkManager **intent** under `settings/network/` (see `docs/NETWORK_NM.md`).
//! Secrets (Wi‑Fi PSK) live in root-only sidecar files, not in `intent.toml`.
//! When STA uses a **USB** iface and hotspot must use another radio, set [`FallbackIntent::hotspot_ifname`].

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::paths;

/// Default Wi‑Fi interface when neither `intent.toml` nor Evo config/env set a name.
/// On many boards the **on-SoC** radio is `wlan0`; a **USB** dongle is often `wlan1` (set explicitly).
pub const DEFAULT_WIFI_IFACE: &str = "wlan0";

/// Primary intent file (no secrets).
pub const INTENT_FILENAME: &str = "intent.toml";
pub const WIFI_STA_PSK_FILENAME: &str = "wifi-sta.psk";
pub const WIFI_AP_PSK_FILENAME: &str = "wifi-ap.psk";

/// NM connection names Evo manages (idempotent apply).
pub const NM_CON_ETHERNET: &str = "volumio-evo-ethernet";
pub const NM_CON_WIFI_STA: &str = "volumio-evo-wifi-sta";
pub const NM_CON_HOTSPOT: &str = "volumio-hotspot";

pub fn network_settings_dir() -> PathBuf {
    paths::settings_dir().join("network")
}

pub fn intent_path() -> PathBuf {
    network_settings_dir().join(INTENT_FILENAME)
}

pub fn wifi_sta_psk_path() -> PathBuf {
    network_settings_dir().join(WIFI_STA_PSK_FILENAME)
}

pub fn wifi_ap_psk_path() -> PathBuf {
    network_settings_dir().join(WIFI_AP_PSK_FILENAME)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Ipv4Mode {
    Dhcp,
    #[serde(alias = "manual")]
    Static,
}

impl Default for Ipv4Mode {
    fn default() -> Self {
        Ipv4Mode::Dhcp
    }
}

/// Single role at a time on one radio (STA vs AP). Concurrent STA+AP needs two interfaces.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WifiRole {
    Sta,
    Ap,
    Disabled,
}

impl Default for WifiRole {
    fn default() -> Self {
        WifiRole::Sta
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EthernetIntent {
    /// Empty: use the first ethernet device reported by NM.
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub ipv4_mode: Ipv4Mode,
    /// Static IPv4 in CIDR form, e.g. `192.168.1.10/24`.
    #[serde(default)]
    pub ipv4_address: String,
    #[serde(default)]
    pub ipv4_gateway: String,
    #[serde(default)]
    pub ipv4_dns: Vec<String>,
}

impl Default for EthernetIntent {
    fn default() -> Self {
        Self {
            device: String::new(),
            ipv4_mode: Ipv4Mode::Dhcp,
            ipv4_address: String::new(),
            ipv4_gateway: String::new(),
            ipv4_dns: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WifiIntent {
    #[serde(default = "default_wlan_if")]
    pub ifname: String,
    #[serde(default)]
    pub role: WifiRole,
    #[serde(default)]
    pub sta_ssid: String,
    /// When false, NM uses WPA-PSK from `wifi-sta.psk` (unless `sta_open` is true).
    #[serde(default)]
    pub sta_open: bool,
    #[serde(default)]
    pub sta_ipv4_mode: Ipv4Mode,
    #[serde(default)]
    pub sta_ipv4_address: String,
    #[serde(default)]
    pub sta_ipv4_gateway: String,
    #[serde(default)]
    pub sta_ipv4_dns: Vec<String>,
    /// AP / hotspot SSID when `role` is `ap`.
    #[serde(default = "default_ap_ssid")]
    pub ap_ssid: String,
    /// AP / hotspot channel (see **`ap_band`**). Used with **`802-11-wireless.channel`** in NM.
    #[serde(default = "default_ap_channel")]
    pub ap_channel: u32,
    /// NM **`802-11-wireless.band`**: empty = infer from **`ap_channel`** (`bg` for 1–14, `a` for 36–177).
    /// Set explicitly for **6 GHz** (**`6GHz`**) or when channel numbers are ambiguous (same number on 2.4 vs 6 GHz).
    /// Valid values: **`bg`**, **`a`**, **`6GHz`** (NetworkManager spelling).
    #[serde(default)]
    pub ap_band: String,
}

fn default_wlan_if() -> String {
    DEFAULT_WIFI_IFACE.to_string()
}

fn default_ap_ssid() -> String {
    default_hotspot_ssid_from_mac()
}

fn default_ap_channel() -> u32 {
    4
}

/// Hotspot SSID when unset: **`Volumio-`** + last three MAC octets (e.g. `Volumio-7B6816`), from `eth0` / first netdev.
pub fn default_hotspot_ssid_from_mac() -> String {
    netdev_mac_suffix_hex_upper()
        .map(|s| format!("Volumio-{s}"))
        .unwrap_or_else(|| "Volumio".to_string())
}

fn netdev_mac_suffix_hex_upper() -> Option<String> {
    for iface in ["eth0", "end0", "wlan0"] {
        let p = std::path::Path::new("/sys/class/net").join(iface).join("address");
        if let Ok(s) = std::fs::read_to_string(&p) {
            return Some(mac_last_three_octets_hex_upper(&s));
        }
    }
    let dir = std::fs::read_dir("/sys/class/net").ok()?;
    for ent in dir.flatten() {
        let name = ent.file_name();
        let n = name.to_string_lossy();
        if n == "lo" || n == "docker0" {
            continue;
        }
        let p = ent.path().join("address");
        if let Ok(s) = std::fs::read_to_string(&p) {
            return Some(mac_last_three_octets_hex_upper(&s));
        }
    }
    None
}

fn mac_last_three_octets_hex_upper(addr: &str) -> String {
    let p: Vec<&str> = addr.trim().split(':').filter(|x| !x.is_empty()).collect();
    if p.len() >= 3 {
        p[p.len() - 3..].join("").to_ascii_uppercase()
    } else {
        addr.replace(':', "").to_ascii_uppercase()
    }
}

impl Default for WifiIntent {
    fn default() -> Self {
        Self {
            ifname: default_wlan_if(),
            role: WifiRole::Sta,
            sta_ssid: String::new(),
            sta_open: false,
            sta_ipv4_mode: Ipv4Mode::Dhcp,
            sta_ipv4_address: String::new(),
            sta_ipv4_gateway: String::new(),
            sta_ipv4_dns: Vec::new(),
            ap_ssid: default_ap_ssid(),
            ap_channel: default_ap_channel(),
            ap_band: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FallbackIntent {
    /// When true, Evo may activate [`NM_CON_HOTSPOT`] if STA fails (watchdog / Phase 3).
    #[serde(default = "default_true")]
    pub hotspot_enabled: bool,
    #[serde(default = "default_hotspot_con")]
    pub hotspot_connection_name: String,
    /// NM **AP / hotspot** interface when it must differ from the STA iface (e.g. STA on USB `wlan1`,
    /// hotspot on SoC `wlan0`). Empty: same interface as STA ([`effective_wifi_ifname`]).
    #[serde(default)]
    pub hotspot_ifname: String,
    /// Stock UI `hotspot_fallback`: auto-start hotspot when STA link is lost (watchdog / Phase 3).
    #[serde(default)]
    pub hotspot_fallback: bool,
}

fn default_true() -> bool {
    true
}

fn default_hotspot_con() -> String {
    NM_CON_HOTSPOT.to_string()
}

impl Default for FallbackIntent {
    fn default() -> Self {
        Self {
            hotspot_enabled: true,
            hotspot_connection_name: default_hotspot_con(),
            hotspot_ifname: String::new(),
            hotspot_fallback: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkIntent {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub ethernet: EthernetIntent,
    #[serde(default)]
    pub wifi: WifiIntent,
    #[serde(default)]
    pub fallback: FallbackIntent,
}

fn default_version() -> u32 {
    1
}

impl Default for NetworkIntent {
    fn default() -> Self {
        Self {
            version: 1,
            ethernet: EthernetIntent::default(),
            wifi: WifiIntent::default(),
            fallback: FallbackIntent::default(),
        }
    }
}

impl NetworkIntent {
    /// Load from disk, or defaults when missing / empty.
    pub fn load() -> Self {
        let path = intent_path();
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!(
                    "{} no {}, using defaults",
                    crate::log_tags::EVO_NET,
                    path.display()
                );
                return Self::default();
            }
            Err(e) => {
                tracing::warn!(
                    "{} read {}: {} — using defaults",
                    crate::log_tags::EVO_NET,
                    path.display(),
                    e
                );
                return Self::default();
            }
        };
        if raw.trim().is_empty() {
            return Self::default();
        }
        match toml::from_str::<NetworkIntent>(&raw) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    "{} parse {}: {} — using defaults",
                    crate::log_tags::EVO_NET,
                    path.display(),
                    e
                );
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let dir = network_settings_dir();
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let path = intent_path();
        let s = toml::to_string_pretty(self).context("serialize network intent")?;
        fs::write(&path, s).with_context(|| format!("write {}", path.display()))?;
        tracing::info!(
            "{} wrote {}",
            crate::log_tags::EVO_NET,
            path.display()
        );
        Ok(())
    }
}

pub fn read_secret_file(path: &Path) -> Option<String> {
    let s = fs::read_to_string(path).ok()?;
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// Writes STA PSK sidecar; empty string removes the file.
pub fn write_wifi_sta_psk(psk: &str) -> Result<()> {
    write_psk_file(&wifi_sta_psk_path(), psk)
}

/// Writes AP passphrase sidecar; empty string removes the file.
pub fn write_wifi_ap_psk(psk: &str) -> Result<()> {
    write_psk_file(&wifi_ap_psk_path(), psk)
}

fn write_psk_file(path: &Path, psk: &str) -> Result<()> {
    if psk.trim().is_empty() {
        let _ = fs::remove_file(path);
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, format!("{}\n", psk.trim())).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    tracing::info!("{} wrote {}", crate::log_tags::EVO_NET, path.display());
    Ok(())
}

pub fn wifi_sta_psk_configured() -> bool {
    read_secret_file(&wifi_sta_psk_path()).is_some()
}

pub fn wifi_ap_psk_configured() -> bool {
    read_secret_file(&wifi_ap_psk_path()).is_some()
}

/// Resolved interface for NM: explicit `wifi.ifname` in intent wins; otherwise Evo [`crate::config::Config::wifi_iface_resolved`].
pub fn effective_wifi_ifname(wifi: &WifiIntent, config: Option<&crate::config::Config>) -> String {
    let t = wifi.ifname.trim();
    if !t.is_empty() {
        return t.to_string();
    }
    config
        .map(|c| c.wifi_iface_resolved())
        .unwrap_or_else(|| DEFAULT_WIFI_IFACE.to_string())
}

/// Interface for **AP / fallback hotspot** profiles. Set when the STA radio (e.g. USB) is **not** AP-capable or must stay in STA only; otherwise same as [`effective_wifi_ifname`].
pub fn effective_hotspot_ifname(intent: &NetworkIntent, config: Option<&crate::config::Config>) -> String {
    let t = intent.fallback.hotspot_ifname.trim();
    if !t.is_empty() {
        return t.to_string();
    }
    effective_wifi_ifname(&intent.wifi, config)
}
