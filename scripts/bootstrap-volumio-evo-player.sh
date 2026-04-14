#!/usr/bin/env bash
set -euo pipefail

# Full bootstrap for a tester-ready Volumio Evo player on Debian/Raspberry Pi OS.
# - installs required packages
# - clones/updates volumio-evo and Volumio2-UI
# - builds Evo backend and UI dist
# - configures MPD, systemd, nginx
# - exposes UI on port 80 for "open UI -> play"

BASE_DIR="${BASE_DIR:-/opt/volumio}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT_REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
if [[ -f "${SCRIPT_REPO_DIR}/Cargo.toml" && -d "${SCRIPT_REPO_DIR}/layer" ]]; then
  DEFAULT_EVO_REPO_DIR="${SCRIPT_REPO_DIR}"
else
  DEFAULT_EVO_REPO_DIR="${BASE_DIR}/volumio-evo"
fi
EVO_REPO_URL="${EVO_REPO_URL:-https://github.com/volumio/volumio-evo.git}"
UI_REPO_URL="${UI_REPO_URL:-https://github.com/volumio/Volumio2-UI.git}"
EVO_REPO_DIR="${EVO_REPO_DIR:-${DEFAULT_EVO_REPO_DIR}}"
UI_REPO_DIR="${UI_REPO_DIR:-${BASE_DIR}/Volumio2-UI}"
UI_THEME="${UI_THEME:-volumio}"
DEVICE_IP="${DEVICE_IP:-$(hostname -I 2>/dev/null | awk '{print $1}')}"
if [[ -z "${DEVICE_IP}" ]]; then
  DEVICE_IP="127.0.0.1"
fi
BACKEND_URL="${BACKEND_URL:-http://${DEVICE_IP}:3000}"
UI_DIST_DIR="${UI_DIST_DIR:-/srv/volumio-ui}"
UI_DIST_SOURCE="${UI_DIST_SOURCE:-}"
UI_BUILD="${UI_BUILD:-auto}"
MUSIC_ROOT="${MUSIC_ROOT:-/var/lib/volumio-evo/music}"
EVO_BINARY_PATH="${EVO_BINARY_PATH:-/usr/local/bin/volumio-evo}"
EVO_SOURCE_AVAILABLE=0

usage() {
  cat <<'EOF'
Usage:
  sudo ./scripts/bootstrap-volumio-evo-player.sh

Optional environment overrides:
  BASE_DIR=/opt/volumio
  EVO_REPO_URL=https://github.com/volumio/volumio-evo.git
  UI_REPO_URL=https://github.com/volumio/Volumio2-UI.git
  EVO_REPO_DIR=/path/to/local/volumio-evo
  UI_REPO_DIR=/opt/volumio/Volumio2-UI
  EVO_REPO_UPDATE=0
  UI_REPO_UPDATE=1
  UI_THEME=volumio
  UI_DIST_SOURCE=/path/to/prebuilt/Volumio2-UI-dist
  UI_BUILD=auto|always|never
  BACKEND_URL=http://<device-ip>:3000
  UI_DIST_DIR=/srv/volumio-ui
  MUSIC_ROOT=/var/lib/volumio-evo/music
  EVO_BINARY_PATH=/usr/local/bin/volumio-evo

Example:
  sudo BASE_DIR=/opt/volumio UI_THEME=volumio ./scripts/bootstrap-volumio-evo-player.sh
EOF
}

need_root() {
  if [[ "$(id -u)" -ne 0 ]]; then
    echo "ERROR: run as root (sudo)."
    exit 1
  fi
}

install_packages() {
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt-get install -y \
    git curl ca-certificates nginx mpd python3 jq acl \
    build-essential pkg-config libssl-dev \
    nodejs npm rustc cargo rsync
  npm install -g bower
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
        echo "Provide local path with *_REPO_DIR or use repository URL with access."
        return 1
      }
    else
      GIT_TERMINAL_PROMPT=0 git clone "${url}" "${dir}" || {
        echo "ERROR: cannot clone ${url}"
        echo "Provide local path with *_REPO_DIR or use repository URL with access."
        return 1
      }
    fi
  elif [[ "${4:-1}" == "1" ]]; then
    git -C "${dir}" fetch --all --prune || return 1
    git -C "${dir}" pull --ff-only || return 1
  else
    echo "Using existing repo without update: ${dir}"
  fi
}

configure_mpd() {
  mkdir -p "${MUSIC_ROOT}"/{local,usb,nas,smb}
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

build_and_install_evo() {
  if [[ "${EVO_SOURCE_AVAILABLE}" == "1" ]]; then
    echo "Building volumio-evo backend..."
    cargo -V >/dev/null
    (cd "${EVO_REPO_DIR}" && cargo build --release -p volumio-evo-core)
    install_evo_binary "${EVO_REPO_DIR}/target/release/volumio-evo"
  else
    if [[ ! -x "${EVO_BINARY_PATH}" ]]; then
      echo "ERROR: volumio-evo source unavailable and binary not found at ${EVO_BINARY_PATH}"
      echo "Provide EVO_REPO_DIR with source, or set EVO_BINARY_PATH to a valid binary."
      exit 1
    fi
    install_evo_binary "${EVO_BINARY_PATH}"
  fi

  mkdir -p /etc/volumio-evo /usr/share/volumio-evo/plugins /var/lib/volumio-evo/albumart
  if [[ "${EVO_SOURCE_AVAILABLE}" == "1" && -f "${EVO_REPO_DIR}/layer/config/volumio-evo.toml.example" ]]; then
    cp "${EVO_REPO_DIR}/layer/config/volumio-evo.toml.example" /etc/volumio-evo/config.toml
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

build_ui() {
  local dist_src=""
  local candidates=()
  if [[ -n "${UI_DIST_SOURCE}" ]]; then
    candidates+=("${UI_DIST_SOURCE}")
  fi
  candidates+=(
    "/home/volumio/ui/Volumio2-UI-dist"
    "/opt/volumio/Volumio2-UI-dist"
    "${UI_REPO_DIR}/dist"
  )

  for c in "${candidates[@]}"; do
    if [[ -f "${c}/index.html" ]]; then
      dist_src="${c}"
      break
    fi
  done

  if [[ "${UI_BUILD}" == "never" && -z "${dist_src}" ]]; then
    echo "ERROR: UI_BUILD=never but no prebuilt dist found."
    echo "Set UI_DIST_SOURCE to a folder containing index.html."
    exit 1
  fi

  if [[ "${UI_BUILD}" == "always" || ( "${UI_BUILD}" == "auto" && -z "${dist_src}" ) ]]; then
    echo "Building Volumio2-UI (${UI_THEME})..."
    (
      cd "${UI_REPO_DIR}" && \
      PHANTOMJS_SKIP_DOWNLOAD=true npm install --legacy-peer-deps --no-audit --no-fund
    )
    (cd "${UI_REPO_DIR}" && bower install --allow-root --config.interactive=false)
    (cd "${UI_REPO_DIR}" && npm run "build:${UI_THEME}")
    dist_src="${UI_REPO_DIR}/dist"
  else
    echo "Using prebuilt UI dist: ${dist_src}"
  fi

  mkdir -p "${UI_DIST_DIR}"
  rsync -a --delete "${dist_src}/" "${UI_DIST_DIR}/"

  mkdir -p "${UI_DIST_DIR}/app"
  cat > "${UI_DIST_DIR}/app/local-config.json" <<EOF
{
  "localhost": "${BACKEND_URL}"
}
EOF

  patch_socketio_client
}

patch_socketio_client() {
  # Volumio2-UI bundles socket.io-client 2.3.1 (protocol v3).
  # socketioxide (Rust) supports protocol v4/v5 only.
  # Fix: inject socket.io v4 client after vendor bundle so window.io is overridden.
  local sio_js="${UI_DIST_DIR}/scripts/socket.io-4.js"
  if [[ ! -f "${sio_js}" ]]; then
    echo "Downloading socket.io v4 client..."
    curl -fsSL "https://cdn.socket.io/4.7.5/socket.io.min.js" -o "${sio_js}" || {
      echo "ERROR: failed to download socket.io v4 client."
      exit 1
    }
  fi

  local idx="${UI_DIST_DIR}/index.html"
  if ! grep -q 'socket.io-4.js' "${idx}"; then
    sed -i 's|</body>|<script src="/scripts/socket.io-4.js"></script>\n</body>|' "${idx}"
    echo "Injected socket.io v4 client into index.html"
  fi
}

ensure_nginx_access() {
  if id -u www-data >/dev/null 2>&1 && command -v setfacl >/dev/null 2>&1; then
    setfacl -R -m u:www-data:rX "${UI_DIST_DIR}" || true
    local path="${UI_DIST_DIR}"
    while true; do
      setfacl -m u:www-data:x "${path}" || true
      if [[ "${path}" == "/" ]]; then
        break
      fi
      path="$(dirname "${path}")"
    done
  else
    chmod -R a+rX "${UI_DIST_DIR}" || true
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

    # Force stock UI to use app/local-config.json fallback.
    location = /api/host {
        return 404;
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

validate_stack() {
  echo "Validating backend..."
  curl -fsS "http://127.0.0.1:3000/api/health" >/dev/null
  curl -fsS "http://127.0.0.1:3000/api/v1/ping" >/dev/null

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

main() {
  if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
  fi

  need_root
  install_packages
  if [[ -f "${EVO_REPO_DIR}/Cargo.toml" && -d "${EVO_REPO_DIR}/layer" ]]; then
    echo "Using local volumio-evo source: ${EVO_REPO_DIR}"
    EVO_SOURCE_AVAILABLE=1
    if [[ -d "${EVO_REPO_DIR}/.git" && "${EVO_REPO_UPDATE:-0}" == "1" ]]; then
      clone_or_update_repo "${EVO_REPO_URL}" "${EVO_REPO_DIR}" "" "1" || \
        echo "WARN: failed to update local volumio-evo source, continuing with current checkout."
    fi
  else
    if [[ -x "${EVO_BINARY_PATH}" ]]; then
      echo "Using existing volumio-evo binary fallback: ${EVO_BINARY_PATH}"
      EVO_SOURCE_AVAILABLE=0
    elif clone_or_update_repo "${EVO_REPO_URL}" "${EVO_REPO_DIR}" "" "1"; then
      EVO_SOURCE_AVAILABLE=1
    else
      echo "WARN: unable to clone volumio-evo source, will use binary path fallback."
      EVO_SOURCE_AVAILABLE=0
    fi
  fi

  if [[ -f "${UI_REPO_DIR}/package.json" ]]; then
    echo "Using local Volumio2-UI source: ${UI_REPO_DIR}"
    if [[ -d "${UI_REPO_DIR}/.git" && "${UI_REPO_UPDATE:-1}" == "1" ]]; then
      clone_or_update_repo "${UI_REPO_URL}" "${UI_REPO_DIR}" "" "1" || \
        echo "WARN: failed to update local Volumio2-UI source, continuing with current checkout."
    fi
  else
    clone_or_update_repo "${UI_REPO_URL}" "${UI_REPO_DIR}" "" "1"
  fi
  configure_mpd
  build_and_install_evo
  build_ui
  ensure_nginx_access
  configure_nginx
  validate_stack
}

main "$@"
