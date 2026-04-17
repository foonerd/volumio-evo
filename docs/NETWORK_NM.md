# Network stack: NetworkManager (`nmcli`)

Volumio Evo is moving from **ifupdown / dhcpcd / hostapd scripts** to **NetworkManager** so one code path can express **DHCP vs static IPv4**, **Wi‑Fi client (STA)**, **access point (AP)**, and coordinated **fallback hotspot** behaviour.

**Privilege:** when the service runs as a **non-root** user, Evo invokes **`sudo -n $VOLUMIO_EVO_NMCLI`** (see **`nmcli_bin()`**). Bootstrap installs **`/etc/sudoers.d/volumio-evo-nmcli`** — see **[OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md)**.

This document is the **contract** for implementation in `crates/core/src/nm_network.rs`, bootstrap, and (eventually) the stock UI plugin `system_controller/network` (implemented in Evo only; see workspace rules for `Volumio2-UI`).

## Goals

| Goal | Mechanism |
|------|-----------|
| Ethernet DHCP or static | `nmcli` **ethernet** (or **802-3-ethernet**) connections, `ipv4.method auto|manual` |
| Wi‑Fi STA | `nmcli` **wifi** connection, `802-11-wireless.mode infrastructure` |
| Wi‑Fi AP / hotspot | `nmcli` **wifi** profile with `mode ap` (and `ipv4.method shared` for NAT DHCP to clients) |
| STA + AP on one radio | **Best effort**: many **single-phy** Wi‑Fi chips **cannot** do concurrent STA+AP reliably. NM may expose only one active mode per interface; Evo documents **two-radio** or **USB Wi‑Fi dongle** for true concurrency. When impossible, policy is **STA preferred** with **fallback AP** on disconnect (see below). |
| Fallback hotspot | If STA fails (no link / bad credentials / no DHCP), activate a **known** NM connection profile (e.g. `volumio-hotspot`) on the Wi‑Fi device. A small **watchdog** (future: `volumio-evo-network-watchdog.service` or a loop inside Evo) re-evaluates periodically. |

## Contract: USB STA vs hotspot iface, one mode per radio, blind entry

These rules are **product/OS policy**, not optional nice-to-haves.

1. **USB dongle may be STA-only** — Many adapters **do not expose AP mode** in Linux (firmware/driver). **Hotspot must use an interface that supports AP**, often the **on-SoC** radio (e.g. `wlan0`) while **STA uses the USB** iface (e.g. `wlan1`). Evo persists this as **`fallback.hotspot_ifname`** in `intent.toml` (empty = same iface as STA for single-radio boards).

2. **One mode per physical radio** — On a given iface, NM uses **either** STA **or** AP at a time, **not both**. A dongle cannot be “STA + hotspot” concurrently on the **same** `wlan*`. Splitting STA (USB) and hotspot (SoC) uses **two** interfaces.

3. **Hotspot is always fallback** — The AP profile is a **recovery path** (user attaches phone/UI, fixes credentials). It is **not** the primary streaming link. Watchdog / policy brings hotspot **up when uplink is unusable**, not as a peer to a working STA on the same dongle.

4. **Blindfold / no-scan provisioning** — Scan (`nmcli dev wifi list`) can fail (wrong iface, driver, regdomain, RF noise). The user must still be able to **enter SSID, security, and password by hand** via persisted intent + REST **`PUT /api/v1/network/nm/intent`** with **`apply: true`**—no dependency on a successful scan. Wrong SSID/key is a user error; the stack should still allow **retry and fallback hotspot** when policy says so.

5. **If configuration is wrong** — Treat **Ethernet**, **manual STA**, and **fallback hotspot** as layered recovery: prefer wired → STA → **hotspot on `fallback.hotspot_ifname` (or STA iface if unset)** so the device remains reachable for correction.

Implementation: **`effective_hotspot_ifname`** in `network_config.rs`; apply logs when **STA iface ≠ hotspot iface**.

## Production hardware: Raspberry Pi 3 and USB Wi‑Fi

The BCM **on-SoC** Wi‑Fi on Pi 3 / 3+ is **weak RF** (small antenna, 802.11n). For a dependable client link and **Wi‑Fi 6**, a **USB dongle** is a **normal, image-supported** path—not optional “hobby” hardware.

1. **Enumeration** — The internal radio is often **`wlan0`**; a USB adapter usually appears as **`wlan1`** (verify after boot: `nmcli device`, `iw dev`).
2. **Evo primary iface** — Set **`wifi_iface = "wlan1"`** in `/etc/volumio-evo/config.toml`, or **`VOLUMIO_EVO_WIFI_IFACE=wlan1`** on the systemd unit. This drives Socket.IO / REST Wi‑Fi scan and is the default when **`wifi.ifname`** in `settings/network/intent.toml` is left empty. An explicit **`wifi.ifname`** in intent **overrides** config.
3. **Apply / NM** — Prefer the **dongle** for **STA** (`wifi.ifname` / `wifi_iface`). If the dongle is **STA-only**, set **`fallback.hotspot_ifname`** to the **SoC** iface (often `wlan0`) so the **hotspot profile** is created on a radio that supports **AP** mode. Leave `hotspot_ifname` empty only when STA and AP share the same iface (single-radio products).
4. **Firmware** — Ship **non-free firmware** packages required by the chosen USB chipset in the OS image; document **CRDA / regdomain** for certification regions.

### Wi‑Fi 5 / 6 / 7 (802.11ac / ax / be) — client vs hotspot

- **STA (client):** Evo does **not** pick a “Wi‑Fi 6 mode” flag. **Association, band, and security** are negotiated by **wpa_supplicant / the driver** from SSID, credentials, and regulatory domain. Evo persists SSID, open vs PSK, and static IP intent; NM applies the profile.
- **Hotspot (AP):** NetworkManager needs an explicit **`802-11-wireless.band`** when setting **`802-11-wireless.channel`**. Valid values are typically **`bg`** (2.4 GHz), **`a`** (5 GHz), and **`6GHz`** (6 GHz). In **`intent.toml`**, **`wifi.ap_band`** sets that property; if empty, Evo **infers** **`bg`** for channels **1–14** and **`a`** for **36–177**. For **6 GHz** AP or ambiguous channel numbers, set **`wifi.ap_band`** to **`6GHz`** (or the correct band) and **`wifi.ap_channel`** to a valid channel for that band. The stock UI channel picker remains **2.4 GHz–oriented** (1–11); 5/6 GHz hotspot tuning is via **intent / REST** until the UI gains band-aware controls.

## Multi-interface scenarios (design)

Phase 2 uses **one managed ethernet profile**, **one STA profile**, and **one hotspot template**. The **primary Wi‑Fi interface** is resolved from, in order: explicit **`wifi.ifname`** in `intent.toml`, then Evo **`wifi_iface`** / **`VOLUMIO_EVO_WIFI_IFACE`**, then default **`wlan0`**. Product requirements below drive **future** intent schema and apply logic (per-interface maps, metrics, watchdog policy).

### Multiple LAN interfaces (SoC, USB, PCIe)

NetworkManager already sees **one device per NIC**. Evo should evolve to **one connection profile per interface** (`eth0`, `enP…`, USB Ethernet, …), each with its own DHCP/static block. Discovery: `nmcli device` / sysfs; no single global “LAN” abstraction in NM.

### Multiple Wi‑Fi interfaces (on-SoC, PCIe, USB)

Same pattern: **key profiles by `ifname`** (e.g. `wlan0`, `wlan1`). **Concurrent STA + AP on one radio** remains hardware-limited; **two radios** (internal + USB) can each host STA or AP independently. USB **hotplug** implies reacting to NM **device-added** (or periodic refresh) so new `wlan*` devices get profiles.

### LAN and Wi‑Fi up together — priority

**Default route and DNS** are OS/NM concerns, not a single Evo toggle:

- Prefer **wired over wireless** by setting **lower `ipv4.route-metric`** on Ethernet than on Wi‑Fi, and/or **`ipv4.never-default`** on backup links.
- Document one **intended primary egress**; avoid ambiguous split defaults unless advanced routing is required.

### Fallback from one link to another

**Autoconnect** helps but is not enough for “STA failed → hotspot” or “Ethernet down → Wi‑Fi”. A small **policy layer** (watchdog in Evo or systemd) should use **connectivity** (not only link up), then **`nmcli connection up/down`** the next profile in order. Chains are product-defined (e.g. eth → STA → hotspot on a **chosen** AP-capable iface).

### On-the-go: no network, new LAN or Wi‑Fi

- **No usable uplink:** bring up **fallback hotspot** on a designated interface so a phone/UI can attach and push new credentials.
- **New SSID or new cable:** user or UI updates **intent**; NM may use **multiple saved Wi‑Fi connections** (one per SSID) — Evo’s single STA profile name is a simplification until multi-profile history exists.
- **Changing locations:** expect **new default route and DNS**; diagnostics should distinguish **L2 up** vs **routable IPv4** where possible.

### AirPlay certification — single network mode

Certification and stable **mDNS/Bonjour** often require **one coherent L3 path** for discovery and streaming:

- **Single-network mode (policy):** when enabled, enforce **one default route** and predictable discovery — e.g. **higher metrics or `never-default` on secondary interfaces**, or only one “active” profile group for AirPlay.
- Implementation detail depends on the AirPlay/shairport stack (binding to one address vs system-wide); Evo can expose a **user-visible mode** that tightens NM policy while AirPlay is enabled.

## Non-interactive operation

All automation uses **`nmcli`** in **non-interactive** mode (no TTY prompts). Evo runs as **root** in default bootstrap, or uses **`sudo -n /usr/bin/nmcli …`** with a **narrow NOPASSWD** rule (see **[OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md)**).

Before Wi‑Fi scan, Evo checks **`/sys/class/rfkill`** for **soft-blocked `wlan`** radios and runs **`rfkill unblock wifi`** (root) or **`sudo -n $VOLUMIO_EVO_RFKILL unblock wifi`** (see **`volumio-evo-rfkill`** sudoers). Install the **`rfkill`** package on minimal images if the binary is missing.

## Persistence

Desired state (SSID, keys, static IP, hotspot SSID, fallback flags) is stored under:

`$VOLUMIO_EVO_SETTINGS_DIR/network/` (see **[SETTINGS_LAYOUT.md](SETTINGS_LAYOUT.md)**).

Runtime truth is always **what NM reports** (`nmcli device`, `nmcli connection show --active`).

**Manual Wi‑Fi (no scan):** set `wifi.sta_ssid`, `wifi.sta_open` / `wifi-sta.psk`, and optional static fields in `intent.toml`, then **`PUT`** with `apply: true`. Scan is optional for provisioning.

### REST (Phase 2)

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/v1/network/nm/intent` | Load `intent.toml` (defaults if missing) + whether PSK sidecars exist (no secret values). |
| PUT | `/api/v1/network/nm/intent` | Replace `intent.toml`. Optional `sta_psk` / `ap_psk` (empty string clears sidecar). Set `apply: true` to run `apply_network_intent` in `crates/core/src/nm_network.rs` (`nmcli`) after save. |

## `nmcli` reference (operations)

- **Status:** `nmcli -t -f DEVICE,TYPE,STATE,CONNECTION device`
- **Wi‑Fi scan:** `nmcli -t -f SSID,SIGNAL,SECURITY,ACTIVE dev wifi list [ifname <iface>]`
- **Activate / deactivate:** `nmcli connection up|down <name|uuid>`
- **AP profile (illustrative):** created once via `nmcli connection add type wifi ifname <iface> con-name volumio-hotspot autoconnect no wifi.mode ap wifi.ssid <ssid> ipv4.method shared …` (exact flags depend on distro/NM version).

## Phased implementation

| Phase | Scope |
|-------|--------|
| **1** | Parse-only + scan → JSON matching `pushWirelessNetworks.available[]`; REST diagnostic; bootstrap installs `network-manager` when enabled. |
| **2 (current)** | `settings/network/intent.toml` + `wifi-*.psk` sidecars; **`wifi_iface`** / **`fallback.hotspot_ifname`** (STA-only USB vs SoC AP); **`GET/PUT /api/v1/network/nm/intent`** (optional `apply: true`); idempotent `nmcli` apply for ethernet + Wi‑Fi STA/AP + dormant fallback hotspot profile (STA mode). |
| **3** | Watchdog / timer to activate fallback hotspot when STA fails; NAT tuning if needed. |
| **4** | Socket.IO parity with Node (`saveWirelessNetworkSettings`, wizard) + polkit fine-tuning for non-root service user. |

## Related

- **[OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md)** — `nmcli` and `sudo -n`
- **[SETTINGS_LAYOUT.md](SETTINGS_LAYOUT.md)** — `settings/network/`
