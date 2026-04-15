//! I2S DAC catalogue (`dacs.json`, same source as Node `system_controller/i2s_dacs`) and boot `dtoverlay`
//! updates. Matches Node banner + `dtoverlay=` block in `/boot/firmware/config.txt` or `/boot/config.txt`.
//!
//! Reads prefer **`fs::read_to_string`** when `/boot/...` is world-readable (usual on Pi). Writes use
//! **`sudo -n tee`** (matches Volumio `volumio-user` sudoers: `NOPASSWD` for `/usr/bin/tee`, not `cat`).

use std::path::Path;
use std::process::{Command, Stdio};
use std::io::Write;

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Deserialize;

/// Same banner as `volumio3-backend/.../i2s_dacs/index.js` (`i2sOverlayBanner` + `\n`).
pub const I2S_BANNER_LINE: &str = "#### Volumio i2s setting below: do not alter ####\n";

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
    #[serde(default)]
    pub needsreboot: String,
    #[serde(default)]
    pub modules: String,
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

fn default_dacs_path() -> std::path::PathBuf {
    std::env::var("VOLUMIO_EVO_DACS_JSON")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/usr/share/volumio-evo/dacs.json"))
}

/// Hardware profile key in `dacs.json` → `devices[].name` (e.g. `Raspberry PI`).
pub fn hardware_profile() -> String {
    std::env::var("VOLUMIO_EVO_DEVICE")
        .unwrap_or_else(|_| "Raspberry PI".to_string())
}

pub fn load_dacs() -> Result<DacsFile> {
    let path = default_dacs_path();
    let raw = if path.exists() {
        std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?
    } else {
        // Dev / unpackaged: try next to repo layer
        let fallback = std::path::PathBuf::from("layer/config/dacs.json");
        if fallback.exists() {
            std::fs::read_to_string(&fallback)
                .with_context(|| format!("read {}", fallback.display()))?
        } else {
            bail!(
                "dacs.json not found at {} or layer/config/dacs.json; set VOLUMIO_EVO_DACS_JSON",
                path.display()
            );
        }
    };
    let parsed: DacsFile = serde_json::from_str(&raw).context("parse dacs.json")?;
    Ok(parsed)
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

/// Prefer Bookworm/Pi layout, then legacy `/boot/config.txt` (often a symlink).
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
pub fn enable_i2s_overlay(overlay: &str) -> Result<()> {
    if overlay.is_empty() {
        bail!("module-based I2S (empty overlay) is not implemented in Evo yet");
    }
    validate_overlay_token(overlay)?;
    let mut txt = read_boot_config()?;
    let block = format!("{}dtoverlay={}\n", I2S_BANNER_LINE, overlay);

    let re = Regex::new(
        r"(?m)^#### Volumio i2s setting below: do not alter ####\r?\n\s*dtoverlay=[^\r\n]*\r?\n",
    )
    .expect("i2s block regex");

    let new_txt = if re.is_match(&txt) {
        re.replace(&txt, block.as_str()).to_string()
    } else {
        if !txt.ends_with('\n') {
            txt.push('\n');
        }
        txt.push('\n');
        txt.push_str(&block);
        txt
    };

    write_boot_config(&new_txt)?;
    tracing::info!(
        "I2S dtoverlay written to {} (reboot usually required)",
        resolved_boot_config_path()
    );
    Ok(())
}

/// Remove the Volumio I2S block (`disableI2SDAC`).
pub fn disable_i2s_overlay() -> Result<()> {
    let txt = read_boot_config()?;
    let re = Regex::new(
        r"(?m)^#### Volumio i2s setting below: do not alter ####\r?\n\s*dtoverlay=[^\r\n]*\r?\n",
    )
    .expect("i2s block regex");
    let new_txt = re.replace_all(&txt, "").to_string();
    if new_txt != txt {
        write_boot_config(&new_txt)?;
        tracing::info!("I2S dtoverlay removed from {}", resolved_boot_config_path());
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
}
