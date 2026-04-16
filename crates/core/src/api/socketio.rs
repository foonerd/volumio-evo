//! Socket.IO adapter: same event names as Node backend so the existing UI works.
//! Maps getState/getQueue/browseLibrary/addToQueue/addPlay/volume/transport to MPD.

use crate::alsa;
use crate::alsa_cards;
use crate::i2s;
use crate::mpd::{
    self, browse_song_albumart_path_only, BrowseItem, BrowseList, BrowseNavigation, BrowsePrev,
    BrowseResponse, MpdConfig,
};
use serde::Deserialize;
use socketioxide::extract::{Data, SocketRef, State, TryData};
use std::time::Instant;

use super::{read_master_volume_percent, AppState};

fn mpd_config(state: &AppState) -> MpdConfig {
    MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    }
}

/// Emit pushBrowseLibrary and store in state for getLastPushedBrowseLibrary.
async fn push_browse_and_store(s: &SocketRef, state: &AppState, resp: &BrowseResponse) {
    let mut resp = resp.clone();
    mpd::browse_response_fill_meta_from_artist(&mut resp);
    s.emit("pushBrowseLibrary", &resp).ok();
    if let Ok(v) = serde_json::to_value(&resp) {
        state.set_last_browse(v).await;
    }
}

/// Register default namespace and all UI event handlers.
pub fn register_handlers(io: &socketioxide::SocketIo) {
    io.ns("/", on_connect);
}

async fn on_connect(s: SocketRef) {
    s.emit("closeAllModals", "").ok();

    s.on("getState", get_state);
    s.on("getQueue", get_queue);
    s.on("browseLibrary", browse_library);
    s.on("addToQueue", add_to_queue);
    s.on("addPlay", add_play);
    s.on("removeFromQueue", remove_from_queue);
    s.on("volume", volume);
    s.on("play", play);
    s.on("pause", pause);
    s.on("toggle", toggle);
    s.on("stop", stop);
    s.on("next", next);
    s.on("prev", prev);
    s.on("seek", seek);
    s.on("setRandom", set_random);
    s.on("setRepeat", set_repeat);
    s.on("clearQueue", clear_queue);
    s.on("getInstalledPlugins", get_installed_plugins);
    s.on("getAvailablePlugins", get_available_plugins);
    s.on("moveQueue", move_queue);
    s.on("playNext", play_next);
    // Playlist manager
    s.on("getPlaylistContent", get_playlist_content);
    s.on("listPlaylist", list_playlist);
    s.on("playPlaylist", play_playlist);
    s.on("saveQueueToPlaylist", save_queue_to_playlist);
    s.on("createPlaylist", create_playlist);
    s.on("deletePlaylist", delete_playlist);
    s.on("addToPlaylist", add_to_playlist);
    s.on("removeFromPlaylist", remove_from_playlist);
    s.on("enqueue", enqueue);
    s.on("GetTrackInfo", get_track_info);
    s.on("callMethod", call_method);
    s.on("pinger", pinger);
    s.on("setConsume", set_consume);
    s.on("getLastPushedBrowseLibrary", get_last_pushed_browse_library);
    s.on("mute", mute);
    s.on("unmute", unmute);
    s.on("rescanDb", rescan_db);
    s.on("updateDb", update_db);
    s.on("replaceAndPlay", replace_and_play);
    s.on("goTo", go_to);
    s.on("replaceAndPlayCue", replace_and_play_cue);
    s.on("addPlayCue", add_play_cue);
    s.on("playItemsList", play_items_list);
    s.on("search", search);
    s.on("superSearch", super_search);
    s.on("getMyCollectionStats", get_my_collection_stats);
    s.on("removeQueueItem", remove_queue_item);
    s.on("addQueueUids", add_queue_uids);
    s.on("skipBackwards", skip_backwards);
    s.on("skipForward", skip_forward);
    s.on("closeModals", close_modals);
    s.on("getInputSources", get_input_sources);
    s.on("getDeviceInfo", get_device_info);
    s.on("getBrowseSources", get_browse_sources);
    s.on("getBrowseFilters", get_browse_filters);
    s.on("getSystemVersion", get_system_version);
    s.on("getSystemInfo", get_system_info);
    s.on("getMenuItems", get_menu_items);
    s.on("getUiConfig", get_ui_config);
    s.on("getDSPUiConfig", get_dsp_ui_config);
    s.on("getAvailableLanguages", get_available_languages);
    s.on("getDeviceName", get_device_name);
    s.on("setLanguage", set_language);
    s.on("getAvailableTimezones", get_available_timezones);
    s.on("getCurrentTimezone", get_current_timezone);
    s.on("setTimezone", set_timezone);
    s.on("initSocket", init_socket);
    s.on("volatilePlay", volatile_play);
    s.on("getLibraryListing", get_library_listing);
    s.on("getLibraryFilters", get_library_filters);
    s.on("getPlaylistIndex", get_playlist_index);
    s.on("getMultiRoomDevices", get_multi_room_devices);
    s.on("serviceUpdateTracklist", service_update_tracklist);
    s.on("updateAllMetadata", update_all_metadata);
    s.on("importServicePlaylists", import_service_playlists);
    s.on("setDeviceName", set_device_name);
    s.on("getDeviceHWUUID", get_device_hw_uuid);
    s.on("getUiSettings", get_ui_settings);
    s.on("getShutdownOrStandbyMode", get_shutdown_or_standby_mode);
    s.on("getPrivacySettings", get_privacy_settings);
    s.on("getInfinityPlayback", get_infinity_playback);
    s.on("setInfinityPlayback", set_infinity_playback);
    s.on("getSleep", get_sleep);
    s.on("setSleep", set_sleep);
    s.on("getAlarms", get_alarms);
    s.on("saveAlarm", save_alarm);
    s.on("getMultiroom", get_multiroom);
    s.on("setMultiroom", set_multiroom);
    s.on("writeMultiroom", write_multiroom);
    s.on("getExtendedOutputDevices", get_extended_output_devices);
    s.on("getOutputDevices", get_output_devices);
    s.on("getBackgrounds", get_backgrounds);
    s.on("setBackgrounds", set_backgrounds);
    s.on("getExperienceAdvancedSettings", get_experience_advanced_settings);
    s.on("setExperienceAdvancedSettings", set_experience_advanced_settings);
    s.on("setOutputDevices", set_output_devices);
    s.on("getDonePage", get_done_page);
    s.on("getWizard", get_wizard);
    s.on("getWizardSteps", get_wizard_steps);
    s.on("getWizardUiConfig", get_wizard_ui_config);
    s.on("deleteBackground", delete_background);
}

async fn get_state(s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    let master = read_master_volume_percent(&state).await;
    match mpd::get_state_connected(&config, &state.config.music_sources.music_root, master).await {
        Ok(payload) => {
            s.emit("pushState", &payload).ok();
        }
        Err(e) => {
            tracing::warn!("getState MPD error: {}", e);
        }
    }
}

async fn get_queue(s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    match mpd::get_queue_connected(&config).await {
        Ok(items) => {
            let payload = serde_json::json!({ "queue": items });
            s.emit("pushQueue", &payload).ok();
        }
        Err(e) => {
            tracing::warn!("getQueue MPD error: {}", e);
        }
    }
}

#[derive(Debug, Deserialize)]
struct SearchPayload {
    #[serde(default)]
    value: String,
    #[serde(default)]
    query: String,
}

async fn search(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<SearchPayload>,
) {
    let q = payload.query.trim();
    let v = payload.value.trim();
    let query = if !q.is_empty() { q } else { v };
    if query.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    match mpd::search_connected(&config, &state.config.music_sources.music_root, query).await {
        Ok(resp) => push_browse_and_store(&s, &state, &resp).await,
        Err(e) => tracing::warn!("search MPD error: {}", e),
    }
}

async fn super_search(
    s: SocketRef,
    state: State<AppState>,
    Data(payload): Data<SearchPayload>,
) {
    // Same as search in Evo (MPD find any); Volumio may differ for superSearch.
    search(s, state, Data(payload)).await
}

async fn get_my_collection_stats(s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    match mpd::collection_stats_connected(&config).await {
        Ok(stats) => {
            s.emit("pushMyCollectionStats", &stats).ok();
        }
        Err(e) => tracing::warn!("getMyCollectionStats MPD error: {}", e),
    }
}

/// removeQueueItem: same as removeFromQueue, payload { value: position } (1-based from UI).
async fn remove_queue_item(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<RemoveFromQueuePayload>,
) {
    let pos = payload.value.saturating_sub(1);
    let config = mpd_config(&state);
    if let Err(e) = mpd::remove_from_queue_connected(&config, pos).await {
        tracing::warn!("removeQueueItem MPD error: {}", e);
    }
}

/// addQueueUids: payload is array of URI strings (or { uids: [...] }); add all to queue.
async fn add_queue_uids(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<AddQueueUidsPayload>,
) {
    let uris: Vec<String> = payload
        .into_uris()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if uris.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    if let Err(e) = mpd::add_multiple_to_queue_connected(&config, &uris).await {
        tracing::warn!("addQueueUids MPD error: {}", e);
    }
}

/// Payload for addQueueUids: client may send raw array ["uri1", ...] or { uids: [...] }.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AddQueueUidsPayload {
    Raw(Vec<String>),
    Wrapped { uids: Vec<String> },
}
impl AddQueueUidsPayload {
    fn into_uris(self) -> Vec<String> {
        match self {
            AddQueueUidsPayload::Raw(v) => v,
            AddQueueUidsPayload::Wrapped { uids } => uids,
        }
    }
}

const SKIP_SECONDS: u64 = 10;

async fn skip_backwards(_s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    if let Err(e) = mpd::skip_backwards_connected(&config, SKIP_SECONDS).await {
        tracing::warn!("skipBackwards MPD error: {}", e);
    }
}

async fn skip_forward(_s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    if let Err(e) = mpd::skip_forward_connected(&config, SKIP_SECONDS).await {
        tracing::warn!("skipForward MPD error: {}", e);
    }
}

async fn close_modals(s: SocketRef) {
    s.emit("closeAllModals", "").ok();
}

async fn get_input_sources(s: SocketRef) {
    // Node: executeBrowseSource('inputs') — Evo has no input plugins yet.
    let sources: Vec<serde_json::Value> = vec![];
    s.emit("pushInputSources", &sources).ok();
}

/// Visible browse sources for the sidebar / browse source picker — must match Node
/// `app/musiclibrary.js` default `browseSources` (when browsesources.json is absent).
/// Sidebar entries: Favourites, tag library, playlists, etc. The `music-library` root listing
/// itself is storage-only (INTERNAL, USB, NAS, SMB) with `albumart`, matching Node browse rows.
fn browse_sources_json() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "albumart": "/albumart?sourceicon=music_service/mpd/favouritesicon.png",
            "name": "Favourites",
            "uri": "favourites",
            "plugin_type": "",
            "plugin_name": ""
        }),
        serde_json::json!({
            "albumart": "/albumart?sourceicon=music_service/mpd/playlisticon.png",
            "name": "Playlists",
            "uri": "playlists",
            "plugin_type": "music_service",
            "plugin_name": "mpd"
        }),
        serde_json::json!({
            "albumart": "/albumart?sourceicon=music_service/mpd/musiclibraryicon.png",
            "name": "Music Library",
            "uri": "music-library",
            "plugin_type": "music_service",
            "plugin_name": "mpd"
        }),
        serde_json::json!({
            "albumart": "/albumart?sourceicon=music_service/mpd/artisticon.png",
            "name": "Artists",
            "uri": "artists://",
            "plugin_type": "music_service",
            "plugin_name": "mpd"
        }),
        serde_json::json!({
            "albumart": "/albumart?sourceicon=music_service/mpd/albumicon.png",
            "name": "Albums",
            "uri": "albums://",
            "plugin_type": "music_service",
            "plugin_name": "mpd"
        }),
        serde_json::json!({
            "albumart": "/albumart?sourceicon=music_service/mpd/genreicon.png",
            "name": "Genres",
            "uri": "genres://",
            "plugin_type": "music_service",
            "plugin_name": "mpd"
        }),
    ]
}

async fn get_device_info(s: SocketRef) {
    let data = serde_json::json!({
        "uuid": "evo-stub",
        "name": "Volumio Evo"
    });
    s.emit("pushDeviceInfo", &data).ok();
}

async fn get_browse_sources(s: SocketRef) {
    let sources = browse_sources_json();
    s.emit("pushBrowseSources", &sources).ok();
}

/// Stock UI calls `getBrowseFilters` on browse init (Node: musiclibrary index filters). Evo uses MPD browse only — empty list.
async fn get_browse_filters(s: SocketRef) {
    s.emit("pushBrowseFilters", &serde_json::json!([])).ok();
}

async fn get_system_version(s: SocketRef) {
    let data = serde_json::json!({
        "systemversion": "4.0",
        "variant": "volumio-evo",
        "hardware": "generic",
        "os": null,
        "builddate": null
    });
    s.emit("pushSystemVersion", &data).ok();
}

async fn get_system_info(s: SocketRef) {
    let data = serde_json::json!({
        "systemversion": "4.0",
        "variant": "volumio-evo",
        "hardware": "generic",
        "os": null,
        "builddate": null,
        "hostname": "volumio-evo",
        "hwUuid": "evo-stub"
    });
    s.emit("pushSystemInfo", &data).ok();
}

/// Main menu for Evo. Node merges `mainmenu.json` with i18n so **names are already human text** before
/// `pushMenuItems`; the Angular template uses `{{::item.name}}` without `| translate`, so we must not
/// emit `TRANSLATE.*` placeholders (those only work after Node's server-side resolution).
///
/// Shape matches `volumio3-backend/app/mainmenu.json` + English `strings_en.json` values.
/// Omits **browse** when `active_layout` is not `manifest` (same as Node `VOLUMIO_ACTIVE_UI_NAME`).
async fn get_menu_items(s: SocketRef, State(state): State<AppState>) {
    let layout = state.config.ui.active_layout.trim();
    let mut items: Vec<serde_json::Value> = Vec::new();
    if layout == "manifest" {
        items.push(serde_json::json!({
            "id": "browse",
            "name": "Music",
            "state": "volumio.browse"
        }));
    }
    items.extend([
        serde_json::json!({"id": "mymusic", "name": "Sources", "state": "volumio.plugin", "params": {"pluginName": "miscellanea/my_music"}}),
        serde_json::json!({"id": "playback_options", "name": "Playback Options", "state": "volumio.plugin", "params": {"pluginName": "audio_interface/alsa_controller"}}),
        serde_json::json!({"id": "appearance", "name": "Appearance", "state": "volumio.plugin", "params": {"pluginName": "miscellanea/appearance"}}),
        serde_json::json!({"id": "network", "name": "Network", "state": "volumio.plugin", "params": {"pluginName": "system_controller/network"}}),
        serde_json::json!({"id": "system", "name": "System", "state": "volumio.plugin", "params": {"pluginName": "system_controller/system"}}),
        serde_json::json!({"id": "plugin-manager", "name": "Plugins", "state": "volumio.plugin-manager"}),
        serde_json::json!({"id": "modal", "name": "Alarm", "params": {"modalName": "modal-alarm-clock"}}),
        serde_json::json!({"id": "modal", "name": "Sleep", "params": {"modalName": "modal-sleep"}}),
        serde_json::json!({"id": "shutdown", "name": "Shutdown", "params": {"modalName": "modal-power-off"}}),
        serde_json::json!({"id": "iframe-page", "name": "Help", "params": {"url": "http://help.volumio.com"}}),
        serde_json::json!({"id": "iframe-page", "name": "Volumio Shop", "params": {"url": "https://volumio.com/shop/"}}),
    ]);
    s.emit("pushMenuItems", &serde_json::Value::Array(items)).ok();
}

/// Empty plugin UI config stub (Node: getUIConfigOnPlugin per page). Payload: { page?: string }.
fn empty_ui_config() -> serde_json::Value {
    serde_json::json!({ "page": { "label": "" }, "sections": [] })
}

#[derive(Debug, Deserialize)]
struct GetUiConfigPayload {
    #[serde(default)]
    #[allow(dead_code)]
    page: String,
}

async fn get_ui_config(s: SocketRef, State(state): State<AppState>, TryData(payload): TryData<GetUiConfigPayload>) {
    let page = payload.ok().map(|p| p.page).unwrap_or_default();
    if page.trim() == "audio_interface/alsa_controller" {
        match build_playback_options_ui(&state).await {
            Ok(v) => {
                s.emit("pushUiConfig", &v).ok();
            }
            Err(e) => {
                tracing::warn!("getUiConfig playback options: {}", e);
                s.emit("pushUiConfig", &empty_ui_config()).ok();
            }
        }
    } else {
        s.emit("pushUiConfig", &empty_ui_config()).ok();
    }
}

async fn emit_playback_options_ui(s: &SocketRef, state: &AppState) {
    match build_playback_options_ui(state).await {
        Ok(v) => {
            s.emit("pushUiConfig", &v).ok();
        }
        Err(e) => tracing::warn!("refresh Playback Options UI: {}", e),
    }
}

async fn build_playback_options_ui(state: &AppState) -> anyhow::Result<serde_json::Value> {
    let settings = state.alsa.read().await.clone();
    let playback = state.playback.read().await.clone();
    let profile = i2s::hardware_profile();
    let dacs_file = i2s::load_dacs().ok();
    let i2s_dacs: Vec<i2s::DacEntry> = dacs_file
        .as_ref()
        .map(|d| i2s::dac_list_for_profile(d, &profile))
        .unwrap_or_default();
    let catalog = alsa_cards::AlsaCardCatalog::load_optional();

    let mut cards = match tokio::task::spawn_blocking(|| alsa::list_playback_cards()).await {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            tracing::warn!("aplay -l: {}", e);
            vec![]
        }
        Err(e) => return Err(anyhow::Error::new(e)),
    };

    if cards.is_empty() {
        tracing::info!("no ALSA playback cards; Playback Options shows a placeholder device");
        cards.push(alsa::AplayCard {
            id: "nodev".to_string(),
            name: "No playback device (is aplay installed?)".to_string(),
        });
    }

    let cards = alsa_cards::prepare_playback_cards(
        cards,
        &settings,
        &catalog,
        dacs_file.as_ref(),
        &profile,
    );
    let settings = alsa::coerce_selection(&cards, settings);

    let card_for_mixers = settings.output_device_id.clone();
    let mixer_controls = match tokio::task::spawn_blocking(move || {
        alsa::list_playback_mixer_controls(&card_for_mixers)
    })
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("mixer probe task: {}", e);
            vec![]
        }
    };

    let params = alsa::PlaybackOptionsUiParams {
        cards: &cards,
        settings: &settings,
        i2s_dacs: &i2s_dacs,
        playback: &playback,
        mixer_controls: &mixer_controls,
    };
    Ok(alsa::playback_options_ui_config(&params))
}

async fn get_dsp_ui_config(s: SocketRef) {
    s.emit("pushDSPUiConfig", &empty_ui_config()).ok();
}

/// Stub: English only (Node reads appearance plugin languages.json).
async fn get_available_languages(s: SocketRef) {
    let data = serde_json::json!({
        "defaultLanguage": { "language": "English", "code": "en" },
        "available": [{ "language": "English", "code": "en" }]
    });
    s.emit("pushAvailableLanguages", &data).ok();
}

async fn get_device_name(s: SocketRef) {
    let data = serde_json::json!({ "name": "Volumio Evo" });
    s.emit("pushDeviceName", &data).ok();
}

/// No-op: Node calls appearance plugin setLanguage; Evo has no persistence for language.
async fn set_language(_s: SocketRef, TryData(_payload): TryData<SetLanguagePayload>) {
    // Accept payload so client doesn't error; do nothing.
}

/// Stub: UTC only (Node uses system plugin getAvailableTimezones).
async fn get_available_timezones(s: SocketRef) {
    let data = serde_json::json!([{ "value": "UTC", "label": "UTC" }]);
    s.emit("pushAvailableTimezones", &data).ok();
}

/// Stub: current timezone UTC (Node uses system plugin getCurrentTimezone).
async fn get_current_timezone(s: SocketRef) {
    let data = serde_json::json!({ "value": "UTC", "label": "UTC" });
    s.emit("pushCurrentTimezone", &data).ok();
}

/// No-op: Node calls system plugin setTimezone; Evo has no timezone persistence.
async fn set_timezone(_s: SocketRef, TryData(_payload): TryData<serde_json::Value>) {
    // Accept any payload; do nothing.
}

/// No-op: Node calls volumiodiscovery plugin initSocket; Evo has no discovery.
async fn init_socket(_s: SocketRef, TryData(_payload): TryData<serde_json::Value>) {
    // Accept any payload; do nothing.
}

/// Same as play: MPD play with optional position (Node: volumioVolatilePlay).
async fn volatile_play(
    _s: SocketRef,
    State(state): State<AppState>,
    payload: TryData<PlayPayload>,
) {
    let position = payload.as_ref().ok().and_then(|p| p.value);
    let config = mpd_config(&state);
    let _ = mpd::run_command_connected(&config, "play", None, position, None, None).await;
}

#[derive(Debug, Deserialize)]
struct GetLibraryListingPayload {
    #[serde(default)]
    #[allow(dead_code)]
    uid: String,
    #[serde(default)]
    #[allow(dead_code)]
    options: Option<serde_json::Value>,
}

/// Stub: no Volumio music-library index (Node: musicLibrary.getListing -> library object).
async fn get_library_listing(s: SocketRef, TryData(_p): TryData<GetLibraryListingPayload>) {
    let stub = serde_json::json!({
        "name": "",
        "type": "root",
        "children": []
    });
    s.emit("pushLibraryListing", &stub).ok();
}

/// Stub: empty children (Node: musicLibrary.getIndex(sUid).children).
async fn get_library_filters(s: SocketRef, TryData(_uid): TryData<serde_json::Value>) {
    s.emit("pushLibraryFilters", &serde_json::json!([])).ok();
}

/// Stub: empty index (Node: playlistFS.getIndex(sUid)).
async fn get_playlist_index(s: SocketRef, TryData(_uid): TryData<serde_json::Value>) {
    s.emit("pushPlaylistIndex", &serde_json::json!([])).ok();
}

/// Stub: no multi-room devices (Node: volumiodiscovery.getDevices -> pushMultiRoomDevices).
/// Payload must match stock shape: `{ misc, list }` — the UI does `data.list.forEach(...)`.
async fn get_multi_room_devices(s: SocketRef, TryData(_data): TryData<serde_json::Value>) {
    s.emit(
        "pushMultiRoomDevices",
        &serde_json::json!({
            "misc": { "debug": false },
            "list": []
        }),
    )
    .ok();
}

/// No-op: Node calls music_service plugin rebuildTracklist; Evo has single MPD source.
async fn service_update_tracklist(_s: SocketRef, TryData(_payload): TryData<serde_json::Value>) {}

/// No-op: Node calls commandRouter.updateAllMetadata (library refresh); Evo has no library DB.
async fn update_all_metadata(_s: SocketRef) {}

/// No-op: Node calls playlistFS.importServicePlaylists; Evo has no service playlists to import.
async fn import_service_playlists(_s: SocketRef) {}

/// No-op: Node saves player_name via system plugin; Evo has no device-name persistence.
async fn set_device_name(_s: SocketRef, TryData(_payload): TryData<serde_json::Value>) {}

/// Stub: same as getSystemInfo hwUuid (Node: commandRouter.getHwuuid -> pushDeviceHWUUID).
async fn get_device_hw_uuid(s: SocketRef) {
    s.emit("pushDeviceHWUUID", &serde_json::json!("evo-stub")).ok();
}

/// UI settings for the stock Angular UI (Node: appearance plugin getUiSettings).
/// `language` is required: `ui-settings.service.js` only calls `$translate.use()` when this is set;
/// otherwise the UI shows raw keys (`COMMON.TAB_BROWSE`, …).
/// `active_layout` mirrors stock `volumioUisList.json` `uiName` (manifest / contemporary / classic).
async fn get_ui_settings(s: SocketRef, State(state): State<AppState>) {
    s.emit(
        "pushUiSettings",
        &serde_json::json!({
            "language": "en",
            "theme": "volumio3",
            "active_layout": state.config.ui.active_layout
        }),
    )
    .ok();
}

/// Stub: shutdown mode (Node: commandRouter.getShutdownOrStandbyMode -> pushShutdownOrStandbyMode).
async fn get_shutdown_or_standby_mode(s: SocketRef) {
    s.emit("pushShutdownOrStandbyMode", &serde_json::json!({})).ok();
}

/// Stub: empty privacy settings (Node: system plugin getPrivacySettings -> pushPrivacySettings).
async fn get_privacy_settings(s: SocketRef) {
    s.emit("pushPrivacySettings", &serde_json::json!({})).ok();
}

/// Stub: infinity playback disabled (Node: metavolumio getInfinityPlayback -> pushInfinityPlayback).
async fn get_infinity_playback(s: SocketRef) {
    s.emit("pushInfinityPlayback", &serde_json::json!({ "enabled": false })).ok();
}

/// No-op: Node calls metavolumio setInfinityPlayback; Evo has no infinity playback.
async fn set_infinity_playback(_s: SocketRef, TryData(_payload): TryData<serde_json::Value>) {}

/// Stub: no sleep timer (Node: alarm-clock getSleep -> pushSleep).
async fn get_sleep(s: SocketRef, TryData(_data): TryData<serde_json::Value>) {
    s.emit("pushSleep", &serde_json::json!({})).ok();
}

/// Stub: accept set, emit pushSleep {} (Node: alarm-clock setSleep -> pushSleep).
async fn set_sleep(s: SocketRef, TryData(_data): TryData<serde_json::Value>) {
    s.emit("pushSleep", &serde_json::json!({})).ok();
}

/// Stub: no alarms (Node: alarm-clock getAlarms -> pushAlarm).
async fn get_alarms(s: SocketRef, TryData(_data): TryData<serde_json::Value>) {
    s.emit("pushAlarm", &serde_json::json!([])).ok();
}

/// Stub: accept save, emit pushSleep {} (Node: alarm-clock saveAlarm -> pushSleep).
async fn save_alarm(s: SocketRef, TryData(_data): TryData<serde_json::Value>) {
    s.emit("pushSleep", &serde_json::json!({})).ok();
}

/// Stub: no multiroom state (Node: multiroom plugin getMultiroom -> pushMultiroom).
async fn get_multiroom(s: SocketRef, TryData(_data): TryData<serde_json::Value>) {
    s.emit("pushMultiroom", &serde_json::json!({})).ok();
}

/// Stub: accept set, emit pushMultiroom {} (Node: multiroom setMultiroom -> pushMultiroom).
async fn set_multiroom(s: SocketRef, TryData(_data): TryData<serde_json::Value>) {
    s.emit("pushMultiroom", &serde_json::json!({})).ok();
}

/// No-op: Node calls multiroom writeMultiRoom; Evo has no multi-room.
async fn write_multiroom(_s: SocketRef, TryData(_data): TryData<serde_json::Value>) {}

/// Stub: no extended output devices (Node: alsa_controller getExtendedOutputDevices -> pushExtendedOutputDevices).
async fn get_extended_output_devices(s: SocketRef) {
    s.emit("pushExtendedOutputDevices", &serde_json::json!([])).ok();
}

/// ALSA device list for wizard / Playback (Node: alsa_controller getAudioDevices -> pushOutputDevices).
async fn get_output_devices(s: SocketRef, State(state): State<AppState>) {
    let settings = state.alsa.read().await.clone();
    let profile = i2s::hardware_profile();
    let dacs_file = i2s::load_dacs().ok();
    let i2s_dacs: Vec<i2s::DacEntry> = dacs_file
        .as_ref()
        .map(|d| i2s::dac_list_for_profile(d, &profile))
        .unwrap_or_default();
    let catalog = alsa_cards::AlsaCardCatalog::load_optional();

    let cards = match tokio::task::spawn_blocking(|| alsa::list_playback_cards()).await {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            tracing::warn!("getOutputDevices aplay: {}", e);
            vec![alsa::AplayCard {
                id: "nodev".into(),
                name: "No playback device".into(),
            }]
        }
        Err(e) => {
            tracing::warn!("getOutputDevices join: {}", e);
            vec![alsa::AplayCard {
                id: "nodev".into(),
                name: "No playback device".into(),
            }]
        }
    };
    let cards = alsa_cards::prepare_playback_cards(
        cards,
        &settings,
        &catalog,
        dacs_file.as_ref(),
        &profile,
    );
    let settings = alsa::coerce_selection(&cards, settings);
    let payload = alsa::push_output_devices_json(&cards, &settings, &i2s_dacs);
    s.emit("pushOutputDevices", &payload).ok();
}

/// Stub: no backgrounds (Node: appearance getBackgrounds -> pushBackgrounds).
async fn get_backgrounds(s: SocketRef) {
    s.emit("pushBackgrounds", &serde_json::json!([])).ok();
}

/// Stub: accept set, emit pushBackgrounds [] (Node: appearance setBackgrounds -> pushBackgrounds).
async fn set_backgrounds(s: SocketRef, TryData(_data): TryData<serde_json::Value>) {
    s.emit("pushBackgrounds", &serde_json::json!([])).ok();
}

/// Stub: empty experience advanced settings (Node: commandRouter.getExperienceAdvancedSettings -> pushExperienceAdvancedSettings).
async fn get_experience_advanced_settings(s: SocketRef) {
    s.emit("pushExperienceAdvancedSettings", &serde_json::json!({})).ok();
}

/// No-op: Node calls system setExperienceAdvancedSettings; Evo has no persistence.
async fn set_experience_advanced_settings(_s: SocketRef, TryData(_data): TryData<serde_json::Value>) {}

/// Persist output device (wizard `setOutputDevices`; same shape as `saveAlsaOptions` data).
async fn set_output_devices(s: SocketRef, State(state): State<AppState>, Data(data): Data<serde_json::Value>) {
    let mut guard = state.alsa.write().await;
    match guard.apply_save_payload(&data) {
        Ok(()) => tracing::info!("ALSA output saved: {:?}", *guard),
        Err(e) => tracing::warn!("setOutputDevices: {}", e),
    }
    drop(guard);
    get_output_devices(s, State(state.clone())).await;
}

/// Stub: no wizard done page (Node: wizard getDonation/getDonationsArray/getDoneMessage -> pushDonePage).
async fn get_done_page(s: SocketRef) {
    s.emit(
        "pushDonePage",
        &serde_json::json!({
            "congratulations": "",
            "title": "",
            "message": "",
            "donation": {},
            "donationAmount": []
        }),
    )
    .ok();
}

/// Stub: wizard not open (Node: wizard showWizard -> pushWizard).
async fn get_wizard(s: SocketRef) {
    s.emit("pushWizard", &serde_json::json!({ "openWizard": false })).ok();
}

/// Stub: no wizard steps (Node: wizard getWizardSteps -> pushWizardSteps).
async fn get_wizard_steps(s: SocketRef) {
    s.emit("pushWizardSteps", &serde_json::json!([])).ok();
}

/// Stub: no wizard UI config (Node: wizard getWizardConfig -> pushWizardUiConfig).
async fn get_wizard_ui_config(s: SocketRef, TryData(_data): TryData<serde_json::Value>) {
    s.emit("pushWizardUiConfig", &serde_json::json!({})).ok();
}

/// No-op: Node calls appearance deleteBackgrounds; Evo has no backgrounds.
async fn delete_background(_s: SocketRef, TryData(_data): TryData<serde_json::Value>) {}

#[derive(Debug, Deserialize)]
struct SetLanguagePayload {
    #[serde(default, rename = "defaultLanguage")]
    #[allow(dead_code)]
    default_language: Option<serde_json::Value>,
    #[serde(default, rename = "disallowReload")]
    #[allow(dead_code)]
    disallow_reload: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct BrowseLibraryPayload {
    #[serde(default)]
    uri: String,
}

async fn browse_library(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<BrowseLibraryPayload>,
) {
    let uri = if payload.uri.is_empty() {
        "music-library"
    } else {
        payload.uri.as_str()
    };

    if uri == "music-library" {
        let resp = mpd::music_library_root_response();
        push_browse_and_store(&s, &state, &resp).await;
        return;
    }

    if uri == "favourites" {
        let resp = mpd::browse_favourites_stub();
        push_browse_and_store(&s, &state, &resp).await;
        return;
    }

    if uri == "playlists" {
        let config = mpd_config(&state);
        match mpd::list_playlists_connected(&config).await {
            Ok(names) => {
                let items: Vec<BrowseItem> = names
                    .into_iter()
                    .map(|name| BrowseItem {
                        item_type: "folder".to_string(),
                        title: name.clone(),
                        uri: format!("playlists/{}", name),
                        service: "mpd".to_string(),
                        artist: None,
                        album: None,
                        duration: None,
                        albumart: Some(
                            "/albumart?sourceicon=music_service/mpd/playlisticon.png".to_string(),
                        ),
                        icon: None,
                        meta: None,
                    })
                    .collect();
                let resp = BrowseResponse {
                    navigation: BrowseNavigation {
                        info: None,
                        prev: BrowsePrev {
                            uri: String::new(),
                        },
                        lists: vec![BrowseList {
                            available_list_views: vec!["list", "grid"],
                            items,
                        }],
                    },
                };
                push_browse_and_store(&s, &state, &resp).await;
            }
            Err(e) => {
                tracing::warn!("browse playlists MPD error: {}", e);
                push_browse_and_store(&s, &state, &mpd::empty_browse_response("music-library")).await;
            }
        }
        return;
    }

    if let Some(playlist_name) = uri.strip_prefix("playlists/") {
        let config = mpd_config(&state);
        match mpd::list_playlist_content_connected(&config, playlist_name).await {
            Ok(uris) => {
                let items: Vec<BrowseItem> = uris
                    .into_iter()
                    .map(|uri| {
                        let title = uri
                            .rsplit('/')
                            .next()
                            .unwrap_or(uri.as_str())
                            .to_string();
                        let albumart = Some(browse_song_albumart_path_only(&uri));
                        BrowseItem {
                            item_type: "song".to_string(),
                            title,
                            uri,
                            service: "mpd".to_string(),
                            artist: None,
                            album: None,
                            duration: None,
                            albumart,
                            icon: None,
                            meta: None,
                        }
                    })
                    .collect();
                let resp = BrowseResponse {
                    navigation: BrowseNavigation {
                        info: None,
                        prev: BrowsePrev {
                            uri: "playlists".to_string(),
                        },
                        lists: vec![BrowseList {
                            available_list_views: vec!["list", "grid"],
                            items,
                        }],
                    },
                };
                push_browse_and_store(&s, &state, &resp).await;
            }
            Err(e) => {
                tracing::warn!("browse playlists/{} MPD error: {}", playlist_name, e);
                push_browse_and_store(&s, &state, &mpd::empty_browse_response("playlists")).await;
            }
        }
        return;
    }

    let config = mpd_config(&state);
    let prev_on_error = if uri.starts_with("music-library/") {
        "music-library"
    } else if uri.starts_with("playlists/") {
        "playlists"
    } else {
        "music-library"
    };
    match mpd::browse_connected(&config, &state.config.music_sources.music_root, uri).await {
        Ok(resp) => {
            push_browse_and_store(&s, &state, &resp).await;
        }
        Err(e) => {
            tracing::warn!("browse {} MPD error: {}", uri, e);
            push_browse_and_store(&s, &state, &mpd::empty_browse_response(prev_on_error)).await;
        }
    }
}

#[derive(Debug, Deserialize)]
struct AddToQueuePayload {
    uri: String,
}

async fn add_to_queue(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<AddToQueuePayload>,
) {
    tracing::info!("addToQueue received uri={:?}", payload.uri);
    if payload.uri.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    if let Err(e) = mpd::add_to_queue_resolved(
        &config,
        &state.config.music_sources.music_root,
        &payload.uri,
    )
    .await
    {
        tracing::warn!("addToQueue MPD error: {}", e);
    }
}

#[derive(Debug, Deserialize)]
struct ReplaceAndPlayPayload {
    #[serde(default)]
    uri: String,
    #[serde(default)]
    #[allow(dead_code)]
    title: String, // Volumio UI sends it; used for playlist name when uri is playlists/Name
}

/// Node `CoreCommandRouter.replaceAndPlay`: clear queue, add uri, play. MPD `add` on a directory adds the subtree.
async fn mpd_replace_and_play_uri(state: &AppState, uri: &str) {
    let config = mpd_config(state);
    let is_playlist = uri.starts_with("playlists/") && !uri.contains("://");
    if is_playlist {
        let name = uri.strip_prefix("playlists/").unwrap_or(uri).to_string();
        if let Err(e) = mpd::load_playlist_connected(&config, &name).await {
            tracing::warn!("replaceAndPlay (playlist) MPD error: {}", e);
        }
    } else if let Err(e) = mpd::replace_and_play_resolved(
        &config,
        &state.config.music_sources.music_root,
        uri,
    )
    .await
    {
        tracing::warn!("replaceAndPlay MPD error: {}", e);
    }
}

async fn replace_and_play(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<ReplaceAndPlayPayload>,
) {
    tracing::info!("replaceAndPlay received uri={:?} title={:?}", payload.uri, payload.title);
    let uri = payload.uri.trim();
    if uri.is_empty() {
        return;
    }
    mpd_replace_and_play_uri(&state, uri).await;
}

#[derive(Debug, Deserialize)]
struct GoToPayload {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    artist: String,
    #[serde(default)]
    album: String,
}

async fn go_to(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<GoToPayload>,
) {
    let config = mpd_config(&state);
    let uri = if payload.r#type == "artist" && !payload.value.is_empty() {
        format!("artists://{}", payload.value)
    } else if payload.r#type == "album" && !payload.artist.is_empty() && !payload.album.is_empty() {
        format!(
            "albums://{}/{}",
            payload.artist,
            payload.album
        )
    } else {
        return;
    };
    match mpd::browse_connected(&config, &state.config.music_sources.music_root, &uri).await {
        Ok(resp) => push_browse_and_store(&s, &state, &resp).await,
        Err(e) => tracing::warn!("goTo {} MPD error: {}", uri, e),
    }
}

#[derive(Debug, Deserialize)]
struct ReplaceAndPlayCuePayload {
    #[serde(default)]
    uri: String,
    #[serde(default)]
    #[allow(dead_code)]
    number: Option<u32>, // CUE track index; we don't support CUE sheets, treat as single uri
    #[serde(default)]
    #[allow(dead_code)]
    service: Option<String>,
}

async fn replace_and_play_cue(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<ReplaceAndPlayCuePayload>,
) {
    let uri = payload.uri.trim();
    if uri.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    // Volumio: clear queue then add CUE entry; we have no CUE support -> clear + add uri (no play).
    if let Err(e) = mpd::clear_and_add_connected(&config, uri).await {
        tracing::warn!("replaceAndPlayCue MPD error: {}", e);
    }
}

#[derive(Debug, Deserialize)]
struct AddPlayCuePayload {
    #[serde(default)]
    uri: String,
    #[serde(default)]
    #[allow(dead_code)]
    number: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    service: Option<String>,
}

async fn add_play_cue(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<AddPlayCuePayload>,
) {
    let uri = payload.uri.trim();
    if uri.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    // Volumio: add CUE entry to queue; we have no CUE support -> add single uri to queue.
    if let Err(e) = mpd::add_to_queue_connected(&config, uri).await {
        tracing::warn!("addPlayCue MPD error: {}", e);
    }
}

#[derive(Debug, Deserialize)]
struct ListItemUri {
    #[serde(default)]
    uri: String,
}

#[derive(Debug, Deserialize)]
struct PlayItemsListPayload {
    /// Browse row when playing a folder from the inline button: only `item` is set (Node → `replaceAndPlay` → `addQueueItems(data.item)`).
    #[serde(default)]
    item: Option<ListItemUri>,
    #[serde(default)]
    list: Vec<ListItemUri>,
    #[serde(default)]
    index: Option<u32>,
}

async fn play_items_list(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<PlayItemsListPayload>,
) {
    // Node `playItemsList` → `replaceAndPlay(data)`; branch `data.item` only (folder / single entry from browse).
    if payload.list.is_empty() {
        if let Some(ref it) = payload.item {
            let uri = it.uri.trim();
            if !uri.is_empty() {
                tracing::info!("playItemsList (item-only) uri={:?}", uri);
                mpd_replace_and_play_uri(&state, uri).await;
            }
        }
        return;
    }

    let index = match payload.index {
        Some(i) => i as usize,
        None => return,
    };
    let list = &payload.list;
    let uris: Vec<String> = list
        .iter()
        .map(|e| e.uri.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if uris.is_empty() || index >= uris.len() {
        return;
    }
    let config = mpd_config(&state);
    if let Err(e) = mpd::play_items_list_connected(&config, &uris, index).await {
        tracing::warn!("playItemsList MPD error: {}", e);
    }
}

#[derive(Debug, Deserialize)]
struct AddPlayPayload {
    uri: String,
}

async fn add_play(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<AddPlayPayload>,
) {
    if payload.uri.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    if let Err(e) = mpd::add_play_append_resolved(
        &config,
        &state.config.music_sources.music_root,
        &payload.uri,
    )
    .await
    {
        tracing::warn!("addPlay MPD error: {}", e);
    }
}

#[derive(Debug, Deserialize)]
struct RemoveFromQueuePayload {
    value: u32,
}

async fn remove_from_queue(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<RemoveFromQueuePayload>,
) {
    // UI may send 1-based index; MPD is 0-based.
    let pos = payload.value.saturating_sub(1);
    let config = mpd_config(&state);
    if let Err(e) = mpd::remove_from_queue_connected(&config, pos).await {
        tracing::warn!("removeFromQueue MPD error: {}", e);
    }
}

/// Stock Volumio drives volume with **ALSA `amixer`** (`volumecontrol.js`); Evo matches that, then
/// **MPD `setvol`** so `getState` / `pushState` stay aligned when MPD accepts the command.
async fn apply_volume_to_system(state: &AppState, v: u8) {
    let alsa = state.alsa.read().await.clone();
    let pb = state.playback.read().await.clone();

    if pb.mixer_type == "None" {
        tracing::debug!("volume: mixer_type None, ignoring (Node: disableVolumeControl)");
        return;
    }

    let v = pb.clamp_volume_percent(v);

    let log_curve = pb
        .volumecurvemode
        .trim()
        .eq_ignore_ascii_case("logarithmic");
    let use_alsa = pb.mixer_type != "Software"
        || alsa::alsa_softmaster_control_present(&alsa);
    if use_alsa {
        let alsa_c = alsa.clone();
        let mt = pb.mixer_type.clone();
        let mn = pb.mixer.clone();
        match tokio::task::spawn_blocking(move || {
            alsa::set_system_volume_percent(&alsa_c, &mt, &mn, log_curve, v)
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!("ALSA volume (amixer, Node alsavolume path): {}", e),
            Err(e) => tracing::warn!("ALSA volume task join: {}", e),
        }
    }

    let config = mpd_config(state);
    if let Err(e) = mpd::run_command_connected(&config, "volume", Some(v), None, None, None).await {
        tracing::warn!("MPD setvol (state sync): {}", e);
    }
}

async fn volume(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<serde_json::Value>,
) {
    let vol = payload
        .as_u64()
        .or_else(|| {
            if payload.as_str() == Some("mute") {
                Some(0)
            } else {
                None
            }
        })
        .and_then(|v| u8::try_from(v).ok());
    let Some(v) = vol else {
        return;
    };
    apply_volume_to_system(&state, v).await;
}

#[derive(Debug, Deserialize)]
struct PlayPayload {
    #[serde(default)]
    value: Option<i64>,
}

async fn play(
    _s: SocketRef,
    State(state): State<AppState>,
    payload: TryData<PlayPayload>,
) {
    let position = payload.as_ref().ok().and_then(|p| p.value);
    let config = mpd_config(&state);
    let _ = mpd::run_command_connected(&config, "play", None, position, None, None).await;
}

async fn pause(_s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    let _ = mpd::run_command_connected(&config, "pause", None, None, None, None).await;
}

async fn toggle(_s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    let _ = mpd::run_command_connected(&config, "toggle", None, None, None, None).await;
}

async fn stop(_s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    let _ = mpd::run_command_connected(&config, "stop", None, None, None, None).await;
}

async fn next(_s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    let _ = mpd::run_command_connected(&config, "next", None, None, None, None).await;
}

async fn prev(_s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    let _ = mpd::run_command_connected(&config, "prev", None, None, None, None).await;
}

async fn seek(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<serde_json::Value>,
) {
    let position = payload.as_i64().or_else(|| payload.as_u64().map(|u| u as i64));
    if let Some(pos) = position {
        let config = mpd_config(&state);
        let _ = mpd::run_command_connected(&config, "seek", None, Some(pos), None, None).await;
    }
}

#[derive(Debug, Deserialize)]
struct SetRandomPayload {
    value: bool,
}

async fn set_random(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<SetRandomPayload>,
) {
    let config = mpd_config(&state);
    let _ = mpd::run_command_connected(&config, "random", None, None, None, Some(payload.value)).await;
}

#[derive(Debug, Deserialize)]
struct SetRepeatPayload {
    value: bool,
    #[serde(default)]
    #[allow(dead_code)]
    repeat_single: bool,
}

async fn set_repeat(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<SetRepeatPayload>,
) {
    let config = mpd_config(&state);
    let _ = mpd::run_command_connected(
        &config,
        "repeat",
        None,
        None,
        Some(payload.value),
        None,
    )
    .await;
}

async fn clear_queue(_s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    let _ = mpd::run_command_connected(&config, "clearQueue", None, None, None, None).await;
}

async fn get_installed_plugins(s: SocketRef, State(state): State<AppState>) {
    let plugins = super::v1::list_installed_plugins(&state).await;
    s.emit("pushInstalledPlugins", &plugins).ok();
}

/// No plugin store in Evo. Stock Volumio UI requires at least one category so
/// `selectedCategory = categories[0]` and `selectedCategory.plugins` are valid.
async fn get_available_plugins(s: SocketRef) {
    let payload = serde_json::json!({
        "categories": [{
            "name": "evo",
            "prettyName": "",
            "plugins": []
        }]
    });
    s.emit("pushAvailablePlugins", &payload).ok();
}

#[derive(Debug, Deserialize)]
struct MoveQueuePayload {
    from: u32,
    to: u32,
}

async fn move_queue(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<MoveQueuePayload>,
) {
    let config = mpd_config(&state);
    match mpd::move_queue_connected(&config, payload.from, payload.to).await {
        Ok(()) => {
            if let Ok(q) = mpd::get_queue_connected(&config).await {
                s.emit("pushQueue", &serde_json::json!({ "queue": q })).ok();
            }
        }
        Err(e) => tracing::warn!("moveQueue MPD error: {}", e),
    }
}

#[derive(Debug, Deserialize)]
struct PlayNextPayload {
    uri: String,
}

async fn play_next(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<PlayNextPayload>,
) {
    if payload.uri.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    match mpd::play_next_resolved(
        &config,
        &state.config.music_sources.music_root,
        &payload.uri,
    )
    .await
    {
        Ok(()) => {
            if let Ok(q) = mpd::get_queue_connected(&config).await {
                s.emit("pushQueue", &serde_json::json!({ "queue": q })).ok();
            }
            let master = read_master_volume_percent(&state).await;
            if let Ok(st) =
                mpd::get_state_connected(&config, &state.config.music_sources.music_root, master)
                    .await
            {
                s.emit("pushState", &st).ok();
            }
        }
        Err(e) => tracing::warn!("playNext MPD error: {}", e),
    }
}

// ---- Playlist manager ----

#[derive(Debug, Deserialize)]
struct PlaylistNamePayload {
    name: String,
}

#[derive(Debug, Deserialize)]
struct AddToPlaylistPayload {
    name: String,
    #[serde(default)]
    service: String,
    uri: String,
    #[serde(default)]
    album_title: String,
}

#[derive(Debug, Deserialize)]
struct RemoveFromPlaylistPayload {
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    service: String,
    uri: String,
}

async fn get_playlist_content(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<PlaylistNamePayload>,
) {
    if payload.name.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    match mpd::list_playlist_content_connected(&config, &payload.name).await {
        Ok(uris) => {
            let items: Vec<serde_json::Value> = uris
                .into_iter()
                .map(|uri| {
                    let title = uri
                        .rsplit('/')
                        .next()
                        .unwrap_or(uri.as_str())
                        .to_string();
                    serde_json::json!({
                        "service": "mpd",
                        "uri": uri,
                        "name": title,
                        "title": title
                    })
                })
                .collect();
            let payload_out = serde_json::json!({ "name": payload.name, "lists": [ items ] });
            s.emit("pushPlaylistContent", &payload_out).ok();
        }
        Err(e) => tracing::warn!("getPlaylistContent MPD error: {}", e),
    }
}

async fn list_playlist(s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    match mpd::list_playlists_connected(&config).await {
        Ok(names) => {
            s.emit("pushListPlaylist", &names).ok();
        }
        Err(e) => tracing::warn!("listPlaylist MPD error: {}", e),
    }
}

async fn play_playlist(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<PlaylistNamePayload>,
) {
    if payload.name.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    match mpd::load_playlist_connected(&config, &payload.name).await {
        Ok(()) => {
            s.emit("pushPlayPlaylist", &serde_json::json!({ "name": payload.name }))
                .ok();
        }
        Err(e) => tracing::warn!("playPlaylist MPD error: {}", e),
    }
}

async fn save_queue_to_playlist(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<PlaylistNamePayload>,
) {
    if payload.name.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    match mpd::save_queue_to_playlist_connected(&config, &payload.name).await {
        Ok(()) => {
            s.emit("pushSaveQueueToPlaylist", &serde_json::json!({ "name": payload.name }))
                .ok();
        }
        Err(e) => tracing::warn!("saveQueueToPlaylist MPD error: {}", e),
    }
}

async fn create_playlist(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<PlaylistNamePayload>,
) {
    if payload.name.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    match mpd::create_playlist_connected(&config, &payload.name).await {
        Ok(()) => {
            s.emit(
                "pushCreatePlaylist",
                &serde_json::json!({ "success": true, "name": payload.name }),
            )
            .ok();
            if let Ok(names) = mpd::list_playlists_connected(&config).await {
                s.emit("pushListPlaylist", &names).ok();
            }
        }
        Err(e) => {
            tracing::warn!("createPlaylist MPD error: {}", e);
            s.emit(
                "pushCreatePlaylist",
                &serde_json::json!({ "success": false, "name": payload.name }),
            )
            .ok();
        }
    }
}

async fn delete_playlist(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<PlaylistNamePayload>,
) {
    if payload.name.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    match mpd::delete_playlist_connected(&config, &payload.name).await {
        Ok(()) => {
            if let Ok(names) = mpd::list_playlists_connected(&config).await {
                s.emit("pushListPlaylist", &names).ok();
                let items: Vec<BrowseItem> = names
                    .into_iter()
                    .map(|name| BrowseItem {
                        item_type: "folder".to_string(),
                        title: name.clone(),
                        uri: format!("playlists/{}", name),
                        service: "mpd".to_string(),
                        artist: None,
                        album: None,
                        duration: None,
                        albumart: Some(
                            "/albumart?sourceicon=music_service/mpd/playlisticon.png".to_string(),
                        ),
                        icon: None,
                        meta: None,
                    })
                    .collect();
                let resp = BrowseResponse {
                    navigation: BrowseNavigation {
                        info: None,
                        prev: BrowsePrev {
                            uri: String::new(),
                        },
                        lists: vec![BrowseList {
                            available_list_views: vec!["list", "grid"],
                            items,
                        }],
                    },
                };
                push_browse_and_store(&s, &state, &resp).await;
            }
        }
        Err(e) => tracing::warn!("deletePlaylist MPD error: {}", e),
    }
}

async fn add_to_playlist(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<AddToPlaylistPayload>,
) {
    if payload.name.is_empty() || payload.uri.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    match mpd::add_to_playlist_connected(&config, &payload.name, &payload.uri).await {
        Ok(()) => {
            if let Ok(names) = mpd::list_playlists_connected(&config).await {
                s.emit("pushListPlaylist", &names).ok();
            }
            s.emit(
                "pushAddToPlaylist",
                &serde_json::json!({
                    "name": payload.name,
                    "service": if payload.service.is_empty() { "mpd" } else { payload.service.as_str() },
                    "uri": payload.uri,
                    "albumTitle": payload.album_title
                }),
            )
            .ok();
        }
        Err(e) => tracing::warn!("addToPlaylist MPD error: {}", e),
    }
}

async fn remove_from_playlist(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<RemoveFromPlaylistPayload>,
) {
    if payload.name.is_empty() || payload.uri.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    let uris = match mpd::list_playlist_content_connected(&config, &payload.name).await {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!("removeFromPlaylist list content MPD error: {}", e);
            return;
        }
    };
    let position = uris
        .iter()
        .position(|u| u == &payload.uri)
        .map(|p| p as u32);
    let Some(pos) = position else {
        tracing::warn!("removeFromPlaylist: uri not found in playlist");
        return;
    };
    match mpd::remove_from_playlist_connected(&config, &payload.name, pos).await {
        Ok(()) => {
            if let Ok(updated) = mpd::list_playlist_content_connected(&config, &payload.name).await
            {
                let items: Vec<BrowseItem> = updated
                    .into_iter()
                    .map(|uri| {
                        let title = uri
                            .rsplit('/')
                            .next()
                            .unwrap_or(uri.as_str())
                            .to_string();
                        let albumart = Some(browse_song_albumart_path_only(&uri));
                        BrowseItem {
                            item_type: "song".to_string(),
                            title,
                            uri,
                            service: "mpd".to_string(),
                            artist: None,
                            album: None,
                            duration: None,
                            albumart,
                            icon: None,
                            meta: None,
                        }
                    })
                    .collect();
                let resp = BrowseResponse {
                    navigation: BrowseNavigation {
                        info: None,
                        prev: BrowsePrev {
                            uri: "playlists".to_string(),
                        },
                        lists: vec![BrowseList {
                            available_list_views: vec!["list", "grid"],
                            items,
                        }],
                    },
                };
                push_browse_and_store(&s, &state, &resp).await;
            }
        }
        Err(e) => tracing::warn!("removeFromPlaylist MPD error: {}", e),
    }
}

async fn enqueue(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<PlaylistNamePayload>,
) {
    if payload.name.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    match mpd::enqueue_playlist_connected(&config, &payload.name).await {
        Ok(()) => {
            s.emit("pushEnqueue", &serde_json::json!({ "name": payload.name }))
                .ok();
            if let Ok(q) = mpd::get_queue_connected(&config).await {
                s.emit("pushQueue", &serde_json::json!({ "queue": q })).ok();
            }
        }
        Err(e) => tracing::warn!("enqueue MPD error: {}", e),
    }
}

/// GetTrackInfo: pass-through so UI can refresh track info; emit same data as pushGetTrackInfo.
async fn get_track_info(s: SocketRef, Data(payload): Data<serde_json::Value>) {
    s.emit("pushGetTrackInfo", &payload).ok();
}

#[derive(Debug, Deserialize)]
struct CallMethodPayload {
    endpoint: Option<String>,
    method: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    data: serde_json::Value,
}

/// callMethod: albumart clear; ALSA / MPD playback saves (stock Playback Options sections).
async fn call_method(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<CallMethodPayload>,
) {
    if payload.endpoint.as_deref() == Some("miscellanea/albumart")
        && payload.method.as_deref() == Some("clearAlbumartCache")
    {
        state.send_clear_albumart_cache();
        return;
    }

    if payload.endpoint.as_deref() == Some("audio_interface/alsa_controller")
        && payload.method.as_deref() == Some("saveAlsaOptions")
    {
        let mut guard = state.alsa.write().await;
        match guard.apply_save_payload(&payload.data) {
            Ok(()) => tracing::info!("ALSA saveAlsaOptions: {:?}", *guard),
            Err(e) => tracing::warn!("saveAlsaOptions: {}", e),
        }
        drop(guard);
        let alsa = state.alsa.read().await.clone();
        {
            let mut pb = state.playback.write().await;
            pb.apply_volume_sanity(&alsa);
            if let Err(e) = pb.save() {
                tracing::warn!("save playback after ALSA volume sanity: {}", e);
            }
        }
        let pb = state.playback.read().await.clone();
        if let Err(e) = pb.write_fragment_and_restart_mpd(&alsa).await {
            tracing::warn!("MPD fragment after ALSA save: {}", e);
        }
        get_output_devices(s.clone(), State(state.clone())).await;
        emit_playback_options_ui(&s, &state).await;
        return;
    }

    if payload.endpoint.as_deref() == Some("music_service/mpd")
        && payload.method.as_deref() == Some("savePlaybackOptions")
    {
        let mut pb = state.playback.write().await;
        pb.merge_playback_section(&payload.data);
        if let Err(e) = pb.save() {
            tracing::warn!("save playback state: {}", e);
        }
        let pb_clone = pb.clone();
        drop(pb);
        let alsa = state.alsa.read().await.clone();
        if let Err(e) = pb_clone.write_fragment_and_restart_mpd(&alsa).await {
            tracing::warn!("savePlaybackOptions MPD: {}", e);
        }
        emit_playback_options_ui(&s, &state).await;
        return;
    }

    if payload.endpoint.as_deref() == Some("audio_interface/alsa_controller")
        && payload.method.as_deref() == Some("saveVolumeOptions")
    {
        let alsa = state.alsa.read().await.clone();
        let prev = state.playback.read().await.clone();
        let mut pb = state.playback.write().await;
        pb.merge_volume_section(&payload.data);
        pb.apply_volume_sanity(&alsa);
        if let Err(e) = pb.save() {
            tracing::warn!("save playback state: {}", e);
        }
        let after = pb.clone();
        drop(pb);

        let log_curve = after
            .volumecurvemode
            .trim()
            .eq_ignore_ascii_case("logarithmic");

        // Hardware → Software: ALSA before fragment, restart MPD, `volume`, then open gain stage:
        // SoftMaster path — HW ramp; no SoftMaster — HW 100% only **after** setvol (never while MPD
        // still used HW volume — avoids full-scale burst).
        let hw_to_sw = prev.mixer_type == "Hardware" && after.mixer_type == "Software";
        if hw_to_sw {
            let hw_sw_t0 = Instant::now();
            alsa::mixer_hw_sw_trace(
                Some(hw_sw_t0),
                "async_begin",
                "saveVolumeOptions Hardware→Software (single timeline, ms)",
            );
            let soft = alsa::alsa_softmaster_control_present(&alsa);
            let prev_hw = prev.mixer.clone();
            let alsa_c = alsa.clone();
            let prev_hw_arg = prev_hw.clone();
            let handoff = tokio::task::spawn_blocking(move || {
                alsa::transition_hardware_to_software_before_mpd(
                    &alsa_c,
                    &prev_hw_arg,
                    log_curve,
                    Some(hw_sw_t0),
                )
            })
            .await;
            match handoff {
                Ok(Ok(pct)) => {
                    let capped = after.clamp_volume_percent(pct);
                    alsa::mixer_hw_sw_trace(
                        Some(hw_sw_t0),
                        "async_after_phase1_blocking",
                        "before_mpd spawn_blocking returned",
                    );
                    if let Err(e) = after.write_fragment_and_restart_mpd(&alsa).await {
                        tracing::warn!("saveVolumeOptions MPD: {}", e);
                    }
                    alsa::mixer_hw_sw_trace(
                        Some(hw_sw_t0),
                        "async_after_fragment_restart",
                        "write_fragment_and_restart_mpd finished",
                    );
                    let config = mpd_config(&state);
                    if let Err(e) = mpd::run_command_connected(
                        &config,
                        "volume",
                        Some(capped),
                        None,
                        None,
                        None,
                    )
                    .await
                    {
                        tracing::warn!("saveVolumeOptions MPD setvol (hardware→software): {}", e);
                    }
                    alsa::mixer_hw_sw_trace(
                        Some(hw_sw_t0),
                        "async_after_setvol",
                        &format!("mpd_volume_pct={capped}"),
                    );
                    if soft {
                        let alsa_c2 = alsa.clone();
                        let prev_hw2 = prev_hw.clone();
                        let finish = tokio::task::spawn_blocking(move || {
                            alsa::transition_hardware_to_software_after_mpd_softmaster(
                                &alsa_c2,
                                &prev_hw2,
                                log_curve,
                                Some(hw_sw_t0),
                            )
                        })
                        .await;
                        match finish {
                            Ok(Ok(())) => {}
                            Ok(Err(e)) => tracing::warn!(
                                "saveVolumeOptions ALSA (hardware→software after MPD): {}",
                                e
                            ),
                            Err(e) => tracing::warn!(
                                "saveVolumeOptions task (hardware→software after MPD): {}",
                                e
                            ),
                        }
                    } else {
                        let alsa_c2 = alsa.clone();
                        let prev_hw2 = prev_hw.clone();
                        let finish = tokio::task::spawn_blocking(move || {
                            alsa::transition_hardware_to_software_after_mpd_no_softmaster(
                                &alsa_c2,
                                &prev_hw2,
                                log_curve,
                                Some(hw_sw_t0),
                            )
                        })
                        .await;
                        match finish {
                            Ok(Ok(())) => {}
                            Ok(Err(e)) => tracing::warn!(
                                "saveVolumeOptions ALSA (hardware→software no SoftMaster after MPD): {}",
                                e
                            ),
                            Err(e) => tracing::warn!(
                                "saveVolumeOptions task (hardware→software no SoftMaster): {}",
                                e
                            ),
                        }
                    }
                    alsa::mixer_hw_sw_trace(
                        Some(hw_sw_t0),
                        "async_end",
                        "saveVolumeOptions hw→sw finished",
                    );
                }
                Ok(Err(e)) => {
                    tracing::warn!("saveVolumeOptions ALSA (hardware→software): {}", e);
                    if let Err(e2) = after.write_fragment_and_restart_mpd(&alsa).await {
                        tracing::warn!("saveVolumeOptions MPD: {}", e2);
                    }
                }
                Err(e) => {
                    tracing::warn!("saveVolumeOptions task (hardware→software): {}", e);
                    if let Err(e2) = after.write_fragment_and_restart_mpd(&alsa).await {
                        tracing::warn!("saveVolumeOptions MPD: {}", e2);
                    }
                }
            }
            emit_playback_options_ui(&s, &state).await;
            return;
        }

        // Software → Hardware: ALSA hand-off, then MPD `volume`, then fragment.
        let sw_to_hw = prev.mixer_type == "Software" && after.mixer_type == "Hardware";
        if sw_to_hw {
            let soft = alsa::alsa_softmaster_control_present(&alsa);
            let mpd_vol_if_no_softmaster = if !soft {
                let config = mpd_config(&state);
                mpd::get_state_connected(
                    &config,
                    &state.config.music_sources.music_root,
                    None,
                )
                .await
                .ok()
                .and_then(|s| s.volume)
            } else {
                None
            };
            let alsa_c = alsa.clone();
            let nh = after.mixer.clone();
            let handoff = tokio::task::spawn_blocking(move || {
                alsa::transition_software_to_hardware_handoff(
                    &alsa_c,
                    &nh,
                    log_curve,
                    mpd_vol_if_no_softmaster,
                )
            })
            .await;
            match handoff {
                Ok(Ok(pct)) => {
                    let capped = after.clamp_volume_percent(pct);
                    let config = mpd_config(&state);
                    if let Err(e) = mpd::run_command_connected(
                        &config,
                        "volume",
                        Some(capped),
                        None,
                        None,
                        None,
                    )
                    .await
                    {
                        tracing::warn!("saveVolumeOptions MPD setvol (software→hardware): {}", e);
                    }
                }
                Ok(Err(e)) => tracing::warn!("saveVolumeOptions ALSA (software→hardware): {}", e),
                Err(e) => tracing::warn!("saveVolumeOptions task (software→hardware): {}", e),
            }
        }

        if let Err(e) = after.write_fragment_and_restart_mpd(&alsa).await {
            tracing::warn!("saveVolumeOptions MPD: {}", e);
        }
        emit_playback_options_ui(&s, &state).await;
        return;
    }

    if payload.endpoint.as_deref() == Some("audio_interface/alsa_controller")
        && payload.method.as_deref() == Some("saveResamplingOpts")
    {
        let mut pb = state.playback.write().await;
        pb.merge_resampling_section(&payload.data);
        if let Err(e) = pb.save() {
            tracing::warn!("save playback state: {}", e);
        }
        let pb_clone = pb.clone();
        drop(pb);
        let alsa = state.alsa.read().await.clone();
        if let Err(e) = pb_clone.write_fragment_and_restart_mpd(&alsa).await {
            tracing::warn!("saveResamplingOpts MPD: {}", e);
        }
        emit_playback_options_ui(&s, &state).await;
    }
}

async fn pinger(s: SocketRef, Data(payload): Data<serde_json::Value>) {
    s.emit("ponger", &payload).ok();
}

#[derive(Debug, Deserialize)]
struct SetConsumePayload {
    value: bool,
}

async fn set_consume(
    s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<SetConsumePayload>,
) {
    let config = mpd_config(&state);
    match mpd::set_consume_connected(&config, payload.value).await {
        Ok(()) => {
            s.emit("pushSetConsume", &serde_json::json!({ "value": payload.value }))
                .ok();
        }
        Err(e) => tracing::warn!("setConsume MPD error: {}", e),
    }
}

async fn get_last_pushed_browse_library(s: SocketRef, State(state): State<AppState>) {
    if let Some(val) = state.get_last_browse().await {
        s.emit("pushBrowseLibrary", &val).ok();
    }
}

async fn mute(_s: SocketRef, State(state): State<AppState>) {
    apply_volume_to_system(&state, 0).await;
}

async fn unmute(_s: SocketRef, State(state): State<AppState>) {
    // Restore to 80% if no pre-mute volume stored (stub; Node uses premutevolume)
    apply_volume_to_system(&state, 80).await;
}

async fn rescan_db(_s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    if let Err(e) = mpd::rescan_connected(&config, None).await {
        tracing::warn!("rescanDb MPD error: {}", e);
    }
}

#[derive(Debug, Deserialize, Default)]
struct UpdateDbPayload {
    #[serde(default)]
    uri: String,
}

async fn update_db(_s: SocketRef, State(state): State<AppState>, Data(payload): Data<UpdateDbPayload>) {
    let config = mpd_config(&state);
    let path = payload.uri.trim();
    let path_opt = if path.is_empty() { None } else { Some(path) };
    if let Err(e) = mpd::update_connected(&config, path_opt).await {
        tracing::warn!("updateDb MPD error: {}", e);
    }
}

/// Poll MPD periodically and broadcast pushState/pushQueue to all Socket.IO clients in the default namespace.
/// Run this in a spawned task so the UI updates when state or queue changes (e.g. from another client or MPD).
pub async fn push_state_queue_loop(state: AppState, io: socketioxide::SocketIo) {
    let config = mpd_config(&state);
    let music_root = state.config.music_sources.music_root.clone();
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        let master = read_master_volume_percent(&state).await;
        if let Ok(s) = mpd::get_state_connected(&config, &music_root, master).await {
            if io.emit("pushState", &s).await.is_err() {
                tracing::debug!("pushState broadcast error (connection closed?)");
            }
        }
        if let Ok(items) = mpd::get_queue_connected(&config).await {
            let payload = serde_json::json!({ "queue": items });
            if io.emit("pushQueue", &payload).await.is_err() {
                tracing::debug!("pushQueue broadcast error (connection closed?)");
            }
        }
    }
}
