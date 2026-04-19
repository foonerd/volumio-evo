# Documentation map

Single index for **volumio-evo**. Other docs own detail; **do not** copy long inventories here—link them.

## Authority (which doc wins)

| Topic | Canonical doc |
|-------|----------------|
| volumio3-backend parity: REST, Socket.IO, stubs | [PORTING.md](PORTING.md) |
| Bootstrap / on-device validation | [TESTER_GUIDE.md](TESTER_GUIDE.md) |
| `sudo -n`, sudoers, service user | [OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md) |
| Plymouth, VOL tokens, **`vol-branding-v1-*`**, UI installer | [BRANDED_BOOT.md](BRANDED_BOOT.md); **`apt`** line = **`scripts/volumio-boot-branding.sh`** |
| NetworkManager / `nmcli` | [NETWORK_NM.md](NETWORK_NM.md) |
| Paths under `/var/lib/volumio-evo/` | [SETTINGS_LAYOUT.md](SETTINGS_LAYOUT.md) |
| Logging / `journalctl` | [OBSERVABILITY.md](OBSERVABILITY.md) |
| WASM plugins | [PLUGIN_ABI.md](PLUGIN_ABI.md) |
| Stock UI optional forks | [UI_GAP.md](UI_GAP.md) |
| Cross-build, `layer/binaries/` | [BUILD_GUIDE.md](BUILD_GUIDE.md), [layer/binaries/README.md](../layer/binaries/README.md) |
| Evo architecture one-pager | [CONCEPT.md](CONCEPT.md) |

## Assumptions

- **OS:** Stock minimal Debian-class or Raspberry Pi OS; Evo is a **layer** on top ([CONCEPT.md](CONCEPT.md)).
- **Ports:** Evo listens on **3000**; UI usually via nginx on **80** ([TESTER_GUIDE.md](TESTER_GUIDE.md)).
- **Socket.IO wire:** Engine.IO **v3** for stock Volumio2-UI (`socketioxide` **`v4`** feature) ([PORTING.md](PORTING.md)).
- **Integration test:** **`scripts/bootstrap-volumio-evo-player.sh`** is the canonical path ([TESTER_GUIDE.md](TESTER_GUIDE.md)).
- **Binary:** Prefer checked-in **`layer/binaries/<triple>/volumio-evo`**; else **`--build`** ([layer/binaries/README.md](../layer/binaries/README.md)).

## Completed in this repo (high level)

| Area | Pointer |
|------|---------|
| Playback, browse, queue, playlists (MPD), album art | [PORTING.md](PORTING.md) Part 2–3 |
| **`GET /api/host`** | Implemented ([PORTING.md](PORTING.md)); nginx proxies from UI host |
| Settings Sources: NAS mounts, share discovery | [PORTING.md](PORTING.md) §3.2 |
| Wi‑Fi list + NM apply (`nmcli`) | [NETWORK_NM.md](NETWORK_NM.md), [PORTING.md](PORTING.md) Phase 3 |
| **`callMethod`**: ALSA/MPD saves, **system_controller/system** saves, **`installBootBranding`** | `socketio.rs`; boot stack [BRANDED_BOOT.md](BRANDED_BOOT.md), [OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md) |
| Plymouth theme **`layer/plymouth/`**, **`vol-branding-v1-*`** units | [BRANDED_BOOT.md](BRANDED_BOOT.md) |
| WASM plugin host | arm64/x86_64 ([PLUGIN_ABI.md](PLUGIN_ABI.md)); armhf core only |

## Not ported / outside this repo

| Item | Notes |
|------|--------|
| Node plugins, plugin store, install zip flow | [PORTING.md](PORTING.md) Part 5–6 |
| My Volumio cloud, stock updater, OAuth/push URLs as in Node | [PORTING.md](PORTING.md) Part 5–6 |
| **`VOL:v1:initrd:*`** from initramfs | volumio-os / image recipes — [BRANDED_BOOT.md](BRANDED_BOOT.md) “Not implemented here” |

## Deferred / reference (not shipped as product requirement)

| Item | Doc |
|------|-----|
| Wayland kiosk | [KIOSK.md](KIOSK.md) |
| ALSA AAMPP priority pipeline | [PRIORITY_ALSA_AAMPP.md](PRIORITY_ALSA_AAMPP.md) |

## Maintenance

When behaviour changes: update **[PORTING.md](PORTING.md)** (parity), **[BRANDED_BOOT.md](BRANDED_BOOT.md)** (boot only), or **[NETWORK_NM.md](NETWORK_NM.md)** (NM)—then adjust **this file** only if the **authority** table or **completed / not ported** boundaries change.
