//! Example Volumio Evo WASM plugin. Exports plugin_init for the host to call.
//!
//! Build: cargo build --target wasm32-unknown-unknown --release

#![no_std]

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

extern "C" {
    /// Host-provided: log message at (ptr, len) in linear memory.
    #[allow(dead_code)]
    fn log(ptr: i32, len: i32);
}

/// Called by the host once after the plugin is loaded.
#[no_mangle]
pub extern "C" fn plugin_init() {
    // Stub: in real ABI we'd write a message to memory and call log(ptr, len).
}

// --- Optional ALSA ABI (see `volumio_evo_plugin_sdk::abi`) ---

/// `1` if this plugin contributes ALSA fragments, else `0`. Absent export = no contribution.
#[no_mangle]
pub extern "C" fn has_alsa_contribution() -> i32 {
    0
}

/// UTF-8 JSON offset in guest linear memory; `0` if none (when `has_alsa_contribution` is `0`).
#[no_mangle]
pub extern "C" fn alsa_contribution_json_ptr() -> i32 {
    0
}

/// Byte length of JSON at `alsa_contribution_json_ptr`.
#[no_mangle]
pub extern "C" fn alsa_contribution_json_len() -> i32 {
    0
}
