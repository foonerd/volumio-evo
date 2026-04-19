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
| `settings/system/` | Settings → System: hostname, timezone, country code (→ `iw reg`), UI language code, kiosk placeholders, privacy/update flags | `state.toml` |
| `settings/alarm/` | Alarm clock + sleep timer (daily playlist alarms, countdown sleep — `state.toml`) | `state.toml` |

**Sleep timer `time` field (`H:M`):** Evo interprets the stock UI string in two ways (preset rows only use **`hour &lt; 12`**):

| `hour` (first number) | Meaning |
|------------------------|---------|
| **0–11** | **Countdown** from Save: **`requested_at` + (`hour` hours + `minute` minutes)** — same idea as presets (**`0:15`** = 15 min, **`4:0`** = 4 h). |
| **12–23** | **Wall clock:** next occurrence of **`hour`:`minute`:00** in the **system local timezone** (today if still in the future, else tomorrow). **`17:0`** → stop at **17:00:00** local civil time (second 0; actual wake uses normal Tokio scheduling, not hard‑RT). |

Persisted optional **`sleep_deadline_rfc3339`** stores the absolute UTC instant for wall‑clock mode so reboot/reschedule matches the intended fire time. **`journalctl`** **`sleep timer armed (wall_clock_local_hm | duration_from_save)`** shows which branch ran.

### Daily alarms — **WYSIWYG** product contract

This is a **first-class behaviour guarantee**, not an implementation detail:

1. **Only the displayed clock face matters** — hour and minute chosen in the UI define the alarm; stray **seconds** in browser‑serialized ISO strings are **ignored**.
2. **On `saveAlarm`**, Evo rewrites each alarm’s persisted **`time`** to canonical **`HH:MM`** (zero‑padded) before writing **`state.toml`**, so disk, **`pushAlarm`**, and scheduling all agree.
3. **Scheduling** uses **`chrono::Local`** (system TZ/DST), normalizes to **`HH:MM:00.000`** local, converts the next occurrence to UTC, and **`sleep_until`** that instant. **`journalctl`** lines with **`EVO ALARM -->`** log **`target`** / **`actual`** / **`skew_ms`** for the scheduler instant; **`skew_ms`** is not “late due to ISO seconds” once canonicalization is in effect.

Hard‑real‑time guarantees still do not apply under load.

Full default paths (when no env override):

- ALSA: `/var/lib/volumio-evo/settings/alsa/state.toml`
- MPD: `/var/lib/volumio-evo/settings/mpd/playback.toml`
- Alarm / sleep: `/var/lib/volumio-evo/settings/alarm/state.toml`

Generated system config that Evo writes but does not treat as the source of truth (e.g. `/etc/volumio-evo/mpd.conf` fragment) is documented with the feature; it is not stored under `settings/`.

## Logging (not under `settings/`)

Log level and **`journalctl`** filtering are documented in **[OBSERVABILITY.md](OBSERVABILITY.md)** (`RUST_LOG`, **`[EVO]`** prefix, domain tags). They do not use paths under **`settings/`**.

## Environment overrides

| Variable | Effect |
|----------|--------|
| `VOLUMIO_EVO_SETTINGS_DIR` | Base directory for all default paths below. Default: `/var/lib/volumio-evo/settings`. |
| `VOLUMIO_EVO_ALSA_STATE` | **Full path** to the ALSA state file. Overrides `settings/alsa/state.toml`. |
| `VOLUMIO_EVO_PLAYBACK_STATE` | **Full path** to the MPD playback options file. Overrides `settings/mpd/playback.toml`. |
| `VOLUMIO_EVO_ALARM_STATE` | **Full path** to alarm/sleep persisted state. Overrides `settings/alarm/state.toml`. |
| `VOLUMIO_EVO_REPO_DIR` | Root of the **volumio-evo** tree (theme + `scripts/`). Default: `/usr/share/volumio-evo/repo`. Used for **Settings → System → Boot branding** and the install scripts. |
| `VOLUMIO_EVO_BOOT_BRANDING_SCRIPT` | Optional full path to **`run-boot-branding.sh`**. Default: `$VOLUMIO_EVO_REPO_DIR/scripts/run-boot-branding.sh`. |
| `VOLUMIO_EVO_BRANDING_READY_URL` | Optional HTTP URL polled by **`vol-branding-v1-app-listening.service`** until ready (drop-in **`Environment=`**). Legacy alias still honored in the unit: **`VOLUMIO_EVO_MILESTONE_URL`**. |

Systemd: `layer/systemd/volumio-evo.service` sets `VOLUMIO_EVO_SETTINGS_DIR` so all subsystems share one root.

## Upgrade note (intermediate Evo layout)

Older builds stored ALSA state at **`settings/alsa-state.toml`** (file at the root of `settings/`). On startup, if `settings/alsa/state.toml` is missing and `settings/alsa-state.toml` exists, Evo **moves** the file into `settings/alsa/state.toml`. No action required for most installs.

## Future subdirectories (planned)

Add sibling directories as features land, for example:

- RTC wake-from-suspend pairing with alarms uses **`rtcwake`** — see **[ALARM_WAKE.md](ALARM_WAKE.md)**

- `settings/network/` — nmcli-backed intent, UI preferences (not necessarily full NetworkManager dump)
- `settings/mounts/` — **implemented:** `shares.toml` lists shares; CIFS passwords use `cifs-<uuid>.cred` (0600) when needed
- `settings/devices/` — other device handling if not better scoped elsewhere

Keep **secrets** out of generic `*.toml` that might be world-readable; use root-only paths or `systemd-creds` and reference them from config.

## Later phases (not implemented yet)

### System-wide OS locale (language)

Persisted **language** today drives only the **web UI** (Angular `translate` / `pushUiSettings`), not system **`LANG`/`LC_*`**.

A **later phase** may apply the same choice OS-wide via **`locale-gen`**, **`/etc/default/locale`**, and related Debian/systemd mechanics so shells, journals, and services see a consistent locale. That needs explicit product and installer policy (UTF-8 defaults, impact on scripts that assume **`C.UTF-8`**, reboot/session rules).

### Regulatory domain via NetworkManager (optional)

**Country** currently maps to **`iw reg set`** plus an ISO 3166-1 alpha-2 code (cfg80211). An optional future enhancement could add **NetworkManager-aligned** hints on platforms where NM exposes them, without removing the **`iw`** baseline.

## Required directories on install

Bootstrap creates:

`mkdir -p /var/lib/volumio-evo/settings/alsa /var/lib/volumio-evo/settings/mpd /var/lib/volumio-evo/settings/mounts /var/lib/volumio-evo/settings/favourites /var/lib/volumio-evo/settings/playlist /var/lib/volumio-evo/settings/network /var/lib/volumio-evo/settings/system /var/lib/volumio-evo/settings/alarm`

so the daemon can write state before the first save.

## Intent vs system truth

For network and mounts, prefer documenting whether Evo files are **desired state** that syncs to the system, or a cache of **last applied** configuration—especially when debugging nmcli, `/etc/fstab`, or mount units.
