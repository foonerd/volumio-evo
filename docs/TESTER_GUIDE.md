# Volumio 4 (Evo) – Tester guide: from a plain OS to a working setup

Use this on a **fresh** Raspberry Pi OS (including **Raspberry Pi OS Lite**), Debian Trixie, or Ubuntu 24.04 (Desktop or Server).

**Supported path:** On a stock, unmodified image, the **only** supported way to turn the machine into a default **Volumio Evo (Rust) player** test rig is to run our **`scripts/bootstrap-volumio-evo-player.sh`**. Do not hand-edit MPD, nginx, or install the UI yourself for a normal test cycle—that duplicates what the script does and drifts from what we ship.

The script installs packages, clones **read-only upstream** [Volumio2-UI](https://github.com/volumio/Volumio2-UI) plus **Evo-owned overlay** from this repo, builds backend (Rust) and UI, configures MPD/systemd/nginx, and serves the web app on port **80**. Testers use **published** `volumio-evo` sources (or a release tarball) that include this script—not ad-hoc forks of the UI repo.

---

## Fast path for testers: open UI and play

On the device, from your **published / checked-out `volumio-evo` tree**:

```bash
cd /path/to/volumio-evo-repo
sudo bash ./scripts/bootstrap-volumio-evo-player.sh
```

Then:

1. Open `http://<device-ip>/playback`
2. Select a track in the UI
3. Press Play and confirm audio from the speaker

---

## What you need before you run bootstrap

1. **A copy of `volumio-evo`** at a known revision (git clone of a **tag or branch** your team published for testing, or an unpacked release tree). The script must live under that tree (or set `EVO_REPO_DIR` if the checkout is elsewhere).
2. **Network access** on the device so the script can clone or update upstream repos (unless you point `EVO_REPO_DIR` / `UI_REPO_DIR` at offline mirrors).
3. **Optional:** a prebuilt `volumio-evo` binary and `EVO_BINARY_PATH` if you are not building from source—see [BUILD_GUIDE.md](BUILD_GUIDE.md) for how developers produce binaries.

You do **not** need a separate UI zip for the default path: bootstrap builds Volumio2-UI (with Evo overlay) and nginx serves it.

---

## Serving only the UI without bootstrap (optional)

If someone installed **only** the Evo backend binary (no bootstrap), the web app is not installed automatically. In that case you can serve a **prebuilt `dist/`** yourself and point it at the API—see **[How to start the Volumio UI (optional)](#how-to-start-the-volumio-ui-optional)** below. That path is **not** the default tester workflow.

---

## How to start the Volumio UI (optional)

**What it is:** The Volumio web app is plain HTML/JS/CSS after a build (the `dist/` output from the [Volumio2-UI](https://github.com/volumio/Volumio2-UI) project, or a zip the developer gives you that contains that folder).

**Why two servers:** Evo listens on port **3000** for the API and Socket.IO only. The UI must be opened from **another** URL (another port or another machine), e.g. `http://192.168.1.10:8080`, so the browser loads the app; the app then connects to Evo at `http://<device-ip>:3000`.

**Steps for the tester:**

1. **Unpack** the UI folder (e.g. `dist/`). You should see `index.html` and an `app/` subfolder (names may vary slightly by build).

2. **Tell the UI where Evo is.** Create or edit the file **`app/local-config.json`** inside that folder (same level as other files under `app/`). It must contain the full URL of the Evo backend, **including port 3000**:
   ```json
   {
     "localhost": "http://192.168.1.50:3000"
   }
   ```
   Replace `192.168.1.50` with the IP address of the machine where `volumio-evo` is running (use `127.0.0.1` only if you open the UI in a browser **on that same machine**).

3. **Serve the folder** with any static web server, from the directory that contains `index.html`:
   ```bash
   cd /path/to/unpacked-ui-folder
   python3 -m http.server 8080
   ```
   Or install `nginx` / use another tool; the important part is that you open **`http://<your-pc-ip>:8080`** (or `http://127.0.0.1:8080` on the same PC) in a browser—not port 3000.

4. **Open the browser** at that URL. The UI loads from the static server; it connects to Evo on port 3000 using `local-config.json`.

**Note:** The classic Volumio UI first tries `GET /api/host` on the **same host** as the page. Evo does not implement that route yet, so the UI falls back to **`/app/local-config.json`**—which is why that file must exist and point to Evo.

**If you build the UI yourself (developers):** Clone [Volumio2-UI](https://github.com/volumio/Volumio2-UI), follow its README (`npm install`, `bower install`, then e.g. `npm run build:volumio` for theme `volumio`, or `npm run build:volumio3` for `volumio3`). Put `local-config.json` into `dist/app/` before zipping. For live development, `gulp serve --theme="volumio"` with `src/app/local-config.json` pointing at the Evo device works too. Prefer **`bootstrap-volumio-evo-player.sh`** for a full device setup: it applies the Evo overlay and pins the same build steps testers rely on.

---

## Manual install (advanced only)

The steps below **repeat what the bootstrap script automates**—MPD layout, binary install, systemd, etc. Use them only when you **cannot** run bootstrap (e.g. debugging one layer) or to understand what the script does. **Testers doing a standard build on Raspberry Pi OS Lite or Debian Lite should use the [Fast path](#fast-path-for-testers-open-ui-and-play) only.**

## Step 1 – Prepare the system

Use one of:

- **Raspberry Pi:** Raspberry Pi OS (64-bit) – e.g. "Raspberry Pi OS Lite" or "with desktop".
- **PC / VM:** Debian Trixie or **Ubuntu 24.04** (Desktop or Server).

Boot the system, log in, and open a **terminal** (on Ubuntu Desktop: search for "Terminal").

Update the system (copy and run each block):

```bash
# On Debian / Raspberry Pi OS:
sudo apt update
sudo apt upgrade -y

# On Ubuntu 24.04:
sudo apt update
sudo apt upgrade -y
```

---

## Step 2 – Install MPD (music player daemon)

Volumio Evo uses MPD for playback. Install and enable it:

```bash
sudo apt install -y mpd
sudo systemctl enable mpd
sudo systemctl start mpd
```

Check that MPD is running:

```bash
systemctl status mpd
```

You should see "active (running)". (Press `q` to exit.)

---

## Step 3 – Create the music folder and configure MPD

Evo expects one "music root" folder with four subfolders: **local**, **usb**, **nas**, **smb**. MPD must use the same folder as its music directory.

Create the folders:

```bash
sudo mkdir -p /var/lib/volumio-evo/music/local
sudo mkdir -p /var/lib/volumio-evo/music/usb
sudo mkdir -p /var/lib/volumio-evo/music/nas
sudo mkdir -p /var/lib/volumio-evo/music/smb
```

Put at least a few music files (MP3, FLAC, etc.) in **one** of these (e.g. `local`) so you can test playback:

```bash
# Example: copy some music into "local" (replace with your own path if you have music elsewhere)
# sudo cp /path/to/your/music/*.mp3 /var/lib/volumio-evo/music/local/
```

Configure MPD to use this folder. Edit the config:

```bash
sudo nano /etc/mpd.conf
```

Find the line with `music_directory` and set it to:

```
music_directory "/var/lib/volumio-evo/music"
```

If that line is missing, add it in the "music" section. Save (Ctrl+O, Enter) and exit (Ctrl+X). Then restart MPD:

```bash
sudo systemctl restart mpd
```

---

## Step 4 – Install the Volumio Evo backend

**4.1 – Copy the binary**

You should have received the `volumio-evo` file. Copy it to the system (e.g. with USB stick, SCP, or download from a link the developer gave you). Then install it:

```bash
sudo cp /path/to/volumio-evo /usr/local/bin/volumio-evo
sudo chmod 755 /usr/local/bin/volumio-evo
```

Replace `/path/to/volumio-evo` with the actual path (e.g. `~/Downloads/volumio-evo` if it's in your Downloads folder).

**4.2 – Create config directory and config file**

```bash
sudo mkdir -p /etc/volumio-evo
```

If you have the volumio-evo source or layer, copy the example config and edit it:

```bash
sudo cp /path/to/volumio-evo/layer/config/volumio-evo.toml.example /etc/volumio-evo/config.toml
sudo nano /etc/volumio-evo/config.toml
```

Otherwise create the config file manually:

```bash
sudo nano /etc/volumio-evo/config.toml
```

Paste this (it uses the same music path as above):

```toml
bind = "0.0.0.0:3000"
plugin_dir = "/usr/share/volumio-evo/plugins"
mpd_host = "127.0.0.1"
mpd_port = 6600
albumart_root = "/var/lib/volumio-evo/albumart"

[music_sources]
music_root = "/var/lib/volumio-evo/music"
```

Save (Ctrl+O, Enter) and exit (Ctrl+X).

**4.3 – Create plugin and album-art directories**

```bash
sudo mkdir -p /usr/share/volumio-evo/plugins
sudo mkdir -p /var/lib/volumio-evo/albumart
```

Plugins are optional; the folder can be empty. If the developer gave you `.wasm` plugin files, copy them into `/usr/share/volumio-evo/plugins/`.

**4.4 – Install the systemd service**

If you have the volumio-evo source or layer, copy the service file:

```bash
sudo cp /path/to/volumio-evo/layer/systemd/volumio-evo.service /etc/systemd/system/
```

Otherwise create it manually:

```bash
sudo nano /etc/systemd/system/volumio-evo.service
```

Paste:

```ini
[Unit]
Description=Volumio Evo backend
After=network.target sound.target mpd.service

[Service]
Type=simple
ExecStart=/usr/local/bin/volumio-evo
Restart=on-failure
RestartSec=5
Environment=VOLUMIO_EVO_CONFIG=/etc/volumio-evo/config.toml

[Install]
WantedBy=multi-user.target
```

Save and exit. Then:

```bash
sudo systemctl daemon-reload
sudo systemctl enable volumio-evo
sudo systemctl start volumio-evo
```

**4.5 – Check that the backend is running**

```bash
systemctl status volumio-evo
```

You should see "active (running)". Then check the API from the same machine:

```bash
curl http://127.0.0.1:3000/
curl http://127.0.0.1:3000/api/health
curl http://127.0.0.1:3000/api/v1/ping
```

You should get: `ok`, `ok`, and `"pong"`. If you do, the backend is working.

---

## How to test (what to validate)

Use this after the [fast path](#fast-path-for-testers-open-ui-and-play) **or** after a manual install.

**Backend only (no UI):**

- From another computer on the same network, open a browser and go to:  
  `http://<IP-OF-THE-DEVICE>:3000/api/health`  
  Example: `http://192.168.1.50:3000/api/health`. You should see `ok`.
- You can also try:  
  `http://<IP>:3000/api/v1/getState`  
  You should get JSON (playback state).  
  That confirms the backend is reachable and the API works.

**Full experience (with Volumio UI):**

- **After bootstrap:** open **`http://<device-ip>/`** (nginx serves the built UI on port **80**; the script wires `local-config.json` to the Evo API on port 3000). Use **`/playback`** or the usual navigation to play music.
- **Without bootstrap (UI served separately):** follow **[How to start the Volumio UI (optional)](#how-to-start-the-volumio-ui-optional)**: static server on another port (e.g. 8080), **`app/local-config.json`** with `"localhost": "http://<device-ip>:3000"`.

Then use the UI to browse music, play/pause, change volume, check playlists, etc., and report any wrong or missing behaviour.

**Simple checklist for the tester:**

1. Backend starts: `systemctl status volumio-evo` shows "active (running)".
2. Health: opening `http://<device-IP>:3000/api/health` in a browser shows `ok`.
3. **Bootstrap path:** open `http://<device-IP>/` and exercise playback from the UI. **Manual UI zip path:** open your static server URL as above.
4. Any screen that doesn't load, shows an error, or does something different from what you expect should be reported (with what you clicked and what you saw).

---

## If something goes wrong

- **"Connection refused" when opening http://...:3000**  
  Backend not running or firewall blocking. Check:  
  `systemctl status volumio-evo`  
  and (if needed):  
  `sudo ufw allow 3000`  
  (Ubuntu) or adjust firewall on your OS.

- **No music in the UI / "empty library"**  
  Check that MPD's `music_directory` in `/etc/mpd.conf` is exactly `/var/lib/volumio-evo/music`, that the four subdirs (local, usb, nas, smb) exist, and that you put at least one music file in one of them. Then:  
  `sudo systemctl restart mpd`  
  and optionally restart Evo:  
  `sudo systemctl restart volumio-evo`.

- **Backend fails to start**  
  Check logs:  
  `journalctl -u volumio-evo -n 50 --no-pager`  
  Send the last 20–30 lines to the developer.

---

## Building from source (if you don't have a binary)

If the developer did not provide a binary, follow **[BUILD_GUIDE.md](BUILD_GUIDE.md)** for step-by-step build instructions for your architecture. In short: clone the repo, install Rust, then run `cargo build --release` for a native build (binary at `target/release/volumio-evo`), or use the cross-compilation steps in the build guide for Raspberry Pi 64-bit or other targets.
