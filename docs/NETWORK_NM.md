# Network stack: NetworkManager (`nmcli`)

Volumio Evo is moving from **ifupdown / dhcpcd / hostapd scripts** to **NetworkManager** so one code path can express **DHCP vs static IPv4**, **Wi‑Fi client (STA)**, **access point (AP)**, and coordinated **fallback hotspot** behaviour.

**Privilege:** when the service runs as a **non-root** user, Evo invokes **`sudo -n $VOLUMIO_EVO_NMCLI`** (see **`nmcli_bin()`**). Bootstrap installs **`/etc/sudoers.d/volumio-evo-nmcli`** — see **[OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md)**.

This document is the **contract** for implementation in `crates/core/src/nm_network.rs`, bootstrap, and (eventually) the stock UI plugin `system_controller/network` (implemented in Evo only; see workspace rules for `Volumio2-UI`).

## Intent vs state (toggles vs NM reality)

**Policy uses two layers; they must not be confused.**

| Layer | Meaning | Owned by |
|-------|---------|----------|
| **Intent** | What we **persist** and **ask** NetworkManager to do — UI toggles, `intent.toml`, “LAN enabled with DHCP or static”, “AP enabled”, etc. | Product / user / Evo settings |
| **State** | What **actually happened** after physics and software — cable in or out, association, DHCP, AP beaconing, NM errors. | Kernel / NM / drivers / PHY |

**Mantra:** *Toggles decide what we try; states decide whether we must fall back.*

When intent and state **diverge**, that gap is where **safety / fallback** logic applies — not as a fourth independent “magic toggle”, but as **derived behaviour** from **intent + measured outcome**.

Examples:

| Intent (toggle / setting) | State (reality) | Why it matters |
|-----------------------------|-----------------|----------------|
| **LAN enabled** (Ethernet on in UI, DHCP or static configured) | **No link** — cable unplugged, switch/port dead (**no carrier**) | User still **wants** wired uplink; **state** says there is none. This is **not** “user turned Ethernet off”. |
| **AP enabled** | **`nmcli connection up` fails** or AP never beaconing after retries | User **wants** provisioning Wi‑Fi; **state** says AP did not come up (intermittent init, concurrent-mode limits, …). |
| **STA enabled** | Not associated / no IP | Wrong PSK, AP out of range, etc. |

### “No LAN” (for fallback rules)

**No LAN** means: **`ethernet.enabled`** is **`true`** in **`intent.toml`** (user wants wired Ethernet, DHCP or static), **but** the resolved NIC has **no usable link** — e.g. **no carrier** (cable gone, path dead). If **`ethernet.enabled`** is **`false`**, Evo does **not** manage an Ethernet NM profile (“Wi‑Fi‑only” intent); **critical recovery** must **not** treat that as “LAN wanted but missing”. Implementations use sysfs **`carrier`** for the resolved iface (see `nm_network.rs`).

### Critical recovery (single Wi‑Fi interface, contract intent)

When **AP is enabled in intent** but **AP state remains failed** after **bounded retries** (intermittent bring-up), **and** **no LAN** holds as above, the device must still be reachable: activate an **open** hotspot (**no WPA passphrase** — omit `802-11-wireless-security`; see the *Open hotspot* paragraph under [Persistence](#persistence)). Exact **retry counts**, **AP-up detection**, and **watchdog cadence** belong in implementation and phase notes — the **product requirement** is: **do not strand** the unit with **no AP** and **no wire**.

### More than one Wi‑Fi interface (SoC vs USB)

Intent/state apply **per interface** once Evo tracks **which `wlan*` is STA vs AP**. See **[Role assignment when several `wlan*` exist](#role-assignment-when-several-wlan-exist-faster-sta-ap-where-supported)** below. **[Contract: USB STA vs hotspot iface](#contract-usb-sta-vs-hotspot-iface-one-mode-per-radio-blind-entry)** and **[Multiple Wi‑Fi interfaces](#multiple-wi-fi-interfaces-on-soc-pcie-usb)** add hardware context; a **full intent/state truth table per radio** remains **follow-up** with product.

## Goals

| Goal | Mechanism |
|------|-----------|
| Ethernet DHCP or static | `nmcli` **ethernet** (`volumio-evo-ethernet`) when **`ethernet.enabled`** is **`true`**; skipped when **`false`** (no NM Ethernet apply). If enabled but **no** NIC exists, apply logs a warning and continues (Wi‑Fi‑only hardware). |
| Wi‑Fi STA | `nmcli` **wifi** connection, `802-11-wireless.mode infrastructure` |
| Wi‑Fi AP / hotspot | `nmcli` **wifi** profile with `mode ap` (and `ipv4.method shared` for NAT DHCP to clients) |
| STA + AP on one radio | **Canonical:** create a **virtual AP vif** on the STA's PHY — `iw dev <sta> interface add <ap> type __ap` — then bind STA NM profile to `<sta>` and AP NM profile to `<ap>` with `802-11-wireless.mode ap` + `ipv4.method shared`. Evo auto-creates `ap0` when `iw phy … valid interface combinations` admits **`managed + AP`** (e.g. **Pi 5 brcmfmac / CYW43455**; see `valid interface combinations`). AP channel **follows STA** because that combination is `#channels <= 1`. When the PHY does **not** list such a combination (true single-mode chips), Evo falls back to the old shared-ifname behaviour and warns accordingly. See **[Verifying concurrent STA + AP on one PHY (`iw`)](#verifying-concurrent-sta--ap-on-one-phy-iw)**. |
| Fallback hotspot | If STA fails (no link / bad credentials / no DHCP), activate a **known** NM connection profile (e.g. `volumio-hotspot`) on the Wi‑Fi device. A small **watchdog** (future: `volumio-evo-network-watchdog.service` or a loop inside Evo) re-evaluates periodically. |

## UI toggles vs hardware capability (product policy)

Stock network UI exposes three related controls. **Effective behaviour** depends on whether the platform can run **STA and AP at the same time** (e.g. two radios, or driver/NM support for concurrent modes) vs **only one mode at a time** on a single radio.

| Hardware | **Wireless Networking** (client / STA) | **Enable Hotspot** (AP) | **Hotspot Fallback** |
|----------|----------------------------------------|-------------------------|-------------------------|
| **STA and AP** (two `wlan*` devices, **or** one iface with driver/NM concurrent STA+AP) | **On/off** — controls STA as usual. | **On/off** — maintains profile; **`connection up`** when rules in [Automatic hotspot](#automatic-hotspot-connection-up-sta-mode) apply. | With **Enable Hotspot**, on a **shared** iface Evo **attempts** **`connection up`** on the hotspot after STA so **both** may be active if the stack supports it. |
| **STA-only stack** (driver cannot combine STA+AP on one phy; no second radio) | **On/off** — STA as usual. | Hotspot **`connection up`** may **fail** while STA is up; use **split** **`fallback.hotspot_ifname`** or disable STA for a dedicated AP. | Same: concurrent mode is **best effort**; failure is a driver limit, not Evo skipping the attempt. |

**Summary**

- **Dual-radio / split iface:** STA and hotspot can both run — see **`fallback.hotspot_ifname`**.
- **Single iface + both hotspot toggles on:** Evo **always attempts** hotspot **`connection up`** after STA (see table below); success requires **chipset + kernel + NM** support for concurrent STA+AP.
- **Hotspot Fallback (runtime):** future watchdog when STA drops — [Phased implementation](#phased-implementation) phase 3.

Implementation status: apply logic matches [Automatic hotspot](#automatic-hotspot-connection-up-sta-mode); watchdog is phase 3.

## Contract: USB STA vs hotspot iface, dual radio and concurrent mode, blind entry

These rules are **product/OS policy**, not optional nice-to-haves.

1. **USB dongle may be STA-only** — Many adapters **do not expose AP mode** in Linux (firmware/driver). **Hotspot must use an interface that supports AP**, often the **on-SoC** radio (e.g. `wlan0`) while **STA uses the USB** iface (e.g. `wlan1`). Evo persists this as **`fallback.hotspot_ifname`** in `intent.toml` (empty = same iface as STA for single-radio boards).

2. **Physical radio vs concurrent STA+AP (canonical single-PHY recipe)** — On a PHY that lists a `managed + AP` combination under `iw phy <n> info` / `valid interface combinations` (e.g. **Pi 5 / brcmfmac CYW43455**), Linux supports concurrent STA+AP by creating a **second virtual interface on the same PHY**, not by binding two NM profiles to one `wlan*`. Evo automates this at apply time:
    1. probe `iw phy` → detect `managed + AP` with `#channels <= 1`,
    2. create `ap0` (or `VOLUMIO_EVO_AP_IFNAME`): `iw dev <sta> interface add <ap> type __ap`,
    3. bind the STA NM profile to `<sta>` and the AP NM profile to `<ap>` with `802-11-wireless.mode ap`, `ipv4.method shared`,
    4. the AP channel/band **follow** whatever STA is associated on (`#channels <= 1` rule); Evo rewrites the AP profile's `band`/`channel` from `iw dev <sta> link` when STA is associated.
    USB adapters that **do not** list such a combination are treated as **single-mode** — Evo binds the hotspot profile to `<sta>` and uses the settle/restore logic for when NM swaps profiles on one device.
    **Dual radios** (`fallback.hotspot_ifname` on another `wlan*`) remain fully supported for adapters that cannot do single-PHY STA+AP.

3. **Hotspot as recovery path** — Where the stack **cannot** keep STA and hotspot up together, AP complements STA for provisioning, not throughput; see [UI toggles vs hardware capability](#ui-toggles-vs-hardware-capability-product-policy). **Hotspot Fallback** only applies where **AP mode exists** on some interface.

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

### Verifying concurrent STA + AP on one PHY (`iw`)

Linux reports wiphy capability in **`iw phy<N> info`** (replace **`phy<N>`** with **`phy0`**, etc.). Under **`valid interface combinations`**, look for a rule that combines **managed** (station / client, “STA”) and **AP**.

Example (**Raspberry Pi 5**, Broadcom **`brcmfmac`**): one combination allows **`# { managed } <= 1`**, **`# { AP } <= 1`**, plus optional P2P roles, **`#channels <= 1`** — meaning the driver permits **one client interface and one AP interface** on that PHY (typically **same channel**). That is **kernel/driver truth**, independent of Evo.

If **`iw`** does **not** list such a combination for a dongle, treat concurrent STA+AP on one **`wlan*`** as unsupported there and use **`fallback.hotspot_ifname`** or a second radio.

**Implementation.** Evo probes the PHY at apply time via **`crate::wifi_phy`** (`phy_capability`, `sta_phy_supports_concurrent_sta_ap`, `ensure_ap_vif_present`/`absent`, `sta_link_info`). On a concurrent-capable PHY it creates **`ap0`** as a `__ap`-type vif with **`iw dev <sta> interface add ap0 type __ap`**, binds the hotspot NM profile to **`ap0`**, and sets the AP band/channel from `iw dev <sta> link` so the AP follows the STA's channel (the `#channels <= 1` rule). Privileged callers: **`iw`** is added to the bootstrap sudoers (`/etc/sudoers.d/volumio-evo-iw`, matching the `nmcli` / `rfkill` drop-ins).

**Referenced recipes / sources** (rationale for this design):

- [Linux Wireless — mac80211 virtual interface support](https://linuxwireless.sipsolutions.net/en/users/Documentation/iw/vif/) — canonical `iw dev interface add type` command.
- [Unix StackExchange — deciphering `valid interface combinations`](https://unix.stackexchange.com/questions/401464/deciphering-the-output-of-iw-list-valid-interface-combinations) — `managed + AP` combination syntax and `#channels` semantics.
- [Ezurio — AP-Sta mode with nmcli](https://www.ezurio.com/support/faqs/how-do-i-configure-ap-sta-mode-using-nmcli) — vendor-confirmed `iw dev wlan0 interface add wlan1 type __ap` + `nmcli con add ifname wlan1 mode ap ipv4.method shared` + `nmcli con add ifname wlan0` pattern; “AP follows STA channel”.
- [Nathan Lewis — AP+STA mode on Raspberry Pi 5](https://nrlewis.dev/blog/rpi-hotspot/) — same `iw … type __ap` recipe for Pi 5 with NetworkManager / Netplan.
- [RaspAP docs — AP-STA mode](https://docs.raspap.com/features-experimental/ap-sta/) — `uap0` virtual interface + `iw phy` capability gate (`managed + AP`, `#channels <= 1`).
- [Raspberry Pi Forums — AP + Wi‑Fi on same device (NetworkManager solution)](https://forums.raspberrypi.com/viewtopic.php?t=372305) — community-validated NM recipe.
- [Raspberry Pi Forums — Run AP and Wi‑Fi simultaneously using nmcli](https://forums.raspberrypi.com/viewtopic.php?t=381051) — further community confirmation.
- [raspberrypi/linux #7092 — brcmfmac concurrent STA+AP crash on kernel 6.12](https://github.com/raspberrypi/linux/issues/7092) — known caveat: **Bookworm** stable, **Trixie / kernel 6.12** exhibits firmware regressions in this mode; product should log and not rely on kernel stability.
- [Raspberry Pi forums — virtual AP + STA state of the art](https://forums.raspberrypi.com/viewtopic.php?t=212991) — MAC address rule: **do not** randomize the virtual AP MAC on brcmfmac (clients get DHCP but cannot communicate).
- [NetworkManager reference — 802-11-wireless](https://networkmanager.dev/docs/api/latest/settings-802-11-wireless.html) — authoritative `mode=ap` / `band` / `channel` / `cloned-mac-address` / `ipv4.method=shared` semantics.
- [Baeldung — WiFi AP via nmcli with `ipv4.method shared`](https://www.baeldung.com/linux/nmcli-wap-sharing-internet) — canonical NM hotspot recipe reused on `ap0`.
- [Red Hat — configuring NetworkManager to ignore devices](https://access.redhat.com/documentation/de-de/red_hat_enterprise_linux/8/html/configuring_and_managing_networking/configuring-networkmanager-to-ignore-certain-devices_configuring-and-managing-networking) — `conf.d` device rules; used when the local distro aggressively ignores or manages new `wlan*` vifs.

## Multi-interface scenarios (design)

Phase 2 uses **one managed ethernet profile**, **one STA profile**, and **one hotspot template**. The **primary Wi‑Fi interface** is resolved from, in order: explicit **`wifi.ifname`** in `intent.toml`, then Evo **`wifi_iface`** / **`VOLUMIO_EVO_WIFI_IFACE`**, then default **`wlan0`**. Product requirements below drive **future** intent schema and apply logic (per-interface maps, metrics, watchdog policy).

### Multiple LAN interfaces (SoC, USB, PCIe)

NetworkManager already sees **one device per NIC**. Evo should evolve to **one connection profile per interface** (`eth0`, `enP…`, USB Ethernet, …), each with its own DHCP/static block. Discovery: `nmcli device` / sysfs; no single global “LAN” abstraction in NM.

### Multiple Wi‑Fi interfaces (on-SoC, PCIe, USB)

Same pattern: **key profiles by `ifname`** (e.g. `wlan0`, `wlan1`). **Concurrent STA + AP on one PHY** depends on **`iw phy … valid interface combinations`** (see above); **two radios** (internal + USB) are another way to run STA and AP independently. USB **hotplug** implies reacting to NM **device-added** (or periodic refresh) so new `wlan*` devices get profiles.

### Role assignment when several `wlan*` exist (faster STA, AP where supported)

**Traffic vs management**

| Role | Purpose | Priority |
|------|---------|----------|
| **STA (client)** | **Traffic** — streaming, internet, primary user data path | **Higher** |
| **AP (hotspot)** | **Management** — direct attachment so the user can configure the device (phone/UI) | **Lower** than STA for throughput; still **required when enabled** so the box stays reachable |

**When more than one interface can do AP+STA**

1. Prefer the **faster** radio for **STA** (typically **802.11ac / ax / be**, **5 GHz / 6 GHz**, better PHY — use **`iw phy`**, chip generation, or band support as signals; see existing notes on dual-band preference).
2. **STA wins** on that iface for **traffic**. **AP is not equal priority** — it exists so you can **manage** the unit when needed.

**When the preferred (faster) STA iface cannot run STA+AP together** (driver limitation, or **AP enabled** but concurrent mode fails in **state** after retries)

- If **Enable Hotspot** is **on** in intent: bring up the hotspot on **an interface that supports AP**, even if that means **another** `wlan*` (e.g. **AP on SoC `wlan0`**, **STA on faster USB `wlan1`**). Persist this split with **`wifi.ifname`** (STA) and **`fallback.hotspot_ifname`** (AP) in `intent.toml` / **`/etc/volumio-evo/config.toml`** when the product UX exposes it.

**When one interface is STA-only** (many USB dongles **do not** expose AP in Linux)

- Use that iface **only for STA** (traffic).
- Never bind the **hotspot profile** to it; set **`fallback.hotspot_ifname`** to an **AP-capable** radio (often on-SoC **`wlan0`**).

**Follow-up:** Quantitative **“faster”** ranking (PHY list, heuristic order) and **runtime split** when concurrent STA+AP fails mid-session can be tightened in implementation; **[Intent vs state](#intent-vs-state-toggles-vs-nm-reality)** still applies per iface.

**Two interfaces, both AP+STA capable, different band support:** Prefer the radio that can use **5 GHz / 6 GHz** for **STA** when the chosen network is available on that band (same SSID on dual-band APs, or scan shows 5/6 GHz). If one radio is **2.4 GHz–only** and the other covers **5/6 GHz**, treat the **2.4 GHz–only** iface as **secondary** (e.g. hotspot-only, backup STA, or fallback) unless only 2.4 GHz is offered. Implementation (future): pick **`wifi.ifname`** / metrics / per-SSID profile binding from **`iw phy`**, scan results, and NM capabilities — not hard-coded SSID logic in Phase 2.

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

### STA WPA‑PSK: `psk-flags` + `connection down` (contract)

Non-interactive **`nmcli connection up`** on WPA‑PSK STA requires the profile to carry a **system-stored** PSK (**`wifi-sec.psk-flags 0`**) in the NM keyfile. Evo reads the passphrase from **`wifi-sta.psk`**, sets **`wifi-sec.psk-flags`** before **`wifi-sec.psk`**, and — because some NetworkManager versions do **not** reliably migrate off **agent-owned** secrets when both land in **one** `nmcli connection modify` line — runs **an additional dedicated** `nmcli connection modify <sta-profile> wifi-sec.psk-flags 0` **immediately before** the full STA profile update.

When applying **`WifiRole::Sta`**, Evo also **`nmcli connection down`** the STA profile **before** rewriting credentials (`connection_down_lossy`). That avoids activation state holding stale secret metadata so **`connection up`** fails with *password for '802-11-wireless-security.psk' not given in 'passwd-file'* despite a valid sidecar (regression fixed 2026‑04; concurrent **`ap0`** STA+AP path exercised the bug most visibly).

The on-disk **`psk=`** value in **`/etc/NetworkManager/system-connections/*.nmconnection`** remains readable to **root** (0600); that is normal NM behaviour for system-stored secrets.

## Persistence

Desired state (SSID, keys, static IP, hotspot SSID, fallback flags) is stored under:

`$VOLUMIO_EVO_SETTINGS_DIR/network/` (see **[SETTINGS_LAYOUT.md](SETTINGS_LAYOUT.md)**).

Runtime truth is always **what NM reports** (`nmcli device`, `nmcli connection show --active`).

**Manual Wi‑Fi (no scan):** set `wifi.sta_ssid`, `wifi.sta_open` / `wifi-sta.psk`, and optional static fields in `intent.toml`, then **`PUT`** with `apply: true`. Scan is optional for provisioning.

**Open hotspot (no AP passphrase):** The profile must have **no** `802-11-wireless-security` section — same as **`nmcli connection add type wifi … wifi.mode ap`** without any `wifi-sec.*` properties. **Do not** use **`wpa-psk`** with an **empty** **`psk`**: NetworkManager defines WPA-PSK as **8–63** ASCII characters (or **64** hex), and hostapd only treats the BSS as **open** when WPA options are **omitted**, not when the passphrase is empty; clients will still see **WPA** and demand a password. For profiles that **previously** had WPA, Evo runs **`nmcli connection modify … remove 802-11-wireless-security`** before bringing the AP up. When **`wifi-ap.psk`** / UI protection is set, Evo applies normal **WPA-PSK** with that passphrase.

**Automatic hotspot `connection up` (STA mode):** `ensure_hotspot_profile` only maintains the NM profile (autoconnect **no**). When **Enable Hotspot** is on, Evo runs **`nmcli connection up`** on the hotspot profile with **retries** (several attempts, short delay — intermittent NM/driver init). **`Hotspot Fallback`** does **not** gate whether we **attempt** AP bring-up when hotspot is enabled; it remains relevant for future **runtime** watchdog behaviour. On a **single** Wi‑Fi interface, concurrent STA+AP requires chipset/driver/NM support. STA **`connection up`** is **nonfatal** when **Enable Hotspot** is on (shared ifname, split radios, or STA on **`wlan0`** + AP on virtual **`ap0`**) so a bad STA link does not block bringing up the hotspot.

If every **`connection up`** attempt fails **and** Ethernet intent targets an iface with **no carrier** ([**No LAN**](#no-lan-for-fallback-rules)), Evo applies **critical recovery**: **`nmcli connection modify … remove 802-11-wireless-security`** then retries **`connection up`** so an **open** AP can come up ([**Critical recovery**](#critical-recovery-single-wi-fi-interface-contract-intent)).

| Wireless (STA) | Enable Hotspot | Same STA/AP iface (`fallback.hotspot_ifname` empty) | Split STA/AP iface |
|----------------|----------------|------------------------------------------------------|---------------------|
| On | **On** | Retries **`connection up`** hotspot after STA (best-effort concurrent STA+AP) | Same retries on hotspot iface |
| On | Off | Hotspot **`connection down`** at start of STA apply | Hotspot down if not enabled |
| Off (wifi role disabled) | — | STA and hotspot brought down earlier in apply | — |

**Hotspot Fallback** toggle: does **not** control the initial **`connection up`** when **Enable Hotspot** is on (see **[Intent vs state](#intent-vs-state-toggles-vs-nm-reality)**).

**Preferred STA interface (multiple radios):** when NetworkManager reports more than one `wifi` device, the Network settings page shows **Preferred Wi-Fi interface** and stores **`settings/network/wifi_iface_preferred`**, merged into **`wifi_iface`** in **`/etc/volumio-evo/config.toml`** when privileges allow (see **[OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md)**). Resolution order for scans/apply: **`VOLUMIO_EVO_WIFI_IFACE`** env → preferred file → `config.toml` → NM heuristic. **`GET /api/v1/network/nm/wifi-devices`** lists devices and the effective choice.

### REST (Phase 2)

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/v1/network/nm/intent` | Load `intent.toml` (defaults if missing) + whether PSK sidecars exist (no secret values). Response **`intent.ethernet.enabled`** reflects persisted LAN intent. |
| PUT | `/api/v1/network/nm/intent` | Replace `intent.toml`. **`intent.ethernet.enabled`**: **`true`** = manage wired NM profile; **`false`** = Wi‑Fi‑only (skip Ethernet apply). Optional `sta_psk` / `ap_psk` (empty string clears sidecar). Set `apply: true` to run `apply_network_intent` (`nmcli`) after save. |

## `nmcli` reference (operations)

- **Status:** `nmcli -t -f DEVICE,TYPE,STATE,CONNECTION device`
- **Wi‑Fi scan:** `nmcli -t -f SSID,SIGNAL,SECURITY,ACTIVE dev wifi list [ifname <iface>]`
- **Activate / deactivate:** `nmcli connection up|down <name|uuid>`
- **AP profile (illustrative):** created once via `nmcli connection add type wifi ifname <iface> con-name volumio-hotspot autoconnect no wifi.mode ap wifi.ssid <ssid> ipv4.method shared …` (exact flags depend on distro/NM version).

## Phased implementation

| Phase | Scope |
|-------|--------|
| **1** | Parse-only + scan → JSON matching `pushWirelessNetworks.available[]`; REST diagnostic; bootstrap installs `network-manager` when enabled. |
| **2 (current)** | `settings/network/intent.toml` + `wifi-*.psk` sidecars; **`wifi_iface`** / **`fallback.hotspot_ifname`** (STA-only USB vs SoC AP); **`GET/PUT /api/v1/network/nm/intent`** (optional `apply: true`); idempotent `nmcli` apply; hotspot **`connection up`** per table above (same-iface + both toggles → try concurrent STA+AP). |
| **3** | Watchdog / timer for **runtime** STA loss (not only apply-time); NAT tuning if needed. |
| **4** | Socket.IO parity with Node (`saveWirelessNetworkSettings`, wizard) + polkit fine-tuning for non-root service user. |

## Related

- **[OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md)** — `nmcli` and `sudo -n`
- **[SETTINGS_LAYOUT.md](SETTINGS_LAYOUT.md)** — `settings/network/`
