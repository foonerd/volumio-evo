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
| `/etc/volumio-evo/config.toml` | Evo main config | Stays **root**, **0644**; Evo reads only |
| **`${EVO_MPD_FRAGMENT}`** (default `/etc/volumio-evo/mpd.conf`) | MPD include snippet rewritten by Evo | **Service user** (so Evo can update mixer/output without root) |
| `/var/lib/volumio-evo/**` | Settings, album art, state | **Service user** |
| `/etc/sudoers.d/volumio-evo-mount` | NOPASSWD mount/umount | Root; content references **service user** |
| `/etc/sudoers.d/volumio-evo-mpd` | NOPASSWD `systemctl restart mpd` | Root; **`systemctl`** path must match **`VOLUMIO_EVO_SYSTEMCTL`** |
| `/etc/sudoers.d/volumio-evo-rfkill` | NOPASSWD **`rfkill unblock wifi`** | Root; **`rfkill`** path must match **`VOLUMIO_EVO_RFKILL`** |
| `/etc/sudoers.d/volumio-evo-nmcli` | NOPASSWD **`nmcli`** (full binary path) | Root; path must match **`VOLUMIO_EVO_NMCLI`** in **`10-runtime-user.conf`** |

## Runtime OS actions (Evo process)

| Action | Condition | How |
|--------|-----------|-----|
| Rewrite MPD fragment | Playback / ALSA / volume saves that affect MPD | **`std::fs::write`** to **`VOLUMIO_EVO_MPD_FRAGMENT`** (or default path) |
| Reload MPD | After a successful fragment write | **Root:** `systemctl restart mpd` (or **`$VOLUMIO_EVO_SYSTEMCTL`**). **Non-root:** **only** `sudo -n $VOLUMIO_EVO_SYSTEMCTL restart mpd` — never a bare `systemctl` first |
| NAS mounts (where implemented) | User adds/edits shares | **`sudo -n /usr/bin/mount`** / **`umount`** as allowed in **`volumio-evo-mount`** |
| **NetworkManager** (`nmcli`) | Wi‑Fi scan, connection add/up/down (network intent apply) | **Root**, or **`sudo -n $VOLUMIO_EVO_NMCLI …`** — bootstrap installs **`/etc/sudoers.d/volumio-evo-nmcli`** when **`EVO_INSTALL_NMCLI_SUDOERS=1`** (must match **`VOLUMIO_EVO_NMCLI`**) |
| **`rfkill`** (`unblock wifi`) | Clear **soft block** before `nmcli` Wi‑Fi scan when `/sys/class/rfkill/*/soft` blocks **wlan** | **Root**, or **`sudo -n $VOLUMIO_EVO_RFKILL unblock wifi`** — bootstrap installs **`/etc/sudoers.d/volumio-evo-rfkill`** when **`EVO_INSTALL_RFKILL_SUDOERS=1`** (must match **`VOLUMIO_EVO_RFKILL`** path) |

Nothing in Evo opens an interactive **`sudo`**, **`su`**, or **`pkexec`** session.

## Bootstrap toggles (reference)

| Variable | Default | Effect |
|--------|---------|--------|
| **`EVO_INSTALL_MOUNT_SUDOERS`** | `1` | Installs **`/etc/sudoers.d/volumio-evo-mount`** |
| **`EVO_INSTALL_MPD_SUDOERS`** | `1` | Installs **`/etc/sudoers.d/volumio-evo-mpd`** for **`systemctl restart mpd`** |
| **`EVO_INSTALL_NMCLI_SUDOERS`** | `1` | Installs **`/etc/sudoers.d/volumio-evo-nmcli`** for **`sudo -n nmcli`** (non-root network apply) |
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
