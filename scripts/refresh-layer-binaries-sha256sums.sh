#!/usr/bin/env sh
# Regenerate layer/binaries/SHA256SUMS for every checked-in release binary present.
# Matches triple layout from docs/BUILD_GUIDE.md — add lines when volumio-evo-kiosk-browser
# (or other artefacts) appear under layer/binaries/<triple>/.
set -eu
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BIN="${REPO_ROOT}/layer/binaries"
cd "${BIN}"

{
  for triple in aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf x86_64-unknown-linux-gnu; do
    for name in volumio-evo volumio-evo-kiosk-browser; do
      p="${triple}/${name}"
      if [ -f "${p}" ]; then
        sha256sum "${p}"
      fi
    done
  done
} > SHA256SUMS.tmp
mv SHA256SUMS.tmp SHA256SUMS
echo "Updated ${BIN}/SHA256SUMS ($(wc -l < SHA256SUMS | tr -d ' ') lines). Verify: ( cd ${BIN} && sha256sum -c SHA256SUMS )"
