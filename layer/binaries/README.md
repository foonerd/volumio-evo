# Prebuilt `volumio-evo` + kiosk browser (Linux)

## OOTB policy (non-negotiable for installers)

**Testers and devices must not compile Rust** to get a working player: `scripts/bootstrap-volumio-evo-player.sh` installs the **checked-in** `volumio-evo` binary from this directory (unless **`--build`** / **`EVO_BUILD_FROM_SOURCE`**).

The on-device **Wayland kiosk** uses a second binary, **`volumio-evo-kiosk-browser`**, from the same layout. `layer/kiosk-wpe/install.sh` prefers **`layer/binaries/${triple}/volumio-evo-kiosk-browser`** when present (see comments in that script); without it the installer falls back to **`cargo build -p volumio-evo-kiosk-browser --release`**, which requires GTK/WebKit dev packages and is intended for developer machines—not production Pis.

Whenever **`crates/core`** changes behaviour—see **[docs/PORTING.md](../../docs/PORTING.md)** and **[docs/DOCUMENTATION_MAP.md](../../docs/DOCUMENTATION_MAP.md)**—**maintainers must** refresh **`volumio-evo`** release artefacts and **`SHA256SUMS`**. After **`crates/kiosk-browser`** changes, rebuild and copy **`volumio-evo-kiosk-browser`** per triple the same way.

Rust **release** builds checked in for offline / fast bootstrap. Layout matches `rustc` target triples:

| Directory | Typical hardware |
|-----------|-------------------|
| **aarch64-unknown-linux-gnu/** | Raspberry Pi 3+ 64-bit, other arm64 SBCs |
| **armv7-unknown-linux-gnueabihf/** | 32-bit armhf (Pi 2, Zero 2 W, many armhf images) |
| **x86_64-unknown-linux-gnu/** | PCs, x86_64 VMs |

**armv7 build:** `--no-default-features` (no wasmtime; 32-bit ARM is unsupported by Cranelift). **aarch64/x86_64** builds use default features (WASM plugin host enabled).

**Kiosk browser build** (workspace member excluded from default `cargo build --release`; needs **libgtk-4-dev**, **libwebkitgtk-6.0-dev**, **libsoup-3.0-dev**, **pkg-config** on the build host):

```bash
cargo build -p volumio-evo-kiosk-browser --release
# optional cross:
# cargo build -p volumio-evo-kiosk-browser --release --target aarch64-unknown-linux-gnu
```

Copy `target/<triple>/release/volumio-evo-kiosk-browser` to **`layer/binaries/<triple>/volumio-evo-kiosk-browser`** (same `<triple>` as `volumio-evo`).

**Integrity:** `SHA256SUMS` in this directory lists every file that exists under the triple directories (backend + kiosk browser when checked in). Verify:

```bash
( cd "$(dirname "$0")" && sha256sum -c SHA256SUMS )
```

**Refreshing checksums** after copying any binary:

```bash
./scripts/refresh-layer-binaries-sha256sums.sh
```

Manual one-liner equivalent (only sums files that exist):

```bash
cd layer/binaries
./../scripts/refresh-layer-binaries-sha256sums.sh
```

**Bootstrap:** When the repo is available and **`--build`** is not used, `scripts/bootstrap-volumio-evo-player.sh` maps **`uname -m`** to a Rust triple (`host_rust_triple`) and installs **`layer/binaries/${triple}/volumio-evo`** if that file is executable; otherwise it errors unless you pass **`--build`**.
