# Volumio Evo - Kiosk layer component (WPE / Cog / Cage)

This directory is the runtime layer for the on-device WPE kiosk. The design
reference is docs/KIOSK.md; this README documents what actually installs and
how it maps to the backend.

The kiosk is a Wayland full-screen shell based on Cog + WPE WebKit running
inside the Cage compositor. It points at http://127.0.0.1/ (nginx root) and
renders the Evo UI on a connected display with touch, keyboard, and mouse
input. Base OS target is Debian 13 Trixie Lite with no graphical stack
pre-installed.

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
        volumio-evo-kiosk-launch        - cage + cog + OSK launcher
        volumio-evo-kiosk-autorotate    - iio-sensor-proxy DBus client
      etc/
        kiosk.toml.example              - seeded once to /etc/volumio-evo/kiosk.toml
        cog-settings.ini.example        - placeholder for forward-compat

## Install

Invoked by the main bootstrap with an additive flag:

    sudo scripts/bootstrap-volumio-evo-player.sh --with-kiosk=wpe

Environment equivalent (useful in piped one-liners):

    sudo KIOSK=wpe ./scripts/bootstrap-volumio-evo-player.sh

The flag is off by default. Running bootstrap without it does not install any
kiosk assets. When set, layer/kiosk-wpe/install.sh is invoked after the Evo
stack validates, adds packages, copies units and helper scripts, seeds
/etc/volumio-evo/kiosk.toml, and runs systemctl daemon-reload. It does NOT
enable the kiosk units - the backend toggle (Settings -> System -> WPE Kiosk)
is the single source of truth for runtime state.

## Hardware matrix

Installer detects arch via dpkg --print-architecture and pulls the matching
Mesa / VA-API userspace. Verified target is Pi 5 arm64; install.sh is
best-effort universal across arm64, armhf, and amd64. armv6 (Pi 0/1) is out
of scope (see docs/KIOSK.md Section 5).

Install (`install.sh`) only requests packages that appear in the current apt
index (`apt-cache show`). Names that are missing on your mirror or suite (for
example optional fonts, GStreamer plugins, or VA drivers) are skipped with a
warning instead of failing the whole run. **`cog` pulls in the correct
`libwpewebkit-*` / `libwpebackend-fdo-*` SONAME for your release** — those
libraries are no longer pinned by name in the installer. **`cog` and `cage`**
must both be available or the install stops with an error (on Ubuntu, enable
the **universe** repository).

Conditional packages:

  - iio-sensor-proxy - only installed when /sys/bus/iio/devices/iio:device*
    reports an accelerometer at install time. Otherwise skipped.
  - VA-API driver on amd64 - selected by lspci vendor ID:
      Intel 0x8086           -> intel-media-va-driver (Gen8+) or
                                i965-va-driver-shaders (Gen5..Gen7)
      AMD   0x1002 / 0x1022  -> mesa-va-drivers
      other                  -> mesa-va-drivers fallback

## Runtime state (overlays)

Persistent state lives under /var/lib/volumio-evo/settings/kiosk/. Each file
holds exactly one value. The launcher reads these at start; the backend
writes them on save. Overlay values take precedence over kiosk.toml.

    settings/kiosk/
      primary_display   - "auto" | "hdmi" | "dsi" | "wayland-default"
      rotation          - "0" | "90" | "180" | "270"
      osk               - "squeekboard" | "wvkbd" | "none"
      cursor            - "auto" | "hide" | "show"
      auto_rotate       - "true" | "false"

## kiosk.toml key reference

First-install /etc/volumio-evo/kiosk.toml is seeded from etc/kiosk.toml.example
and is NEVER overwritten after that. Runtime-editable keys also have overlay
files above; the overlay always wins.

    url            = "http://127.0.0.1/"        - fixed in P0
    output         = ""                          - wlroots output name, "" = first
    rotation       = "normal"                    - normal | 90 | 180 | 270
    cursor         = "auto"                      - auto | hide | show
    osk            = "squeekboard"               - squeekboard | wvkbd | none
    osk_force_show = false                       - debug: keep OSK visible
    auto_rotate    = false                       - requires accelerometer
    color_depth    = "auto"                      - auto | 16 | 24 | 32
    xkb_layout     = "us"                        - passed to cage via WLR_XKB_LAYOUT
    [env]                                        - extra env for cog/cage

## Backend toggle and sudoers

Settings -> System -> WPE Kiosk exposes:

  - Enable kiosk     (switch, SystemSettings.kiosk_enabled)
  - Primary display  (select, SystemSettings.primary_display)
  - Rotation         (select 0/90/180/270, SystemSettings.kiosk_rotation)
  - Auto rotate      (switch, SystemSettings.kiosk_auto_rotate)
  - On-screen keyb.  (select, SystemSettings.kiosk_osk)
  - Cursor           (select, SystemSettings.kiosk_cursor)

Save goes through Socket.IO callMethod system_controller/system
saveKioskSettings. The Rust side persists state.toml, writes the overlay
files, and starts/stops the kiosk unit via:

    /etc/sudoers.d/volumio-evo-kiosk-control

Contents of that drop-in (managed by bootstrap):

    <user> ALL=(root) NOPASSWD: /usr/bin/systemctl start volumio-evo-kiosk.service
    <user> ALL=(root) NOPASSWD: /usr/bin/systemctl stop volumio-evo-kiosk.service
    <user> ALL=(root) NOPASSWD: /usr/bin/systemctl restart volumio-evo-kiosk.service
    <user> ALL=(root) NOPASSWD: /usr/bin/systemctl enable volumio-evo-kiosk.service
    <user> ALL=(root) NOPASSWD: /usr/bin/systemctl disable volumio-evo-kiosk.service
    <user> ALL=(root) NOPASSWD: /usr/bin/systemctl start volumio-evo-kiosk-autorotate.service
    <user> ALL=(root) NOPASSWD: /usr/bin/systemctl stop volumio-evo-kiosk-autorotate.service
    <user> ALL=(root) NOPASSWD: /usr/bin/systemctl enable volumio-evo-kiosk-autorotate.service
    <user> ALL=(root) NOPASSWD: /usr/bin/systemctl disable volumio-evo-kiosk-autorotate.service

Resolved systemctl binary is published in 10-runtime-user.conf as
Environment=VOLUMIO_EVO_KIOSK_SYSTEMCTL=<path>.

## VT selection

The unit sets **TTYPath=/dev/tty1** (same slot a display manager uses on Lite
installs). **Conflicts=getty@tty1.service** mirrors that behaviour so the kiosk
owns the VT while running. systemd reads **EnvironmentFile** before
**ExecStartPre**, so discovering TTY dynamically in preflight cannot work — an
older revision tried that and caused pam_systemd “VT number out of range”.
Preflight now only probes **/dev/dri/card***; overlays and kiosk.toml are read by
`/usr/local/bin/volumio-evo-kiosk-launch`.

## systemd target

WantedBy=multi-user.target. No systemctl set-default. graphical.target is
intentionally avoided to keep the boot graph minimal on Pi 2 class hardware.
Cage talks directly to DRM/KMS via logind; display-manager.service is not
required.

## OSK behaviour

Default OSK is squeekboard. Fallback is wvkbd. Selection lives in
settings/kiosk/osk or kiosk.toml osk key. Both packages install by default
so switching is a runtime change with no apt churn.

Squeekboard auto-show requires the compositor to forward text-input-v3 and
input-method-v2. Cage 0.2.0 in Trixie is expected to handle this; the
launcher still sets the squeekboard gsettings key on first start. If your
hardware needs manual toggling, pick wvkbd in the UI.

## Headless / VM guard

install.sh copies every file regardless of /dev/dri/card* presence (the
enable/disable toggle lives in the backend and may later flip when hardware
is added). The Rust apply_kiosk_settings() helper refuses to start the unit
when no DRM device is visible and emits a toast.

## Root-user policy

The kiosk runs as the same user that runs volumio-evo.service (resolved by
bootstrap via EVO_SERVICE_USER). If that resolution is empty (service runs
as root), install.sh refuses to enable kiosk by default. Override with
KIOSK_ALLOW_ROOT=1 to proceed.

## Observability

All kiosk log lines from the Rust side carry the tag "EVO KIOSK -->" for
journalctl grep. Service logs live under the usual systemd journals:

    journalctl -u volumio-evo-kiosk -n 100 --no-pager
    journalctl -u volumio-evo-kiosk-autorotate -n 100 --no-pager
    journalctl -u volumio-evo           | grep -F 'EVO KIOSK'

## Uninstall

This component uninstalls by:

    sudo systemctl disable --now volumio-evo-kiosk.service
    sudo systemctl disable --now volumio-evo-kiosk-autorotate.service
    sudo rm -f /etc/systemd/system/volumio-evo-kiosk*.service
    sudo rm -f /usr/local/bin/volumio-evo-kiosk-*
    sudo rm -f /etc/sudoers.d/volumio-evo-kiosk-control
    sudo systemctl daemon-reload

The apt packages (cog, libwpewebkit-2.0-1, cage, squeekboard, wvkbd, ...)
remain installed; remove them with apt if desired. Persisted settings under
/var/lib/volumio-evo/settings/kiosk/ and /etc/volumio-evo/kiosk.toml are
left in place.

## Related docs

- docs/KIOSK.md         - design concept and rationale
- docs/OS_PRIVILEGE_MODEL.md - sudoers model (this drop-in fits the pattern)
- docs/SETTINGS_LAYOUT.md - /var/lib/volumio-evo/settings/ conventions
- docs/OBSERVABILITY.md - journalctl filtering
