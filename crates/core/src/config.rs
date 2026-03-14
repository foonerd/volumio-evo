//! Configuration for Volumio Evo.

use std::path::PathBuf;

use serde::Deserialize;

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
}

fn default_bind() -> String {
    "0.0.0.0:3000".to_string()
}

fn default_plugin_dir() -> PathBuf {
    PathBuf::from("/usr/share/volumio-evo/plugins")
}

fn default_mpd_host() -> String {
    "127.0.0.1".to_string()
}

/// Load config from file and env. Path: VOLUMIO_EVO_CONFIG or default paths.
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

    let mut config: Config = if let Some(p) = path {
        let s = std::fs::read_to_string(&p)?;
        toml::from_str(&s)?
    } else {
        Config::default()
    };

    if config.mpd_port == 0 {
        config.mpd_port = 6600;
    }

    Ok(config)
}
