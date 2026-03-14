# Volumio Evo

Rust backend + WASM plugins on a stock minimal OS. No Node, no debootstrap.

## Concept

- **Base:** Stock minimal image (Raspberry Pi OS Lite / Debian Trixie).
- **Layer:** Volumio Evo binary, plugins, config, and systemd applied on top.
- **Backend:** Single Rust binary; loads sandboxed WASM plugins.
- **UI:** Unchanged (e.g. React) over HTTP/WebSocket.

See [docs/CONCEPT.md](docs/CONCEPT.md) and [docs/PLUGIN_ABI.md](docs/PLUGIN_ABI.md).

## Build

```bash
cargo build --release
```

Binary: `target/release/volumio-evo`.

### Cross-compile (arm64 / amd64 / armhf)

Supported targets for minimal Trixie or Pi OS images:

| Target | Use case | Backend |
|--------|----------|---------|
| **arm64** (aarch64) | Pi OS 64-bit, Rock Pi, Khadas, Trixie arm64 | Cranelift (JIT) |
| **amd64** (x86_64) | Trixie amd64, x86 PCs | Cranelift (JIT) |
| **armhf** (armv7) | Pi 0, Pi 1, 32-bit Pi OS, Trixie armhf | Pulley (interpreter) |

Use [cross](https://github.com/cross-rs/cross) or the [CI workflow](.github/workflows/build.yml):

```bash
# arm64 (Pi OS 64-bit, Debian arm64)
cross build --release --target aarch64-unknown-linux-gnu

# amd64 (Debian x86_64; native on x86_64 host)
cross build --release --target x86_64-unknown-linux-gnu

# armhf (Pi 0, Pi 1, 32-bit Pi OS) – uses Pulley interpreter
cross build --release --target armv7-unknown-linux-gnueabihf --no-default-features --features pulley -p volumio-evo-core
```

On **armhf**, the core is built with wasmtime’s **Pulley** interpreter instead of Cranelift (which does not support 32-bit ARM). Plugin execution is slower but runs on Pi 0, Pi 1, and 32-bit images.

## Layer

Apply the `layer/` contents on a minimal Pi OS or Debian Trixie image. See [layer/README.md](layer/README.md).

## License

GPL-2.0. See [LICENSE](LICENSE).
