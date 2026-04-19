//! **Settings → System → Boot branding:** run `scripts/volumio-boot-branding.sh` via **`sudo -n`** and
//! mirror Node **`openModal` → `modalProgress` → `modalDone`** so the stock progress modal updates.

use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::api::AppState;
use crate::paths;
use crate::system_settings::normalize_plymouth_rotation;

async fn broadcast_emit(state: &AppState, event: &str, payload: &serde_json::Value) {
    let io = match state.socket_io_broadcast.lock() {
        Ok(g) => g.as_ref().cloned(),
        Err(_) => None,
    };
    if let Some(io) = io {
        let _ = io.emit(event, payload).await;
    }
}

fn parse_branding_line(line: &str) -> Option<(u8, String)> {
    let t = line.trim_end();
    let rest = t.strip_prefix("::BRANDING ")?.trim_start();
    let (pct_s, msg) = rest
        .split_once(char::is_whitespace)
        .map(|(a, b)| (a, b.trim_start()))
        .unwrap_or((rest, ""));
    let pct = pct_s.parse::<u8>().unwrap_or(0);
    Some((pct, msg.to_string()))
}

pub(crate) async fn spawn_install(state: AppState, payload_data: serde_json::Value) {
    let rotation = {
        let mut sys = state.system_settings.write().await;
        sys.merge_boot_branding_rotation_payload(&payload_data);
        let r = normalize_plymouth_rotation(sys.boot_branding_plymouth_rotation);
        sys.boot_branding_plymouth_rotation = r;
        if let Err(e) = sys.save() {
            tracing::warn!(
                "{} boot branding: failed to save system settings: {}",
                crate::log_tags::EVO_UI,
                e
            );
        }
        r
    };

    let runner = paths::boot_branding_run_script_path();
    let install_script = paths::boot_branding_install_script_path();
    let evo_repo = paths::evo_repo_dir();
    let title = "Volumio boot branding";

    if !runner.is_file() {
        tracing::warn!(
            "{} boot branding wrapper missing: {:?}",
            crate::log_tags::EVO_UI,
            runner
        );
        broadcast_emit(
            &state,
            "openModal",
            &json!({
                "progress": true,
                "progressNumber": 0,
                "title": title,
                "message": "Boot branding script not found on this system.",
                "size": "lg",
                "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
            }),
        )
        .await;
        broadcast_emit(
            &state,
            "modalDone",
            &json!({
                "title": title,
                "message": format!(
                    "Expected {} (or set VOLUMIO_EVO_BOOT_BRANDING_SCRIPT).\n\nInstall: run bootstrap so it creates /usr/share/volumio-evo/repo → your checkout and branding scripts — sudo ./scripts/bootstrap-volumio-evo-player.sh or --upgrade-evo (see docs/BRANDED_BOOT.md).",
                    runner.display()
                ),
                "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
            }),
        )
        .await;
        return;
    }

    if !evo_repo.join("layer/plymouth/volumio-adaptive").is_dir() {
        tracing::warn!(
            "{} EVO_REPO_DIR {:?} has no layer/plymouth/volumio-adaptive — install may fail",
            crate::log_tags::EVO_UI,
            evo_repo
        );
    }
    if !install_script.is_file() {
        tracing::warn!(
            "{} missing {:?}; wrapper will fail",
            crate::log_tags::EVO_UI,
            install_script
        );
    }

    broadcast_emit(
        &state,
        "openModal",
        &json!({
            "progress": true,
            "progressNumber": 0,
            "title": title,
            "message": "Installing packages and configuring boot splash…",
            "size": "lg",
            "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
        }),
    )
    .await;

    let mut child = match Command::new("sudo")
        .arg("-n")
        .arg(&runner)
        .arg(rotation.to_string())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("{} boot branding spawn failed: {}", crate::log_tags::EVO_UI, e);
            broadcast_emit(
                &state,
                "modalDone",
                &json!({
                    "title": title,
                    "message": format!("Could not run installer ({e}). Ensure sudoers allows sudo -n for {:?}.", runner),
                    "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
                }),
            )
            .await;
            return;
        }
    };

    let stderr = child.stderr.take();
    if let Some(e) = stderr {
        tokio::spawn(async move {
            let reader = BufReader::new(e);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    tracing::warn!("{} boot branding (stderr): {}", crate::log_tags::EVO_UI, line);
                }
            }
        });
    }

    let mut exit_ok = false;
    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some((pct, msg)) = parse_branding_line(&line) {
                broadcast_emit(
                    &state,
                    "modalProgress",
                    &json!({
                        "progressNumber": pct,
                        "title": title,
                        "message": msg,
                    }),
                )
                .await;
            }
        }
    }

    match child.wait().await {
        Ok(st) => {
            exit_ok = st.success();
            if !exit_ok {
                tracing::warn!(
                    "{} boot branding script exited with {:?}",
                    crate::log_tags::EVO_UI,
                    st.code()
                );
            }
        }
        Err(e) => {
            tracing::warn!("{} boot branding wait: {}", crate::log_tags::EVO_UI, e);
        }
    }

    if exit_ok {
        broadcast_emit(
            &state,
            "modalDone",
            &json!({
                "title": "Boot branding enabled",
                "message": "Restart the device to apply the splash screen and kernel parameters.",
                "buttons": [
                    {
                        "name": "Restart",
                        "class": "btn btn-warning ng-scope",
                        "emit": "reboot",
                        "payload": ""
                    },
                    {
                        "name": "Got it",
                        "class": "btn btn-info ng-scope",
                        "emit": "closeModals",
                        "payload": ""
                    }
                ]
            }),
        )
        .await;
    } else {
        broadcast_emit(
            &state,
            "modalDone",
            &json!({
                "title": "Boot branding failed",
                "message": "Check journalctl for volumio-evo and verify sudo can run the branding script without a password.",
                "buttons": [{ "name": "Got it", "class": "btn btn-info ng-scope", "emit": "closeModals", "payload": "" }]
            }),
        )
        .await;
    }
}
