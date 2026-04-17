//! Configuration for Volumio Evo.
//!
//! **Logging:** `log_level` in `config.toml` sets the default filter when **`RUST_LOG`** is unset.
//! **`RUST_LOG`** wins if set (full `tracing-subscriber` directive). **`VOLUMIO_EVO_LOG_LEVEL`**
//! overrides the file value (one of: `error`, `warn`, `info`, `verbose`, `debug`, `trace`).

use std::path::PathBuf;

use serde::Deserialize;

/// Log verbosity for [`Config::log_level`]. Becomes the default `tracing-subscriber` env filter when **`RUST_LOG`** is unset.
#[derive(Debug, Clone, Copy, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    /// More detail in Evo crates; dependencies stay at `info`.
    Verbose,
    Debug,
    Trace,
}

impl LogLevel {
    /// Parse from env / CLI (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Self::Error),
            "warn" | "warning" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "verbose" => Some(Self::Verbose),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }

    /// `tracing_subscriber::EnvFilter` directive when **`RUST_LOG`** is not set.
    pub fn env_filter_directive(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            // Evo code at debug; custom targets use `volumio_evo::`; crate modules use `volumio_evo_core::`.
            Self::Verbose => {
                "info,volumio_evo_core=debug,volumio_evo=debug,mpd_protocol=info,socketioxide=info,axum=info,tower_http=info,h2=info,hyper=info"
            }
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

/// Evo-owned layout for music sources. MPD's `music_directory` must be set to
/// `music_root` so that paths like `INTERNAL/`, `USB/`, `NAS/`, `SMB/` exist under it
/// (as dirs or symlinks), matching stock Volumio browse URIs. Works on vanilla Debian.
#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)] // used by config file and future init/ensure-layout
pub struct MusicSourcesConfig {
    /// Base path for music. MPD must use this as music_directory.
    /// Under it: INTERNAL, USB, NAS, SMB (create or symlink per deployment).
    #[serde(default = "default_music_root")]
    pub music_root: PathBuf,
    /// Optional path for on-device storage. If set, installer/startup should ensure
    /// `music_root/INTERNAL` exists and points here (e.g. symlink).
    pub local: Option<PathBuf>,
    /// Optional path for USB media (e.g. /media). music_root/usb can symlink here.
    pub usb: Option<PathBuf>,
    /// Optional path for NAS mount. music_root/nas can be a mount point or symlink.
    pub nas: Option<PathBuf>,
    /// Optional path for SMB mount. music_root/smb can be a mount point or symlink.
    pub smb: Option<PathBuf>,
}

fn default_music_root() -> PathBuf {
    PathBuf::from("/var/lib/volumio-evo/music")
}

/// User-aware default when no config file exists: prefer XDG_DATA_HOME or HOME
/// so a non-volumio user gets a writable path. Install/first-run can set
/// VOLUMIO_EVO_MUSIC_ROOT instead.
fn user_or_system_music_root_default() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        let p = PathBuf::from(xdg).join("volumio-evo").join("music");
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home).join(".local").join("share").join("volumio-evo").join("music");
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    default_music_root()
}

/// Top-level directory names under `music_root` / browse URIs `music-library/<SEGMENT>/...`.
/// Segments match Node `stickingMusicLibrary` + `lsinfo` (`INTERNAL`, not `local`).
pub const MUSIC_SOURCE_NAMES: &[(&str, &str)] = &[
    ("INTERNAL", "INTERNAL"),
    ("USB", "USB"),
    ("NAS", "NAS"),
    ("SMB", "SMB"),
];

/// User interface layout (stock `volumioUisList.json` uses the same `uiName` strings).
/// Static files (nginx) must match; Evo only records intent here until multi-root install is automated.
#[derive(Debug, Clone, Deserialize)]
pub struct UiConfig {
    /// `manifest` | `contemporary` | `classic`
    #[serde(default = "default_ui_active_layout")]
    pub active_layout: String,
}

fn default_ui_active_layout() -> String {
    "contemporary".to_string()
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            active_layout: default_ui_active_layout(),
        }
    }
}

/// Optional API keys for online album-art providers. Used when fetching art
/// for large libraries (e.g. 10k+ tracks) to stay within rate limits or enable access.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AlbumArtProvidersConfig {
    /// Last.fm API key (album.getinfo, artist.getinfo). Get one at https://www.last.fm/api/account/create.
    /// Env override: `VOLUMIO_EVO_LASTFM_API_KEY`.
    /// Strongly recommended for browse **album/artist story** text (`POST /api/v1/pluginEndpoint`, metavolumio);
    /// without it, Evo falls back to Wikipedia (needs outbound HTTPS and a working DNS path).
    pub lastfm_api_key: Option<String>,
    /// User-Agent for MusicBrainz / Cover Art Archive (required by their API policy).
    /// Env override: VOLUMIO_EVO_MUSICBRAINZ_USER_AGENT.
    pub musicbrainz_user_agent: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    /// Default log filter when **`RUST_LOG`** is unset (`error` … `trace`; see [`LogLevel`]).
    #[serde(default)]
    pub log_level: LogLevel,
    /// Bind address for the HTTP/WebSocket server.
    #[serde(default = "default_bind")]
    pub bind: String,
    /// Directory containing WASM plugin modules.
    #[serde(default = "default_plugin_dir")]
    #[allow(dead_code)]
    pub plugin_dir: PathBuf,
    /// MPD host.
    #[serde(default = "default_mpd_host")]
    #[allow(dead_code)]
    pub mpd_host: String,
    /// MPD port.
    #[serde(default)]
    pub mpd_port: u16,
    /// Music sources layout (local, usb, nas, smb). Evo drives this; MPD uses music_root.
    #[serde(default)]
    #[allow(dead_code)]
    pub music_sources: MusicSourcesConfig,
    /// Root for album art cache and personal uploads (folder, metadata, web, personal).
    #[serde(default = "default_albumart_root")]
    pub albumart_root: PathBuf,
    /// Optional API keys for online album-art providers (Last.fm, MusicBrainz User-Agent, etc.).
    #[serde(default)]
    pub albumart_providers: AlbumArtProvidersConfig,
    /// Path to exiftool for extracting embedded album art (metadata=true). Env: VOLUMIO_EVO_EXIFTOOL_PATH.
    #[serde(default = "default_exiftool_path")]
    pub exiftool_path: PathBuf,
    /// Which stock UI layout is active (manifest / contemporary / classic).
    #[serde(default)]
    pub ui: UiConfig,
    /// Primary Wi‑Fi interface for NM (scan, apply when `intent.toml` has empty `wifi.ifname`).
    /// When omitted, Evo **auto-picks** a `wifi` device from NetworkManager (preferring one not in
    /// `unavailable` state when several radios exist). Raspberry Pi 3-class **on-SoC Wi‑Fi is weak**;
    /// set to the USB dongle iface (often `wlan1`) to force that radio.
    /// Override env: **`VOLUMIO_EVO_WIFI_IFACE`** (wins over this file after load).
    #[serde(default)]
    pub wifi_iface: Option<String>,
}

impl Config {
    /// Sync fallback Wi‑Fi interface name (default `wlan0`). Async code should prefer
    /// [`crate::nm_network::resolve_effective_wifi_iface`] so NM device state can steer the choice.
    pub fn wifi_iface_resolved(&self) -> String {
        if let Some(ref s) = self.wifi_iface {
            let t = s.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
        // Keep literal here to avoid `config` ↔ `network_config` coupling (same as `network_config::DEFAULT_WIFI_IFACE`).
        "wlan0".to_string()
    }
}

fn default_exiftool_path() -> PathBuf {
    PathBuf::from("/usr/bin/exiftool")
}

fn default_bind() -> String {
    "0.0.0.0:3000".to_string()
}

fn default_plugin_dir() -> PathBuf {
    PathBuf::from("/usr/share/volumio-evo/plugins")
}

fn default_albumart_root() -> PathBuf {
    PathBuf::from("/var/lib/volumio-evo/albumart")
}

fn default_mpd_host() -> String {
    "127.0.0.1".to_string()
}

/// Load config from file and env. Path: VOLUMIO_EVO_CONFIG or default paths.
/// music_root: set at install or first run via VOLUMIO_EVO_MUSIC_ROOT, or in config;
/// when no config file exists, a user-aware default is used (XDG_DATA_HOME or
/// HOME/.local/share/volumio-evo/music) so a different-than-volumio user can run.
pub fn load() -> anyhow::Result<Config> {
    let path = std::env::var("VOLUMIO_EVO_CONFIG")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            [
                "/etc/volumio-evo/config.toml",
                "config/volumio-evo.toml",
            ]
            .into_iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
        });

    let from_file = path.is_some();
    let mut config: Config = if let Some(p) = path {
        let s = std::fs::read_to_string(&p)?;
        toml::from_str(&s)?
    } else {
        Config::default()
    };

    if config.mpd_port == 0 {
        config.mpd_port = 6600;
    }

    // music_root: env overrides; when no config file, use user-aware default
    if let Ok(env_root) = std::env::var("VOLUMIO_EVO_MUSIC_ROOT") {
        config.music_sources.music_root = PathBuf::from(env_root);
    } else if !from_file {
        config.music_sources.music_root = user_or_system_music_root_default();
    }

    if config.albumart_root.as_os_str().is_empty() {
        config.albumart_root = default_albumart_root();
    }
    if let Ok(env_albumart) = std::env::var("VOLUMIO_EVO_ALBUMART_ROOT") {
        config.albumart_root = PathBuf::from(env_albumart);
    }

    // Album-art provider keys: env overrides (so keys can be set without editing config file)
    if let Ok(k) = std::env::var("VOLUMIO_EVO_LASTFM_API_KEY") {
        if !k.is_empty() {
            config.albumart_providers.lastfm_api_key = Some(k);
        }
    }
    if let Ok(ua) = std::env::var("VOLUMIO_EVO_MUSICBRAINZ_USER_AGENT") {
        if !ua.is_empty() {
            config.albumart_providers.musicbrainz_user_agent = Some(ua);
        }
    }

    if let Ok(p) = std::env::var("VOLUMIO_EVO_EXIFTOOL_PATH") {
        if !p.is_empty() {
            config.exiftool_path = PathBuf::from(p);
        }
    }

    if let Ok(v) = std::env::var("VOLUMIO_EVO_ACTIVE_LAYOUT") {
        if !v.is_empty() {
            config.ui.active_layout = v;
        }
    }
    if let Ok(v) = std::env::var("VOLUMIO_EVO_WIFI_IFACE") {
        if !v.trim().is_empty() {
            config.wifi_iface = Some(v);
        }
    }
    if let Ok(s) = std::env::var("VOLUMIO_EVO_LOG_LEVEL") {
        if let Some(l) = LogLevel::parse(&s) {
            config.log_level = l;
        } else {
            eprintln!(
                "VOLUMIO_EVO_LOG_LEVEL={s:?} is not a valid log level (error|warn|info|verbose|debug|trace); ignoring"
            );
        }
    }

    Ok(config)
}

/// Run after [`tracing_subscriber`] is initialized so [`normalize_ui_active_layout`] warnings are recorded.
pub fn finalize_loaded_config(config: &mut Config) {
    normalize_ui_active_layout(&mut config.ui);
}

const UI_LAYOUT_NAMES: &[&str] = &["manifest", "contemporary", "classic"];

fn normalize_ui_active_layout(ui: &mut UiConfig) {
    let s = ui.active_layout.trim().to_lowercase();
    if UI_LAYOUT_NAMES.contains(&s.as_str()) {
        ui.active_layout = s;
        return;
    }
    tracing::warn!(
        "{} ui.active_layout {:?} is not one of {:?}; using {:?}",
        crate::log_tags::EVO_CONFIG,
        ui.active_layout,
        UI_LAYOUT_NAMES,
        default_ui_active_layout()
    );
    ui.active_layout = default_ui_active_layout();
}

#[cfg(test)]
mod log_level_tests {
    use super::LogLevel;

    #[test]
    fn parse_case_insensitive() {
        assert_eq!(LogLevel::parse("INFO"), Some(LogLevel::Info));
        assert_eq!(LogLevel::parse("Verbose"), Some(LogLevel::Verbose));
        assert_eq!(LogLevel::parse("bogus"), None);
    }

    #[test]
    fn env_filter_directives_are_valid() {
        for l in [
            LogLevel::Error,
            LogLevel::Warn,
            LogLevel::Info,
            LogLevel::Verbose,
            LogLevel::Debug,
            LogLevel::Trace,
        ] {
            let d = l.env_filter_directive();
            tracing_subscriber::EnvFilter::new(d);
        }
    }
}
