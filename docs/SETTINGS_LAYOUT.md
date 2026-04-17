# Evo persisted settings layout

All mutable **Evo-controlled** state that must survive reboots lives under a single root so it can be backed up, permissioned, and extended by subsystem without a flat sprawl of unrelated files.

## Root directory

| Path | Purpose |
|------|---------|
| `/var/lib/volumio-evo/music/` | Music library root (see `[music_sources] music_root` in `config.toml`) |
| `/var/lib/volumio-evo/albumart/` | Album art cache and uploads |
| `/var/lib/volumio-evo/settings/` | **Persisted Evo settings** (this document) |

Static read-only data shipped with the image stays under `/usr/share/volumio-evo/` (e.g. `alsa/dacs.json`, `alsa/cards.json`).

## Namespace rule

**One concern → one subdirectory under `settings/`**, each with its own primary file(s). New features (network, NFS, SMB, devices, …) add **new directories**, not new loose files at the root of `settings/`.

## Current layout

| Directory | Contents | Primary file |
|-----------|----------|--------------|
| `settings/alsa/` | ALSA output device selection, I2S enablement, DAC id | `state.toml` |
| `settings/mpd/` | MPD / Playback Options (buffer, DSD, mixer type, resampling, …) | `playback.toml` |
| `settings/mounts/` | NAS/SMB/NFS share definitions + CIFS credential sidecars | `shares.toml` (mode `0600`), optional `cifs-<uuid>.cred` |
| `settings/favourites/` | Library favourites + radio favourites (JSON, Node-compatible) | `favourites`, `radio-favourites` |
| `settings/playlist/` | User playlists as JSON files (filename = playlist name, no extension) | One file per playlist |
| `settings/network/` | NetworkManager intent: DHCP/static, Wi‑Fi STA/AP, hotspot fallback (see **[NETWORK_NM.md](NETWORK_NM.md)**) | `intent.toml`: **`ethernet.enabled`** (default **true**; set **false** for Wi‑Fi‑only), **`fallback.hotspot_ifname`** when STA iface ≠ AP iface, optional `wifi-sta.psk` / `wifi-ap.psk` (0600), **`wifi_iface_preferred`** (one line: UI-chosen STA `wlan*`), staging **`config.toml.pending`** (full merged TOML before `install` to `/etc`) |

Full default paths (when no env override):

- ALSA: `/var/lib/volumio-evo/settings/alsa/state.toml`
- MPD: `/var/lib/volumio-evo/settings/mpd/playback.toml`

Generated system config that Evo writes but does not treat as the source of truth (e.g. `/etc/volumio-evo/mpd.conf` fragment) is documented with the feature; it is not stored under `settings/`.

## Logging (not under `settings/`)

Log level and **`journalctl`** filtering are documented in **[OBSERVABILITY.md](OBSERVABILITY.md)** (`RUST_LOG`, **`[EVO]`** prefix, domain tags). They do not use paths under **`settings/`**.

## Environment overrides

| Variable | Effect |
|----------|--------|
| `VOLUMIO_EVO_SETTINGS_DIR` | Base directory for all default paths below. Default: `/var/lib/volumio-evo/settings`. |
| `VOLUMIO_EVO_ALSA_STATE` | **Full path** to the ALSA state file. Overrides `settings/alsa/state.toml`. |
| `VOLUMIO_EVO_PLAYBACK_STATE` | **Full path** to the MPD playback options file. Overrides `settings/mpd/playback.toml`. |

Systemd: `layer/systemd/volumio-evo.service` sets `VOLUMIO_EVO_SETTINGS_DIR` so all subsystems share one root.

## Upgrade note (intermediate Evo layout)

Older builds stored ALSA state at **`settings/alsa-state.toml`** (file at the root of `settings/`). On startup, if `settings/alsa/state.toml` is missing and `settings/alsa-state.toml` exists, Evo **moves** the file into `settings/alsa/state.toml`. No action required for most installs.

## Future subdirectories (planned)

Add sibling directories as features land, for example:

- `settings/network/` — nmcli-backed intent, UI preferences (not necessarily full NetworkManager dump)
- `settings/mounts/` — **implemented:** `shares.toml` lists shares; CIFS passwords use `cifs-<uuid>.cred` (0600) when needed
- `settings/devices/` — other device handling if not better scoped elsewhere

Keep **secrets** out of generic `*.toml` that might be world-readable; use root-only paths or `systemd-creds` and reference them from config.

## Required directories on install

Bootstrap creates:

`mkdir -p /var/lib/volumio-evo/settings/alsa /var/lib/volumio-evo/settings/mpd /var/lib/volumio-evo/settings/mounts /var/lib/volumio-evo/settings/favourites /var/lib/volumio-evo/settings/playlist /var/lib/volumio-evo/settings/network`

so the daemon can write state before the first save.

## Intent vs system truth

For network and mounts, prefer documenting whether Evo files are **desired state** that syncs to the system, or a cache of **last applied** configuration—especially when debugging nmcli, `/etc/fstab`, or mount units.
