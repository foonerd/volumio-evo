# Plymouth themes (vendored)

Boot splash assets are part of the **layer** ([CONCEPT.md](../../docs/CONCEPT.md) §7). Mentions of **Node** or **volumio-os** below refer to **other** release pipelines or sync points — not the Evo steward/plugin model ([CONCEPT.md](../../docs/CONCEPT.md)).

## `generate-overlays.sh` (dev / maintainers)

Lives in **`layer/plymouth/`** (sibling to **`volumio-adaptive/`**), so copying only the theme directory to a device does not ship the generator. **ImageMagick** (`convert`), **`bc`**, **bash 4+** required.

- **`./generate-overlays.sh`** — (re)builds all **`overlay-vol-*.png`** (19 tokens × 4 rotations × 2 sizes) from the embedded **VOL registry** (kept in sync with **`volumio-adaptive.script`** and the table below).
- **`./generate-overlays.sh --prune`** — remove any remaining **legacy** `overlay-*.png` in the four `sequenceR/` trees, then generate.
- **`./generate-overlays.sh --prune-only`** — delete legacy only.

Regenerate after changing **message text** or **colours** in the script, or when adding a new **VOL** key (update **three** places: `volumio-adaptive.script`, this README table, and **`generate-overlays.sh`** registries).

## `volumio-adaptive/`

**Complete copy** of the Volumio OS theme from **`volumio-os`**:

`volumio-os/volumio/plymouth/themes/volumio-adaptive/`

This repo’s copy is the **working tree for Evo**. Legacy English overlays were removed in favour of **`overlay-vol-*`** — regenerate with **`generate-overlays.sh`** above for placeholder **text-based** PNGs, or replace with final brand art (same basenames). **Do not** edit **`volumio-os`** copy unless syncing upstream intentionally.

### Install on a device (manual)

```bash
sudo cp -a volumio-adaptive /usr/share/plymouth/themes/
sudo plymouth-set-default-theme volumio-adaptive --rebuild-initrd
# or set Theme=volumio-adaptive in /etc/plymouth/plymouthd.conf then:
sudo update-initramfs -u
```

Ensure kernel cmdline includes **`splash`** (and **`plymouth.ignore-serial-consoles`** when using serial console).

### Relationship to VOL branding stages

Emitters send **`plymouth message --text="VOL:v1:…"`** (see **`docs/BRANDED_BOOT.md`** — **`vol-branding-v1-*.service`**). **`volumio-adaptive.script`** maps each token to an overlay **basename** (files: **`sequence{R}/<basename>{,-compact}.png`**).

### VOL:v1 → overlay basenames (greenfield)

**Rule:** each token maps to **`overlay-vol-<domain>-<code>.png`** (and **`-compact`**) under each **`sequenceR/`**. Add art for every row you ship; missing file → no overlay for that size/rotation (script hides sprite). Shipped **systemd** units today only emit the **fs / sys / app** rows; other rows are for initramfs, maint scripts, or future work.

| Token (exact `plymouth message` string) | Overlay basename |
|----------------------------------------|--------------------|
| `VOL:v1:initrd:early` | `overlay-vol-initrd-early` |
| `VOL:v1:fs:checking` | `overlay-vol-fs-checking` |
| `VOL:v1:fs:root-ready` | `overlay-vol-fs-root-ready` |
| `VOL:v1:fs:data` | `overlay-vol-fs-data` |
| `VOL:v1:sys:base` | `overlay-vol-sys-base` |
| `VOL:v1:app:starting` | `overlay-vol-app-starting` |
| `VOL:v1:app:listening` | `overlay-vol-app-listening` |
| `VOL:v1:ui:ready` | `overlay-vol-ui-ready` |
| `VOL:v1:maint:resize-start` | `overlay-vol-maint-resize-start` |
| `VOL:v1:maint:resize-done` | `overlay-vol-maint-resize-done` |
| `VOL:v1:maint:first-boot` | `overlay-vol-maint-first-boot` |
| `VOL:v1:maint:usb-update-start` | `overlay-vol-maint-usb-update-start` |
| `VOL:v1:maint:usb-update-done` | `overlay-vol-maint-usb-update-done` |
| `VOL:v1:maint:factory-reset` | `overlay-vol-maint-factory-reset` |
| `VOL:v1:maint:network-recovery` | `overlay-vol-maint-network-recovery` |
| `VOL:v1:net:link` | `overlay-vol-net-link` |
| `VOL:v1:net:configured` | `overlay-vol-net-configured` |
| `VOL:v1:diag:degraded` | `overlay-vol-diag-degraded` |
| `VOL:v1:diag:fail` | `overlay-vol-diag-fail` |

**Node/volumio-os** initramfs must be updated to emit **`VOL:v1:…`** (and optional `plymouth_msg` → same strings) if that image uses this Evo theme; English text is no longer matched in **`GetOverlayFilename()`**.

### Asset preparation (order of work)

1. **Script** — VOL registry in **`GetOverlayFilename()`** (greenfield **`overlay-vol-*`** names in this repo).  
2. **PNG exports** — For each shipped token, author **`overlay-vol-*.png`** (+ **`-compact`**) in **`sequence0/`**, **`sequence90/`**, **`sequence180/`**, **`sequence270/`**. Dimensions should match composition of existing legacy overlays until a new layout guide exists.  
3. **Deploy** — copy theme to **`/usr/share/plymouth/themes/`**, set default, **`update-initramfs -u`**.

### Source of truth for OS image builds

Production **Node-based** images may continue to ship the theme from **`volumio-os`** until Evo replaces that pipeline; Evo images should install **this** copy when the branded-boot package is defined.
