//! Port of Node `alsa_controller/cards.json` (installed as `.../alsa/cards.json`): map `aplay -l` card names to human-readable labels and
//! detect I2S HATs so we can mirror stock behaviour — when **I2S DAC is off**, hide HAT entries and
//! show integrated outputs (HDMI, headphone, …) with proper names.

use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::alsa::{resolve_dac_card_number_using_cards, AplayCard, AlsaSettings};
use crate::i2s;

#[derive(Deserialize)]
struct CardsFile {
    cards: Vec<CardRow>,
}

#[derive(Deserialize)]
struct CardRow {
    name: String,
    /// Multidevice-only rows in Node JSON omit this (pretty names live under `devices[]`).
    #[serde(default)]
    prettyname: String,
    /// Node uses `"i2S"` for HATs; onboard audio is usually `"integrated"`.
    #[serde(default, rename = "type")]
    card_type: String,
}

pub struct AlsaCardCatalog {
    by_name: HashMap<String, CardMeta>,
}

struct CardMeta {
    prettyname: String,
    is_i2s_hat: bool,
}

impl AlsaCardCatalog {
    /// Resolves `cards.json`: optional `VOLUMIO_EVO_ALSA_CARDS_JSON`, else
    /// `{VOLUMIO_EVO_ALSA_DIR}/cards.json`, else dev `layer/config/alsa/cards.json`.
    pub fn load() -> Result<Self> {
        if let Ok(p) = std::env::var("VOLUMIO_EVO_ALSA_CARDS_JSON") {
            if !p.is_empty() {
                let path = std::path::PathBuf::from(p);
                let raw = std::fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                return Self::from_json_str(&raw);
            }
        }
        let primary = canonical_cards_json_path();
        if primary.exists() {
            let raw = std::fs::read_to_string(&primary)
                .with_context(|| format!("read {}", primary.display()))?;
            return Self::from_json_str(&raw);
        }
        let dev = std::path::PathBuf::from("layer/config/alsa/cards.json");
        if dev.exists() {
            let raw = std::fs::read_to_string(&dev)
                .with_context(|| format!("read {}", dev.display()))?;
            return Self::from_json_str(&raw);
        }
        anyhow::bail!(
            "cards.json not found at {} (or layer/config/alsa/cards.json for dev); run bootstrap or set VOLUMIO_EVO_ALSA_CARDS_JSON",
            primary.display()
        );
    }

    pub(crate) fn from_json_str(raw: &str) -> Result<Self> {
        let parsed: CardsFile = serde_json::from_str(raw).context("parse cards.json")?;
        let mut by_name = HashMap::new();
        for c in parsed.cards {
            by_name.insert(
                c.name.trim().to_string(),
                CardMeta {
                    prettyname: c.prettyname,
                    is_i2s_hat: c.card_type.eq_ignore_ascii_case("i2s"),
                },
            );
        }
        Ok(Self { by_name })
    }

    pub fn load_optional() -> Self {
        match Self::load() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    "{} ALSA cards.json catalog unavailable (playback names may be raw): {}",
                    crate::log_tags::EVO_ALSA,
                    e
                );
                Self {
                    by_name: HashMap::new(),
                }
            }
        }
    }

    fn pretty_label(&self, aplay_name: &str) -> Option<String> {
        self.by_name.get(aplay_name.trim()).and_then(|m| {
            if m.prettyname.is_empty() {
                None
            } else {
                Some(m.prettyname.clone())
            }
        })
    }

    fn is_i2s_hat(&self, aplay_name: &str) -> bool {
        self.by_name
            .get(aplay_name.trim())
            .is_some_and(|m| m.is_i2s_hat)
    }
}

fn canonical_cards_json_path() -> std::path::PathBuf {
    let alsa_dir = std::env::var("VOLUMIO_EVO_ALSA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(crate::i2s::DEFAULT_ALSA_SHARE_DIR));
    alsa_dir.join("cards.json")
}

/// Filter and rename cards like Node `getAlsaCards` + `getAlsaCardsWithoutI2SDAC`:
/// - Apply **prettyname** from the catalog when present.
/// - If **I2S is off**: drop entries catalogued as I2S HATs (`type: i2S`) so only “local” outputs remain.
/// - If **I2S is on**: drop the ALSA card for the active DAC (matched by **`alsacard`** vs `aplay -l`, same
///   idea as [`crate::alsa::resolve_dac_card_number`], with `/proc/asound/cards` fallback; then catalogue **`alsanum`** if still unresolved).
pub fn prepare_playback_cards(
    raw: Vec<AplayCard>,
    settings: &AlsaSettings,
    catalog: &AlsaCardCatalog,
    dacs: Option<&i2s::DacsFile>,
    profile: &str,
) -> Vec<AplayCard> {
    let i2s_hat_card_id: Option<String> = if settings.i2s_enabled {
        match (dacs, settings.i2s_dac_id.as_deref()) {
            (Some(dacs), Some(dac_id)) => i2s::find_dac(dacs, profile, dac_id).and_then(|entry| {
                resolve_dac_card_number_using_cards(&raw, entry).or_else(|| {
                    let t = entry.alsanum.trim();
                    if t.is_empty() {
                        None
                    } else {
                        Some(t.to_string())
                    }
                })
            }),
            _ => None,
        }
    } else {
        None
    };

    let mut out = Vec::new();
    for c in raw {
        let skip = if settings.i2s_enabled {
            i2s_hat_card_id.as_ref().is_some_and(|hid| {
                c.id == hid.as_str() || c.id.starts_with(&format!("{hid},"))
            })
        } else {
            catalog.is_i2s_hat(&c.name)
        };
        if skip {
            continue;
        }
        let name = catalog
            .pretty_label(&c.name)
            .unwrap_or_else(|| c.name.clone());
        out.push(AplayCard {
            id: c.id,
            alsacard: c.alsacard,
            name,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_i2s_when_off() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../layer/config/alsa/cards.json");
        let cat = AlsaCardCatalog::from_json_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let raw = vec![
            AplayCard {
                id: "0".into(),
                alsacard: "vc4hdmi0".into(),
                name: "vc4-hdmi-0".into(),
            },
            AplayCard {
                id: "1".into(),
                alsacard: "vc4hdmi1".into(),
                name: "vc4-hdmi-1".into(),
            },
            AplayCard {
                id: "2".into(),
                alsacard: "sndrpihifiberry".into(),
                name: "snd_rpi_hifiberry_dacplushd".into(),
            },
        ];
        let settings = AlsaSettings {
            i2s_enabled: false,
            ..Default::default()
        };
        let v = prepare_playback_cards(raw, &settings, &cat, None, "Raspberry PI");
        assert_eq!(v.len(), 2);
        assert!(v.iter().any(|c| c.name.contains("HDMI 0")));
        assert!(v.iter().all(|c| !c.name.to_lowercase().contains("hifiberry")));
    }

    #[test]
    fn hides_active_i2s_hat_by_alsacard_not_alsanum() {
        let dac_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../layer/config/alsa/dacs.json");
        let dacs: crate::i2s::DacsFile =
            serde_json::from_str(&std::fs::read_to_string(dac_path).unwrap()).unwrap();
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../layer/config/alsa/cards.json");
        let cat = AlsaCardCatalog::from_json_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        let raw = vec![
            AplayCard {
                id: "0".into(),
                alsacard: "bcm2835_headpho".into(),
                name: "bcm2835 Headphones".into(),
            },
            AplayCard {
                id: "1".into(),
                alsacard: "sndrpihifiberry".into(),
                name: "snd_rpi_hifiberry_dacplushd".into(),
            },
            AplayCard {
                id: "2".into(),
                alsacard: "vc4hdmi0".into(),
                name: "vc4-hdmi-0".into(),
            },
        ];
        let settings = AlsaSettings {
            i2s_enabled: true,
            i2s_dac_id: Some("hifiberry-dac2hd".into()),
            output_device_id: "1".into(),
            ..Default::default()
        };
        let v = prepare_playback_cards(raw, &settings, &cat, Some(&dacs), "Raspberry PI");
        assert_eq!(v.len(), 2);
        assert!(!v.iter().any(|c| c.id == "1"));
        assert!(v.iter().any(|c| c.id == "2"));
    }
}
