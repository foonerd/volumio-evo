//! Socket.IO adapter: same event names as Node backend so the existing UI works.
//! Maps getState/getQueue/browseLibrary/addToQueue/addPlay/volume/transport to MPD.

use crate::config::{Config, MUSIC_SOURCE_NAMES};
use crate::mpd::{
    self, BrowseItem, BrowseList, BrowseNavigation, BrowsePrev, BrowseResponse, MpdConfig,
};
use serde::Deserialize;
use socketioxide::extract::{Data, SocketRef, State, TryData};
use std::sync::Arc;

pub type AppState = Arc<Config>;

fn mpd_config(state: &AppState) -> MpdConfig {
    MpdConfig {
        host: state.mpd_host.clone(),
        port: state.mpd_port,
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
        s.emit("pushBrowseLibrary", &resp).ok();
        return;
    }

    let config = mpd_config(&state);
    match mpd::browse_connected(&config, uri).await {
        Ok(resp) => {
            s.emit("pushBrowseLibrary", &resp).ok();
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

async fn get_installed_plugins(s: SocketRef) {
    let empty: Vec<serde_json::Value> = vec![];
    s.emit("pushInstalledPlugins", &empty).ok();
}
