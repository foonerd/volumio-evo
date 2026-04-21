#!/usr/bin/env bash
# Rebuild checked-in layer binaries for commit: volumio-evo + volumio-evo-kiosk-browser
# per Rust triple, then refresh layer/binaries/SHA256SUMS.
#
# Requires Docker + `cross` on PATH (repo Cross.toml supplies kiosk GTK images).
# Usage (from repo root):
#   ./scripts/build-layer-binaries.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DEST="${ROOT}/layer/binaries"

TRIPLES=(
  aarch64-unknown-linux-gnu
  armv7-unknown-linux-gnueabihf
  x86_64-unknown-linux-gnu
)

if ! command -v cross >/dev/null 2>&1; then
  echo "ERROR: install cross (\`cargo install cross\`) and Docker; see docs/BUILD_GUIDE.md." >&2
  exit 1
fi

cd "${ROOT}"

echo "==> cargo clean"
cargo clean

for triple in "${TRIPLES[@]}"; do
  echo ""
  echo "========== ${triple} =========="

  core_extra=()
  if [[ "${triple}" == armv7-unknown-linux-gnueabihf ]]; then
    core_extra+=( --no-default-features )
  fi

  echo "--> volumio-evo-core -> volumio-evo"
  cross build --release -p volumio-evo-core "${core_extra[@]}" --target "${triple}"

  echo "--> volumio-evo-kiosk-browser"
  cross build --release -p volumio-evo-kiosk-browser --target "${triple}"

  mkdir -p "${DEST}/${triple}"
  cp -f "${ROOT}/target/${triple}/release/volumio-evo" "${DEST}/${triple}/volumio-evo"
  cp -f "${ROOT}/target/${triple}/release/volumio-evo-kiosk-browser" "${DEST}/${triple}/volumio-evo-kiosk-browser"
  chmod +x "${DEST}/${triple}/volumio-evo" "${DEST}/${triple}/volumio-evo-kiosk-browser"
done

echo ""
echo "==> SHA256SUMS"
"${SCRIPT_DIR}/refresh-layer-binaries-sha256sums.sh"

echo ""
echo "Done. Verify: ( cd layer/binaries && sha256sum -c SHA256SUMS )"
