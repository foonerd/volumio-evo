//! Volumio v1 REST API: getState, commands, getQueue, browse, ping, getSystemVersion, getSystemInfo, listplaylists, search, stubs.
//! Mirrors the Node backend so the existing UI works.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::config::{Config, MUSIC_SOURCE_NAMES};
use crate::mpd::{self, BrowseItem, BrowseList, BrowseNavigation, BrowsePrev, BrowseResponse, MpdConfig, VolumioState};

/// Shared app state (config only; MPD is per-request connect).
pub type AppState = Arc<Config>;

pub fn mpd_config_from_app(state: &AppState) -> MpdConfig {
    MpdConfig {
        host: state.mpd_host.clone(),
        port: state.mpd_port,
    }
}

/// GET /api/v1/getState
pub async fn get_state(State(state): State<AppState>) -> impl IntoResponse {
    let config = mpd_config_from_app(&state);
    match mpd::get_state_connected(&config).await {
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
        return match mpd::add_to_queue_connected(&config, uri).await {
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
        return match mpd::add_play_connected(&config, uri).await {
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

/// GET /api/v1/getInstalledPlugins - stub for UI compatibility
pub async fn get_installed_plugins() -> impl IntoResponse {
    Json(serde_json::json!([]))
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
    match mpd::search_connected(&config, &q.query).await {
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

    // Root: return our four sources (no MPD call). MPD music_directory must be music_root.
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
        return Json(resp).into_response();
    }

    let config = mpd_config_from_app(&state);
    match mpd::browse_connected(&config, uri).await {
        Ok(resp) => Json(resp).into_response(),
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
