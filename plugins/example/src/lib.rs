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
