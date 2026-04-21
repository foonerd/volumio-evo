# Documentation map

Single index for **volumio-evo**. Other docs own detail; **do not** copy long inventories here - link them.

## Authority (which doc wins)

| Topic | Canonical doc |
|-------|----------------|
| volumio3-backend parity: REST, Socket.IO, stubs | [PORTING.md](PORTING.md) |
| Bootstrap / on-device validation, git depth & updates | [TESTER_GUIDE.md](TESTER_GUIDE.md) |
| `sudo -n`, sudoers, service user | [OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md) |
| Plymouth, VOL tokens, **`vol-branding-v1-*`**, UI installer | [BRANDED_BOOT.md](BRANDED_BOOT.md); **`apt`** line = **`layer/install/volumio-boot-branding.sh`** |
| NetworkManager / `nmcli` | [NETWORK_NM.md](NETWORK_NM.md) |
| Paths under `/var/lib/volumio-evo/` | [SETTINGS_LAYOUT.md](SETTINGS_LAYOUT.md) |
| Logging / `journalctl` | [OBSERVABILITY.md](OBSERVABILITY.md) |
| WASM plugins | [PLUGIN_ABI.md](PLUGIN_ABI.md) |
| Stock UI optional forks | [UI_GAP.md](UI_GAP.md) |
| Cross-build, `layer/binaries/` | [BUILD_GUIDE.md](BUILD_GUIDE.md), [layer/binaries/README.md](../layer/binaries/README.md) |
| Evo architecture one-pager | [CONCEPT.md](CONCEPT.md) |
| Alarm / RTC wake | [ALARM_WAKE.md](ALARM_WAKE.md) |
| Album art provider order / URLs | [ALBUMART_PROVIDERS.md](ALBUMART_PROVIDERS.md) |
| Playback timer / queue UI contract | [PLAYBACK_STATE_REQUIREMENTS.md](PLAYBACK_STATE_REQUIREMENTS.md) |
| External `.cue` files (normalize, browse, MPD `load`) | [CUE_SHEETS.md](CUE_SHEETS.md) |
| Runtime user / mount helpers | [RUNTIME_USER.md](RUNTIME_USER.md) |
| WPE kiosk concept and wiring | [KIOSK.md](KIOSK.md); implementation in `layer/kiosk-wpe/` + `crates/core/src/kiosk.rs` |

## Every markdown file under `docs/`

All paths relative to **`docs/`**. Owning doc for parity is usually **PORTING.md** unless another row in **Authority** applies.

| File | Role |
|------|------|
| [DOCUMENTATION_MAP.md](DOCUMENTATION_MAP.md) | **This index** - assumptions, authority, completed vs not ported, deferred. |
| [PORTING.md](PORTING.md) | volumio3-backend <-> Evo parity inventory and phased status. |
| [TESTER_GUIDE.md](TESTER_GUIDE.md) | Canonical on-device bootstrap and validation (incl. shallow **`EVO_REPO_DEPTH`** / lightweight git updates). |
| [BUILD_GUIDE.md](BUILD_GUIDE.md) | Compile and cross-compile **`volumio-evo`**. |
| [CONCEPT.md](CONCEPT.md) | Architecture one-pager. |
| [NETWORK_NM.md](NETWORK_NM.md) | NetworkManager contract + **implementation status** table. |
| [SETTINGS_LAYOUT.md](SETTINGS_LAYOUT.md) | **`/var/lib/volumio-evo/settings/`** layout. |
| [OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md) | **`sudo -n`**, sudoers, service user. |
| [RUNTIME_USER.md](RUNTIME_USER.md) | Effective user for mounts and runtime. |
| [OBSERVABILITY.md](OBSERVABILITY.md) | Logging and **`journalctl`**. |
| [PLUGIN_ABI.md](PLUGIN_ABI.md) | WASM exports; **`plugin_handle_request`** remains **TBD** until ABI freeze ([PRIORITY_ALSA_AAMPP.md](PRIORITY_ALSA_AAMPP.md)). |
| [PRIORITY_ALSA_AAMPP.md](PRIORITY_ALSA_AAMPP.md) | ALSA merge pipeline - **deferred** implementation. |
| [BRANDED_BOOT.md](BRANDED_BOOT.md) | Plymouth, VOL tokens, branding units, **`vol-branding-v1-*`**. |
| [UI_GAP.md](UI_GAP.md) | Stock UI changes when paired with Evo (fork/upstream checklist). |
| [ALBUMART_PROVIDERS.md](ALBUMART_PROVIDERS.md) | Online album-art provider behaviour. |
| [ALARM_WAKE.md](ALARM_WAKE.md) | **`rtcwake`** / alarm persistence. |
| [PLAYBACK_STATE_REQUIREMENTS.md](PLAYBACK_STATE_REQUIREMENTS.md) | Timer and **`pushState`** expectations for the UI. |
| [CUE_SHEETS.md](CUE_SHEETS.md) | `.cue` normalization, browse expansion, **`load`** vs **`add`**; deferred sidecar/multi-file work. |
| [KIOSK.md](KIOSK.md) | WPE Wayland kiosk concept. Implementation lives in `layer/kiosk-wpe/` and `crates/core/src/kiosk.rs`; enabled via `--with-kiosk=wpe`. |

## Documentation update rule (non-negotiable)

1. **Behaviour** is described only in the **authority** doc for that topic (table above).
2. **No placeholder sections:** "future / optional / TBD" in prose must either name the **deferred doc** (below), **PORTING** phase **Outstanding**, or **NETWORK_NM** implementation gaps - or be removed.
3. After changing code, update the owning doc in the **same change set** when behaviour is user-visible or parity-relevant.

## Assumptions

- **OS:** Stock minimal Debian-class or Raspberry Pi OS; Evo is a **layer** on top ([CONCEPT.md](CONCEPT.md)).
- **Ports:** Evo listens on **3000**; UI usually via nginx on **80** ([TESTER_GUIDE.md](TESTER_GUIDE.md)).
- **Socket.IO wire:** Engine.IO **v3** for stock Volumio2-UI (`socketioxide` **`v4`** feature) ([PORTING.md](PORTING.md)).
- **Integration test:** **`scripts/bootstrap-volumio-evo-player.sh`** is the canonical path ([TESTER_GUIDE.md](TESTER_GUIDE.md)).
- **Binary:** Prefer checked-in **`layer/binaries/<triple>/volumio-evo`**; else **`--build`** ([layer/binaries/README.md](../layer/binaries/README.md)).

## Completed in this repo (high level)

| Area | Pointer |
|------|---------|
| Playback, browse, queue, playlists (MPD), album art | [PORTING.md](PORTING.md) Part 2-3 |
| **`GET /api/host`** | Implemented ([PORTING.md](PORTING.md)); nginx proxies from UI host |
| Settings Sources: NAS mounts, share discovery | [PORTING.md](PORTING.md) 3.2 |
| Wi-Fi list + NM apply (`nmcli`) | [NETWORK_NM.md](NETWORK_NM.md), [PORTING.md](PORTING.md) Phase 3 |
| **`callMethod`**: ALSA/MPD saves (**`saveAlsaOptions`** may **`openModal`** reboot after I2S **`dtoverlay`**), **system_controller/system** saves, **`installBootBranding`** | `socketio.rs`; parity [PORTING.md](PORTING.md) 3.1 Playback/ALSA; boot stack [BRANDED_BOOT.md](BRANDED_BOOT.md), [OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md) |
| Plymouth theme **`layer/plymouth/`**, **`vol-branding-v1-*`** units | [BRANDED_BOOT.md](BRANDED_BOOT.md) |
| WPE kiosk (layer component + Rust wiring) | `layer/kiosk-wpe/`, `crates/core/src/kiosk.rs`, [KIOSK.md](KIOSK.md). `GET /api/v1/kiosk/status`; Settings -> System -> WPE Kiosk drives the unit via sudoers drop-in. |
| WASM plugin host | arm64/x86_64 ([PLUGIN_ABI.md](PLUGIN_ABI.md)); armhf core only |

## Not ported / outside this repo

| Item | Notes |
|------|--------|
| Node plugins, plugin store, install zip flow | [PORTING.md](PORTING.md) Part 5-6 |
| My Volumio cloud, stock updater, OAuth/push URLs as in Node | [PORTING.md](PORTING.md) Part 5-6 |
| **`VOL:v1:initrd:*`** from initramfs | volumio-os / image recipes - [BRANDED_BOOT.md](BRANDED_BOOT.md) "Not implemented here" |

## Deferred / reference (not shipped as product requirement)

| Item | Doc |
|------|-----|
| ALSA AAMPP priority pipeline | [PRIORITY_ALSA_AAMPP.md](PRIORITY_ALSA_AAMPP.md) |
| NM runtime STA-loss watchdog (phase 3) | [NETWORK_NM.md](NETWORK_NM.md) Phased implementation |
| WASM `plugin_handle_request` + full generic RPC | [PLUGIN_ABI.md](PLUGIN_ABI.md), [PORTING.md](PORTING.md) Part 5 |
| OS-wide locale (`locale-gen`, `/etc/default/locale`) | [SETTINGS_LAYOUT.md](SETTINGS_LAYOUT.md) Later phases |
| NM-aligned regulatory hints (beyond `iw reg set`) | [SETTINGS_LAYOUT.md](SETTINGS_LAYOUT.md) Regulatory domain |
| Evo **`Type=notify`** / `sd_notify` for branding "app listening" | [BRANDED_BOOT.md](BRANDED_BOOT.md) |

## Maintenance

Extends **[Documentation update rule](#documentation-update-rule-non-negotiable)**. When **Authority**, **Completed in this repo**, **Not ported**, or **Deferred / reference** boundaries change, update those tables **in the same change set** as **[PORTING.md](PORTING.md)**, **[BRANDED_BOOT.md](BRANDED_BOOT.md)**, **[NETWORK_NM.md](NETWORK_NM.md)**, or the other owning doc.
