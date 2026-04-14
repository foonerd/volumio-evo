# Volumio Evo

Rust backend + WASM plugins on a stock minimal OS. No Node, no debootstrap.

## Concept

- **Base:** Stock minimal image (Raspberry Pi OS Lite / Debian Trixie).
- **Layer:** Volumio Evo binary, plugins, config, and systemd applied on top.
- **Backend:** Single Rust binary; loads sandboxed WASM plugins.
- **UI:** Unchanged (e.g. React) over HTTP and Socket.IO.

See [docs/CONCEPT.md](docs/CONCEPT.md), [docs/PLUGIN_ABI.md](docs/PLUGIN_ABI.md), and [docs/PORTING.md](docs/PORTING.md) for API port status.

- **Run and test:** [docs/TESTER_GUIDE.md](docs/TESTER_GUIDE.md) — step-by-step from a plain OS (Raspberry Pi OS, Debian Trixie, Ubuntu 24.04) to a working setup and validation (no source needed; use a pre-built binary).
- **One-command full player bootstrap:** `scripts/bootstrap-volumio-evo-player.sh` — installs dependencies, clones/builds Evo + UI, configures services, and exposes a tester-ready player on port 80.
- **Build the binary:** [docs/BUILD_GUIDE.md](docs/BUILD_GUIDE.md) — build `volumio-evo` for each architecture (native, arm64, amd64, armhf) so you can provide it to testers.

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

## Music sources (local, USB, NAS, SMB)

Evo uses its **own layout** for music sources instead of relying on Volumio OS paths (`/mnt/INTERNAL`, overlayfs, etc.). This works on vanilla Debian (e.g. Trixie) and keeps MPD integration explicit.

- **Base path:** `music_sources.music_root`. Can be set at **install** or **first run** so it's not tied to a specific user (e.g. not only `volumio`):
  - **Env:** `VOLUMIO_EVO_MUSIC_ROOT` overrides config (e.g. in systemd unit or install script).
  - **Config file:** `[music_sources] music_root = "..."` in `/etc/volumio-evo/config.toml` or `config/volumio-evo.toml`.
  - **No config file:** user-aware default: `$XDG_DATA_HOME/volumio-evo/music` or `$HOME/.local/share/volumio-evo/music`, else `/var/lib/volumio-evo/music`. So a different user (e.g. `pi`, `debian`) gets a writable path when running without a config file.
- **MPD:** Set MPD's `music_directory` to the same path (install or MPD config) so MPD sees one root with four subdirs.
- **Subdirs:** Under `music_root` create (or symlink) **local**, **usb**, **nas**, **smb**:
  - **local** - on-device storage (e.g. symlink to `/data/INTERNAL` on Volumio OS, or a dir on rootfs on vanilla).
  - **usb** - removable media (e.g. symlink to `/media`).
  - **nas** / **smb** - mount points or symlinks for network shares.

Browse root `GET /api/v1/browse?uri=music-library` returns these four sources; subpaths use MPD `lsinfo`. The four subdirs must exist (or be symlinks) under `music_root` so MPD can list them. In config you can set optional `music_sources.local`, `usb`, `nas`, `smb` to document where each source points; the installer or a small init script then creates the dirs/symlinks under `music_root`.

## Layer

Apply the `layer/` contents on a minimal Pi OS or Debian Trixie image. See [layer/README.md](layer/README.md).

## License

GPL-2.0. See [LICENSE](LICENSE).
