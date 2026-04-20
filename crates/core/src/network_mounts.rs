//! NAS / SMB / NFS mounts (stock Volumio `system_controller/networkfs` behaviour).
//! Shares persist under `settings/mounts/shares.toml`; mount points match Node: `/mnt/NAS/<sanitized_alias>`.
//!
//! **Boot:** [`NetworkMounts::mount_all_at_boot`] waits until NetworkManager reports usable L3 (**full**
//! / **limited** connectivity, or global **STATE=connected** — the latter suits slow Wi‑Fi where the
//! captive/connectivity probe lags DHCP), then retries transient “network unreachable” errors once.
//!
//! **Runtime:** A background task periodically calls [`NetworkMounts::remount_unmounted_shares_best_effort`]
//! so shares come online after the user changes network or moves the device (no reboot).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::config::Config;
use crate::mpd::{self, MpdConfig};
use crate::paths;

/// Root for CIFS/NFS mounts (same as volumio3-backend `networkfs`).
pub const NAS_MOUNT_ROOT: &str = "/mnt/NAS";

/// Ensure `music_root/NAS` → [`NAS_MOUNT_ROOT`] so MPD `music_directory` layout (`NAS/<alias>/…`) matches
/// actual mounts under `/mnt/NAS/<alias>/…`. Replaces an **empty** `NAS` directory (bootstrap placeholder).
pub fn ensure_music_library_nas_symlink(music_root: &Path) -> std::io::Result<()> {
    #[cfg(not(unix))]
    {
        let _ = music_root;
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::fs;
        use std::os::unix::fs::symlink;
        let nas_link = music_root.join("NAS");
        let target = Path::new(NAS_MOUNT_ROOT);
        if let Ok(meta) = fs::symlink_metadata(&nas_link) {
            if meta.file_type().is_symlink() {
                if fs::read_link(&nas_link).ok().as_ref() == Some(&target.to_path_buf()) {
                    return Ok(());
                }
                fs::remove_file(&nas_link)?;
            } else if meta.is_dir() {
                let empty = fs::read_dir(&nas_link)
                    .map(|mut d| d.next().is_none())
                    .unwrap_or(false);
                if empty {
                    fs::remove_dir(&nas_link)?;
                } else {
                    tracing::warn!(
                        "{} {} is a non-empty directory; not replacing with symlink to {} (empty NAS/ first, or merge manually)",
                        crate::log_tags::EVO_UI,
                        nas_link.display(),
                        target.display()
                    );
                    return Ok(());
                }
            } else {
                tracing::warn!(
                    "{} {} exists and is not a directory or symlink; skipping NAS bridge",
                    crate::log_tags::EVO_UI,
                    nas_link.display()
                );
                return Ok(());
            }
        }
        if let Some(parent) = nas_link.parent() {
            fs::create_dir_all(parent)?;
        }
        symlink(target, &nas_link)?;
        tracing::info!(
            "{} linked {} -> {} (MPD music_directory ↔ /mnt/NAS mounts)",
            crate::log_tags::EVO_UI,
            nas_link.display(),
            target.display()
        );
        Ok(())
    }
}

/// Automatic SMB dialect ladder (lowest acceptable first). SMB1/NT1 only via explicit `vers=` in advanced options.
/// See `mount.cifs(8)` — kernel accepts these `vers=` values.
const CIFS_VERS_PROBE_LADDER: &[&str] = &["2.0", "2.1", "3.0", "3.02", "3.1.1"];

fn options_has_explicit_vers(opts: &str) -> bool {
    for part in opts.split(',') {
        let p = part.trim();
        if let Some((k, _)) = p.split_once('=') {
            if k.trim().eq_ignore_ascii_case("vers") {
                return true;
            }
        }
    }
    false
}

/// Strips `vers=…` tokens so the probe loop can inject `vers=` per attempt. Other advanced options are kept.
fn options_without_vers(opts: &str) -> String {
    opts.split(',')
        .filter(|p| {
            let t = p.trim();
            !t.split_once('=')
                .map(|(k, _)| k.trim().eq_ignore_ascii_case("vers"))
                .unwrap_or(false)
        })
        .filter(|p| !p.trim().is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

fn merge_vers_into_options(options: &mut String, vers: &str) {
    let cleaned = options_without_vers(options.as_str());
    if cleaned.is_empty() {
        *options = format!("vers={vers}");
    } else {
        *options = format!("{cleaned},vers={vers}");
    }
}

/// `nmcli -t -f STATE general` → `connected` / `disconnected` / …
async fn nm_general_state() -> Option<String> {
    let out = Command::new("nmcli")
        .args(["-t", "-f", "STATE", "general"])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

/// `nmcli -t -f CONNECTIVITY general` → `full` / `limited` / …
async fn nm_connectivity_state() -> Option<String> {
    let out = Command::new("nmcli")
        .args(["-t", "-f", "CONNECTIVITY", "general"])
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

/// **full** / **limited**: good. **STATE=connected**: NM has brought up a profile (Wi‑Fi often reaches
/// this before `CONNECTIVITY` becomes `full` on slow links / Pi 2-class hardware).
fn nm_reports_ready_for_nas(connectivity: Option<&str>, general_state: Option<&str>) -> bool {
    if matches!(
        connectivity,
        Some(c) if c == "full" || c == "limited"
    ) {
        return true;
    }
    matches!(general_state, Some(s) if s == "connected")
}

/// Wait until NetworkManager reports a state where LAN access is plausible, or timeout.
/// Avoids mounting NFS/CIFS while the stack still has **no route** (daemon often starts before DHCP).
async fn wait_for_network_before_nas_mounts() {
    // Pi 2 + Wi‑Fi: association + DHCP + slow connectivity checks can exceed 90s.
    const MAX_WAIT: Duration = Duration::from_secs(180);
    const POLL: Duration = Duration::from_millis(500);

    let nmcli_ok = Command::new("nmcli")
        .args(["-t", "-f", "CONNECTIVITY", "general"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !nmcli_ok {
        tracing::warn!(
            "{} boot: nmcli unavailable; waiting 10s before NAS mounts",
            crate::log_tags::EVO_UI
        );
        tokio::time::sleep(Duration::from_secs(10)).await;
        return;
    }

    let start = std::time::Instant::now();
    loop {
        if start.elapsed() >= MAX_WAIT {
            tracing::warn!(
                "{} boot: NM not ready (full/limited or state=connected) after {:?}; attempting NAS mounts anyway",
                crate::log_tags::EVO_UI,
                MAX_WAIT
            );
            return;
        }
        let conn = nm_connectivity_state().await;
        let st = nm_general_state().await;
        let conn_s = conn.as_deref();
        let st_s = st.as_deref();
        if nm_reports_ready_for_nas(conn_s, st_s) {
            tracing::info!(
                "{} boot: network ready for NAS (CONNECTIVITY={:?} STATE={:?})",
                crate::log_tags::EVO_UI,
                conn_s,
                st_s
            );
            return;
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Errors that usually clear once L3 routing exists (same boot window as “Network is unreachable”).
fn mount_error_likely_transient_network(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("network is unreachable")
        || m.contains("no route to host")
        || m.contains("name or service not known")
        || m.contains("couldn't resolve host")
        || m.contains("connection timed out")
        || m.contains("resource temporarily unavailable")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SharesFile {
    #[serde(default)]
    pub shares: Vec<ShareRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareRecord {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub path: String,
    pub fstype: String,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub options: String,
}

#[derive(Debug)]
pub enum MountOutcome {
    Success,
    PermissionDenied,
    Fail(String),
}

struct CifsMountInner {
    outcome: MountOutcome,
    /// When probing found a working dialect, persist this `vers=` into `shares.toml` (unless user already set `vers=`).
    persist_vers: Option<String>,
}

pub enum AddShareResult {
    Duplicate,
    Mounted { name: String },
    NeedCredentials {
        id: String,
        name: String,
        username: String,
        password: String,
    },
    MountError { name: String, reason: String },
}

/// Coordinates load/save and mount operations.
pub struct NetworkMounts {
    shares_path: PathBuf,
    creds_dir: PathBuf,
    op: Mutex<()>,
}

impl NetworkMounts {
    pub fn new() -> Self {
        let base = paths::settings_dir().join("mounts");
        Self {
            shares_path: base.join("shares.toml"),
            creds_dir: base,
            op: Mutex::new(()),
        }
    }

    fn ensure_layout(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.creds_dir)?;
        std::fs::create_dir_all(NAS_MOUNT_ROOT)?;
        Ok(())
    }

    pub async fn load(&self) -> anyhow::Result<SharesFile> {
        let _g = self.op.lock().await;
        self.load_unlocked()
    }

    fn load_unlocked(&self) -> anyhow::Result<SharesFile> {
        self.ensure_layout()?;
        if !self.shares_path.exists() {
            return Ok(SharesFile::default());
        }
        let raw = std::fs::read_to_string(&self.shares_path)?;
        let v: SharesFile = toml::from_str(&raw).unwrap_or_default();
        Ok(v)
    }

    fn save_unlocked(&self, file: &SharesFile) -> anyhow::Result<()> {
        self.ensure_layout()?;
        let raw = toml::to_string_pretty(file)?;
        let tmp = self.shares_path.with_extension("toml.tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, &self.shares_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&self.shares_path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o600);
                let _ = std::fs::set_permissions(&self.shares_path, perms);
            }
        }
        Ok(())
    }

    /// Sanitized folder name under `/mnt/NAS/` (matches Node).
    pub fn mount_id(name: &str) -> String {
        name.chars()
            .map(|c| if c.is_whitespace() || c == '\\' { '_' } else { c })
            .collect()
    }

    pub fn mountpoint_for_name(name: &str) -> PathBuf {
        Path::new(NAS_MOUNT_ROOT).join(Self::mount_id(name))
    }

    fn cred_path(&self, id: &str) -> PathBuf {
        self.creds_dir.join(format!("cifs-{id}.cred"))
    }

    async fn write_cifs_cred_file(&self, id: &str, user: &str, pass: &str) -> anyhow::Result<PathBuf> {
        let p = self.cred_path(id);
        let mut f = tokio::fs::File::create(&p).await?;
        f.write_all(format!("username={user}\npassword={pass}\n").as_bytes())
            .await?;
        f.sync_all().await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&p)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&p, perms)?;
        }
        Ok(p)
    }

    fn remove_creds_if_any(&self, id: &str) {
        let _ = std::fs::remove_file(self.cred_path(id));
    }

    /// Returns whether `path` is a mount point (best-effort).
    pub fn is_mounted(path: &Path) -> bool {
        std::process::Command::new("mountpoint")
            .arg("-q")
            .arg(path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn df_used_human(path: &Path) -> String {
        if !Self::is_mounted(path) {
            return String::new();
        }
        let out = std::process::Command::new("df")
            .args(["-B1", "--output=used"])
            .arg(path)
            .output();
        let Ok(out) = out else {
            return String::new();
        };
        if !out.status.success() {
            return String::new();
        }
        let line = String::from_utf8_lossy(&out.stdout);
        let last = line.lines().last().unwrap_or("");
        let bytes: u64 = last.trim().parse().unwrap_or(0);
        if bytes == 0 {
            return "0.00 MB".to_string();
        }
        let mut mb = bytes as f64 / (1024.0 * 1024.0);
        let mut unit = "MB";
        if mb > 1024.0 {
            mb /= 1024.0;
            unit = "GB";
            if mb > 1024.0 {
                mb /= 1024.0;
                unit = "TB";
            }
        }
        format!("{mb:.2} {unit}")
    }

    /// Build `pushListShares` entries (same fields as Node `getMountSize` / `listShares`).
    pub async fn list_shares_json(&self) -> Vec<serde_json::Value> {
        let _g = self.op.lock().await;
        let Ok(file) = self.load_unlocked() else {
            return vec![];
        };
        let mut out = Vec::with_capacity(file.shares.len());
        for sh in &file.shares {
            let mp = Self::mountpoint_for_name(&sh.name);
            let mounted = Self::is_mounted(&mp);
            let size = Self::df_used_human(&mp);
            out.push(serde_json::json!({
                "path": sh.path,
                "ip": sh.ip,
                "name": sh.name,
                "fstype": sh.fstype,
                "username": sh.user,
                "password": sh.password,
                "options": sh.options,
                "id": sh.id,
                "mounted": mounted,
                "size": size,
            }));
        }
        out
    }

    fn normalize_path_for_fstype(fstype: &str, path: &str) -> String {
        let fst = fstype.to_ascii_lowercase();
        if fst == "cifs" {
            path.replace("//", "/")
                .trim_matches('/')
                .split('/')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("/")
        } else if fst == "nfs" {
            let mut p = path.trim().to_string();
            if !p.starts_with('/') {
                p = format!("/{p}");
            }
            p
        } else {
            path.to_string()
        }
    }

    fn find_duplicate<'a>(
        file: &'a SharesFile,
        name: &str,
        ip: &str,
        path: &str,
        except_id: Option<&str>,
    ) -> Option<&'a ShareRecord> {
        file.shares.iter().find(|s| {
            s.name == name
                && s.ip == ip
                && s.path == path
                && except_id.map(|ex| s.id.as_str() != ex).unwrap_or(true)
        })
    }

    /// Mount one share (sudo). On success, triggers MPD library update.
    /// CIFS: probes `vers=` from 2.0 upward unless advanced options already contain `vers=` (then that wins).
    /// After a successful probe, persists `vers=` into `shares.toml` so the next boot does a single mount.
    pub async fn mount_share_record(
        &self,
        cfg: &Config,
        rec: &ShareRecord,
    ) -> Result<MountOutcome, String> {
        let (outcome, persist_vers) = {
            let _g = self.op.lock().await;
            self.ensure_layout().map_err(|e| e.to_string())?;
            let mp = Self::mountpoint_for_name(&rec.name);
            std::fs::create_dir_all(&mp).map_err(|e| e.to_string())?;
            let fst = rec.fstype.to_ascii_lowercase();
            match fst.as_str() {
                "cifs" => {
                    let inner = self.mount_cifs(rec, &mp).await?;
                    (inner.outcome, inner.persist_vers)
                }
                "nfs" => (self.mount_nfs(rec, &mp).await?, None),
                _ => return Err(format!("unsupported fstype: {}", rec.fstype)),
            }
        };

        if matches!(outcome, MountOutcome::Success) {
            if let Err(e) = ensure_music_library_nas_symlink(&cfg.music_sources.music_root) {
                tracing::warn!(
                    "{} could not symlink music_root/NAS -> {}: {}",
                    crate::log_tags::EVO_UI,
                    NAS_MOUNT_ROOT,
                    e
                );
            }
            if let Some(ref v) = persist_vers {
                if let Err(e) = self.persist_probed_cifs_vers(&rec.id, v).await {
                    tracing::warn!(
                        "{} could not persist probed SMB vers={}: {}",
                        crate::log_tags::EVO_UI,
                        v,
                        e
                    );
                }
            }
            let mpd_cfg = MpdConfig {
                host: cfg.mpd_host.clone(),
                port: cfg.mpd_port,
            };
            if let Err(e) = mpd::update_connected(&mpd_cfg, None).await {
                tracing::warn!(
                    "{} NAS mount ok but MPD update failed: {}",
                    crate::log_tags::EVO_DB,
                    e
                );
            }
        }

        Ok(outcome)
    }

    async fn persist_probed_cifs_vers(&self, id: &str, vers: &str) -> anyhow::Result<()> {
        let _g = self.op.lock().await;
        let mut file = self.load_unlocked()?;
        let Some(sh) = file.shares.iter_mut().find(|s| s.id == id) else {
            return Ok(());
        };
        merge_vers_into_options(&mut sh.options, vers);
        self.save_unlocked(&file)?;
        Ok(())
    }

    /// Build leading CIFS options (credentials or guest + common flags). Does not include user advanced options or `vers=`.
    async fn cifs_base_opts(&self, rec: &ShareRecord) -> Result<String, String> {
        let mut opts = String::new();
        let use_cred = !rec.user.is_empty() || !rec.password.is_empty();
        if use_cred {
            let cred = self
                .write_cifs_cred_file(&rec.id, &rec.user, &rec.password)
                .await
                .map_err(|e| e.to_string())?;
            opts.push_str(&format!("credentials={},", cred.display()));
        } else {
            opts.push_str("guest,");
        }
        opts.push_str("ro,dir_mode=0777,file_mode=0666,iocharset=utf8,noauto,soft");
        Ok(opts)
    }

    async fn cifs_mount_once(
        source: &str,
        mp: &Path,
        opts: &str,
    ) -> Result<MountOutcome, String> {
        let out = Command::new("sudo")
            .args(["-n", "/usr/bin/mount", "-t", "cifs", source])
            .arg(mp)
            .args(["-o", opts])
            .output()
            .await
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(MountOutcome::Success);
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let combined = format!("{stdout}{stderr}");
        if combined.contains("Permission denied")
            || combined.to_ascii_lowercase().contains("permission denied")
        {
            return Ok(MountOutcome::PermissionDenied);
        }
        Ok(MountOutcome::Fail(
            combined.trim().to_string().chars().take(500).collect(),
        ))
    }

    async fn mount_cifs(&self, rec: &ShareRecord, mp: &Path) -> Result<CifsMountInner, String> {
        let path_part = rec.path.trim().trim_matches('/');
        let source = format!("//{}/{path_part}", rec.ip.trim());
        let base = self.cifs_base_opts(rec).await?;
        let user_opts = rec.options.trim();

        // Advanced options: if `vers=` is set, use exactly one mount attempt (SMB1 etc. only here).
        if options_has_explicit_vers(user_opts) {
            let mut opts = base;
            if !user_opts.is_empty() {
                opts.push(',');
                opts.push_str(user_opts);
            }
            tracing::debug!(
                "{} CIFS {:?}: single mount (explicit vers in options)",
                crate::log_tags::EVO_UI,
                rec.name
            );
            let outcome = Self::cifs_mount_once(&source, mp, &opts).await?;
            return Ok(CifsMountInner {
                outcome,
                persist_vers: None,
            });
        }

        // Automatic ladder: 2.0 → 2.1 → … — inject `vers=`; keep other advanced options (no vers).
        let user_extra = options_without_vers(user_opts);
        let mut last_fail = String::from("mount failed for all probed SMB versions");
        for vers in CIFS_VERS_PROBE_LADDER {
            let mut opts = base.clone();
            if !user_extra.is_empty() {
                opts.push(',');
                opts.push_str(&user_extra);
            }
            opts.push_str(&format!(",vers={vers}"));
            tracing::debug!(
                "{} CIFS {:?}: trying vers={vers}",
                crate::log_tags::EVO_UI,
                rec.name
            );
            match Self::cifs_mount_once(&source, mp, &opts).await? {
                MountOutcome::Success => {
                    tracing::info!(
                        "{} CIFS {:?}: mounted with vers={vers} (auto probe)",
                        crate::log_tags::EVO_UI,
                        rec.name
                    );
                    return Ok(CifsMountInner {
                        outcome: MountOutcome::Success,
                        persist_vers: Some(vers.to_string()),
                    });
                }
                MountOutcome::PermissionDenied => {
                    return Ok(CifsMountInner {
                        outcome: MountOutcome::PermissionDenied,
                        persist_vers: None,
                    });
                }
                MountOutcome::Fail(msg) => {
                    last_fail = msg;
                    continue;
                }
            }
        }

        Ok(CifsMountInner {
            outcome: MountOutcome::Fail(last_fail),
            persist_vers: None,
        })
    }

    async fn mount_nfs(&self, rec: &ShareRecord, mp: &Path) -> Result<MountOutcome, String> {
        let remote = format!("{}:{}", rec.ip.trim(), rec.path.trim());
        let mut opts = "ro,soft,noauto".to_string();
        if !rec.options.trim().is_empty() {
            opts.push(',');
            opts.push_str(rec.options.trim());
        }
        let out = Command::new("sudo")
            .args(["-n", "/usr/bin/mount", "-t", "nfs", &remote])
            .arg(mp)
            .args(["-o", &opts])
            .output()
            .await
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(MountOutcome::Success);
        }
        let msg = String::from_utf8_lossy(&out.stderr).to_string();
        Ok(MountOutcome::Fail(msg.chars().take(500).collect()))
    }

    /// Unmount `/mnt/NAS/<alias>/` the same way volumio3-backend does: **`umount -f`** (see
    /// `ControllerNetworkfs.prototype.umountShare`), with **`-l`** fallback for busy CIFS.
    ///
    /// Only removes the mountpoint directory when it is **no longer** a mount point (never
    /// `rm -rf` across an active mount).
    pub async fn umount_by_name(&self, name: &str) -> Result<(), String> {
        let mp = Self::mountpoint_for_name(name);
        let _g = self.op.lock().await;
        Self::umount_mountpoint_robust(&mp).await
    }

    /// Unmount all configured SMB/NFS shares (volumio3-backend `umountAllShares` / `onVolumioShutdown`).
    /// Logs per-share failures and continues so shutdown can still proceed.
    pub async fn umount_all_shares(&self) -> Result<(), String> {
        let names: Vec<String> = {
            let _g = self.op.lock().await;
            let file = self.load_unlocked().map_err(|e| e.to_string())?;
            file.shares.iter().map(|s| s.name.clone()).collect()
        };
        for name in names {
            if let Err(e) = self.umount_by_name(&name).await {
                tracing::warn!(
                    "{} umount_all_shares {:?}: {}",
                    crate::log_tags::EVO_UI,
                    name,
                    e
                );
            }
        }
        Ok(())
    }

    async fn umount_mountpoint_robust(mp: &Path) -> Result<(), String> {
        let path_str = mp.to_str().ok_or("invalid mount path")?;

        if !Self::is_mounted(mp) {
            let _ = std::fs::remove_dir_all(mp);
            return Ok(());
        }

        async fn umount_output(args: &[&str], target: &str) -> (bool, String) {
            let mut cmd = Command::new("sudo");
            cmd.args(["-n", "/usr/bin/umount"]);
            for a in args {
                cmd.arg(a);
            }
            cmd.arg(target);
            match cmd.output().await {
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    let combined = if err.is_empty() { out } else { err };
                    (o.status.success(), combined)
                }
                Err(e) => (false, e.to_string()),
            }
        }

        // Match Node: `sudo /bin/umount -f <mountpoint>` (networkfs `umountShare`).
        let (ok, e1) = umount_output(&["-f"], path_str).await;
        if ok {
            if !Self::is_mounted(mp) {
                let _ = std::fs::remove_dir_all(mp);
            }
            return Ok(());
        }

        // Busy CIFS: lazy detach (may take a moment to drop from mount tables).
        let (ok2, e2) = umount_output(&["-l"], path_str).await;
        if ok2 {
            for _ in 0..20 {
                if !Self::is_mounted(mp) {
                    let _ = std::fs::remove_dir_all(mp);
                    return Ok(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
            if !Self::is_mounted(mp) {
                let _ = std::fs::remove_dir_all(mp);
            }
            return Ok(());
        }

        let (ok3, e3) = umount_output(&[], path_str).await;
        if ok3 {
            if !Self::is_mounted(mp) {
                let _ = std::fs::remove_dir_all(mp);
            }
            return Ok(());
        }

        let detail = [e1, e2, e3]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
        Err(format!(
            "umount failed: {}",
            detail.chars().take(450).collect::<String>()
        ))
    }

    /// Add share: persist, then mount.
    pub async fn add_share(
        &self,
        cfg: &Config,
        name: String,
        ip: String,
        mut path: String,
        fstype: String,
        username: String,
        password: String,
        options: String,
    ) -> Result<AddShareResult, String> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("Shares must have an alias".to_string());
        }
        if name.contains('/') {
            return Err("Share names cannot contain /".to_string());
        }
        let fst = fstype.to_ascii_lowercase();
        if fst != "cifs" && fst != "nfs" {
            return Err("fstype must be cifs or nfs".to_string());
        }
        path = Self::normalize_path_for_fstype(&fst, &path);
        if path.is_empty() {
            return Err("Share path must be defined".to_string());
        }

        let ip = ip.trim().to_string();

        let _g = self.op.lock().await;
        let mut file = self.load_unlocked().map_err(|e| e.to_string())?;
        if Self::find_duplicate(&file, &name, &ip, &path, None).is_some() {
            return Ok(AddShareResult::Duplicate);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let rec = ShareRecord {
            id: id.clone(),
            name: name.clone(),
            ip,
            path,
            fstype: fst,
            user: username,
            password,
            options,
        };
        file.shares.push(rec.clone());
        self.save_unlocked(&file).map_err(|e| e.to_string())?;
        drop(_g);

        match self.mount_share_record(cfg, &rec).await {
            Ok(MountOutcome::Success) => Ok(AddShareResult::Mounted { name: rec.name }),
            Ok(MountOutcome::PermissionDenied) => Ok(AddShareResult::NeedCredentials {
                id: rec.id,
                name: rec.name,
                username: rec.user,
                password: rec.password,
            }),
            Ok(MountOutcome::Fail(reason)) => Ok(AddShareResult::MountError {
                name: rec.name,
                reason,
            }),
            Err(e) => Err(e),
        }
    }

    /// Remove a persisted share: **unmount first**, then drop from `shares.toml` (same order as
    /// volumio3-backend `deleteShare` — config is only deleted after a successful unmount when mounted).
    pub async fn delete_share(&self, cfg: &Config, id: &str) -> Result<(), String> {
        let name: String = {
            let _g = self.op.lock().await;
            let file = self.load_unlocked().map_err(|e| e.to_string())?;
            file.shares
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.name.clone())
                .ok_or_else(|| "share not found".to_string())?
        };

        self.umount_by_name(&name).await?;

        {
            let _g = self.op.lock().await;
            let mut file = self.load_unlocked().map_err(|e| e.to_string())?;
            let pos = file
                .shares
                .iter()
                .position(|s| s.id == id)
                .ok_or_else(|| "share not found".to_string())?;
            file.shares.remove(pos);
            self.save_unlocked(&file).map_err(|e| e.to_string())?;
        }

        self.remove_creds_if_any(id);
        let mpd_cfg = MpdConfig {
            host: cfg.mpd_host.clone(),
            port: cfg.mpd_port,
        };
        let _ = mpd::update_connected(&mpd_cfg, None).await;
        Ok(())
    }

    pub async fn info_share(&self, id: &str) -> Option<serde_json::Value> {
        let file = self.load().await.ok()?;
        let sh = file.shares.iter().find(|s| s.id == id)?;
        Some(serde_json::json!({
            "path": sh.path,
            "name": sh.name,
            "ip": sh.ip,
            "fstype": sh.fstype,
            "username": sh.user,
            "password": sh.password,
            "options": sh.options,
            "id": sh.id,
        }))
    }

    /// Boot: mount every persisted share (best-effort).
    ///
    /// Waits until NetworkManager reports **full**/**limited** connectivity or global **STATE=connected**
    /// before mounting so NFS/CIFS does not hit “Network is unreachable” while Ethernet/Wi‑Fi is still
    /// configuring. Transient failures get one delayed retry (~20s) for slow links or late DHCP.
    pub async fn mount_all_at_boot(&self, cfg: std::sync::Arc<Config>) {
        wait_for_network_before_nas_mounts().await;

        let Ok(file) = self.load().await else {
            return;
        };
        let shares = file.shares.clone();
        let mut transient_retry: Vec<ShareRecord> = Vec::new();

        for (i, sh) in shares.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
            match self.mount_share_record(&cfg, sh).await {
                Ok(MountOutcome::Success) => {
                    tracing::info!(
                        "{} boot: mounted NAS share {:?}",
                        crate::log_tags::EVO_UI,
                        sh.name
                    );
                }
                Ok(MountOutcome::PermissionDenied) => {
                    tracing::info!(
                        "{} boot: {:?} needs credentials (skipped)",
                        crate::log_tags::EVO_UI,
                        sh.name
                    );
                }
                Ok(MountOutcome::Fail(r)) => {
                    if mount_error_likely_transient_network(&r) {
                        tracing::info!(
                            "{} boot: mount {:?} deferred (transient: {}); will retry",
                            crate::log_tags::EVO_UI,
                            sh.name,
                            r
                        );
                        transient_retry.push(sh.clone());
                    } else {
                        tracing::warn!(
                            "{} boot: mount {:?} failed: {}",
                            crate::log_tags::EVO_UI,
                            sh.name,
                            r
                        );
                    }
                }
                Err(e) => tracing::warn!(
                    "{} boot: mount {:?}: {}",
                    crate::log_tags::EVO_UI,
                    sh.name,
                    e
                ),
            }
        }

        if transient_retry.is_empty() {
            return;
        }
        tracing::info!(
            "{} boot: retrying {} NAS mount(s) after 20s (transient network)",
            crate::log_tags::EVO_UI,
            transient_retry.len()
        );
        tokio::time::sleep(Duration::from_secs(20)).await;

        for sh in transient_retry {
            match self.mount_share_record(&cfg, &sh).await {
                Ok(MountOutcome::Success) => {
                    tracing::info!(
                        "{} boot: mounted NAS share {:?} (retry)",
                        crate::log_tags::EVO_UI,
                        sh.name
                    );
                }
                Ok(MountOutcome::PermissionDenied) => {
                    tracing::info!(
                        "{} boot: {:?} needs credentials (skipped, retry)",
                        crate::log_tags::EVO_UI,
                        sh.name
                    );
                }
                Ok(MountOutcome::Fail(r)) => {
                    tracing::warn!(
                        "{} boot: mount {:?} failed after retry: {}",
                        crate::log_tags::EVO_UI,
                        sh.name,
                        r
                    );
                }
                Err(e) => tracing::warn!(
                    "{} boot: mount {:?} (retry): {}",
                    crate::log_tags::EVO_UI,
                    sh.name,
                    e
                ),
            }
        }
    }

    /// Mount every persisted share that is currently **not** mounted (best-effort).
    ///
    /// Intended for periodic runs after network changes (new SSID/LAN/NAS reachable again). Skips shares
    /// whose mount points are already active; does **not** wait minutes for NM (only runs when a quick
    /// connectivity/state check says the stack is up).
    pub async fn remount_unmounted_shares_best_effort(&self, cfg: &Config, reason: &'static str) {
        let conn = nm_connectivity_state().await;
        let st = nm_general_state().await;
        if !nm_reports_ready_for_nas(conn.as_deref(), st.as_deref()) {
            tracing::trace!(
                "{} {}: skip NAS remount (CONNECTIVITY={:?} STATE={:?})",
                crate::log_tags::EVO_UI,
                reason,
                conn.as_deref(),
                st.as_deref()
            );
            return;
        }

        let Ok(file) = self.load().await else {
            return;
        };
        for sh in &file.shares {
            let mp = Self::mountpoint_for_name(&sh.name);
            if Self::is_mounted(&mp) {
                continue;
            }
            match self.mount_share_record(cfg, sh).await {
                Ok(MountOutcome::Success) => {
                    tracing::info!(
                        "{} {}: mounted NAS {:?}",
                        crate::log_tags::EVO_UI,
                        reason,
                        sh.name
                    );
                }
                Ok(MountOutcome::PermissionDenied) => {
                    tracing::debug!(
                        "{} {}: {:?} needs credentials (skipped)",
                        crate::log_tags::EVO_UI,
                        reason,
                        sh.name
                    );
                }
                Ok(MountOutcome::Fail(r)) => {
                    tracing::debug!(
                        "{} {}: {:?} not mountable yet: {}",
                        crate::log_tags::EVO_UI,
                        reason,
                        sh.name,
                        r
                    );
                }
                Err(e) => tracing::debug!(
                    "{} {}: {:?}: {}",
                    crate::log_tags::EVO_UI,
                    reason,
                    sh.name,
                    e
                ),
            }
        }
    }

    /// Edit share: unmount, update config, remount (Node-compatible).
    pub async fn edit_share(
        &self,
        cfg: &Config,
        id: &str,
        name: Option<String>,
        path: Option<String>,
        ip: Option<String>,
        fstype: Option<String>,
        username: Option<String>,
        password: Option<String>,
        options: Option<String>,
    ) -> Result<EditShareResult, String> {
        let old = {
            let _g = self.op.lock().await;
            let file = self.load_unlocked().map_err(|e| e.to_string())?;
            file.shares
                .iter()
                .find(|s| s.id == id)
                .cloned()
                .ok_or_else(|| "share not found".to_string())?
        };

        let mut new_rec = old.clone();
        if let Some(ref n) = name {
            new_rec.name = n.trim().to_string();
        }
        if let Some(ref p) = path {
            let fst = new_rec.fstype.to_ascii_lowercase();
            new_rec.path = Self::normalize_path_for_fstype(&fst, p);
        }
        if let Some(ref i) = ip {
            new_rec.ip = i.trim().to_string();
        }
        if let Some(ref f) = fstype {
            new_rec.fstype = f.to_ascii_lowercase();
        }
        if let Some(ref u) = username {
            new_rec.user = u.clone();
        }
        if let Some(ref p) = password {
            new_rec.password = p.clone();
        }
        if let Some(ref o) = options {
            new_rec.options = o.clone();
        }

        {
            let _g = self.op.lock().await;
            let file = self.load_unlocked().map_err(|e| e.to_string())?;
            if Self::find_duplicate(
                &file,
                &new_rec.name,
                &new_rec.ip,
                &new_rec.path,
                Some(id),
            )
            .is_some()
            {
                return Ok(EditShareResult::Duplicate);
            }
        }

        self.umount_by_name(&old.name).await?;

        {
            let _g = self.op.lock().await;
            let mut file = self.load_unlocked().map_err(|e| e.to_string())?;
            let idx = file
                .shares
                .iter()
                .position(|s| s.id == id)
                .ok_or_else(|| "share not found".to_string())?;
            file.shares[idx] = new_rec.clone();
            self.save_unlocked(&file).map_err(|e| e.to_string())?;
        }

        match self.mount_share_record(cfg, &new_rec).await {
            Ok(MountOutcome::Success) => Ok(EditShareResult::OkToast),
            Ok(MountOutcome::PermissionDenied) => Ok(EditShareResult::NasCredentials {
                id: new_rec.id,
                name: new_rec.name,
                username: new_rec.user,
                password: new_rec.password,
            }),
            Ok(MountOutcome::Fail(reason)) => Ok(EditShareResult::MountFail(reason)),
            Err(e) => Err(e),
        }
    }
}

pub enum EditShareResult {
    OkToast,
    Duplicate,
    NasCredentials {
        id: String,
        name: String,
        username: String,
        password: String,
    },
    MountFail(String),
}
