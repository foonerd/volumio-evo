//! Socket.IO adapter: same event names as Node backend so the existing UI works.
//! Maps getState/getQueue/browseLibrary/addToQueue/addPlay/volume/transport to MPD.

use crate::config::MUSIC_SOURCE_NAMES;
use crate::mpd::{
    self, BrowseItem, BrowseList, BrowseNavigation, BrowsePrev, BrowseResponse, MpdConfig,
};
use serde::Deserialize;
use socketioxide::extract::{Data, SocketRef, State, TryData};

use super::AppState;

fn mpd_config(state: &AppState) -> MpdConfig {
    MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    }
}

/// Emit pushBrowseLibrary and store in state for getLastPushedBrowseLibrary.
async fn push_browse_and_store(s: &SocketRef, state: &AppState, resp: &BrowseResponse) {
    s.emit("pushBrowseLibrary", resp).ok();
    if let Ok(v) = serde_json::to_value(resp) {
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
}

async fn get_state(s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    match mpd::get_state_connected(&config).await {
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
        let items: Vec<BrowseItem> = MUSIC_SOURCE_NAMES
            .iter()
            .map(|(name, title)| BrowseItem {
                item_type: "folder".to_string(),
                title: title.to_string(),
                uri: format!("music-library/{}", name),
                service: "mpd".to_string(),
                artist: None,
                album: None,
                duration: None,
            })
            .collect();
        let resp = BrowseResponse {
            navigation: BrowseNavigation {
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
                    })
                    .collect();
                let resp = BrowseResponse {
                    navigation: BrowseNavigation {
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
            Err(e) => tracing::warn!("browse playlists MPD error: {}", e),
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
                        BrowseItem {
                            item_type: "song".to_string(),
                            title,
                            uri,
                            service: "mpd".to_string(),
                            artist: None,
                            album: None,
                            duration: None,
                        }
                    })
                    .collect();
                let resp = BrowseResponse {
                    navigation: BrowseNavigation {
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
            Err(e) => tracing::warn!("browse playlists/{} MPD error: {}", playlist_name, e),
        }
        return;
    }

    let config = mpd_config(&state);
    match mpd::browse_connected(&config, uri).await {
        Ok(resp) => {
            push_browse_and_store(&s, &state, &resp).await;
        }
        Err(e) => {
            tracing::warn!("browse {} MPD error: {}", uri, e);
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
    if payload.uri.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    if let Err(e) = mpd::add_to_queue_connected(&config, &payload.uri).await {
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

async fn replace_and_play(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<ReplaceAndPlayPayload>,
) {
    let uri = payload.uri.trim();
    if uri.is_empty() {
        return;
    }
    let config = mpd_config(&state);
    // Volumio: "playlists/Name" (no ://) -> load playlist and play; else clear + add uri + play.
    let is_playlist = uri.starts_with("playlists/") && !uri.contains("://");
    if is_playlist {
        let name = uri.strip_prefix("playlists/").unwrap_or(uri).to_string();
        if let Err(e) = mpd::load_playlist_connected(&config, &name).await {
            tracing::warn!("replaceAndPlay (playlist) MPD error: {}", e);
        }
    } else if let Err(e) = mpd::add_play_connected(&config, uri).await {
        tracing::warn!("replaceAndPlay MPD error: {}", e);
    }
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
    match mpd::browse_connected(&config, &uri).await {
        Ok(resp) => push_browse_and_store(&s, &state, &resp).await,
        Err(e) => tracing::warn!("goTo {} MPD error: {}", uri, e),
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
    if let Err(e) = mpd::add_play_connected(&config, &payload.uri).await {
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
    if let Some(v) = vol {
        let config = mpd_config(&state);
        let _ = mpd::run_command_connected(&config, "volume", Some(v), None, None, None).await;
    }
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
    match mpd::play_next_connected(&config, &payload.uri).await {
        Ok(()) => {
            if let Ok(q) = mpd::get_queue_connected(&config).await {
                s.emit("pushQueue", &serde_json::json!({ "queue": q })).ok();
            }
            if let Ok(st) = mpd::get_state_connected(&config).await {
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
                    })
                    .collect();
                let resp = BrowseResponse {
                    navigation: BrowseNavigation {
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
                        BrowseItem {
                            item_type: "song".to_string(),
                            title,
                            uri,
                            service: "mpd".to_string(),
                            artist: None,
                            album: None,
                            duration: None,
                        }
                    })
                    .collect();
                let resp = BrowseResponse {
                    navigation: BrowseNavigation {
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

/// callMethod: handle miscellanea/albumart clearAlbumartCache (trigger broadcast so clients refresh).
async fn call_method(
    _s: SocketRef,
    State(state): State<AppState>,
    Data(payload): Data<CallMethodPayload>,
) {
    if payload.endpoint.as_deref() == Some("miscellanea/albumart")
        && payload.method.as_deref() == Some("clearAlbumartCache")
    {
        state.send_clear_albumart_cache();
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
    let config = mpd_config(&state);
    let _ = mpd::run_command_connected(&config, "volume", Some(0), None, None, None).await;
}

async fn unmute(_s: SocketRef, State(state): State<AppState>) {
    let config = mpd_config(&state);
    // Restore to 80% if no pre-mute volume stored
    let _ = mpd::run_command_connected(&config, "volume", Some(80), None, None, None).await;
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
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        if let Ok(s) = mpd::get_state_connected(&config).await {
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
