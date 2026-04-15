# Volumio Evo layer

Apply this on top of a minimal base image (Raspberry Pi OS Lite or Debian Trixie) to turn it into Volumio Evo. For detailed step-by-step instructions (including MPD setup and validation), see [docs/TESTER_GUIDE.md](../docs/TESTER_GUIDE.md).

## Contents

- **systemd/** - `volumio-evo.service` for the backend process.
- **binaries/** - Prebuilt **`volumio-evo`** per Linux target triple (`binaries/README.md`). Bootstrap installs the matching binary when present (avoids `cargo build` on device).
- **config/** - Example config (`volumio-evo.toml.example`). Copy to `/etc/volumio-evo/config.toml` and adjust.
- **web/** - Vendored static UI trees for **classic** / **contemporary** / **manifest** (see `web/README.md`). Used by the bootstrap script when present.
- **volumio2-ui-overlay/** - Optional reference patches for **host-side** Volumio2-UI builds (not used by bootstrap; see `volumio2-ui-overlay/README.txt`).

## How to apply

1. Copy the `volumio-evo` binary to `/usr/local/bin/` (or `/usr/bin/`).
2. Create `/usr/share/volumio-evo/plugins` and drop `.wasm` plugin files there.
3. Install the systemd unit: copy `systemd/volumio-evo.service` to `/etc/systemd/system/`, then `systemctl daemon-reload` and `systemctl enable volumio-evo`.
4. Install config: copy `config/volumio-evo.toml.example` to `/etc/volumio-evo/config.toml` and edit (bind, plugin_dir, mpd_host, mpd_port, music_sources.music_root). Set **music_root** at install or first run so MPD and Evo use the same path; the service may run as a different user (e.g. `pi`), so you can set `VOLUMIO_EVO_MUSIC_ROOT` in a systemd override instead of editing config.
5. Create music_root and subdirs (INTERNAL, USB, NAS, SMB); point MPD's `music_directory` at music_root.
6. Start: `systemctl start volumio-evo`.

Automation (e.g. Ansible playbook) can be added later to do the above from a single "apply layer" step.
