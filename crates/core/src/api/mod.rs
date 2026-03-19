//! HTTP and Socket.IO API.

mod http;
mod socketio;
mod v1;

use crate::config::Config;
use std::sync::Arc;

/// Shared state: config + channel to trigger album-art cache-clear broadcast (for upload / callMethod).
pub struct RouterState {
    pub config: Arc<Config>,
    albumart_clear_tx: tokio::sync::mpsc::UnboundedSender<()>,
}

impl RouterState {
    /// Trigger broadcast of clearAlbumartCache to all Socket.IO clients (no-op if tx closed).
    pub fn send_clear_albumart_cache(&self) {
        let _ = self.albumart_clear_tx.send(());
    }
}

pub type AppState = Arc<RouterState>;

pub use http::router;
pub use socketio::push_state_queue_loop;
