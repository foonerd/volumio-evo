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

use super::playback_clock::ui_seek_ms;
use super::pushstate_log;
use super::{apply_volume_mute_overlay, read_master_volume_percent, AppState};

pub fn mpd_config_from_app(state: &AppState) -> MpdConfig {
    MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    }
}

/// GET /api/v1/getState
pub async fn get_state(State(state): State<AppState>) -> impl IntoResponse {
    let config = mpd_config_from_app(&state);
    let master = read_master_volume_percent(&state).await;
    match mpd::get_state_connected(&config, &state.config.music_sources.music_root, master).await {
        Ok(mut s) => {
            s.seek = {
                let clock = state.playback_clock.read().await;
                ui_seek_ms(clock.seek_for_emit_before_resync(&s), s.duration)
            };
            apply_volume_mute_overlay(&state, &mut s).await;
            state.store_mpd_snapshot(&s).await;
            pushstate_log::debug_volumio_state("REST GET /api/v1/getState (response body)", &s);
            Json::<VolumioState>(s).into_response()
        }
        Err(e) => {
            tracing::warn!("{} getState MPD error: {}", crate::log_tags::EVO_STATE, e);
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
            Ok(()) => {
                state.notify_push_state();
                state.notify_push_queue();
                Json(serde_json::json!({"response": "addToQueue Success"})).into_response()
            }
            Err(e) => {
                tracing::warn!("{} addToQueue MPD error: {}", crate::log_tags::EVO_QUEUE, e);
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
            Ok(()) => {
                state.notify_push_state();
                state.notify_push_queue();
                Json(serde_json::json!({"response": "addPlay Success"})).into_response()
            }
            Err(e) => {
                tracing::warn!("{} addPlay MPD error: {}", crate::log_tags::EVO_PLAY, e);
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"error": "MPD unavailable"})),
                )
                    .into_response()
            }
        };
    }

    let _vol_apply = if cmd == "volume" {
        Some(state.volume_apply.lock().await)
    } else {
        None
    };

    match mpd::run_command_connected(&config, cmd, volume, position, repeat, random).await {
        Ok(()) => {
            state.notify_push_state();
            if cmd == "clearQueue" {
                state.notify_push_queue();
            }
            Json(serde_json::json!({
                "response": cmd.to_string() + " Success"
            }))
            .into_response()
        }
        Err(e) => {
            tracing::warn!("{} commands {} MPD error: {}", crate::log_tags::EVO_API, cmd, e);
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
    match mpd::get_queue_connected(&config, &state.config.music_sources.music_root).await {
        Ok(items) => {
            pushstate_log::debug_queue_snapshot("REST GET /api/v1/getQueue (response body)", items.len());
            Json(serde_json::json!({ "queue": items })).into_response()
        }
        Err(e) => {
            tracing::warn!("{} getQueue MPD error: {}", crate::log_tags::EVO_QUEUE, e);
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

/// GET /api/v1/getSystemVersion — **`systemversion`** is the **volumio-evo** daemon semver (`CARGO_PKG_VERSION`).
pub async fn get_system_version(State(state): State<AppState>) -> impl IntoResponse {
    let hostname = state.system_settings.read().await.device_name.clone();
    Json(serde_json::json!({
        "systemversion": crate::version::VOLUMIO_EVO_VERSION,
        "variant": "volumio-evo",
        "hardware": "generic",
        "os": null,
        "builddate": null,
        "hostname": hostname
    }))
}

/// GET /api/v1/getSystemInfo — same as [`get_system_version`] plus **`hwUuid`** stub.
pub async fn get_system_info(State(state): State<AppState>) -> impl IntoResponse {
    let hostname = state.system_settings.read().await.device_name.clone();
    Json(serde_json::json!({
        "systemversion": crate::version::VOLUMIO_EVO_VERSION,
        "variant": "volumio-evo",
        "hardware": "generic",
        "os": null,
        "builddate": null,
        "hostname": hostname,
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
            tracing::warn!("{} listplaylists MPD error: {}", crate::log_tags::EVO_PLAYLIST, e);
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
            tracing::warn!("{} search MPD error: {}", crate::log_tags::EVO_SEARCH, e);
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

/// GET /api/v1/collectionstats — same semantics as Socket.IO `getMyCollectionStats` / `pushMyCollectionStats`
/// (`count group artist` + `list album group albumartist`, not bare `stats`).
pub async fn collection_stats(State(state): State<AppState>) -> impl IntoResponse {
    let config = mpd_config_from_app(&state);
    match mpd::collection_stats_connected(&config).await {
        Ok(stats) => Json(stats).into_response(),
        Err(e) => {
            tracing::warn!("{} collectionstats MPD error: {}", crate::log_tags::EVO_DB, e);
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
        Ok(()) => {
            state.notify_push_state();
            state.notify_push_queue();
            Json(serde_json::json!({"response": "success"})).into_response()
        }
        Err(e) => {
            tracing::warn!("{} replaceAndPlay MPD error: {}", crate::log_tags::EVO_PLAY, e);
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
    match mpd::browse_connected(&config, &state.config.music_sources.music_root, uri).await {
        Ok(mut resp) => {
            mpd::browse_response_fill_meta_from_artist(&mut resp);
            Json(resp).into_response()
        }
        Err(e) => {
            tracing::warn!("{} browse {} MPD error: {}", crate::log_tags::EVO_BROWSE, uri, e);
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "MPD unavailable"})),
            )
                .into_response()
        }
    }
}

/// GET /api/v1/network/nm/status — NetworkManager / `nmcli` diagnostic (Phase 1).
pub async fn network_nm_status(State(state): State<AppState>) -> impl IntoResponse {
    let iface = crate::nm_network::resolve_effective_wifi_iface(&state.config).await;
    let snap = crate::nm_network::diagnostic_snapshot(Some(iface.as_str())).await;
    Json(snap).into_response()
}

/// GET /api/v1/network/nm/wifi-devices — Wi-Fi `wlan*` device names from NM (excludes `p2p-dev-*`).
pub async fn network_nm_wifi_devices(State(state): State<AppState>) -> impl IntoResponse {
    let effective = crate::nm_network::resolve_effective_wifi_iface(&state.config).await;
    let rows = crate::nm_network::nm_device_table().await.unwrap_or_default();
    let wifi_rows: Vec<crate::nm_network::NmDeviceRow> = rows
        .into_iter()
        .filter(|r| {
            r.kind.eq_ignore_ascii_case("wifi")
                && !r.device.starts_with("p2p-dev-")
                && !r.device.trim().is_empty()
        })
        .collect();
    let iw_devs = crate::wifi_phy::list_wifi_devices().await.unwrap_or_default();
    let devices: Vec<String> = wifi_rows.iter().map(|r| r.device.clone()).collect();
    let mut detailed: Vec<serde_json::Value> = Vec::with_capacity(wifi_rows.len());
    for r in &wifi_rows {
        let iw_info = iw_devs.iter().find(|d| d.ifname == r.device);
        let iftype = iw_info.map(|d| d.iftype.clone());
        let phy = iw_info.map(|d| d.phy.clone());
        let sta_capable = crate::wifi_phy::is_sta_capable(&r.device).await;
        detailed.push(serde_json::json!({
            "ifname": r.device,
            "phy": phy,
            "iftype": iftype,
            "sta_capable": sta_capable,
            "nm_state": r.state,
            "nm_connection": r.connection,
        }));
    }
    Json(serde_json::json!({
        "devices": devices,
        "devices_detailed": detailed,
        "effective": effective,
        "preferred_file": crate::network_config::read_wifi_iface_preferred(),
    }))
    .into_response()
}

/// GET /api/v1/network/nm/intent — persisted NM intent (no PSK values; includes **`ethernet.enabled`**).
#[derive(Serialize)]
pub struct NetworkNmIntentResponse {
    pub intent: crate::network_config::NetworkIntent,
    pub sta_psk_configured: bool,
    pub ap_psk_configured: bool,
}

pub async fn network_nm_intent_get() -> impl IntoResponse {
    let intent = crate::network_config::NetworkIntent::load();
    Json(NetworkNmIntentResponse {
        sta_psk_configured: crate::network_config::wifi_sta_psk_configured(),
        ap_psk_configured: crate::network_config::wifi_ap_psk_configured(),
        intent,
    })
    .into_response()
}

/// PUT /api/v1/network/nm/intent — replace `intent.toml`; optional PSK sidecars; optional `apply` (runs `nmcli`).
#[derive(Deserialize)]
pub struct NetworkNmIntentPut {
    pub intent: crate::network_config::NetworkIntent,
    #[serde(default)]
    pub sta_psk: Option<String>,
    #[serde(default)]
    pub ap_psk: Option<String>,
    #[serde(default)]
    pub apply: bool,
}

pub async fn network_nm_intent_put(
    State(state): State<AppState>,
    Json(body): Json<NetworkNmIntentPut>,
) -> impl IntoResponse {
    if let Err(e) = body.intent.save() {
        tracing::warn!("{} save intent: {}", crate::log_tags::EVO_NET, e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{}", e)})),
        )
            .into_response();
    }
    if let Some(ref p) = body.sta_psk {
        if let Err(e) = crate::network_config::write_wifi_sta_psk(p) {
            tracing::warn!("{} write wifi-sta.psk: {}", crate::log_tags::EVO_NET, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("{}", e)})),
            )
                .into_response();
        }
    }
    if let Some(ref p) = body.ap_psk {
        if let Err(e) = crate::network_config::write_wifi_ap_psk(p) {
            tracing::warn!("{} write wifi-ap.psk: {}", crate::log_tags::EVO_NET, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("{}", e)})),
            )
                .into_response();
        }
    }

    let apply_report = if body.apply {
        let r =
            crate::nm_network::apply_network_intent_exclusive(&body.intent, state.config.as_ref())
                .await;
        crate::nm_network::log_network_apply_result("rest_put_network_nm_intent", &r);
        Some(r)
    } else {
        None
    };

    Json(serde_json::json!({
        "ok": true,
        "apply": apply_report,
    }))
    .into_response()
}
