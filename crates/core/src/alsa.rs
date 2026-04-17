//! ALSA playback device discovery and persisted output selection for Playback Options (stock UI).
//!
//! Full MPD/ALSA apply pipeline (asound, modular snippets) is deferred; we persist the user choice
//! and expose the same Socket.IO / UI shapes as `audio_interface/alsa_controller` on Node.
//!
//! `list_playback_mixer_controls` mirrors Node `getMixerControls` (`amixer -c N scontents`, Playback
//! controls only) so hardware mixer names match stock Volumio and MPD `mixer_control` lines.
//!
//! **Modular ALSA pipeline** (Node `MODULAR_ALSA_PIPELINE=true`, stock Volumio OS): when
//! `aplay -L` lists PCM **`volumio`**, MPD uses that logical device (see `/etc/asound.conf`). If the
//! flag is on but **`volumio` is missing** (plain Pi OS, dev kit), Evo falls back to **direct `hw:…`**
//! so MPD does not fail with `Unknown PCM volumio`. `mixer_device` for hardware volume still follows
//! the modular rules (`hw:N,0` / `hw:N,M`, or **`SoftMaster`** when software volume is enabled). Set
//! `MODULAR_ALSA_PIPELINE=false` for the legacy path (`mixer_device` = `hw:N` without `,0` for
//! single-card). When the variable is **unset**, Evo defaults to **modular**; see
//! `modular_alsa_pipeline_enabled`.
//! I2S DAC list comes from `dacs.json` (see `crate::i2s`); boot `dtoverlay` uses `sudo` like Node.
//! Output device labels and I2S vs integrated filtering follow Node `alsa_controller/cards.json`
//! (`crate::alsa_cards`).

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context;
use regex::Regex;

use crate::paths;
use serde::{Deserialize, Serialize};

use crate::i2s::DacEntry;

/// One ALSA playback card line from `aplay -l` (first line per card, matching Node `getAplayInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AplayCard {
    pub id: String,
    pub name: String,
}

/// Persisted selection (mirrors Node `outputdevice` / `outputdevicename` at a minimal level).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlsaSettings {
    pub output_device_id: String,
    pub output_device_label: String,
    /// When `true`, volume goes through the softvol / **`SoftMaster`** path (Node `softvolume`).
    #[serde(default)]
    pub softvolume: bool,
    #[serde(default)]
    pub i2s_enabled: bool,
    #[serde(default)]
    pub i2s_dac_id: Option<String>,
    #[serde(default)]
    pub i2s_dac_label: String,
}

impl Default for AlsaSettings {
    fn default() -> Self {
        Self {
            output_device_id: "0".to_string(),
            output_device_label: "Default".to_string(),
            softvolume: false,
            i2s_enabled: false,
            i2s_dac_id: None,
            i2s_dac_label: String::new(),
        }
    }
}

fn state_path() -> PathBuf {
    std::env::var("VOLUMIO_EVO_ALSA_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| paths::default_alsa_state_path())
}

/// Earlier Evo builds used `settings/alsa-state.toml` at the root of `settings/`; relocate to `settings/alsa/state.toml`.
fn migrate_intermediate_alsa_flat_file_if_needed() {
    if std::env::var("VOLUMIO_EVO_ALSA_STATE").is_ok() {
        return;
    }
    let new_path = paths::default_alsa_state_path();
    if new_path.exists() {
        return;
    }
    let old = paths::settings_dir().join("alsa-state.toml");
    if !old.is_file() {
        return;
    }
    if let Some(parent) = new_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::rename(&old, &new_path) {
        Ok(()) => tracing::info!(
            from = %old.display(),
            to = %new_path.display(),
            "{} relocated ALSA state into settings/alsa/",
            crate::log_tags::EVO_ALSA
        ),
        Err(e) => {
            tracing::warn!(
                "{} could not rename {} to {}: {}; trying copy",
                crate::log_tags::EVO_ALSA,
                old.display(),
                new_path.display(),
                e
            );
            if std::fs::copy(&old, &new_path).is_ok() {
                let _ = std::fs::remove_file(&old);
            }
        }
    }
}

impl AlsaSettings {
    pub fn load() -> Self {
        migrate_intermediate_alsa_flat_file_if_needed();
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

    /// Apply `saveAlsaOptions` / `setOutputDevices` JSON (`output_device` + optional `i2s` / `i2sid`).
    pub fn apply_save_payload(&mut self, data: &serde_json::Value) -> anyhow::Result<()> {
        let i2s = data.get("i2s").and_then(|v| v.as_bool()).unwrap_or(false);
        let dacs = crate::i2s::load_dacs()?;
        let profile = crate::i2s::hardware_profile();

        if i2s {
            let i2sid = data.get("i2sid").context("i2sid required when i2s is true")?;
            let dac_id = match i2sid.get("value") {
                Some(serde_json::Value::String(s)) => s.as_str(),
                Some(serde_json::Value::Number(_)) => {
                    return Err(anyhow::anyhow!("i2sid.value must be string (dac id)"));
                }
                _ => anyhow::bail!("i2sid.value missing"),
            };
            let entry = crate::i2s::find_dac(&dacs, &profile, dac_id)
                .context("unknown I2S DAC id for this hardware profile")?;
            if !entry.modules.is_empty() && entry.overlay.is_empty() {
                anyhow::bail!(
                    "DAC {:?} uses kernel modules, not dtoverlay — not implemented in Evo yet",
                    entry.id
                );
            }
            crate::i2s::enable_i2s_overlay(&entry.overlay)?;
            if entry.needs_reboot() {
                tracing::info!(
                    dac = %entry.name,
                    "{} I2S dtoverlay updated; reboot required for audio device to match catalogue",
                    crate::log_tags::EVO_ALSA
                );
            }
            self.i2s_enabled = true;
            self.i2s_dac_id = Some(entry.id.clone());
            self.i2s_dac_label = entry.name.clone();
            self.output_device_id = entry.alsanum.clone();
            self.output_device_label = entry.name.clone();
            self.save()?;
            return Ok(());
        }

        if self.i2s_enabled {
            crate::i2s::disable_i2s_overlay()?;
        }
        self.i2s_enabled = false;
        self.i2s_dac_id = None;
        self.i2s_dac_label.clear();

        let od = data
            .get("output_device")
            .context("missing output_device when I2S is off")?;
        let label = od
            .get("label")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .replace("USB: ", "");
        let id = match od.get("value") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Number(n)) => n.to_string(),
            _ => anyhow::bail!("output_device.value must be string or number"),
        };
        self.output_device_id = id;
        self.output_device_label = label;
        self.save()?;
        Ok(())
    }
}

/// Run `aplay -l` and parse playback devices (one entry per card; mirrors Node card list shape).
pub fn list_playback_cards() -> std::io::Result<Vec<AplayCard>> {
    let out = std::process::Command::new("/usr/bin/aplay")
        .args(["-l"])
        .output()?;

    if !out.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("aplay -l failed: {}", String::from_utf8_lossy(&out.stderr)),
        ));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(parse_aplay_l(&stdout))
}

fn parse_aplay_l(stdout: &str) -> Vec<AplayCard> {
    let mut cards = Vec::new();
    let mut last_card: Option<u32> = None;

    for line in stdout.lines() {
        if !line.contains("card ") || !line.contains('[') {
            continue;
        }
        let head = line.split(',').next().unwrap_or("");
        let Some(colon1) = head.find(':') else {
            continue;
        };
        let card_prefix = head[..colon1].trim();
        let num_str = card_prefix.strip_prefix("card").map(str::trim).unwrap_or("");
        let Ok(num) = num_str.parse::<u32>() else {
            continue;
        };
        if last_card == Some(num) {
            continue;
        }
        last_card = Some(num);
        let name = if let (Some(a), Some(b)) = (head.find('['), head.rfind(']')) {
            head[a + 1..b].trim().to_string()
        } else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        cards.push(AplayCard {
            id: num.to_string(),
            name,
        });
    }

    cards
}

/// If the saved device is missing (USB unplugged), fall back to the first card.
/// When I2S is enabled, ALSA card numbers may not match `aplay` until after reboot — skip remap.
pub fn coerce_selection(cards: &[AplayCard], mut settings: AlsaSettings) -> AlsaSettings {
    if settings.i2s_enabled {
        return settings;
    }
    if cards.is_empty() {
        return settings;
    }
    if cards.iter().any(|c| c.id == settings.output_device_id) {
        return settings;
    }
    settings.output_device_id = cards[0].id.clone();
    settings.output_device_label = cards[0].name.clone();
    settings
}

/// Card index for `amixer -c` (Node `getMixerControls`: use only the number before `,`).
fn amixer_card_index(output_device_id: &str) -> Option<&str> {
    let id = output_device_id.trim();
    if id.is_empty() || id == "nodev" {
        return None;
    }
    Some(id.split_once(',').map(|(a, _)| a).unwrap_or(id))
}

/// `MODULAR_ALSA_PIPELINE` (Node / Volumio OS). When unset or empty, **true** so Evo matches stock
/// Volumio; set to `false` / `0` / `no` / `off` for the legacy MPD device path.
pub fn modular_alsa_pipeline_enabled() -> bool {
    match std::env::var("MODULAR_ALSA_PIPELINE") {
        Ok(s) => {
            let t = s.trim().to_ascii_lowercase();
            if t.is_empty() {
                return true;
            }
            !matches!(t.as_str(), "false" | "0" | "no" | "off")
        }
        Err(_) => true,
    }
}

/// `aplay -L` lists PCM names on non-indented lines. Returns whether the **`volumio`** PCM exists
/// (stock Volumio `/etc/asound.conf`). If modular mode is on but this is false, MPD must not use
/// `device "volumio"` or playback fails with `Unknown PCM volumio`.
pub(crate) fn volumio_pcm_listed_in_aplay_l(stdout: &str) -> bool {
    for line in stdout.lines() {
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let Some(first) = line.split_whitespace().next() else {
            continue;
        };
        let name = first.split(':').next().unwrap_or(first);
        if name == "volumio" {
            return true;
        }
    }
    false
}

fn pcm_volumio_available() -> bool {
    let out = Command::new("/usr/bin/aplay")
        .args(["-L"])
        .output()
        .or_else(|_| Command::new("aplay").args(["-L"]).output());
    let Ok(out) = out else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    volumio_pcm_listed_in_aplay_l(&String::from_utf8_lossy(&out.stdout))
}

fn direct_hw_playback_device(id: &str) -> String {
    if id.contains(',') {
        format!("hw:{id}")
    } else {
        format!("hw:{id},0")
    }
}

/// MPD `audio_output` `device` string. Modular: **`volumio`** only if that PCM exists (`aplay -L`);
/// otherwise **direct `hw:…`** (Pi OS / dev trees without Volumio asound). Legacy: always direct `hw:…`.
pub fn mpd_playback_device(alsa: &AlsaSettings) -> String {
    let id = alsa.output_device_id.trim();
    if id.is_empty() || id == "nodev" {
        return "default".to_string();
    }
    if modular_alsa_pipeline_enabled() && pcm_volumio_available() {
        return "volumio".to_string();
    }
    if modular_alsa_pipeline_enabled() {
        tracing::info!(
            "{} MODULAR_ALSA_PIPELINE is set but ALSA PCM \"volumio\" not in aplay -L; using direct {} for MPD (install Volumio asound or define pcm.volumio)",
            crate::log_tags::EVO_ALSA,
            direct_hw_playback_device(id)
        );
    }
    direct_hw_playback_device(id)
}

/// ALSA **control** string for MPD when it must **`snd_ctl_open`** the mixer (card only).
///
/// Playback [`mpd_playback_device`] may use **`hw:N,0`**; that form is **invalid** for the control
/// interface and triggers MPD **`Invalid CTL hw:N,0`** / mixer failures when MPD uses that string for
/// **`mixer_device`**. Playback may still use **`hw:N,0`** via [`mpd_playback_device`]. The Evo fragment
/// emits **`mixer_device` only with `mixer_type "hardware"`** — MPD 0.24+ rejects `mixer_device` for
/// `software` / `none`. Use this card-only form in [`mixer_device_for_mpd`].
pub fn mpd_alsa_ctl_mixer_device(alsa: &AlsaSettings) -> String {
    let id = alsa.output_device_id.trim();
    if id.is_empty() || id == "nodev" {
        return "default".to_string();
    }
    let Some(card) = amixer_card_index(id) else {
        return "default".to_string();
    };
    format!("hw:{card}")
}

/// MPD `mixer_device` for hardware / softvol (Node `createMPDFile` `mixerdev`).
///
/// Must be a valid **`snd_ctl_open`** name. **`hw:N,0`** is for PCM, not the mixer control — use
/// [`mpd_alsa_ctl_mixer_device`] (same as this function for non-**SoftMaster** paths).
pub fn mixer_device_for_mpd(alsa: &AlsaSettings) -> String {
    let id = alsa.output_device_id.trim();
    if id.is_empty() || id == "nodev" {
        return "default".to_string();
    }
    if alsa.softvolume {
        return "SoftMaster".to_string();
    }
    mpd_alsa_ctl_mixer_device(alsa)
}

const INVALID_MIXER_SUBSTR: [&str; 3] = ["Clock Validity", "Tx Source", "Internal Validity"];

fn mixer_name_is_valid(name: &str) -> bool {
    !INVALID_MIXER_SUBSTR
        .iter()
        .any(|bad| name.contains(bad))
}

/// Skip switch-style playback rows (invert, filter) when opening the **line** to unity at boot.
fn is_alsa_startup_line_fader(name: &str) -> bool {
    let base = name.split(',').next().unwrap_or(name).trim();
    if !mixer_name_is_valid(base) {
        return false;
    }
    let low = base.to_lowercase();
    !(low.contains("invert") || low.contains("rolloff") || low.contains("deemph"))
}

/// Unmute and set **100%** on **Playback** faders when there is **no** ALSA **`SoftMaster`** softvol.
///
/// With **`skip_control == None`**, every eligible fader is set (used for **Software** startup: open
/// the full DAC path before MPD **`setvol`**).
///
/// With **`skip_control == Some(name)`**, that control is left for a later **`set_system_volume_percent`**
/// (used for **Hardware** startup: open **sibling** faders without touching MPD’s primary control first).
pub fn open_alsa_playback_line_unity_except(
    alsa: &AlsaSettings,
    volumecurve_logarithmic: bool,
    skip_control: Option<&str>,
) {
    if alsa_softmaster_control_present(alsa) {
        return;
    }
    let skip = skip_control.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    });
    for raw in list_playback_mixer_controls(alsa.output_device_id.trim()) {
        let t = raw.trim();
        if t.is_empty() || t == "SoftMaster" {
            continue;
        }
        if !is_alsa_startup_line_fader(t) {
            continue;
        }
        if skip.is_some_and(|sk| sk == t) {
            continue;
        }
        if let Err(e) = set_system_volume_percent(alsa, "Hardware", t, volumecurve_logarithmic, 100)
        {
            tracing::debug!(
                control = %t,
                err = %e,
                "{} open ALSA playback line: could not set 100% (non-fatal)",
                crate::log_tags::EVO_ALSA
            );
        }
    }
}

/// Software startup: all eligible Playback faders → unity, then MPD **`setvol`** carries the level.
pub fn open_alsa_playback_line_unity_before_mpd_volume(
    alsa: &AlsaSettings,
    volumecurve_logarithmic: bool,
) {
    open_alsa_playback_line_unity_except(alsa, volumecurve_logarithmic, None);
}

/// Hardware startup: sibling faders → unity (**skip** the primary control name), then set primary to
/// **`percent`** (same rules as [`set_system_volume_percent`] for **Hardware**).
pub fn apply_startup_volume_hardware_mixer(
    alsa: &AlsaSettings,
    mixer_name_field: &str,
    volumecurve_logarithmic: bool,
    percent: u8,
) -> Result<(), String> {
    let primary = playback_mixer_control_name(alsa, "Hardware", mixer_name_field)?;
    open_alsa_playback_line_unity_except(
        alsa,
        volumecurve_logarithmic,
        Some(primary.as_str()),
    );
    set_system_volume_percent(
        alsa,
        "Hardware",
        mixer_name_field,
        volumecurve_logarithmic,
        percent,
    )
}

/// Parse `amixer scontents` like Node `getMixerControls`: Playback simple controls, de-duped with
/// `,1`, `,2`, … suffixes.
pub(crate) fn parse_amixer_scontents(stdout: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for chunk in stdout.split("Simple mixer control") {
        if !chunk.contains("Playback") {
            continue;
        }
        let first_line = chunk.lines().next().unwrap_or("");
        let before_comma = first_line.split(',').next().unwrap_or("");
        let stripped = before_comma.replace('\'', "");
        let base = stripped.trim().to_string();
        if base.is_empty() || !mixer_name_is_valid(&base) {
            continue;
        }
        let mut candidate = base.clone();
        let mut n = 0u32;
        while out.iter().any(|e| e == &candidate) {
            n += 1;
            candidate = format!("{base},{n}");
        }
        out.push(candidate);
    }
    out
}

/// ALSA playback mixer control names for the selected output card (Node `getMixerControls`).
pub fn list_playback_mixer_controls(output_device_id: &str) -> Vec<String> {
    let Some(card) = amixer_card_index(output_device_id) else {
        return Vec::new();
    };
    let Ok(out) = Command::new("amixer")
        .args(["-c", card, "scontents"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        tracing::warn!(
            stderr = %String::from_utf8_lossy(&out.stderr),
            "{} amixer scontents failed for card {}",
            crate::log_tags::EVO_ALSA,
            card
        );
        return Vec::new();
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_amixer_scontents(&stdout)
}

/// Resolve the simple mixer control name for **Software** / **Hardware** (same rules as volume set/get).
fn playback_mixer_control_name(
    alsa: &AlsaSettings,
    mixer_type: &str,
    mixer_name: &str,
) -> Result<String, String> {
    match mixer_type {
        "Software" => Ok("SoftMaster".into()),
        "Hardware" => {
            let t = mixer_name.trim();
            if !t.is_empty() {
                return Ok(t.into());
            }
            let Some(card) = amixer_card_index(alsa.output_device_id.trim()) else {
                return Err("no ALSA output card for amixer".into());
            };
            list_playback_mixer_controls(&alsa.output_device_id)
                .into_iter()
                .next()
                .ok_or_else(|| {
                    format!(
                        "no Playback mixer controls on card {} (Hardware mixer name empty)",
                        card
                    )
                })
        }
        _ => Err(format!("unsupported mixer_type {:?}", mixer_type)),
    }
}

/// Set ALSA volume like Node `CoreVolumeController.setVolume` (`amixer` / `alsavolume`): **Hardware**
/// uses the named Playback control (or the first from [`list_playback_mixer_controls`] if empty);
/// **Software** uses **`SoftMaster`**. Logarithmic curve adds **`-M`**, matching `volumecontrol.js`.
///
/// Call this for Evo UI volume when stock Volumio would use ALSA rather than relying on MPD `setvol`
/// alone (same path as **alsamixer**).
pub fn set_system_volume_percent(
    alsa: &AlsaSettings,
    mixer_type: &str,
    mixer_name: &str,
    volumecurve_logarithmic: bool,
    percent: u8,
) -> Result<(), String> {
    if mixer_type == "None" {
        return Err("mixer_type is None".into());
    }
    let Some(card) = amixer_card_index(alsa.output_device_id.trim()) else {
        return Err("no ALSA output card for amixer".into());
    };
    let p = percent.min(100);
    let mixer = playback_mixer_control_name(alsa, mixer_type, mixer_name)?;

    // Stereo / dual-mono softvol: a single `80%` can move only one channel on some cards, so
    // `get_system_volume_percent` (max across channels, see b4b1b01) then reports a misleading
    // level and the UI feels “stuck” on one side. Match common practice: `80%,80%`. Retry with
    // `80%` if the control is mono-only (amixer rejects the dual form).
    let pct_stereo = format!("{p}%,{p}%");
    let pct_mono = format!("{p}%");
    let mut cmd = Command::new("amixer");
    if volumecurve_logarithmic {
        cmd.arg("-M");
    }
    cmd.args(["set", "-c", card, mixer.as_str(), "unmute", &pct_stereo]);
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    let err_stereo = String::from_utf8_lossy(&out.stderr);
    let mut cmd2 = Command::new("amixer");
    if volumecurve_logarithmic {
        cmd2.arg("-M");
    }
    cmd2.args(["set", "-c", card, mixer.as_str(), "unmute", &pct_mono]);
    let out2 = cmd2.output().map_err(|e| e.to_string())?;
    if !out2.status.success() {
        return Err(format!(
            "amixer: {} (stereo form failed: {})",
            String::from_utf8_lossy(&out2.stderr).trim(),
            err_stereo.trim()
        ));
    }
    Ok(())
}

/// Regex for `amixer get` lines like `Front Left: Playback 204 [85%]` (Node `volumecontrol` `reInfo`).
fn amixer_playback_percent_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i):\s*Playback\s+[0-9-]+\s+\[(\d+)%\]")
            .expect("amixer playback percent regex")
    })
}

/// Read current **master** level 0–100 from ALSA for the same control as
/// [`set_system_volume_percent`] (Node `getInfo` / `retrievevolume`). Returns `None` if unavailable
/// or unparsed — caller should fall back to MPD `status.volume`.
pub fn get_system_volume_percent(
    alsa: &AlsaSettings,
    mixer_type: &str,
    mixer_name: &str,
    volumecurve_logarithmic: bool,
) -> Option<u8> {
    if mixer_type == "None" {
        return None;
    }
    let card = amixer_card_index(alsa.output_device_id.trim())?;
    let mixer = playback_mixer_control_name(alsa, mixer_type, mixer_name).ok()?;

    let mut cmd = Command::new("amixer");
    if volumecurve_logarithmic {
        cmd.arg("-M");
    }
    cmd.args(["get", "-c", card, mixer.as_str()]);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let re = amixer_playback_percent_re();
    // Use the **maximum** across all `Playback … [n%]` lines. Some cards list a channel at 0% or
    // ordering differs; taking only the first match made pushState report 0 while another channel
    // held the real level (UI flicker on Hardware mixer).
    let mut best: Option<u8> = None;
    for cap in re.captures_iter(&stdout) {
        if let Ok(n) = cap[1].parse::<u32>() {
            let v = (n.min(100)) as u8;
            best = Some(best.map_or(v, |b| b.max(v)));
        }
    }
    best
}

/// Whether the output card lists a **`SoftMaster`** playback control (Volumio softvol / `asound`).
pub fn alsa_softmaster_control_present(alsa: &AlsaSettings) -> bool {
    list_playback_mixer_controls(alsa.output_device_id.trim())
        .iter()
        .any(|n| n.split(',').next().unwrap_or(n).trim() == "SoftMaster")
}

/// ALSA switch mute: `amixer set -c CARD CONTROL mute|unmute` (no `-M`). Best-effort for transitions.
pub fn set_playback_switch_mute(
    alsa: &AlsaSettings,
    mixer_type: &str,
    mixer_name: &str,
    mute: bool,
) -> Result<(), String> {
    if mixer_type == "None" {
        return Err("mixer_type is None".into());
    }
    let Some(card) = amixer_card_index(alsa.output_device_id.trim()) else {
        return Err("no ALSA output card for amixer".into());
    };
    let mixer = playback_mixer_control_name(alsa, mixer_type, mixer_name)?;
    let sw = if mute { "mute" } else { "unmute" };
    let out = Command::new("amixer")
        .args(["set", "-c", card, mixer.as_str(), sw])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "amixer mute: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// **Software → Hardware** mixer hand-off: apply the **named hardware control** (`hardware_mixer_name`,
/// same as `playback_options.mixer`) to the carried level, max out **SoftMaster** when it exists so
/// MPD hardware mixer is not stacked with a low soft stage.
///
/// Carried level: **SoftMaster** read when [`alsa_softmaster_control_present`], otherwise
/// `mpd_volume_when_no_softmaster` (MPD `status.volume` — e.g. MPD-only software volume on `hw:`).
///
/// Call **before** updating MPD to `mixer_type hardware` / [`crate::playback_options::PlaybackOptions::write_fragment_and_restart_mpd`].
pub fn transition_software_to_hardware_handoff(
    alsa: &AlsaSettings,
    hardware_mixer_name: &str,
    volumecurve_logarithmic: bool,
    mpd_volume_when_no_softmaster: Option<u8>,
) -> Result<u8, String> {
    let log = volumecurve_logarithmic;
    let pct = if alsa_softmaster_control_present(alsa) {
        get_system_volume_percent(alsa, "Software", "SoftMaster", log).unwrap_or(50)
    } else {
        mpd_volume_when_no_softmaster.unwrap_or_else(|| {
            tracing::warn!(
                "{} Software→Hardware: no ALSA SoftMaster; missing MPD volume, defaulting carried level to 50%",
                crate::log_tags::EVO_ALSA
            );
            50
        })
    };

    let _ = set_playback_switch_mute(alsa, "Software", "", true);
    let _ = set_playback_switch_mute(alsa, "Hardware", hardware_mixer_name, true);
    set_system_volume_percent(alsa, "Hardware", hardware_mixer_name, log, pct)?;
    if alsa_softmaster_control_present(alsa) {
        set_system_volume_percent(alsa, "Software", "SoftMaster", log, 100)?;
    }
    let _ = set_playback_switch_mute(alsa, "Hardware", hardware_mixer_name, false);
    let _ = set_playback_switch_mute(alsa, "Software", "", false);
    Ok(pct)
}

/// Tracing target for **hardware → software** mixer transition timelines. Filter logs, e.g.
/// `RUST_LOG=volumio_evo::mixer_hw_sw=info`.
pub const MIXER_HW_SW_TRACE_TARGET: &str = "volumio_evo::mixer_hw_sw";

/// Settle after phase-1 ALSA mute/zero (SoftMaster path), ms.
const HW_SW_PHASE1_SETTLE_MS: u64 = 150;
/// Ramp steps opening hardware 0→100% after MPD (SoftMaster path).
const HW_SW_RAMP_STEPS: usize = 8;
/// Even spacing between ramp steps, ms.
const HW_SW_RAMP_STEP_MS: u64 = 12;

#[inline]
pub(crate) fn mixer_hw_sw_trace(t0: Option<Instant>, stage: &'static str, detail: &str) {
    let Some(start) = t0 else {
        return;
    };
    let elapsed_ms = start.elapsed().as_millis() as u64;
    tracing::info!(
        target: MIXER_HW_SW_TRACE_TARGET,
        elapsed_ms,
        stage,
        detail,
        "{} mixer_hw_sw hw→sw",
        crate::log_tags::EVO_ALSA
    );
}

/// **Hardware → Software** (phase 1): read level from the **hardware** control, then either
/// (**SoftMaster** present) mute, **hardware → 0%**, **SoftMaster → 100%** (unity — same role as
/// [`transition_software_to_hardware_handoff`], which maxes SoftMaster so level is not stacked); or
/// (**no SoftMaster**) **do not** change the hardware mixer here — opening it to 100% while MPD is
/// still in **hardware** volume mode caused a **full-scale burst**. Caller runs
/// [`transition_hardware_to_software_after_mpd_no_softmaster`] **after** MPD software `setvol`.
///
/// The carried percentage is returned for **MPD `setvol` only** after restart (caller should apply it
/// **before** [`transition_hardware_to_software_after_mpd_softmaster`] when SoftMaster exists).
///
/// Call **before** [`crate::playback_options::PlaybackOptions::write_fragment_and_restart_mpd`].
///
/// `timeline_t0`: pass [`Some`] ([`Instant::now`] from the caller) for **millisecond** timeline logs
/// on target [`MIXER_HW_SW_TRACE_TARGET`]; [`None`] disables those lines.
pub fn transition_hardware_to_software_before_mpd(
    alsa: &AlsaSettings,
    prev_hardware_mixer: &str,
    volumecurve_logarithmic: bool,
    timeline_t0: Option<Instant>,
) -> Result<u8, String> {
    let log = volumecurve_logarithmic;
    mixer_hw_sw_trace(timeline_t0, "phase1_begin", "enter before_mpd");
    let pct = get_system_volume_percent(alsa, "Hardware", prev_hardware_mixer, log).unwrap_or(50);
    mixer_hw_sw_trace(
        timeline_t0,
        "phase1_read_hw",
        &format!("hardware_pct={pct} softmaster={}", alsa_softmaster_control_present(alsa)),
    );

    if alsa_softmaster_control_present(alsa) {
        let _ = set_playback_switch_mute(alsa, "Hardware", prev_hardware_mixer, true);
        let _ = set_playback_switch_mute(alsa, "Software", "", true);
        mixer_hw_sw_trace(timeline_t0, "phase1_muted", "hw+sw switches");
        set_system_volume_percent(alsa, "Hardware", prev_hardware_mixer, log, 0)?;
        mixer_hw_sw_trace(timeline_t0, "phase1_hw_zero", "hardware 0%");
        set_system_volume_percent(alsa, "Software", "SoftMaster", log, 100)?;
        mixer_hw_sw_trace(timeline_t0, "phase1_softmaster_unity", "SoftMaster 100%");
        thread::sleep(Duration::from_millis(HW_SW_PHASE1_SETTLE_MS));
        mixer_hw_sw_trace(
            timeline_t0,
            "phase1_settle_done",
            &format!("slept_ms={HW_SW_PHASE1_SETTLE_MS}"),
        );
    } else {
        mixer_hw_sw_trace(
            timeline_t0,
            "phase1_no_softmaster_hold_hw",
            "leave hardware at current % until MPD is on software volume (avoid blast)",
        );
    }
    mixer_hw_sw_trace(timeline_t0, "phase1_end", "before_mpd complete");
    Ok(pct)
}

/// **Hardware → Software** (no SoftMaster): after MPD is on **software** volume and **`setvol`**
/// has been applied, set the **hardware** mixer to **100%** so gain is not stacked (MPD × low HW).
///
/// Call **only** when [`alsa_softmaster_control_present`] is **false**; the SoftMaster path uses
/// [`transition_hardware_to_software_after_mpd_softmaster`] instead.
pub fn transition_hardware_to_software_after_mpd_no_softmaster(
    alsa: &AlsaSettings,
    prev_hardware_mixer: &str,
    volumecurve_logarithmic: bool,
    timeline_t0: Option<Instant>,
) -> Result<(), String> {
    if alsa_softmaster_control_present(alsa) {
        return Ok(());
    }
    let log = volumecurve_logarithmic;
    set_system_volume_percent(alsa, "Hardware", prev_hardware_mixer, log, 100)?;
    mixer_hw_sw_trace(
        timeline_t0,
        "phase2_hw_unity_no_softmaster",
        "hardware 100% after MPD software+setvol",
    );
    Ok(())
}

/// **Hardware → Software** (phase 2, SoftMaster only): after MPD software volume is set to the
/// carried level, ramp the **hardware** line **0% → 100%** so the DAC is fully open and attenuation
/// stays in MPD (SoftMaster remains at 100% from phase 1).
///
/// `timeline_t0`: same as [`transition_hardware_to_software_before_mpd`] (one continuous timeline).
pub fn transition_hardware_to_software_after_mpd_softmaster(
    alsa: &AlsaSettings,
    prev_hardware_mixer: &str,
    volumecurve_logarithmic: bool,
    timeline_t0: Option<Instant>,
) -> Result<(), String> {
    if !alsa_softmaster_control_present(alsa) {
        return Ok(());
    }
    let log = volumecurve_logarithmic;
    mixer_hw_sw_trace(timeline_t0, "phase2_ramp_begin", &format!("steps={HW_SW_RAMP_STEPS} step_ms={HW_SW_RAMP_STEP_MS}"));
    for i in 1..=HW_SW_RAMP_STEPS {
        let p = ((i * 100 / HW_SW_RAMP_STEPS).min(100)) as u8;
        set_system_volume_percent(alsa, "Hardware", prev_hardware_mixer, log, p)?;
        thread::sleep(Duration::from_millis(HW_SW_RAMP_STEP_MS));
        mixer_hw_sw_trace(
            timeline_t0,
            "phase2_ramp_step",
            &format!("step={i}/{HW_SW_RAMP_STEPS} hw_pct={p} after_step_sleep_ms={HW_SW_RAMP_STEP_MS}"),
        );
    }
    let _ = set_playback_switch_mute(alsa, "Hardware", prev_hardware_mixer, false);
    let _ = set_playback_switch_mute(alsa, "Software", "", false);
    mixer_hw_sw_trace(timeline_t0, "phase2_unmute_done", "hw+sw unmuted");
    mixer_hw_sw_trace(timeline_t0, "phase2_end", "after_mpd_softmaster complete");
    Ok(())
}

/// `pushOutputDevices` body (wizard + internal consistency); matches Node `getAudioDevices` shape.
pub fn push_output_devices_json(
    cards: &[AplayCard],
    settings: &AlsaSettings,
    i2s_available: &[DacEntry],
) -> serde_json::Value {
    let available: Vec<serde_json::Value> = cards
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "name": c.name,
            })
        })
        .collect();

    let mut base = serde_json::json!({
        "devices": {
            "active": {
                "id": settings.output_device_id,
                "name": settings.output_device_label,
            },
            "available": available,
        }
    });

        if !i2s_available.is_empty() {
        let i2s_opts: Vec<serde_json::Value> = i2s_available
            .iter()
            .filter(|e| !e.overlay.is_empty() && e.modules.is_empty())
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "name": e.name,
                })
            })
            .collect();
        let active_name = if settings.i2s_enabled {
            settings.i2s_dac_label.clone()
        } else {
            i2s_available
                .first()
                .map(|e| e.name.clone())
                .unwrap_or_default()
        };
        base.as_object_mut().unwrap().insert(
            "i2s".to_string(),
            serde_json::json!({
                "enabled": settings.i2s_enabled,
                "active": active_name,
                "available": i2s_opts,
            }),
        );
    }

    base
}

pub struct PlaybackOptionsUiParams<'a> {
    pub cards: &'a [AplayCard],
    pub settings: &'a AlsaSettings,
    pub i2s_dacs: &'a [DacEntry],
    pub playback: &'a crate::playback_options::PlaybackOptions,
    /// `list_playback_mixer_controls(settings.output_device_id)` for the mixer name dropdown.
    pub mixer_controls: &'a [String],
}

/// Stock plugin UI for `audio_interface/alsa_controller` (output device + optional I2S DAC model).
pub fn playback_options_ui_config(p: &PlaybackOptionsUiParams<'_>) -> serde_json::Value {
    let output_options: Vec<serde_json::Value> = p
        .cards
        .iter()
        .map(|c| {
            serde_json::json!({
                "value": c.id,
                "label": c.name,
            })
        })
        .collect();

    let show_i2s = !p.i2s_dacs.is_empty();
    let i2s_select_options: Vec<serde_json::Value> = p
        .i2s_dacs
        .iter()
        .filter(|e| !e.overlay.is_empty() && e.modules.is_empty())
        .map(|e| {
            serde_json::json!({
                "value": e.id,
                "label": e.name,
            })
        })
        .collect();

    let i2s_id = p
        .settings
        .i2s_dac_id
        .as_deref()
        .and_then(|id| p.i2s_dacs.iter().find(|e| e.id == id))
        .map(|e| e.id.clone())
        .or_else(|| p.i2s_dacs.first().map(|e| e.id.clone()))
        .unwrap_or_default();
    let i2s_label = p
        .settings
        .i2s_dac_id
        .as_deref()
        .and_then(|id| p.i2s_dacs.iter().find(|e| e.id == id))
        .map(|e| e.name.clone())
        .or_else(|| p.i2s_dacs.first().map(|e| e.name.clone()))
        .unwrap_or_default();

    let save_fields: Vec<serde_json::Value> = if show_i2s {
        vec![
            serde_json::json!("output_device"),
            serde_json::json!("i2s"),
            serde_json::json!("i2sid"),
        ]
    } else {
        vec![serde_json::json!("output_device")]
    };

    // `visibleIf` resolves against other `section.content` items by id. Only reference `i2s` when
    // that switch exists (I2S catalogue loaded); otherwise the UI hides the output row — empty page.
    let mut output_device = serde_json::json!({
        "id": "output_device",
        "element": "select",
        "doc": "Local outputs from aplay (HDMI, headphone, USB, …). Shown when I2S DAC is off.",
        "label": "Output device",
        "value": {
            "value": p.settings.output_device_id,
            "label": p.settings.output_device_label
        },
        "options": output_options
    });
    if show_i2s {
        output_device
            .as_object_mut()
            .expect("object")
            .insert(
                "visibleIf".to_string(),
                serde_json::json!({ "field": "i2s", "value": false }),
            );
    }

    let mut content: Vec<serde_json::Value> = vec![output_device];

    if show_i2s {
        content.push(serde_json::json!({
            "id": "i2s",
            "element": "switch",
            "doc": "Enable an I2S HAT: writes dtoverlay to boot config (sudo). Reboot required for most boards.",
            "label": "I2S DAC",
            "hidden": false,
            "value": p.settings.i2s_enabled
        }));
        content.push(serde_json::json!({
            "id": "i2sid",
            "element": "select",
            "doc": "DAC model from Volumio dacs.json (same catalogue as stock Volumio).",
            "label": "DAC model",
            "value": {
                "value": i2s_id,
                "label": i2s_label
            },
            "visibleIf": {
                "field": "i2s",
                "value": true
            },
            "options": i2s_select_options
        }));
    }

    let mut sections: Vec<serde_json::Value> = vec![serde_json::json!({
        "id": "alsa_options",
        "element": "section",
        "label": "Audio output",
        "icon": "fa-volume-up",
        "onSave": {"type": "controller", "endpoint": "audio_interface/alsa_controller", "method": "saveAlsaOptions"},
        "saveButton": {
            "label": "Save",
            "data": save_fields
        },
        "value": {
            "value": p.settings.output_device_id,
            "label": p.settings.output_device_label
        },
        "content": content
    })];
    sections.extend(crate::playback_options::playback_mpd_ui_sections(
        p.playback,
        p.mixer_controls,
    ));

    serde_json::json!({
        "page": {
            "label": "Playback options"
        },
        "sections": sections
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// `MODULAR_ALSA_PIPELINE` is process-global; serialize tests that mutate it.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn modular_mpd_uses_volumio_or_hw_fallback_and_ctl_mixer_hw_n() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("MODULAR_ALSA_PIPELINE");
        let a = AlsaSettings {
            output_device_id: "2".into(),
            ..Default::default()
        };
        assert!(modular_alsa_pipeline_enabled());
        let dev = mpd_playback_device(&a);
        assert!(
            dev == "volumio" || dev == "hw:2,0",
            "with modular ALSA, MPD device is volumio when pcm exists, else direct hw; got {dev:?}"
        );
        assert_eq!(mixer_device_for_mpd(&a), "hw:2");
    }

    #[test]
    fn modular_subdevice_mixer_ctl_uses_card_only() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("MODULAR_ALSA_PIPELINE");
        let a = AlsaSettings {
            output_device_id: "1,1".into(),
            ..Default::default()
        };
        let dev = mpd_playback_device(&a);
        assert!(
            dev == "volumio" || dev == "hw:1,1",
            "expected volumio or hw:1,1, got {dev:?}"
        );
        assert_eq!(mixer_device_for_mpd(&a), "hw:1");
    }

    #[test]
    fn aplay_l_detects_volumio_pcm_line() {
        let sample = "null\n    Discard all samples (playback)\nvolumio\n    Volumio device\n";
        assert!(volumio_pcm_listed_in_aplay_l(sample));
        assert!(!volumio_pcm_listed_in_aplay_l(
            "default\n    Default Audio Device\n"
        ));
    }

    #[test]
    fn mpd_ctl_mixer_device_is_hw_card_only_never_subdevice() {
        let a = AlsaSettings {
            output_device_id: "2".into(),
            ..Default::default()
        };
        assert_eq!(mpd_alsa_ctl_mixer_device(&a), "hw:2");
        let a2 = AlsaSettings {
            output_device_id: "2,0".into(),
            ..Default::default()
        };
        assert_eq!(mpd_alsa_ctl_mixer_device(&a2), "hw:2");
    }

    #[test]
    fn legacy_mpd_uses_direct_hw_strings() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("MODULAR_ALSA_PIPELINE", "false");
        let a = AlsaSettings {
            output_device_id: "2".into(),
            ..Default::default()
        };
        assert!(!modular_alsa_pipeline_enabled());
        assert_eq!(mpd_playback_device(&a), "hw:2,0");
        assert_eq!(mixer_device_for_mpd(&a), "hw:2");
        std::env::remove_var("MODULAR_ALSA_PIPELINE");
    }

    #[test]
    fn softvolume_requests_softmaster_mixer_device() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("MODULAR_ALSA_PIPELINE");
        let a = AlsaSettings {
            output_device_id: "1".into(),
            softvolume: true,
            ..Default::default()
        };
        assert_eq!(mixer_device_for_mpd(&a), "SoftMaster");
    }

    #[test]
    fn amixer_get_parses_playback_percent_like_alsamixer() {
        let sample = "\
Simple mixer control 'DAC',0
  Capabilities: pvolume
  Front Left: Playback 204 [85%] [-18.00dB]
  Front Right: Playback 204 [85%] [-18.00dB]
";
        let re = amixer_playback_percent_re();
        let cap = re.captures(sample).expect("playback line");
        assert_eq!(cap.get(1).unwrap().as_str(), "85");
    }

    #[test]
    fn amixer_get_max_playback_percent_across_channels() {
        let sample = "\
Simple mixer control 'PCM',0
  Front Left: Playback 0 [0%] [off]
  Front Right: Playback 200 [78%] [-8.00dB]
";
        let re = amixer_playback_percent_re();
        let max_pct: u8 = re
            .captures_iter(sample)
            .filter_map(|c| c.get(1).and_then(|m| m.as_str().parse::<u32>().ok()))
            .map(|n| (n.min(100)) as u8)
            .max()
            .expect("at least one percent");
        assert_eq!(max_pct, 78);
    }

    #[test]
    fn parse_amixer_scontents_matches_node_shape() {
        let sample = r"
Simple mixer control 'PCM',0
  Capabilities: pvolume pswitch
  Playback channels: Front Left - Front Right
Simple mixer control 'Mic',0
  Capabilities: cvolume cswitch
  Capture channels: Mono
Simple mixer control 'Digital',0
  Capabilities: pvolume
  Playback channels: Mono
";
        let v = parse_amixer_scontents(sample);
        assert_eq!(v, vec!["PCM", "Digital"]);
    }

    #[test]
    fn parse_amixer_skips_invalid_mixers() {
        let sample = r"
Simple mixer control 'Clock Validity',0
  Playback channels: Mono
Simple mixer control 'PCM',0
  Playback channels: Mono
";
        let v = parse_amixer_scontents(sample);
        assert_eq!(v, vec!["PCM"]);
    }

    #[test]
    fn parse_amixer_duplicate_playback_names_get_suffix() {
        let sample = r"
Simple mixer control 'PCM',0
  Playback channels: Front Left - Front Right
Simple mixer control 'PCM',0
  Playback channels: Front Left - Front Right
";
        let v = parse_amixer_scontents(sample);
        assert_eq!(v, vec!["PCM", "PCM,1"]);
    }

    #[test]
    fn parse_aplay_sample() {
        let sample = r"**** List of PLAYBACK Hardware Devices ****
card 0: PCH [HDA Intel PCH], device 0: ALC892 Analog [ALC892 Analog]
card 1: Device [USB Audio], device 0: USB Audio [USB Audio]
";
        let c = parse_aplay_l(sample);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].id, "0");
        assert!(c[0].name.contains("HDA") || c[0].name.contains("Intel"));
        assert_eq!(c[1].id, "1");
    }
}
