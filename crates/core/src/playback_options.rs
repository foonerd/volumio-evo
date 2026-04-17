//! Stock Playback Options (Node `music_service/mpd` + `alsa_controller`): persist and apply to
//! `/etc/volumio-evo/mpd.conf` (included from main `/etc/mpd.conf`).
//!
//! Persisted playback/MPD options: **`settings/mpd/playback.toml`** (see `paths::default_mpd_playback_path`).
//! ALSA output state: **`settings/alsa/state.toml`**. Catalog JSON stays under `/usr/share/volumio-evo/alsa/`.
//! Volume curve / max volume / steps match Node config for UI parity. **Default startup volume**
//! (`volumestart`) applies **ALSA `amixer`** then **MPD `setvol`** on Evo boot when not `disabled` and
//! mixer type is not `None` (Node: `volumecontrol.setStartupVolume`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::alsa::AlsaSettings;
use crate::paths;

/// Path to Evo MPD fragment (see bootstrap `EVO_MPD_FRAGMENT`).
pub const MPD_FRAGMENT_PATH: &str = "/etc/volumio-evo/mpd.conf";

fn state_path() -> PathBuf {
    std::env::var("VOLUMIO_EVO_PLAYBACK_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| paths::default_mpd_playback_path())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlaybackOptions {
    /// DSD DoP vs native (DSD direct).
    #[serde(default)]
    pub dop: bool,
    #[serde(default)]
    pub dsd_autovolume: bool,
    #[serde(default)]
    pub volume_normalization: bool,
    /// MPD `audio_buffer_size` in kilobytes (UI labels: 1–12 MB).
    #[serde(default = "default_audio_buffer_kb")]
    pub audio_buffer_size: u32,
    /// Stock Volumio default is `10%` (`music_service/mpd/config.json`). Not written to `mpd.conf`:
    /// MPD 0.24+ deprecates global `buffer_before_play` (fixed internal pre-buffer behaviour).
    #[serde(default = "default_buffer_before")]
    pub buffer_before_play: String,
    /// Node uses this for `/data/queue`; Evo uses MPD queue—value is stored for UI parity.
    #[serde(default = "default_true")]
    pub persistent_queue: bool,
    #[serde(default)]
    pub iso: bool,
    /// `continuous` | `single` (Node `playback_mode_list`).
    #[serde(default = "default_playback_mode")]
    pub playback_mode_list: String,

    #[serde(default = "default_mixer_software")]
    pub mixer_type: String,
    #[serde(default)]
    pub mixer: String,
    #[serde(default = "default_vol_start")]
    pub volumestart: String,
    #[serde(default = "default_vol_max")]
    pub volumemax: String,
    #[serde(default = "default_vol_steps")]
    pub volumesteps: String,
    #[serde(default = "default_volumecurve")]
    pub volumecurvemode: String,
    #[serde(default)]
    pub mpdvolume: bool,

    #[serde(default)]
    pub resampling: bool,
    #[serde(default = "default_star")]
    pub resampling_target_bitdepth: String,
    #[serde(default = "default_star")]
    pub resampling_target_samplerate: String,
    #[serde(default = "default_quality")]
    pub resampling_quality: String,
}

fn default_audio_buffer_kb() -> u32 {
    2048
}

fn default_buffer_before() -> String {
    "10%".to_string()
}

fn default_true() -> bool {
    true
}

fn default_playback_mode() -> String {
    "continuous".to_string()
}

fn default_mixer_software() -> String {
    "Software".to_string()
}

fn default_vol_start() -> String {
    "disabled".to_string()
}

fn default_vol_max() -> String {
    "100".to_string()
}

fn default_vol_steps() -> String {
    "10".to_string()
}

fn default_volumecurve() -> String {
    "logarithmic".to_string()
}

fn default_star() -> String {
    "*".to_string()
}

fn default_quality() -> String {
    "high".to_string()
}

fn parse_volume_percent_0_100(s: &str) -> Option<u8> {
    let n: u32 = s.trim().parse().ok()?;
    Some(n.min(100) as u8)
}

impl Default for PlaybackOptions {
    fn default() -> Self {
        Self {
            dop: false,
            dsd_autovolume: false,
            volume_normalization: false,
            audio_buffer_size: default_audio_buffer_kb(),
            buffer_before_play: default_buffer_before(),
            persistent_queue: true,
            iso: false,
            playback_mode_list: default_playback_mode(),
            mixer_type: default_mixer_software(),
            mixer: String::new(),
            volumestart: default_vol_start(),
            volumemax: default_vol_max(),
            volumesteps: default_vol_steps(),
            volumecurvemode: default_volumecurve(),
            mpdvolume: false,
            resampling: false,
            resampling_target_bitdepth: default_star(),
            resampling_target_samplerate: default_star(),
            resampling_quality: default_quality(),
        }
    }
}

impl PlaybackOptions {
    /// Target 0–100 for MPD `setvol` on boot, or `None` if startup volume is off or inapplicable.
    /// Caps by **`volumemax`** when that parses as a percentage (Node stores both; Evo applies cap here).
    pub fn startup_volume_percent_for_mpd(&self) -> Option<u8> {
        let s = self.volumestart.trim();
        if s.is_empty() || s.eq_ignore_ascii_case("disabled") {
            return None;
        }
        if self.mixer_type == "None" {
            return None;
        }
        let v = parse_volume_percent_0_100(s)?;
        let cap = parse_volume_percent_0_100(&self.volumemax).unwrap_or(100);
        Some(v.min(cap))
    }

    /// Clamp a UI volume request to **`volumemax`** (Node `alsavolume` default branch / step cases).
    pub fn clamp_volume_percent(&self, v: u8) -> u8 {
        let cap = parse_volume_percent_0_100(&self.volumemax).unwrap_or(100);
        v.min(cap)
    }

    /// Align persisted volume/mixer fields with ALSA reality (Node never emits hardware MPD mixer
    /// lines unless `mixer` + `mpdvolume`; UI only offers Hardware when `getMixerControls` is non-empty).
    pub fn apply_volume_sanity(&mut self, alsa: &AlsaSettings) {
        if self.mixer_type != "Hardware" {
            return;
        }
        let controls = crate::alsa::list_playback_mixer_controls(&alsa.output_device_id);
        let trimmed = self.mixer.trim();
        if controls.is_empty() {
            tracing::warn!(
                card = %alsa.output_device_id,
                "{} Hardware mixer requested but this card exposes no ALSA Playback controls; switching mixer_type to Software (see Node getMixerControls)",
                crate::log_tags::EVO_PLAYBACK
            );
            self.mixer_type = "Software".to_string();
            self.mixer.clear();
            return;
        }
        if !trimmed.is_empty() && !controls.iter().any(|c| c == trimmed) {
            tracing::warn!(
                card = %alsa.output_device_id,
                requested = %trimmed,
                available = ?controls,
                "{} Hardware mixer control name not found on card; switching mixer_type to Software",
                crate::log_tags::EVO_PLAYBACK
            );
            self.mixer_type = "Software".to_string();
            self.mixer.clear();
        }
    }

    pub fn load() -> Self {
        let path = state_path();
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        toml::from_str(&raw).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = state_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = toml::to_string_pretty(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })?;
        std::fs::write(&path, s)
    }

    /// Merge Node `savePlaybackOptions` payload.
    pub fn merge_playback_section(&mut self, data: &serde_json::Value) {
        if let Some(v) = data.get("dop").and_then(|x| x.as_bool()) {
            self.dop = v;
        }
        if let Some(v) = data.get("dsd_autovolume").and_then(|x| x.as_bool()) {
            self.dsd_autovolume = v;
        }
        if let Some(v) = data.get("volume_normalization").and_then(|x| x.as_bool()) {
            self.volume_normalization = v;
        }
        if let Some(v) = data.get("audio_buffer_size").and_then(|x| x.get("value")) {
            if let Some(n) = v.as_u64() {
                self.audio_buffer_size = n as u32;
            } else if let Some(n) = v.as_i64() {
                self.audio_buffer_size = n as u32;
            }
        }
        if let Some(v) = data.get("buffer_before_play").and_then(|x| x.get("value")) {
            if let Some(s) = v.as_str() {
                self.buffer_before_play = s.to_string();
            }
        }
        if let Some(v) = data.get("persistent_queue").and_then(|x| x.as_bool()) {
            self.persistent_queue = v;
        }
        if let Some(v) = data.get("iso").and_then(|x| x.as_bool()) {
            self.iso = v;
        }
        if let Some(v) = data.get("playback_mode_list").and_then(|x| x.get("value")) {
            if let Some(s) = v.as_str() {
                self.playback_mode_list = s.to_string();
            }
        }
    }

    /// Merge Node `saveVolumeOptions` payload.
    pub fn merge_volume_section(&mut self, data: &serde_json::Value) {
        if let Some(v) = data.get("mixer_type").and_then(|x| x.get("value")) {
            if let Some(s) = v.as_str() {
                self.mixer_type = s.to_string();
            }
        }
        if let Some(v) = data.get("mixer").and_then(|x| x.get("value")) {
            if let Some(s) = v.as_str() {
                self.mixer = s.to_string();
            }
        }
        if let Some(v) = data.get("volumestart").and_then(|x| x.get("value")) {
            if let Some(s) = v.as_str() {
                self.volumestart = s.to_string();
            }
        }
        if let Some(v) = data.get("volumemax").and_then(|x| x.get("value")) {
            if let Some(s) = v.as_str() {
                self.volumemax = s.to_string();
            }
        }
        if let Some(v) = data.get("volumesteps").and_then(|x| x.get("value")) {
            if let Some(s) = v.as_str() {
                self.volumesteps = s.to_string();
            }
        }
        if let Some(v) = data.get("volumecurvemode").and_then(|x| x.get("value")) {
            if let Some(s) = v.as_str() {
                self.volumecurvemode = s.to_string();
            }
        }
        if let Some(v) = data.get("mpdvolume").and_then(|x| x.as_bool()) {
            self.mpdvolume = v;
        }
    }

    /// Merge Node `saveResamplingOpts` payload.
    pub fn merge_resampling_section(&mut self, data: &serde_json::Value) {
        if let Some(v) = data.get("resampling").and_then(|x| x.as_bool()) {
            self.resampling = v;
        }
        if let Some(v) = data.get("resampling_target_bitdepth").and_then(|x| x.get("value")) {
            if let Some(s) = v.as_str() {
                self.resampling_target_bitdepth = s.to_string();
            }
        }
        if let Some(v) = data.get("resampling_target_samplerate").and_then(|x| x.get("value")) {
            if let Some(s) = v.as_str() {
                self.resampling_target_samplerate = s.to_string();
            }
        }
        if let Some(v) = data.get("resampling_quality").and_then(|x| x.get("value")) {
            if let Some(s) = v.as_str() {
                self.resampling_quality = s.to_string();
            }
        }
    }

    fn yn(b: bool) -> &'static str {
        if b {
            "yes"
        } else {
            "no"
        }
    }

    fn dop_str(&self) -> &'static str {
        if self.dop {
            "yes"
        } else {
            "no"
        }
    }

    /// Node `createMPDFile`: `mixer_device` / `mixer_control` / `mixer_type "hardware"` only when
    /// `mixer.length > 0 && mpdvolume`. Otherwise `${mixer}` is empty — no implicit PCM default.
    ///
    /// When this is true, MPD's **`setvol`** drives the **same** ALSA Playback control as Evo's
    /// Hardware mixer path (`amixer`). Calling both back-to-back makes MPD fight the level Evo just
    /// set (often resetting to 0). Stock Volumio applies Hardware volume via **ALSA only**
    /// (`volumecontrol.js`); Evo skips redundant `setvol` in that configuration.
    pub fn mpd_shares_alsa_hardware_mixer(&self, alsa: &AlsaSettings) -> bool {
        self.mpd_use_hardware_mixer_block(alsa).is_some()
    }

    fn mpd_use_hardware_mixer_block(&self, alsa: &AlsaSettings) -> Option<(String, String)> {
        if self.mixer_type != "Hardware" || !self.mpdvolume {
            if self.mixer_type == "Hardware" && !self.mpdvolume {
                tracing::info!(
                    "{} mixer_type Hardware but mpdvolume disabled: omitting hardware mixer block (matches Node createMPDFile)",
                    crate::log_tags::EVO_PLAYBACK
                );
            }
            return None;
        }
        let control = self.mixer.trim();
        if control.is_empty() {
            tracing::warn!(
                "{} mixer_type Hardware with mpdvolume but no mixer_control set; using MPD software volume",
                crate::log_tags::EVO_PLAYBACK
            );
            return None;
        }
        let available = crate::alsa::list_playback_mixer_controls(&alsa.output_device_id);
        if !available.iter().any(|c| c == control) {
            tracing::warn!(
                card = %alsa.output_device_id,
                control = %control,
                available = ?available,
                "{} hardware mixer_control not present on card; using MPD software volume (avoids default PCM lookup)",
                crate::log_tags::EVO_PLAYBACK
            );
            return None;
        }
        let dev = crate::alsa::mixer_device_for_mpd(alsa);
        Some((dev, control.to_string()))
    }

    fn audio_output_format_line(&self) -> Option<String> {
        if !self.resampling {
            return None;
        }
        let d = self.resampling_target_bitdepth.trim();
        let r = self.resampling_target_samplerate.trim();
        if d == "*" || r == "*" || d.is_empty() || r.is_empty() {
            return None;
        }
        Some(format!("\tformat\t\t\"{r}:{d}:2\""))
    }

    fn resampler_block(&self) -> String {
        let q = self.resampling_quality.as_str();
        let threads = "0";
        format!(
            r#"resampler {{
		plugin		"soxr"
		quality		"{q}"
		threads		"{threads}"
}}"#
        )
    }

    /// Full fragment file: globals + resampler + audio_output (matches Node template subset).
    pub fn render_mpd_fragment(&self, alsa: &AlsaSettings) -> String {
        let dev = crate::alsa::mpd_playback_device(alsa);
        let dop = self.dop_str();
        let vol_norm = Self::yn(self.volume_normalization);
        let buf = self.audio_buffer_size;
        let resampler = self.resampler_block();
        let fmt = self
            .audio_output_format_line()
            .unwrap_or_default();

        let hw = self.mpd_use_hardware_mixer_block(alsa);
        let mixer_type_single = if hw.is_some() {
            None
        } else if self.mixer_type == "None" {
            Some("none")
        } else {
            Some("software")
        };

        // Do not emit `buffer_before_play`: deprecated in MPD 0.24+ (see mpd log). Value remains in
        // playback.toml for UI parity with stock Volumio only.
        let mut globals = format!(
            r#"# Generated by volumio-evo — do not edit by hand (Playback Options).

volume_normalization		"{vol_norm}"
audio_buffer_size		"{buf}"

"#
        );
        globals.push_str(&resampler);
        globals.push_str("\n\naudio_output {\n");
        globals.push_str("\t\ttype\t\t\"alsa\"\n");
        globals.push_str("\t\tname\t\t\"volumio-evo\"\n");
        globals.push_str(&format!("\t\tdevice\t\t\"{dev}\"\n"));
        globals.push_str(&format!("\t\tdop\t\t\"{dop}\"\n"));
        if let Some((mixer_dev, mixer_ctl)) = &hw {
            globals.push_str(&format!("\t\tmixer_device\t\"{mixer_dev}\"\n"));
            globals.push_str(&format!("\t\tmixer_control\t\"{mixer_ctl}\"\n"));
            globals.push_str("\t\tmixer_type\t\"hardware\"\n");
        } else if let Some(t) = mixer_type_single {
            // `mixer_device` / `mixer_control` are only for MPD’s **hardware** ALSA mixer plugin.
            // With `mixer_type` `software` or `none`, MPD 0.24+ rejects `mixer_device` ("not recognized").
            globals.push_str(&format!("\t\tmixer_type\t\"{t}\"\n"));
        }
        if !fmt.is_empty() {
            globals.push_str(&fmt);
            globals.push('\n');
        }
        globals.push_str("}\n");
        globals
    }

    /// Write fragment and restart MPD (requires write access to the fragment + permission to reload MPD).
    pub async fn write_fragment_and_restart_mpd(&self, alsa: &AlsaSettings) -> anyhow::Result<()> {
        let path = std::env::var("VOLUMIO_EVO_MPD_FRAGMENT").unwrap_or_else(|_| MPD_FRAGMENT_PATH.to_string());
        let body = self.render_mpd_fragment(alsa);
        let parent = std::path::Path::new(&path).parent().unwrap_or(std::path::Path::new("/"));
        std::fs::create_dir_all(parent)?;
        std::fs::write(&path, body)?;
        restart_mpd_after_fragment_write().await
    }
}

/// Effective UID on Linux (`/proc/self/status`). Used to avoid a doomed `systemctl` call as non-root,
/// which still emits **Failed to restart mpd.service: Interactive authentication required** in the journal.
/// Privilege contract: `docs/OS_PRIVILEGE_MODEL.md`.
#[cfg(target_os = "linux")]
fn linux_effective_uid() -> Option<u32> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in s.lines() {
        let line = line.trim_start();
        let rest = line.strip_prefix("Uid:")?;
        let eff = rest.split_whitespace().nth(1)?.parse().ok()?;
        return Some(eff);
    }
    None
}

/// When true, only `sudo -n … systemctl` is used (non-root cannot restart units without auth).
fn restart_mpd_use_sudo_only() -> bool {
    #[cfg(target_os = "linux")]
    if let Some(uid) = linux_effective_uid() {
        return uid != 0;
    }
    // Non-Linux dev builds, or if /proc is unreadable: match bootstrap’s non-root drop-in.
    std::env::var("VOLUMIO_EVO_RUNTIME_USER")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

/// Reload MPD so it picks up the Evo fragment. Non-root Evo uses `sudo -n` with bootstrap sudoers.
async fn restart_mpd_after_fragment_write() -> anyhow::Result<()> {
    let systemctl = std::env::var("VOLUMIO_EVO_SYSTEMCTL").unwrap_or_else(|_| "/usr/bin/systemctl".to_string());

    if restart_mpd_use_sudo_only() {
        let sudo = tokio::process::Command::new("/usr/bin/sudo")
            .arg("-n")
            .arg(&systemctl)
            .args(["restart", "mpd"])
            .status()
            .await
            .map_err(|e| anyhow::anyhow!("sudo {systemctl} restart mpd: {e}"))?;
        if !sudo.success() {
            anyhow::bail!(
                "sudo -n {systemctl} restart mpd failed ({sudo}). Non-root Evo needs NOPASSWD for `{systemctl} restart mpd` — re-run bootstrap (see docs/RUNTIME_USER.md)."
            );
        }
        return Ok(());
    }

    let direct = tokio::process::Command::new(&systemctl)
        .args(["restart", "mpd"])
        .status()
        .await
        .map_err(|e| anyhow::anyhow!("{systemctl} restart mpd: {e}"))?;
    if direct.success() {
        return Ok(());
    }
    let sudo = tokio::process::Command::new("/usr/bin/sudo")
        .arg("-n")
        .arg(&systemctl)
        .args(["restart", "mpd"])
        .status()
        .await
        .map_err(|e| anyhow::anyhow!("sudo {systemctl} restart mpd: {e}"))?;
    if !sudo.success() {
        anyhow::bail!(
            "restart mpd failed (direct {direct}; sudo {sudo}). Non-root service needs NOPASSWD for `{systemctl} restart mpd` — re-run bootstrap (see docs/RUNTIME_USER.md)."
        );
    }
    Ok(())
}

/// Three stock sections: General playback, Volume options, Audio resampling (Node UIConfig shape).
/// `mixer_controls`: from [`crate::alsa::list_playback_mixer_controls`] for the active card — when
/// empty, Hardware is omitted (Node `getMixerControls`).
pub fn playback_mpd_ui_sections(
    pb: &PlaybackOptions,
    mixer_controls: &[String],
) -> Vec<serde_json::Value> {
    let dop_label = if pb.dop {
        "DoP (DSD over PCM)"
    } else {
        "DSD Direct"
    };
    let buffer_val = pb.audio_buffer_size;
    let buffer_label = match buffer_val {
        1024 => "1 MB",
        2048 => "2 MB",
        4096 => "4 MB",
        8192 => "8 MB",
        12288 => "12 MB",
        _ => "Custom",
    };
    let playback_mode_label = if pb.playback_mode_list == "single" {
        "Single"
    } else {
        "Continuous"
    };
    let mixer_label = if pb.mixer.is_empty() {
        "—".to_string()
    } else {
        pb.mixer.clone()
    };

    let mut mixer_type_options: Vec<serde_json::Value> = Vec::new();
    if !mixer_controls.is_empty() {
        mixer_type_options.push(serde_json::json!({"value": "Hardware", "label": "Hardware"}));
    }
    mixer_type_options.push(serde_json::json!({"value": "Software", "label": "Software"}));
    mixer_type_options.push(serde_json::json!({"value": "None", "label": "None"}));

    let mixer_select_options: Vec<serde_json::Value> = mixer_controls
        .iter()
        .map(|m| serde_json::json!({ "value": m, "label": m }))
        .collect();

    // Node `alsa_controller` getUIConfig: when mixer_type is None, `hidden: true` on mixer row and
    // volumestart / volumemax / volumesteps / volumecurve / mpdvolume (sections[3].content[1..6]).
    // Stock `isItemVisible` respects `hidden` only — no UI fork required.
    let hide_volume_details = pb.mixer_type == "None";

    vec![
        serde_json::json!({
            "id": "playback_options",
            "element": "section",
            "label": "Playback options",
            "icon": "fa-sliders",
            "onSave": {"type": "controller", "endpoint": "music_service/mpd", "method": "savePlaybackOptions"},
            "saveButton": {
                "label": "Save",
                "data": ["dop", "dsd_autovolume", "volume_normalization", "audio_buffer_size", "buffer_before_play", "persistent_queue", "iso", "playback_mode_list"]
            },
            "content": [
                {
                    "id": "dop",
                    "element": "select",
                    "doc": "DSD transport: native (DSD Direct) or DoP.",
                    "label": "DSD playback mode",
                    "value": {"value": pb.dop, "label": dop_label},
                    "options": [
                        {"value": false, "label": "DSD Direct"},
                        {"value": true, "label": "DoP (DSD over PCM)"}
                    ]
                },
                {
                    "id": "dsd_autovolume",
                    "element": "switch",
                    "doc": "When playing DSD, set volume to 100% (Node behaviour; Evo stores for future use).",
                    "label": "DSD auto volume level",
                    "value": pb.dsd_autovolume
                },
                {
                    "id": "volume_normalization",
                    "element": "switch",
                    "doc": "MPD volume_normalization.",
                    "label": "Volume normalization",
                    "value": pb.volume_normalization
                },
                {
                    "id": "audio_buffer_size",
                    "element": "select",
                    "doc": "MPD audio_buffer_size (kilobytes).",
                    "label": "Audio buffer size",
                    "value": {"value": buffer_val, "label": buffer_label},
                    "options": [
                        {"value": 1024, "label": "1 MB"},
                        {"value": 2048, "label": "2 MB"},
                        {"value": 4096, "label": "4 MB"},
                        {"value": 8192, "label": "8 MB"},
                        {"value": 12288, "label": "12 MB"}
                    ]
                },
                {
                    "id": "buffer_before_play",
                    "element": "select",
                    "doc": "MPD buffer_before_play (if supported).",
                    "label": "Buffer before play",
                    "value": {"value": pb.buffer_before_play, "label": pb.buffer_before_play.clone()},
                    "options": [
                        {"value": "10%", "label": "10%"},
                        {"value": "20%", "label": "20%"},
                        {"value": "30%", "label": "30%"},
                        {"value": "40%", "label": "40%"}
                    ]
                },
                {
                    "id": "persistent_queue",
                    "element": "switch",
                    "doc": "Stock Volumio persists queue in /data/queue; Evo uses the MPD queue. Stored for UI parity.",
                    "label": "Persistent queue",
                    "value": pb.persistent_queue
                },
                {
                    "id": "iso",
                    "element": "switch",
                    "hidden": true,
                    "doc": "ISO/SACD image playback (not available in Evo).",
                    "label": "ISO playback",
                    "value": pb.iso
                },
                {
                    "id": "playback_mode_list",
                    "element": "select",
                    "doc": "Stored for parity with stock Volumio (Node uses PLAYBACK_MODE env).",
                    "label": "Playback mode",
                    "value": {"value": pb.playback_mode_list, "label": playback_mode_label},
                    "options": [
                        {"value": "continuous", "label": "Continuous"},
                        {"value": "single", "label": "Single"}
                    ]
                }
            ]
        }),
        serde_json::json!({
            "id": "volume_options",
            "element": "section",
            "label": "Volume options",
            "icon": "fa-volume-up",
            "onSave": {"type": "controller", "endpoint": "audio_interface/alsa_controller", "method": "saveVolumeOptions"},
            "saveButton": {
                "label": "Save",
                "data": ["mixer_type", "mixer", "volumestart", "volumemax", "volumesteps", "volumecurvemode", "mpdvolume"]
            },
            "content": [
                {
                    "id": "mixer_type",
                    "element": "select",
                    "doc": "Hardware only appears when ALSA reports Playback controls on this card (Node getMixerControls). MPD hardware lines require MPD clients volume + a control name.",
                    "label": "Mixer type",
                    "value": {"value": pb.mixer_type, "label": pb.mixer_type},
                    "options": mixer_type_options
                },
                {
                    "id": "mixer",
                    "element": "select",
                    "hidden": hide_volume_details,
                    "visibleIf": {"field": "mixer_type", "value": "Hardware"},
                    "doc": "ALSA simple control name (amixer). Used for MPD mixer_device/mixer_control when MPD clients volume is on.",
                    "label": "Mixer control name",
                    "value": {"value": pb.mixer, "label": mixer_label},
                    "options": mixer_select_options
                },
                {
                    "id": "volumestart",
                    "element": "select",
                    "hidden": hide_volume_details,
                    "doc": "Applied on Evo boot via MPD setvol when not Disabled (Node: setStartupVolume). Hidden when mixer type is None.",
                    "label": "Default startup volume",
                    "value": {"value": pb.volumestart, "label": pb.volumestart},
                    "options": [
                        {"value": "disabled", "label": "Disabled"},
                        {"value": "5", "label": "5"}, {"value": "10", "label": "10"},
                        {"value": "15", "label": "15"}, {"value": "20", "label": "20"},
                        {"value": "25", "label": "25"}, {"value": "30", "label": "30"},
                        {"value": "35", "label": "35"}, {"value": "40", "label": "40"},
                        {"value": "45", "label": "45"}, {"value": "50", "label": "50"},
                        {"value": "55", "label": "55"}, {"value": "60", "label": "60"},
                        {"value": "65", "label": "65"}, {"value": "70", "label": "70"},
                        {"value": "75", "label": "75"}, {"value": "80", "label": "80"},
                        {"value": "85", "label": "85"}, {"value": "90", "label": "90"},
                        {"value": "95", "label": "95"}, {"value": "100", "label": "100"}
                    ]
                },
                {
                    "id": "volumemax",
                    "element": "select",
                    "hidden": hide_volume_details,
                    "doc": "Maximum volume cap (stored for future volume daemon). Hidden when mixer type is None.",
                    "label": "Max volume level",
                    "value": {"value": pb.volumemax, "label": pb.volumemax},
                    "options": [
                        {"value": "10", "label": "10"}, {"value": "20", "label": "20"},
                        {"value": "30", "label": "30"}, {"value": "40", "label": "40"},
                        {"value": "50", "label": "50"}, {"value": "60", "label": "60"},
                        {"value": "70", "label": "70"}, {"value": "80", "label": "80"},
                        {"value": "90", "label": "90"}, {"value": "100", "label": "100"}
                    ]
                },
                {
                    "id": "volumesteps",
                    "element": "select",
                    "hidden": hide_volume_details,
                    "doc": "One-click volume step size (stored for UI). Hidden when mixer type is None.",
                    "label": "One click volume steps",
                    "value": {"value": pb.volumesteps, "label": pb.volumesteps},
                    "options": [
                        {"value": "1", "label": "1"}, {"value": "2", "label": "2"},
                        {"value": "4", "label": "4"}, {"value": "5", "label": "5"},
                        {"value": "10", "label": "10"}, {"value": "20", "label": "20"}
                    ]
                },
                {
                    "id": "volumecurvemode",
                    "element": "select",
                    "hidden": true,
                    "doc": "Volume curve.",
                    "label": "Volume curve mode",
                    "value": {"value": pb.volumecurvemode, "label": pb.volumecurvemode},
                    "options": [
                        {"value": "logarithmic", "label": "Natural"},
                        {"value": "linear", "label": "Linear"}
                    ]
                },
                {
                    "id": "mpdvolume",
                    "element": "switch",
                    "hidden": hide_volume_details,
                    "doc": "Stock Volumio only writes MPD mixer_device/mixer_control when this is on and a mixer name is set (createMPDFile). Hidden when mixer type is None.",
                    "label": "MPD clients volume control",
                    "value": pb.mpdvolume
                }
            ]
        }),
        serde_json::json!({
            "id": "advanced_twaaks",
            "element": "section",
            "label": "Audio resampling",
            "icon": "fa-tachometer",
            "onSave": {"type": "controller", "endpoint": "audio_interface/alsa_controller", "method": "saveResamplingOpts"},
            "saveButton": {
                "label": "Save",
                "data": ["resampling", "resampling_target_bitdepth", "resampling_target_samplerate", "resampling_quality"]
            },
            "content": [
                {
                    "id": "resampling",
                    "element": "switch",
                    "doc": "Enable explicit output format (soxr resampler block is always written; format line optional).",
                    "label": "Audio resampling",
                    "value": pb.resampling
                },
                {
                    "id": "resampling_target_bitdepth",
                    "element": "select",
                    "visibleIf": {"field": "resampling", "value": true},
                    "doc": "Target bit depth for MPD format string.",
                    "label": "Target bit depth",
                    "value": {"value": pb.resampling_target_bitdepth, "label": pb.resampling_target_bitdepth},
                    "options": [
                        {"value": "*", "label": "Native"},
                        {"value": "16", "label": "16"},
                        {"value": "24", "label": "24"},
                        {"value": "32", "label": "32"}
                    ]
                },
                {
                    "id": "resampling_target_samplerate",
                    "element": "select",
                    "visibleIf": {"field": "resampling", "value": true},
                    "doc": "Target sample rate (Hz) for MPD format string.",
                    "label": "Target sample rate",
                    "value": {"value": pb.resampling_target_samplerate, "label": pb.resampling_target_samplerate},
                    "options": [
                        {"value": "*", "label": "Native"},
                        {"value": "44100", "label": "44100"},
                        {"value": "48000", "label": "48000"},
                        {"value": "88200", "label": "88200"},
                        {"value": "96000", "label": "96000"},
                        {"value": "176400", "label": "176400"},
                        {"value": "192000", "label": "192000"},
                        {"value": "352800", "label": "352800"},
                        {"value": "384000", "label": "384000"}
                    ]
                },
                {
                    "id": "resampling_quality",
                    "element": "select",
                    "visibleIf": {"field": "resampling", "value": true},
                    "doc": "soxr quality.",
                    "label": "Resampling quality",
                    "value": {"value": pb.resampling_quality, "label": pb.resampling_quality},
                    "options": [
                        {"value": "high", "label": "High"},
                        {"value": "very high", "label": "Very high"}
                    ]
                }
            ]
        }),
    ]
}

#[cfg(test)]
mod startup_volume_tests {
    use super::*;

    #[test]
    fn startup_volume_none_when_disabled() {
        let mut p = PlaybackOptions::default();
        p.volumestart = "disabled".into();
        p.mixer_type = "Software".into();
        assert_eq!(p.startup_volume_percent_for_mpd(), None);
    }

    #[test]
    fn startup_volume_none_when_mixer_none() {
        let mut p = PlaybackOptions::default();
        p.volumestart = "50".into();
        p.mixer_type = "None".into();
        assert_eq!(p.startup_volume_percent_for_mpd(), None);
    }

    #[test]
    fn startup_volume_caps_to_volumemax() {
        let mut p = PlaybackOptions::default();
        p.mixer_type = "Software".into();
        p.volumestart = "90".into();
        p.volumemax = "80".into();
        assert_eq!(p.startup_volume_percent_for_mpd(), Some(80));
    }

    #[test]
    fn clamp_volume_respects_volumemax() {
        let mut p = PlaybackOptions::default();
        p.volumemax = "75".into();
        assert_eq!(p.clamp_volume_percent(100), 75);
        assert_eq!(p.clamp_volume_percent(50), 50);
    }

    #[test]
    fn clamp_volume_invalid_volumemax_defaults_cap_100() {
        let mut p = PlaybackOptions::default();
        p.volumemax = "oops".into();
        assert_eq!(p.clamp_volume_percent(99), 99);
    }
}

