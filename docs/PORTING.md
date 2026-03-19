# Porting: Volumio backend to Evo

This document first lists **all current Volumio (volumio3-backend) functionality** so we can make informed decisions on what to take over and how. The second part summarizes **Evo port status and decisions**.

Reference codebase: **volumio3-backend** (Node/Express). Volumio4, if present elsewhere, can be added to the inventory later.

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
| `/v1/pluginEndpoint` | GET/POST | Plugin REST (payload: endpoint, method, data) |
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

- **Sources (in order):** path on disk (cover files in folder, or exiftool embedded picture) -> `/data/albumart` cache (folder, metadata, web, personal) -> online (Volumio meta API for artist art, Last.fm for album art) -> icon/sectionimage/sourceicon from plugins -> default image.
- **Params:** `web` (artist/album/resolution), `path` (file/folder), `metadata=true`, `icon`, `sourceicon`, `sectionimage`.

### 1.5 Plugins and extensibility

- **Plugin types:** system_controller, music_service, audio_interface, miscellanea (and others). Each plugin can register REST endpoints and Socket.IO is wired via callMethod(endpoint, method, data).
- **Plugin lifecycle:** install, update, uninstall, enable, disable, getInstalledPlugins, getAvailablePlugins (from store/cloud), getPluginDetails.

---

## Part 2: What is already covered in Evo

Quick reference: what exists in Evo today (implemented or stubbed). Details and gaps are in 2.1–2.3 below.

### HTTP routes (root)

| Volumio route | Covered? | Evo behaviour |
|---------------|----------|----------------|
| `GET /` | Yes | Returns "ok" (health; UI served elsewhere or same host) |
| `GET /api/health` | Yes | Returns "ok" |
| `GET /albumart` | Yes | Serves default from albumart_root (default.jpg/png) or embedded placeholder |
| `GET /tinyart/*` | Yes | Same default/placeholder |
| `GET /albumartd` | Yes | Same default/placeholder |
| `/api` (REST) | Yes | Mounted; v1 under `/api/v1` |
| `/dev`, `/plugin-serve`, `/stream`, `/partnerlogo`, `/status` | No | Not implemented |
| `POST /plugin-upload`, `/backgrounds-upload`, `/albumart-upload` | No | Not implemented |

### REST API `/api/v1/`

| Endpoint | Covered? | Evo behaviour |
|----------|----------|----------------|
| `GET /v1/browse` | Yes | Evo layout (local/usb/nas/smb) + MPD lsinfo |
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
| `GET/POST /v1/pluginEndpoint` | No | Deferred |
| `GET/POST/DELETE /v1/pushNotificationUrls` | No | Not implemented |
| `GET /v1/oauth` | No | Not implemented |
| `/api/` (welcome), `/api/host` | No | Not implemented |

### Socket.IO events (client -> server)

| Event(s) | Covered? | Evo behaviour |
|----------|----------|----------------|
| closeAllModals (on connect) | Yes | Emitted on connect |
| getState | Yes | MPD state -> pushState |
| getQueue | Yes | MPD queue -> pushQueue |
| browseLibrary | Yes | Evo layout + MPD lsinfo -> pushBrowseLibrary |
| addToQueue, addPlay | Yes | MPD add + optional play |
| removeFromQueue | Yes | MPD delete (position) |
| volume, play, pause, toggle, stop, next, prev, seek | Yes | MPD commands |
| setRandom, setRepeat, clearQueue | Yes | MPD |
| getInstalledPlugins | Yes | List .wasm -> pushInstalledPlugins |
| All other Socket.IO events | No | Not implemented (playNext, moveQueue, playlists, favourites, multiroom, network, plugins lifecycle, wizard, etc.) |

### Other

| Area | Covered? | Evo behaviour |
|------|----------|----------------|
| Music layout | Yes | music_root, local/usb/nas/smb, config + env, MPD music_directory |
| Album art logic (local cache, exiftool, online) | No | Default/placeholder from albumart_root or embedded; full resolution TBD |

---

## Part 3: Evo port status and decisions

Using the inventory above, we decide what to implement, stub, or defer so the existing UI and clients work against Evo.

### 3.1 Implemented in Evo (summary)

- **Health:** `GET /`, `GET /api/health` -> "ok".
- **REST v1 (core):** getState, getQueue, commands (play, pause, toggle, stop, next, prev, volume, seek, repeat, random, clearQueue, addToQueue, addPlay), replaceAndPlay (POST), browse, listplaylists, search, superSearch, collectionstats, getzones (stub), ping, getSystemVersion, getSystemInfo (stubs), getInstalledPlugins (list .wasm).
- **Socket.IO (core):** getState, getQueue, browseLibrary, addToQueue, addPlay, removeFromQueue, volume, play, pause, toggle, stop, next, prev, seek, setRandom, setRepeat, clearQueue, getInstalledPlugins; closeAllModals on connect; responses: pushState, pushQueue, pushBrowseLibrary, pushInstalledPlugins.
- **Album art routes:** GET /albumart, /albumartd, /tinyart/* present; serve default image from albumart_root (default.jpg/default.png) or embedded placeholder.
- **Music layout:** music_root + local/usb/nas/smb; config and env; MPD alignment.

### 3.2 Deferred or stubbed

- **Plugin REST/WebSocket:** pluginEndpoint, callMethod -> defer until plugin ABI and HTTP needs are clear.
- **Multi-room:** getzones -> stub `{ zones: [] }`; getMultiRoomDevices and related -> not implemented.
- **HDMI standby, OAuth, pushNotificationUrls:** stubs or skip for minimal port.
- **Playlist manager (create/delete/addToPlaylist/...), favourites, web radio, backup/restore:** not ported; add when UI or product requires.
- **System (network, wireless, updater, factory reset, My Volumio, wizard, appearance, timezone, etc.):** not ported; stubs where needed for UI (e.g. getSystemVersion/getSystemInfo).
- **Album art logic:** default/placeholder implemented (from config albumart_root or embedded). Full resolution (local cache, exiftool, Volumio/Last.fm) not yet ported.

### 3.3 Optional / future

- **Push state/queue:** MPD idle or polling -> broadcast pushState/pushQueue to Socket.IO clients.
- **Queue reorder:** moveQueue, playNext (MPD move/priority).
- **Album art:** full resolution (path → folder cache → metadata/exiftool → personal → online → icon) and MPD readpicture; default/placeholder already served.

---

## How to use this doc

1. **Before adding a feature:** Check Part 1 for the exact contract (params, response shape, Socket.IO event names) and any dependencies (plugins, paths, external APIs).
2. **What's already done:** Use Part 2 ("What is already covered") to see at a glance which routes and events Evo implements or stubs.
3. **When deciding scope:** Use Part 3 to keep “implemented / stubbed / deferred” consistent and to avoid ad-hoc gaps.
4. **When updating:** Add or refine inventory items from volumio3-backend (or volumio4) first; then update Part 2 and Part 3.
