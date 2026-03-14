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

### Cross-compile (arm / arm64 / amd64)

Use [cross](https://github.com/cross-rs/cross) or the [CI workflow](.github/workflows/build.yml):

```bash
cross build --release --target armv7-unknown-linux-gnueabihf
cross build --release --target aarch64-unknown-linux-gnu
cross build --release --target x86_64-unknown-linux-gnu
```

## Layer

Apply the `layer/` contents on a minimal Pi OS or Debian Trixie image. See [layer/README.md](layer/README.md).

## License

GPL-2.0. See [LICENSE](LICENSE).
