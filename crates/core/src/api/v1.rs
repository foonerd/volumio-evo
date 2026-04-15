//! Volumio v1 REST API: getState, commands, getQueue, browse, ping, getSystemVersion, getSystemInfo, listplaylists, search, stubs.
//! Mirrors the Node backend so the existing UI works.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::metavolumio::{metavolumio_response, PluginEndpointBody};
use crate::mpd::{self, MpdConfig, VolumioState};

use super::AppState;

pub fn mpd_config_from_app(state: &AppState) -> MpdConfig {
    MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    }
}

/// GET /api/v1/getState
pub async fn get_state(State(state): State<AppState>) -> impl IntoResponse {
    let config = mpd_config_from_app(&state);
    match mpd::get_state_connected(&config, &state.config.music_sources.music_root).await {
        Ok(s) => Json::<VolumioState>(s).into_response(),
        Err(e) => {
            tracing::warn!("getState MPD error: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "MPD unavailable"})),
            )
                .into_response()
        }
    }
}

/// GET /api/v1/commands?cmd=...&volume=...&position=...&value=...&N=...
#[derive(Debug, Deserialize)]
pub struct CommandsQuery {
    pub cmd: Option<String>,
    pub volume: Option<String>,
    pub position: Option<String>,
    pub value: Option<String>,
    /// Position (same as position); Volumio UI sometimes sends N
    #[serde(rename = "N")]
    pub n: Option<String>,
}

pub async fn commands(
    State(state): State<AppState>,
    Query(q): Query<CommandsQuery>,
) -> impl IntoResponse {
    let cmd = match &q.cmd {
        Some(c) => c.as_str(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"Error": "Missing cmd"})),
            )
                .into_response()
        }
    };

    let volume = q
        .volume
        .as_deref()
        .and_then(|s| s.parse::<u8>().ok())
        .or_else(|| if q.volume.as_deref() == Some("mute") { Some(0) } else { None });
    let position = q
        .position
        .or(q.n)
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok());
    let repeat = (cmd == "repeat").then(|| q.value.as_deref() == Some("true"));
    let random = (cmd == "random").then(|| q.value.as_deref() == Some("true"));

    let config = mpd_config_from_app(&state);

    if cmd == "addToQueue" {
        let uri = match &q.value {
            Some(u) if !u.is_empty() => u.as_str(),
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"Error": "addToQueue requires value=uri"})),
                )
                    .into_response()
            }
        };
        return match mpd::add_to_queue_resolved(
            &config,
            &state.config.music_sources.music_root,
            uri,
        )
        .await
        {
            Ok(()) => Json(serde_json::json!({"response": "addToQueue Success"})).into_response(),
            Err(e) => {
                tracing::warn!("addToQueue MPD error: {}", e);
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"error": "MPD unavailable"})),
                )
                    .into_response()
            }
        };
    }
    if cmd == "addPlay" {
        let uri = match &q.value {
            Some(u) if !u.is_empty() => u.as_str(),
            _ => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({"Error": "addPlay requires value=uri"})),
                )
                    .into_response()
            }
        };
        return match mpd::add_play_append_resolved(
            &config,
            &state.config.music_sources.music_root,
            uri,
        )
        .await
        {
            Ok(()) => Json(serde_json::json!({"response": "addPlay Success"})).into_response(),
            Err(e) => {
                tracing::warn!("addPlay MPD error: {}", e);
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"error": "MPD unavailable"})),
                )
                    .into_response()
            }
        };
    }

    match mpd::run_command_connected(&config, cmd, volume, position, repeat, random).await {
        Ok(()) => Json(serde_json::json!({
            "response": cmd.to_string() + " Success"
        }))
        .into_response(),
        Err(e) => {
            tracing::warn!("commands {} MPD error: {}", cmd, e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "MPD unavailable"})),
            )
                .into_response()
        }
    }
}

/// GET /api/v1/getQueue
pub async fn get_queue(State(state): State<AppState>) -> impl IntoResponse {
    let config = mpd_config_from_app(&state);
    match mpd::get_queue_connected(&config).await {
        Ok(items) => Json(serde_json::json!({ "queue": items })).into_response(),
        Err(e) => {
            tracing::warn!("getQueue MPD error: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "MPD unavailable"})),
            )
                .into_response()
        }
    }
}

/// Plugin list entry for getInstalledPlugins (name from .wasm filename stem).
#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub name: String,
}

/// List WASM plugins in plugin_dir (.wasm files; name = filename stem).
pub async fn list_installed_plugins(state: &AppState) -> Vec<PluginInfo> {
    let mut read_dir = match tokio::fs::read_dir(&state.config.plugin_dir).await {
        Ok(rd) => rd,
        Err(_) => return vec![],
    };
    let mut plugins = Vec::new();
    while let Ok(Some(entry)) = read_dir.next_entry().await {
        let name = entry.file_name();
        let s = name.to_string_lossy();
        if s.ends_with(".wasm") {
            let name = entry
                .path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            plugins.push(PluginInfo { name });
        }
    }
    plugins
}

/// GET /api/v1/getInstalledPlugins - list WASM plugins from plugin_dir
pub async fn get_installed_plugins(State(state): State<AppState>) -> impl IntoResponse {
    Json(list_installed_plugins(&state).await)
}

/// POST /api/v1/pluginEndpoint — stock UI metavolumio (album story, credits, artist bio).
pub async fn plugin_endpoint(
    State(state): State<AppState>,
    Json(body): Json<PluginEndpointBody>,
) -> impl IntoResponse {
    Json(metavolumio_response(&state.config, &body).await)
}

/// GET /api/v1/ping - liveness (Node returns "pong")
pub async fn ping() -> &'static str {
    "pong"
}

/// GET /api/v1/getSystemVersion - stub for UI (Node returns systemversion, variant, hardware, os, builddate)
pub async fn get_system_version() -> impl IntoResponse {
    Json(serde_json::json!({
        "systemversion": "4.0",
        "variant": "volumio-evo",
        "hardware": "generic",
        "os": null,
        "builddate": null
    }))
}

/// GET /api/v1/getSystemInfo - stub for UI (Node returns getSystemVersion + hostname, hwUuid, etc.)
pub async fn get_system_info() -> impl IntoResponse {
    Json(serde_json::json!({
        "systemversion": "4.0",
        "variant": "volumio-evo",
        "hardware": "generic",
        "os": null,
        "builddate": null,
        "hostname": "volumio-evo",
        "hwUuid": "evo-stub"
    }))
}

/// GET /api/v1/listplaylists - MPD listplaylists (Node returns array of { name } or similar)
pub async fn list_playlists(State(state): State<AppState>) -> impl IntoResponse {
    let config = mpd_config_from_app(&state);
    match mpd::list_playlists_connected(&config).await {
        Ok(names) => {
            let list: Vec<serde_json::Value> = names
                .into_iter()
                .map(|n| serde_json::json!({ "name": n }))
                .collect();
            Json(serde_json::Value::Array(list)).into_response()
        }
        Err(e) => {
            tracing::warn!("listplaylists MPD error: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "MPD unavailable"})),
            )
                .into_response()
        }
    }
}

/// GET /api/v1/search?query=... - MPD find (Node listingSearch returns browse-like result)
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub query: String,
}

pub async fn search(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> impl IntoResponse {
    if q.query.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "No search query provided"})),
        )
            .into_response();
    }
    let config = mpd_config_from_app(&state);
    match mpd::search_connected(&config, &state.config.music_sources.music_root, &q.query).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => {
            tracing::warn!("search MPD error: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "MPD unavailable"})),
            )
                .into_response()
        }
    }
}

/// GET /api/v1/superSearch?query=... - same as search (Node listingSuperSearch, browse-like result)
pub async fn super_search(
    State(state): State<AppState>,
    Query(q): Query<SearchQuery>,
) -> impl IntoResponse {
    search(State(state), Query(q)).await
}

/// GET /api/v1/collectionstats - MPD stats (artists, albums, songs, playtime)
pub async fn collection_stats(State(state): State<AppState>) -> impl IntoResponse {
    let config = mpd_config_from_app(&state);
    match mpd::collection_stats_connected(&config).await {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => {
            tracing::warn!("collectionstats MPD error: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "MPD unavailable"})),
            )
                .into_response()
        }
    }
}

/// GET /api/v1/getzones - multi-room zones stub (Node returns { zones: list })
pub async fn get_zones() -> impl IntoResponse {
    Json(serde_json::json!({ "zones": [] }))
}

/// GET /api/v1/getActiveUi — which stock UI layout is configured (`volumioUisList.json` `uiName` values).
pub async fn get_active_ui(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "active_layout": state.config.ui.active_layout,
    }))
}

/// POST /api/v1/replaceAndPlay - JSON body { uri }, clear queue + add + play
#[derive(Debug, Deserialize)]
pub struct ReplaceAndPlayBody {
    pub uri: String,
}

pub async fn replace_and_play(
    State(state): State<AppState>,
    Json(body): Json<ReplaceAndPlayBody>,
) -> impl IntoResponse {
    if body.uri.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "Missing uri"})),
        )
            .into_response();
    }
    let config = mpd_config_from_app(&state);
    match mpd::replace_and_play_resolved(
        &config,
        &state.config.music_sources.music_root,
        &body.uri,
    )
    .await
    {
        Ok(()) => Json(serde_json::json!({"response": "success"})).into_response(),
        Err(e) => {
            tracing::warn!("replaceAndPlay MPD error: {}", e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "MPD unavailable"})),
            )
                .into_response()
        }
    }
}

/// GET /api/v1/browse?uri=music-library|music-library/...
#[derive(Debug, Deserialize)]
pub struct BrowseQuery {
    #[serde(default)]
    pub uri: String,
}

/// GET /api/v1/browse - Evo-driven layout (local, usb, nas, smb) then MPD lsinfo
pub async fn browse(
    State(state): State<AppState>,
    Query(q): Query<BrowseQuery>,
) -> impl IntoResponse {
    let uri = if q.uri.is_empty() {
        "music-library"
    } else {
        q.uri.as_str()
    };

    // Root: storage sources only with albumart (see mpd::music_library_root_response); sidebar lists Favourites / tag library.
    if uri == "music-library" {
        return Json(mpd::music_library_root_response()).into_response();
    }

    let config = mpd_config_from_app(&state);
    if uri == "favourites" {
        return Json(mpd::browse_favourites_stub()).into_response();
    }
    match mpd::browse_connected(&config, &state.config.music_sources.music_root, uri).await {
        Ok(mut resp) => {
            mpd::browse_response_fill_meta_from_artist(&mut resp);
            Json(resp).into_response()
        }
        Err(e) => {
            tracing::warn!("browse {} MPD error: {}", uri, e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "MPD unavailable"})),
            )
                .into_response()
        }
    }
}
