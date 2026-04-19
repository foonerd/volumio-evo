# Evo runtime user (no hardcoded `volumio` / uid 1000)

**Privilege and OS contract (sudo, `systemctl`, `/etc` files, journal behaviour):** see **[OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md)**. Evo is designed for **non-interactive** operation: no password prompts or ad-hoc polkit dialogs in the service path; bootstrap installs narrow **NOPASSWD** rules and ownership that match what the binary does.

Stock Volumio OS assumes login **`volumio`** with uid **1000**. **Volumio Evo does not** — the backend should run as **whichever account owns the install**, typically the same user you use over **SSH**.

## Systemd service user

The shipped unit has **no `User=`** in the repo. **Bootstrap** decides whether to add a drop-in:

- **`EVO_SERVICE_USER` unset** (recommended default): use the **current session login** — when you run bootstrap with **`sudo`**, that is typically **`SUDO_USER`** (the account that invoked `sudo`). Otherwise a non-root **`USER`**, else **`logname(1)`**. If that resolves to **root** or empty (e.g. real root shell, cron), the service stays **root** (no drop-in).
- **`EVO_SERVICE_USER` set to a name**: use that login for `User=` / `Group=`.
- **`EVO_SERVICE_USER` set but empty** (`EVO_SERVICE_USER=`): force **root** (no drop-in), same as the old “no runtime user” default.

| Variable | Meaning |
|----------|---------|
| **`EVO_SERVICE_USER`** | Omit: auto (session user). Non-empty: that login. Empty: root. |
| **`EVO_INSTALL_MOUNT_SUDOERS`** | If `1` (default), install **`/etc/sudoers.d/volumio-evo-mount`** with **NOPASSWD** for **`/usr/bin/mount`**, **`/usr/bin/umount`**, **`/bin/umount`** only — for future NAS/SMB helpers. Set `0` to skip. |
| **`EVO_INSTALL_MPD_SUDOERS`** | If `1` (default), install **`/etc/sudoers.d/volumio-evo-mpd`** with **NOPASSWD** for **`systemctl restart mpd`** (exact path from **`command -v systemctl`**) so Evo can reload MPD after editing the fragment. Set `0` to skip. |
| **`EVO_INSTALL_RFKILL_SUDOERS`** | If `1` (default), install **`/etc/sudoers.d/volumio-evo-rfkill`** with **NOPASSWD** for **`rfkill unblock wifi`** so Evo can clear Wi‑Fi soft block before **`nmcli`** when running non-root. Set `0` to skip. |
| **`EVO_INSTALL_NMCLI_SUDOERS`** | If `1` (default), install **`/etc/sudoers.d/volumio-evo-nmcli`** with **NOPASSWD** for **`nmcli`** (resolved path, matches **`VOLUMIO_EVO_NMCLI`** in the service drop-in) so Evo can add/up/modify NetworkManager connections when non-root. Set `0` to skip. |
| **`EVO_INSTALL_HOSTNAME_TIMEDATE_SUDOERS`** | If `1` (default), install **`/etc/sudoers.d/volumio-evo-hostname-timedate`** so the service user may run **`sudo -n hostnamectl set-hostname`** and **`sudo -n timedatectl set-timezone`** (exact paths match **`VOLUMIO_EVO_HOSTNAMECTL`** / **`VOLUMIO_EVO_TIMEDATECTL`**). Required for **Settings → System** when Evo is not root — otherwise **polkit** rejects bare **`hostnamectl`**. Set `0` to skip. |
| **`EVO_INSTALL_RTCWAKE_SUDOERS`** | If `1` (default), install **`/etc/sudoers.d/volumio-evo-rtcwake`** so the service user may run **`sudo -n rtcwake`** (path **`VOLUMIO_EVO_RTCWAKE`**) for RTC alarm / wake-from-suspend (**[ALARM_WAKE.md](ALARM_WAKE.md)**). Set `0` to skip. |
| **`EVO_INSTALL_NETWORK_STORAGE_PKGS`** | If `1` (default), **`apt install`** **`cifs-utils`**, **`nfs-common`**, **`smbclient`**, **`avahi-utils`** (CIFS/NFS mounts, **`smbclient`**, LAN **`avahi-browse`** for Network Drives discovery). Set `0` to skip. |

Examples:

```bash
# Default: same user as your sudo/SSH session
sudo ./scripts/bootstrap-volumio-evo-player.sh

# Explicit user
sudo EVO_SERVICE_USER=andrew ./scripts/bootstrap-volumio-evo-player.sh
```

Bootstrap will:

- Write **`/etc/systemd/system/volumio-evo.service.d/10-runtime-user.conf`** with `User=`, `Group=`, **`SupplementaryGroups=audio video render`** ( **`video`/`render`** needed for LAN **HLS** hardware encode **`h264_v4l2m2m`** on some boards), `HOME=`, **`VOLUMIO_EVO_RUNTIME_USER=<name>`**, **`VOLUMIO_EVO_SYSTEMCTL=<path>`**, **`VOLUMIO_EVO_HOSTNAMECTL`** / **`VOLUMIO_EVO_TIMEDATECTL`**, **`VOLUMIO_EVO_RTCWAKE`** (paths must match **`volumio-evo-hostname-timedate`** / **`volumio-evo-rtcwake`** sudoers when installed).
- Install **`/etc/sudoers.d/volumio-evo-mpd`** (unless **`EVO_INSTALL_MPD_SUDOERS=0`**) so the service user may run **`sudo -n <systemctl> restart mpd`** without a TTY after Evo rewrites the MPD fragment.
- **`chown -R`** **`/var/lib/volumio-evo`**, **`MUSIC_ROOT`**, **`/usr/share/volumio-evo/plugins`** to that user.
- Add the user to the **`audio`** group (`usermod -aG audio`).

To go back to **root**, clear the user and re-run bootstrap:

```bash
sudo EVO_SERVICE_USER= ./scripts/bootstrap-volumio-evo-player.sh --upgrade-evo
```

(or remove the drop-in and `systemctl daemon-reload` manually).

## Application code

Evo does **not** read uid **1000**. When a service user is configured, the process **effective uid** is that user. Code that needs root must use **`sudo -n`** with commands allowed in **OS_PRIVILEGE_MODEL.md** / bootstrap sudoers — never interactive **`sudo`**.

For the full matrix (fragment writes, MPD reload, mount), see **[OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md)**.

## MPD and permissions

**`music_directory`** must remain readable by the **MPD** user (often **`mpd`**) and by the **Evo** user. Bootstrap creates **`MUSIC_ROOT`** with world-readable permissions for the tree; if you tighten permissions, add **`mpd`** (and Evo) to a shared group or use ACLs.

After Evo writes the MPD fragment, it reloads MPD as described in **OS_PRIVILEGE_MODEL.md**: **root** uses direct **`systemctl`**; **non-root** uses **`sudo -n $VOLUMIO_EVO_SYSTEMCTL restart mpd` only** (no doomed bare `systemctl` first). **`EVO_INSTALL_MPD_SUDOERS`** installs the matching **NOPASSWD** rule for that exact path.

## Config files

`/etc/volumio-evo/config.toml` stays **root-owned** and **0644** unless you change it; Evo only needs read access.

The MPD include snippet (default **`/etc/volumio-evo/mpd.conf`**, see **`EVO_MPD_FRAGMENT`** in the bootstrap script) is **owned by the Evo service user** when a non-root runtime user is configured, so Evo can rewrite it when Playback Options change the mixer or output.
