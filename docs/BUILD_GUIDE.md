# Building Volumio Evo

Produces the **`volumio-evo`** **steward** binary ([CONCEPT.md](CONCEPT.md) §5); workspace members may include kiosk browser and WASM example guests — not the entire future plugin catalogue.

**On-device integration test (full stack: Rust + static UI + nginx + MPD):** run **`scripts/bootstrap-volumio-evo-player.sh` only** — not manual `git pull` or `cargo build` as a substitute. This document is for **cross-compiling** or **host-side** `cargo` when you need a binary artifact; it does not replace bootstrap for verification on a Raspberry Pi or Debian test machine.

---

Step-by-step instructions to build the `volumio-evo` binary for each supported architecture. Use this guide when you need a prebuilt binary or host development — see [TESTER_GUIDE.md](TESTER_GUIDE.md) for the canonical device workflow.

---

## Prerequisites

### Rust toolchain

**Do not use Debian/Raspberry Pi OS `apt install rustc cargo` for this project** — the packaged compiler is often too old; dependencies need a current **stable** (see `rust-toolchain.toml` in the repo root). **`scripts/bootstrap-volumio-evo-player.sh`** installs **rustup** under `/usr/local/rustup` and `/usr/local/cargo` and builds with that toolchain.

For a manual dev machine install:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then:

```bash
rustup update stable
```

### For cross-compilation: choose one method

| Method | Best for | Notes |
|--------|----------|--------|
| **cross** | Easiest, no host libs | Uses Docker; run `cargo install cross` and have Docker installed. |
| **System toolchain** | CI, no Docker | Install the appropriate `gcc-*-linux-gnu` package and set the linker (see per-arch steps). |

- **cross:** `cargo install cross`. Requires [Docker](https://docs.docker.com/get-docker/) to be installed and running.
- **System toolchain (Debian/Ubuntu):** Install the target compiler, e.g. `sudo apt install gcc-aarch64-linux-gnu` for arm64.

---

## Clone the repository

```bash
git clone https://github.com/foonerd/volumio-evo.git
cd volumio-evo
```

The bootstrap script uses the same public clone URL by default. When `github.com/volumio/volumio-evo` is published and anonymously cloneable, use that URL instead.

---

## Architecture overview

| Target triple | Architecture | Use case | WASM plugins |
|---------------|--------------|----------|--------------|
| **(host)** | Same as your PC/device | Local run, same-arch testing | Yes |
| **aarch64-unknown-linux-gnu** | arm64 (64-bit ARM) | Raspberry Pi OS 64-bit, Rock Pi, Khadas, Debian arm64 | Yes |
| **x86_64-unknown-linux-gnu** | amd64 (64-bit x86) | Debian/Ubuntu PC, VM, x86 NAS | Yes |
| **armv7-unknown-linux-gnueabihf** | armhf (32-bit ARM) | Raspberry Pi 0/1, 32-bit Pi OS | No (core only) |

---

## 1. Native build (same architecture as your machine)

Use this when you are building **on** the same architecture you will run on (e.g. on a Raspberry Pi 64-bit for Pi, or on an x86_64 PC for a PC/VM).

```bash
cd volumio-evo
cargo build --release -p volumio-evo-core
```

**Output binary:**

- `target/release/volumio-evo`

The workspace has multiple members; **`-p volumio-evo-core`** builds only the main binary (recommended). **`cargo build --release`** without **`-p`** builds every workspace crate (including the example WASM plugin).

---

## 2. Cross-build: arm64 (Raspberry Pi 64-bit, Rock Pi, Debian arm64)

Use this to build **on an x86_64 PC** a binary that runs on **Raspberry Pi OS 64-bit**, Rock Pi, Khadas, or any Debian/Ubuntu arm64 system.

### 2a. Using `cross` (recommended if you have Docker)

```bash
cargo install cross
cd volumio-evo
cross build --release -p volumio-evo-core --target aarch64-unknown-linux-gnu
```

**Output binary:** `target/aarch64-unknown-linux-gnu/release/volumio-evo`

Copy this file to the Pi (e.g. via SCP, USB stick, or shared folder) and use it in the [Tester guide](TESTER_GUIDE.md) Step 4.

### 2b. Using system toolchain (no Docker)

On Debian or Ubuntu:

```bash
sudo apt update
sudo apt install -y gcc-aarch64-linux-gnu
rustup target add aarch64-unknown-linux-gnu
cd volumio-evo
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
cargo build --release -p volumio-evo-core --target aarch64-unknown-linux-gnu
```

**Output binary:** `target/aarch64-unknown-linux-gnu/release/volumio-evo`

---

## 3. Cross-build: amd64 (x86_64 PC, VM, Debian/Ubuntu PC)

Use this when you need an **x86_64 Linux** binary. If you are already on an x86_64 Linux host, use the [Native build](#1-native-build-same-architecture-as-your-machine) instead.

### 3a. On x86_64 host (native – no cross needed)

```bash
cd volumio-evo
cargo build --release -p volumio-evo-core
```

**Output binary:** `target/release/volumio-evo`

### 3b. On arm64 host (e.g. building on a Pi for a PC)

Using `cross`:

```bash
cargo install cross
cd volumio-evo
cross build --release -p volumio-evo-core --target x86_64-unknown-linux-gnu
```

**Output binary:** `target/x86_64-unknown-linux-gnu/release/volumio-evo`

Using system toolchain (Debian/Ubuntu on arm64):

```bash
sudo apt install -y gcc-x86-64-linux-gnu
rustup target add x86_64-unknown-linux-gnu
cd volumio-evo
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc
cargo build --release -p volumio-evo-core --target x86_64-unknown-linux-gnu
```

**Output binary:** `target/x86_64-unknown-linux-gnu/release/volumio-evo`

---

## 4. Cross-build: armhf (32-bit Raspberry Pi, Pi 0, Pi 1)

Use this for **32-bit Raspberry Pi OS** or Pi Zero (non-W) / Pi 1. This build **disables WASM plugins** (wasmtime does not support 32-bit ARM). The backend API and MPD playback work; plugin features are unavailable.

### 4a. Using `cross`

```bash
cargo install cross
cd volumio-evo
cross build --release -p volumio-evo-core --target armv7-unknown-linux-gnueabihf --no-default-features
```

**Output binary:** `target/armv7-unknown-linux-gnueabihf/release/volumio-evo`

### 4b. Using system toolchain

On Debian or Ubuntu:

```bash
sudo apt update
sudo apt install -y gcc-arm-linux-gnueabihf
rustup target add armv7-unknown-linux-gnueabihf
cd volumio-evo
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc
cargo build --release -p volumio-evo-core --target armv7-unknown-linux-gnueabihf --no-default-features
```

**Output binary:** `target/armv7-unknown-linux-gnueabihf/release/volumio-evo`

---

## Summary: where is the binary?

| Build type | Binary path |
|------------|-------------|
| Native | `target/release/volumio-evo` |
| arm64 cross | `target/aarch64-unknown-linux-gnu/release/volumio-evo` |
| amd64 cross | `target/x86_64-unknown-linux-gnu/release/volumio-evo` |
| armhf cross | `target/armv7-unknown-linux-gnueabihf/release/volumio-evo` |

**Shipping in-repo:** After cross-build, copy each `target/<triple>/release/volumio-evo` to **`layer/binaries/<triple>/volumio-evo`**, then regenerate **`layer/binaries/SHA256SUMS`** (exact command in **`layer/binaries/README.md`**). Bootstrap installs the matching triple from **`layer/binaries/`** on the device when present and **`--build`** is not used (no `cargo` on target). **OOTB policy:** any change to backend behaviour must ship updated prebuilts in that directory—see **`layer/binaries/README.md`** (“OOTB policy”).

**Devices pulling the repo:** installers use **shallow** git by default (**`EVO_REPO_DEPTH=1`**) so slow links do not transfer full history; see [TESTER_GUIDE.md](TESTER_GUIDE.md) (*Git checkout size*).

Use this binary in [TESTER_GUIDE.md](TESTER_GUIDE.md) Step 4.1 as the file to copy to `/usr/local/bin/volumio-evo` on the target device.

---

## Kiosk browser binary (`volumio-evo-kiosk-browser`)

The on-device **Wayland kiosk** uses a **second** release binary, built from **`crates/kiosk-browser/`** (workspace member; **excluded** from default **`cargo build --release`** at the repo root so normal dev/CI does not need GTK/WebKit C headers). The package is **`volumio-evo-kiosk-browser`**; it links **gtk4** + **webkit6** (webkit2gtk **6.0** on the system).

**Captured build dependencies:** the canonical Debian/Ubuntu package list for compiling this crate is **`crates/kiosk-browser/apt-build-deps.txt`** (native amd64 `apt-get install`). Transitive **`pkg-config`** libraries (`glib-2.0`, `cairo`, `gdk-pixbuf-2.0`, …) come from those `-dev` metapackages. **GTK version:** current **gtk-rs** bindings expect **GTK 4.10+** (`gtk4.pc`); **Ubuntu 22.04** often ships **GTK 4.6**, which is too old — use **Ubuntu 24.04+** or **Debian Trixie** (or pin older gtk crates).

**Host prerequisites (same list as the file above):** install **`build-essential`**, **`pkg-config`**, **`libgtk-4-dev`**, **`libwebkitgtk-6.0-dev`**, **`libsoup-3.0-dev`**. Runtime libraries on the device are installed by [`layer/kiosk-wpe/install.sh`](../layer/kiosk-wpe/install.sh).

**Native build (same arch as host):**

```bash
cd volumio-evo
cargo build --release -p volumio-evo-kiosk-browser
# target/release/volumio-evo-kiosk-browser
```

**Cross-build with `cross`:** stock cross Docker images use **Ubuntu ≤20.04**, which cannot **`apt`**-install **GTK 4.10+** / **webkit2gtk 6** dev packages, and their **glibc** is too old to run **build script** binaries that were produced on a **newer** host OS (shared **`target/`** reuse). Repo-root **`Cross.toml`** overrides:

- **`x86_64-unknown-linux-gnu`** → **`docker/cross-kiosk/Dockerfile.x86_64-unknown-linux-gnu`** (Ubuntu **24.04**).
- **`aarch64-unknown-linux-gnu`** → **`docker/cross-kiosk/Dockerfile.aarch64-unknown-linux-gnu`** (Noble + **ports.ubuntu.com** arm64 packages + **`aarch64-linux-gnu-*`** toolchain).
- **`armv7-unknown-linux-gnueabihf`** → **`docker/cross-kiosk/Dockerfile.armv7-unknown-linux-gnueabihf`** (Noble + **ports.ubuntu.com** armhf packages + **`arm-linux-gnueabihf-*`** toolchain).

Examples:

```bash
cargo install cross
cd volumio-evo
cross build --release -p volumio-evo-kiosk-browser --target x86_64-unknown-linux-gnu
# target/x86_64-unknown-linux-gnu/release/volumio-evo-kiosk-browser

cross build --release -p volumio-evo-kiosk-browser --target aarch64-unknown-linux-gnu
# target/aarch64-unknown-linux-gnu/release/volumio-evo-kiosk-browser

cross build --release -p volumio-evo-kiosk-browser --target armv7-unknown-linux-gnueabihf
# target/armv7-unknown-linux-gnueabihf/release/volumio-evo-kiosk-browser
```

If **`cross`** fails with **`GLIBC_2.xx not found`** on **`build-script-build`** under **`target/release/build/`**, run **`cargo clean`** (or delete **`target/release/build`**) so build scripts are **rebuilt inside** the container instead of reusing host-linked binaries from a shared **`target/`** directory.

**Cross-build without Docker** still needs a **sysroot** (or device-native build) that provides the same **`pkg-config`** files for the target triple:

```bash
cargo build --release -p volumio-evo-kiosk-browser --target aarch64-unknown-linux-gnu
# target/aarch64-unknown-linux-gnu/release/volumio-evo-kiosk-browser
```

**Ship in-repo** (required for OOTB kiosk on devices without `cargo` + dev headers): copy to **`layer/binaries/<triple>/volumio-evo-kiosk-browser`**, then run **`./scripts/refresh-layer-binaries-sha256sums.sh`**. See **[layer/binaries/README.md](../layer/binaries/README.md)**. On the device, **`layer/kiosk-wpe/install.sh`** prefers that prebuilt; it only runs **`cargo build -p volumio-evo-kiosk-browser --release`** as a fallback when the prebuilt is missing and a toolchain is available.

---

## Optional: build the example WASM plugin

The example plugin is built for the **wasm32** target and can be loaded by the Evo backend (on arm64/amd64 builds with WASM enabled). You do not need it for basic testing.

```bash
rustup target add wasm32-unknown-unknown
cargo build --release -p volumio-evo-plugin-example --target wasm32-unknown-unknown
```

**Output:** `target/wasm32-unknown-unknown/release/libvolumio_evo_plugin_example.wasm`. Copy it to the device’s plugin directory (e.g. `/usr/share/volumio-evo/plugins/`) if you want to test plugin loading. The backend discovers `.wasm` files in that directory.

---

## Troubleshooting

- **Linker errors when cross-compiling:** Ensure the correct `gcc-*-linux-gnu` package is installed and the `CARGO_TARGET_*_LINKER` env var is set (see the system-toolchain steps above).
- **Docker / cross: "cannot connect to Docker daemon":** Start Docker (e.g. `systemctl start docker` on Linux) or use the system toolchain method instead.
- **armhf build fails with wasmtime or "optional" feature:** You must pass `--no-default-features` for the armv7 target so the WASM plugin layer is disabled.
