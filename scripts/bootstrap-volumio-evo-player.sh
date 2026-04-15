#!/usr/bin/env bash
set -euo pipefail

# CANONICAL FULL-STACK INSTALL (on device): this script only.
# Re-run this script; by default it installs the prebuilt backend from layer/binaries/<triple>/
# (no rustup). Pass --build or EVO_BUILD_FROM_SOURCE=1 to compile on device (rustup + cargo).
# Copies static UI from layer/web/, configures MPD/systemd/nginx. Set EVO_REPO_UPDATE=0 only
# for offline or pinned checkouts.
#
# One-shot tester install for Debian / Raspberry Pi OS Lite. Run as root.
#
# UI: vendored trees under layer/web/{classic,contemporary,manifest} (stock-style static
# assets). Optional: UI_DIST_SOURCE=path to one dist/ with index.html (copied to all three
# layout roots for development). No git clone of Volumio2-UI and no npm/gulp on device.
#
# - apt: nginx, mpd, toolchain, python3, …
# - git: volumio-evo only (when not using a local checkout)
# - systemd + nginx on port 80; GET /api/host proxied to Evo for dynamic Socket.IO base URL

BASE_DIR="${BASE_DIR:-/opt/volumio}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT_REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
if [[ -f "${SCRIPT_REPO_DIR}/Cargo.toml" && -d "${SCRIPT_REPO_DIR}/layer" ]]; then
  DEFAULT_EVO_REPO_DIR="${SCRIPT_REPO_DIR}"
else
  DEFAULT_EVO_REPO_DIR="${BASE_DIR}/volumio-evo"
fi
# Default must be anonymously git-cloneable (no GitHub login). github.com/volumio/volumio-evo
# is not public yet; override when it is: EVO_REPO_URL=https://github.com/volumio/volumio-evo.git
EVO_REPO_URL="${EVO_REPO_URL:-https://github.com/foonerd/volumio-evo.git}"
EVO_REPO_DIR="${EVO_REPO_DIR:-${DEFAULT_EVO_REPO_DIR}}"
DEVICE_IP="${DEVICE_IP:-$(hostname -I 2>/dev/null | awk '{print $1}')}"
if [[ -z "${DEVICE_IP}" ]]; then
  DEVICE_IP="127.0.0.1"
fi
# Must match Evo HTTP bind port in /etc/volumio-evo/config.toml (default 3000).
EVO_HTTP_PORT="${EVO_HTTP_PORT:-3000}"
BACKEND_URL="${BACKEND_URL:-http://${DEVICE_IP}:${EVO_HTTP_PORT}}"
# Default static roots per stock layout (volumioUisList.reference.json). Final UI_DIST_DIR is set
# from /etc/volumio-evo/config.toml [ui] active_layout unless UI_DIST_OVERRIDE is set.
UI_ROOT_MANIFEST="${UI_ROOT_MANIFEST:-/srv/volumio-ui-manifest}"
UI_ROOT_CONTEMPORARY="${UI_ROOT_CONTEMPORARY:-/srv/volumio-ui}"
UI_ROOT_CLASSIC="${UI_ROOT_CLASSIC:-/srv/volumio-ui-classic}"
UI_DIST_DIR="${UI_DIST_DIR:-/srv/volumio-ui}"
UI_DIST_OVERRIDE="${UI_DIST_OVERRIDE:-}"
# Optional: single prebuilt dist/ (index.html); copied to all three layout roots. Prefer layer/web.
UI_DIST_SOURCE="${UI_DIST_SOURCE:-}"
# Set when UI trees installed (all three roots need nginx ACLs).
EVO_UI_INSTALLED_ALL_LAYOUTS=0
MUSIC_ROOT="${MUSIC_ROOT:-/var/lib/volumio-evo/music}"
EVO_BINARY_PATH="${EVO_BINARY_PATH:-/usr/local/bin/volumio-evo}"
# 1: compile on device with cargo (slow). Default 0: install from layer/binaries/<triple>/ only.
EVO_BUILD_FROM_SOURCE="${EVO_BUILD_FROM_SOURCE:-0}"
# Set by argv --build or EVO_BUILD_FROM_SOURCE=1 before install.
EVO_BOOTSTRAP_BUILD=0
# 1: run rustup/cargo only when building from source (--build).
EVO_INSTALL_RUST=0
# Default 1: re-running bootstrap refreshes git checkouts (no separate manual git pull).
EVO_REPO_UPDATE="${EVO_REPO_UPDATE:-1}"
# 1: if git clone fails, use pre-installed EVO_BINARY_PATH only (no layer/web — not recommended).
EVO_ALLOW_BINARY_FALLBACK="${EVO_ALLOW_BINARY_FALLBACK:-0}"
EVO_SOURCE_AVAILABLE=0
BOOTSTRAP_MODE="full"

usage() {
  cat <<'EOF'
Usage:
  sudo ./scripts/bootstrap-volumio-evo-player.sh [MODE]

Modes (default: full):
  (none) | --full     Full install: clone/update repo, backend, static UI, MPD, nginx, validate.
  --reset             Same as --full, but stops volumio-evo first (clean reinstall from repo).
  --upgrade-evo       Clone/pull repo, stop service, replace binary, restart (no UI/nginx/mpd).
  --upgrade-nginx     Re-read [ui] active_layout, rewrite nginx, reload (alias: --apply-ui-only).

  --build             Compile volumio-evo on the device with cargo (installs rustup). Default is
                      to install the prebuilt binary from layer/binaries/<arch-triple>/ only.

Requires a git checkout at EVO_REPO_DIR (default /opt/volumio/volumio-evo) with layer/web/ or
UI_DIST_SOURCE, and (unless --build) layer/binaries/<triple>/volumio-evo for this machine.

Environment (common):
  BASE_DIR=/opt/volumio
  EVO_REPO_URL=https://github.com/foonerd/volumio-evo.git
  EVO_REPO_DIR=/opt/volumio/volumio-evo
  EVO_REPO_UPDATE=1
  EVO_ALLOW_BINARY_FALLBACK=0   # set 1 only for air-gapped + pre-placed binary (no layer/web)
  UI_DIST_SOURCE=/path/to/single/dist
  BACKEND_URL=http://<device-ip>:3000
  EVO_HTTP_PORT=3000
  UI_DIST_OVERRIDE=
  UI_ROOT_MANIFEST=/srv/volumio-ui-manifest
  UI_ROOT_CONTEMPORARY=/srv/volumio-ui
  UI_ROOT_CLASSIC=/srv/volumio-ui-classic
  MUSIC_ROOT=/var/lib/volumio-evo/music
  EVO_BINARY_PATH=/usr/local/bin/volumio-evo
  EVO_BUILD_FROM_SOURCE=0   # or 1 to force cargo like --build

Example:
  sudo BASE_DIR=/opt/volumio ./scripts/bootstrap-volumio-evo-player.sh
  sudo ./scripts/bootstrap-volumio-evo-player.sh --upgrade-evo
  sudo ./scripts/bootstrap-volumio-evo-player.sh --full --build   # compile on device instead of layer/binaries
EOF
}

need_root() {
  if [[ "$(id -u)" -ne 0 ]]; then
    echo "ERROR: run as root (sudo)."
    exit 1
  fi
}

# Read [ui] active_layout from /etc/volumio-evo/config.toml (manifest | contemporary | classic).
read_ui_active_layout_from_config() {
  local cfg="/etc/volumio-evo/config.toml"
  local fallback="contemporary"
  if [[ ! -f "${cfg}" ]]; then
    echo "${fallback}"
    return
  fi
  local parsed=""
  parsed="$(
    python3 - "${cfg}" <<'PY' 2>/dev/null || true
import pathlib, sys

cfg = pathlib.Path(sys.argv[1])
try:
    import tomllib
    data = tomllib.loads(cfg.read_text(encoding="utf-8"))
except Exception:
    sys.exit(1)
ui = data.get("ui") or {}
al = ui.get("active_layout", "contemporary")
if not isinstance(al, str):
    print("contemporary")
else:
    print(al.strip().lower())
PY
  )"
  if [[ -n "${parsed}" ]]; then
    echo "${parsed}"
    return
  fi
  local line
  line="$(grep -E '^[[:space:]]*active_layout[[:space:]]*=' "${cfg}" 2>/dev/null | head -1 || true)"
  if [[ -z "${line}" ]]; then
    echo "${fallback}"
    return
  fi
  line="${line#*=}"
  line="$(echo "${line}" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' -e 's/^"//' -e 's/"$//' -e "s/^'//" -e "s/'$//")"
  echo "${line:-${fallback}}" | tr '[:upper:]' '[:lower:]'
}

# Set UI_DIST_DIR from [ui] active_layout unless UI_DIST_OVERRIDE is set.
apply_ui_dist_dir_from_config() {
  if [[ -n "${UI_DIST_OVERRIDE}" ]]; then
    UI_DIST_DIR="${UI_DIST_OVERRIDE}"
    echo "UI: UI_DIST_OVERRIDE -> nginx root ${UI_DIST_DIR}"
    return 0
  fi
  local layout
  layout="$(read_ui_active_layout_from_config)"
  case "${layout}" in
    manifest)
      UI_DIST_DIR="${UI_ROOT_MANIFEST}"
      ;;
    contemporary)
      UI_DIST_DIR="${UI_ROOT_CONTEMPORARY}"
      ;;
    classic)
      UI_DIST_DIR="${UI_ROOT_CLASSIC}"
      ;;
    *)
      echo "WARN: unknown active_layout '${layout}', using contemporary root"
      UI_DIST_DIR="${UI_ROOT_CONTEMPORARY}"
      ;;
  esac
  echo "UI: active_layout=${layout} -> nginx root ${UI_DIST_DIR}"
}

ensure_config_has_ui_section() {
  local cfg="/etc/volumio-evo/config.toml"
  [[ -f "${cfg}" ]] || return 0
  if grep -qE '^[[:space:]]*\[ui\][[:space:]]*$' "${cfg}"; then
    if ! grep -qE '^[[:space:]]*active_layout[[:space:]]*=' "${cfg}"; then
      sed -i '/^[[:space:]]*\[ui\][[:space:]]*$/a active_layout = "contemporary"' "${cfg}"
    fi
  else
    cat >> "${cfg}" <<'EOF'

[ui]
active_layout = "contemporary"
EOF
  fi
}

layer_web_trees_complete() {
  local repo="$1"
  local lw="${repo}/layer/web"
  [[ -f "${lw}/classic/index.html" && -f "${lw}/contemporary/index.html" && -f "${lw}/manifest/index.html" ]]
}

write_ui_local_config() {
  local root="$1"
  mkdir -p "${root}/app"
  cat > "${root}/app/local-config.json" <<EOF
{
  "localhost": "${BACKEND_URL}"
}
EOF
}

strip_optional_socketio_inject() {
  local idx="$1"
  [[ -f "${idx}" ]] || return 0
  sed -i 's|<script src="/scripts/socket.io-4.js"></script>||g' "${idx}" 2>/dev/null || true
  rm -f "$(dirname "${idx}")/scripts/socket.io-4.js" 2>/dev/null || true
}

install_ui_from_layer_web() {
  local lw="${EVO_REPO_DIR}/layer/web"
  echo "Installing UI from ${lw} (classic / contemporary / manifest) ..."
  rsync -a --delete "${lw}/classic/" "${UI_ROOT_CLASSIC}/"
  rsync -a --delete "${lw}/contemporary/" "${UI_ROOT_CONTEMPORARY}/"
  rsync -a --delete "${lw}/manifest/" "${UI_ROOT_MANIFEST}/"
  local d
  for d in "${UI_ROOT_CLASSIC}" "${UI_ROOT_CONTEMPORARY}" "${UI_ROOT_MANIFEST}"; do
    write_ui_local_config "${d}"
    strip_optional_socketio_inject "${d}/index.html"
  done
  EVO_UI_INSTALLED_ALL_LAYOUTS=1
}

install_packages() {
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt-get install -y \
    git curl ca-certificates nginx mpd python3 acl \
    build-essential pkg-config libssl-dev \
    rsync
  if [[ "${EVO_INSTALL_RUST:-0}" == "1" ]]; then
    ensure_rustup_toolchain
  else
    echo "Skipping rustup (installing prebuilt volumio-evo from layer/binaries; use --build to compile on device)."
  fi
}

# Installs rustup to /usr/local/{rustup,cargo} so cargo/rustc are new enough (see rust-toolchain.toml).
ensure_rustup_toolchain() {
  export RUSTUP_HOME="${RUSTUP_HOME:-/usr/local/rustup}"
  export CARGO_HOME="${CARGO_HOME:-/usr/local/cargo}"
  local cargo_bin="${CARGO_HOME}/bin/cargo"
  if [[ -x "${cargo_bin}" ]]; then
    echo "Using existing rustup cargo at ${cargo_bin}"
    "${cargo_bin}" --version
    return 0
  fi
  echo "Installing Rust via rustup (apt rustc is too old for this project)..."
  # rustup refuses to install if /usr/bin/rustc exists (Debian rust/cargo packages).
  if command -v apt-get >/dev/null 2>&1; then
    apt-get remove -y rustc cargo rust-gdb rust-doc 2>/dev/null || true
  fi
  # Non-apt leftovers or mixed installs: allow rustup to proceed without interactive "yes".
  export RUSTUP_INIT_SKIP_PATH_CHECK=yes

  mkdir -p "$(dirname "${RUSTUP_HOME}")" "$(dirname "${CARGO_HOME}")"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable
  if [[ ! -x "${cargo_bin}" ]]; then
    echo "ERROR: rustup install failed; ${cargo_bin} missing."
    exit 1
  fi
  "${cargo_bin}" --version
}

# Stop backend before replacing /usr/local/bin/volumio-evo (avoids "Text file busy" / stale process).
stop_volumio_evo_if_running() {
  if systemctl is-active --quiet volumio-evo 2>/dev/null; then
    echo "Stopping volumio-evo before replacing binary..."
    systemctl stop volumio-evo
  fi
}

# Clone or pull EVO_REPO_DIR. Fails unless EVO_ALLOW_BINARY_FALLBACK=1 with a binary on disk.
ensure_volumio_evo_checkout() {
  if [[ -f "${EVO_REPO_DIR}/Cargo.toml" && -d "${EVO_REPO_DIR}/layer" ]]; then
    echo "Using volumio-evo at ${EVO_REPO_DIR}"
    EVO_SOURCE_AVAILABLE=1
    if [[ -d "${EVO_REPO_DIR}/.git" && "${EVO_REPO_UPDATE:-1}" == "1" ]]; then
      clone_or_update_repo "${EVO_REPO_URL}" "${EVO_REPO_DIR}" "" "1" || {
        echo "ERROR: git update failed for ${EVO_REPO_DIR}"
        return 1
      }
    fi
    return 0
  fi

  echo "Cloning volumio-evo -> ${EVO_REPO_DIR} (from ${EVO_REPO_URL}) ..."
  mkdir -p "$(dirname "${EVO_REPO_DIR}")"
  if clone_or_update_repo "${EVO_REPO_URL}" "${EVO_REPO_DIR}" "" "1"; then
    EVO_SOURCE_AVAILABLE=1
    return 0
  fi

  if [[ "${EVO_ALLOW_BINARY_FALLBACK:-0}" == "1" && -x "${EVO_BINARY_PATH}" ]]; then
    echo "WARN: clone failed; using existing binary at ${EVO_BINARY_PATH} (EVO_ALLOW_BINARY_FALLBACK=1). Static UI will be missing unless provided separately."
    EVO_SOURCE_AVAILABLE=0
    return 0
  fi

  echo "ERROR: Cannot clone or update volumio-evo at ${EVO_REPO_DIR}."
  echo "Fix network, credentials, or EVO_REPO_URL; or clone manually. Air-gap: copy a full repo tree to EVO_REPO_DIR, or set EVO_ALLOW_BINARY_FALLBACK=1 (binary-only fallback)."
  return 1
}

clone_or_update_repo() {
  local url="$1"
  local dir="$2"
  local branch="${3:-}"
  if [[ ! -d "${dir}/.git" ]]; then
    mkdir -p "$(dirname "${dir}")"
    if [[ -n "${branch}" ]]; then
      GIT_TERMINAL_PROMPT=0 git clone --branch "${branch}" --depth 1 "${url}" "${dir}" || {
        echo "ERROR: cannot clone ${url}"
        echo "Private or missing repo: use a public URL (EVO_REPO_URL=...) or clone with SSH/credentials."
        echo "Or place a full checkout at EVO_REPO_DIR and re-run."
        return 1
      }
    else
      GIT_TERMINAL_PROMPT=0 git clone "${url}" "${dir}" || {
        echo "ERROR: cannot clone ${url}"
        echo "Private or missing repo: use a public URL (EVO_REPO_URL=...) or clone with SSH/credentials."
        echo "Or place a full checkout at EVO_REPO_DIR and re-run."
        return 1
      }
    fi
  elif [[ "${4:-1}" == "1" ]]; then
    if [[ -n "${branch}" ]]; then
      git -C "${dir}" fetch origin "${branch}" || true
      git -C "${dir}" checkout "${branch}" || true
    fi
    git -C "${dir}" fetch --all --prune || return 1
    git -C "${dir}" pull --ff-only || return 1
  else
    echo "Using existing repo without update: ${dir}"
  fi
}

configure_mpd() {
  mkdir -p "${MUSIC_ROOT}"/{INTERNAL,USB,NAS,SMB}
  if grep -q '^[[:space:]]*music_directory' /etc/mpd.conf; then
    sed -i 's|^[[:space:]]*music_directory.*|music_directory "'"${MUSIC_ROOT}"'"|' /etc/mpd.conf
  else
    echo 'music_directory "'"${MUSIC_ROOT}"'"' >> /etc/mpd.conf
  fi
  systemctl enable mpd >/dev/null 2>&1 || true
  systemctl restart mpd
}

install_evo_binary() {
  local src="$1"
  local dst="/usr/local/bin/volumio-evo"
  if [[ "$(readlink -f "${src}")" == "$(readlink -f "${dst}")" ]]; then
    chmod 0755 "${dst}"
    echo "Using existing volumio-evo binary at ${dst}"
    return 0
  fi
  install -m 0755 "${src}" "${dst}"
}

# Map `uname -m` to Rust target triple (Linux GNU) for layer/binaries/<triple>/volumio-evo.
host_rust_triple() {
  local m
  m="$(uname -m)"
  case "${m}" in
    aarch64) echo "aarch64-unknown-linux-gnu" ;;
    armv7l | armv6l | armv5tel) echo "armv7-unknown-linux-gnueabihf" ;;
    x86_64 | amd64) echo "x86_64-unknown-linux-gnu" ;;
    *) echo "" ;;
  esac
}

build_and_install_evo() {
  export RUSTUP_HOME="${RUSTUP_HOME:-/usr/local/rustup}"
  export CARGO_HOME="${CARGO_HOME:-/usr/local/cargo}"
  export PATH="${CARGO_HOME}/bin:${PATH}"

  stop_volumio_evo_if_running

  if [[ "${EVO_SOURCE_AVAILABLE}" == "1" ]]; then
    local triple lb
    triple="$(host_rust_triple)"
    lb="${EVO_REPO_DIR}/layer/binaries/${triple}/volumio-evo"
    if [[ "${EVO_BOOTSTRAP_BUILD:-0}" == "1" ]]; then
      echo "Building volumio-evo from source (--build)..."
      if ! command -v cargo >/dev/null 2>&1; then
        echo "ERROR: cargo not in PATH (${CARGO_HOME}/bin). EVO_INSTALL_RUST should have run."
        exit 1
      fi
      cargo -V
      (cd "${EVO_REPO_DIR}" && cargo build --release -p volumio-evo-core)
      install_evo_binary "${EVO_REPO_DIR}/target/release/volumio-evo"
    elif [[ -n "${triple}" && -x "${lb}" ]]; then
      echo "Installing volumio-evo from ${lb}"
      install_evo_binary "${lb}"
    else
      echo "ERROR: internal: no prebuilt at ${lb} and --build not set."
      exit 1
    fi
  else
    if [[ ! -x "${EVO_BINARY_PATH}" ]]; then
      echo "ERROR: volumio-evo source unavailable and binary not found at ${EVO_BINARY_PATH}"
      echo "Clone failed or EVO_REPO_DIR missing: set EVO_REPO_URL to a public git URL, or copy a full checkout to EVO_REPO_DIR, or set EVO_BINARY_PATH."
      exit 1
    fi
    install_evo_binary "${EVO_BINARY_PATH}"
  fi

  mkdir -p /etc/volumio-evo /usr/share/volumio-evo/plugins /var/lib/volumio-evo/albumart
  # Stock browse-source icons (music_service/mpd/*icon.png) for GET /albumart?sourceicon=...
  if [[ "${EVO_SOURCE_AVAILABLE}" == "1" && -d "${EVO_REPO_DIR}/crates/core/assets/bundled-plugins" ]]; then
    cp -a "${EVO_REPO_DIR}/crates/core/assets/bundled-plugins/." /usr/share/volumio-evo/plugins/
  fi
  if [[ "${EVO_SOURCE_AVAILABLE}" == "1" && -f "${EVO_REPO_DIR}/layer/config/volumio-evo.toml.example" ]]; then
    if [[ ! -f /etc/volumio-evo/config.toml ]]; then
      cp "${EVO_REPO_DIR}/layer/config/volumio-evo.toml.example" /etc/volumio-evo/config.toml
    fi
  elif [[ ! -f /etc/volumio-evo/config.toml ]]; then
    cat > /etc/volumio-evo/config.toml <<EOF
bind = "0.0.0.0:3000"
plugin_dir = "/usr/share/volumio-evo/plugins"
mpd_host = "127.0.0.1"
mpd_port = 6600
albumart_root = "/var/lib/volumio-evo/albumart"

[music_sources]
music_root = "${MUSIC_ROOT}"
EOF
  fi

  ensure_config_has_ui_section

  if grep -q '^[[:space:]]*music_root' /etc/volumio-evo/config.toml; then
    sed -i 's|^[[:space:]]*music_root.*|music_root = "'"${MUSIC_ROOT}"'"|' /etc/volumio-evo/config.toml
  else
    cat >> /etc/volumio-evo/config.toml <<EOF
[music_sources]
music_root = "${MUSIC_ROOT}"
EOF
  fi

  if [[ "${EVO_SOURCE_AVAILABLE}" == "1" && -f "${EVO_REPO_DIR}/layer/systemd/volumio-evo.service" ]]; then
    cp "${EVO_REPO_DIR}/layer/systemd/volumio-evo.service" /etc/systemd/system/volumio-evo.service
  else
    cat > /etc/systemd/system/volumio-evo.service <<'EOF'
[Unit]
Description=Volumio Evo backend
After=network.target sound.target mpd.service

[Service]
Type=simple
ExecStart=/usr/local/bin/volumio-evo
Restart=on-failure
RestartSec=5
Environment=VOLUMIO_EVO_CONFIG=/etc/volumio-evo/config.toml

[Install]
WantedBy=multi-user.target
EOF
  fi

  systemctl daemon-reload
  systemctl enable volumio-evo >/dev/null 2>&1 || true
  systemctl restart volumio-evo
}

install_ui() {
  EVO_UI_INSTALLED_ALL_LAYOUTS=0
  if [[ -n "${UI_DIST_SOURCE}" && -f "${UI_DIST_SOURCE}/index.html" ]]; then
    echo "Installing UI from UI_DIST_SOURCE=${UI_DIST_SOURCE} (same tree -> all layout roots) ..."
    local d
    for d in "${UI_ROOT_CLASSIC}" "${UI_ROOT_CONTEMPORARY}" "${UI_ROOT_MANIFEST}"; do
      mkdir -p "${d}"
      rsync -a --delete "${UI_DIST_SOURCE}/" "${d}/"
      write_ui_local_config "${d}"
      strip_optional_socketio_inject "${d}/index.html"
    done
    EVO_UI_INSTALLED_ALL_LAYOUTS=1
    return 0
  fi

  if layer_web_trees_complete "${EVO_REPO_DIR}"; then
    install_ui_from_layer_web
    return 0
  fi

  echo "ERROR: No static UI. Add layer/web/classic, contemporary, and manifest (each with index.html),"
  echo "or set UI_DIST_SOURCE to a directory containing index.html."
  exit 1
}

ensure_nginx_access_for_dir() {
  local dir="$1"
  [[ -d "${dir}" ]] || return 0
  if id -u www-data >/dev/null 2>&1 && command -v setfacl >/dev/null 2>&1; then
    setfacl -R -m u:www-data:rX "${dir}" || true
    local path="${dir}"
    while true; do
      setfacl -m u:www-data:x "${path}" || true
      if [[ "${path}" == "/" ]]; then
        break
      fi
      path="$(dirname "${path}")"
    done
  else
    chmod -R a+rX "${dir}" || true
  fi
}

ensure_nginx_access() {
  ensure_nginx_access_for_dir "${UI_DIST_DIR}"
  if [[ "${EVO_UI_INSTALLED_ALL_LAYOUTS:-0}" == "1" ]]; then
    ensure_nginx_access_for_dir "${UI_ROOT_MANIFEST}"
    ensure_nginx_access_for_dir "${UI_ROOT_CONTEMPORARY}"
    ensure_nginx_access_for_dir "${UI_ROOT_CLASSIC}"
  fi
}

configure_nginx() {
  local site="/etc/nginx/sites-available/volumio-evo-player"
  # Remove legacy site names from earlier bootstrap attempts to avoid
  # duplicate "default_server" listeners on port 80.
  rm -f /etc/nginx/sites-enabled/volumio-evo-ui-test
  rm -f /etc/nginx/sites-available/volumio-evo-ui-test

  cat > "${site}" <<EOF
server {
    listen 80 default_server;
    listen [::]:80 default_server;
    server_name _;

    root ${UI_DIST_DIR};
    index index.html;

    # Stock UI GET /api/host — dynamic backend URL (Socket.IO) when IP/interface changes.
    location = /api/host {
        proxy_pass http://127.0.0.1:${EVO_HTTP_PORT};
        proxy_http_version 1.1;
        proxy_set_header Host \$host;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
    }

    # Prevent browser from caching backend config (stale IP breaks Socket.IO).
    location = /app/local-config.json {
        add_header Cache-Control "no-store, no-cache, must-revalidate";
        add_header Pragma "no-cache";
        expires 0;
    }

    location / {
        try_files \$uri \$uri/ /index.html;
    }
}
EOF
  rm -f /etc/nginx/sites-enabled/default
  ln -sf "${site}" /etc/nginx/sites-enabled/volumio-evo-player
  nginx -t
  systemctl enable nginx >/dev/null 2>&1 || true
  systemctl restart nginx
}

validate_backend_only() {
  echo "Validating backend..."
  curl -fsS "http://127.0.0.1:${EVO_HTTP_PORT}/api/health" >/dev/null
  curl -fsS "http://127.0.0.1:${EVO_HTTP_PORT}/api/v1/ping" >/dev/null
  echo "Backend OK."
}

validate_stack() {
  validate_backend_only

  local ip
  ip="$(hostname -I | awk '{print $1}')"
  if [[ -z "${ip}" ]]; then
    ip="127.0.0.1"
  fi

  echo
  echo "Bootstrap complete."
  echo "Player URL: http://${ip}/playback"
  echo
  echo "Expected tester action:"
  echo "  Open URL -> select track -> press Play -> verify speaker audio."
}

parse_bootstrap_mode() {
  case "${1:-}" in
    "" | --full)
      BOOTSTRAP_MODE="full"
      ;;
    --reset)
      BOOTSTRAP_MODE="reset"
      ;;
    --upgrade-evo)
      BOOTSTRAP_MODE="upgrade-evo"
      ;;
    --upgrade-nginx | --apply-ui-only)
      BOOTSTRAP_MODE="upgrade-nginx"
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "ERROR: unknown argument: $1"
      usage
      exit 1
      ;;
  esac
}

main() {
  EVO_BOOTSTRAP_BUILD=0
  [[ "${EVO_BUILD_FROM_SOURCE:-0}" == "1" ]] && EVO_BOOTSTRAP_BUILD=1
  local -a mode_args=()
  local a
  for a in "$@"; do
    if [[ "$a" == "--build" ]]; then
      EVO_BOOTSTRAP_BUILD=1
    else
      mode_args+=("$a")
    fi
  done
  parse_bootstrap_mode "${mode_args[0]:-}"

  if [[ "${BOOTSTRAP_MODE}" == "upgrade-nginx" ]]; then
    need_root
    apply_ui_dist_dir_from_config
    ensure_nginx_access
    configure_nginx
    echo "nginx root: ${UI_DIST_DIR}"
    exit 0
  fi

  need_root

  if [[ "${BOOTSTRAP_MODE}" == "reset" ]]; then
    echo "Reset: stopping volumio-evo before full reinstall..."
    stop_volumio_evo_if_running
  fi

  ensure_volumio_evo_checkout || exit 1

  EVO_INSTALL_RUST=0
  if [[ "${EVO_SOURCE_AVAILABLE}" == "1" ]]; then
    local triple lb
    triple="$(host_rust_triple)"
    lb="${EVO_REPO_DIR}/layer/binaries/${triple}/volumio-evo"
    if [[ "${EVO_BOOTSTRAP_BUILD}" == "1" ]]; then
      EVO_INSTALL_RUST=1
    else
      if [[ -z "${triple}" ]]; then
        echo "ERROR: Unsupported machine ($(uname -m)). Use --build to compile from source on this device."
        exit 1
      fi
      if [[ ! -x "${lb}" ]]; then
        echo "ERROR: No prebuilt binary at ${lb}"
        echo "Ensure layer/binaries is populated in the repo (see docs/BUILD_GUIDE.md), or run with --build to compile (installs rustup; slow on Pi)."
        exit 1
      fi
    fi
  fi

  install_packages

  if [[ "${BOOTSTRAP_MODE}" == "upgrade-evo" ]]; then
    build_and_install_evo
    validate_backend_only
    exit 0
  fi

  configure_mpd
  build_and_install_evo
  apply_ui_dist_dir_from_config
  install_ui
  ensure_nginx_access
  configure_nginx
  validate_stack
}

main "$@"
