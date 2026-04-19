# Porting: Volumio backend to Evo

This document (1) lists **all current Volumio (volumio3-backend) functionality**, (2) summarizes **what Evo already covers**, (3) records **decisions**, (4) sets **outstanding work by phase** (core, UI compatibility, networking, plugins, system, other), and (5) states **what cannot be ported and why**.

Reference codebase: **volumio3-backend** (Node/Express). Volumio4, if present elsewhere, can be added to the inventory later.

### UI layout (Manifest / Contemporary / Classic)

On stock Volumio, **`volumio/volumioUisList.json`** defines **`uiName`** + **`uiPath`** for each layout; **`miscellanea/appearance`** persists the user’s choice and the system serves that static tree.

**Evo (Rust):** **`[ui] active_layout`** in **`/etc/volumio-evo/config.toml`** (or env **`VOLUMIO_EVO_ACTIVE_LAYOUT`**) holds the same logical name: **`manifest`**, **`contemporary`**, or **`classic`**. It is exposed in **`pushUiSettings`** (`active_layout`) and **`GET /api/v1/getActiveUi`**. A copy of the stock list (paths are reference-only on full OS) lives at **`layer/config/volumioUisList.reference.json`**.

**Bootstrap / nginx:** **`scripts/bootstrap-volumio-evo-player.sh`** reads **`[ui] active_layout`**, sets nginx **`root`** to the matching **`UI_ROOT_*`** (defaults under **`/srv/...`**). It installs static UI from **`layer/web/`** (three trees) or **`UI_DIST_SOURCE`** (one tree copied to all roots). No npm/gulp on device. Switching layout is a config edit plus **`sudo … --apply-ui-only`** (or a full re-bootstrap). Override the served tree with **`UI_DIST_OVERRIDE`**.

**Backend URL / IP changes:** The stock UI calls **`GET /api/host`** for the Socket.IO base URL. Evo implements this (live IPv4 discovery; prefers the address that matches the HTTP **`Host`** header when it is a local interface). Nginx proxies **`/api/host`** to Evo; **`app/local-config.json`** is only a fallback when that request fails.

**Socket.IO protocol (naming is confusing):** In **`socketioxide`**, the Cargo feature **`v4`** does **not** mean “only Socket.IO JS v4 clients.” It enables the **older** wire stack: **Engine.IO v3**, which is what **socket.io-client 1.x / 2.x** (stock **Volumio2-UI**) uses. The **default** `socketioxide` build (without that feature) speaks **protocol v5** / Engine.IO v4 and targets **socket.io-client 3.x+** only.

**Evo already “downgrades” the server for the stock UI:** **`crates/core/Cargo.toml`** enables **`socketioxide`** with **`features = ["state", "v4"]`** (which pulls in **`engineioxide/v3`**). Do **not** remove **`v4`** unless you also ship a UI built with **socket.io-client ≥ 3**.

If the UI still spins: check the browser console (**`connect_error`**), that **`GET /api/host`** returns a reachable **`http://<ip>:3000`**, and that nothing blocks port **3000** from the client. The default bootstrap does **not** proxy **`/socket.io`** through nginx; the socket goes to Evo on **3000** directly.

**Not done yet in Evo (parity with stock Node UI):** in-browser **`setActiveUI`** / **`reloadUi()`** wired to Evo’s **`active_layout`** (today the value is API/config-driven; the stock UI may still expect Node behaviour).

---

## Part 1: Volumio backend inventory

### 1.1 HTTP routes (root level)

| Route | Method | Handler / purpose |
|-------|--------|-------------------|
| `/` | GET | Static UI (VOLUMIO_ACTIVE_UI_PATH) or wizard |
| `/albumart` | GET | Album art: query `web`, `path`, `metadata`, `icon`, `sourceicon`, `sectionimage`; local cache + exiftool + Volumio meta + Last.fm; default image on failure |
| `/tinyart/*` | GET | Tiny art variant; same sources, different response |
| `/albumartd` | GET | Direct album art; same params, sendTinyArt response |
| `/api` | (mounted) | REST API (see 1.2) |
| `/dev` | (mounted) | Dev UI (EJS views) |
| `/plugin-serve` | (static) | Serve plugin files (e.g. from /tmp/plugins) |
| `/stream` | (static) | HLS stream from /tmp/hls |
| `/partnerlogo` | (static) | Partner logo image |
| `/status` | GET | Returns `process.env.VOLUMIO_SYSTEM_STATUS` |
| `/plugin-upload` | POST | Multipart: upload plugin zip, then Socket.IO installPlugin |
| `/backgrounds-upload` | POST | Multipart: upload background image to /data/backgrounds |
| `/albumart-upload` | POST | Multipart: artist, album, filePath; custom album art to /data/albumart/personal |

### 1.2 REST API (`/api`)

From `http/restapi.js`: router mounted at `/api`, then `api.use('/api', api)` and `api.use('/v1', api)` (user_interface/rest_api). So v1 routes live at **`/api/v1/...`** (path may be /api/api/v1 depending on app mount; typically exposed as /api/v1).

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/` | GET | Welcome message |
| `/host` | GET | Get host IP(s) (ifconfig) |
| `/host` | POST | Set primary host |
| **v1** | | |
| `/v1/browse` | GET | Browse listing (uri) |
| `/v1/search` | GET | listingSearch (query) |
| `/v1/superSearch` | GET | listingSuperSearch (query) |
| `/v1/listplaylists` | GET | MPD listplaylists |
| `/v1/collectionstats` | GET | Collection stats (artists, albums, songs, playtime) |
| `/v1/getzones` | GET | Multi-room zones |
| `/v1/ping` | GET | Liveness |
| `/v1/getSystemVersion` | GET | System version info |
| `/v1/getSystemInfo` | GET | System info (hostname, etc.) |
| `/v1/getInstalledPlugins` | GET | Installed plugins list |
| `/v1/enableHDMIDisplayStandby` | GET | Enable HDMI standby |
| `/v1/disableHDMIDisplayStandby` | GET | Disable HDMI standby |
| `/v1/commands` | GET | Playback commands (cmd, volume, position, value, N) |
| `/v1/getState` | GET | Playback state + current track |
| `/v1/getQueue` | GET | Queue |
| `/v1/addToQueue` | POST | Add to queue |
| `/v1/addPlay` | POST | Add and play |
| `/v1/replaceAndPlay` | POST | Clear, add URI, play |
| `/v1/pluginEndpoint` | POST | Plugin REST: **`endpoint: "metavolumio"`** for browse story/credits (see §2 table). Node’s other plugin endpoints not implemented. **GET** not implemented. |
| `/v1/pushNotificationUrls` | GET | Get push notification URLs |
| `/v1/pushNotificationUrls` | POST | Add push notification URL |
| `/v1/pushNotificationUrls` | DELETE | Remove push notification URL |
| `/v1/oauth` | GET | OAuth flow |

### 1.3 Socket.IO (default namespace)

**Connection:** `closeAllModals` emitted on connect.

**Events (client -> server) – grouped by area:**

- **Playback:** getState, getQueue, removeQueueItem, addQueueUids, addToQueue, playNext, replaceAndPlay, replaceAndPlayCue, addPlay, playItemsList, addPlayCue, removeFromQueue, seek, play, volatilePlay, pause, toggle, stop, clearQueue, prev, next, setRandom, setRepeat, skipBackwards, skipForward, volume, mute, unmute.
- **Browse / library:** getLibraryListing, getLibraryFilters, getPlaylistIndex, browseLibrary, getInputSources, search, superSearch, goTo, GetTrackInfo, getLastPushedBrowseLibrary.
- **Playlists (MPD / manager):** getPlaylistContent, createPlaylist, deletePlaylist, listPlaylist, addToPlaylist, removeFromPlaylist, playPlaylist, enqueue, saveQueueToPlaylist, setConsume, moveQueue.
- **Favourites:** addToFavourites, removeFromFavourites, playFavourites; addToRadioFavourites, removeFromRadioFavourites, playRadioFavourites.
- **System / device:** getDeviceInfo, getSystemVersion, getSystemInfo, getMenuItems, getUiConfig, getDSPUiConfig, shutdown, standby, reboot, getDeviceName, setDeviceName, getDeviceHWUUID, getShutdownOrStandbyMode.
- **Multiroom / zones:** getMultiRoomDevices, getMultiroom, setMultiroom, writeMultiroom, receiveMultiroomDeviceUpdate, setAsMultiroomSingle, setAsMultiroomServer, setAsMultiroomClient, getBrowseSources.
- **Network:** getWirelessNetworks, getWirelessNetworksCache, saveWirelessNetworkSettings, getInfoNetwork, connectWirelessNetworkWizard.
- **Plugins:** callMethod (generic plugin endpoint), getInstalledPlugins, getAvailablePlugins, getPluginDetails, pluginManager, installPlugin, updatePlugin, unInstallPlugin, enablePlugin, disablePlugin, modifyPluginStatus, preUninstallPlugin.
- **Library / MPD:** getMyCollectionStats, rescanDb, updateDb, deleteFolder.
- **Shares / storage:** addShare, deleteShare, getListShares, getInfoShare, editShare, listUsbDrives, safeRemoveDrive.
- **Audio outputs:** getAudioOutputs, enableAudioOutput, disableAudioOutput, setAudioOutputVolume, audioOutputPlay, audioOutputPause, getExtendedOutputDevices, getOutputDevices, setOutputDevices.
- **UI / appearance:** getUiSettings, getBackgrounds, setBackgrounds, deleteBackground, regenerateThumbnails, getAvailableLanguages, setLanguage, getAvailableTimezones, getCurrentTimezone, setTimezone, getExperienceAdvancedSettings, setExperienceAdvancedSettings.
- **Wizard / onboarding:** getOnboardingWizard, setOnboardingWizardFalse, getWizard, runFirstConfigWizard, getWizardSteps, setWizardAction, getWizardUiConfig, getDonePage.
- **Alarm / sleep:** getSleep, setSleep, getAlarms, saveAlarm.
- **Web radio:** addWebRadio, removeWebRadio.
- **Backup / restore:** manageBackup, getBackup, restoreConfig.
- **Updates:** updateCheck, updateCheckCache, ClientUpdateReady, update, getAutomaticUpdateEnabled, getUpdaterChannel, setUpdaterChannel.
- **Factory / user data:** deleteUserData, factoryReset.
- **My Volumio / cloud:** setDeviceActivationCode, getDeviceActivationStatus, getMyVolumioStatus, getMyVolumioToken, setMyVolumioToken, myVolumioLogout, enableMyVolumioDevice, disableMyVolumioDevice, deleteMyVolumioDevice, getMyMusicPlugins, enableDisableMyMusicPlugin.
- **Other:** initSocket (volumiodiscovery), pinger (-> ponger), closeModals, checkPassword, getPrivacySettings, setTOSAccepted, isLatestTOSAccepted, getInfinityPlayback, setInfinityPlayback, installToDisk.

**Events (server -> client):** pushState, pushQueue, pushBrowseLibrary, pushLibraryFilters, pushLibraryListing, pushPlaylistIndex, pushMultiRoomDevices, pushMenuItems, pushUiConfig, pushDSPUiConfig, pushBrowseSources, pushInputSources, pushInstalledPlugins, pushAvailablePlugins, pushToastMessage, openModal, closeAllModals, pushDeviceInfo, pushSystemVersion, pushSystemInfo, pushMyCollectionStats, pushListPlaylist, pushPlaylistContent, pushSaveQueueToPlaylist, pushSetConsume, pushAudioOutputs, pushUiSettings, pushBackgrounds, pushWirelessNetworks, pushInfoNetwork, pushSleep, pushAlarm, pushMultiroom, pushListShares, pushInstallPlugin, pushUnInstallPlugin, pushEnablePlugin, pushDisablePlugin, pushModifyPluginStatus, pushWizard, pushWizardSteps, pushDonePage, pushDeviceActivationCodeResult, pushDeviceActivationStatus, pushMyVolumioStatus, pushMyVolumioToken, pushMyMusicPlugins, pushExtendedOutputDevices, pushOutputDevices, pushDeviceName, pushDeviceHWUUID, pushPrivacySettings, pushLatestTOSAccepted, pushInfinityPlayback, pushShutdownOrStandbyMode, pushUpdaterChannel, updateWaitMsg, updateReadyCache, updateReady, updateProgress, ponger, and others.

### 1.4 Album art behaviour (summary)

- **Sources (in order):** path on disk (cover files in folder, or exiftool embedded picture) -> `/data/albumart` cache (folder, metadata, web, personal) -> online (Volumio: meta for artist, Last.fm for album; Evo plan: multiple providers — see [ALBUMART_PROVIDERS.md](ALBUMART_PROVIDERS.md)) -> icon/sectionimage/sourceicon from plugins -> default image.
- **Params:** `web` (artist/album/resolution), `path` (file/folder), `metadata=true`, `icon`, `sourceicon`, `sectionimage`.

### 1.5 Plugins and extensibility

- **Plugin types:** system_controller, music_service, audio_interface, miscellanea (and others). Each plugin can register REST endpoints and Socket.IO is wired via callMethod(endpoint, method, data).
- **Plugin lifecycle:** install, update, uninstall, enable, disable, getInstalledPlugins, getAvailablePlugins (from store/cloud), getPluginDetails.

---

## Part 2: What is already covered in Evo

Quick reference: what exists in Evo today. Details and gaps are in 2.1–2.3 below.

**Terminology used in this doc:**

| Term | Meaning |
|------|--------|
| **Implemented** | Evo has real behaviour (e.g. MPD for playback, real browse, real playlists). |
| **Minimal response** | Evo emits the same event/response name with empty or fixed payload (e.g. `[]`, `{}`) so the existing UI gets a valid response and does not hang. No real feature behind it. |
| **No-op** | Handler registered; it accepts the payload but does nothing (optionally emits a minimal response). Used when the UI sends a command that has no Evo equivalent. |
| **Deferred** | Not implemented yet; design or dependency (e.g. plugin ABI) not decided. May be implemented in a later phase. |
| **Not ported** | Not implemented; could be added in a later phase if the product requires it. |
| **Cannot be ported** | Fundamentally incompatible with Evo’s architecture or stack; would require a different design or service (see Part 5). |

### HTTP routes (root)

| Volumio route | Covered? | Evo behaviour |
|---------------|----------|----------------|
| `GET /` | Yes | Returns "ok" (health; UI served elsewhere or same host) |
| `GET /api/health` | Yes | Returns "ok" |
| `GET /albumart` | Yes | Query path/web/metadata; resolve path → folder cache → metadata cache → folder covers → personal → default |
| `GET /tinyart/*` | Yes | Same resolution; URL path used as web when query web absent |
| `GET /albumartd` | Yes | Same resolution as /albumart |
| `/api` (REST) | Yes | Mounted; v1 under `/api/v1` |
| `/dev`, `/plugin-serve`, `/stream`, `/partnerlogo` | No | Not implemented |
| `GET /status` | Yes | Returns VOLUMIO_SYSTEM_STATUS env or "ready" |
| `POST /albumart-upload` | Yes | Multipart artist, album (optional), file → personal/album or personal/artist; 1MB max; JSON { path } |
| `POST /plugin-upload`, `/backgrounds-upload` | No | Not implemented |

### REST API `/api/v1/`

| Endpoint | Covered? | Evo behaviour |
|----------|----------|----------------|
| `GET /v1/browse` | Yes | Root `music-library`: **INTERNAL**, **USB**, **NAS**, **SMB** only (each with `albumart` via bundled SVGs: `icon=microchip` / `usb` / `server` / `server`, same as Node `getAlbumArt`); Favourites / tag library / playlists are sidebar `browseSources`, not duplicated here. Sub-URIs: MPD `lsinfo` — directory rows use Node types **`internal-folder`**, **`remdisk`**, or **`folder`**, with **`albumart`** (`/albumart?path=...&icon=…`, `folder-o` for normal folders, same icons at top-level storage roots); songs include `albumart` with `icon=music`. Virtual `artists://`, `albums://`, `genres://`, `favourites`, `playlists` when browsing those URIs — **`artists://`** lists **`AlbumArtist`** (Node `artistsort`), merges case/whitespace variants, falls back to **`Artist`** if no album-artist tags; **`genres://&lt;genre&gt;`** artist column uses **`AlbumArtist`** when present |
| `GET /v1/search` | Yes | MPD find, browse-like response |
| `GET /v1/superSearch` | Yes | Same as search |
| `GET /v1/listplaylists` | Yes | MPD listplaylists |
| `GET /v1/collectionstats` | Yes | MPD stats (artists, albums, songs, playtime) |
| `GET /v1/getzones` | Yes (stub) | `{ zones: [] }` |
| `GET /v1/ping` | Yes | "pong" |
| `GET /v1/getSystemVersion` | Yes (stub) | Fixed JSON (systemversion, variant, hardware, os, builddate) |
| `GET /v1/getSystemInfo` | Yes (stub) | Stub JSON (+ hostname, hwUuid) |
| `GET /v1/getInstalledPlugins` | Yes | List .wasm in plugin_dir, array of `{ name }` |
| `GET /v1/commands` | Yes | play, pause, toggle, stop, next, prev, volume, seek, repeat, random, clearQueue, addToQueue, addPlay (GET query) |
| `GET /v1/getState` | Yes | MPD status + current song |
| `GET /v1/getQueue` | Yes | MPD queue |
| `POST /v1/replaceAndPlay` | Yes | JSON `{ uri }`, clear + add + play |
| `/v1/enableHDMIDisplayStandby`, `/v1/disableHDMIDisplayStandby` | No | Not implemented |
| `POST /v1/pluginEndpoint` | Yes | JSON `{ "endpoint": "metavolumio", "data": { … } }` for browse album/artist story and credits (modes e.g. `storyAlbum`, `storyArtist`, `creditsAlbum`). Implemented in Rust (`metavolumio.rs`): Last.fm (wiki or tags fallback), MusicBrainz, English Wikipedia/Wikidata; optional `VOLUMIO_EVO_LASTFM_API_KEY` / `[albumart_providers] lastfm_api_key`. **GET** not implemented. |
| `GET/POST/DELETE /v1/pushNotificationUrls` | No | Not implemented |
| `GET /v1/oauth` | No | Not implemented |
| `GET /api/host` | Yes | JSON base URL(s) for Socket.IO + REST (see introduction above). |
| `GET /api` (welcome, if distinct from `/`) | No | Not implemented as separate route; **`GET /`** returns **`ok`**. |

### Socket.IO events (client -> server)

| Event(s) | Covered? | Evo behaviour |
|----------|----------|----------------|
| closeAllModals (on connect), closeModals | Yes | closeAllModals on connect; closeModals -> emit closeAllModals to client. |
| getState | Yes | MPD state -> pushState |
| getQueue | Yes | MPD queue → **`pushQueue` with array payload** (not wrapped); see **Queue and pushQueue** below |
| browseLibrary, getInputSources, getBrowseSources | Yes | browseLibrary: same root as REST (storage roots + `albumart`); virtual URIs `artists://`, `albums://`, `genres://`, `favourites`, `playlists`; getBrowseSources -> sidebar entries (Favourites, Playlists, Music Library, …). |
| addToQueue, addPlay, addQueueUids | Yes | MPD add + optional play; addQueueUids adds multiple URIs (payload: array or { uids }). |
| removeFromQueue, removeQueueItem | Yes | MPD delete (position); both use payload { value } (1-based from UI). |
| volume, play, pause, toggle, stop, next, prev, seek, skipBackwards, skipForward | Yes | MPD commands; skipBackwards/skipForward seek ±10s within current track. |
| setRandom, setRepeat, clearQueue | Yes | MPD |
| getInstalledPlugins | Yes | List .wasm -> pushInstalledPlugins |
| moveQueue, playNext | Yes | MPD move / add after current; pushQueue (and pushState for playNext) |
| getPlaylistContent, listPlaylist, playPlaylist, saveQueueToPlaylist, createPlaylist, deletePlaylist, addToPlaylist, removeFromPlaylist, enqueue | Yes | JSON/favourites: append to playlist JSON on disk; MPD-only playlists: **`playlistadd`** via **`add_to_playlist_resolved`** (virtual **`artists://` / `albums://` / `genres://`** expanded to files first — see **MPD stored playlists and addToPlaylist** below). Same MPD playlist commands otherwise; pushes as in Node |
| GetTrackInfo | Yes | Echo payload -> pushGetTrackInfo |
| callMethod (miscellanea/albumart clearAlbumartCache) | Yes | Triggers broadcast of callMethod to all clients (so UI can refresh); also broadcast after POST /albumart-upload |
| callMethod (**system_controller/system**) | Partial | **save*** settings (general, locale, kiosk, updates, privacy), **`installBootBranding`** (boot installer — [BRANDED_BOOT.md](BRANDED_BOOT.md)); ALSA/MPD paths under **audio_interface/** and **music_service/** — see `socketio.rs` |
| pinger | Yes | Echo payload -> ponger (connection liveness) |
| setConsume | Yes | MPD consume mode -> pushSetConsume({ value }) |
| getLastPushedBrowseLibrary | Yes | Emit last pushBrowseLibrary payload (stored on each browse) |
| mute, unmute | Yes | mute -> volume 0; unmute -> volume 80 |
| rescanDb, updateDb | Yes | MPD rescan / update (optional path for updateDb) |
| replaceAndPlay | Yes | Clear queue, add uri and play; if uri is playlists/Name then load playlist and play |
| goTo | Yes | type=artist -> pushBrowseLibrary(albums by artist); type=album -> pushBrowseLibrary(songs in album). Uses artists:// and albums:// browse. |
| replaceAndPlayCue | Yes | Clear queue, add single uri (no play). No CUE sheet support; treated as single track. |
| addPlayCue | Yes | Add single uri to queue. No CUE sheet support. |
| playItemsList | Yes | With `list` + `index`: clear queue, add uris, play at index (songs). With **`item` only** (Node folder inline play): same as `replaceAndPlay` on `item.uri` (MPD `add` expands directories). |
| search, superSearch | Yes | MPD find (any); payload value or query -> pushBrowseLibrary. Same behaviour for both. |
| getMyCollectionStats | Yes | MPD stats -> pushMyCollectionStats (artists, albums, songs, playtime). |
| getDeviceInfo | Yes | pushDeviceInfo({ uuid, name }) — stub: uuid "evo-stub", name "Volumio Evo". |
| getSystemVersion, getSystemInfo | Yes | pushSystemVersion / pushSystemInfo — stub (systemversion 4.0, variant volumio-evo, hardware generic; getSystemInfo adds hostname, hwUuid). |
| getMenuItems | Yes | pushMenuItems — minimal stub (browse, mymusic entries; same shape as Node mainmenu.json). |
| getUiConfig, getDSPUiConfig | Yes | pushUiConfig / pushDSPUiConfig — empty stub { page: { label: '' }, sections: [] } (no plugin configs in Evo). |
| getAvailableLanguages | Yes | pushAvailableLanguages — stub (defaultLanguage + available: English only). |
| getDeviceName | Yes | pushDeviceName({ name }) — stub name "Volumio Evo". |
| setLanguage | Yes | No-op (no language persistence in Evo). |
| getAvailableTimezones, getCurrentTimezone, setTimezone | Yes | pushAvailableTimezones / pushCurrentTimezone — stub (UTC only); setTimezone no-op. |
| initSocket | Yes | No-op (Node: volumiodiscovery plugin). |
| volatilePlay | Yes | Same as play: MPD play with optional position (payload.value). |
| getLibraryListing, getLibraryFilters, getPlaylistIndex | Yes (stub) | pushLibraryListing: minimal `{ name, type, children: [] }`; pushLibraryFilters / pushPlaylistIndex: `[]` (Node uses musicLibrary / playlistFS index; Evo has no library DB). |
| getMultiRoomDevices | Yes (stub) | pushMultiRoomDevices: `{ misc, list: [] }` (Node: volumiodiscovery.getDevices; UI reads `data.list`). |
| serviceUpdateTracklist, updateAllMetadata, importServicePlaylists | Yes (no-op) | No-op (Node: plugin rebuildTracklist / updateAllMetadata / importServicePlaylists; Evo single MPD, no library DB). |
| setDeviceName, getDeviceHWUUID | Yes | setDeviceName no-op; getDeviceHWUUID -> pushDeviceHWUUID stub "evo-stub". |
| getUiSettings, getShutdownOrStandbyMode | Yes (stub) | pushUiSettings: `{}`; pushShutdownOrStandbyMode: `{}` (Node: appearance plugin / commandRouter). |
| getPrivacySettings, getInfinityPlayback, setInfinityPlayback | Yes (stub/no-op) | pushPrivacySettings: `{}`; pushInfinityPlayback: `{ enabled: false }`; setInfinityPlayback no-op. |
| getSleep, setSleep, getAlarms, saveAlarm | Yes (stub) | pushSleep: `{}`; pushAlarm: `[]` (Node: alarm-clock plugin; Evo has no sleep/alarms). |
| getMultiroom, setMultiroom, writeMultiroom | Yes (stub/no-op) | getMultiroom/setMultiroom -> pushMultiroom `{}`; writeMultiroom no-op. |
| getExtendedOutputDevices, getOutputDevices | Yes (stub) | pushExtendedOutputDevices / pushOutputDevices: `[]` (Node: alsa_controller). |
| getBackgrounds, setBackgrounds | Yes (stub) | getBackgrounds -> pushBackgrounds `[]`; setBackgrounds -> pushBackgrounds `[]` (Node: appearance). |
| getExperienceAdvancedSettings, setExperienceAdvancedSettings | Yes (stub/no-op) | getExperienceAdvancedSettings -> pushExperienceAdvancedSettings `{}`; setExperienceAdvancedSettings no-op. |
| setOutputDevices | Yes (no-op) | No-op (Node: alsa_controller saveAlsaOptions; Evo has no ALSA device config). |
| getDonePage, getWizard, getWizardSteps, getWizardUiConfig | Yes (stub) | pushDonePage: minimal object (congratulations, title, message, donation, donationAmount); pushWizard: `{ openWizard: false }`; pushWizardSteps: `[]`; pushWizardUiConfig: `{}` (Node: wizard plugin). |
| deleteBackground | Yes (no-op) | No-op (Node: appearance deleteBackgrounds; Evo has no backgrounds). |
| Other Socket.IO events | Partial / No | Anything not listed above: see **Part 4** (outstanding by phase). |

### Other

| Area | Covered? | Evo behaviour |
|------|----------|----------------|
| Music layout | Yes | music_root, INTERNAL/USB/NAS/SMB dir names, config + env, MPD music_directory |
| Album art resolution (path, cache, personal, MPD readpicture, exiftool, online, icon, resize) | Yes | path → folder/metadata cache → folder covers → personal → MPD readpicture (path param) → exiftool (metadata=true) → web cache → online → icon/sectionimage/sourceicon → default; albumartd 500px, tinyart 250px |

### Queue and `pushQueue` / `getQueue` (Volumio2-UI contract)

The stock UI treats **Socket.IO `pushQueue` payloads as a raw array** (`play-queue.service.js` assigns `this._queue = data` and uses `data.length`). **Node** emits `io.emit('pushQueue', queueArray)`. Evo matches that: **`pushQueue` sends the array only**, not `{ queue: [...] }`.

**REST** differs on purpose: **`GET /api/v1/getQueue`** returns **`{ "queue": [ … ] }`**, same as Node’s REST handler.

Each queue element mirrors Node’s play-queue shape where it matters for the UI:

| Field | Evo notes |
|-------|-----------|
| `name`, `title` | Both set from the MPD title when present (controller renders `name`). |
| `uri` | Volumio form **`music-library/...`** (from MPD file URL + `music_root`), aligned with browse and **`/albumart?path=`**. |
| `albumart` | Full query string to **`GET /albumart`** (e.g. `metadata=true`, `path=music-library/...`, optional `web=artist/album/extralarge` from tags) — same builder as **`pushState.albumart`** for the current track so row thumbnails and background themes resolve like Node. |
| `service` | Always **`mpd`** in Evo. |

### MPD stored playlists and `addToPlaylist` (do not regress)

The UI sends **the same `uri` field** whether the user picked a row under **Music Library** (filesystem) or under **Artists / Albums / Genres** (tag library). Those are **not** the same kind of string:

| Browse source | Typical item `uri` | Valid for raw MPD `playlistadd`? |
|---------------|----------------------|-----------------------------------|
| Music Library → storage → files | `music-library/INTERNAL/.../file.flac` → strip prefix → path under `music_directory` | **Yes** |
| Tag library rows (folders) | `artists://…`, `albums://…`, `genres://…` | **No** — MPD returns **`Unsupported URI scheme`** if passed verbatim |

**Rule:** Anything that ends up as MPD **`playlistadd`** must use a **concrete library path** (or one **`playlistadd` per file**), not a virtual browse URI.

**Implementation (Evo):**

- **`addToPlaylist`** (`socketio.rs`): if the target is **JSON** (file under `settings/playlist/`) or **`favourites`**, entries are stored via **`playlist_library::add_to_json_playlist`** — URIs are saved **as strings** (including virtual ones); MPD is not used for that append.
- If the target is an **MPD-only** stored playlist (no JSON file for that name), Evo calls **`mpd::add_to_playlist_resolved`**, which runs **`resolve_uri_for_queue`** (`mpd.rs`) to expand **`albums://`**, **`artists://`**, and **`genres://`** into **`music-library/...`** song URIs, then issues **`playlistadd`** once per file (or **`add_to_playlist_connected`** when a single path remains).

**Play stored playlist (`playPlaylist`, **`replaceAndPlay`** with **`playlists/Name`):** JSON/favourites content uses **`play_items_list_connected`** (**`clear`** then add URIs). The MPD fallback uses **`load`**, which **appends** to the queue in MPD unless the queue was cleared first — **`load_playlist_connected`** must **`clear`** before **`load`** so “clear and play” replaces the queue.

**If you change playlist add behaviour:** never route virtual tag URIs straight to **`add_to_playlist_connected`** / **`playlistadd`**. Queue operations already used **`resolve_uri_for_queue`** for replace/add; playlist add must stay aligned.

---

## Part 3: Evo port status and decisions

Using the inventory above, we decide what to implement, stub, or defer so the existing UI and clients work against Evo.

### 3.1 Implemented in Evo (summary)

- **Health / UI bootstrap:** `GET /`, `GET /api/health` -> "ok"; **`GET /api/host`** -> JSON for Socket.IO base URL (matches stock UI).
- **REST v1 (core):** getState, getQueue, commands (play, pause, toggle, stop, next, prev, volume, seek, repeat, random, clearQueue, addToQueue, addPlay), replaceAndPlay (POST), browse, listplaylists, search, superSearch, collectionstats, getzones (stub), ping, getSystemVersion, getSystemInfo (stubs), getInstalledPlugins (list .wasm), **POST pluginEndpoint** (metavolumio: album/artist story + credits via Last.fm / MusicBrainz / Wikipedia).
- **Socket.IO (core):** getState, getQueue, browseLibrary (including uri `playlists` and `playlists/<name>` for stored playlists; virtual `artists://<name>` and `albums://<artist>/<album>` for goTo), addToQueue, addPlay, removeFromQueue, volume, play, pause, toggle, stop, next, prev, seek, setRandom, setRepeat, clearQueue, getInstalledPlugins, moveQueue, playNext; playlist manager: getPlaylistContent, listPlaylist, playPlaylist, saveQueueToPlaylist, createPlaylist, deletePlaylist, addToPlaylist, removeFromPlaylist, enqueue; pinger (-> ponger), setConsume (-> pushSetConsume), getLastPushedBrowseLibrary (-> pushBrowseLibrary with last result), mute, unmute, rescanDb, updateDb; replaceAndPlay (uri or playlists/Name -> clear+add+play or load playlist+play), goTo (type=artist|album -> pushBrowseLibrary), replaceAndPlayCue (clear+add uri, no play), addPlayCue (add uri to queue), playItemsList (clear+add list+play at index), search, superSearch (MPD find -> pushBrowseLibrary), getMyCollectionStats (-> pushMyCollectionStats), removeQueueItem (same as removeFromQueue), addQueueUids (add multiple URIs), skipBackwards, skipForward (seek ±10s), closeModals (-> closeAllModals), getInputSources (-> pushInputSources), getDeviceInfo, getBrowseSources, getSystemVersion, getSystemInfo, getMenuItems (-> pushMenuItems), getUiConfig (-> pushUiConfig), getDSPUiConfig (-> pushDSPUiConfig), getAvailableLanguages (-> pushAvailableLanguages), getDeviceName (-> pushDeviceName), setLanguage (no-op), getAvailableTimezones (-> pushAvailableTimezones), getCurrentTimezone (-> pushCurrentTimezone), setTimezone (no-op), initSocket (no-op), volatilePlay (same as play), getLibraryListing (-> pushLibraryListing stub), getLibraryFilters (-> pushLibraryFilters []), getPlaylistIndex (-> pushPlaylistIndex []), getMultiRoomDevices (-> pushMultiRoomDevices []), serviceUpdateTracklist, updateAllMetadata, importServicePlaylists (no-op), setDeviceName (no-op), getDeviceHWUUID (-> pushDeviceHWUUID stub), getUiSettings (-> pushUiSettings {}), getShutdownOrStandbyMode (-> pushShutdownOrStandbyMode {}), getPrivacySettings (-> pushPrivacySettings {}), getInfinityPlayback (-> pushInfinityPlayback), setInfinityPlayback (no-op), getSleep/setSleep (-> pushSleep {}), getAlarms (-> pushAlarm []), saveAlarm (-> pushSleep {}), getMultiroom/setMultiroom (-> pushMultiroom {}), writeMultiroom (no-op), getExtendedOutputDevices/getOutputDevices (-> pushExtendedOutputDevices/pushOutputDevices []), getBackgrounds (-> pushBackgrounds []), setBackgrounds (-> pushBackgrounds []), getExperienceAdvancedSettings (-> pushExperienceAdvancedSettings {}), setExperienceAdvancedSettings (no-op), setOutputDevices (no-op), getDonePage (-> pushDonePage stub), getWizard (-> pushWizard { openWizard: false }), getWizardSteps (-> pushWizardSteps []), getWizardUiConfig (-> pushWizardUiConfig {}), deleteBackground (no-op); closeAllModals on connect; responses: pushState, pushQueue, pushBrowseLibrary (and last stored for getLastPushedBrowseLibrary), pushInstalledPlugins, pushPlaylistContent, pushListPlaylist, pushPlayPlaylist, pushSaveQueueToPlaylist, pushCreatePlaylist, pushAddToPlaylist, pushEnqueue, pushSetConsume, ponger, pushDeviceInfo, pushBrowseSources, pushSystemVersion, pushSystemInfo, pushMenuItems, pushUiConfig, pushDSPUiConfig, pushAvailableLanguages, pushDeviceName, pushAvailableTimezones, pushCurrentTimezone, pushLibraryListing, pushLibraryFilters, pushPlaylistIndex, pushMultiRoomDevices, pushDeviceHWUUID, pushUiSettings, pushShutdownOrStandbyMode, pushPrivacySettings, pushInfinityPlayback, pushSleep, pushAlarm, pushMultiroom, pushExtendedOutputDevices, pushOutputDevices, pushBackgrounds, pushExperienceAdvancedSettings, pushDonePage, pushWizard, pushWizardSteps, pushWizardUiConfig. Background polling (2s) broadcasts pushState/pushQueue to all clients when MPD state or queue changes.
- **Album art routes:** GET /albumart, /albumartd, /tinyart/*; query path/web/metadata/icon/sourceicon/sectionimage; resolution: path → folder/metadata cache → folder covers → personal → MPD readpicture → exiftool (metadata=true) → web cache → online providers → icon/sectionimage/sourceicon from plugin dirs → default. albumartd/tinyart resized (500px/250px). POST /albumart-upload (multipart → personal); after upload, broadcast callMethod(clearAlbumartCache) to all Socket.IO clients.
- **Status:** GET /status returns VOLUMIO_SYSTEM_STATUS or "ready".
- **Music layout:** music_root + INTERNAL/USB/NAS/SMB; config and env; MPD alignment.

### 3.2 Deferred or stubbed

- **Settings → Sources (stock menu → `miscellanea/my_music`):** `getUiConfig` with `page: miscellanea/my_music` emits the same plugin section layout as Node (`volumio3-backend` `app/plugins/miscellanea/my_music/UIConfig.json`: core sections `my-music`, `network-drives`, `my-music-plugin-enabler`, plus album-art / music-library / browse-visibility blocks). Collection stats and rescan/update already use existing Socket.IO (`getMyCollectionStats`, `rescanDb`, `updateDb`). **Network drives:** `getListShares` → `pushListShares`, `addShare`, `editShare`, `deleteShare`, `getInfoShare` → `pushInfoShare` are implemented with persistence under `settings/mounts/shares.toml`, mounts at `/mnt/NAS/<alias>`, `sudo mount`/`umount` (see [RUNTIME_USER.md](RUNTIME_USER.md)), and Node-compatible toasts / `nasCredentialsCheck`. **`getNetworkSharesDiscovery`** uses **`avahi-browse -p -r _smb._tcp`** (requires **`avahi-utils`** + **`avahi-daemon`**) then **`smbclient -L`** per host (guest, `-m SMB3_11`), matching Node’s `{ nas: [ { name, ip, shares, version? } ] }` shape. **`getListUsbDrives` / `listUsbDrives`** remain empty until USB integration. Saving album-art / “music library” / browse-visibility sections via `callMethod` still needs parity with Node where not yet mapped to Evo config.
- **Plugin REST:** `POST /api/v1/pluginEndpoint` is implemented for **`endpoint: "metavolumio"`** only (browse story/credits). Other plugin endpoints remain unimplemented. **callMethod** (Socket.IO): **`clearAlbumartCache`**, **`installBootBranding`**, ALSA/MPD and **system_controller/system** saves — see **`socketio.rs`**; full arbitrary plugin ABI is still future work.
- **Multi-room:** getzones -> stub `{ zones: [] }`; getMultiRoomDevices and related -> not implemented.
- **HDMI standby, OAuth, pushNotificationUrls:** stubs or skip for minimal port.
- **Playlist manager:** implemented (Socket.IO: getPlaylistContent, listPlaylist, playPlaylist, saveQueueToPlaylist, createPlaylist, deletePlaylist, addToPlaylist, removeFromPlaylist, enqueue; MPD load/save/rm/listplaylist/playlistadd/playlistdelete; browseLibrary supports uri `playlists` and `playlists/<name>`).
- **Favourites, web radio, backup/restore:** not ported; add when UI or product requires.
- **System:** many areas stubbed or minimal (getSystemVersion, wizard, appearance, timezone, …). **Wireless / LAN status:** real **`nmcli`** path — [NETWORK_NM.md](NETWORK_NM.md), Phase 3 below. **Updater, factory reset, My Volumio:** not ported ([PORTING.md](PORTING.md) Part 5–6).
- **Album art:** full handling implemented: exiftool (embedded art when metadata=true; path configurable via config `exiftool_path` or env `VOLUMIO_EVO_EXIFTOOL_PATH`, default `/usr/bin/exiftool`), MPD readpicture (embedded art from file URI), online providers, icon/sectionimage/sourceicon from plugin dirs, resize for albumartd (500px) and tinyart (250px).

### 3.3 Optional / future

- Reserved for future items.

---

## Part 4: Outstanding work by phase

What is left to do, grouped by phase. Each phase states what is **done**, what is **outstanding** (could be implemented or minimal/no-op for UI), and what **cannot be ported** (reasons in Part 5).

### Phase 1 – Core playback & browse

| Status | Scope |
|--------|--------|
| **Done** | Playback (getState, getQueue, play, pause, volume, seek, etc.), queue ops, browseLibrary (Evo layout + MPD), playlists (MPD stored), album art (path/cache/personal/MPD/exiftool/online), music layout (INTERNAL/USB/NAS/SMB), replaceAndPlay, goTo, search, superSearch, getMyCollectionStats, rescanDb, updateDb. |
| **Outstanding** | None. |
| **Cannot be ported** | N/A. |

### Phase 2 – Socket.IO UI compatibility

Events the Volumio UI sends that only need a **valid response** (minimal or no-op) so the UI does not hang. Most are done; remaining:

| Status | Events / area |
|--------|----------------|
| **Done** | closeAllModals, getState, getQueue, browseLibrary, playlists, device/system/menu/ui stubs (getDeviceInfo, getSystemVersion, getMenuItems, getUiConfig, getUiSettings, getBackgrounds, getExperienceAdvancedSettings, etc.), getDonePage, getWizard, getWizardSteps, getWizardUiConfig, deleteBackground, getExtendedOutputDevices, getOutputDevices, setOutputDevices, getMultiRoomDevices, getMultiroom, setMultiroom, writeMultiroom, getSleep, getAlarms, getPrivacySettings, getInfinityPlayback, timezone/language stubs, initSocket, serviceUpdateTracklist, updateAllMetadata, importServicePlaylists. **Wi‑Fi / LAN via NM:** **`getWirelessNetworks`**, **`getInfoNetwork`**, etc. — see **Phase 3**. |
| **Outstanding** | **Network UI (non-scan):** some flows still need minimal responses (see Phase 3). **Other:** getOnboardingWizard, setOnboardingWizardFalse, runFirstConfigWizard, setWizardAction; getAvailablePlugins → pushAvailablePlugins; getPluginDetails; getDeviceActivationStatus → pushDeviceActivationStatus; getMyVolumioStatus → pushMyVolumioToken; getMyMusicPlugins → pushMyMusicPlugins; getAutomaticUpdateEnabled, getUpdaterChannel, updateCheckCache → push*; setTOSAccepted, isLatestTOSAccepted → pushLatestTOSAccepted; checkPassword; regenerateThumbnails. Minimal or no-op where full feature not ported. |
| **Cannot be ported** | Same event names can be handled; “cannot be ported” applies to the *legacy Node implementation* behind some features (see Part 5–6 for plugins, My Volumio, updater). **Networking:** Evo uses NM, not a line‑by‑line port of the Node WiFi plugin — Phase 3. |

### Phase 3 – Networking

| Status | Scope |
|--------|--------|
| **Done (partial)** | **`nmcli`** integration: **`getWirelessNetworks`** / **`getWirelessNetworksCache`** → **`pushWirelessNetworks`** (scan); **`getInfoNetwork`** / reload; **`saveWirelessNetworkSettings`**, **`connectWirelessNetworkWizard`** — see [NETWORK_NM.md](NETWORK_NM.md), **`crates/core/src/nm_network.rs`**. Not a port of the Node **`network_manager`** plugin; Evo uses NetworkManager. |
| **Outstanding** | Full parity with Node wizard UX and every edge case in [NETWORK_NM.md](NETWORK_NM.md); Phase 2 **minimal** responses for wizard-only events until stubbed. |
| **Cannot be ported** | The legacy Node WiFi plugin implementation line-for-line. Evo’s NM stack is a **new** implementation behind the same Socket.IO names. |

### Phase 4 – Plugins

Breakdown of what exists in Volumio vs Evo.

| Sub-area | Volumio | Evo status | Notes |
|----------|---------|------------|--------|
| **List installed** | getInstalledPlugins → pushInstalledPlugins (Node plugins) | **Implemented** | Evo lists `.wasm` files in plugin dir; same event name, different payload shape (name only). |
| **Plugin store / catalog** | getAvailablePlugins, getPluginDetails (from Volumio store/cloud) | **Not ported** | Store is a Volumio service for **Node** plugins. Evo has no plugin store yet. |
| **Install / update / uninstall** | installPlugin, updatePlugin, unInstallPlugin (zip from store or upload) | **Not ported** | Node: npm/zip install, enable/disable in config. Evo: different mechanism (e.g. drop .wasm, config). |
| **Enable / disable / status** | enablePlugin, disablePlugin, modifyPluginStatus, preUninstallPlugin | **Not ported** | Node: config + plugin lifecycle. Evo: no equivalent lifecycle yet. |
| **Generic plugin RPC** | callMethod(endpoint, method, data), pluginEndpoint REST | **Partial** | **POST /api/v1/pluginEndpoint** **`metavolumio`**; **callMethod** **`clearAlbumartCache`**, **system**/**ALSA**/**MPD** saves, **`installBootBranding`** — see **`socketio.rs`**, [BRANDED_BOOT.md](BRANDED_BOOT.md). Other plugin methods **deferred** until ABI. |
| **Plugin UI config** | getUiConfig, getDSPUiConfig (from plugins) | **Minimal response** | Evo emits empty `{ page, sections }` so UI does not hang; no plugin-provided config. |

**Outstanding (UI only):** getAvailablePlugins → pushAvailablePlugins `[]`; getPluginDetails → push (minimal or no-op); installPlugin, updatePlugin, unInstallPlugin, enablePlugin, disablePlugin, modifyPluginStatus, preUninstallPlugin → no-op or minimal push so UI does not block.

**Cannot be ported:** See Part 5 (Node plugin system, plugin store).

### Phase 5 – System (updates, factory, HDMI, OAuth, etc.)

| Area | Status | Notes |
|------|--------|------|
| **Updates** | **Not ported** | updateCheck, updateCheckCache, update, getAutomaticUpdateEnabled, getUpdaterChannel, setUpdaterChannel. Volumio uses its own updater (OS/image). Evo may have a different update story. **Outstanding (UI):** minimal push* responses so UI does not hang. |
| **Factory reset / user data** | **Not ported** | deleteUserData, factoryReset. Could be ported if Evo defines “factory reset” (e.g. wipe config, preserve OS). |
| **HDMI standby** | **Not ported** | enableHDMIDisplayStandby, disableHDMIDisplayStandby. Often platform-specific (e.g. Raspberry Pi). Could be ported per platform if Evo has a HAL. |
| **OAuth** | **Not ported** | GET /v1/oauth. Volumio-specific flow. **Cannot be ported** as-is without Volumio OAuth provider. |
| **Push notification URLs** | **Not ported** | GET/POST/DELETE /v1/pushNotificationUrls. **Cannot be ported** without the same backend service. |
| **GET /api/host** | **Implemented** | JSON for UI Socket.IO base URL (**GET** only in Evo). Node’s **POST** `/api/host` not implemented. **`GET /api`** welcome route: not implemented separately from **`GET /`**. |

### Phase 6 – Other (favourites, radio, backup, multiroom, My Volumio, shares, etc.)

| Area | Status | Notes |
|------|--------|------|
| **Favourites** | **Not ported** | addToFavourites, removeFromFavourites, playFavourites; radio favourites. Node: stored in plugin/data. Evo could implement with its own storage; no port of Node code. |
| **Web radio** | **Not ported** | addWebRadio, removeWebRadio. Evo could support streams via MPD; no port of Node web-radio plugin. |
| **Backup / restore** | **Not ported** | manageBackup, getBackup, restoreConfig. Volumio-specific format. Could be reimplemented for Evo config/data. |
| **Multi-room** | **Minimal response** | getMultiRoomDevices, getMultiroom, setMultiroom, writeMultiroom already stubbed. receiveMultiroomDeviceUpdate, setAsMultiroomSingle/Server/Client **not ported**. Real multi-room would be a new implementation (discovery + sync), not a port of Node volumiodiscovery. |
| **My Volumio (cloud)** | **Not ported** | setDeviceActivationCode, getDeviceActivationStatus, getMyVolumioStatus, getMyVolumioToken, setMyVolumioToken, myVolumioLogout, enable/disable/delete My Volumio device, getMyMusicPlugins, enableDisableMyMusicPlugin. **Cannot be ported** without Volumio cloud (see Part 5). **Outstanding (UI):** minimal push* so UI does not hang. |
| **Shares / storage** | **Partial** | **NAS/SMB/NFS:** `addShare`, `deleteShare`, `getListShares`, `getInfoShare`, `editShare` implemented (`settings/mounts/`, `/mnt/NAS`). **USB / safe remove:** `listUsbDrives`, `safeRemoveDrive` not ported. |
| **Audio outputs (extra)** | **Not ported** | getAudioOutputs, enableAudioOutput, disableAudioOutput, setAudioOutputVolume, audioOutputPlay, audioOutputPause → pushAudioOutputs. Node: alsa_controller. Evo has getExtendedOutputDevices/getOutputDevices (minimal `[]`); full ALSA output switching **not ported**. |
| **Library** | **Not ported** | deleteFolder. MPD could support; not implemented in Evo. |
| **Misc** | **Not ported** | regenerateThumbnails (no-op or minimal); installToDisk (OS installer, out of scope). |

---

## Part 5: Cannot be ported (and why)

Features that are **fundamentally not portable** from Node/Volumio to Evo as-is. Evo may later offer equivalent or alternative behaviour with a different design.

| Feature / area | Why it cannot be ported |
|----------------|-------------------------|
| **Node.js plugin system** | Volumio plugins are Node modules (JavaScript, npm, require). Evo uses WebAssembly (.wasm) and a different ABI. There is no 1:1 port of “a Node plugin” to Evo. Plugin *concepts* (e.g. “music source”, “settings panel”) can be reimplemented in Evo’s plugin model. |
| **callMethod / pluginEndpoint (generic)** | These call into Node plugins by name and method. Evo has no Node runtime. **Exceptions:** **POST pluginEndpoint** **`metavolumio`**; **`callMethod`** **`clearAlbumartCache`**; **`system_controller/system`** saves and **`installBootBranding`**; **`audio_interface` / `music_service`** ALSA/MPD **`save*`** — see **`socketio.rs`**. Full generic plugin RPC is **deferred** until Evo’s plugin API is defined. |
| **Volumio plugin store** | The store is a Volumio-hosted service that serves **Node** plugin metadata and packages. Evo has no Node plugin store. A future Evo “store” would serve Wasm or other Evo-native artifacts, not Node packages. |
| **My Volumio (cloud)** | Activation, device linking, tokens, and My Music plugins depend on Volumio’s cloud and APIs. Evo does not implement or replace that backend. Without the same (or a compatible) cloud service, those flows **cannot be ported** as-is. |
| **Volumio OS updater** | Update flow (updateCheck, update, channels) is tied to Volumio OS images and their update mechanism. Evo may use a different OS or update strategy; porting would mean reimplementing an updater for Evo’s stack, not reusing Node updater code. |
| **OAuth / pushNotificationUrls** | Depend on Volumio or third-party OAuth and push services. No implementation in Evo; would require the same or compatible providers. |
| **Multi-room (Volumio discovery/sync)** | Node uses volumiodiscovery and in-house sync protocols. Evo has no equivalent discovery or sync stack. Real multi-room on Evo would be a **new design** (e.g. same protocol or a new one), not a port of the Node code. |
| **Node-based networking (WiFi wizard)** | The **Node plugin** code paths cannot be reused in Evo. Evo already implements **NetworkManager** (`nmcli`) behind the same Socket.IO names — [NETWORK_NM.md](NETWORK_NM.md), Phase 3, **`crates/core/src/nm_network.rs`**. Parity with every Node edge case is **not** guaranteed. |
| **installToDisk** | Volumio OS installer (e.g. write image to SD/USB). This is an OS-level tool, not part of the “backend API” port. Out of scope for Evo backend. |

---

## How to use this doc

1. **Before adding a feature:** Check Part 1 for the exact contract (params, response shape, Socket.IO event names) and any dependencies (plugins, paths, external APIs).
2. **What's already done:** Use Part 2 ("What is already covered") and the terminology table to see what Evo implements, minimal response, no-op, or defers.
3. **When deciding scope:** Use Part 3 for high-level status and Part 4 for outstanding work by phase (networking, plugins breakdown, system, etc.).
4. **What we will never port as-is:** Use Part 5 ("Cannot be ported") for architecture and service limits (Node plugins, cloud, updater, etc.).
5. **When updating:** Add or refine inventory from volumio3-backend first; then update Part 2, Part 3, and Part 4 so the phased list stays accurate.
6. **UI-only gaps (optional fork / upstream):** See [UI_GAP.md](UI_GAP.md) for tracked changes in Volumio2-UI (where, why, Evo workaround today).
