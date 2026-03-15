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
| **REST quick wins** | done: GET /ping (pong), /getSystemVersion, /getSystemInfo (stubs), /listplaylists (MPD), /search (MPD find), /superSearch (alias), /collectionstats (MPD stats), /getzones (stub), POST /replaceAndPlay (JSON body) |

## Next (by priority)

### 1. REST API gaps (v1)

From `rest_api/index.js`, not yet in Evo:

| Endpoint | Use |
|----------|-----|
| **GET /search** | done: MPD find, browse-like response |
| **GET /superSearch** | done: same as /search (MPD find) |
| **GET /listplaylists** | done: MPD listplaylists |
| **GET /collectionstats** | done: MPD stats (artists, albums, songs, playtime) |
| **GET /getzones** | done: stub `{ zones: [] }` |
| **GET /getSystemVersion** | done: stub |
| **GET /getSystemInfo** | done: stub |
| **GET /ping** | done: returns "pong" |
| **POST /replaceAndPlay** | done: JSON body `{ uri }`, clear + add + play |
| **removeFromQueue** | done (Socket.IO + MPD delete) |

REST v1 gaps listed above are implemented.

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
2) ~~**REST quick wins**~~ Done: ping, getSystemVersion, getSystemInfo (stubs), listplaylists, search, superSearch, collectionstats, getzones (stub), replaceAndPlay.  
3) **Album art** if the UI requires it.  
4) **getInstalledPlugins** real (list WASM) and plugin endpoints when needed.
