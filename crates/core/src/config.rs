//! Configuration for Volumio Evo.

use std::path::PathBuf;

use serde::Deserialize;

/// Evo-owned layout for music sources. MPD's `music_directory` must be set to
/// `music_root` so that paths like `local/`, `usb/`, `nas/`, `smb/` exist under it
/// (as dirs or symlinks). Works on vanilla Debian (no Volumio overlayfs).
#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)] // used by config file and future init/ensure-layout
pub struct MusicSourcesConfig {
    /// Base path for music. MPD must use this as music_directory.
    /// Under it: local, usb, nas, smb (create or symlink per deployment).
    #[serde(default = "default_music_root")]
    pub music_root: PathBuf,
    /// Optional path for "local" (on-device) storage. If set, installer/startup
    /// should ensure music_root/local exists and points here (e.g. symlink).
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

/// Default source names under music_root. URI prefix: music-library/<name>/...
pub const MUSIC_SOURCE_NAMES: &[(&str, &str)] = &[
    ("local", "Local"),
    ("usb", "USB"),
    ("nas", "NAS"),
    ("smb", "SMB"),
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
    /// Env override: VOLUMIO_EVO_LASTFM_API_KEY.
    pub lastfm_api_key: Option<String>,
    /// User-Agent for MusicBrainz / Cover Art Archive (required by their API policy).
    /// Env override: VOLUMIO_EVO_MUSICBRAINZ_USER_AGENT.
    pub musicbrainz_user_agent: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Config {
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
    normalize_ui_active_layout(&mut config.ui);

    Ok(config)
}

const UI_LAYOUT_NAMES: &[&str] = &["manifest", "contemporary", "classic"];

fn normalize_ui_active_layout(ui: &mut UiConfig) {
    let s = ui.active_layout.trim().to_lowercase();
    if UI_LAYOUT_NAMES.contains(&s.as_str()) {
        ui.active_layout = s;
        return;
    }
    tracing::warn!(
        "ui.active_layout {:?} is not one of {:?}; using {:?}",
        ui.active_layout,
        UI_LAYOUT_NAMES,
        default_ui_active_layout()
    );
    ui.active_layout = default_ui_active_layout();
}
