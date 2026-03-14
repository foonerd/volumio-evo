//! Volumio v1 REST API: getState, commands, getQueue, stubs for browse/getInstalledPlugins.
//! Mirrors the Node backend so the existing UI works.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::config::Config;
use crate::mpd::{self, MpdConfig, VolumioState};

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
    match mpd::with_mpd(&config, |client| Box::pin(mpd::get_state(client))).await {
        Ok(s) => Json(s).into_response(),
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

/// GET /api/v1/commands?cmd=...&volume=...&position=...&value=...
#[derive(Debug, Deserialize)]
pub struct CommandsQuery {
    pub cmd: Option<String>,
    pub volume: Option<String>,
    pub position: Option<String>,
    pub value: Option<String>,
    pub N: Option<String>,
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
        .or(q.N)
        .as_deref()
        .and_then(|s| s.parse::<i64>().ok());
    let repeat = (cmd == "repeat").then(|| q.value.as_deref() == Some("true"));
    let random = (cmd == "random").then(|| q.value.as_deref() == Some("true"));

    let config = mpd_config_from_app(&state);
    match mpd::with_mpd(&config, |client| {
        Box::pin(mpd::run_command(client, cmd, volume, position, repeat, random))
    })
    .await
    {
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
    match mpd::with_mpd(&config, |client| Box::pin(mpd::get_queue(client))).await {
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

/// GET /api/v1/browse - stub (returns empty navigation)
pub async fn browse() -> impl IntoResponse {
    Json(serde_json::json!({
        "navigation": { "lists": [] }
    }))
}
