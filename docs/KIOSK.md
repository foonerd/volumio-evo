# Volumio Evo - Kiosk concept and implementation record

**Status:** Implemented (April 2026). `layer/kiosk-wpe/` is live on the `kiosk-wpe` branch and has been validated on Pi 5 arm64 with Trixie Lite. The authoritative reference for what actually installs is [`layer/kiosk-wpe/README.md`](../layer/kiosk-wpe/README.md). The sections below describe the original design concept and the deviations the implementation took from it.

The directory and the bootstrap flag (`--with-kiosk=wpe`) keep the historical name for continuity; the implementation is no longer WPE-based.

## 0. Implementation update (read first)

The original concept in this document chose **cog + WPE WebKit + cage** as the reference stack. During bring-up on Pi 5 / Trixie three upstream issues forced deviations from that plan. The implementation that ships on `kiosk-wpe` now is:

| Role | Concept said | Implementation uses | Why |
|------|--------------|---------------------|-----|
| Compositor | cage | **labwc** | cage lacks `wlr_layer_shell_v1` so squeekboard cannot render. labwc is a wlroots stacking compositor with layer-shell; it is the current default on Raspberry Pi OS. |
| Browser | cog (WPE) | **purpose-built GTK 4 + webkit2gtk 6.0 shell** (`/usr/local/bin/volumio-evo-kiosk-browser`, ~100 lines of Python) | WPE WebKit 2.48.3 as packaged in Trixie does not dispatch pointer button or keyboard events to the DOM on Pi 5 class hardware. libinput delivers events at the kernel layer; WPE's `wl` and `fdo` platform plugins drop them before reaching WebCore. webkit2gtk (same engine family, GTK platform path) delivers every event correctly. Epiphany in `--application-mode` was also tried; it requires `xdg-desktop-portal` access to `/proc/<pid>/root` which is denied in a PAM-login systemd session. |
| Fullscreen mode | xdg `set_fullscreen` | **xdg `set_maximized`** | squeekboard is hardcoded to `ZWLR_LAYER_SHELL_V1_LAYER_TOP`. wlroots' layer ordering places "fullscreen windows" above `LAYER_TOP`, so a true-fullscreen kiosk client covers the OSK. Maximized xdg_toplevels live on the regular window layer, below `LAYER_TOP`; the OSK renders above them. Combined with `<decoration>client</decoration>` and no panel, maximized is visually identical to fullscreen. References: [labwc/labwc#2926](https://github.com/labwc/labwc/issues/2926), [raspberrypi-ui/squeekboard#13](https://github.com/raspberrypi-ui/squeekboard/issues/13). |

Other concept-to-implementation mappings:

- **Chromium** was rejected on memory grounds (~250-400 MB RSS is too much for Pi 1 / 2 / Zero class targets).
- **Midori** is no longer in Debian main.
- **surf** (suckless webkit2gtk) ships in Trixie but its `-K` kiosk flag blocks only keystrokes and right-click, not `window.open` / `target=_blank`.
- **cog-settings.ini** example file removed; cog is not installed.
- `COG_PLATFORM_FDO_VIEW_FULLSCREEN` and `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS` removed from the unit; neither applies to the webkit2gtk shell.

The sections that follow (original design from late 2025 / early 2026) are retained as historical context. Where they say "cog" / "cage" / "WPE" / "fullscreen", refer to the table above for the actual shipped choice.

---

# Volumio Evo - Kiosk concept (original WPE reference path, historical)

**Roadmap:** **Deferred.** This document is a **design reference for a future layer component**, not something you need for day-to-day Evo development today. Treat kiosk work as **after the Node backend is fully replaced** by the Rust core (playback, browse, REST/Socket.IO parity, installer story) so the on-device browser shell targets a **stable API and UI**. Do not block backend porting milestones on kiosk.

**Status:** concept draft. Nothing implemented in-tree (`layer/kiosk-wpe/` may appear only when this phase starts). Targets the greenfield Evo stack (stock minimal Debian Lite + Evo layer). No inheritance from the legacy Volumio OS kiosk (X11 + openbox + Chromium/Vivaldi + Chrome extension keyboard). **No WASM plugin is involved in this concept**; see [Section 3](#3-terminology) for why.

Related docs:

| Document | Topic |
|----------|--------|
| [docs/CONCEPT.md](CONCEPT.md) | overall Evo architecture |
| [docs/PORTING.md](PORTING.md) | backend port status |
| [docs/PLUGIN_ABI.md](PLUGIN_ABI.md) | WASM plugin ABI (distinct from the "kiosk" here) |
| [layer/README.md](../layer/README.md) | layer-component conventions |

## 1. Purpose

Turn a connected display into a functional, touch-capable Volumio Evo player front-end on a stock minimal Debian install, with hardware acceleration used wherever the OS provides it, and with a system-level on-screen keyboard so touch-only devices can type.

Primary user flow: attach display and touchscreen, boot, see the Evo web UI full screen, tap a text field, system OSK appears, dismiss by tapping outside or via a toggle, continue.

**Out of scope** for this concept:

- Ports of legacy Volumio kiosk scripts or Chrome extensions.
- Graphical desktop sessions (GNOME, KDE, Pixel desktop).
- Driving remote or secondary displays, mirroring, HDMI-CEC.
- Network provisioning, Bluetooth pairing UIs (Evo core concern).
- Sleep/standby policy, power management beyond DPMS default.

## 2. Non-goals

- Do not reimplement compositor, seat, input, output, power, or locale management. Delegate to existing packages ([Section 13](#13-desktop-component-delegation)).
- Do not inject JavaScript, CSS, or Chrome-style extensions into the Evo web UI for the keyboard. The OSK is a system package.
- Do not ship a second browser engine. **Cog + WPE WebKit** is the choice for this concept.
- Do not gate the concept on fixing deep upstream issues (cursor suppression on pure-touch wlroots, GStreamer H.265 regressions on Trixie). Observe first; only fix what blocks the happy path.

## 3. Terminology

**"Kiosk"** in this document is a Volumio Evo layer component: a set of OS-level assets (packages to install, systemd units, config files, launch wrapper) that sit in `layer/kiosk-wpe/` and are applied on top of a minimal Debian install by the bootstrap script.

This is **not** a "WASM plugin" in the Evo plugin ABI sense. A sandboxed WASM guest has a single host import today (`env.log`) and no access to filesystem, systemd, apt, DRM, or process spawn. A kiosk needs all of those. The kiosk therefore lives in the layer, not in the plugin runtime. See [docs/PLUGIN_ABI.md](PLUGIN_ABI.md) for the WASM plugin model.

If at some point the Evo plugin ABI grows a declarative "contribution" export (analogous to `AlsaContribution`) for kiosk metadata, a thin WASM shim can register its presence and defaults. That is a later phase; the concept does not depend on it.

## 4. Reference stack

All components are OS packages in Debian Trixie (13) and forward to Forky (14). No custom compiles in the default path.

| Role | Package | Notes |
|------|---------|-------|
| Browser | **cog** | WPE WebKit launcher |
| WebKit | **libwpewebkit-2.0-1** | WebKit/WPE 2.48.x on Trixie, 2.50.x on Forky |
| FDO backend | **libwpebackend-fdo-1.0-1** | required by cog's `fdo` platform |
| Compositor | **cage** | Wayland kiosk compositor on wlroots 0.18.x (Trixie) / 0.19.x (Forky) |
| OSK (default) | **squeekboard** | input-method-v2 + text-input + virtual-keyboard + wlr-layer-shell |
| OSK (fallback) | `wvkbd` or `maliit-keyboard` | not enabled by default; see [Section 11](#11-osk-wiring) |
| Session supervisor | **systemd** | |
| Seat | **systemd-logind** | no seatd |
| Multimedia pipeline | `gstreamer1.0-plugins-{base,good,bad,libav,gl}` | for WebKit media; HW decode is arch-dependent |

**Why cog + cage:**

- Cog draws the WebKit viewport straight into an EGL surface on a Wayland surface. No X11, no per-frame copy.
- Cage is a minimal wlroots compositor designed for the exact shape of this problem: one fullscreen client, hardware-accelerated, DRM/KMS + GBM + DMA-BUF, seat + libinput via wlroots, outputs via wlroots.
- Both are maintained, packaged, and present on every arch we care about.

**Why squeekboard (and not JS injection):**

- It is a system package, versioned with the distro, zero maintenance in the Evo tree.
- It reacts to standard Wayland text-input and input-method protocols, so any compositor that forwards them gets auto-show behaviour.
- Works across browsers if we ever swap the engine. No coupling to the Evo web UI source.

## 5. Architecture and distro matrix

### In scope

| Arch | Representative devices |
|------|------------------------|
| **arm64** | Pi 3 64-bit, Pi 4, Pi 5, CM4, CM5, generic aarch64 SBCs |
| **armhf** (armv7) | Pi 2, Pi 3 32-bit, other armv7 SBCs |
| **amd64** | generic x86 PCs, thin clients |

### Out of scope

| Arch | Reason |
|------|--------|
| armv6 (Pi 0/0W/1) | Debian armhf requires armv7 + VFPv3; only Raspberry Pi OS Lite supports armv6, and 512 MB / 1 core is insufficient for cog + WebKit + cage. Revisit on request. |
| armel, riscv64, ppc64el, s390x, i386 | Out of product scope. |

### Distro base

- **Debian 13 Trixie** (current stable) - primary test target.
- **Debian 14 Forky** - best-effort. The `libwpewebkit-1.1-0` -> `libwpewebkit-2.0-1` transition is already in Trixie, so Forky is an incremental bump.
- **Raspberry Pi OS Lite** (Bookworm/Trixie) - accepted alternative base. Package names match Debian except where Pi firmware/kernel packages diverge.

## 6. Package set per arch

### Common to all arches (Debian Trixie)

```
cog
libwpewebkit-2.0-1
libwpebackend-fdo-1.0-1
cage
squeekboard
fonts-dejavu-core
fonts-noto-core
libinput-bin
gstreamer1.0-plugins-base
gstreamer1.0-plugins-good
gstreamer1.0-plugins-bad
gstreamer1.0-libav
gstreamer1.0-gl
xdg-user-dirs
```

Note: **seatd is not used**. `systemd-logind` provides the seat.

### Arch-specific additions

| Arch | Packages | Purpose |
|------|----------|---------|
| arm64 / armhf (Pi) | `libgl1-mesa-dri`, `libgles2-mesa`, `libegl1`, `libdrm2` | v3d userspace, EGL/GLES |
| arm64 / armhf (generic SBC) | same as Pi | panfrost / lima are in `libgl1-mesa-dri` automatically |
| amd64 (Intel Gen8+) | `libgl1-mesa-dri`, `libegl1`, `intel-media-va-driver` | VA-API for modern Intel |
| amd64 (Intel Gen5-Gen7) | `libgl1-mesa-dri`, `libegl1`, `i965-va-driver-shaders` | VA-API for older Intel |
| amd64 (AMD / fallback) | `libgl1-mesa-dri`, `libegl1`, `mesa-va-drivers` | VA-API via Mesa |

The installer picks the VA-API driver by PCI probe; if none of the Intel/AMD VA drivers match, it skips them and falls back to `mesa-va-drivers`. No failure.

Diagnostic-only packages (not installed by default): `mesa-utils` (ARM GL probe), `vainfo` (amd64 VA probe).

## 7. Hardware acceleration

### Compositing (always GPU-accelerated with this stack)

- Cage draws via wlroots' GL renderer using EGL on the KMS backend.
- Output buffers are **DMA-BUF** where the driver supports it, otherwise shared-memory fallback.
- Cog presents the page via wpebackend-fdo, which exports a DMA-BUF to the compositor; WebKit paints into a GL surface.

### Page rendering

- WebKit/WPE uses OpenGL ES for layer compositing when compositing mode is on. Pi vc4/v3d, Mali panfrost, Intel/AMD Mesa all expose GLES via EGL on Trixie.
- Environment variables to **verify on target** (do not set blindly):

| Variable | Default | Override only if |
|----------|---------|------------------|
| `WEBKIT_DISABLE_COMPOSITING_MODE` | unset | never, in our path |
| `WEBKIT_FORCE_COMPOSITING_MODE` | unset | diagnostics show compositing is off |
| `COG_PLATFORM_FDO_VIEW_FULLSCREEN` | `1` | we want fullscreen |

### Video decode (nice to have, not required for the music-player UI)

| Arch | Path | Status |
|------|------|--------|
| Pi 4 / Pi 5 | GStreamer v4l2 via bcm2835-codec and rpivid | H.264 works; H.265 has a known Trixie regression (raspberrypi/linux issue 7137); do **not** block the concept on H.265. |
| amd64 | VA-API via `intel-media`, `i965`, or `mesa-va` | normal Mesa path |
| Generic aarch64 (Rockchip, Amlogic, NXP) | Mesa + v4l2 codec per SoC | best-effort |

Cursor and touch input hardware paths are the compositor's job. The kiosk does not configure `/dev/input` directly.

## 8. Repository placement

All new assets land under `layer/kiosk-wpe/`:

```
layer/
  kiosk-wpe/
    README.md                         component purpose, how bootstrap applies it
    install.sh                        idempotent installer, distro + arch aware
    systemd/
      volumio-evo-kiosk.service       kiosk session, type=simple, User=kiosk
    bin/
      volumio-evo-kiosk-launch        exec cage + cog with flags from kiosk.toml
    etc/
      kiosk.toml.example              url, rotation, cursor, osk, env
      cog-settings.ini.example        cog-specific tweaks if any
```

**What is intentionally not here:**

- `polkit/` - not needed. The kiosk user does not perform privileged operations; systemd (running as root) manages the unit.
- `udev/` - not needed. Debian's default udev rules already label `/dev/dri/card*`, `/dev/dri/renderD*`, and `/dev/input/event*` with the correct groups. The installer adds the kiosk user to `video`, `input`, `render`.

These subtrees may be added later if P3 introduces pointer-hotplug cursor toggling (udev) or P4 exposes privileged runtime operations to the kiosk user (polkit). Do not add them speculatively.

**No Rust code changes in this phase.** No edits to `crates/core`. The core already serves the UI on port 3000 and nginx (from the bootstrap) serves the static tree on port 80; the kiosk just points cog at `http://127.0.0.1/` and lets nginx proxy `/api/host` to Evo.

Optional future additions:

- `crates/core/src/kiosk.rs` for runtime toggles (OSK on/off, rotation, brightness, restart kiosk). Exposed via REST or Socket.IO. **Deferred** until the layer component is proven.

## 9. Installer design

`layer/kiosk-wpe/install.sh` responsibilities:

1. Root check. Bail if not root.
2. Detect distro: `/etc/os-release` `ID` + `VERSION_ID`. Accept `debian 13`, `debian 14`, `raspbian 13`, `raspbian 12` (deprecate warning).
3. Detect arch via `dpkg --print-architecture`: `amd64`, `arm64`, `armhf`.
4. Detect GPU vendor on amd64 via `lspci -n` vendor IDs for VA-API driver selection: Intel `0x8086`, AMD `0x1002` / `0x1022`.
5. Compute package set ([Section 6](#6-package-set-per-arch)) and run `apt-get install -y`. Idempotent via apt.
6. Create system user `kiosk` if not present, add to `video`, `input`, `render`, `audio` groups. Home at `/var/lib/volumio-evo-kiosk`.
7. Install systemd unit (`systemd/volumio-evo-kiosk.service`) and launch wrapper (`bin/volumio-evo-kiosk-launch`) with `0755` perms.
8. Install `/etc/volumio-evo/kiosk.toml` from the example if not present. **Never overwrite.**
9. Enable and start the unit unless `--no-start` is passed.
10. Print a clear status line (`active`, `inactive`, `failed`) and log locations.

### Bootstrap hook

`scripts/bootstrap-volumio-evo-player.sh` grows a single hook:

```
--with-kiosk=wpe
```

When set, after the main install completes the bootstrap invokes `layer/kiosk-wpe/install.sh`. The flag and its env equivalent (`KIOSK=wpe`) are documented in `--help`.

## 10. Systemd and launch

### Service layout

```ini
[Unit]
Description=Volumio Evo Kiosk (WPE)
After=network-online.target systemd-user-sessions.service volumio-evo.service
Wants=network-online.target volumio-evo.service

[Service]
Type=simple
User=kiosk
PAMName=login
TTYPath=/dev/tty7
StandardInput=tty
StandardOutput=journal
StandardError=journal
Environment=XDG_SESSION_TYPE=wayland
Environment=WLR_LIBINPUT_NO_DEVICES=0
Environment=COG_PLATFORM_FDO_VIEW_FULLSCREEN=1
Environment=GDK_BACKEND=wayland
ExecStart=/usr/local/bin/volumio-evo-kiosk-launch
Restart=always
RestartSec=2

[Install]
WantedBy=graphical.target
```

`PAMName=login` is required so logind creates a seat and `XDG_RUNTIME_DIR`. The `TTYPath` may be adjusted to `tty1` if Trixie Lite leaves that VT free; decide when writing the unit.

### Launch wrapper

`volumio-evo-kiosk-launch` responsibilities:

1. Source `/etc/volumio-evo/kiosk.toml` via a small `toml-parse` shell helper, or parse with `python3 -c "import tomllib..."` if available. Decide at install time which is present.
2. Start squeekboard in the background (if enabled in `kiosk.toml`).
3. Exec cage with the cog command line built from config:

   ```sh
   exec cage -s -- cog \
     --platform=fdo \
     --fullscreen \
     "${URL}"
   ```

   where `URL` defaults to `http://127.0.0.1/`.
4. cage exits when cog exits; systemd restarts the unit.

### Config keys in `/etc/volumio-evo/kiosk.toml`

```toml
url            = "http://127.0.0.1/"
rotation       = "normal"              # normal | 90 | 180 | 270
cursor         = "auto"                # auto | hide | show
osk            = "squeekboard"         # squeekboard | none | wvkbd | maliit
osk_force_show = false                 # debug helper
output         = ""                    # "" = first output, else wlr name
color_depth    = "auto"                # auto | 16 | 24 | 32

[env]
# extra env vars passed to cog/cage
```

Rotation and touch calibration delegate to libinput and wlroots via the compositor, not by the launcher. See [Section 13](#13-desktop-component-delegation).

## 11. OSK wiring

**Default:** squeekboard.

### Protocols used

| Protocol | Purpose |
|----------|---------|
| `wlr-layer-shell` | squeekboard panel positioning |
| `input-method-unstable-v2` | squeekboard receives focus events |
| `virtual-keyboard-unstable-v1` | squeekboard sends key events |
| `text-input-v3` | cog -> compositor -> squeekboard relay |

### Launch flow

1. systemd unit starts the launch wrapper.
2. Wrapper starts cage; cage spawns cog as its fullscreen client. Cage passes text-input events from cog to registered input methods.
3. Squeekboard is started as a child of the wrapper (not cage), so it connects to the same `WAYLAND_DISPLAY`. On input focus in the web UI, it shows automatically.

### Verify on target

- **Cage 0.2.0 (Trixie)** is claimed to implement input-method-v2 and text-input-v3 far enough for squeekboard to auto-show without phoc. **Must be confirmed on device.** If it does not, fall back to `phosh-osk-stub` or `wvkbd` (manual DBus toggle).
- Squeekboard's gsettings key `org.gnome.desktop.a11y.applications screen-keyboard-enabled` may need to be set `true` in the kiosk user session. The installer sets it via `gsettings set` in the `kiosk` user's session on first run.
- Manual override for debugging:

  ```sh
  busctl call --user sm.puri.OSK0 /sm/puri/OSK0 \
    sm.puri.OSK0 SetVisible b true
  ```

### Fallback if squeekboard does not auto-show

| OSK | Pros | Cons |
|-----|------|------|
| `wvkbd` | tiny (~5 MB RSS), no GTK | no auto-focus; manual show/hide via `SIGUSR1` / `SIGUSR2` |
| `maliit-keyboard` | DBus API, rich | Qt-based, heavier |

## 12. Cursor policy

**Default:** let the compositor show a cursor. Do not patch, do not hide.

Rationale: on pure-touch wlroots had a long-standing issue where the pointer device list was non-empty because the touch controller reports as both touch and pointer via evdev. Newer cage / wlroots releases may or may not have addressed this. The concept does not block on the fix.

### Concept behaviour

1. Boot with default cursor visible.
2. If cursor is visibly disruptive on target hardware, set `cursor = "hide"` in `kiosk.toml`.
3. Implementation of `"hide"` in the launcher tries, in order, stopping at first that works:

   | Step | Method | Notes |
   |------|--------|-------|
   | a | cage supports `-d` or equivalent "hide cursor" flag in the installed version | use it |
   | b | transparent `cursor-theme` with 1x1 PNG cursors via `XCURSOR_THEME` and `XCURSOR_SIZE=1` | known to work on wlroots 0.17+ |
   | c | accept cursor visible | document the limitation; do **not** patch cage from source |

4. Toggle (`cursor = "auto"`) means "show cursor only when a pointer device (mouse, trackpad) is actually connected, hide when only touch is present". Best-effort; implement via udev hotplug if (a) or (b) above can be flipped at runtime; otherwise document as "requires restart of kiosk service on hotplug".

**No custom cage/wlroots build in the concept.** That path remains open for a later phase if (a), (b), and (c) all fail on a target we care about.

## 13. Desktop-component delegation

The kiosk is deliberately thin. Where an existing package does the job, use it and write nothing.

| Concern | Delegated to | Notes |
|---------|--------------|-------|
| Seat management | **systemd-logind** | |
| Input (touch, mouse) | **libinput via wlroots** | compositor handles it |
| Touch calibration | **libinput calibration matrix** | `kiosk.toml.matrix = "a b c d e f"`, applied via a `libinput-quirks` drop-in |
| Output configuration | **wlroots output protocol** + `kanshi` (optional) | kiosk.toml `output` + `rotation` as cage env vars |
| Auto-rotate | `iio-sensor-proxy` + small agent | **not in scope** for concept; add on devices with accelerometers |
| DPMS / screen blanking | **systemd logind IdleAction** + wlroots idle-inhibit | defaults only |
| Audio | Evo + MPD | **out of scope for this component** |
| Network | Evo or NetworkManager | **out of scope** |
| Locale / keymap | XKB from `kiosk.toml.layout` | passed to cage via `WLR_XKB_LAYOUT` |

**Evaluate on first contact, commit later:**

- `kanshi` for per-output profiles when more than one display is connected (Pi 4/5 HDMI0 + HDMI1).
- `iio-sensor-proxy` for tablet-style auto-rotate.

**Do not introduce a full desktop** (LXDE, XFCE, GNOME) to gain any of these. Each delegation is a single package or a small config drop-in.

## 14. Verify-on-target checklist

Each item here is a claim that must be confirmed on a real device before it is baked into defaults. **Do not assume.**

- [ ] cog 0.18.4 runs fullscreen inside cage 0.2.0 on Trixie arm64 with Pi 5. Time to first paint under 3 seconds on warm boot.
- [ ] cog 0.18.4 runs inside cage on Trixie arm64 on Pi 4 (2 GB and 4 GB variants).
- [ ] cog 0.18.4 runs inside cage on Trixie armhf on Pi 2 / Pi 3 32-bit. (Expected: slow but functional.)
- [ ] cog 0.18.4 runs inside cage on Trixie amd64 on a low-end Celeron N4020 or similar thin-client class machine.
- [ ] squeekboard auto-shows on input focus inside the Evo web UI in cog without any JS changes. If not, document exact failing protocol exchange and move to `wvkbd`.
- [ ] GL compositing is on by default (check with `WEBKIT_DEBUG=LayerCompositing` or GPU usage indicator). If off, identify why before setting `WEBKIT_FORCE_COMPOSITING_MODE`.
- [ ] libinput calibration matrix survives resume from blank.
- [ ] Touch tap-through outside text fields dismisses the OSK.
- [ ] Output rotation applies cleanly (cage env, not a compositor restart).
- [ ] With `cursor = "auto"` and no mouse connected, cursor is not visibly persistent over the UI.
- [ ] kiosk service survives Evo core restart (systemd restart loop does not tight-loop).
- [ ] `kiosk.toml` changes are picked up on unit restart.
- [ ] Power cycle: boot to UI in under 25 seconds on Pi 5, under 45 seconds on Pi 4. Numbers are targets to measure, not SLOs.

## 15. Red-herring list

Do **not** spend time on these during the concept phase. Document, do not chase.

- **Chromium GPU flags on Pi.** Ineffective on vc4/v3d. We are not using Chromium anyway.
- **Transparent cursor themes** above [Section 12](#12-cursor-policy) point (b). Tried extensively in prior work; marginal benefit. If (a) is available, use it; if not, accept cursor.
- **Custom wlroots or cage patches** before (a), (b), (c) are tried in [Section 12](#12-cursor-policy).
- **WebKit backing store / pixmap flags.** Defaults on modern WPE are correct.
- **Retrofitting Xwayland or an X11 fallback.** The whole point is to drop X11. If Wayland does not work on a target, the target is out of scope for this concept.
- **seatd replacement of systemd-logind.** systemd-logind is already there; adding seatd introduces a second seat authority for no gain.
- **H.265 hardware decode on Trixie** (raspberrypi/linux issue 7137). Not on the critical path for a music player UI.
- **Font hinting, ClearType-style subpixel rendering tweaks.** Out of scope for this phase.

## 16. Phased task list (for Cursor)

### P0 - Bring-up on one reference target (Pi 5 arm64, Debian Trixie)

1. Create `layer/kiosk-wpe/` with the file tree in [Section 8](#8-repository-placement).
2. Write `install.sh` per [Section 9](#9-installer-design) points 1-10.
3. Write `volumio-evo-kiosk.service` per [Section 10](#10-systemd-and-launch).
4. Write `volumio-evo-kiosk-launch` per [Section 10](#10-systemd-and-launch).
5. Write `kiosk.toml.example` per [Section 10](#10-systemd-and-launch).
6. Wire `--with-kiosk=wpe` into `scripts/bootstrap-volumio-evo-player.sh` with no side effects when absent.
7. Manual run on Pi 5: bootstrap, reboot, observe UI fullscreen.

### P1 - OSK

1. Verify squeekboard auto-show inside cage + cog on the Evo UI.
2. If **pass**: pin squeekboard in the package set; set gsettings on first run. Done.
3. If **fail**: document failing case, add `wvkbd` to package set, wire a launcher helper that shows/hides wvkbd on a configurable hotkey or screen-edge hotspot. Mark squeekboard as optional.

### P2 - Multi-arch validation

1. Run the same install on Pi 4 arm64 Trixie. Note deltas.
2. Run on Pi 3 armhf (32-bit) Trixie. Note deltas.
3. Run on amd64 thin client (Intel or AMD). Pick the right VA driver in `install.sh`.
4. Update the package list in [Section 6](#6-package-set-per-arch) if any real differences emerge; do **not** speculate in advance.

### P3 - Cursor and input polish

1. Measure cursor behaviour on each target.
2. Apply [Section 12](#12-cursor-policy) ladder: (a) cage flag, (b) transparent theme, (c) accept.
3. Implement touch calibration matrix pipeline from `kiosk.toml`.
4. Test rotation 0/90/180/270 on each target.

### P4 (optional) - Runtime control from Evo core

1. Add `crates/core/src/kiosk.rs` with REST/Socket.IO endpoints:

   | Endpoint | Method | Payload |
   |----------|--------|---------|
   | `/api/v1/kiosk/status` | GET | - |
   | `/api/v1/kiosk/osk` | POST | `{ visible: bool }` |
   | `/api/v1/kiosk/rotation` | POST | `{ value: 0\|90\|180\|270 }` |
   | `/api/v1/kiosk/restart` | POST | - |

2. Implementation talks to the kiosk service over a small Unix socket exposed by the launch wrapper. **Do not bake `systemctl` calls into the core.**
3. Out of scope for P0-P3.

## 17. Acceptance criteria for the concept phase

- **P0 complete:** on Pi 5 with Debian 13 Trixie arm64, running `sudo scripts/bootstrap-volumio-evo-player.sh --with-kiosk=wpe` after an Evo install yields a boot-to-UI kiosk within 30 seconds of power-on. Touch moves work. Audio from an MPD-played file is audible.
- **P1 complete:** tapping a text input in the Evo web UI shows the system OSK; typing produces characters in the input; tapping elsewhere hides the OSK.
- **P2 complete:** P0 + P1 both demonstrated on at least one arm64, one armhf, and one amd64 target.
- **Footprint:** kiosk layer adds less than 250 MB resident memory on top of base Evo + MPD on a cold UI.

## 18. Open questions (intentionally)

Decisions left to the implementation, not the concept:

- Which VT does the kiosk occupy? `/dev/tty7` is conventional; `tty1` may be freer on minimal images. Decide when writing the unit.
- Is the kiosk user named `kiosk` or `volumio-kiosk`? Bikeshed; pick one in `install.sh` and keep it.
- Does the kiosk unit start on `graphical.target` or `multi-user.target`? Probably `graphical.target` via an alias; verify on a fresh Trixie Lite which target is reached by default.
- Squeekboard layout autodetect vs a fixed set bundled by Evo. Squeekboard reads layouts from XKB; default XKB layout is `en_US`. Defer localization to a later phase.
- If the bootstrap runs inside a VM or headless cloud image, the kiosk service must not start. Add an install-time guard that detects no `/dev/dri/card*` and skips enabling the unit, leaving the component installed but inactive.

---

Changelog:

- 2026-04-17: initial draft.
