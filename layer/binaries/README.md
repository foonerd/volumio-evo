# Prebuilt `volumio-evo` binaries (Linux)

## OOTB policy (non-negotiable for installers)

**Testers and devices must not compile Rust** to get a working player: `scripts/bootstrap-volumio-evo-player.sh` installs the **checked-in** binary from this directory (unless **`--build`** / **`EVO_BUILD_FROM_SOURCE`**).

Whenever **`crates/core`** changes behaviour—see **[docs/PORTING.md](../../docs/PORTING.md)** and **[docs/DOCUMENTATION_MAP.md](../../docs/DOCUMENTATION_MAP.md)**—**maintainers must** refresh **release** binaries below and **`SHA256SUMS`** before **`git pull` + bootstrap** reflects those changes.

Rust **release** builds checked in for offline / fast bootstrap. Layout matches `rustc` target triples:

| Directory | Typical hardware |
|-----------|-------------------|
| **aarch64-unknown-linux-gnu/** | Raspberry Pi 3+ 64-bit, other arm64 SBCs |
| **armv7-unknown-linux-gnueabihf/** | 32-bit armhf (Pi 2, Zero 2 W, many armhf images) |
| **x86_64-unknown-linux-gnu/** | PCs, x86_64 VMs |

**armv7 build:** `--no-default-features` (no wasmtime; 32-bit ARM is unsupported by Cranelift). **aarch64/x86_64** builds use default features (WASM plugin host enabled).

**Integrity:** `SHA256SUMS` in this directory. Verify after copy:

```bash
( cd "$(dirname "$0")" && sha256sum -c SHA256SUMS )
```

**Refreshing:** Rebuild per [docs/BUILD_GUIDE.md](../../docs/BUILD_GUIDE.md), copy `target/<triple>/release/volumio-evo` to **`layer/binaries/<triple>/volumio-evo`**, then regenerate **`SHA256SUMS`**:

```bash
cd layer/binaries
sha256sum aarch64-unknown-linux-gnu/volumio-evo armv7-unknown-linux-gnueabihf/volumio-evo x86_64-unknown-linux-gnu/volumio-evo > SHA256SUMS
```

**Bootstrap:** When the repo is available and **`--build`** is not used, `scripts/bootstrap-volumio-evo-player.sh` maps **`uname -m`** to a Rust triple (`host_rust_triple`) and installs **`layer/binaries/${triple}/volumio-evo`** if that file is executable; otherwise it errors unless you pass **`--build`**.
