//! Plugin ABI: contract between host (core) and guest (WASM plugin).
//!
//! Plugins export:
//! - `plugin_init` (optional): called once after load.
//! - `plugin_handle_request` (optional): called for each request; signature TBD.
//! - ALSA pipeline (optional): see [`ALSA_PLUGIN_ABI_VERSION`] and [`AlsaContribution`].
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

// --- Optional ALSA graph contributions (narrow ABI) ---------------------------------------------

/// Version of the [`AlsaContribution`] JSON schema. Bump when fields change.
pub const ALSA_PLUGIN_ABI_VERSION: u32 = 1;

/// Export names (C ABI, `extern "C"`).
pub mod alsa_exports {
    pub const HAS_ALSA_CONTRIBUTION: &str = "has_alsa_contribution";
    pub const ALSA_JSON_PTR: &str = "alsa_contribution_json_ptr";
    pub const ALSA_JSON_LEN: &str = "alsa_contribution_json_len";
}

/// Optional ALSA contribution: declarative fragments the host **merges** into its single orchestrated
/// graph (plugins must not write `/etc/asound.conf` directly).
///
/// The guest encodes this as UTF-8 JSON readable from guest linear memory (see
/// [`alsa_exports`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlsaContribution {
    #[serde(default = "alsa_abi_version_default")]
    pub abi_version: u32,
    /// Ordered fragments; lower [`AlsaFragment::order`] = earlier in the pipeline (closer to hardware),
    /// unless the host documents a different convention for its merger.
    pub fragments: Vec<AlsaFragment>,
}

fn alsa_abi_version_default() -> u32 {
    ALSA_PLUGIN_ABI_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlsaFragment {
    /// Stable id for dedup / updates (e.g. `fusion-dsp`).
    pub id: String,
    /// Sort key for merge order relative to other plugins’ fragments.
    pub order: i32,
    /// One ALSA config snippet (e.g. a `pcm.{ ... }` or `ctl.{ ... }` block). Host validates/sandboxes.
    pub asound_snippet: String,
}
