# Volumio Evo - Kiosk layer component (labwc + webkit2gtk shell)

This directory is the runtime layer for the on-device kiosk. The design
reference is docs/KIOSK.md; this README documents what actually installs
and how it maps to the backend.

The kiosk is a Wayland fullscreen shell that renders the Evo UI
(http://127.0.0.1/ on the nginx root) on a connected display with touch,
keyboard, and mouse input. Base OS target is Debian 13 Trixie Lite with
no graphical stack pre-installed.

**Stack:**

- **labwc** — wlroots-based stacking compositor with wlr-layer-shell.
- **`volumio-evo-kiosk-browser`** — Rust binary built from **`crates/kiosk-browser`** (GTK **4** + **`webkit6`** crate / webkit2gtk **6** runtime). One maximized undecorated window, one **`WebView`**, one URL. Blocks popups, **`target=_blank`**, and context menus (see **`src/main.rs`**).
- **squeekboard** / **wvkbd** - on-screen keyboards (squeekboard is the
  default; it follows text-input-v3 focus events).

The directory is still named **`kiosk-wpe`**; bootstrap accepts **`--with-kiosk=wpe`**, **`--kiosk-wpe`**, or **`KIOSK=wpe`** for continuity. The runtime stack is **labwc + webkit2gtk**, not WPE.

## Why not cog/WPE, cage, epiphany, or chromium

Evaluated and rejected, in order:

1. **cog + WPE WebKit** on Trixie (libwpewebkit 2.48.3). Input dispatch
   to the DOM is broken on Pi 5 class hardware: libinput delivers
   pointer button events at the kernel layer (verified with `libinput
   debug-events`), but WPE's `wl` and `fdo` platform plugins drop them
   before reaching WebCore. UI renders and hover works; clicks never
   land. Same symptom under cage and under labwc, ruling out the
   compositor.
2. **cage**. Lacks `wlr_layer_shell_v1` - the OSK cannot render. Same
   input-dispatch problem as above when combined with cog.
3. **epiphany-browser** in `--application-mode`. Requires
   `xdg-desktop-portal` access to `/proc/<pid>/root` and a registered
   `org.gnome.Epiphany.WebApp_*` profile. Both fail inside a PAM-login
   systemd session. Without application mode, Epiphany is a full
   browser with chrome, tabs, and popups - not a kiosk.
4. **chromium**. ~250-400 MB resident; too heavy for Pi 1 / 2 / Zero
   class targets.
5. **midori**. No longer in Debian main.
6. **surf** (suckless). `-K` blocks keystrokes and right-click only,
   not `window.open` / `target=_blank`. Wayland support on Trixie is
   through GDK_BACKEND and tagged X11-interface - not production-ready.

Purpose-built GTK + webkit2gtk shell wins:

- Same WebKit engine family as WPE so the UI renders identically to
  what cog would have produced — zero UI-compatibility risk.
- GTK platform path for input is mature; button and keyboard events reach the DOM on the same
  hardware where WPE fails.
- Deterministic policy: **`crates/kiosk-browser`** is small and auditable; popups / new windows / context menus blocked in code.
- Memory footprint comparable to cog (~80-150 MB resident with one
  WebProcess), acceptable on 1 GB Pi class hardware.

## File tree

    layer/kiosk-wpe/
      README.md                         - this file
      install.sh                        - idempotent installer, distro+arch aware
      systemd/
        volumio-evo-kiosk.service       - kiosk session (Type=simple, User=<evo>)
        volumio-evo-kiosk-autorotate.service
                                        - accelerometer watcher, disabled by default
      bin/
        volumio-evo-kiosk-preflight     - DRM probe only (ExecStartPre)
        volumio-evo-kiosk-launch        - overlays/TOML; exec labwc --session with session helper
        volumio-evo-kiosk-session       - labwc session: OSK, wlr-randr scale, gsettings OSK layout, exec browser
        volumio-evo-kiosk-browser       - **not** shipped here as source; produced by **`crates/kiosk-browser`** and installed to **`/usr/local/bin/`** by **`install.sh`** (prebuilt **`layer/binaries/<triple>/`** preferred)
        volumio-evo-kiosk-autorotate    - iio-sensor-proxy DBus client
      etc/
        kiosk.toml.example              - seeded once to /etc/volumio-evo/kiosk.toml
        labwc/rc.xml                    - copied to /etc/volumio-evo/labwc/rc.xml

## Install

Invoked by the main bootstrap with an additive flag (equivalent forms):

    sudo scripts/bootstrap-volumio-evo-player.sh --full --with-kiosk=wpe
    sudo scripts/bootstrap-volumio-evo-player.sh --full --kiosk-wpe

Environment equivalent:

    sudo KIOSK=wpe ./scripts/bootstrap-volumio-evo-player.sh

**Upgrade path:** after `git pull`, refresh backend **and** kiosk layer without a full reinstall:

    sudo EVO_WITH_KIOSK=wpe ./scripts/bootstrap-volumio-evo-player.sh --upgrade-evo

The kiosk flags are off by default. When set on **full**/**reset**, `layer/kiosk-wpe/install.sh` runs after the Evo stack validates: installs packages (**labwc**, **`libgtk-4-1`**, **`libwebkitgtk-6.0-1`**, dev headers only if **`install_kiosk_browser_binary`** must compile — prefer checked-in **`layer/binaries/<triple>/volumio-evo-kiosk-browser`**), squeekboard, wvkbd, wtype, **`bubblewrap`**, **`xdg-dbus-proxy`**, copies units and shell helpers, installs **`volumio-evo-kiosk-browser`** to **`/usr/local/bin/`**, seeds **`/etc/volumio-evo/kiosk.toml`** and **`/etc/volumio-evo/labwc/rc.xml`**, **`systemctl daemon-reload`**. It does **not** enable **`volumio-evo-kiosk.service`** — **Settings → System → Kiosk** (`kiosk_enabled`) is the runtime switch. When the operator enables the kiosk from the UI and the layer has never been installed (or **`installKioskLayer`** is invoked), the backend runs **`sudo -n …/run-kiosk-wpe-install.sh`** first — see **`docs/KIOSK.md`** and **`docs/OS_PRIVILEGE_MODEL.md`**.

## Hardware matrix

Installer detects arch via `dpkg --print-architecture` and pulls the
matching Mesa / VA-API userspace. Verified target is Pi 5 arm64;
`install.sh` is best-effort universal across arm64, armhf, and amd64.
armv6 (Pi 0/1 original) is out of scope (see docs/KIOSK.md).

`install.sh` only requests packages that appear in the current apt
index (`apt-cache show`). Names that are missing on your mirror or
suite (for example optional fonts, GStreamer plugins, or VA drivers)
are skipped with a warning instead of failing the whole run. The
following packages are hard-required and the installer stops if any
are unavailable (runtime): `labwc`, `libgtk-4-1`, `libwebkitgtk-6.0-1`;
(build headers + **`pkg-config`** are also required when **`install.sh`** must invoke **`cargo build`** because no prebuilt **`volumio-evo-kiosk-browser`** exists for this arch).

Conditional packages:

  - `iio-sensor-proxy` - only installed when
    `/sys/bus/iio/devices/iio:device*` reports an accelerometer at
    install time. Otherwise skipped.
  - VA-API driver on amd64 - selected by `lspci` vendor ID:

        Intel 0x8086           -> intel-media-va-driver (Gen8+) or
                                  i965-va-driver-shaders (Gen5..Gen7)
        AMD   0x1002 / 0x1022  -> mesa-va-drivers
        other                  -> mesa-va-drivers fallback

## Runtime state (overlays)

Persistent state lives under `/var/lib/volumio-evo/settings/kiosk/`.
Each file holds exactly one value. The launcher reads these at start;
the backend writes them on save. Overlay values take precedence over
`kiosk.toml`.

    settings/kiosk/
      primary_display   - "auto" | "hdmi" | "dsi" | "wayland-default"
      rotation          - "0" | "90" | "180" | "270"
      osk               - "squeekboard" | "wvkbd" | "none"
      cursor            - "auto" | "hide" | "show"
      zoom              - WebKit zoom ladder string (e.g. `1.2`) → **`KIOSK_ZOOM`** for the browser
      scale             - `auto` or fractional scale → **`wlr-randr`** in session script
      osk_layout        - resolved XKB code (`us`, `gb`, …) for squeekboard via **`gsettings`**
      auto_rotate       - "true" | "false"
      osk_force_show    - "true" | "false"  (debug only)

When any of these overlays changes and the kiosk unit is active, the
backend restarts the unit so the new value takes effect.

## kiosk.toml key reference

First-install `/etc/volumio-evo/kiosk.toml` is seeded from
`etc/kiosk.toml.example` and is **never** overwritten after that.
Runtime-editable keys also have overlay files above; the overlay always
wins.

    url            = "http://127.0.0.1/"        - fixed in P0
    output         = ""                         - wlroots output name, "" = first
    rotation       = "normal"                   - normal | 90 | 180 | 270
    cursor         = "auto"                     - auto | hide | show
    osk            = "squeekboard"              - squeekboard | wvkbd | none
    osk_force_show = false                      - debug: keep OSK visible on launch
    auto_rotate    = false                      - requires accelerometer
    color_depth    = "auto"                     - auto | 16 | 24 | 32
    xkb_layout     = "us"                       - passed to wlroots via WLR_XKB_LAYOUT
    [env]                                       - extra env for labwc + browser

## Backend toggle, layer install, and sudoers

Settings → System → Kiosk exposes (persisted in **`settings/system/state.toml`**):

  - Enable kiosk       (switch, `SystemSettings.kiosk_enabled`)
  - Primary display    (select, `SystemSettings.primary_display`)
  - Rotation           (select 0 / 90 / 180 / 270, `SystemSettings.kiosk_rotation`)
  - Auto rotate        (switch, `SystemSettings.kiosk_auto_rotate`)
  - On-screen keyboard (select, `SystemSettings.kiosk_osk`)
  - OSK layout         (select, `SystemSettings.kiosk_osk_layout` — `auto` follows UI language)
  - Cursor             (select, `SystemSettings.kiosk_cursor`)
  - Zoom               (select, `SystemSettings.kiosk_zoom` — WebKit/CSS viewport; Node parity ladder)
  - Scale              (select, `SystemSettings.kiosk_scale` — Wayland output scale via **`wlr-randr`**)

Save goes through Socket.IO **`callMethod system_controller/system saveKioskSettings`**. **`crates/core/src/kiosk.rs`** writes overlay files under **`settings/kiosk/`**, then starts / stops / restarts **`volumio-evo-kiosk.service`** (and autorotate) via **`sudo -n`** and:

    /etc/sudoers.d/volumio-evo-kiosk-control

When the operator enables the kiosk **and** the layer is not yet on disk (**`/usr/local/bin/volumio-evo-kiosk-browser`** missing or first enable from off), **`crates/core/src/api/kiosk_install.rs`** runs **`sudo -n /usr/share/volumio-evo/repo/layer/install/run-kiosk-wpe-install.sh`** (same script as bootstrap) under **`/etc/sudoers.d/volumio-evo-kiosk-layer-install`**, then applies settings. **`installKioskLayer`** **`callMethod`** re-runs that installer explicitly (parity with **`installBootBranding`**).

Example **kiosk-control** lines (paths managed by bootstrap; **`systemctl`** must match **`VOLUMIO_EVO_KIOSK_SYSTEMCTL`**):

    <user> ALL=(root) NOPASSWD: /usr/bin/systemctl start volumio-evo-kiosk.service
    <user> ALL=(root) NOPASSWD: /usr/bin/systemctl stop volumio-evo-kiosk.service
    …

Resolved **`systemctl`** binary is published in **`10-runtime-user.conf`** as **`Environment=VOLUMIO_EVO_KIOSK_SYSTEMCTL=<path>`**.

## VT selection

The unit sets `TTYPath=/dev/tty1` (same slot a display manager uses on
Lite installs). `Conflicts=getty@tty1.service` mirrors that behaviour
so the kiosk owns the VT while running. systemd reads `EnvironmentFile`
before `ExecStartPre`, so discovering TTY dynamically in preflight
cannot work - an older revision tried that and caused pam_systemd "VT
number out of range". Preflight now only probes `/dev/dri/card*`;
overlays and `kiosk.toml` are read by
`/usr/local/bin/volumio-evo-kiosk-launch`.

## systemd target

`WantedBy=multi-user.target`. No `systemctl set-default`.
`graphical.target` is intentionally avoided to keep the boot graph
minimal on Pi 2 class hardware. labwc talks directly to DRM/KMS via
logind; `display-manager.service` is not required.

## Maximize, not fullscreen

The browser calls `xdg_toplevel.set_maximized`, not
`xdg_toplevel.set_fullscreen`. Background:

  - Squeekboard's layer-shell surface is hardcoded on
    `ZWLR_LAYER_SHELL_V1_LAYER_TOP`.
  - wlroots layer ordering places "fullscreen windows" **above**
    `LAYER_TOP`. A fullscreen kiosk client therefore covers the OSK.
  - Maximized xdg_toplevels live on the regular window layer, below
    `LAYER_TOP`. The OSK renders above them.
  - Combined with `<decoration>client</decoration>` in `rc.xml` and no
    panel, maximized is visually identical to fullscreen.
  - Bonus: when the OSK claims an exclusive zone, labwc resizes the
    maximized window to avoid it, so the focused text field stays
    visible (same behaviour as mobile browsers).

See upstream references in `docs/KIOSK.md`.

## OSK behaviour

Default OSK is squeekboard. Fallback is wvkbd. Selection lives in
`settings/kiosk/osk` or `kiosk.toml` `osk` key. Both packages install
by default so switching is a runtime change with no apt churn.

Squeekboard's auto-show is driven by `text-input-v3`: when the user
focuses an editable DOM element, WebKit's `GtkIMContext` on Wayland
sends `text_input_v3.enable` to the compositor, labwc forwards to
squeekboard via `input_method_v2.activate`, squeekboard shows. On blur,
the reverse: `disable` -> `deactivate` -> hide. For this chain to work
the WebView widget must hold GTK focus, which the kiosk browser ensures
with `grab_focus()` after the window is realized.

If your hardware needs manual toggling rather than auto-show, pick
wvkbd in the UI.

**cursor=hide:** the session script sends a synthetic **F24** via
**wtype** (matched to **HideCursor** in
`/etc/volumio-evo/labwc/rc.xml`).

## Headless / VM guard

`install.sh` copies every file regardless of `/dev/dri/card*` presence
(the enable/disable toggle lives in the backend and may later flip
when hardware is added). The Rust `apply_kiosk_settings()` helper
refuses to start the unit when no DRM device is visible and emits a
toast.

## Root-user policy

The kiosk runs as the same user that runs `volumio-evo.service`
(resolved by bootstrap via `EVO_SERVICE_USER`). If that resolution is
empty (service runs as root), `install.sh` refuses to enable kiosk by
default. Override with `KIOSK_ALLOW_ROOT=1` to proceed.

## Observability

All kiosk log lines from the Rust side carry the tag `"EVO KIOSK -->"`
for `journalctl` grep. Service logs live under the usual systemd
journals:

    journalctl -u volumio-evo-kiosk -n 100 --no-pager
    journalctl -u volumio-evo-kiosk-autorotate -n 100 --no-pager
    journalctl -u volumio-evo           | grep -F 'EVO KIOSK'

The kiosk browser itself logs with the prefix `[kiosk-browser]`; the
session script with `[kiosk-session]`; the launcher with
`[kiosk-launch]`; the preflight probe with `[kiosk-preflight]`.

## Uninstall

This component uninstalls by:

    sudo systemctl disable --now volumio-evo-kiosk.service
    sudo systemctl disable --now volumio-evo-kiosk-autorotate.service
    sudo rm -f /etc/systemd/system/volumio-evo-kiosk*.service
    sudo rm -rf /etc/systemd/system/volumio-evo-kiosk.service.d
    sudo rm -rf /etc/systemd/system/volumio-evo-kiosk-autorotate.service.d
    sudo rm -f /usr/local/bin/volumio-evo-kiosk-*
    sudo rm -f /etc/sudoers.d/volumio-evo-kiosk-control
    sudo systemctl daemon-reload

The apt packages (labwc, libgtk-4-1, libwebkitgtk-6.0-1, GTK/WebKit **-dev** if installed,
squeekboard, wvkbd, …) remain installed; remove them with apt if
desired. Persisted settings under `/var/lib/volumio-evo/settings/kiosk/`
and `/etc/volumio-evo/kiosk.toml` are left in place.

## Related docs

- [docs/KIOSK.md](../../docs/KIOSK.md) — architecture, bootstrap/UI install, binaries
- [docs/OS_PRIVILEGE_MODEL.md](../../docs/OS_PRIVILEGE_MODEL.md) — **`volumio-evo-kiosk-control`** + **`volumio-evo-kiosk-layer-install`**
- [docs/SETTINGS_LAYOUT.md](../../docs/SETTINGS_LAYOUT.md) — **`settings/kiosk/`** overlays vs **`state.toml`**
- [docs/PORTING.md](../../docs/PORTING.md) — **`saveKioskSettings`**, **`installKioskLayer`**
- [docs/OBSERVABILITY.md](../../docs/OBSERVABILITY.md) — journal grep patterns
