//! WASM plugin loading and execution. Stubbed when built without `wasm` feature (e.g. armhf).

#[cfg(feature = "wasm")]
mod wasm;

#[cfg(feature = "wasm")]
pub use wasm::{load_plugin, PluginHandle};

/// Stub when WASM plugins are disabled (e.g. armhf build; wasmtime does not support 32-bit ARM).
#[cfg(not(feature = "wasm"))]
#[derive(Debug)]
pub struct PluginHandle;

#[cfg(not(feature = "wasm"))]
#[allow(dead_code)]
pub fn load_plugin(_path: &std::path::Path) -> anyhow::Result<PluginHandle> {
    Ok(PluginHandle)
}
