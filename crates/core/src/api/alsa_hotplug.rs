//! ALSA **`sound`** subsystem hotplug: when a USB DAC appears or disappears while another output
//! (e.g. headphone jack) is active, refresh **`pushOutputDevices`** and Playback Options **`pushUiConfig`**
//! so the wizard / settings list updates without a reboot (Node uses `volumio usbattach` for a narrow
//! Primo case; Evo listens to udev for any card add/remove under `sound`).

use crate::api::AppState;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

fn sound_subsystem_hotplug_line(line: &str) -> bool {
    let t = line.trim();
    if !t.starts_with("UDEV") {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    if !(lower.contains(" add ") || lower.contains(" remove ")) {
        return false;
    }
    lower.contains("/sound/")
}

/// Re-run `aplay -l`, broadcast wizard + Playback output list, toast, and **`pushState`** (volume may
/// change if [`crate::alsa::coerce_selection`] fell back after unplug).
pub async fn broadcast_alsa_hotplug_refresh(state: &AppState) {
    let (cards, settings, i2s_dacs) =
        crate::api::socketio::sync_playback_cards_from_aplay(state).await;
    let push_dev = crate::alsa::push_output_devices_json(&cards, &settings, &i2s_dacs);
    let ui_cfg = match crate::api::socketio::playback_options_ui_from_synced_cards(
        state,
        &cards,
        &settings,
        &i2s_dacs,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "{} hotplug Playback Options UI: {}",
                crate::log_tags::EVO_OUTPUT,
                e
            );
            return;
        }
    };

    let io = match state.socket_io_broadcast.lock() {
        Ok(g) => g.as_ref().cloned(),
        Err(_) => None,
    };
    if let Some(io) = io {
        let _ = io.emit("pushOutputDevices", &push_dev).await;
        let _ = io.emit("pushUiConfig", &ui_cfg).await;
        let _ = io
            .emit(
                "pushToastMessage",
                &serde_json::json!({
                    "type": "info",
                    "title": "Audio devices",
                    "message": "Playback output list updated."
                }),
            )
            .await;
    }

    state.notify_push_state();
    tracing::info!(
        "{} ALSA hotplug: refreshed output devices and Playback Options UI",
        crate::log_tags::EVO_OUTPUT
    );
}

/// `udevadm monitor --subsystem-match=sound` → debounced [`broadcast_alsa_hotplug_refresh`].
pub async fn run_alsa_sound_hotplug_loop(state: AppState) {
    let (debounce_tx, mut debounce_rx) = mpsc::channel::<()>(8);
    let debounce_state = state.clone();
    tokio::spawn(async move {
        while debounce_rx.recv().await.is_some() {
            tokio::time::sleep(Duration::from_millis(450)).await;
            while debounce_rx.try_recv().is_ok() {}
            broadcast_alsa_hotplug_refresh(&debounce_state).await;
        }
    });

    loop {
        let mut child = match Command::new("udevadm")
            .args(["monitor", "--subsystem-match=sound", "--udev"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    "{} udevadm monitor not started (no udev?): {} — retry in 30s",
                    crate::log_tags::EVO_OUTPUT,
                    e
                );
                tokio::time::sleep(Duration::from_secs(30)).await;
                continue;
            }
        };

        let stdout = match child.stdout.take() {
            Some(s) => s,
            None => {
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    if sound_subsystem_hotplug_line(&line) {
                        tracing::debug!(
                            "{} ALSA udev: {}",
                            crate::log_tags::EVO_OUTPUT,
                            line.trim()
                        );
                        let _ = debounce_tx.try_send(());
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        "{} udevadm monitor read: {}",
                        crate::log_tags::EVO_OUTPUT,
                        e
                    );
                    break;
                }
            }
        }

        let _ = child.kill().await;
        tracing::warn!(
            "{} udevadm monitor exited; restarting in 5s",
            crate::log_tags::EVO_OUTPUT
        );
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
