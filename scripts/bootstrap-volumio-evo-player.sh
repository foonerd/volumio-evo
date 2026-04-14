#!/usr/bin/env bash
set -euo pipefail

# One-shot tester install for Debian / Raspberry Pi OS (including Raspberry Pi OS Lite).
# This is the supported way to turn a stock "lite" image into a default Volumio Evo player
# rig; do not replicate the steps by hand for normal test builds.
#
# Run ONLY this script as root. It installs packages, clones repos, normalizes Volumio2-UI
# for Node 20 + arm64 when the git default still lists legacy deps (phantom / node-sass),
# then runs npm/bower/gulp internally — no manual npm steps.
#
# UI dist: for typical Evo installs this script is the only supported path to a fresh
# Volumio2-UI dist (npm run build:<theme> → dist/). Alternatively set UI_DIST_SOURCE to a
# prebuilt tree, or UI_BUILD=never when dist/ already exists — see build_ui().
# Product-specific Volumio2-UI files (volumio3 theme, etc.) live under this repo:
#   layer/volumio2-ui-overlay/   → rsynced onto the UI checkout after normalize (see apply_volumio2_ui_overlay).
#
# - apt: nginx, mpd, toolchain, jq, python3, …
# - git: volumio-evo + Volumio2-UI
# - UI: normalize() → wipe node_modules + lockfile → npm install → bower → gulp build → /srv
# - systemd + nginx on port 80

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
UI_REPO_URL="${UI_REPO_URL:-https://github.com/volumio/Volumio2-UI.git}"
UI_REPO_BRANCH="${UI_REPO_BRANCH:-}"
EVO_REPO_DIR="${EVO_REPO_DIR:-${DEFAULT_EVO_REPO_DIR}}"
UI_REPO_DIR="${UI_REPO_DIR:-${BASE_DIR}/Volumio2-UI}"
# Default theme for Evo + stock Volumio2-UI package.json scripts (build:volumio3).
UI_THEME="${UI_THEME:-volumio3}"
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
Usage (this is the only command testers should run):
  sudo ./scripts/bootstrap-volumio-evo-player.sh

Do not run npm, npx, or bower yourself; this script invokes them internally.

Optional environment overrides:
  BASE_DIR=/opt/volumio
  EVO_REPO_URL=https://github.com/foonerd/volumio-evo.git
  UI_REPO_URL=https://github.com/volumio/Volumio2-UI.git
  EVO_REPO_DIR=/path/to/local/volumio-evo
  UI_REPO_DIR=/opt/volumio/Volumio2-UI
  EVO_REPO_UPDATE=0
  UI_REPO_UPDATE=1
  UI_THEME=volumio3
  UI_DIST_SOURCE=/path/to/prebuilt/Volumio2-UI-dist
  UI_BUILD=auto|always|never
  BACKEND_URL=http://<device-ip>:3000
  UI_DIST_DIR=/srv/volumio-ui
  MUSIC_ROOT=/var/lib/volumio-evo/music
  EVO_BINARY_PATH=/usr/local/bin/volumio-evo

Example:
  sudo BASE_DIR=/opt/volumio UI_THEME=volumio3 ./scripts/bootstrap-volumio-evo-player.sh

  UI_THEME=volumio   # classic theme if you need build:volumio instead of volumio3

  UI_REPO_BRANCH=my-branch   # optional: git branch for Volumio2-UI clone/pull (omit = remote default)
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
    rustc cargo rsync
  # UI build uses Node 20 via nvm in build_ui(); avoid apt nodejs (too old) conflicting with nvm.
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

# Align an older Volumio2-UI git tree with what Evo expects on Node 20 / linux-arm64 (no separate npm steps for the user).
# Stock upstream may still list karma-phantomjs / node-sass; npm install then fails on Raspberry Pi. This runs inside bootstrap only.
normalize_volumio2_ui_checkout() {
  local root="${UI_REPO_DIR}"
  local pj="${root}/package.json"
  [[ -f "${pj}" ]] || return 0
  command -v jq >/dev/null 2>&1 || {
    echo "ERROR: jq is required (install_packages should have installed it)."
    exit 1
  }
  command -v python3 >/dev/null 2>&1 || {
    echo "ERROR: python3 is required."
    exit 1
  }

  echo "Normalizing Volumio2-UI sources for Node 20 + arm64 (in-place; part of this script only)..."
  local tmp
  tmp="$(mktemp)"
  jq '
    .devDependencies = (.devDependencies // {})
    | .devDependencies["gulp-sass"] = "^5.1.0"
    | .devDependencies["sass"] = "^1.77.0"
    | .devDependencies["bower"] = (.devDependencies.bower // "^1.8.14")
    | .devDependencies["karma-chrome-launcher"] = "^3.2.0"
    | del(.devDependencies["karma-phantomjs-launcher"])
    | del(.devDependencies["node-sass"])
    | .engines.node = "20.0.0"
    | .overrides = (.overrides // {})
    | .overrides["graceful-fs"] = "^4.2.11"
  ' "${pj}" > "${tmp}" && mv "${tmp}" "${pj}"

  if [[ -f "${root}/karma.conf.js" ]]; then
    sed -i \
      -e "s/karma-phantomjs-launcher/karma-chrome-launcher/g" \
      -e "s/'PhantomJS'/'ChromeHeadless'/g" \
      -e 's/"PhantomJS"/"ChromeHeadless"/g' \
      "${root}/karma.conf.js" 2>/dev/null || true
  fi

  python3 - "${root}" <<'PY'
import pathlib, re, sys

root = pathlib.Path(sys.argv[1])

def write(p: pathlib.Path, text: str) -> None:
    p.write_text(text, encoding="utf-8")

gs = root / "gulp" / "styles.js"
if gs.is_file():
    t = gs.read_text(encoding="utf-8")
    if "require('gulp-sass')(require('sass'))" not in t and "gulp-load-plugins" in t:
        t = re.sub(
            r"(var \$ = require\('gulp-load-plugins'\)\(\);\s*\n)",
            r"\1var gulpSass = require('gulp-sass')(require('sass'));\n\n",
            t,
            count=1,
        )
        t = re.sub(
            r"var sassOptions = \{\s*style:\s*'expanded'\s*\};",
            "var sassOptions = {\n    outputStyle: 'expanded',\n    quietDeps: true\n  };",
            t,
            count=1,
        )
        t = t.replace(".pipe($.sass(sassOptions))", ".pipe(gulpSass(sassOptions))")
        write(gs, t)

gj = root / "gulp" / "scripts.js"
if gj.is_file():
    t = gj.read_text(encoding="utf-8")
    if "requiredNodeVersion + '.*'" in t:
        new_if = (
            "if (compareVersions(process.versions.node, requiredNodeVersion) < 0) {\n"
            "  console.log('\\x1b[31m%s\\x1b[0m', 'WARNING!',  'Unsupported nodejs version: ' + process.versions.node + ' found, required at least: ' + requiredNodeVersion);\n"
            "  console.log('Install NVM and type: nvm install 20 && nvm use 20');\n"
            "}\n"
        )
        # Use a callable replacement so \\x1b in the string is not parsed as a re template (Python 3.13+).
        t2, n = re.subn(
            r"if \(compareVersions\(process\.versions\.node, requiredNodeVersion\) !== 0 && "
            r"compareVersions\(process\.versions\.node, requiredNodeVersion \+ '\.\*'\) !== 0\) \{[\s\S]*?\n\}\n",
            lambda _m: new_if,
            t,
            count=1,
        )
        if n:
            write(gj, t2)

sm = root / "src" / "app" / "themes" / "volumio3" / "components" / "side-menu" / "volumio3-side-menu.scss"
if sm.is_file():
    t = sm.read_text(encoding="utf-8")
    fixes = (
        ("max-height: calc(100vh - #{$theme-player-height-smartphone};", "max-height: calc(100vh - #{$theme-player-height-smartphone});"),
        ("max-height: calc(100vh - #{$theme-player-height-tablet};", "max-height: calc(100vh - #{$theme-player-height-tablet});"),
        ("max-height: calc(100vh - #{$theme-player-height-desktop};", "max-height: calc(100vh - #{$theme-player-height-desktop});"),
    )
    changed = False
    for a, b in fixes:
        if a in t:
            t = t.replace(a, b)
            changed = True
    if changed:
        write(sm, t)
PY

}

# Evo-owned Volumio2-UI deltas: mirror paths under Volumio2-UI repo root (see layer/volumio2-ui-overlay/README.txt).
apply_volumio2_ui_overlay() {
  local overlay="${EVO_REPO_DIR}/layer/volumio2-ui-overlay"
  if [[ ! -d "${overlay}/src" ]]; then
    return 0
  fi
  echo "Applying Evo volumio2-ui overlay from ${overlay} ..."
  rsync -a "${overlay}/" "${UI_REPO_DIR}/"
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
    upgrade_ui_socketio_client
    echo "Building Volumio2-UI (${UI_THEME})..."

    # Volumio2-UI expects Node 20 (gulp 3 + npm overrides + Dart sass); use nvm so apt node is not required.
    export NVM_DIR="${NVM_DIR:-/usr/local/nvm}"
    if [[ ! -s "${NVM_DIR}/nvm.sh" ]]; then
      echo "Installing nvm..."
      mkdir -p "${NVM_DIR}"
      curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | \
        PROFILE=/dev/null bash
    fi
    # shellcheck source=/dev/null
    . "${NVM_DIR}/nvm.sh"
    nvm install 20
    nvm use 20

    normalize_volumio2_ui_checkout
    apply_volumio2_ui_overlay

    # Wipe deps and lockfile: stale node-sass / phantom trees cause ENOTEMPTY and arm64 postinstall failures.
    chmod -R u+w "${UI_REPO_DIR}/node_modules" 2>/dev/null || true
    rm -rf "${UI_REPO_DIR}/node_modules"
    rm -f "${UI_REPO_DIR}/package-lock.json"

    # Non-interactive for any tool that might prompt (e.g. npx).
    export NPM_CONFIG_YES=true

    # --ignore-scripts: skip any leftover native postinstall (e.g. phantomjs-prebuilt) on arm64; gulp build does not need them.
    (cd "${UI_REPO_DIR}" && npm install --no-audit --no-fund --ignore-scripts)
    if [[ -x "${UI_REPO_DIR}/node_modules/.bin/bower" ]]; then
      (cd "${UI_REPO_DIR}" && ./node_modules/.bin/bower install --allow-root --config.interactive=false)
    else
      (cd "${UI_REPO_DIR}" && npx --yes bower@1.8.14 install --allow-root --config.interactive=false)
    fi
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

  # Remove leftover v4 injection from previous bootstrap runs.
  local idx="${UI_DIST_DIR}/index.html"
  sed -i 's|<script src="/scripts/socket.io-4.js"></script>||g' "${idx}" 2>/dev/null || true
  rm -f "${UI_DIST_DIR}/scripts/socket.io-4.js"
}

upgrade_ui_socketio_client() {
  # Replace socket.io-client 2.3.1 (protocol v3) with v4 (protocol v5)
  # in the UI source BEFORE building, so the vendor bundle includes v4 natively.
  local sio_dir="${UI_REPO_DIR}/src/app/lib/socket"
  local sio_v4="${sio_dir}/socket.io-4.7.5.js"

  if [[ ! -d "${sio_dir}" ]]; then
    echo "WARN: ${sio_dir} not found, skipping socket.io upgrade."
    return
  fi

  if [[ ! -f "${sio_v4}" ]]; then
    echo "Downloading socket.io v4 client into UI source..."
    curl -fsSL "https://cdn.socket.io/4.7.5/socket.io.min.js" -o "${sio_v4}" || {
      echo "ERROR: failed to download socket.io v4 client."
      exit 1
    }
  fi

  # Update index.html to reference v4 instead of v2
  local idx="${UI_REPO_DIR}/src/index.html"
  if [[ -f "${idx}" ]]; then
    sed -i 's|socket\.io-2\.3\.1\.js|socket.io-4.7.5.js|g' "${idx}"
    sed -i 's|socket\.io-1\.[0-9.]*\.js|socket.io-4.7.5.js|g' "${idx}"
    echo "Updated UI source index.html to use socket.io v4 client."
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
      clone_or_update_repo "${UI_REPO_URL}" "${UI_REPO_DIR}" "${UI_REPO_BRANCH}" "1" || \
        echo "WARN: failed to update local Volumio2-UI source, continuing with current checkout."
    fi
  else
    clone_or_update_repo "${UI_REPO_URL}" "${UI_REPO_DIR}" "${UI_REPO_BRANCH}" "1"
  fi
  configure_mpd
  build_and_install_evo
  build_ui
  ensure_nginx_access
  configure_nginx
  validate_stack
}

main "$@"
