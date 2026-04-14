# Prebuilt `volumio-evo` binaries (Linux)

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

**Refreshing:** Rebuild per [docs/BUILD_GUIDE.md](../../docs/BUILD_GUIDE.md), copy `target/<triple>/release/volumio-evo` here, update `SHA256SUMS`.

Bootstrap prefers **`layer/binaries/<triple>/volumio-evo`** when it matches **`uname -m`** (see `EVO_USE_LAYER_BINARY` in `scripts/bootstrap-volumio-evo-player.sh`).
