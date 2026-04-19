# Branded boot: Plymouth, `VOL` branding stages, and release process

This document is the **single place** for Volumio Evo’s **Plymouth** story: token contract, shipped systemd units, development workflow (Pi 5 prototype vs RC on other hardware), packaging/theme work still to do, and testing expectations.

Related: **`layer/systemd/`** (`volumio-evo.service`, **`vol-branding-v1-*.service`**), **`layer/plymouth/`** (theme + **`generate-overlays.sh`** — dev-only, see **`layer/plymouth/README.md`**), **`layer/README.md`**.

---

## Goals

- **Informative boot** on slow boards (primary); fast boards may only show some frames or none.
- **Stable contract** between boot logic and theme: **`VOL:v1:…`** strings, not user-facing prose.
- **Rotation**: theme assets remain **pre-baked per orientation** (see volumio-os **`volumio-adaptive`** pattern); Plymouth script maps tokens to overlay PNGs per `sequence{N}/`.

### Integration (Evo UI + scripts)

**Settings → System → Boot branding** runs the installer with a progress modal (**`openModal` / `modalProgress` / `modalDone`**, same pattern as install-to-disk). It executes:

1. **`scripts/run-boot-branding.sh`** with a rotation argument (**0**, **90**, **180**, or **270**) via **`sudo -n`** — thin wrapper around **`scripts/volumio-boot-branding.sh`** so **`/etc/sudoers.d/`** can list one stable command (see below).
2. **`volumio-boot-branding.sh`** installs Debian **`plymouth`** packages, copies **`layer/plymouth/volumio-adaptive`** into **`/usr/share/plymouth/themes/`**, enables **`vol-branding-v1.target`**, patches **Pi** **`/boot/firmware/cmdline.txt`** or drops **`/etc/default/grub.d/50-volumio-evo-plymouth.cfg`** on **GRUB** systems, runs **`update-initramfs -u`**, and prints progress lines **`::BRANDING % message`** for the UI.

**Paths:** Evo resolves the repo root with **`VOLUMIO_EVO_REPO_DIR`** (systemd sets this when bootstrap runs); default **`/usr/share/volumio-evo/repo`**. **`scripts/bootstrap-volumio-evo-player.sh`** creates **`/usr/share/volumio-evo/repo`** as a **symlink** to the real checkout (`EVO_REPO_DIR`, e.g. **`/opt/volumio/volumio-evo`**), sets **`Environment=VOLUMIO_EVO_REPO_DIR=…`** on **`volumio-evo.service`**, and installs **`/etc/sudoers.d/volumio-evo-boot-branding`**. That matches the wrapper default inside **`run-boot-branding.sh`** after **`sudo`** clears the environment. Override only if you insist on a custom layout: **`VOLUMIO_EVO_BOOT_BRANDING_SCRIPT`**.

**Sudo (non‑negotiable for the UI path):** bootstrap installs (for the runtime service user):

```text
youruser ALL=(root) NOPASSWD: /usr/share/volumio-evo/repo/scripts/run-boot-branding.sh
```

Disable with **`EVO_INSTALL_BOOT_BRANDING_SUDOERS=0`** only if you replace this manually.

Manual installs (no bootstrap): symlink **`/usr/share/volumio-evo/repo`** → your **`volumio-evo`** checkout **or** set **`VOLUMIO_EVO_REPO_DIR`** / **`VOLUMIO_EVO_BOOT_BRANDING_SCRIPT`** and matching sudoers.

**Rotation:** the UI sends **`plymouth=0|90|180|270`** via the wrapper argument; the script adds **`plymouth=N`** to the kernel command line (together with **`splash`** and **`plymouth.ignore-serial-consoles`**). Reboot to apply.

Vanilla OS **`.deb`** set for Plymouth is whatever **`scripts/volumio-boot-branding.sh`** installs (**`apt-get install -y plymouth plymouth-themes`** today — single source of truth).

---

## `VOL` token format (v1)

```
VOL:<spec_version>:<domain>:<code>
```

| Part | Rule |
|------|------|
| **`VOL`** | Literal prefix. |
| **`spec_version`** | **`v1`** today; bump only if semantics of the scheme change. |
| **`domain`** | Short category: `fs`, `sys`, `app`, `initrd`, `maint`, `net`, `diag`, … |
| **`code`** | `a-z`, `0-9`, `-`; no spaces; keep codes short (e.g. `root-ready`, `listening`). |

Emit with: `plymouth message --text="VOL:v1:fs:root-ready"` (same string in journal for correlation).

---

## Implemented userspace branding stages (this repo)

Systemd units live under **`layer/systemd/`**. Copy to **`/etc/systemd/system/`**, **`systemctl daemon-reload`**, **`systemctl enable vol-branding-v1.target`**.

| Token | Unit | Notes |
|--------|------|--------|
| `VOL:v1:fs:root-ready` | `vol-branding-v1-fs-root-ready.service` | After **`local-fs.target`** + **`plymouth-start`**. |
| `VOL:v1:sys:base` | `vol-branding-v1-sys-base.service` | After **`basic.target`**. |
| `VOL:v1:fs:data` | `vol-branding-v1-fs-data.service` | Only if **`/data`** exists (Volumio-style layout). |
| `VOL:v1:app:starting` | `vol-branding-v1-app-starting.service` | Before **`volumio-evo.service`**. |
| `VOL:v1:app:listening` | `vol-branding-v1-app-listening.service` | After **`volumio-evo.service`**; polls **`http://127.0.0.1:3000/api/host`** by default (override with **`VOLUMIO_EVO_BRANDING_READY_URL`** in a drop-in; legacy **`VOLUMIO_EVO_MILESTONE_URL`** is still read by the probe). |

**Rename from older trees:** if you previously enabled **`vol-milestones-v1.target`**, disable it and switch to **`vol-branding-v1.target`** (old unit names were **`vol-milestone-v1-*`**):

```bash
sudo systemctl disable --now vol-milestones-v1.target 2>/dev/null || true
sudo rm -f /etc/systemd/system/vol-milestone-v1-*.service /etc/systemd/system/vol-milestones-v1.target
sudo systemctl daemon-reload
# then reinstall from Evo layer or re-run volumio-boot-branding.sh
```

**`ExecStart=-/usr/bin/plymouth`** (leading **`-`**): if **`plymouthd`** has already exited (common on a **fast** boot), `plymouth message` fails non-zero; systemd must **not** mark the oneshot as failed. Same idea for **`app-listening`**’s shell wrapper.

**Plymouth on the OS image** requires the **`plymouth`** package and kernel cmdline **`splash`** (and typically **`plymouth.ignore-serial-consoles`** when serial console is enabled). Theme choice is **`/etc/plymouth/plymouthd.conf`** and/or **`plymouth-set-default-theme`**; stock Debian may not register **`update-alternatives`** for **`default.plymouth`** — **`Theme=…`** in **`plymouthd.conf`** is still authoritative.

---

## Not implemented here (OS / theme packages)

| Item | Owner |
|------|--------|
| **`VOL:v1:initrd:*`** | Initramfs scripts (e.g. volumio-os **`initv3`** / **`volumio-functions`**) — `plymouth message` from early boot. |
| **`VOL:v1:maint:*`** | USB update / factory reset / resize paths. |
| **Theme** (e.g. **`volumio-adaptive`** v2) | Overlay PNGs per token **×** rotation; **`GetOverlayFilename`** should match **`VOL:v1:`** strings. |
| **`.deb` / image recipe** | Install theme under **`/usr/share/plymouth/themes/`**, initramfs hook so the theme is in initrd, **`update-initramfs`** in postinst. |

---

## Development process: prototype vs RC

| Stage | Hardware | Purpose |
|-------|----------|---------|
| **Prototype** | **Raspberry Pi 5** (or equivalent fast lab device) | Install Plymouth, branding units, theme package; verify journal (`grep 'VOL:v1'`), **`systemctl --failed`**, theme loads. Visibility of every frame is **not** guaranteed. |
| **RC** | Slower **arm/arm64** and **amd64** devices as they become available | Readable overlays, slow initramfs, DRM/fb quirks, **`plymouth=90|180|270`** + correct asset trees. |
| **Gaps** | After each RC | Fix ordering, timeouts, assets, cmdline (**`quiet`** / loglevel), getty gap, packaging — only what RC proves. |

Do **not** block prototype on RC hardware; do **not** call the theme release-complete without at least **one slow** device and **one non-Pi** architecture in RC if the product ships on those.

---

## Release phases (theme + OS + QA)

Work in roughly this order; details can track a project board.

1. **Contract freeze** — Table: **`VOL:v1:…` → overlay basename** (and which tokens are v1-ship).  
2. **Theme + assets** — Script + PNGs per rotation (and compact tier if needed); map **exact** tokens.  
3. **Package** — `.deb` or rootfs recipe; initramfs hook; dependencies on **`plymouth`**.  
4. **Wiring** — Cmdline **`splash`**, default theme, initramfs + maint emitters where applicable.  
5. **Testing** — See “Testing” below.

---

## Testing checklist

- **Journal:** `journalctl -b | grep 'VOL:v1'` — tokens appear in plausible order.  
- **Failed units:** `systemctl --failed` — branding units should not stay failed (rely on **`ExecStart=-`**).  
- **Plymouth:** `journalctl -b -u plymouth-start -u plymouth-quit-wait -u plymouth-quit`.  
- **Rotation:** Kernel cmdline **`plymouth=0|90|180|270`** + theme **`sequence{N}/`** (volumio-adaptive patching pattern on some images).  
- **RC devices:** Repeat on slow / diverse hardware.

---

## Known UX limits

- **Fast boot:** Splash may end before later **`VOL`** messages; messages can still appear in **journal**.  
- **After Plymouth quits:** Brief **console / text gap** before login is common (Plymouth vs getty); **`quiet`** reduces kernel noise but does not remove all gaps.  
- **`app:listening`** polls HTTP until Evo supports **`Type=notify`** (`sd_notify`) on the **`volumio-evo`** unit — **not implemented**; listed in **[DOCUMENTATION_MAP.md](DOCUMENTATION_MAP.md)** *Deferred / reference*.

---

## References

- **`layer/plymouth/generate-overlays.sh`** — regenerates **`overlay-vol-*.png`** from the VOL registry (dev tool; see **`layer/plymouth/README.md`**).
- **Evo working copy (preferred for theme changes):** **`layer/plymouth/volumio-adaptive/`** — **`volumio-adaptive.script`** maps **only** **`VOL:v1:`** → **`overlay-vol-*`** (table in **`layer/plymouth/README.md`**). Ship PNGs per **`sequenceR/`** for each token you use.
- volumio-os initramfs still emits **English** strings today — **those overlays will not match** until that stack is switched to **`VOL:v1:`** or uses this Evo theme branch only after porting **`plymouth_msg`** calls.
- Evo layer: **`layer/systemd/vol-branding-v1-*.service`**, **`vol-branding-v1.target`**.
