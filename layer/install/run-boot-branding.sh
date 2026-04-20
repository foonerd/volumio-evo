#!/bin/bash
# Thin wrapper so sudoers can allow: sudo /path/run-boot-branding.sh <0|90|180|270>
# without env-based command lines. Delegates to volumio-boot-branding.sh.
set -euo pipefail
export EVO_REPO_DIR="${EVO_REPO_DIR:-/usr/share/volumio-evo/repo}"
ROT="${1:-0}"
case "${ROT}" in 0|90|180|270) ;; *) ROT=0 ;; esac
export PLYMOUTH_ROTATION="${ROT}"
exec /bin/bash "${EVO_REPO_DIR}/layer/install/volumio-boot-branding.sh"
