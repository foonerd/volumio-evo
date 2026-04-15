//! ALSA playback device discovery and persisted output selection for Playback Options (stock UI).
//!
//! Full MPD/ALSA apply pipeline (asound, modular snippets) is deferred; we persist the user choice
//! and expose the same Socket.IO / UI shapes as `audio_interface/alsa_controller` on Node.

use std::path::PathBuf;

use anyhow::Context;
use serde::{Deserialize, Serialize};

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
}

impl Default for AlsaSettings {
    fn default() -> Self {
        Self {
            output_device_id: "0".to_string(),
            output_device_label: "Default".to_string(),
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

    /// Apply `saveAlsaOptions` / `setOutputDevices` JSON (`output_device` + optional `i2s`).
    /// I2S enable path is ignored until Evo wires `i2s_dacs`.
    pub fn apply_save_payload(&mut self, data: &serde_json::Value) -> anyhow::Result<()> {
        let od = data
            .get("output_device")
            .context("missing output_device in save payload")?;
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
pub fn coerce_selection(cards: &[AplayCard], mut settings: AlsaSettings) -> AlsaSettings {
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

/// `pushOutputDevices` body (wizard + internal consistency); matches Node `getAudioDevices` without I2S extras.
pub fn push_output_devices_json(cards: &[AplayCard], settings: &AlsaSettings) -> serde_json::Value {
    let available: Vec<serde_json::Value> = cards
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "name": c.name,
            })
        })
        .collect();

    serde_json::json!({
        "devices": {
            "active": {
                "id": settings.output_device_id,
                "name": settings.output_device_label,
            },
            "available": available,
        }
    })
}

/// Stock plugin UI for `audio_interface/alsa_controller` — enough for the output device selector.
pub fn playback_options_ui_config(cards: &[AplayCard], settings: &AlsaSettings) -> serde_json::Value {
    let options: Vec<serde_json::Value> = cards
        .iter()
        .map(|c| {
            serde_json::json!({
                "value": c.id,
                "label": c.name,
            })
        })
        .collect();

    let selected_value = settings.output_device_id.clone();
    let selected_label = settings.output_device_label.clone();

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
                    "data": ["output_device"]
                },
                "value": {
                    "value": selected_value,
                    "label": selected_label
                },
                "content": [
                    {
                        "id": "output_device",
                        "element": "select",
                        "doc": "Choose the ALSA device used for playback.",
                        "label": "Output device",
                        "value": {
                            "value": settings.output_device_id,
                            "label": settings.output_device_label
                        },
                        "visibleIf": {
                            "field": "i2s",
                            "value": false
                        },
                        "options": options
                    },
                    {
                        "id": "i2s",
                        "element": "switch",
                        "doc": "I2S DAC (not used on Evo yet).",
                        "label": "I2S DAC",
                        "hidden": true,
                        "value": false
                    }
                ]
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
