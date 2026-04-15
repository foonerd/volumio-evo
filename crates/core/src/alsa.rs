//! ALSA playback device discovery and persisted output selection for Playback Options (stock UI).
//!
//! Full MPD/ALSA apply pipeline (asound, modular snippets) is deferred; we persist the user choice
//! and expose the same Socket.IO / UI shapes as `audio_interface/alsa_controller` on Node.
//! I2S DAC list comes from `dacs.json` (see `crate::i2s`); boot `dtoverlay` uses `sudo` like Node.
//! Output device labels and I2S vs integrated filtering follow Node `alsa_controller/cards.json`
//! (`crate::alsa_cards`).

use std::path::PathBuf;

use anyhow::Context;
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
            i2s_enabled: false,
            i2s_dac_id: None,
            i2s_dac_label: String::new(),
        }
    }
}

fn state_path() -> PathBuf {
    std::env::var("VOLUMIO_EVO_ALSA_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/var/lib/volumio-evo/alsa-state.toml"))
}

impl AlsaSettings {
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
                    "I2S dtoverlay updated; reboot required for audio device to match catalogue"
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

    serde_json::json!({
        "page": {
            "label": "Playback options"
        },
        "sections": [
            {
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
            }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
