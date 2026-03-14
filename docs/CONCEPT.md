# Volumio Evo – concept outline

## 1. Base system (no debootstrap)

- **Before:** Build rootfs from scratch with multistrap/debootstrap and long bash pipelines.
- **After:** Start from a **stock minimal image** and add Volumio on top.
  - **Pi:** e.g. Raspberry Pi OS Lite (or Debian Trixie when available).
  - **Other / generic:** Minimal Debian Trixie (netinst, cloud, or board image).
- **Method:** “Base image + Volumio layer” (Ansible, installer script, or package). No custom rootfs build.

## 2. Backend: Rust core + WASM plugins (no Node)

- **Core (Rust):** One static binary per arch (arm/arm64/amd64). HTTP/WebSocket API for the UI. Talks to MPD, system (e.g. systemd, ALSA), config. Loads and runs plugins as WASM modules.
- **Plugins (WASM):** Extensions are `.wasm` modules; one clear ABI (e.g. init + handle request). Sandboxed: no arbitrary OS access unless the host exposes it. Add/update by adding or replacing files; no recompile of the core.
- **Result:** No Node, no npm on device; robust, single binary, easy to extend via plugins.

## 3. Frontend

- **UI:** Can stay as-is (e.g. React) and talk to the new backend over HTTP/WebSocket.
- **Deployment:** Backend binary + plugin `.wasm` files + UI assets; all laid on top of the base image.

## 4. Build / image pipeline (simplified)

- **Input:** Official minimal image (Pi OS Lite or Debian Trixie).
- **Steps:** (1) Get base image, (2) optionally resize partitions, (3) apply Volumio layer (backend binary, plugins, UI, config, systemd, MPD, etc.), (4) optionally repack and ship a “Volumio image.”
- **Device-specific:** Only what’s really device-specific (bootloader, kernel/firmware, partition layout) stays as scripts or profiles; no more giant per-device bash recipes building the whole rootfs.

## 5. One-line summary

**Stock minimal OS + Volumio layer (Rust + WASM backend, existing UI); no debootstrap, no Node; plugins as sandboxed WASM.**
