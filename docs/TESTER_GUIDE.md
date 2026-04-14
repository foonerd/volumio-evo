# Volumio 4 (Evo) – Tester guide: from a plain OS to a working setup

Use this on a **fresh** Raspberry Pi OS (including **Raspberry Pi OS Lite**), Debian Trixie, or Ubuntu 24.04 (Desktop or Server).

## Policy (read this)

**Full-stack install and test on the device are defined only by `scripts/bootstrap-volumio-evo-player.sh`.**

Do **not** use a parallel workflow of manual `git pull`, `cargo build`, or hand-edited MPD/nginx as your “official” test. **Re-run the same script** when you need to refresh sources or rebuild: it clones or pulls **volumio-evo**, installs rustup, builds the Rust backend, copies static UI from **`layer/web/`**, and configures services. There is **no** npm/gulp or Volumio2-UI clone on device.

[BUILD_GUIDE.md](BUILD_GUIDE.md) is for cross-compiling or host-side binaries — **not** a substitute for on-device verification with bootstrap.

---

## Run bootstrap (only command that matters)

As **root**, from a directory that contains **`scripts/bootstrap-volumio-evo-player.sh`** (clone, tarball, or copy onto the machine; the script will clone/update **`volumio-evo`** under **`BASE_DIR`**, default `/opt/volumio`). The repo must include **`layer/web/{classic,contemporary,manifest}`** with **`index.html`** in each, **or** set **`UI_DIST_SOURCE`** to a single prebuilt **`dist/`** (see script **`--help`**).

```bash
cd /path/that/contains/the/script
sudo bash ./scripts/bootstrap-volumio-evo-player.sh
```

**Updates:** run the **same command again**. By default the script **git pull**s the **volumio-evo** checkout when **`EVO_REPO_UPDATE=1`**. Set **`EVO_REPO_UPDATE=0`** only for offline or pinned trees.

Then:

1. Open `http://<device-ip>/playback`
2. Select a track in the UI
3. Press Play and confirm audio from the speaker

The script installs packages, builds the backend (Rust), installs UI assets, configures MPD/systemd/nginx, and serves the web app on port **80**. **`GET /api/host`** is proxied to Evo so the UI gets a current Socket.IO base URL when the IP changes.

---

## What you need

1. **Network** on the device (for git and rustup/cargo crates unless you use fully offline mirrors and disable pulls).
2. **Root** (`sudo`).
3. **Optional:** `EVO_BINARY_PATH` if you are not building from source — normally the script builds from the cloned repo.

You do **not** run separate UI build steps: bootstrap copies **`layer/web`** (or **`UI_DIST_SOURCE`**) and nginx serves it.

---

## How to test (what to validate)

**Backend:**

- `http://<device-ip>:3000/api/health` should show `ok`.
- `http://<device-ip>:3000/api/v1/getState` should return JSON.

**Full UI (after bootstrap):**

- Open **`http://<device-ip>/`** — nginx serves the static UI on port **80**. The stock UI uses **`GET /api/host`** first for the API/Socket.IO base URL; **`app/local-config.json`** is only a fallback.

**Checklist:**

1. `systemctl status volumio-evo` shows **active (running)**.
2. Health URL above shows `ok`.
3. Open **`http://<device-IP>/`**, browse music, play — audio and UI updates behave as expected.
4. Report unexpected behaviour with steps to reproduce.

---

## If something goes wrong

- **"Connection refused" on port 3000**  
  Check `systemctl status volumio-evo` and firewall (`sudo ufw allow 3000` on Ubuntu if needed).

- **Empty library**  
  Music lives under `/var/lib/volumio-evo/music/` with subfolders **local**, **usb**, **nas**, **smb** — put files in one of them, then `sudo systemctl restart mpd` and optionally `sudo systemctl restart volumio-evo`.

- **Backend fails to start**  
  `journalctl -u volumio-evo -n 50 --no-pager` — send the last lines to the developer.

- **Git / Rust / missing UI**  
  Do not “fix” by running random `git pull` or `cargo` yourself and declaring success. Capture the **full bootstrap log** and the script version (commit) from the **`volumio-evo`** checkout the script used. Ensure **`layer/web`** is populated or set **`UI_DIST_SOURCE`**.

---

## Cross-compile / prebuilt binary (not the integration test)

If you need a **standalone binary** built on another machine, see [BUILD_GUIDE.md](BUILD_GUIDE.md). That path does **not** replace bootstrap for verifying the full player on a Raspberry Pi or Debian test host.
