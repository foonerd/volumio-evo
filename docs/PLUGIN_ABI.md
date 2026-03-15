# Volumio Evo - Plugin ABI

Contract between the host (Rust core) and guest (WASM plugin).

## Guest exports (plugin implements)

| Name                | Signature   | Description                    |
|---------------------|------------|--------------------------------|
| `plugin_init`       | `() -> ()` | Called once after load.        |
| `plugin_handle_request` | TBD    | Per-request handler; details TBD. |

## Host imports (core provides)

| Name | Signature   | Description                          |
|------|------------|--------------------------------------|
| `log` | `(ptr: i32, len: i32) -> ()` | Log UTF-8 message from guest linear memory at `[ptr, ptr+len)`. |

Further host calls (e.g. `mpd_command`, `config_get`, `http_fetch`) will be added as the ABI is refined. All imports live in the `env` module.

## Memory

- Guest has a single linear memory. Host reads guest memory only at explicit (ptr, len) from guest-provided calls.
- No shared memory across plugins; each plugin instance has its own store.

## Lifecycle

1. Host loads `.wasm` and instantiates with imports.
2. Host calls `plugin_init()` if present.
3. Host routes requests to plugins (mechanism TBD) and may call `plugin_handle_request` or other exports.
4. Plugin can call host imports at any time (e.g. `log`).

## Versioning

ABI version will be reflected in the import module name or via a version export. TBD.
