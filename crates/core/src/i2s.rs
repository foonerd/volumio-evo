//! I2S DAC catalogue (`dacs.json`, same source as Node `system_controller/i2s_dacs`) and boot `dtoverlay`
//! updates. Matches Node banner + `dtoverlay=` block in `/boot/firmware/config.txt` or `/boot/config.txt`.
//! On **Raspberry PI**, enabling an I2S DAC also ensures `dtparam=i2c_arm=on` and `dtparam=i2s=on` are
//! active (stock images often leave them commented).
//!
//! Reads prefer **`fs::read_to_string`** when `/boot/...` is world-readable (usual on Pi). Writes use
//! **`sudo -n tee`** (matches Volumio `volumio-user` sudoers: `NOPASSWD` for `/usr/bin/tee`, not `cat`).

use std::path::Path;
use std::process::{Command, Stdio};
use std::io::Write;

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Deserializer};

/// Same banner as `volumio3-backend/.../i2s_dacs/index.js` (`i2sOverlayBanner` + `\n`).
pub const I2S_BANNER_LINE: &str = "#### Volumio i2s setting below: do not alter ####\n";

/// Packaged ALSA-related JSON (`dacs.json`, `cards.json`) under this directory on device.
/// Override with `VOLUMIO_EVO_ALSA_DIR`, or set `VOLUMIO_EVO_DACS_JSON` to point at `dacs.json` directly.
pub const DEFAULT_ALSA_SHARE_DIR: &str = "/usr/share/volumio-evo/alsa";

/// Banner line + following `dtoverlay=` line (full Volumio I2S block).
const I2S_MANAGED_BLOCK_REGEX: &str = r"(?m)^#### Volumio i2s setting below: do not alter ####\r?\n\s*dtoverlay=[^\r\n]*\r?\n";

#[derive(Debug, Deserialize)]
pub struct DacsFile {
    devices: Vec<DacsHardware>,
}

#[derive(Debug, Deserialize)]
pub struct DacsHardware {
    name: String,
    data: Vec<DacEntry>,
}

/// One row from `dacs.json` `devices[].data[]`.
#[derive(Debug, Deserialize, Clone)]
pub struct DacEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub overlay: String,
    #[serde(default)]
    pub alsanum: String,
    /// Short ALSA id from `aplay -l` / `/proc/asound/cards` brackets (e.g. `sndrpihifiberry`). Used to
    /// resolve the real card index — `alsanum` is a legacy hint and varies between Pi models.
    #[serde(default)]
    pub alsacard: String,
    #[serde(default)]
    pub needsreboot: String,
    /// Stock `dacs.json` uses `""` or, in some device rows, a JSON array of module names.
    #[serde(default, deserialize_with = "deserialize_modules_loose")]
    pub modules: String,
}

fn deserialize_modules_loose<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(deserializer)?;
    Ok(match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Array(a) => a
            .into_iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect::<Vec<_>>()
            .join(","),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Object(_) => String::new(),
    })
}

impl DacEntry {
    /// `dacs.json` uses `"yes"` for most boards; treat empty as no.
    pub fn needs_reboot(&self) -> bool {
        matches!(
            self.needsreboot.to_ascii_lowercase().as_str(),
            "yes" | "true" | "1"
        )
    }
}

fn canonical_dacs_json_path() -> std::path::PathBuf {
    let alsa_dir = std::env::var("VOLUMIO_EVO_ALSA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(DEFAULT_ALSA_SHARE_DIR));
    alsa_dir.join("dacs.json")
}

/// Hardware profile key in `dacs.json` → `devices[].name` (e.g. `Raspberry PI`).
pub fn hardware_profile() -> String {
    std::env::var("VOLUMIO_EVO_DEVICE")
        .unwrap_or_else(|_| "Raspberry PI".to_string())
}

pub fn load_dacs() -> Result<DacsFile> {
    if let Ok(p) = std::env::var("VOLUMIO_EVO_DACS_JSON") {
        if !p.is_empty() {
            let path = std::path::PathBuf::from(p);
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))?;
            return serde_json::from_str(&raw).context("parse dacs.json");
        }
    }
    let primary = canonical_dacs_json_path();
    if primary.exists() {
        let raw = std::fs::read_to_string(&primary)
            .with_context(|| format!("read {}", primary.display()))?;
        return serde_json::from_str(&raw).context("parse dacs.json");
    }
    // Unpackaged dev: git checkout only (same tree as bootstrap installs on device).
    let dev = std::path::PathBuf::from("layer/config/alsa/dacs.json");
    if dev.exists() {
        let raw = std::fs::read_to_string(&dev)
            .with_context(|| format!("read {}", dev.display()))?;
        return serde_json::from_str(&raw).context("parse dacs.json");
    }
    bail!(
        "dacs.json not found at {} (or layer/config/alsa/dacs.json for dev); run bootstrap or set VOLUMIO_EVO_DACS_JSON",
        primary.display()
    );
}

pub fn dac_list_for_profile(dacs: &DacsFile, profile: &str) -> Vec<DacEntry> {
    dacs
        .devices
        .iter()
        .find(|d| d.name == profile)
        .map(|d| d.data.clone())
        .unwrap_or_default()
}

pub fn find_dac<'a>(dacs: &'a DacsFile, profile: &str, dac_id: &str) -> Option<&'a DacEntry> {
    let hw = dacs.devices.iter().find(|d| d.name == profile)?;
    hw.data.iter().find(|e| e.id == dac_id)
}

/// `dtoverlay=` payload only; commas allowed (Pi5 `,slave` etc.).
fn validate_overlay_token(overlay: &str) -> Result<()> {
    if overlay.is_empty() {
        bail!("empty overlay");
    }
    if !overlay.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == ',' || c == '_' || c == '-' || c == '.'
    }) {
        bail!("invalid overlay characters");
    }
    Ok(())
}

/// Prefer Bookworm/Pi layout (`/boot/firmware/config.txt`), then `/boot/config.txt` when present.
pub fn resolved_boot_config_path() -> &'static str {
    if Path::new("/boot/firmware/config.txt").exists() {
        "/boot/firmware/config.txt"
    } else {
        "/boot/config.txt"
    }
}

fn read_boot_config() -> Result<String> {
    let path = resolved_boot_config_path();
    match std::fs::read_to_string(path) {
        Ok(s) => return Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(e) => return Err(e).with_context(|| format!("read {}", path)),
    }
    let out = Command::new("sudo")
        .args(["-n", "cat", path])
        .output()
        .context("sudo cat boot config")?;
    if !out.status.success() {
        bail!(
            "read {} failed (direct and sudo): {} — on minimal images grant volumio read access to /boot or NOPASSWD for /bin/cat",
            path,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn regex_i2s_managed_block() -> Regex {
    Regex::new(I2S_MANAGED_BLOCK_REGEX).expect("I2S managed block regex")
}

/// Remove the Volumio banner + `dtoverlay=` line only (text in memory).
fn strip_volumio_i2s_block(txt: &str) -> String {
    regex_i2s_managed_block()
        .replace_all(txt, "")
        .to_string()
}

/// Remove every standalone `dtoverlay=<overlay>` line (e.g. duplicate under `[all]`).
fn strip_duplicate_dtoverlay_lines(txt: &str, overlay: &str) -> Result<String> {
    let pat = format!(
        r"(?m)^\s*dtoverlay=\s*{}\s*\r?\n",
        regex::escape(overlay)
    );
    let re = Regex::new(&pat).context("dtoverlay dedupe regex")?;
    Ok(re.replace_all(txt, "").to_string())
}

fn is_raspberry_pi_profile() -> bool {
    hardware_profile().trim() == "Raspberry PI"
}

/// Stock Pi images ship with `#dtparam=i2c_arm=on` / `#dtparam=i2s=on` commented; I2S HATs need the
/// I2S clock/data lines and often I2C (ID EEPROM, volume control). Uncomment, flip `off` → `on`, or
/// append missing `dtparam=` lines. Only used for [`Raspberry PI`] in `dacs.json`.
fn ensure_raspberry_pi_i2c_i2s_dtparams(txt: String) -> Result<String> {
    let mut out = txt;
    let re_comment_i2c =
        Regex::new(r"(?m)^(\s*)#\s*dtparam=i2c_arm=on\s*$").context("i2c uncomment")?;
    out = re_comment_i2c
        .replace_all(&out, "dtparam=i2c_arm=on")
        .to_string();
    let re_comment_i2s =
        Regex::new(r"(?m)^(\s*)#\s*dtparam=i2s=on\s*$").context("i2s uncomment")?;
    out = re_comment_i2s
        .replace_all(&out, "dtparam=i2s=on")
        .to_string();

    let re_off_i2c = Regex::new(r"(?m)^(\s*)dtparam=i2c_arm=(off|0|false)\s*$")
        .context("i2c off regex")?;
    out = re_off_i2c
        .replace_all(&out, "dtparam=i2c_arm=on")
        .to_string();
    let re_off_i2s =
        Regex::new(r"(?m)^(\s*)dtparam=i2s=(off|0|false)\s*$").context("i2s off regex")?;
    out = re_off_i2s.replace_all(&out, "dtparam=i2s=on").to_string();

    let has_i2c = Regex::new(r"(?m)^\s*dtparam=i2c_arm=on\s*$")
        .context("i2c active check")?
        .is_match(&out);
    let has_i2s =
        Regex::new(r"(?m)^\s*dtparam=i2s=on\s*$").context("i2s active check")?.is_match(&out);

    if !has_i2c || !has_i2s {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("\n# Volumio Evo: I2S DAC — enable SoC I2C + I2S (do not remove)\n");
        if !has_i2c {
            out.push_str("dtparam=i2c_arm=on\n");
        }
        if !has_i2s {
            out.push_str("dtparam=i2s=on\n");
        }
    }

    Ok(out)
}

fn write_boot_config(content: &str) -> Result<()> {
    let path = resolved_boot_config_path();
    let mut child = Command::new("sudo")
        .args(["-n", "tee", path])
        .stdin(Stdio::piped())
        .spawn()
        .context("sudo tee boot config")?;
    child
        .stdin
        .as_mut()
        .context("stdin")?
        .write_all(content.as_bytes())?;
    let st = child.wait().context("wait tee")?;
    if !st.success() {
        bail!("sudo tee {} failed with {}", path, st);
    }
    Ok(())
}

/// Set or replace the Volumio I2S `dtoverlay=` block (Node `writeI2SDAC`).
///
/// If the same `dtoverlay=<overlay>` already appears elsewhere (e.g. under `[all]` from an image or
/// manual edit), those lines are removed so the overlay is only defined once — in the Volumio block
/// at the end of the file.
///
/// When `hardware_profile()` is `Raspberry PI`, also uncomments or appends `dtparam=i2c_arm=on` and
/// `dtparam=i2s=on` so optional interfaces are enabled for HAT overlays.
pub fn enable_i2s_overlay(overlay: &str) -> Result<()> {
    if overlay.is_empty() {
        bail!("module-based I2S (empty overlay) is not implemented in Evo yet");
    }
    validate_overlay_token(overlay)?;
    let mut txt = read_boot_config()?;
    txt = strip_volumio_i2s_block(&txt);
    txt = strip_duplicate_dtoverlay_lines(&txt, overlay)?;
    if is_raspberry_pi_profile() {
        txt = ensure_raspberry_pi_i2c_i2s_dtparams(txt)?;
    }
    if !txt.ends_with('\n') {
        txt.push('\n');
    }
    txt.push('\n');
    txt.push_str(&format!("{}dtoverlay={}\n", I2S_BANNER_LINE, overlay));

    write_boot_config(&txt)?;
    tracing::info!(
        "{} I2S dtoverlay written to {} (reboot usually required)",
        crate::log_tags::EVO_I2S,
        resolved_boot_config_path()
    );
    Ok(())
}

/// Remove the Volumio I2S block (`disableI2SDAC`).
pub fn disable_i2s_overlay() -> Result<()> {
    let txt = read_boot_config()?;
    let new_txt = strip_volumio_i2s_block(&txt);
    if new_txt != txt {
        write_boot_config(&new_txt)?;
        tracing::info!(
            "{} I2S dtoverlay removed from {}",
            crate::log_tags::EVO_I2S,
            resolved_boot_config_path()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_validation() {
        assert!(validate_overlay_token("hifiberry-dac").is_ok());
        assert!(validate_overlay_token("hifiberry-dacplus-std,slave").is_ok());
        assert!(validate_overlay_token("bad path").is_err());
    }

    #[test]
    fn enable_removes_duplicate_dtoverlay_before_banner() {
        let sample = r#"[cm5]
dtoverlay=dwc2,dr_mode=host

[all]
dtoverlay=hifiberry-dacplushd

#### Volumio i2s setting below: do not alter ####
dtoverlay=hifiberry-dacplushd
"#;
        let mut t = strip_volumio_i2s_block(sample);
        t = strip_duplicate_dtoverlay_lines(&t, "hifiberry-dacplushd").unwrap();
        assert_eq!(
            t.matches("dtoverlay=hifiberry-dacplushd").count(),
            0,
            "all duplicate lines removed before append"
        );
        if !t.ends_with('\n') {
            t.push('\n');
        }
        t.push('\n');
        t.push_str(&format!("{}dtoverlay=hifiberry-dacplushd\n", I2S_BANNER_LINE));
        assert_eq!(t.matches("dtoverlay=hifiberry-dacplushd").count(), 1);
        assert!(t.contains(I2S_BANNER_LINE.trim_end()));
        assert!(t.contains("dtoverlay=dwc2"));
    }

    /// Regression: full stock `dacs.json` must parse (some rows use `modules` as a JSON array).
    #[test]
    fn layer_dacs_json_parses() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../layer/config/alsa/dacs.json");
        let raw = std::fs::read_to_string(&p).expect("read layer/config/alsa/dacs.json");
        let parsed: DacsFile = serde_json::from_str(&raw).expect("parse dacs.json");
        assert!(
            parsed
                .devices
                .iter()
                .any(|d| d.name == "Raspberry PI" && !d.data.is_empty()),
            "expected Raspberry PI section"
        );
    }

    #[test]
    fn raspberry_pi_uncomments_stock_i2c_i2s_dtparams() {
        let sample = "# Optional hardware\n#dtparam=i2c_arm=on\n#dtparam=i2s=on\n";
        let out = ensure_raspberry_pi_i2c_i2s_dtparams(sample.to_string()).unwrap();
        assert!(out.contains("\ndtparam=i2c_arm=on\n") || out.starts_with("dtparam=i2c_arm=on"));
        assert!(out.lines().any(|l| l.trim() == "dtparam=i2c_arm=on"));
        assert!(out.lines().any(|l| l.trim() == "dtparam=i2s=on"));
        assert!(!out.contains("#dtparam=i2c_arm=on"));
        assert!(!out.contains("#dtparam=i2s=on"));
    }

    #[test]
    fn raspberry_pi_appends_dtparams_when_absent() {
        let sample = "[all]\nenable_uart=1\n";
        let out = ensure_raspberry_pi_i2c_i2s_dtparams(sample.to_string()).unwrap();
        assert!(out.contains("dtparam=i2c_arm=on"));
        assert!(out.contains("dtparam=i2s=on"));
        assert!(out.contains("Volumio Evo: I2S DAC"));
    }

    #[test]
    fn raspberry_pi_flips_dtparam_off_to_on() {
        let sample = "dtparam=i2c_arm=off\ndtparam=i2s=off\n";
        let out = ensure_raspberry_pi_i2c_i2s_dtparams(sample.to_string()).unwrap();
        assert_eq!(
            out.matches("dtparam=i2c_arm=on").count(),
            1,
            "{out}"
        );
        assert_eq!(out.matches("dtparam=i2s=on").count(), 1);
    }
}
