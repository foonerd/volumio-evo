# Volumio Evo layer

Apply this on top of a minimal base image (Raspberry Pi OS Lite or Debian Trixie) to turn it into Volumio Evo. For detailed step-by-step instructions (including MPD setup and validation), see [docs/TESTER_GUIDE.md](../docs/TESTER_GUIDE.md). Persisted Evo state on disk (ALSA, MPD playback options, future subsystems) is namespaced under `/var/lib/volumio-evo/settings/` — see [docs/SETTINGS_LAYOUT.md](../docs/SETTINGS_LAYOUT.md). Journald filtering and log markers: [docs/OBSERVABILITY.md](../docs/OBSERVABILITY.md). Branded boot (Plymouth, **`VOL:v1`** branding stages, Pi 5 vs RC testing, theme/packaging roadmap): [docs/BRANDED_BOOT.md](../docs/BRANDED_BOOT.md).

## Contents

- **plymouth/** - Vendored **`volumio-adaptive`** theme and dev tool **`generate-overlays.sh`** (ImageMagick) to (re)build **`overlay-vol-*.png`** — see **`plymouth/README.md`**.
- **systemd/** - `volumio-evo.service` for the backend process. **Optional** `vol-branding-v1-*.service` units send `VOL:v1:...` strings to Plymouth for branded boot (see `systemd/vol-branding-v1.target` and each unit's header); copy to `/etc/systemd/system/`, `daemon-reload`, `enable` the target or individual services, and require `plymouth` + `splash` in the kernel cmdline. **`ExecStart=-/usr/bin/plymouth`** uses systemd's `-` prefix so `plymouth message` failing after the splash has quit (common on fast boots) does not mark the unit as failed.
- **install/** - Shell hooks shipped with the repo for bootstrap and sudoers-stable paths (boot-branding wrappers, **`volumio-evo-smb-user-sync.sh`** - copied from here by **`scripts/bootstrap-volumio-evo-player.sh`**).
- **binaries/** - Prebuilt **`volumio-evo`** per Linux target triple (`binaries/README.md`). Bootstrap installs the matching binary when present (avoids `cargo build` on device).
- **config/** - Example config (`volumio-evo.toml.example`). Copy to `/etc/volumio-evo/config.toml` and adjust.
- **web/** - Vendored static UI trees for **classic** / **contemporary** / **manifest** (see `web/README.md`). Used by the bootstrap script when present.
- **volumio2-ui-overlay/** - Optional reference patches for **host-side** Volumio2-UI builds (not used by bootstrap; see `volumio2-ui-overlay/README.txt`).
- **kiosk-wpe/** - Wayland kiosk layer (**labwc** compositor + **GTK 4 / webkit2gtk** Python shell `volumio-evo-kiosk-browser`, squeekboard / wvkbd). The directory and `--with-kiosk=wpe` keep a historical name; the stack is not WPE-based (see `docs/KIOSK.md`). Installer, systemd units, helper scripts, `labwc/rc.xml`, and kiosk.toml example. Enabled via `--with-kiosk=wpe` on the main bootstrap; off by default. Backend control in **Settings -> System** (kiosk section; see `layer/kiosk-wpe/README.md`).

## How to apply

1. Copy the `volumio-evo` binary to `/usr/local/bin/` (or `/usr/bin/`).
2. Create `/usr/share/volumio-evo/plugins` and drop `.wasm` plugin files there.
3. Install the systemd unit: copy `systemd/volumio-evo.service` to `/etc/systemd/system/`, then `systemctl daemon-reload` and `systemctl enable volumio-evo`.
4. Install config: copy `config/volumio-evo.toml.example` to `/etc/volumio-evo/config.toml` and edit (bind, plugin_dir, mpd_host, mpd_port, music_sources.music_root). Set **music_root** at install or first run so MPD and Evo use the same path; the service may run as a different user (e.g. `pi`), so you can set `VOLUMIO_EVO_MUSIC_ROOT` in a systemd override instead of editing config. **Prefer** running the daemon as your SSH login (not uid 1000): bootstrap defaults to the session user unless **`EVO_SERVICE_USER`** is set - see [docs/RUNTIME_USER.md](../docs/RUNTIME_USER.md).
5. Create music_root and subdirs (INTERNAL, USB, NAS, SMB); point MPD's `music_directory` at music_root.
6. Start: `systemctl start volumio-evo`.
7. Optional: enable the Wayland kiosk on a connected display:
   `sudo scripts/bootstrap-volumio-evo-player.sh --with-kiosk=wpe`
   This installs the kiosk-wpe layer; the actual start/stop is controlled by the backend kiosk toggle under **Settings -> System**.

Automation (e.g. Ansible playbook) can be added later to do the above from a single "apply layer" step.
