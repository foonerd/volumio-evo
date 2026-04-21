#!/usr/bin/env bash
# volumio-evo: kiosk layer installer.
#
# Called by scripts/bootstrap-volumio-evo-player.sh when --with-kiosk=wpe
# (or KIOSK=wpe) is set. May also be invoked directly after a full
# bootstrap. Idempotent: re-running refreshes packages and redeploys
# unit / helper files.
#
# Design: docs/KIOSK.md; runtime overlays: layer/kiosk-wpe/README.md.
# Stack: labwc (Wayland compositor) + volumio-evo-kiosk-browser (Rust
# binary built from crates/kiosk-browser, linking GTK 4 + webkit2gtk
# 6.0 + libsoup 3). cog / WPE / cage were evaluated and rejected - see
# README.md for the full rationale. The repo path is still named
# kiosk-wpe for historical continuity; --with-kiosk=wpe is the
# bootstrap flag.
#
# NEVER enables the kiosk unit here - the backend switch drives state
# (crates/core/src/kiosk.rs + Settings -> System -> Kiosk).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# Service user resolution mirrors bootstrap-volumio-evo-player.sh. The kiosk
# runs as the same user as volumio-evo.service by policy.
KIOSK_SERVICE_USER="${KIOSK_SERVICE_USER:-${EVO_SERVICE_USER-}}"
KIOSK_ALLOW_ROOT="${KIOSK_ALLOW_ROOT:-0}"
# 0: install files, don't enable unit (backend toggle decides). 1: force enable.
KIOSK_FORCE_ENABLE="${KIOSK_FORCE_ENABLE:-0}"
# Default on. Set 0 to skip the apt install stage (re-running installer).
KIOSK_INSTALL_PACKAGES="${KIOSK_INSTALL_PACKAGES:-1}"
# Default on. Set 0 to skip the cargo build of the Rust kiosk-browser
# binary (e.g. running the installer on a host that already has it
# installed). When 1, a prebuilt at layer/binaries/<triple>/ is
# preferred; missing prebuilt falls back to cargo build if toolchain
# and source are present.
KIOSK_BUILD_BROWSER="${KIOSK_BUILD_BROWSER:-1}"

need_root() {
  if [[ "$(id -u)" -ne 0 ]]; then
    echo "ERROR: run as root (sudo)." >&2
    exit 1
  fi
}

log() {
  printf '[kiosk-wpe] %s\n' "$*"
}

warn() {
  printf '[kiosk-wpe] WARN: %s\n' "$*" >&2
}

fail() {
  printf '[kiosk-wpe] ERROR: %s\n' "$*" >&2
  exit 1
}

# Resolve service user the same way bootstrap does: SUDO_USER, else non-root
# USER, else logname. Empty means "root" and requires KIOSK_ALLOW_ROOT=1.
resolve_kiosk_user() {
  if [[ -n "${KIOSK_SERVICE_USER}" ]]; then
    echo "${KIOSK_SERVICE_USER}"
    return
  fi
  local candidate=""
  if [[ -n "${SUDO_USER:-}" ]]; then
    candidate="${SUDO_USER}"
  elif [[ -n "${USER:-}" && "${USER}" != "root" ]]; then
    candidate="${USER}"
  else
    candidate="$(logname 2>/dev/null || true)"
  fi
  if [[ -z "${candidate}" || "${candidate}" == "root" ]]; then
    echo ""
  else
    echo "${candidate}"
  fi
}

detect_distro() {
  if [[ -r /etc/os-release ]]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    echo "${ID:-unknown} ${VERSION_ID:-unknown}"
  else
    echo "unknown unknown"
  fi
}

detect_arch() {
  if command -v dpkg >/dev/null 2>&1; then
    dpkg --print-architecture
  else
    case "$(uname -m)" in
      aarch64) echo "arm64" ;;
      armv7l|armv6l) echo "armhf" ;;
      x86_64|amd64) echo "amd64" ;;
      *) echo "unknown" ;;
    esac
  fi
}

host_rust_triple() {
  case "$(uname -m)" in
    aarch64) echo "aarch64-unknown-linux-gnu" ;;
    armv7l|armv6l|armv5tel) echo "armv7-unknown-linux-gnueabihf" ;;
    x86_64|amd64) echo "x86_64-unknown-linux-gnu" ;;
    *) echo "" ;;
  esac
}

# Return "intel", "amd", or "" based on lspci vendor ID of the first VGA class.
detect_amd64_gpu_vendor() {
  if ! command -v lspci >/dev/null 2>&1; then
    echo ""
    return
  fi
  local line
  line="$(lspci -nn 2>/dev/null | grep -E 'VGA compatible controller|3D controller' | head -n1 || true)"
  case "${line}" in
    *"[8086:"*) echo "intel" ;;
    *"[1002:"*|*"[1022:"*) echo "amd" ;;
    *) echo "" ;;
  esac
}

# Check /sys/bus/iio/devices/iio:device* for any `in_accel_*` attribute.
detect_accelerometer() {
  local d f
  for d in /sys/bus/iio/devices/iio:device*; do
    [[ -d "${d}" ]] || continue
    for f in "${d}"/in_accel_x_raw "${d}"/in_accel_scale; do
      if [[ -e "${f}" ]]; then
        echo "yes"
        return
      fi
    done
  done
  echo "no"
}

# True if apt knows this binary package name (index has a candidate).
pkg_in_apt_index() {
  local p="$1"
  [[ -n "${p}" ]] || return 1
  apt-cache show "${p}" >/dev/null 2>&1
}

# Keep only names that exist in the configured apt sources (avoids one missing
# package failing the whole apt-get install line).
filter_apt_packages() {
  local -n _out="$1"
  shift
  local p
  _out=()
  for p in "$@"; do
    if pkg_in_apt_index "${p}"; then
      _out+=("${p}")
    else
      warn "Package '${p}' not in apt index - skipping (add repo or see docs/KIOSK.md)."
    fi
  done
}

install_packages() {
  if [[ "${KIOSK_INSTALL_PACKAGES}" != "1" ]]; then
    log "Skipping apt install (KIOSK_INSTALL_PACKAGES=0)."
    return 0
  fi
  local arch gpu accel
  arch="$(detect_arch)"
  accel="$(detect_accelerometer)"
  log "Arch: ${arch}; accelerometer present: ${accel}"

  # Runtime + build stack for the Rust kiosk-browser:
  #   labwc                  - wlroots stacking compositor (layer-shell, xdg-shell)
  #   wlr-randr              - used by session script to apply output scale
  #   libgtk-4-1             - GTK 4 runtime
  #   libwebkitgtk-6.0-4 or  - WebKit 6 / webkit2gtk 6.0 runtime (Debian uses -4;
  #   libwebkitgtk-6.0-1       Ubuntu often -1 — filter picks whichever exists)
  #   libsoup-3.0-0          - HTTP stack used by WebKit 6
  #   libgtk-4-dev           - build-time headers for crates/kiosk-browser
  #   libwebkitgtk-6.0-dev   - build-time headers for crates/kiosk-browser
  #   libsoup-3.0-dev        - build-time headers for crates/kiosk-browser
  #   pkg-config             - cargo finds the three libs above via pkg-config
  #   bubblewrap +           - required by the webkit2gtk sandbox when enabled
  #     xdg-dbus-proxy         (unit currently sets WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
  #                            - see systemd/volumio-evo-kiosk.service; install them
  #                            anyway so flipping the sandbox env is a pure
  #                            config change without apt churn)
  #   squeekboard            - on-screen keyboard (text-input-v3 driven)
  #   wvkbd                  - fallback on-screen keyboard
  #   wtype                  - virtual-keyboard-v1 client for HideCursor bind
  #   xkb-data               - XKB layouts (wlroots reads WLR_XKB_LAYOUT)
  #   fonts-*                - base CJK-free font set
  #   libinput-tools         - `libinput debug-events` for field diagnostics
  #   xdg-user-dirs          - standard user dirs (profile cache path)
  #   gstreamer*             - WebKit media backend
  local -a pkgs=(
    labwc
    wlr-randr
    libgtk-4-1
    libwebkitgtk-6.0-4
    libwebkitgtk-6.0-1
    libsoup-3.0-0
    libgtk-4-dev
    libwebkitgtk-6.0-dev
    libsoup-3.0-dev
    pkg-config
    bubblewrap
    xdg-dbus-proxy
    squeekboard
    wvkbd
    wtype
    xkb-data
    fonts-dejavu-core
    fonts-noto-core
    libinput-tools
    xdg-user-dirs
    gstreamer1.0-plugins-base
    gstreamer1.0-plugins-good
    gstreamer1.0-plugins-bad
    gstreamer1.0-libav
    gstreamer1.0-gl
  )

  case "${arch}" in
    arm64|armhf)
      # libgles2-mesa dropped in Mesa 23.1.4-1 (Aug 2023); Trixie uses libgles2 (GLVND).
      pkgs+=(libgl1-mesa-dri libgles2 libegl1 libdrm2)
      ;;
    amd64)
      pkgs+=(libgl1-mesa-dri libgles2 libegl1 libdrm2)
      gpu="$(detect_amd64_gpu_vendor)"
      case "${gpu}" in
        intel)
          if pkg_in_apt_index intel-media-va-driver; then
            pkgs+=(intel-media-va-driver)
          fi
          if pkg_in_apt_index i965-va-driver-shaders; then
            pkgs+=(i965-va-driver-shaders)
          fi
          ;;
        amd)
          pkgs+=(mesa-va-drivers)
          ;;
        *)
          pkgs+=(mesa-va-drivers)
          ;;
      esac
      ;;
    *)
      warn "Unsupported arch '${arch}'; installing common packages only."
      ;;
  esac

  if [[ "${accel}" == "yes" ]]; then
    pkgs+=(iio-sensor-proxy)
  fi

  export DEBIAN_FRONTEND=noninteractive
  export APT_LISTCHANGES_FRONTEND=none

  apt-get update

  # Heal half-configured/broken dpkg state before a large install (common
  # on interrupted apt runs or minimal images).
  dpkg --configure -a 2>/dev/null || true
  apt-get -y -f install 2>/dev/null || true

  local -a resolved=()
  filter_apt_packages resolved "${pkgs[@]}"

  # Hard-required: the kiosk cannot function without labwc or the WebKit
  # runtime + headers the browser binary links against.
  local has_labwc=0 has_gtk4_rt=0 has_webkit_rt=0 has_gtk4_dev=0 has_webkit_dev=0 has_pkgc=0
  local q
  for q in "${resolved[@]}"; do
    [[ "${q}" == "labwc" ]] && has_labwc=1
    [[ "${q}" == "libgtk-4-1" ]] && has_gtk4_rt=1
    [[ "${q}" == "libwebkitgtk-6.0-1" || "${q}" == "libwebkitgtk-6.0-4" ]] && has_webkit_rt=1
    [[ "${q}" == "libgtk-4-dev" ]] && has_gtk4_dev=1
    [[ "${q}" == "libwebkitgtk-6.0-dev" ]] && has_webkit_dev=1
    [[ "${q}" == "pkg-config" ]] && has_pkgc=1
  done
  if [[ "${has_labwc}" != "1" ]]; then
    fail "Required package 'labwc' not available from apt. Enable Debian main / Ubuntu universe. See docs/KIOSK.md."
  fi
  if [[ "${has_gtk4_rt}" != "1" || "${has_webkit_rt}" != "1" ]]; then
    fail "Required runtime libraries missing (libgtk-4-1, libwebkitgtk-6.0-4 or libwebkitgtk-6.0-1). Install from Debian main / security. See docs/KIOSK.md."
  fi
  if [[ "${has_gtk4_dev}" != "1" || "${has_webkit_dev}" != "1" || "${has_pkgc}" != "1" ]]; then
    fail "Required build headers missing (libgtk-4-dev, libwebkitgtk-6.0-dev, pkg-config). Needed to compile crates/kiosk-browser."
  fi

  log "Installing ${#resolved[@]} package(s) (filtered to index-available names)."
  apt-get install -y --no-install-recommends "${resolved[@]}"

  apt-get -y -f install

  if ! printf '%s\n' "${resolved[@]}" | grep -qx 'squeekboard'; then
    warn "squeekboard was not installed (not in apt index). Use on-screen keyboard 'wvkbd' in Settings or install squeekboard manually."
  fi
}

ensure_kiosk_user_groups() {
  local u="$1"
  if [[ -z "${u}" ]]; then
    return 0
  fi
  if ! id -u "${u}" >/dev/null 2>&1; then
    fail "Resolved kiosk user '${u}' does not exist."
  fi
  local grp
  for grp in video input render seat plugdev audio; do
    if getent group "${grp}" >/dev/null 2>&1; then
      usermod -aG "${grp}" "${u}" 2>/dev/null || true
    fi
  done
  log "User ${u} added to groups: video input render seat plugdev audio (where present)."
}

install_unit_files() {
  local src_unit="${SCRIPT_DIR}/systemd/volumio-evo-kiosk.service"
  local src_auto="${SCRIPT_DIR}/systemd/volumio-evo-kiosk-autorotate.service"
  local dst_unit="/etc/systemd/system/volumio-evo-kiosk.service"
  local dst_auto="/etc/systemd/system/volumio-evo-kiosk-autorotate.service"

  [[ -f "${src_unit}" ]] || fail "Missing source unit ${src_unit}"
  [[ -f "${src_auto}" ]] || fail "Missing source unit ${src_auto}"

  install -m 0644 "${src_unit}" "${dst_unit}"
  install -m 0644 "${src_auto}" "${dst_auto}"
  log "Installed ${dst_unit}"
  log "Installed ${dst_auto}"
}

install_unit_user_drop_in() {
  local u="$1"
  local drop_dir="/etc/systemd/system/volumio-evo-kiosk.service.d"
  local drop_in="${drop_dir}/10-user.conf"
  local auto_drop_dir="/etc/systemd/system/volumio-evo-kiosk-autorotate.service.d"
  local auto_drop_in="${auto_drop_dir}/10-user.conf"
  if [[ -z "${u}" ]]; then
    rm -f "${drop_in}" "${auto_drop_in}" 2>/dev/null || true
    rmdir "${drop_dir}" "${auto_drop_dir}" 2>/dev/null || true
    return 0
  fi
  local g
  g="$(id -gn "${u}")"
  mkdir -p "${drop_dir}" "${auto_drop_dir}"
  cat > "${drop_in}" <<EOF
# Generated by layer/kiosk-wpe/install.sh - runs kiosk as the Evo service user.
[Service]
User=${u}
Group=${g}
Environment=HOME=$(getent passwd "${u}" | cut -d: -f6)
EOF
  cat > "${auto_drop_in}" <<EOF
# Generated by layer/kiosk-wpe/install.sh - same user as the kiosk itself.
[Service]
User=${u}
Group=${g}
Environment=HOME=$(getent passwd "${u}" | cut -d: -f6)
EOF
  log "Installed ${drop_in}"
  log "Installed ${auto_drop_in}"
}

# Shell helpers (preflight / launch / session / autorotate). The browser
# is NOT in this list; it is a compiled binary installed by
# install_kiosk_browser_binary() below.
install_helper_scripts() {
  local bin
  for bin in volumio-evo-kiosk-preflight volumio-evo-kiosk-launch volumio-evo-kiosk-session volumio-evo-kiosk-autorotate; do
    local src="${SCRIPT_DIR}/bin/${bin}"
    local dst="/usr/local/bin/${bin}"
    [[ -f "${src}" ]] || fail "Missing helper ${src}"
    install -m 0755 "${src}" "${dst}"
    log "Installed ${dst}"
  done
}

# Build and install the Rust kiosk-browser binary.
#
# Resolution order:
#   1. Prebuilt at ${REPO_DIR}/layer/binaries/${triple}/volumio-evo-kiosk-browser
#   2. cargo build -p volumio-evo-kiosk-browser --release (uses the workspace
#      Cargo.toml at ${REPO_DIR}/Cargo.toml with its default-members
#      exclusion; the -p flag targets the kiosk crate explicitly, so
#      building the kiosk does not pull the whole backend along with it).
#
# Either path leaves the final binary at /usr/local/bin/volumio-evo-kiosk-browser.
install_kiosk_browser_binary() {
  local dst="/usr/local/bin/volumio-evo-kiosk-browser"
  if [[ "${KIOSK_BUILD_BROWSER}" != "1" ]]; then
    if [[ ! -x "${dst}" ]]; then
      fail "KIOSK_BUILD_BROWSER=0 but ${dst} is missing. Provide the binary or re-run with KIOSK_BUILD_BROWSER=1."
    fi
    log "Reusing existing ${dst} (KIOSK_BUILD_BROWSER=0)."
    return 0
  fi

  local triple
  triple="$(host_rust_triple)"
  local prebuilt=""
  if [[ -n "${triple}" ]]; then
    prebuilt="${REPO_DIR}/layer/binaries/${triple}/volumio-evo-kiosk-browser"
  fi

  if [[ -n "${prebuilt}" && -x "${prebuilt}" ]]; then
    log "Installing prebuilt kiosk-browser from ${prebuilt}"
    install -m 0755 "${prebuilt}" "${dst}"
    return 0
  fi

  # Fall back to cargo build. Needs a cargo in PATH and source in repo.
  local cargo_bin=""
  if command -v cargo >/dev/null 2>&1; then
    cargo_bin="$(command -v cargo)"
  elif [[ -x /usr/local/cargo/bin/cargo ]]; then
    cargo_bin="/usr/local/cargo/bin/cargo"
  fi
  if [[ -z "${cargo_bin}" ]]; then
    fail "No prebuilt at ${prebuilt:-<none>} and cargo not found. Install Rust (bootstrap --build installs rustup) or provide layer/binaries/<triple>/volumio-evo-kiosk-browser."
  fi
  if [[ ! -f "${REPO_DIR}/crates/kiosk-browser/Cargo.toml" ]]; then
    fail "crates/kiosk-browser/Cargo.toml missing in ${REPO_DIR}; kiosk crate not in this checkout."
  fi
  log "Building kiosk-browser from source: ${cargo_bin} build -p volumio-evo-kiosk-browser --release"
  (
    cd "${REPO_DIR}"
    export RUSTUP_HOME="${RUSTUP_HOME:-/usr/local/rustup}"
    export CARGO_HOME="${CARGO_HOME:-/usr/local/cargo}"
    export PATH="${CARGO_HOME}/bin:${PATH}"
    "${cargo_bin}" build -p volumio-evo-kiosk-browser --release
  )
  local built="${REPO_DIR}/target/release/volumio-evo-kiosk-browser"
  if [[ ! -x "${built}" ]]; then
    fail "cargo build succeeded but ${built} is missing."
  fi
  install -m 0755 "${built}" "${dst}"
  log "Installed ${dst}"
}

seed_etc_kiosk_toml() {
  local src="${SCRIPT_DIR}/etc/kiosk.toml.example"
  local dst="/etc/volumio-evo/kiosk.toml"
  mkdir -p /etc/volumio-evo
  if [[ -f "${dst}" ]]; then
    log "Keeping existing ${dst}"
  else
    install -m 0644 "${src}" "${dst}"
    log "Seeded ${dst}"
  fi
}

install_labwc_config() {
  local src="${SCRIPT_DIR}/etc/labwc/rc.xml"
  local dir="/etc/volumio-evo/labwc"
  local dst="${dir}/rc.xml"
  [[ -f "${src}" ]] || fail "Missing labwc config ${src}"
  mkdir -p "${dir}"
  install -m 0644 "${src}" "${dst}"
  log "Installed ${dst}"
}

ensure_settings_dir() {
  local u="$1"
  local dir="/var/lib/volumio-evo/settings/kiosk"
  mkdir -p "${dir}"
  if [[ -n "${u}" ]]; then
    local g
    g="$(id -gn "${u}")"
    chown -R "${u}:${g}" "${dir}" 2>/dev/null || true
  fi
  log "Ensured ${dir}"
}

drm_present() {
  compgen -G '/dev/dri/card*' >/dev/null
}

print_status() {
  local u="$1"
  local distro arch
  distro="$(detect_distro)"
  arch="$(detect_arch)"
  log "Summary:"
  log "  distro          : ${distro}"
  log "  arch            : ${arch}"
  log "  service user    : ${u:-<root>}"
  if drm_present; then
    log "  DRM device      : yes (/dev/dri/card*)"
  else
    log "  DRM device      : no (kiosk install complete but not started; toggle requires hardware)"
  fi
  if [[ -n "${u}" ]]; then
    log "  sudoers         : /etc/sudoers.d/volumio-evo-kiosk-control managed by bootstrap"
  else
    log "  sudoers         : not installed (root service user; enable via KIOSK_ALLOW_ROOT=1 and re-run)"
  fi
  log "  browser binary  : /usr/local/bin/volumio-evo-kiosk-browser (Rust, from crates/kiosk-browser)"
  log "  backend toggle  : Settings -> System -> Kiosk (enable/disable)"
  log "  logs            : journalctl -u volumio-evo-kiosk -n 100 --no-pager"
}

main() {
  need_root

  local distro arch user
  distro="$(detect_distro)"
  arch="$(detect_arch)"
  user="$(resolve_kiosk_user)"

  log "Starting kiosk install on ${distro} (${arch})"

  if [[ -z "${user}" && "${KIOSK_ALLOW_ROOT}" != "1" ]]; then
    warn "No non-root service user resolved. Kiosk will be installed but"
    warn "the backend toggle will refuse to enable the unit. To override,"
    warn "re-run with KIOSK_ALLOW_ROOT=1."
  fi

  install_packages
  ensure_kiosk_user_groups "${user}"
  install_unit_files
  install_unit_user_drop_in "${user}"
  install_helper_scripts
  install_kiosk_browser_binary
  seed_etc_kiosk_toml
  install_labwc_config
  ensure_settings_dir "${user}"

  systemctl daemon-reload

  if ! drm_present; then
    warn "No DRM device (/dev/dri/card*) found. Kiosk files installed but"
    warn "the unit will not be started. Connect a display and flip the"
    warn "toggle in Settings -> System -> Kiosk."
  fi

  if [[ "${KIOSK_FORCE_ENABLE}" == "1" && -n "${user}" ]]; then
    if drm_present; then
      log "KIOSK_FORCE_ENABLE=1: enabling and starting volumio-evo-kiosk.service"
      systemctl enable volumio-evo-kiosk.service
      systemctl start  volumio-evo-kiosk.service
    else
      warn "KIOSK_FORCE_ENABLE=1 but no DRM device; not starting."
    fi
  fi

  print_status "${user}"
  log "Install complete."
}

main "$@"
