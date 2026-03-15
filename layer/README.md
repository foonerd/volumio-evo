# Volumio Evo layer

Apply this on top of a minimal base image (Raspberry Pi OS Lite or Debian Trixie) to turn it into Volumio Evo.

## Contents

- **systemd/** - `volumio-evo.service` for the backend process.
- **config/** - Example config (`volumio-evo.toml.example`). Copy to `/etc/volumio-evo/config.toml` and adjust.

## How to apply

1. Copy the `volumio-evo` binary to `/usr/local/bin/` (or `/usr/bin/`).
2. Create `/usr/share/volumio-evo/plugins` and drop `.wasm` plugin files there.
3. Install the systemd unit: copy `systemd/volumio-evo.service` to `/etc/systemd/system/`, then `systemctl daemon-reload` and `systemctl enable volumio-evo`.
4. Install config: copy `config/volumio-evo.toml.example` to `/etc/volumio-evo/config.toml` and edit (bind, plugin_dir, mpd_host, mpd_port, music_sources.music_root). Set **music_root** at install or first run so MPD and Evo use the same path; the service may run as a different user (e.g. `pi`), so you can set `VOLUMIO_EVO_MUSIC_ROOT` in a systemd override instead of editing config.
5. Create music_root and subdirs (local, usb, nas, smb); point MPD's `music_directory` at music_root.
6. Start: `systemctl start volumio-evo`.

Automation (e.g. Ansible playbook) can be added later to do the above from a single "apply layer" step.
