# Vendored Volumio2-UI static output

Prebuilt **web roots** for the three stock layouts (same roles as `/volumio/http/www`, `www3`, `www4` on full Volumio OS):

| Directory | Layout (`active_layout`) | Typical upstream source |
|-----------|-------------------------|-------------------------|
| **classic/** | `classic` | `dist` branch (Classic theme) |
| **contemporary/** | `contemporary` | `dist3` branch (volumio3 / Contemporary) |
| **manifest/** | `manifest` | Manifest / www4 build |

**Bootstrap** (`scripts/bootstrap-volumio-evo-player.sh`) installs these to **`UI_ROOT_*`** under `/srv/...` (defaults), writes **`app/local-config.json`** per tree, and points nginx at the layout from **`[ui] active_layout`** in `/etc/volumio-evo/config.toml`.

**Required** for install unless **`UI_DIST_SOURCE`** is set to a single **`dist/`** tree (that tree is copied to all three roots).

**Refreshing assets:** replace trees from upstream published branches or device copies; record commit or date in your release notes. Do not commit device-specific **`app/local-config.json`** here — bootstrap generates it at install time.

**Playback dial / codec badges:** the stock UI loads SVGs from **`/app/assets-common/format-icons/<codec>.svg`** (e.g. `mp3.svg`, `flac.svg`). Each layout tree includes **`app/assets-common/format-icons/`**; nginx `root` must be that layout’s folder so `/app/...` resolves. The backend sets **`trackType`** (lowercase extension) on `pushState` so `player.service.js` can pick the icon.
