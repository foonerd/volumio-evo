# Porting list: Node backend -> Volumio Evo

What's done vs what's left to port from volumio3-backend so the existing UI and clients work.

## Done (REST / behaviour)

| Area | Status |
|------|--------|
| **getState** | done: MPD status + current song |
| **getQueue** | done: MPD queue |
| **commands** | done: play, pause, toggle, stop, next, prev, volume, seek, repeat, random, clearQueue, **addToQueue**, **addPlay** (GET with query) |
| **browse** | done: Evo layout (local, usb, nas, smb); MPD lsinfo under each source |
| **getInstalledPlugins** | done: Stub (returns `[]`) for UI compatibility |
| **Music layout** | done: Config + music_root (install/first-run, different user) |
| **Health** | done: `/`, `/api/health` |

## Next (by priority)

### 1. WebSocket / real-time (UI expects Socket.IO)

The current UI talks to the Node backend over **Socket.IO** (e.g. `getState`, `getQueue`, `browseLibrary`, `pushState`, `pushBrowseLibrary`). Evo only exposes **REST** so far.

- **Option A:** Add a **WebSocket** server that:
  - Handles the same event names the UI sends (e.g. `getState`, `getQueue`, `browseLibrary`, `addToQueue`, `volume`, ...).
  - Replies with the same payloads (e.g. `pushState`, `pushBrowseLibrary`).
  - Optionally push state/queue when MPD changes (e.g. poll or MPD idle).
- **Option B:** Keep REST-only and adapt the UI to poll `/api/v1/getState`, `/api/v1/getQueue`, and use GET for browse/commands (addToQueue/addPlay already work via GET).

**Suggested next step:** Implement a minimal WebSocket handler that maps the main UI events to existing REST/MPD logic (getState, getQueue, browseLibrary -> browse, addToQueue, addPlay, volume, play, pause, etc.) so the existing UI works without changing the frontend first.

### 2. REST API gaps (v1)

From `rest_api/index.js`, not yet in Evo:

| Endpoint | Use |
|----------|-----|
| **GET /search** | listingSearch - search library |
| **GET /superSearch** | listingSuperSearch |
| **GET /listplaylists** | MPD playlists list |
| **GET /collectionstats** | Stats (e.g. track count) |
| **GET /getzones** | Multi-room / zones (can stay stub) |
| **GET /getSystemVersion** | Version string (stub or from build) |
| **GET /getSystemInfo** | Hostname, etc. (stub) |
| **GET /ping** | Liveness (we have /api/health; alias if UI calls /api/v1/ping) |
| **POST /replaceAndPlay** | Clear, add URI, play (we have addPlay; replaceAndPlay may take list) |
| **removeFromQueue** | MPD delete position (REST + WS) |

Quick wins: **ping** (alias), **getSystemVersion** / **getSystemInfo** (stubs), **listplaylists** (MPD listplaylists), **search** (MPD search if MPD supports it).

### 3. Album art / assets

Node serves:

- `/albumart`, `/tinyart/*`, `/albumartd` (proxy to local or MPD/readpicture).

Evo: not implemented. UI may show placeholders or break without. Lower priority if UI tolerates missing art; otherwise add a small route that fetches art (e.g. from MPD or music_root) or returns 404.

### 4. Plugins and config

- **getInstalledPlugins:** Stub is enough for now; real implementation = list WASM plugins from `plugin_dir`.
- **Plugin REST/WebSocket endpoints** (pluginEndpoint, etc.): Defer until plugin ABI is fixed and plugins need HTTP.

### 5. Other (later)

- HDMIDisplayStandby, OAuth, pushNotificationUrls: Stubs or skip for minimal port.
- Queue reorder (moveItem), playNext: MPD supports move/priority; add when UI needs them.

---

**Summary - suggested order:**  
1) **WebSocket adapter** for getState / getQueue / browseLibrary / addToQueue / addPlay / volume / transport so the current UI works.  
2) **REST:** ping, getSystemVersion, getSystemInfo (stubs), listplaylists, then search/replaceAndPlay/removeFromQueue.  
3) **Album art** if the UI requires it.  
4) **getInstalledPlugins** real (list WASM) and plugin endpoints when needed.
