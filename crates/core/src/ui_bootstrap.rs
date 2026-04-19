//! Apply **`[ui] active_layout`** to nginx via **`bootstrap-volumio-evo-player.sh --apply-ui-only`**.

use crate::api::{broadcast_reload_ui, AppState};

/// Persist layout, update in-memory [`crate::api::RouterState::active_layout`], re-run bootstrap, **`reloadUi`** all clients.
pub async fn apply_active_layout_change(state: &AppState, layout_name: &str) -> anyhow::Result<()> {
    let l = layout_name.trim().to_lowercase();
    if !crate::config::is_valid_ui_layout(&l) {
        anyhow::bail!("unknown UI layout {:?}", layout_name);
    }
    crate::config::persist_ui_active_layout(&l).await?;
    *state.active_layout.write().await = l;
    run_apply_ui_bootstrap().await;
    broadcast_reload_ui(state).await;
    Ok(())
}

/// Run packaged bootstrap script so nginx **`root`** matches **`config.toml`** **`[ui] active_layout`**.
pub async fn run_apply_ui_bootstrap() {
    let script = std::env::var("VOLUMIO_EVO_BOOTSTRAP_SCRIPT")
        .ok()
        .map(std::path::PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            crate::paths::evo_repo_dir()
                .join("scripts")
                .join("bootstrap-volumio-evo-player.sh")
        });
    if !script.is_file() {
        tracing::warn!(
            "{} bootstrap script missing {:?} — set VOLUMIO_EVO_BOOTSTRAP_SCRIPT or install repo under {}",
            crate::log_tags::EVO_UI,
            script,
            crate::paths::evo_repo_dir().display()
        );
        return;
    }
    let arg0 = script.to_string_lossy().into_owned();
    match tokio::process::Command::new("sudo")
        .args(["-n", &arg0, "--apply-ui-only"])
        .output()
        .await
    {
        Ok(o) => {
            if !o.status.success() {
                tracing::warn!(
                    "{} bootstrap --apply-ui-only exited {:?}: stderr={}",
                    crate::log_tags::EVO_UI,
                    o.status.code(),
                    String::from_utf8_lossy(&o.stderr).trim()
                );
            } else {
                tracing::info!(
                    "{} nginx UI roots refreshed (--apply-ui-only)",
                    crate::log_tags::EVO_UI
                );
            }
        }
        Err(e) => tracing::warn!(
            "{} bootstrap spawn: {}",
            crate::log_tags::EVO_UI,
            e
        ),
    }
}
