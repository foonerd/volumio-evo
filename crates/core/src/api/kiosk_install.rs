//! **Settings → System → Kiosk:** run **`layer/kiosk-wpe/install.sh`** via **`sudo -n`** (same as
//! bootstrap **`--with-kiosk=wpe`**) when the operator enables the kiosk or calls **`installKioskLayer`**,
//! mirroring **`boot_branding::spawn_install`**.

use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::api::AppState;
use crate::log_tags::{EVO_KIOSK, EVO_UI};
use crate::paths;
use socketioxide::extract::SocketRef;

use super::socketio::emit_system_ui_config;

async fn broadcast_emit(state: &AppState, event: &str, payload: &serde_json::Value) {
    let io = match state.socket_io_broadcast.lock() {
        Ok(g) => g.as_ref().cloned(),
        Err(_) => None,
    };
    if let Some(io) = io {
        let _ = io.emit(event, payload).await;
    }
}

/// Toast + optional revert for [`crate::kiosk::apply_kiosk_settings`] results from **`saveKioskSettings`**.
pub(crate) async fn notify_kiosk_apply_outcome(
    s: &SocketRef,
    state: &AppState,
    outcome: crate::kiosk::ApplyOutcome,
    prev_enabled: bool,
) {
    match outcome {
        crate::kiosk::ApplyOutcome::Running => {
            let _ = s.emit(
                "pushToastMessage",
                &json!({
                    "type": "success",
                    "title": "Display",
                    "message": "Kiosk started"
                }),
            );
        }
        crate::kiosk::ApplyOutcome::Stopped => {
            let _ = s.emit(
                "pushToastMessage",
                &json!({
                    "type": "success",
                    "title": "Display",
                    "message": "Display settings saved"
                }),
            );
        }
        crate::kiosk::ApplyOutcome::NoDrm => {
            {
                let mut sys = state.system_settings.write().await;
                sys.kiosk_enabled = prev_enabled;
                if let Err(e) = sys.save() {
                    tracing::warn!("{} saveKioskSettings revert: {}", EVO_KIOSK, e);
                }
            }
            let _ = s.emit(
                "pushToastMessage",
                &json!({
                    "type": "error",
                    "title": "Display",
                    "message": "No display detected. Connect HDMI or DSI and retry."
                }),
            );
        }
        crate::kiosk::ApplyOutcome::RootUserRefused => {
            {
                let mut sys = state.system_settings.write().await;
                sys.kiosk_enabled = prev_enabled;
                if let Err(e) = sys.save() {
                    tracing::warn!("{} saveKioskSettings revert (root refusal): {}", EVO_KIOSK, e);
                }
            }
            let _ = s.emit(
                "pushToastMessage",
                &json!({
                    "type": "error",
                    "title": "Display",
                    "message": "Kiosk requires a non-root service user."
                }),
            );
        }
        crate::kiosk::ApplyOutcome::PartialFailure => {
            let _ = s.emit(
                "pushToastMessage",
                &json!({
                    "type": "warning",
                    "title": "Display",
                    "message": "Settings saved but kiosk did not change state. Check journalctl -u volumio-evo-kiosk."
                }),
            );
        }
    }
}

async fn revert_kiosk_enabled_to(state: &AppState, enabled: bool) {
    let mut sys = state.system_settings.write().await;
    sys.kiosk_enabled = enabled;
    if let Err(e) = sys.save() {
        tracing::warn!("{} revert kiosk_enabled: {}", EVO_KIOSK, e);
    }
}

/// Run **`sudo -n layer/install/run-kiosk-wpe-install.sh`** with progress modal; used before
/// **`apply_kiosk_settings`** when the layer is missing or the user just enabled the kiosk.
pub(crate) async fn run_kiosk_layer_install_with_modal(
    _s: &SocketRef,
    state: &AppState,
    title: &str,
) -> bool {
    let runner = paths::kiosk_wpe_install_run_script_path();

    if !runner.is_file() {
        tracing::warn!(
            "{} kiosk layer install script missing: {:?}",
            EVO_UI,
            runner
        );
        broadcast_emit(
            state,
            "openModal",
            &json!({
                "progress": true,
                "progressNumber": 0,
                "title": title,
                "message": "Kiosk installer not found on this system.",
                "size": "lg",
                "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
            }),
        )
        .await;
        broadcast_emit(
            state,
            "modalDone",
            &json!({
                "title": title,
                "message": format!(
                    "Expected {} (repo checkout under /usr/share/volumio-evo/repo). Run: sudo ./scripts/bootstrap-volumio-evo-player.sh --upgrade-evo",
                    runner.display()
                ),
                "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
            }),
        )
        .await;
        return false;
    }

    broadcast_emit(
        state,
        "openModal",
        &json!({
            "progress": true,
            "progressNumber": 0,
            "title": title,
            "message": "Installing kiosk packages and helpers…",
            "size": "lg",
            "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
        }),
    )
    .await;

    let mut child = match Command::new("sudo")
        .arg("-n")
        .arg(&runner)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("{} kiosk layer install spawn: {}", EVO_UI, e);
            broadcast_emit(
                state,
                "modalDone",
                &json!({
                    "title": title,
                    "message": format!("Could not run installer ({e}). Ensure /etc/sudoers.d/volumio-evo-kiosk-layer-install allows sudo -n for {:?}.", runner),
                    "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
                }),
            )
            .await;
            return false;
        }
    };

    let stderr = child.stderr.take();
    if let Some(e) = stderr {
        tokio::spawn(async move {
            let reader = BufReader::new(e);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    tracing::warn!("{} kiosk layer install (stderr): {}", EVO_UI, line);
                }
            }
        });
    }

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut n = 0u8;
        while let Ok(Some(line)) = lines.next_line().await {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            tracing::info!("{} kiosk layer install: {}", EVO_KIOSK, t);
            n = n.saturating_add(1);
            let pct = (n % 100) as u8;
            broadcast_emit(
                state,
                "modalProgress",
                &json!({
                    "progressNumber": pct,
                    "title": title,
                    "message": t.chars().take(200).collect::<String>(),
                }),
            )
            .await;
        }
    }

    let exit_ok = match child.wait().await {
        Ok(st) => st.success(),
        Err(e) => {
            tracing::warn!("{} kiosk layer install wait: {}", EVO_UI, e);
            false
        }
    };

    if exit_ok {
        let done_msg = if title == "Kiosk update" {
            "Update complete."
        } else {
            "Starting the kiosk…"
        };
        broadcast_emit(
            state,
            "modalDone",
            &json!({
                "title": "Kiosk ready",
                "message": done_msg,
                "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
            }),
        )
        .await;
        true
    } else {
        broadcast_emit(
            state,
            "modalDone",
            &json!({
                "title": "Kiosk setup failed",
                "message": "Check journalctl for volumio-evo and verify sudo can run the kiosk installer. Ensure layer/binaries/<arch>/volumio-evo-kiosk-browser exists or install Rust build deps on device.",
                "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
            }),
        )
        .await;
        false
    }
}

/// After **`saveKioskSettings`** persisted **`kiosk_enabled`**, run layer install then apply.
pub(crate) async fn deferred_kiosk_enable_after_layer_install(
    s: SocketRef,
    state: AppState,
    prev_enabled: bool,
) {
    let ok = run_kiosk_layer_install_with_modal(&s, &state, "Kiosk setup").await;
    if !ok {
        revert_kiosk_enabled_to(&state, prev_enabled).await;
        emit_system_ui_config(&s, &state).await;
        return;
    }

    let outcome = crate::kiosk::apply_kiosk_settings(&state).await;
    notify_kiosk_apply_outcome(&s, &state, outcome, prev_enabled).await;
    emit_system_ui_config(&s, &state).await;
}

/// **`installKioskLayer`** — refresh packages + binaries without changing settings (boot branding style).
pub(crate) async fn spawn_install_only(state: AppState, s: SocketRef) {
    let runner = paths::kiosk_wpe_install_run_script_path();

    if !runner.is_file() {
        broadcast_emit(
            &state,
            "openModal",
            &json!({
                "progress": true,
                "progressNumber": 0,
                "title": "Kiosk update",
                "message": "Kiosk installer not found.",
                "size": "lg",
                "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
            }),
        )
        .await;
        broadcast_emit(
            &state,
            "modalDone",
            &json!({
                "title": "Kiosk update",
                "message": format!("Expected {} on this system.", runner.display()),
                "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
            }),
        )
        .await;
        return;
    }

    let ok = run_kiosk_layer_install_with_modal(&s, &state, "Kiosk update").await;
    if ok {
        let _ = s.emit(
            "pushToastMessage",
            &json!({
                "type": "success",
                "title": "Display",
                "message": "Kiosk layer updated"
            }),
        );
    }
    emit_system_ui_config(&s, &state).await;
}
