# Volumio Evo layer

The **fabric** runs on the device; the **layer** is how a stock OS becomes an Evo device ([**CONCEPT.md**](../docs/CONCEPT.md) §5, §7). Apply this on top of a minimal base image (Raspberry Pi OS Lite or Debian Trixie) to turn it into Volumio Evo. For detailed step-by-step instructions (including MPD setup and validation), see [docs/TESTER_GUIDE.md](../docs/TESTER_GUIDE.md). Persisted Evo state on disk (ALSA, MPD playback options, future subsystems) is namespaced under `/var/lib/volumio-evo/settings/` — see [docs/SETTINGS_LAYOUT.md](../docs/SETTINGS_LAYOUT.md). Journald filtering and log markers: [docs/OBSERVABILITY.md](../docs/OBSERVABILITY.md). Branded boot (Plymouth, **`VOL:v1`** branding stages, Pi 5 vs RC testing, theme/packaging roadmap): [docs/BRANDED_BOOT.md](../docs/BRANDED_BOOT.md).

## Contents

- **plymouth/** - Vendored **`volumio-adaptive`** theme and dev tool **`generate-overlays.sh`** (ImageMagick) to (re)build **`overlay-vol-*.png`** — see **`plymouth/README.md`**.
- **systemd/** - `volumio-evo.service` for the backend process. **Optional** `vol-branding-v1-*.service` units send `VOL:v1:...` strings to Plymouth for branded boot (see `systemd/vol-branding-v1.target` and each unit's header); copy to `/etc/systemd/system/`, `daemon-reload`, `enable` the target or individual services, and require `plymouth` + `splash` in the kernel cmdline. **`ExecStart=-/usr/bin/plymouth`** uses systemd's `-` prefix so `plymouth message` failing after the splash has quit (common on fast boots) does not mark the unit as failed.
- **install/** - Shell hooks shipped with the repo for bootstrap and sudoers-stable paths: **`run-boot-branding.sh`**, **`run-kiosk-wpe-install.sh`** (thin wrappers so **`sudo -n`** NOPASSWD lines stay narrow), **`volumio-evo-smb-user-sync.sh`**, etc. Copied or referenced by **`scripts/bootstrap-volumio-evo-player.sh`**.
- **binaries/** - Prebuilt **`volumio-evo`** per Linux target triple; optional **`volumio-evo-kiosk-browser`** for the Wayland kiosk (**`binaries/README.md`**). Bootstrap installs the matching **`volumio-evo`** when present (avoids **`cargo`** on device). Kiosk **`install.sh`** prefers **`volumio-evo-kiosk-browser`** from the same triple directory when checked in.
- **config/** - Example config (`volumio-evo.toml.example`). Copy to `/etc/volumio-evo/config.toml` and adjust.
- **web/** - Vendored static UI trees for **classic** / **contemporary** / **manifest** (see `web/README.md`). Used by the bootstrap script when present.
- **volumio2-ui-overlay/** - Optional reference patches for **host-side** Volumio2-UI builds (not used by bootstrap; see `volumio2-ui-overlay/README.txt`).
- **kiosk-wpe/** - Wayland kiosk layer (**labwc** + Rust **`volumio-evo-kiosk-browser`** from **`crates/kiosk-browser`**, squeekboard / wvkbd). Directory name and **`--with-kiosk=wpe`** / **`--kiosk-wpe`** are historical; the stack is **webkit2gtk**, not WPE. Installer, systemd units, helper scripts, **`labwc/rc.xml`**, kiosk.toml example. Enable at bootstrap with **`--with-kiosk=wpe`** or **`--kiosk-wpe`**; runtime on/off from **Settings → System → Kiosk** (backend + **`sudo -n`** — see [docs/KIOSK.md](../docs/KIOSK.md), [docs/OS_PRIVILEGE_MODEL.md](../docs/OS_PRIVILEGE_MODEL.md)).

## How to apply

1. Copy the `volumio-evo` binary to `/usr/local/bin/` (or `/usr/bin/`).
2. Create `/usr/share/volumio-evo/plugins` and drop `.wasm` plugin files there.
3. Install the systemd unit: copy `systemd/volumio-evo.service` to `/etc/systemd/system/`, then `systemctl daemon-reload` and `systemctl enable volumio-evo`.
4. Install config: copy `config/volumio-evo.toml.example` to `/etc/volumio-evo/config.toml` and edit (bind, plugin_dir, mpd_host, mpd_port, music_sources.music_root). Set **music_root** at install or first run so MPD and Evo use the same path; the service may run as a different user (e.g. `pi`), so you can set `VOLUMIO_EVO_MUSIC_ROOT` in a systemd override instead of editing config. **Prefer** running the daemon as your SSH login (not uid 1000): bootstrap defaults to the session user unless **`EVO_SERVICE_USER`** is set - see [docs/RUNTIME_USER.md](../docs/RUNTIME_USER.md).
5. Create music_root and subdirs (INTERNAL, USB, NAS, SMB); point MPD's `music_directory` at music_root.
6. Start: `systemctl start volumio-evo`.
7. Optional: enable the Wayland kiosk stack on a connected display (packages, **`volumio-evo-kiosk-browser`**, labwc units):  
   `sudo ./scripts/bootstrap-volumio-evo-player.sh --with-kiosk=wpe`  
   or **`--kiosk-wpe`** / **`KIOSK=wpe`**. Commit **`layer/binaries/<triple>/volumio-evo-kiosk-browser`** when possible so the device does not compile GTK. Actual **start/stop** of **`volumio-evo-kiosk.service`** is driven by **Settings → System → Kiosk** (or **`saveKioskSettings`** may run **`run-kiosk-wpe-install.sh`** first if the layer is missing).

Automation (e.g. Ansible playbook) can be added later to do the above from a single "apply layer" step.
