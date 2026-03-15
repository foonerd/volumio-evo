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
| **Socket.IO** | done: socketioxide (v4); getState, getQueue, browseLibrary, addToQueue, addPlay, removeFromQueue, volume, play, pause, toggle, stop, next, prev, seek, setRandom, setRepeat, clearQueue, getInstalledPlugins |

## Next (by priority)

### 1. REST API gaps (v1)

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

### 2. Album art / assets

Node serves:

- `/albumart`, `/tinyart/*`, `/albumartd` (proxy to local or MPD/readpicture).

Evo: not implemented. UI may show placeholders or break without. Lower priority if UI tolerates missing art; otherwise add a small route that fetches art (e.g. from MPD or music_root) or returns 404.

### 3. Plugins and config

- **getInstalledPlugins:** Stub is enough for now; real implementation = list WASM plugins from `plugin_dir`.
- **Plugin REST/WebSocket endpoints** (pluginEndpoint, etc.): Defer until plugin ABI is fixed and plugins need HTTP.

### 4. Optional: push state/queue on MPD changes

Socket.IO adapter currently responds to UI requests only. For live updates (e.g. when MPD state changes from another client), add MPD idle or polling and broadcast pushState/pushQueue to connected sockets.

### 5. Other (later)

- HDMIDisplayStandby, OAuth, pushNotificationUrls: Stubs or skip for minimal port.
- Queue reorder (moveItem), playNext: MPD supports move/priority; add when UI needs them.

---

**Summary - suggested order:**  
1) ~~**WebSocket adapter**~~ Done (Socket.IO with socketioxide v4).  
2) **REST:** ping, getSystemVersion, getSystemInfo (stubs), listplaylists, then search/replaceAndPlay (removeFromQueue done via Socket.IO + MPD delete).  
3) **Album art** if the UI requires it.  
4) **getInstalledPlugins** real (list WASM) and plugin endpoints when needed.
