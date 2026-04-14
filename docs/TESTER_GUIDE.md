# Volumio 4 (Evo) – Tester guide: from a plain OS to a working setup

Use this on a **fresh** Raspberry Pi OS, Debian Trixie, or Ubuntu 24.04 (Desktop or Server). Steps are copy-paste where possible.

---

## Fast path for testers: open UI and play

On a fresh machine, run one command on the device:

```bash
cd /path/to/volumio-evo-repo
sudo bash ./scripts/bootstrap-volumio-evo-player.sh
```

Then the tester only does:

1. Open `http://<device-ip>/playback`
2. Select a track in the UI
3. Press Play and confirm audio from speaker

This script installs dependencies, clones/updates required repos, builds backend and UI, configures MPD/systemd/nginx, writes `local-config.json`, and serves the UI on port `80`.

---

## What you need from the developer

Before starting, get from the developer:

1. **The `volumio-evo` binary** for your machine (e.g. for Raspberry Pi 64-bit, or for PC/amd64). The developer can build it using [BUILD_GUIDE.md](BUILD_GUIDE.md).
2. **(Optional but recommended)** A **built Volumio UI** (a folder of static files, often zipped). Evo does **not** ship the web interface; you run the UI with a **separate** small web server and point it at the Evo backend. See **[How to start the Volumio UI (optional)](#how-to-start-the-volumio-ui-optional)** below. Without the UI, you can still verify the backend with `curl` and browser URLs (e.g. `/api/health`).

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

**If you build the UI yourself (developers):** Clone [Volumio2-UI](https://github.com/volumio/Volumio2-UI), follow its README (`npm install`, `bower install`, then e.g. `npm run build:volumio` for theme `volumio`, or `npm run build:volumio3` for `volumio3`). Put `local-config.json` into `dist/app/` before zipping. For live development, `gulp serve --theme="volumio"` with `src/app/local-config.json` pointing at the Evo device works too.

---

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

## Step 5 – How to test (what to validate)

**Backend only (no UI):**

- From another computer on the same network, open a browser and go to:  
  `http://<IP-OF-THE-DEVICE>:3000/api/health`  
  Example: `http://192.168.1.50:3000/api/health`. You should see `ok`.
- You can also try:  
  `http://<IP>:3000/api/v1/getState`  
  You should get JSON (playback state).  
  That confirms the backend is reachable and the API works.

**Full experience (with Volumio UI):**

- Follow **[How to start the Volumio UI (optional)](#how-to-start-the-volumio-ui-optional)** above: serve the built UI on a **different port** (e.g. 8080), set **`app/local-config.json`** so `"localhost"` is `http://<IP-OF-THE-DEVICE>:3000`, then open the UI in the browser on that port.
- Then use the UI to browse music, play/pause, change volume, check playlists, etc., and report any wrong or missing behaviour.

**Simple checklist for the tester:**

1. Backend starts: `systemctl status volumio-evo` shows "active (running)".
2. Health: opening `http://<device-IP>:3000/api/health` in a browser shows `ok`.
3. If you have the UI: you can open the UI, see "My Music" (or similar), browse into **local** (or the folder where you put files), add a track to the queue, and play it. You hear audio and the UI updates (play/pause, progress, etc.).
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
