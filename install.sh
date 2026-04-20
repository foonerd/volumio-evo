#!/usr/bin/env bash
# Remote entrypoint: clone (if needed) and run the canonical bootstrap.
# Usage on a fresh Debian / Raspberry Pi OS host:
#   curl -fsSL https://raw.githubusercontent.com/foonerd/volumio-evo/main/install.sh | sudo bash
# Optional:
#   sudo EVO_GIT_REF=main EVO_REPO_URL=https://github.com/foonerd/volumio-evo.git bash -s -- --build
# Shallow clone by default (matches bootstrap): EVO_REPO_DEPTH=1. Full history: EVO_REPO_DEPTH=0 before curl.
set -euo pipefail

if [[ "${EUID:-}" -ne 0 ]]; then
  echo "This installer must run as root. Example:"
  echo "  curl -fsSL https://raw.githubusercontent.com/foonerd/volumio-evo/main/install.sh | sudo bash"
  exit 1
fi

BASE_DIR="${BASE_DIR:-/opt/volumio}"
EVO_REPO_URL="${EVO_REPO_URL:-https://github.com/foonerd/volumio-evo.git}"
EVO_GIT_REF="${EVO_GIT_REF:-main}"
# Passed through to bootstrap so docs/env stay aligned (bootstrap uses EVO_REPO_BRANCH when set).
EVO_REPO_BRANCH="${EVO_REPO_BRANCH:-${EVO_GIT_REF}}"
EVO_REPO_DEPTH="${EVO_REPO_DEPTH:-1}"
# Piped from curl: BASH_SOURCE is "-"; default clone dir. Local: ./install.sh next to Cargo.toml uses this tree.
DEFAULT_EVO_REPO_DIR="${BASE_DIR}/volumio-evo"
SCRIPT_SRC="${BASH_SOURCE[0]:-}"
if [[ "${SCRIPT_SRC}" != "-" && -n "${SCRIPT_SRC}" && "${SCRIPT_SRC}" != "bash" ]]; then
  _HERE="$(cd "$(dirname "${SCRIPT_SRC}")" && pwd)"
  if [[ -f "${_HERE}/Cargo.toml" && -d "${_HERE}/layer" ]]; then
    DEFAULT_EVO_REPO_DIR="${_HERE}"
  fi
fi
EVO_REPO_DIR="${EVO_REPO_DIR:-${DEFAULT_EVO_REPO_DIR}}"

BOOTSTRAP="${EVO_REPO_DIR}/scripts/bootstrap-volumio-evo-player.sh"

_ensure_git() {
  if command -v git >/dev/null 2>&1; then
    return 0
  fi
  echo "git is required to clone the repository but was not found. Installing..."
  if command -v apt-get >/dev/null 2>&1; then
    DEBIAN_FRONTEND=noninteractive apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq git ca-certificates
  elif command -v dnf >/dev/null 2>&1; then
    dnf install -y git ca-certificates
  elif command -v yum >/dev/null 2>&1; then
    yum install -y git ca-certificates
  elif command -v apk >/dev/null 2>&1; then
    apk add --no-cache git ca-certificates
  else
    echo "ERROR: Install git (and ca-certificates) manually, then re-run this installer."
    exit 1
  fi
}

if [[ ! -f "${EVO_REPO_DIR}/Cargo.toml" || ! -d "${EVO_REPO_DIR}/layer" ]]; then
  _ensure_git
  if [[ -e "${EVO_REPO_DIR}" ]]; then
    echo "ERROR: ${EVO_REPO_DIR} exists but is not a complete volumio-evo checkout."
    echo "Remove it, or set EVO_REPO_DIR to an empty path, then re-run."
    exit 1
  fi
  echo "Cloning ${EVO_REPO_URL} (branch ${EVO_REPO_BRANCH}, depth=${EVO_REPO_DEPTH:-1}) -> ${EVO_REPO_DIR}"
  mkdir -p "$(dirname "${EVO_REPO_DIR}")"
  if [[ "${EVO_REPO_DEPTH:-1}" != "0" && -n "${EVO_REPO_DEPTH:-1}" ]]; then
    GIT_TERMINAL_PROMPT=0 git clone --depth "${EVO_REPO_DEPTH:-1}" --single-branch --branch "${EVO_REPO_BRANCH}" "${EVO_REPO_URL}" "${EVO_REPO_DIR}"
  else
    GIT_TERMINAL_PROMPT=0 git clone --branch "${EVO_REPO_BRANCH}" "${EVO_REPO_URL}" "${EVO_REPO_DIR}"
  fi
fi

if [[ ! -x "${BOOTSTRAP}" ]]; then
  echo "ERROR: bootstrap script missing or not executable: ${BOOTSTRAP}"
  exit 1
fi

export BASE_DIR
export EVO_REPO_DIR
export EVO_REPO_URL
export EVO_REPO_BRANCH
export EVO_REPO_DEPTH
exec bash "${BOOTSTRAP}" "$@"
