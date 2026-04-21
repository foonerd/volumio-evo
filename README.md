# Volumio Evo

**Evo-next contract:** a **steward** process administers a **catalogue** (racks, shelves, slots); **plugins** stock slots; consumers get **projections** and **happenings** — no plugin-to-plugin traffic. Full vocabulary and commitments: **[docs/CONCEPT.md](docs/CONCEPT.md)**.

**Today's repository** still ships much of that behaviour inside one Rust binary plus optional WASM guests and OS-layer installers — a **transitional** layout on the path to manifest-driven plugins and a service-agnostic steward ([docs/PLUGIN_CORE_VS_EXTENSIONS.md](docs/PLUGIN_CORE_VS_EXTENSIONS.md)). Base OS: **Debian Trixie minimal** (lite); Evo applies as a **layer**, not a from-scratch rootfs ([docs/CONCEPT.md](docs/CONCEPT.md) §5).

## Documentation index

**Start here:** [docs/DOCUMENTATION_MAP.md](docs/DOCUMENTATION_MAP.md) — supremacy (**CONCEPT.md**), authority table, assumptions, completed vs not ported, deferred topics, update rules.

Further pointers:

- Parity with volumio3-backend: [docs/PORTING.md](docs/PORTING.md)
- WASM plugin ABI (one admissible host): [docs/PLUGIN_ABI.md](docs/PLUGIN_ABI.md)
- Plugin hosts / trust / native stacks: [docs/PLUGIN_SYSTEM_EXTENSIONS.md](docs/PLUGIN_SYSTEM_EXTENSIONS.md)

- **Persisted settings:** [docs/SETTINGS_LAYOUT.md](docs/SETTINGS_LAYOUT.md) — namespace under `/var/lib/volumio-evo/settings/`, env overrides, secrets guidance.
- **Logs and journald:** [docs/OBSERVABILITY.md](docs/OBSERVABILITY.md) — `[EVO]` prefix, domain tags, `RUST_LOG`, `journalctl` examples.
- **Run and test:** [docs/TESTER_GUIDE.md](docs/TESTER_GUIDE.md) — from plain OS to validation; bootstrap installs **prebuilt** **`layer/binaries/<triple>/`** when present; use **`--build`** to compile on device.
- **One-command player bootstrap:** `scripts/bootstrap-volumio-evo-player.sh` — dependencies, repo clone/update, backend install, **`layer/web/`** UI, MPD/systemd/nginx; UI on **80**, Evo API on **3000**.
- **Fresh host:** `curl -fsSL https://raw.githubusercontent.com/foonerd/volumio-evo/main/install.sh | sudo bash` — shallow clone into **`/opt/volumio/volumio-evo`**; overrides **`EVO_REPO_URL`**, **`EVO_REPO_DIR`**, **`EVO_REPO_BRANCH`** / **`EVO_GIT_REF`**, **`EVO_REPO_DEPTH`**. Pass bootstrap flags after **`--`**. See [docs/TESTER_GUIDE.md](docs/TESTER_GUIDE.md) § Git checkout size.
- **Build:** [docs/BUILD_GUIDE.md](docs/BUILD_GUIDE.md) — cross-compile and refresh **`layer/binaries/`**.

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

- **Base path:** `music_sources.music_root`. Set at **install** or **first run** so it is not tied to a specific user (e.g. not only `volumio`):
  - **Env:** `VOLUMIO_EVO_MUSIC_ROOT` overrides config (e.g. in systemd unit or install script).
  - **Config file:** `[music_sources] music_root = "..."` in `/etc/volumio-evo/config.toml` or `config/volumio-evo.toml`.
  - **No config file:** user-aware default: `$XDG_DATA_HOME/volumio-evo/music` or `$HOME/.local/share/volumio-evo/music`, else `/var/lib/volumio-evo/music`.
- **MPD:** Set MPD's `music_directory` to the same path.
- **Subdirs:** Under `music_root` create (or symlink) **INTERNAL**, **USB**, **NAS**, **SMB** (same names as stock Volumio / Node `stickingMusicLibrary`).

Browse root `GET /api/v1/browse?uri=music-library` returns these four sources with `albumart` (bundled `sourceicon` PNGs). Optional `music_sources.local`, `usb`, `nas`, `smb` in config document paths for installers.

## Layer

Apply the `layer/` contents on a minimal Pi OS or Debian Trixie image. See [layer/README.md](layer/README.md).

## More documentation

| Document | Role |
|----------|------|
| [docs/DOCUMENTATION_MAP.md](docs/DOCUMENTATION_MAP.md) | **Index:** supremacy, authority, done vs not ported |
| [docs/CONCEPT.md](docs/CONCEPT.md) | **Fabric contract** |
| [docs/PORTING.md](docs/PORTING.md) | volumio3-backend → Evo parity |
| [docs/TESTER_GUIDE.md](docs/TESTER_GUIDE.md) | On-device bootstrap (canonical test) |
| [docs/BRANDED_BOOT.md](docs/BRANDED_BOOT.md) | Plymouth / boot branding |
| [docs/OS_PRIVILEGE_MODEL.md](docs/OS_PRIVILEGE_MODEL.md) | Sudoers, non-interactive contract |
| [docs/PLAYBACK_STATE_REQUIREMENTS.md](docs/PLAYBACK_STATE_REQUIREMENTS.md) | `pushState` / `pushQueue` |
| [docs/PLUGIN_ABI.md](docs/PLUGIN_ABI.md) | WASM plugin ABI |
| [docs/PLUGIN_SYSTEM_EXTENSIONS.md](docs/PLUGIN_SYSTEM_EXTENSIONS.md) | Hosts + trust; native stacks |
| [docs/PLUGIN_CORE_VS_EXTENSIONS.md](docs/PLUGIN_CORE_VS_EXTENSIONS.md) | Monolith vs fabric (transitional) |
| [docs/UI_PLUGIN_ROUTING.md](docs/UI_PLUGIN_ROUTING.md) | Stock UI wire routes (compatibility) |
| [docs/BUILD_GUIDE.md](docs/BUILD_GUIDE.md) | Cross-compilation, `layer/binaries/` |
| [docs/NETWORK_NM.md](docs/NETWORK_NM.md) | NetworkManager / `nmcli` |
| [docs/KIOSK.md](docs/KIOSK.md) | Wayland kiosk reference |
| [docs/UI_GAP.md](docs/UI_GAP.md) | Optional Volumio2-UI notes |

All other `docs/*.md` files: [docs/DOCUMENTATION_MAP.md](docs/DOCUMENTATION_MAP.md).

## License

GPL-2.0. See [LICENSE](LICENSE).
