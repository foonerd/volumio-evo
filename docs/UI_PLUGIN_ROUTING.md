# Stock UI routes (compatibility adapter)

The Angular UI targets **volumio3-backend**, where **Settings** pages were real plugin packages (`app/plugins/...`). **Evo** keeps the **same string IDs** on the wire: `pluginName`, **`getUiConfig`** `page`, and **`callMethod`** `endpoint` + `method`.

Per **[CONCEPT.md](CONCEPT.md) §6**, this surface is a **temporary projection adapter** — stock UI parity until consumers use **fabric projections** and **happenings** directly. This document is only the **routing map** (which Rust modules answer which wire paths). It is not the long-term plugin contract.

**Scope:** behavioural mapping only. It does not define WASM import/export ABIs ([PLUGIN_ABI.md](PLUGIN_ABI.md)) or rack slot shapes ([CONCEPT.md](CONCEPT.md)).

---

## 1. Main menu → logical plugin

From **`get_menu_items`** (`api/socketio.rs`): each Settings row uses **`state: volumio.plugin`** and **`params.pluginName`** (historical plugin path).

| Menu id | Label | `pluginName` | Conceptual plugin |
|---------|-------|----------------|-------------------|
| mymusic | Sources | `miscellanea/my_music` | Sources / NAS |
| playback_options | Playback Options | `audio_interface/alsa_controller` | ALSA + MPD playback options (one page, two save namespaces in **`callMethod`)** |
| appearance | Appearance | `miscellanea/appearance` | Layout + language + wallpapers |
| network | Network | `system_controller/network` | NM + wired/wireless/hotspot + SMB block |
| system | System | `system_controller/system` | Device name, locale, privacy, updates, kiosk, branding installers |
| plugin-manager | Plugins | *(state `volumio.plugin-manager`)* | Plugin manager UI (store mostly stubbed) |

Other entries (**Music** browse, **Alarm** / **Sleep** / **Shutdown** modals, **Help** / **Shop** iframes) are **not** `pluginName` routes — they are **core shell** navigation.

---

## 2. `getUiConfig` — page string → implementation

Payload **`{ page: "<plugin path>" }`** → **`pushUiConfig`**. Implemented branches:

| `page` | Behaviour |
|--------|-----------|
| **`audio_interface/alsa_controller`** | Builds Playback Options UI (`build_playback_options_ui` — ALSA cards, DAC list, mixer, volume section scaffolding). |
| **`miscellanea/my_music`** | **`sources_ui::my_music_ui_config`** — Sources form (shares, discovery, NAS). |
| **`system_controller/network`** | **`network_ui::network_settings_ui_config_merged_enriched`** — may also **`openModal`** for preferred Wi‑Fi iface. |
| **`system_controller/system`** | **`emit_system_ui_config`** — System page (locale, kiosk, SMB summary, branding buttons, …). |
| **`miscellanea/appearance`** | **`system_ui::miscellanea_appearance_ui_config`** — layout picker, language, background UI metadata. |
| **anything else** | **`empty_ui_config()`** — empty sections (logical “plugin not loaded”). |

So **`miscellanea/appearance`** *is* the Appearance plugin route in the same sense Node used — only the implementation file is **`system_ui.rs`** + **`appearance`** / **`backgrounds`** / **`ui_bootstrap`**.

---

## 3. `callMethod` — `endpoint` + `method` → logical plugin slices

All handled in **`call_method`** (`api/socketio.rs`). Grouped by **endpoint prefix** (same folder semantics as Node plugins).

### `miscellanea/*`

| Endpoint | Method | Role |
|----------|--------|------|
| `miscellanea/albumart` | `clearAlbumartCache` | Album art cache bust → **`albumart`** |
| `miscellanea/appearance` | `setVolumio3UI` | Active layout → **`ui_bootstrap`** |
| `miscellanea/appearance` | `setLanguage` | **`system_settings`** language |

### `system_controller/system`

| Method | Role |
|--------|------|
| `saveGeneralSettings` | Hostname / device name |
| `saveLocaleSettings` | Timezone + reg domain |
| `saveUpdateSettings` | Update prefs (stub semantics) |
| `savePrivacySettings` | Telemetry flag |
| `saveKioskSettings` | Kiosk toggle + layer install path → **`kiosk`**, **`kiosk_install`** |
| `installBootBranding` | **`boot_branding`** |
| `installKioskLayer` | **`kiosk_install`** |

### `audio_interface/alsa_controller`

| Method | Role |
|--------|------|
| `saveAlsaOptions` | I2S / output device / **`i2s`**, **`alsa`**, MPD fragment |
| `saveVolumeOptions` | Mixer curve + HW/SW transitions |
| `saveResamplingOpts` | Resampling → playback fragment |

### `music_service/mpd`

| Method | Role |
|--------|------|
| `savePlaybackOptions` | Buffer, DSD, normalisation, … → **`playback_options`**, MPD fragment |

### `system_controller/network`

| Method | Role |
|--------|------|
| `savePreferredWifiIface` | Wi‑Fi PHY preference |
| `saveWiredNet` | Ethernet intent → **`nm_network`** |
| `saveWirelessNet` | Wi‑Fi STA intent |
| `saveHotspotSettings` | AP / hotspot |
| `saveSambaSettings` | SMB server → **`samba_*`** |

Unhandled **`callMethod`** pairs fall through to a debug log (**“no Evo handler”**) — same as a missing Node plugin method.

---

## 4. Transport vs Settings routes in this picture

| Layer | Meaning here |
|-------|----------------|
| **Shell + playback (not Settings plugin paths)** | Socket handlers such as **`getState`**, **`browseLibrary`**, queue, **`volume`**, **`play`**, **`search`**, **`pushState`**, **`mpd.rs`**, **`RouterState`** playback fields. The browse/play shell works without opening Settings. |
| **Settings-shaped routes** | **`miscellanea/*`**, **`system_controller/*`**, **`audio_interface/*`**, **`music_service/mpd`** — historical plugin IDs on the wire; Rust modules grouped by those names. Target: same concerns as **fabric rack** settings, surfaced via projections ([CONCEPT.md](CONCEPT.md)). |

These boundaries are useful for refactors today; **slot manifests** replace “logical plugin” as the long-term structural unit ([CONCEPT.md](CONCEPT.md) §3–4).

---

## Related

- Parity inventory: [PORTING.md](PORTING.md).
- Module inventory vs fabric racks: [PLUGIN_CORE_VS_EXTENSIONS.md](PLUGIN_CORE_VS_EXTENSIONS.md).
- Index: [DOCUMENTATION_MAP.md](DOCUMENTATION_MAP.md).
