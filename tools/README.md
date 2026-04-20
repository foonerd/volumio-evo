# Developer tools (not installed on devices)

Maintenance scripts for translations, kiosk strings, and ad-hoc API checks. **`scripts/bootstrap-volumio-evo-player.sh`** does **not** copy these.

- **data/network-smb-i18n.json** — locale table for **`NETWORK.SMB_*`** strings; consumed by **`i18n-apply-network-smb-translations.py`**.
- Run Python helpers from the repo root (`python3 tools/<script>.py`).

Runtime hooks that ship with the player live under **`layer/install/`**.
