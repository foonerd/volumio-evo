# UI gap tracking (Volumio2-UI ↔ Evo)

This document records **optional or recommended changes in the stock Volumio web UI** ([Volumio2-UI](https://github.com/volumio/Volumio2-ui)) when pairing it with **Volumio Evo** as the backend. Evo intentionally does **not** modify that repository; workarounds live in Evo where possible. Use this file to track **what** would need to change **where**, and **why**, if you later fork the UI or contribute upstream.

Long term, consumers should attach to **fabric projections** and **happenings** ([CONCEPT.md](CONCEPT.md)); entries here assume the **stock UI compatibility adapter** ([CONCEPT.md](CONCEPT.md) §6 UI row).

**Related:** [PORTING.md](PORTING.md) (parity inventory), [CONCEPT.md](CONCEPT.md) (fabric).

---

## Why this exists

- Evo targets **API compatibility** (REST + Socket.IO names and coarse payload shapes) so the existing UI can run unmodified.
- Some UI code assumes **Node/Volumio3** behaviour (non-empty arrays, paired request/response events, exact Socket.IO casing). Evo stubs or synthesize data where needed; **UI-side hardening** would remove awkward backend shims and improve behaviour on **any** minimal backend.

---

## How to use this doc

| Column / section | Meaning |
|------------------|--------|
| **Where** | Path under `Volumio2-UI/src/app/…` (or other repo). |
| **Change** | Concrete adjustment (behaviour or structure). |
| **Why** | User-visible or correctness reason; Evo impact called out when relevant. |
| **Evo today** | What Evo does without UI changes (if anything). |
| **Priority** | Suggested order: **P0** broken/hung UX, **P1** correctness/bugs, **P2** polish, **P3** optional cleanup. |

When Evo gains a new handler, update **Evo today** and downgrade or strike the row. When the UI is forked for Evo, implement rows as tickets.

---

## 1. Plugin manager — empty catalog and payloads

| ID | Where | Change | Why | Evo today | Priority |
|----|--------|--------|-----|-----------|----------|
| UI-PLUG-01 | `plugin-manager/plugin-manager.controller.js` | On `pushAvailablePlugins`, normalize `data` to `{ categories: Array.isArray(data?.categories) ? data.categories : [] }`. Set `selectedCategory` only if `categories.length > 0`, else `null`. | Avoids `categories[0]` / `selectedCategory.plugins` when the store is empty. | Evo emits a **placeholder category** `{ name: "evo", prettyName: "", plugins: [] }` so stock UI does not throw. Removes fake category button if UI is fixed. | P2 |
| UI-PLUG-02 | `plugin-manager/plugin-manager.controller.js` | Initialise `availablePlugins = { categories: [] }`, `installedPlugins = []` in constructor. On `pushInstalledPlugins`, use `Array.isArray(data) ? data : []`. | Safe template binding before first socket response. | Partially redundant with Node; still good for slow/failed sockets. | P3 |
| UI-PLUG-03 | `plugin-manager/elements/search-plugin.html` | Guard bindings: e.g. `ng-class` only compares `selectedCategory` when defined; wrap plugin list in `ng-if="pluginManager.selectedCategory"` (or equivalent). | Prevents runtime errors when `categories` is empty. | Same as UI-PLUG-01: allows Evo to send `categories: []`. | P2 |
| UI-PLUG-04 | `plugin-manager/plugin-manager.controller.js` | In `installPlugin` / `updatePlugin` / `showPluginDetails`, return early if `!selectedCategory`. | Defensive when catalog is empty. | Low risk today with Evo placeholder category. | P3 |

---

## 2. Socket.IO event names and orphan emits

| ID | Where | Change | Why | Evo today | Priority |
|----|--------|--------|-----|-----------|----------|
| UI-SOCK-01 | `services/player.service.js` (`initService`) | Emit **`GetTrackInfo`** (capital G) to match `volumio3-backend` websocket and Evo, **or** document and standardise on one name and alias in all backends. | Stock UI emits `getTrackInfo`; Node listens for `GetTrackInfo` — likely dead path; Evo implements `GetTrackInfo` only. | Playback still works via `pushState`; track-info side channel may never fire. | P2 |
| UI-SOCK-02 | `services/player.service.js` | Remove or gate **`getSeek`** emit if no backend implements it; or implement handler in backend. | No matching handler found in volumio3-backend grep; adds noise. | None in Evo. | P3 |
| UI-SOCK-03 | `services/player.service.js` | Remove or feature-gate **`spopUpdateTracklist`** and **`rebuildLibrary`** if unused by MPD path. | SPoP-specific / legacy; no handler in main websocket inventory. | Evo has no handlers. | P3 |
| UI-SOCK-04 | `services/play-queue.service.js` (`addAndPlayList`) | Align event name with backend (**`addPlay`** / **`playItemsList`** / documented contract) or add **`addPlayList`** handler in backends that must support it. | `addPlayList` not registered in `volumio3-backend` `websocket/index.js`; feature may never work on stock Volumio3 either. | Evo has no `addPlayList`. | P2 |

---

## 3. Loading bar: request/response pairing (`socket.service.js`)

| ID | Where | Change | Why | Evo today | Priority |
|----|--------|--------|-----|-----------|----------|
| UI-LBAR-01 | `services/socket.service.js` | For each `loadingBarRequestEvents` entry, ensure a corresponding **`loadingBarResponseEvents`** (or timeout/complete on error). | Wizard emits **`getWirelessNetworks`** (in list); if no **`pushWirelessNetworks`**, bar can stick. | **`getWirelessNetworks`** → **`pushWirelessNetworks`** is implemented ([NETWORK_NM.md](NETWORK_NM.md)). Events that still get **minimal or no-op** responses and can leave the bar hanging on full My Volumio/plugin flows are the **Phase 2 → Outstanding** list in [PORTING.md](PORTING.md) (`getAvailablePlugins`, **`getDeviceActivationStatus`**, updater/TOS rows, etc.). | P2 |
| UI-LBAR-02 | `services/socket.service.js` | Add **`getAvailablePlugins`** / **`pushAvailablePlugins`** to paired lists if plugin manager should show loading state consistently. | Symmetry with other plugin calls. | Optional; plugin manager may complete without bar pairing. | P3 |

**Reference:** `loadingBarRequestEvents` / `loadingBarResponseEvents` in `socket.service.js` (includes `getWirelessNetworks`, `GetTrackInfo`, etc.).

---

## 4. Wizard and first-run (`wizard/wizard.controller.js`)

| ID | Where | Change | Why | Evo today | Priority |
|----|--------|--------|-----|-----------|----------|
| UI-WIZ-01 | `wizard/wizard.controller.js` + templates | When backend is minimal (Evo), **skip** or **short-circuit** steps that require **`getDeviceActivationStatus`**, **`setWizardAction`**, **`setDeviceActivationCode`**, etc., **or** require Evo to stub all wizard emits (see PORTING Phase 2). Wi‑Fi scan/join is partially implemented (NM); cloud activation is not. | Avoids hung wizards and meaningless steps without cloud. | Evo stubs some wizard pushes; **My Volumio / activation** not ported ([PORTING.md](PORTING.md) Part 5). | P1 |
| UI-WIZ-02 | Wizard flow | Detect “offline / Evo” mode via **capability flag** from backend (new REST or `pushSystemInfo`) instead of hard-coding hostnames. | Single code path for multiple backends. | Not implemented. | P2 |

---

## 5. Plugin settings and `callMethod` (`plugin/components/plugin.component.js`)

| ID | Where | Change | Why | Evo today | Priority |
|----|--------|--------|-----|-----------|----------|
| UI-CALL-01 | `plugin/components/plugin.component.js` | Show clear error or hide save when **`callMethod`** / **`getUiConfig`** return empty stubs (Evo) or plugin not present. | Avoid silent no-op saves. | Evo implements **documented** `callMethod` paths only ([PORTING.md](PORTING.md) Part 5 exception row — **`clearAlbumartCache`**, **`installBootBranding`**, system/ALSA/MPD saves, **`metavolumio`** endpoint). | P2 |

---

## 6. Favourites, web radio, My Volumio, updates

These UIs **emit** events documented in PORTING Part 1 / Part 6. Evo does not implement full behaviour.

| ID | Where (examples) | Change | Why | Evo today | Priority |
|----|------------------|--------|-----|-----------|----------|
| UI-FEAT-01 | `services/playlist.service.js` | Disable or hide favourite / web-radio actions when backend reports no capability. | Prevents dead clicks. | No `addToFavourites`, `addWebRadio`, etc. | P2 |
| UI-FEAT-02 | `services/myvolumio/*.js`, templates | Hide My Volumio flows when not supported. | Cloud features cannot work without Volumio cloud (PORTING Part 5). | Not ported in Evo. | P2 |
| UI-FEAT-03 | `services/updater.service.js` | Gate updater UI when `updateCheck` / `update` unsupported. | Avoids broken update UX. | Not ported in Evo. | P2 |

---

## 7. Upload URLs pointing at backend host

| ID | Where | Change | Why | Evo today | Priority |
|----|--------|--------|-----|-----------|----------|
| UI-URL-01 | `plugin-manager/plugin-manager.controller.js` (`uploadPlugin`) | Use **`plugin-upload`** only if backend exposes it; for Evo, document **separate** upload path or disable upload tab. | Evo does not implement `POST /plugin-upload` (PORTING part 1.1). | Upload fails against Evo unless added. | P1 |

---

## Changelog (maintainers)

| Date | Note |
|------|------|
| 2026-04-04 | Initial gap list: plugin manager, socket naming/orphans, loading bar, wizard, callMethod, feature gating, plugin upload URL. |
| 2026-04-16 | Queue **`pushQueue`** shape and per-row **`albumart`** are handled in Evo; see [PORTING.md](PORTING.md) queue contract — no UI fork required for basic queue thumbnails. |

---

## Suggested Evo ↔ UI workflow

1. Prefer **Evo stubs** and minimal payloads so **stock UI** keeps working (see PORTING).
2. When a stub is **misleading** (placeholder plugin category, fake data), add a **UI_GAP** row and consider a **UI fork** change instead of growing Evo workarounds.
3. After each Evo release, skim **Phase 2–6 outstanding** in PORTING and update **Evo today** columns here.
