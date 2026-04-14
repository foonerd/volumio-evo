# Building Volumio Evo

Step-by-step instructions to build the `volumio-evo` binary for each supported architecture. Use this guide to produce the binary you provide to testers (see [TESTER_GUIDE.md](TESTER_GUIDE.md)).

---

## Prerequisites

### Rust toolchain

Install Rust (if not already installed):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Choose the default installation. Then ensure the stable toolchain is installed:

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
cargo build --release
```

**Output binary:**

- `target/release/volumio-evo`

To build only the core (skips the example plugin, same binary):

```bash
cargo build --release -p volumio-evo-core
```

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

Use this binary in [TESTER_GUIDE.md](TESTER_GUIDE.md) Step 4.1 as the file to copy to `/usr/local/bin/volumio-evo` on the target device.

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
