#!/usr/bin/env bash
# volumio-evo: kiosk layer installer.
#
# Called by scripts/bootstrap-volumio-evo-player.sh when --with-kiosk=wpe
# (or KIOSK=wpe) is set. May also be invoked directly after a full
# bootstrap. Idempotent: re-running refreshes packages and redeploys
# unit / helper files.
#
# Design: docs/KIOSK.md; runtime overlays: layer/kiosk-wpe/README.md.
# Stack: labwc (Wayland compositor) + volumio-evo-kiosk-browser (GTK4 +
# webkit2gtk 6.0 kiosk shell). cog / WPE / cage were evaluated and
# rejected - see README.md for the full rationale. The repo path is
# still named kiosk-wpe for historical continuity; --with-kiosk=wpe is
# the bootstrap flag.
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

  # Core stack:
  #   labwc         - wlroots-based stacking compositor (layer-shell, xdg-shell)
  #   python3-gi    - GObject-Introspection bindings for Python (kiosk browser)
  #   gir1.2-gtk-4.0       - GTK 4 bindings   (kiosk browser)
  #   gir1.2-webkit-6.0    - WebKit 6 bindings (kiosk browser, pulls webkit2gtk)
  #   bubblewrap + xdg-dbus-proxy - required by the webkit2gtk sandbox.
  #                 The unit currently sets WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
  #                 (see systemd/volumio-evo-kiosk.service for why) so these
  #                 are not on the critical path today, but installing them
  #                 ahead of time means flipping the sandbox env later is
  #                 just an env-var change with no apt churn.
  #   squeekboard   - on-screen keyboard (text-input-v3 driven)
  #   wvkbd         - fallback on-screen keyboard
  #   wtype         - virtual-keyboard-v1 client used by the session script
  #                   to fire the HideCursor keybind when cursor=hide
  #   xkb-data      - XKB layouts (wlroots reads WLR_XKB_LAYOUT at start)
  #   fonts-*       - base CJK-free font set
  #   libinput-tools - libinput debug-events etc. for field diagnostics
  #   xdg-user-dirs - standard user dirs (profile cache path resolution)
  #   gstreamer*    - WebKit media backend
  local -a pkgs=(
    labwc
    python3-gi
    gir1.2-gtk-4.0
    gir1.2-webkit-6.0
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
      # Match arm64/armhf GLES/DRM baseline; mesa pulls these transitively but
      # explicit installs avoid silent breakage if Debian ever splits deps.
      pkgs+=(libgl1-mesa-dri libgles2 libegl1 libdrm2)
      gpu="$(detect_amd64_gpu_vendor)"
      case "${gpu}" in
        intel)
          # Best-effort: prefer intel-media on Gen8+, fall back to i965 shaders.
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

  # Hard-required core: the kiosk cannot function without labwc or the
  # Python + WebKit bindings the browser depends on.
  local has_labwc has_pygi has_gtk4 has_webkit6
  has_labwc=0
  has_pygi=0
  has_gtk4=0
  has_webkit6=0
  local q
  for q in "${resolved[@]}"; do
    [[ "${q}" == "labwc" ]] && has_labwc=1
    [[ "${q}" == "python3-gi" ]] && has_pygi=1
    [[ "${q}" == "gir1.2-gtk-4.0" ]] && has_gtk4=1
    [[ "${q}" == "gir1.2-webkit-6.0" ]] && has_webkit6=1
  done
  if [[ "${has_labwc}" != "1" ]]; then
    fail "Required package 'labwc' not available from apt. On Ubuntu enable 'universe'; on Debian use main. See docs/KIOSK.md."
  fi
  if [[ "${has_pygi}" != "1" || "${has_gtk4}" != "1" || "${has_webkit6}" != "1" ]]; then
    fail "Required kiosk-browser runtime missing (python3-gi, gir1.2-gtk-4.0, gir1.2-webkit-6.0). Install from Debian Trixie main / Ubuntu main. See docs/KIOSK.md."
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

install_helper_scripts() {
  local bin
  for bin in volumio-evo-kiosk-preflight volumio-evo-kiosk-launch volumio-evo-kiosk-session volumio-evo-kiosk-browser volumio-evo-kiosk-autorotate; do
    local src="${SCRIPT_DIR}/bin/${bin}"
    local dst="/usr/local/bin/${bin}"
    [[ -f "${src}" ]] || fail "Missing helper ${src}"
    install -m 0755 "${src}" "${dst}"
    log "Installed ${dst}"
  done
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
  # labwc configuration lives in its own dir to avoid collision with any
  # /etc/xdg/labwc that other packages might drop. The launcher invokes
  # labwc with --config-dir /etc/volumio-evo/labwc so only this file is
  # used. Contents: server-side decorations off, F24 bound to HideCursor
  # (the session script fires F24 via wtype when cursor=hide). No
  # windowRule for fullscreen is set - the kiosk browser requests
  # xdg_toplevel.set_maximized so layer-shell surfaces (squeekboard OSK)
  # render above it.
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
