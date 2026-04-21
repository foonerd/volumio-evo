#!/bin/bash
# Thin wrapper so sudoers can allow: sudo -n /path/run-kiosk-wpe-install.sh
# without shell globbing or env-injected command lines. Idempotent: re-runs
# layer/kiosk-wpe/install.sh (apt, units, prebuilt or built kiosk-browser).
set -euo pipefail
export EVO_REPO_DIR="${EVO_REPO_DIR:-/usr/share/volumio-evo/repo}"
exec /bin/bash "${EVO_REPO_DIR}/layer/kiosk-wpe/install.sh"
