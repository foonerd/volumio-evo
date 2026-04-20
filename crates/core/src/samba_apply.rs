//! Apply SMB configuration to the OS (`smb.conf`, `systemctl`, Unix/Samba users).

use crate::config::Config;
use crate::paths::default_samba_generated_smb_conf_path;
use crate::samba_conf::{self, validate_extra_share_path};
use crate::samba_settings::{SambaSettings, SambaUserRecord};
use serde::Deserialize;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const SMB_USER_SYNC_SCRIPT: &str = "/usr/local/bin/volumio-evo-smb-user-sync.sh";

/// Effective guest mapping for **`guest account`** (SMB guest sessions map to this Unix account).
pub fn smb_guest_unix_user() -> String {
    std::env::var("VOLUMIO_EVO_RUNTIME_USER")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "nobody".to_string())
}

#[cfg(target_os = "linux")]
fn linux_effective_uid() -> Option<u32> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines().find_map(|line| {
        let line = line.trim_start();
        let rest = line.strip_prefix("Uid:")?;
        let eff = rest.split_whitespace().nth(1)?.parse().ok()?;
        Some(eff)
    })
}

fn running_as_root() -> bool {
    #[cfg(target_os = "linux")]
    if let Some(uid) = linux_effective_uid() {
        return uid == 0;
    }
    false
}

fn use_privileged_sudo() -> bool {
    #[cfg(target_os = "linux")]
    if let Some(uid) = linux_effective_uid() {
        return uid != 0;
    }
    std::env::var("VOLUMIO_EVO_RUNTIME_USER")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

fn systemctl_bin() -> String {
    std::env::var("VOLUMIO_EVO_SYSTEMCTL").unwrap_or_else(|_| "/usr/bin/systemctl".to_string())
}

/// Stop **`smbd`** / **`nmbd`** (best effort when SMB is disabled or packages missing).
pub async fn stop_smb_services() -> anyhow::Result<()> {
    let sc = systemctl_bin();
    if use_privileged_sudo() {
        let smbd = tokio::process::Command::new("/usr/bin/sudo")
            .args(["-n", &sc, "stop", "smbd"])
            .status()
            .await;
        let nmbd = tokio::process::Command::new("/usr/bin/sudo")
            .args(["-n", &sc, "stop", "nmbd"])
            .status()
            .await;
        if smbd.map(|s| s.success()).unwrap_or(false) {
            tracing::info!(
                "{} smbd stopped (sudo)",
                crate::log_tags::EVO_NET
            );
        }
        if nmbd.map(|s| s.success()).unwrap_or(false) {
            tracing::info!(
                "{} nmbd stopped (sudo)",
                crate::log_tags::EVO_NET
            );
        }
        return Ok(());
    }
    let _ = tokio::process::Command::new(&sc)
        .args(["stop", "smbd"])
        .status()
        .await;
    let _ = tokio::process::Command::new(&sc)
        .args(["stop", "nmbd"])
        .status()
        .await;
    Ok(())
}

async fn restart_systemd_unit_privileged(systemctl: &str, unit: &str) -> bool {
    if use_privileged_sudo() {
        tokio::process::Command::new("/usr/bin/sudo")
            .args(["-n", systemctl, "restart", unit])
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        tokio::process::Command::new(systemctl)
            .args(["restart", unit])
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

async fn restart_smb_services() -> anyhow::Result<()> {
    let sc = systemctl_bin();
    let mut smbd_ok = restart_systemd_unit_privileged(sc.as_str(), "smbd").await;
    let mut nmbd_ok = restart_systemd_unit_privileged(sc.as_str(), "nmbd").await;
    if smbd_ok && nmbd_ok {
        return Ok(());
    }
    if !use_privileged_sudo() {
        smbd_ok = tokio::process::Command::new("/usr/bin/sudo")
            .args(["-n", sc.as_str(), "restart", "smbd"])
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);
        nmbd_ok = tokio::process::Command::new("/usr/bin/sudo")
            .args(["-n", sc.as_str(), "restart", "nmbd"])
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);
    }
    if smbd_ok && nmbd_ok {
        return Ok(());
    }
    anyhow::bail!(
        "restart smbd/nmbd failed — see docs/OS_PRIVILEGE_MODEL.md (EVO_INSTALL_SAMBA_SUDOERS)"
    )
}

async fn install_generated_to_etc(src: &Path) -> anyhow::Result<()> {
    let dest = "/etc/samba/smb.conf";
    if use_privileged_sudo() {
        let st = tokio::process::Command::new("/usr/bin/sudo")
            .args([
                "-n",
                "/usr/bin/install",
                "-o",
                "root",
                "-g",
                "root",
                "-m",
                "644",
            ])
            .arg(src)
            .arg(dest)
            .status()
            .await?;
        if st.success() {
            return Ok(());
        }
        anyhow::bail!(
            "sudo install smb.conf failed — bootstrap EVO_INSTALL_SAMBA_SUDOERS (see docs/OS_PRIVILEGE_MODEL.md)"
        );
    }
    let st = tokio::process::Command::new("/usr/bin/install")
        .args(["-o", "root", "-g", "root", "-m", "644"])
        .arg(src)
        .arg(dest)
        .status()
        .await?;
    if st.success() {
        Ok(())
    } else {
        anyhow::bail!("install {src:?} -> {dest} failed (run Evo as root or fix permissions)");
    }
}

fn validate_settings_for_apply(settings: &SambaSettings) -> anyhow::Result<()> {
    for es in &settings.extra_shares {
        validate_extra_share_path(es.path.as_str())
            .map_err(|e| anyhow::anyhow!("extra share {:?}: {e}", es.name))?;
    }
    Ok(())
}

/// Rewrite **`/etc/samba/smb.conf`** and reload services, or stop services when disabled.
pub async fn apply_samba_os_configuration(config: &Config, device_name: &str) -> anyhow::Result<()> {
    let settings = SambaSettings::load();
    validate_settings_for_apply(&settings)?;

    if !settings.enabled {
        stop_smb_services().await?;
        tracing::info!(
            "{} SMB server disabled — smbd/nmbd stop attempted",
            crate::log_tags::EVO_NET
        );
        return Ok(());
    }

    let netbios = samba_conf::netbios_name_from_device(device_name);
    let guest = smb_guest_unix_user();
    let body =
        samba_conf::render_smb_conf(&settings, &config.music_sources, netbios.as_str(), guest.as_str());

    let gen_path: PathBuf = default_samba_generated_smb_conf_path();
    if let Some(parent) = gen_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&gen_path, &body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&gen_path, std::fs::Permissions::from_mode(0o644));
    }

    install_generated_to_etc(&gen_path).await?;
    restart_smb_services().await?;
    tracing::info!(
        "{} SMB configuration applied (netbios={})",
        crate::log_tags::EVO_NET,
        netbios
    );
    Ok(())
}

pub fn linux_username_ok(s: &str) -> bool {
    let t = s.trim();
    if t.len() > 32 {
        return false;
    }
    let mut it = t.chars();
    let Some(first) = it.next() else {
        return false;
    };
    if !matches!(first, 'a'..='z' | '_') {
        return false;
    }
    for c in std::iter::once(first).chain(it) {
        if !matches!(c, 'a'..='z' | '0'..='9' | '_' | '-') {
            return false;
        }
    }
    true
}

/// Parsed row from UI / Socket.IO (**password** optional: required for brand-new users).
#[derive(Debug, Deserialize, Clone)]
pub struct IncomingSmbUserRow {
    pub username: String,
    pub password: Option<String>,
}

/// Reconcile Samba login users vs previous records; apply **add** / **delete** via [`SMB_USER_SYNC_SCRIPT`].
pub fn reconcile_samba_user_accounts(
    previous: &[SambaUserRecord],
    rows: &[IncomingSmbUserRow],
) -> anyhow::Result<Vec<SambaUserRecord>> {
    let mut next: Vec<SambaUserRecord> = Vec::new();
    let prev_set: HashSet<String> = previous.iter().map(|u| u.username.clone()).collect();

    let mut incoming_names: HashSet<String> = HashSet::new();
    for row in rows {
        let name = row.username.trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        if !linux_username_ok(name.as_str()) {
            anyhow::bail!("invalid SMB username {:?}", name);
        }
        if !incoming_names.insert(name.clone()) {
            anyhow::bail!("duplicate SMB username {:?}", name);
        }
        let pwd = row.password.as_ref().map(|s| s.as_str()).unwrap_or("");
        let pwd_nonempty = !pwd.trim().is_empty();
        if pwd_nonempty && name == "root" {
            anyhow::bail!("refusing to set password for root via SMB sync");
        }
        if !pwd_nonempty && !prev_set.contains(&name) {
            anyhow::bail!(
                "new SMB user {:?} requires a password on first save",
                name
            );
        }
        next.push(SambaUserRecord {
            username: name.clone(),
        });
    }

    let next_set: HashSet<String> = next.iter().map(|u| u.username.clone()).collect();

    // Deletes first (accounts removed from UI).
    for u in previous {
        if next_set.contains(&u.username) {
            continue;
        }
        sync_user_delete(u.username.as_str())?;
    }

    // Adds / password updates
    for row in rows {
        let name = row.username.trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        let pwd = row.password.as_ref().map(|s| s.as_str()).unwrap_or("");
        if pwd.trim().is_empty() {
            continue;
        }
        sync_user_add(name.as_str(), pwd.trim())?;
    }

    Ok(next)
}

fn sync_user_add(username: &str, password: &str) -> anyhow::Result<()> {
    let mut cmd = if running_as_root() {
        Command::new(SMB_USER_SYNC_SCRIPT)
    } else {
        let mut c = Command::new("/usr/bin/sudo");
        c.args(["-n", SMB_USER_SYNC_SCRIPT]);
        c
    };
    cmd.args(["add", username]);
    cmd.stdin(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| anyhow::anyhow!("spawn smb sync add: {e}"))?;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(password.as_bytes())?;
    stdin.write_all(b"\n")?;
    drop(stdin);
    let st = child.wait()?;
    if !st.success() {
        anyhow::bail!("smbpasswd/useradd failed for {}", username);
    }
    Ok(())
}

fn sync_user_delete(username: &str) -> anyhow::Result<()> {
    let mut cmd = if running_as_root() {
        Command::new(SMB_USER_SYNC_SCRIPT)
    } else {
        let mut c = Command::new("/usr/bin/sudo");
        c.args(["-n", SMB_USER_SYNC_SCRIPT]);
        c
    };
    cmd.args(["delete", username]);
    let st = cmd
        .status()
        .map_err(|e| anyhow::anyhow!("spawn smb sync delete: {e}"))?;
    if !st.success() {
        tracing::warn!(
            "{} smb user delete failed ({:?}) for {}",
            crate::log_tags::EVO_NET,
            st,
            username
        );
    }
    Ok(())
}

