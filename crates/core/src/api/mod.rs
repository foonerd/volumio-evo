//! HTTP and Socket.IO API.

mod http;
mod socketio;
mod v1;

use crate::alsa::AlsaSettings;
use crate::config::Config;
use std::sync::Arc;

/// Shared state: config + channel to trigger album-art cache-clear broadcast + last browse for getLastPushedBrowseLibrary.
pub struct RouterState {
    pub config: Arc<Config>,
    /// Persisted ALSA output selection (Playback Options); full pipeline apply is future work.
    pub alsa: Arc<tokio::sync::RwLock<AlsaSettings>>,
    albumart_clear_tx: tokio::sync::mpsc::UnboundedSender<()>,
    /// Last pushBrowseLibrary payload (for getLastPushedBrowseLibrary).
    pub last_browse: Arc<tokio::sync::RwLock<Option<serde_json::Value>>>,
}

impl RouterState {
    /// Trigger broadcast of clearAlbumartCache to all Socket.IO clients (no-op if tx closed).
    pub fn send_clear_albumart_cache(&self) {
        let _ = self.albumart_clear_tx.send(());
    }

    /// Store last browse response for getLastPushedBrowseLibrary.
    pub async fn set_last_browse(&self, value: serde_json::Value) {
        *self.last_browse.write().await = Some(value);
    }

    /// Read last browse response (clone).
    pub async fn get_last_browse(&self) -> Option<serde_json::Value> {
        self.last_browse.read().await.clone()
    }
}

pub type AppState = Arc<RouterState>;

pub use http::router;
pub use socketio::push_state_queue_loop;
