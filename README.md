# Volumio Evo

Rust backend + WASM plugins on a stock minimal OS. No Node, no debootstrap.

## Concept

- **Base:** Stock minimal image (Raspberry Pi OS Lite / Debian Trixie).
- **Layer:** Volumio Evo binary, plugins, config, and systemd applied on top.
- **Backend:** Single Rust binary; loads sandboxed WASM plugins.
- **UI:** Unchanged (e.g. React) over HTTP and Socket.IO.

See [docs/CONCEPT.md](docs/CONCEPT.md), [docs/PLUGIN_ABI.md](docs/PLUGIN_ABI.md), and [docs/PORTING.md](docs/PORTING.md) for API port status.

- **Persisted settings on disk:** [docs/SETTINGS_LAYOUT.md](docs/SETTINGS_LAYOUT.md) — namespace under `/var/lib/volumio-evo/settings/` (`alsa/`, `mpd/`, future `network/`, `mounts/`, …), env overrides, secrets guidance.

- **Logs and journald:** [docs/OBSERVABILITY.md](docs/OBSERVABILITY.md) — `[EVO]` line prefix, domain tags (`EVO VOLUME -->`, …), `RUST_LOG` / config precedence, `journalctl` examples.

- **Run and test:** [docs/TESTER_GUIDE.md](docs/TESTER_GUIDE.md) — step-by-step from a plain OS (Raspberry Pi OS, Debian Trixie, Ubuntu 24.04) to a working setup and validation. By default the bootstrap script installs the **prebuilt** binary from **`layer/binaries/<triple>/`** when the repo checkout is present; use **`--build`** to compile on the device instead.
- **One-command full player bootstrap:** `scripts/bootstrap-volumio-evo-player.sh` — installs dependencies, clones or updates the repo, installs the backend (prebuilt from **`layer/binaries/`** by default, or **`cargo`** with **`--build`**), copies static UI from **`layer/web/`**, configures MPD/systemd/nginx, and serves the UI on port **80** (Evo API on **3000**).
- **Fresh host, no git clone yet:** `curl -fsSL https://raw.githubusercontent.com/foonerd/volumio-evo/main/install.sh | sudo bash` — shallow-clones into **`/opt/volumio/volumio-evo`** and runs that bootstrap. Override clone URL or branch with **`EVO_REPO_URL`**, **`EVO_GIT_REF`**, or target dir with **`EVO_REPO_DIR`** / **`BASE_DIR`**. Pass bootstrap flags after **`--`**, e.g. `| sudo bash -s -- --build`.
- **Build the binary:** [docs/BUILD_GUIDE.md](docs/BUILD_GUIDE.md) — build `volumio-evo` for each architecture (native, arm64, amd64, armhf) and optionally refresh **`layer/binaries/`** for shipping.

## Build

From the repo root (workspace):

```bash
cargo build --release -p volumio-evo-core
```

**Binary:** `target/release/volumio-evo`. Using **`-p volumio-evo-core`** builds only the main binary; plain **`cargo build --release`** builds all workspace members (including the example WASM plugin crate).

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
cross build --release -p volumio-evo-core --target aarch64-unknown-linux-gnu

# amd64 (Debian x86_64; native on x86_64 host)
cross build --release -p volumio-evo-core --target x86_64-unknown-linux-gnu

# armhf (32-bit Pi OS): core only, no WASM (wasmtime doesn't build for 32-bit ARM)
cross build --release -p volumio-evo-core --target armv7-unknown-linux-gnueabihf --no-default-features
```

On armhf the core runs without the WASM plugin layer. For full plugin support use arm64 (e.g. Pi Zero 2 W with 64-bit Pi OS).

## Music sources (INTERNAL, USB, NAS, SMB)

Evo uses its **own layout** for music sources instead of relying on Volumio OS paths (`/mnt/INTERNAL`, overlayfs, etc.). This works on vanilla Debian (e.g. Trixie) and keeps MPD integration explicit.

- **Base path:** `music_sources.music_root`. Can be set at **install** or **first run** so it's not tied to a specific user (e.g. not only `volumio`):
  - **Env:** `VOLUMIO_EVO_MUSIC_ROOT` overrides config (e.g. in systemd unit or install script).
  - **Config file:** `[music_sources] music_root = "..."` in `/etc/volumio-evo/config.toml` or `config/volumio-evo.toml`.
  - **No config file:** user-aware default: `$XDG_DATA_HOME/volumio-evo/music` or `$HOME/.local/share/volumio-evo/music`, else `/var/lib/volumio-evo/music`. So a different user (e.g. `pi`, `debian`) gets a writable path when running without a config file.
- **MPD:** Set MPD's `music_directory` to the same path (install or MPD config) so MPD sees one root with four subdirs.
- **Subdirs:** Under `music_root` create (or symlink) **INTERNAL**, **USB**, **NAS**, **SMB** (same names as stock Volumio / Node `stickingMusicLibrary` so `lsinfo` matches browse URIs):
  - **INTERNAL** - on-device storage (e.g. symlink to `/data/INTERNAL` on Volumio OS, or a dir on rootfs on vanilla).
  - **USB** - removable media (e.g. symlink under `/media`).
  - **NAS** / **SMB** - mount points or symlinks for network shares.

Browse root `GET /api/v1/browse?uri=music-library` returns these four sources with `albumart` (bundled `sourceicon` PNGs); subpaths use MPD `lsinfo`. The four subdirs must exist under `music_root`. If you still have a lowercase **`local`** tree from an older Evo install, add e.g. `ln -s local INTERNAL` under `music_root` until you rename dirs. Optional `music_sources.local`, `usb`, `nas`, `smb` in config document where each source points for installers.

## Layer

Apply the `layer/` contents on a minimal Pi OS or Debian Trixie image. See [layer/README.md](layer/README.md).

## More documentation

| Document | Topic |
|----------|--------|
| [docs/TESTER_GUIDE.md](docs/TESTER_GUIDE.md) | On-device bootstrap (canonical test) |
| [docs/BUILD_GUIDE.md](docs/BUILD_GUIDE.md) | Cross-compilation, `layer/binaries/` |
| [docs/OBSERVABILITY.md](docs/OBSERVABILITY.md) | `journalctl`, `[EVO]` prefix, `RUST_LOG` |
| [docs/SETTINGS_LAYOUT.md](docs/SETTINGS_LAYOUT.md) | Paths under `/var/lib/volumio-evo/settings/` |
| [docs/PORTING.md](docs/PORTING.md) | volumio3-backend inventory vs Evo |
| [docs/UI_GAP.md](docs/UI_GAP.md) | Optional Volumio2-UI adjustments |
| [docs/PLUGIN_ABI.md](docs/PLUGIN_ABI.md) | WASM plugin contract |
| [docs/PRIORITY_ALSA_AAMPP.md](docs/PRIORITY_ALSA_AAMPP.md) | ALSA/AAMPP plugin pipeline (not done) |
| [docs/ALBUMART_PROVIDERS.md](docs/ALBUMART_PROVIDERS.md) | Online album-art providers |

## License

GPL-2.0. See [LICENSE](LICENSE).
