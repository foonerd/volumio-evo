# Volumio Evo - Plugin ABI

Contract between the host (Rust core) and guest (WASM plugin).

**Feature flag:** The WASM plugin host is built only when the **`wasm`** crate feature is enabled (default on **aarch64** / **x86_64**). **armv7** / **armhf** release builds use **`--no-default-features`**, so there is **no** plugin loader — ABI applies when that feature is on. See [BUILD_GUIDE.md](BUILD_GUIDE.md).

## Guest exports (plugin implements)

| Name                | Signature   | Description                    |
|---------------------|------------|--------------------------------|
| `plugin_init`       | `() -> ()` | Called once after load.        |
| `plugin_handle_request` | TBD    | Per-request handler; details TBD. |
| `has_alsa_contribution` | `() -> i32` | **Optional.** Returns `1` if the plugin contributes ALSA fragments, else `0`. Missing export = no contribution. |
| `alsa_contribution_json_ptr` | `() -> i32` | **Optional** (required if `has_alsa_contribution` returns `1`). Byte offset in guest linear memory where UTF-8 JSON starts. |
| `alsa_contribution_json_len` | `() -> i32` | **Optional** (required if `has` is `1`). Length of that JSON in bytes. |

### ALSA contribution JSON

When `has_alsa_contribution` returns `1`, the host reads `memory[ptr .. ptr+len]` and parses JSON matching `AlsaContribution` in `crates/plugin-sdk/src/abi.rs`: `abi_version` (must match host), `fragments[]` with `id`, `order`, and `asound_snippet`. The host merges snippets in order; plugins must not write ALSA files on disk directly.

**Host merge + rebuild:** specified here; **implementation is high-priority and not yet complete** — see [`PRIORITY_ALSA_AAMPP.md`](./PRIORITY_ALSA_AAMPP.md).

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

- **`AlsaContribution.abi_version`** must equal **`ALSA_PLUGIN_ABI_VERSION`** in `plugin-sdk` (see `crates/plugin-sdk/src/abi.rs`). Bump when the JSON schema changes.
- General plugin ABI versioning (imports module name, etc.) TBD.
