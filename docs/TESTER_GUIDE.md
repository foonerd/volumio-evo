# Volumio 4 (Evo) – Tester guide: from a plain OS to a working setup

Use this on a **fresh** Raspberry Pi OS (including **Raspberry Pi OS Lite**), Debian Trixie, or Ubuntu 24.04 (Desktop or Server).

## Policy (read this)

**Full-stack build and test on the device are defined only by `scripts/bootstrap-volumio-evo-player.sh`.**

Do **not** use a parallel workflow of manual `git pull`, `cargo build`, `npm install`, `gulp`, or hand-edited MPD/nginx as your “official” test. **Re-run the same script** when you need to refresh sources or rebuild: it clones or pulls repos (see defaults below), installs rustup, builds the backend and UI, and configures services.

[BUILD_GUIDE.md](BUILD_GUIDE.md) is for cross-compiling or host-side binaries — **not** a substitute for on-device verification with bootstrap.

---

## Run bootstrap (only command that matters)

As **root**, from a directory that contains **`scripts/bootstrap-volumio-evo-player.sh`** (how that directory gets onto the machine is up to your team — clone, tarball, or copy; the script itself will clone/update **`volumio-evo`** and **Volumio2-UI** under **`BASE_DIR`**, default `/opt/volumio`):

```bash
cd /path/that/contains/the/script
sudo bash ./scripts/bootstrap-volumio-evo-player.sh
```

Optional: `sudo UI_THEME=volumio3 ./scripts/bootstrap-volumio-evo-player.sh` (default theme is already `volumio3`).

**Updates:** run the **same command again**. By default the script **git pull**s existing checkouts (`EVO_REPO_UPDATE=1`, `UI_REPO_UPDATE=1`). Set them to `0` only for offline or pinned trees.

Then:

1. Open `http://<device-ip>/playback`
2. Select a track in the UI
3. Press Play and confirm audio from the speaker

The script installs packages, clones **read-only upstream** [Volumio2-UI](https://github.com/volumio/Volumio2-UI), applies **Evo overlay** from the cloned **`volumio-evo`** tree, builds backend (Rust) and UI, configures MPD/systemd/nginx, and serves the web app on port **80**.

---

## What you need

1. **Network** on the device (for git and rustup/cargo crates unless you use fully offline mirrors and disable pulls).
2. **Root** (`sudo`).
3. **Optional:** `EVO_BINARY_PATH` if you are not building from source — normally the script builds from the cloned repo.

You do **not** run separate UI build steps: bootstrap builds Volumio2-UI (with Evo overlay) and nginx serves it.

---

## How to test (what to validate)

**Backend:**

- `http://<device-ip>:3000/api/health` should show `ok`.
- `http://<device-ip>:3000/api/v1/getState` should return JSON.

**Full UI (after bootstrap):**

- Open **`http://<device-ip>/`** — nginx serves the built UI on port **80**; the script wires **`local-config.json`** to the Evo API on port 3000.

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

- **Git / Rust / UI build errors**  
  Do not “fix” by running random `git pull` or `cargo`/`npm` yourself and declaring success. Capture the **full bootstrap log** and the script version (commit) from the **`volumio-evo`** checkout the script used.

---

## Cross-compile / prebuilt binary (not the integration test)

If you need a **standalone binary** built on another machine, see [BUILD_GUIDE.md](BUILD_GUIDE.md). That path does **not** replace bootstrap for verifying the full player on a Raspberry Pi or Debian test host.
