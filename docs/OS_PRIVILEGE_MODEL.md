# OS privilege model and bootstrap contract

This document is the **single source of truth** for how Volumio Evo interacts with the host OS (users, systemd, sudo, files under `/etc`). It exists so behaviour is **predictable**, **reviewable**, and **not** a series of ad-hoc user interactions with the system.

## Non‑negotiables

1. **No interactive authentication in the service path**  
   The `volumio-evo` daemon must never block on a password, TTY, or polkit dialog. Operations that need root use **`sudo -n`** (non-interactive) together with **narrow NOPASSWD** rules installed by **bootstrap**, or the process runs as **root** (no drop-in).

2. **No “try systemctl and see” for non-root**  
   Calling `systemctl` as an unprivileged user **fails** with *Interactive authentication required* and **pollutes the journal** even when a fallback succeeds. Evo therefore **does not** invoke plain `systemctl restart mpd` when the effective UID is not 0; it uses **`sudo -n`** with the path from **`VOLUMIO_EVO_SYSTEMCTL`**, which must match **sudoers**.

3. **Bootstrap owns the privilege contract**  
   Sudoers fragments, systemd drop-ins, ownership of the MPD fragment, and **`Environment=`** lines that Evo relies on are **written by** `scripts/bootstrap-volumio-evo-player.sh` (or documented manual equivalents). Application code does not silently broaden privileges.

4. **Least privilege**  
   Sudoers entries allow **only** the listed command paths (and, for `systemctl`, the exact **`restart mpd`** invocation). They are not generic `NOPASSWD: ALL`.

## What the service user is

See **[RUNTIME_USER.md](RUNTIME_USER.md)** for **`EVO_SERVICE_USER`**, session detection, and how **`10-runtime-user.conf`** is generated.

When a non-root user runs Evo:

- **`VOLUMIO_EVO_RUNTIME_USER`** is set in the drop-in (diagnostics / fallback heuristics).
- **`VOLUMIO_EVO_SYSTEMCTL`** is set to the resolved **`systemctl`** binary path (from **`command -v systemctl`**, defaulting to **`/usr/bin/systemctl`**). This path **must** be the same string used in **`/etc/sudoers.d/volumio-evo-mpd`**.

## Files and ownership (typical defaults)

| Path | Purpose | Ownership after bootstrap (non-root service) |
|------|---------|-----------------------------------------------|
| `/etc/volumio-evo/config.toml` | Evo main config | **Root**, **0644**; Evo **reads** always. **Network** merges **`wifi_iface`** from **`…/settings/network/config.toml.pending`**. **Appearance** merges **`[ui] active_layout`** from **`…/settings/ui/config.toml.pending`**. Both use **`sudo -n install`** (**`volumio-evo-config-install`** sudoers allow **both** fixed source paths → **`/etc/volumio-evo/config.toml`**). **`settings/ui/active_layout`** mirrors the layout line when **`/etc`** cannot be updated |
| **`${EVO_MPD_FRAGMENT}`** (default `/etc/volumio-evo/mpd.conf`) | MPD include snippet rewritten by Evo | **Service user** (so Evo can update mixer/output without root) |
| `/var/lib/volumio-evo/**` | Settings, album art, state | **Service user** |
| `/etc/sudoers.d/volumio-evo-mount` | NOPASSWD mount/umount | Root; content references **service user** |
| `/etc/sudoers.d/volumio-evo-mpd` | NOPASSWD `systemctl restart mpd` | Root; **`systemctl`** path must match **`VOLUMIO_EVO_SYSTEMCTL`** |
| **`/etc/sudoers.d/volumio-evo-power`** (bootstrap; disable with **`EVO_INSTALL_POWER_SUDOERS=0`**) | NOPASSWD **`systemctl`** **`reboot`** / **`poweroff`**, **`reboot`** binary, **`shutdown -h now`** — paths must match **`Environment=`** **`VOLUMIO_EVO_SYSTEMCTL`**, **`VOLUMIO_EVO_REBOOT_BIN`**, **`VOLUMIO_EVO_SHUTDOWN_BIN`** | Service user; Evo uses **`sudo -n`** only (**[`system_power.rs`](../crates/core/src/api/system_power.rs)**) |
| `/etc/sudoers.d/volumio-evo-rfkill` | NOPASSWD **`rfkill unblock wifi`** | Root; **`rfkill`** path must match **`VOLUMIO_EVO_RFKILL`** |
| `/etc/sudoers.d/volumio-evo-nmcli` | NOPASSWD **`nmcli`** (full binary path) | Root; path must match **`VOLUMIO_EVO_NMCLI`** in **`10-runtime-user.conf`** |
| `/etc/sudoers.d/volumio-evo-hostname-timedate` | NOPASSWD **`hostnamectl set-hostname *`** and **`timedatectl set-timezone *`** | Root; paths must match **`VOLUMIO_EVO_HOSTNAMECTL`** / **`VOLUMIO_EVO_TIMEDATECTL`** in **`10-runtime-user.conf`** |
| `/etc/sudoers.d/volumio-evo-rtcwake` | NOPASSWD **`rtcwake`** (full binary path) | Root; path must match **`VOLUMIO_EVO_RTCWAKE`** — alarm RTC wake / suspend tests (**[ALARM_WAKE.md](ALARM_WAKE.md)**) |
| `/etc/sudoers.d/volumio-evo-boot-branding` (bootstrap; disable with **`EVO_INSTALL_BOOT_BRANDING_SUDOERS=0`**) | NOPASSWD **`/usr/share/volumio-evo/repo/layer/install/run-boot-branding.sh`** (or the path from **`VOLUMIO_EVO_BOOT_BRANDING_SCRIPT`**) | Service user; allow only this wrapper (see **[BRANDED_BOOT.md](BRANDED_BOOT.md)**) |
| `/etc/sudoers.d/volumio-evo-samba` (disable with **`EVO_INSTALL_SAMBA_SUDOERS=0`**) | NOPASSWD **`install`** **`…/settings/samba/smb.conf.generated`** → **`/etc/samba/smb.conf`**, **`systemctl`** **`stop`**/**`restart`** **`smbd`**/**`nmbd`**, **`/usr/local/bin/volumio-evo-smb-user-sync.sh`** | Root; paths must match [**`paths.rs`**](../crates/core/src/paths.rs) default generated file and **`VOLUMIO_EVO_SYSTEMCTL`** (**[SAMBA.md](SAMBA.md)**) |

## Runtime OS actions (Evo process)

| Action | Condition | How |
|--------|-----------|-----|
| Rewrite MPD fragment | Playback / ALSA / volume saves that affect MPD | **`std::fs::write`** to **`VOLUMIO_EVO_MPD_FRAGMENT`** (or default path) |
| Reload MPD | After a successful fragment write | **Root:** `systemctl restart mpd` (or **`$VOLUMIO_EVO_SYSTEMCTL`**). **Non-root:** **only** `sudo -n $VOLUMIO_EVO_SYSTEMCTL restart mpd` — never a bare `systemctl` first |
| **`systemctl`** **`reboot`** / **`poweroff`** | Settings → Shutdown, Socket.IO **`reboot`** / **`shutdown`** (boot branding modal **Restart**) | **Root:** direct **`systemctl`**. **Non-root:** **`sudo -n $VOLUMIO_EVO_SYSTEMCTL`** **`reboot`** \| **`poweroff`** only — bootstrap **`volumio-evo-power`**; fallback after 3s uses **`sudo -n $VOLUMIO_EVO_REBOOT_BIN`** or **`sudo -n $VOLUMIO_EVO_SHUTDOWN_BIN -h now`** |
| NAS mounts (where implemented) | User adds/edits shares | **`sudo -n /usr/bin/mount`** / **`umount`** as allowed in **`volumio-evo-mount`** |
| **NetworkManager** (`nmcli`) | Wi‑Fi scan, connection add/up/down (network intent apply) | **Root**, or **`sudo -n $VOLUMIO_EVO_NMCLI …`** — bootstrap installs **`/etc/sudoers.d/volumio-evo-nmcli`** when **`EVO_INSTALL_NMCLI_SUDOERS=1`** (must match **`VOLUMIO_EVO_NMCLI`**) |
| **`rfkill`** (`unblock wifi`) | Clear **soft block** before `nmcli` Wi‑Fi scan when `/sys/class/rfkill/*/soft` blocks **wlan** | **Root**, or **`sudo -n $VOLUMIO_EVO_RFKILL unblock wifi`** — bootstrap installs **`/etc/sudoers.d/volumio-evo-rfkill`** when **`EVO_INSTALL_RFKILL_SUDOERS=1`** (must match **`VOLUMIO_EVO_RFKILL`** path) |
| **`install`** (fixed paths) | Merge **`wifi_iface`** from **`…/network/config.toml.pending`** or **`[ui] active_layout`** from **`…/ui/config.toml.pending`** into **`/etc/volumio-evo/config.toml`** | **Root**, or **`sudo -n /usr/bin/install …`** — bootstrap **`volumio-evo-config-install`** (**two** NOPASSWD lines; paths must match Rust [`paths.rs`](../crates/core/src/paths.rs)) |
| **`hostnamectl`** (`set-hostname`) | Persisted device name → OS hostname (**Settings → System**) | **Root**, or **`sudo -n $VOLUMIO_EVO_HOSTNAMECTL set-hostname …`** — bootstrap installs **`volumio-evo-hostname-timedate`** when **`EVO_INSTALL_HOSTNAME_TIMEDATE_SUDOERS=1`** (bare **`hostnamectl`** triggers polkit “Interactive authentication required” when non‑root) |
| **`timedatectl`** (`set-timezone`) | Persisted timezone | **Root**, or **`sudo -n $VOLUMIO_EVO_TIMEDATECTL set-timezone …`** — same sudoers fragment as **`hostnamectl`** |
| **`rtcwake`** | Program/clear RTC alarm for wake-from-suspend (**alarm clock** groundwork) | **Root**, or **`sudo -n $VOLUMIO_EVO_RTCWAKE …`** — bootstrap **`volumio-evo-rtcwake`** when **`EVO_INSTALL_RTCWAKE_SUDOERS=1`** (**[ALARM_WAKE.md](ALARM_WAKE.md)**) |
| **Boot branding** installer | **Settings → System → Boot branding** | **Root**, or **`sudo -n /path/to/run-boot-branding.sh <rotation>`** — narrow sudoers line for the wrapper script only (**[BRANDED_BOOT.md](BRANDED_BOOT.md)**) |
| **SMB server** (`smb.conf`, **`smbd`**/**`nmbd`**, optional Unix/Samba users) | Settings → Network → SMB | **Root:** write generated file under **`…/settings/samba/`**, **`install`** → **`/etc/samba/smb.conf`**, **`systemctl`** **`smbd`**/**`nmbd`**. **Non-root:** same via **`sudo -n`** — bootstrap **`volumio-evo-samba`** when **`EVO_INSTALL_SAMBA_SUDOERS=1`**; named users via **`sudo -n /usr/local/bin/volumio-evo-smb-user-sync.sh`** only (**[SAMBA.md](SAMBA.md)**) |

Nothing in Evo opens an interactive **`sudo`**, **`su`**, or **`pkexec`** session.

## Bootstrap toggles (reference)

| Variable | Default | Effect |
|--------|---------|--------|
| **`EVO_INSTALL_MOUNT_SUDOERS`** | `1` | Installs **`/etc/sudoers.d/volumio-evo-mount`** |
| **`EVO_INSTALL_MPD_SUDOERS`** | `1` | Installs **`/etc/sudoers.d/volumio-evo-mpd`** for **`systemctl restart mpd`** |
| **`EVO_INSTALL_POWER_SUDOERS`** | `1` | Installs **`/etc/sudoers.d/volumio-evo-power`** for **`sudo -n`** graceful reboot/shutdown (**Socket.IO**, branding modal) |
| **`EVO_INSTALL_NMCLI_SUDOERS`** | `1` | Installs **`/etc/sudoers.d/volumio-evo-nmcli`** for **`sudo -n nmcli`** (non-root network apply) |
| **`EVO_INSTALL_CONFIG_INSTALL_SUDOERS`** | `1` | Installs **`/etc/sudoers.d/volumio-evo-config-install`** for **`sudo -n install`** (**`network/config.toml.pending`** **and** **`ui/config.toml.pending`** → **`/etc/volumio-evo/config.toml`**) |
| **`EVO_INSTALL_HOSTNAME_TIMEDATE_SUDOERS`** | `1` | Installs **`/etc/sudoers.d/volumio-evo-hostname-timedate`** for **`sudo -n hostnamectl`** / **`timedatectl`** (device name + timezone when non‑root) |
| **`EVO_INSTALL_RTCWAKE_SUDOERS`** | `1` | Installs **`/etc/sudoers.d/volumio-evo-rtcwake`** for **`sudo -n rtcwake`** (RTC alarm / wake-from-suspend — **[ALARM_WAKE.md](ALARM_WAKE.md)**) |
| **`EVO_INSTALL_SAMBA_SUDOERS`** | `1` | Installs **`/etc/sudoers.d/volumio-evo-samba`** for SMB server apply (**`install`** **`smb.conf.generated`**, **`systemctl`** **`smbd`**/**`nmbd`**, **`volumio-evo-smb-user-sync.sh`** — **[SAMBA.md](SAMBA.md)**) |
| **`EVO_SERVICE_USER`** | unset → auto | See **RUNTIME_USER.md** |

Setting **`EVO_INSTALL_MPD_SUDOERS=0`** while running Evo **as non-root** means fragment writes may succeed but **MPD reload will fail** unless you run Evo as **root** or install an equivalent **NOPASSWD** rule yourself.

## Operator expectations

1. After changing service user or **`systemctl`** location, **re-run bootstrap** (e.g. **`--upgrade-evo`**) so drop-in, sudoers, and **`chown`** stay aligned.
2. **Journal** should not show spurious *Failed to restart mpd … Interactive authentication required* from Evo when the privilege model is correctly installed; if it does, fix the drop-in / sudoers path mismatch or update Evo.
3. **`MPDCONF` unset** warnings from **`mpd.service`** come from the **OS/mpd package**, not from Evo; address via distro unit overrides if desired.

## Related documents

- **[RUNTIME_USER.md](RUNTIME_USER.md)** — bootstrap variables and service user selection  
- **[SETTINGS_LAYOUT.md](SETTINGS_LAYOUT.md)** — persisted paths under `/var/lib/volumio-evo/settings/`  
- **[OBSERVABILITY.md](OBSERVABILITY.md)** — logging and `journalctl`
