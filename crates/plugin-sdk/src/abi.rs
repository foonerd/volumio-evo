//! Plugin ABI: contract between host (core) and guest (WASM plugin).
//!
//! Plugins export:
//! - `plugin_init` (optional): called once after load.
//! - `plugin_handle_request` (optional): called for each request; signature TBD.
//!
//! Host provides (imports):
//! - `log(ptr, len)`: log UTF-8 message from guest memory.
//! - Further host calls (mpd_command, config_get, etc.) TBD.
//!
//! Details and wire format in [docs/PLUGIN_ABI.md](../../../docs/PLUGIN_ABI.md).

use serde::{Deserialize, Serialize};

/// Plugin metadata (name, version). May be embedded in WASM or separate manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
}
