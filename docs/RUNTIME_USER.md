# Evo runtime user (no hardcoded `volumio` / uid 1000)

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
| **`EVO_INSTALL_NETWORK_STORAGE_PKGS`** | If `1` (default), **`apt install`** **`cifs-utils`**, **`nfs-common`**, **`smbclient`**, **`avahi-utils`** (CIFS/NFS mounts, **`smbclient`**, LAN **`avahi-browse`** for Network Drives discovery). Set `0` to skip. |

Examples:

```bash
# Default: same user as your sudo/SSH session
sudo ./scripts/bootstrap-volumio-evo-player.sh

# Explicit user
sudo EVO_SERVICE_USER=andrew ./scripts/bootstrap-volumio-evo-player.sh
```

Bootstrap will:

- Write **`/etc/systemd/system/volumio-evo.service.d/10-runtime-user.conf`** with `User=`, `Group=`, `SupplementaryGroups=audio`, `HOME=`, and **`VOLUMIO_EVO_RUNTIME_USER=<name>`** for logs/diagnostics.
- **`chown -R`** **`/var/lib/volumio-evo`**, **`MUSIC_ROOT`**, **`/usr/share/volumio-evo/plugins`** to that user.
- Add the user to the **`audio`** group (`usermod -aG audio`).

To go back to **root**, clear the user and re-run bootstrap:

```bash
sudo EVO_SERVICE_USER= ./scripts/bootstrap-volumio-evo-player.sh --upgrade-evo
```

(or remove the drop-in and `systemctl daemon-reload` manually).

## Application code

Evo does **not** read uid **1000**. When a service user is configured, the process **effective uid** is that user; future mount/NAS code should use **`geteuid()`** / the runtime environment — or invoke **`sudo /usr/bin/mount …`** only for operations that still require root, relying on the narrow **sudoers** rules above.

## MPD and permissions

**`music_directory`** must remain readable by the **MPD** user (often **`mpd`**) and by the **Evo** user. Bootstrap creates **`MUSIC_ROOT`** with world-readable permissions for the tree; if you tighten permissions, add **`mpd`** (and Evo) to a shared group or use ACLs.

## Config files

`/etc/volumio-evo/config.toml` stays **root-owned** and **0644** unless you change it; Evo only needs read access.
