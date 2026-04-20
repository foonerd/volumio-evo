# SMB server

Volumio Evo exposes an SMB file server comparable to classic Node Volumio (guest-friendly shares, optional named users), with moderation of export paths and privilege rules in **[`OS_PRIVILEGE_MODEL.md`](OS_PRIVILEGE_MODEL.md)**.

The Rust backend writes **`settings/samba/smb.conf.generated`**, installs it to **`/etc/samba/smb.conf`** via **`sudo -n install`** (narrow sudoers **`volumio-evo-samba`**), then **`restart smbd`**/**`nmbd`** (or **`stop`** when SMB is disabled). **`VOLUMIO_EVO_SKIP_STARTUP_SAMBA_APPLY=1`** skips the boot-time apply hook.

**Bootstrap / OS:** Install **`smbd`** + **`nmbd`** with **`--no-install-recommends`** so Debian does not pull **`samba-ad-dc`** (Active Directory domain controller) and enable **`samba-ad-dc.service`** — Evo is a standalone file server only. Bootstrap runs **`systemctl disable --now samba-ad-dc.service`** if that unit exists (e.g. after an older install used the full **`samba`** metapackage).

This document covers **UX placement**, **i18n**, persisted settings, and **allowed paths** for user-defined shares.

## UX (Settings → Network only)

SMB stays entirely under **Settings → Network** (no separate top-level menu).

| Layer | Responsibility |
|-------|----------------|
| **`network_ui_config.json`** | Scalar settings only: e.g. enable SMB, minimum SMB protocol, stock-style music shares behaviour — sections with **`onSave`** / **`saveButton`** and **`callMethod`** (`system_controller/network`), matching other Network rows. |
| **`coreSection`: `smb`** | Dynamic UI: **SMB users** and **extra shares** (paths moderated by **`ALLOWED_ROOTS`**). Implemented as **`app/plugin/core-plugin/smb-plugin.html`** + **`smb-plugin.controller.js`**. The controller is **not** in the webpack bundle: **`index.html`** loads **`app/plugin/core-plugin/smb-plugin.controller.js`** after the main **`scripts/app-*.js`** so **`SmbPluginController`** registers on the **`volumio`** module (same pattern could be replicated in **Volumio2-UI** builds). **`{"coreSection":"smb"}`** is enabled in Evo’s **`network_ui_config.json`** (after **`section_smb_server`**). Assets are duplicated under **`layer/web/{classic,contemporary,manifest}/`**. |

**Rationale:** The stock plugin renderer has no repeatable “list” element in JSON. Lists belong in a **core plugin** template (same pattern as Sources → network drives). A standalone **alarms-style** modal opened only from the shell is a poor fit for SMB entry from Network; use the core section instead (optional small modals **inside** that template for confirm / password remain fine).

### i18n (themes and languages)

- **Themes:** All three layouts use the same **`core-plugin/smb-plugin.*`** include — no duplicate SMB UI per theme unless you intentionally fork styling.
- **Strings in the core plugin:** Prefer Angular **`$translate('NETWORK.SMB_*')`** (or a dedicated **`SMB.*`** namespace) so non‑English locales use the same **`strings_*.json`** pipeline as elsewhere (e.g. network-drives).
- **Strings in `network_ui_config.json`:** Use **`TRANSLATE.NETWORK.*`** tokens; Evo resolves them with **`resolve_translate_tokens`** (embedded **`strings_en.json`** per theme). **`NETWORK.SMB_*`** keys exist in **`layer/web/<theme>/app/i18n/strings_*.json`** for every locale. Non‑English strings are maintained in **`scripts/data/network-smb-i18n.json`** and applied per locale by **`scripts/i18n-apply-network-smb-translations.py`** (run after editing translations). The three **`strings_en.json`** files are the English source of truth.
- **Adding or changing SMB keys:** update all three **`strings_en.json`**, add the same keys (per locale) to **`network-smb-i18n.json`**, run **`i18n-merge-network-smb-keys.py`** if you need to copy new keys from English into any file that is still missing them, then run **`i18n-apply-network-smb-translations.py`**. For **server-side** `getUiConfig` in the user’s live language, Evo would need the same per-request i18n path as the rest of the app; today the embedded dictionary is English-only, while the full locale files back Angular **`$translate`** in the browser.

## Persisted state

See **[`SETTINGS_LAYOUT.md`](SETTINGS_LAYOUT.md)** — **`settings/samba/`** (e.g. **`state.toml`**) for SMB enable, protocol floor, share list, user list metadata (passwords live in Samba’s **passdb**, not plain text in TOML).

## Testing (without relying on the full Angular UI)

1. **OS / files (always useful):** After enabling SMB and saving, on the device check **`/var/lib/volumio-evo/settings/samba/smb.conf.generated`**, **`/etc/samba/smb.conf`**, and **`systemctl status smbd nmbd`**. Expect **`install`** + **`restart`** only when **`sudo -n`** rules from bootstrap are installed (**[`OS_PRIVILEGE_MODEL.md`](OS_PRIVILEGE_MODEL.md)**).

2. **Browser + Socket.IO (quick API check):** Open the web UI so the socket connects, then in the devtools console (if Angular is available): get **`socketService`** from the injector and emit the same events the plugin uses:
   - `emit('getSmbServerLists')` → expect **`pushSmbServerLists`** with **`extra_shares`** and **`smb_users`**.
   - **`saveSmbExtraShares`** / **`saveSmbUsers`** payloads match the Rust handlers in **`socketio.rs`** (objects with **`extra_shares`** / **`smb_users`** arrays).

3. **Standalone Socket.IO client:** Repo script **`scripts/dev-smb-socket-smoke.js`** (**`npm install socket.io-client`**) connects to Evo, emits **`getSmbServerLists`**, prints **`pushSmbServerLists`**. Or use any client matching the server’s **`socket.io`** protocol to **`emit('getSmbServerLists')`** / listen for **`pushSmbServerLists`** on **`http://<device>:3000`** — no browser UI required.

4. **Rust:** **`cargo check`** / targeted tests for validation helpers; full SMB apply still needs root/sudo on a real image.

New **`NETWORK.SMB_*`** UI strings: refresh non‑English locale files via **`scripts/i18n-merge-network-smb-keys.py`** and **`scripts/i18n-apply-network-smb-translations.py`** after updating **`scripts/data/network-smb-i18n.json`**.

---

## Allowed paths for user-defined shares

Custom SMB shares (extra `[share]` sections beyond stock music exports) must **not** accept arbitrary filesystem paths. Evo moderates exports with a fixed **allowlist** of path prefixes plus a smaller **denylist** for sensitive subtrees even when nested under an allowed root.

This section pins **`ALLOWED_ROOTS`** and **`DENIED_PREFIXES`** so bootstrap, backend validation, and reviews stay aligned.

## Validation rule

1. Resolve the configured path with **`canonicalize`** (or equivalent): reject if missing or if resolution fails.
2. Reject if the canonical path **starts with** any entry in **`DENIED_PREFIXES`** (longest-prefix wins when documenting edge cases).
3. Accept only if the canonical path **starts with** any entry in **`ALLOWED_ROOTS`** (after denial check).

Symlinks are safe only **after** canonicalization: a path under `/var/lib/volumio-evo/...` that symlinks outside those roots must be **rejected** if the canonical path leaves the allowlist.

## `ALLOWED_ROOTS` (absolute path prefixes)

| Prefix | Purpose |
|--------|---------|
| **`/var/lib/volumio-evo`** | Evo data root: `music/`, `albumart/`, future `staging/` (e.g. plugin uploads), and other controlled subtrees ([`SETTINGS_LAYOUT.md`](SETTINGS_LAYOUT.md)). |
| **`/mnt/NAS`** | Network-drive mount points (`/mnt/NAS/<alias>`); created by bootstrap for the Sources/NAS UI. |
| **`/mnt/USB`** | Classic-style USB mount parent (parity with stock `smb.conf` USB share); image/bootstrap should ensure it exists when USB exports are supported. |

## `DENIED_PREFIXES` (always reject)

These paths are **never** valid SMB export targets, even when they sit under an allowed root:

| Prefix | Reason |
|--------|--------|
| **`/var/lib/volumio-evo/settings`** | Persisted secrets and state: NAS credentials, Wi‑Fi PSK sidecars, alarm/network staging, etc. ([`SETTINGS_LAYOUT.md`](SETTINGS_LAYOUT.md)). |

Optional future denials (enable when the threat model requires):

- **`/var/lib/volumio-evo/settings/mounts`** — explicit creds (`shares.toml`, `cifs-*.cred`).

## Product defaults (recommended)

- **Plugin / bootstrap staging:** create **`/var/lib/volumio-evo/staging/plugins`** (mode and owner per [`OS_PRIVILEGE_MODEL.md`](OS_PRIVILEGE_MODEL.md) service user) and offer it as a **preset** share path in the UI — users still cannot type paths outside **`ALLOWED_ROOTS`**.

## Source of truth in code

The canonical lists live as **`SMB_SHARE_ALLOWED_ROOTS`** and **`SMB_SHARE_DENIED_PREFIXES`** in **`crates/core/src/paths.rs`**. Update this markdown and those constants together.
