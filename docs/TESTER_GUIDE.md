# Volumio 4 (Evo) – Tester guide: from a plain OS to a working setup

Use this on a **fresh** Raspberry Pi OS (including **Raspberry Pi OS Lite**), Debian Trixie, or Ubuntu 24.04 (Desktop or Server).

## Policy (read this)

**Full-stack install and test on the device are defined only by `scripts/bootstrap-volumio-evo-player.sh`.**

Do **not** use a parallel workflow of manual `git pull`, `cargo build`, or hand-edited MPD/nginx as your “official” test. **Re-run the same script** when you need to refresh sources or reinstall: it clones or pulls **volumio-evo**, copies static UI from **`layer/web/`**, configures MPD/systemd/nginx, and installs the backend. **By default** (no **`--build`**) the script installs the **prebuilt** **`volumio-evo`** from **`layer/binaries/<arch-triple>/`** when that file exists and matches **`uname -m`** — no **rustup/cargo** on the device. Pass **`--build`** (or set **`EVO_BUILD_FROM_SOURCE=1`**) to compile the Rust binary on the device (**installs rustup**; slow on a Pi). There is **no** npm/gulp or Volumio2-UI clone on device.

[BUILD_GUIDE.md](BUILD_GUIDE.md) is for cross-compiling or host-side binaries — **not** a substitute for on-device verification with bootstrap.

**Documentation index** (assumptions, canonical topics, implemented vs not): [DOCUMENTATION_MAP.md](DOCUMENTATION_MAP.md).

---

## Run bootstrap (only command that matters)

As **root**, run the script (path to the script only matters for finding it; the checkout lives at **`EVO_REPO_DIR`**, default **`/opt/volumio/volumio-evo`**). The script **always tries to clone or git pull** that repo — it does **not** skip cloning just because a binary already exists under **`/usr/local/bin`**. The **backend binary** for normal runs comes from **`layer/binaries/<triple>/volumio-evo`** inside that checkout unless you use **`--build`**.

The repo must include **`layer/web/{classic,contemporary,manifest}`** with **`index.html`** in each, **or** set **`UI_DIST_SOURCE`** to a single prebuilt **`dist/`** (see **`--help`**). Air-gapped installs can use **`EVO_ALLOW_BINARY_FALLBACK=1`** (not recommended for a full UI).

```bash
sudo /path/to/bootstrap-volumio-evo-player.sh
```

**No repo on disk yet** (one-liner; clones then runs the same bootstrap):

```bash
curl -fsSL https://raw.githubusercontent.com/foonerd/volumio-evo/main/install.sh | sudo bash
```

Optional: `EVO_REPO_URL`, `EVO_GIT_REF` (default **`main`**), `BASE_DIR` / `EVO_REPO_DIR`. Pass bootstrap flags after `--`, e.g. `| sudo bash -s -- --build`.

By default, bootstrap picks the **current session user** (e.g. **`SUDO_USER`** when you use `sudo`), so **`volumio-evo`** usually runs as your **SSH login** without extra flags. To force root or another account, see [RUNTIME_USER.md](RUNTIME_USER.md). **Sudo, `systemctl`, and `/etc` ownership** are defined in [OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md) — the service must stay **non-interactive** (no password prompts in normal operation).

**Modes** (see script **`--help`**): **`--full`** (default), **`--reset`** (stop backend first, then full reinstall), **`--upgrade-evo`** (backend binary only), **`--upgrade-nginx`** / **`--apply-ui-only`** (nginx + UI roots from config).

**Updates:** run the **same command again**. By default the script **git pull**s **`EVO_REPO_DIR`** when **`EVO_REPO_UPDATE=1`**. Set **`EVO_REPO_UPDATE=0`** only for offline or pinned trees.

Then:

1. Open `http://<device-ip>/playback`
2. Select a track in the UI
3. Press Play and confirm audio from the speaker

The script installs packages, installs the backend (prebuilt binary by default, or **`cargo`** build with **`--build`**), installs UI assets, configures MPD/systemd/nginx, and serves the web app on port **80**. Evo listens on **`3000`**; **`GET /api/host`** is proxied by nginx so the UI gets a current Socket.IO base URL when the IP changes (the Socket.IO client still connects to **`http://<ip>:3000`** per that response; nginx does not proxy **`/socket.io`** by default — see [PORTING.md](PORTING.md)).

---

## What you need

1. **Network** on the device (for **`git clone`** / **`git pull`** of the repo). **rustup/cargo** are only needed if you pass **`--build`** (or otherwise force on-device compile).
2. **Root** (`sudo`).
3. **Checkout** must contain **`layer/binaries/<triple>/volumio-evo`** for your architecture when **not** using **`--build`** (see [BUILD_GUIDE.md](BUILD_GUIDE.md) and **`layer/binaries/README.md`**). If git is unavailable, **`EVO_ALLOW_BINARY_FALLBACK=1`** and a pre-placed **`EVO_BINARY_PATH`** are documented in the script **`--help`** (limited; not a full UI install).
4. **Network storage (optional):** Bootstrap installs **`cifs-utils`**, **`nfs-common`**, **`smbclient`**, and **`avahi-utils`** by default (mounts, LAN SMB browse via **`avahi-browse`**, **`smbclient -L`**). Set **`EVO_INSTALL_NETWORK_STORAGE_PKGS=0`** to skip on air-gapped or minimal images. **`avahi-daemon`** should be active on the OS for mDNS discovery.

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
  Music lives under `/var/lib/volumio-evo/music/` with subfolders **INTERNAL**, **USB**, **NAS**, **SMB** — put files in one of them, then `sudo systemctl restart mpd` and optionally `sudo systemctl restart volumio-evo`.

- **Backend fails to start**  
  `journalctl -u volumio-evo -n 100 --no-pager` — send the last lines to the developer. Filter to Evo-formatted lines: `journalctl -u volumio-evo -n 200 --no-pager | grep -F '[EVO]'` (see [OBSERVABILITY.md](OBSERVABILITY.md)).

- **Git / Rust / missing UI**  
  Do not “fix” by running random `git pull` or `cargo` yourself and declaring success. Capture the **full bootstrap log** and the script version (commit) from the **`volumio-evo`** checkout the script used. Ensure **`layer/web`** is populated or set **`UI_DIST_SOURCE`**.

---

## Cross-compile / prebuilt binary (not the integration test)

If you need a **standalone binary** built on another machine, see [BUILD_GUIDE.md](BUILD_GUIDE.md). That path does **not** replace bootstrap for verifying the full player on a Raspberry Pi or Debian test host.
