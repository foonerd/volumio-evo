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

| Target | Use case | WASM plugins |
|--------|----------|--------------|
| **arm64** (aarch64) | Pi OS 64-bit, Rock Pi, Khadas, Trixie arm64 | Yes (Cranelift) |
| **amd64** (x86_64) | Trixie amd64, x86 PCs | Yes (Cranelift) |
| **armhf** (armv7) | Pi 0, Pi 1, 32-bit Pi OS | No (core only) |

Use [cross](https://github.com/cross-rs/cross) or the [CI workflow](.github/workflows/build.yml):

```bash
# arm64 (Pi OS 64-bit, Debian arm64)
cross build --release --target aarch64-unknown-linux-gnu

# amd64 (Debian x86_64; native on x86_64 host)
cross build --release --target x86_64-unknown-linux-gnu

# armhf (32-bit Pi OS): core only, no WASM (wasmtime doesn't build for 32-bit ARM)
cross build --release --target armv7-unknown-linux-gnueabihf -p volumio-evo-core --no-default-features
```

On armhf the core runs without the WASM plugin layer. For full plugin support use arm64 (e.g. Pi Zero 2 W with 64-bit Pi OS).

## Layer

Apply the `layer/` contents on a minimal Pi OS or Debian Trixie image. See [layer/README.md](layer/README.md).

## License

GPL-2.0. See [LICENSE](LICENSE).
