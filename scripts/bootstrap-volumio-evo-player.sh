#!/usr/bin/env bash
set -euo pipefail

# CANONICAL FULL-STACK INSTALL (on device): this script only.
# Re-run this script; by default it installs the prebuilt backend from layer/binaries/<triple>/
# (no rustup). Pass --build or EVO_BUILD_FROM_SOURCE=1 to compile on device (rustup + cargo).
# Copies static UI from layer/web/, configures MPD/systemd/nginx. MPD: sets music_directory,
# idempotently appends include_optional for EVO_MPD_FRAGMENT (default /etc/volumio-evo/mpd.conf).
# Installs ALSA JSON under
# /usr/share/volumio-evo/alsa/ (dacs.json, cards.json). Set EVO_REPO_UPDATE=0 only
# for offline or pinned checkouts.
#
# One-shot tester install for Debian / Raspberry Pi OS Lite. Run as root.
#
# OS privilege contract (service user, sudoers for mount + mpd restart, fragment ownership):
#   docs/OS_PRIVILEGE_MODEL.md — Evo must not require interactive auth in the service path.
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
# After systemctl restart, the HTTP server may not accept connections immediately; validate_backend_only retries.
EVO_BACKEND_WAIT_SECS="${EVO_BACKEND_WAIT_SECS:-60}"
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
# Evo-owned MPD fragment; main /etc/mpd.conf must include this (bootstrap adds include_optional if missing).
EVO_MPD_FRAGMENT="${EVO_MPD_FRAGMENT:-/etc/volumio-evo/mpd.conf}"
EVO_BINARY_PATH="${EVO_BINARY_PATH:-/usr/local/bin/volumio-evo}"
# Optional: run volumio-evo.service as a normal login user (never hardcode uid 1000).
# Default: the login for this session — SUDO_USER when using sudo, else a non-root USER, else logname(1).
# Do not set EVO_SERVICE_USER here: unset means "auto", empty means "force root" (see resolve_evo_service_user).
# EVO_SERVICE_USER_USE_SUDO_INVOKER is deprecated (ignored); auto behavior matches the old "invoker=1" case.
# 1: install /etc/sudoers.d/volumio-evo-mount with NOPASSWD for /usr/bin/mount,/usr/bin/umount (future NAS UI).
EVO_INSTALL_MOUNT_SUDOERS="${EVO_INSTALL_MOUNT_SUDOERS:-1}"
# 1: install /etc/sudoers.d/volumio-evo-mpd so non-root Evo can `sudo -n systemctl restart mpd` after fragment writes.
EVO_INSTALL_MPD_SUDOERS="${EVO_INSTALL_MPD_SUDOERS:-1}"
EVO_INSTALL_RFKILL_SUDOERS="${EVO_INSTALL_RFKILL_SUDOERS:-1}"
# 1: apt install CIFS/NFS/SMB client packages (cifs-utils, nfs-common, smbclient) for NAS/SMB mounts and Sources UI.
EVO_INSTALL_NETWORK_STORAGE_PKGS="${EVO_INSTALL_NETWORK_STORAGE_PKGS:-1}"
# 1: apt install network-manager (nmcli) for Evo NetworkManager integration (see docs/NETWORK_NM.md).
EVO_INSTALL_NETWORK_MANAGER="${EVO_INSTALL_NETWORK_MANAGER:-1}"
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
  --upgrade-evo       Clone/pull repo, stop service, replace binary, refresh dacs.json, restart (no UI/nginx/mpd).
  --upgrade-nginx     Refresh dacs.json, re-read [ui] active_layout, rewrite nginx, reload (alias: --apply-ui-only).

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
  EVO_BACKEND_WAIT_SECS=60   # max wait for /api/health after systemctl restart (slow Pi / first start)
  UI_DIST_OVERRIDE=
  UI_ROOT_MANIFEST=/srv/volumio-ui-manifest
  UI_ROOT_CONTEMPORARY=/srv/volumio-ui
  UI_ROOT_CLASSIC=/srv/volumio-ui-classic
  MUSIC_ROOT=/var/lib/volumio-evo/music
  EVO_MPD_FRAGMENT=/etc/volumio-evo/mpd.conf   # Evo-owned MPD snippet; bootstrap ensures include_optional in /etc/mpd.conf
  EVO_BINARY_PATH=/usr/local/bin/volumio-evo
  EVO_BUILD_FROM_SOURCE=0   # or 1 to force cargo like --build
  EVO_SERVICE_USER=          # omit: use session user (SUDO_USER / USER / logname); empty: run service as root
  EVO_INSTALL_MOUNT_SUDOERS=1           # 0 to skip sudoers drop-in for mount/umount
  EVO_INSTALL_MPD_SUDOERS=1             # 0 to skip sudoers for systemctl restart mpd (non-root service)
  EVO_INSTALL_RFKILL_SUDOERS=1          # 0 to skip sudoers for sudo -n rfkill unblock wifi (Wi-Fi soft block)
  EVO_INSTALL_NMCLI_SUDOERS=1           # 0 to skip sudoers for sudo -n nmcli (NetworkManager; non-root service)
  EVO_INSTALL_IW_SUDOERS=1              # 0 to skip sudoers for sudo -n iw (AP vif + phy capability; non-root service)
  EVO_INSTALL_HOSTNAME_TIMEDATE_SUDOERS=1  # 0 to skip sudoers for sudo -n hostnamectl/timedatectl (Settings → System)
  EVO_INSTALL_RTCWAKE_SUDOERS=1            # 0 to skip sudoers for sudo -n rtcwake (RTC wake from suspend; see docs/ALARM_WAKE.md)
  EVO_INSTALL_CONFIG_INSTALL_SUDOERS=1  # 0 to skip sudoers for sudo -n install (merge preferred wifi_iface → /etc/volumio-evo/config.toml)
  EVO_INSTALL_NETWORK_STORAGE_PKGS=1    # 0 to skip cifs-utils nfs-common smbclient avahi-utils
  EVO_INSTALL_NETWORK_MANAGER=1         # 0 to skip network-manager (nmcli); Evo network stack uses NM

Example:
  sudo BASE_DIR=/opt/volumio ./scripts/bootstrap-volumio-evo-player.sh
  sudo ./scripts/bootstrap-volumio-evo-player.sh --upgrade-evo
  sudo ./scripts/bootstrap-volumio-evo-player.sh --full --build   # compile on device instead of layer/binaries

  Run Evo as your SSH/sudo user (default when EVO_SERVICE_USER is unset):
  sudo ./scripts/bootstrap-volumio-evo-player.sh
  # or explicit:
  sudo EVO_SERVICE_USER=andrew ./scripts/bootstrap-volumio-evo-player.sh
  # force root service user:
  sudo EVO_SERVICE_USER= ./scripts/bootstrap-volumio-evo-player.sh
EOF
}

# Resolve which Unix account should run volumio-evo (empty => root, no User= drop-in).
# If EVO_SERVICE_USER is unset: use session login (sudo -> SUDO_USER, else non-root USER, else logname).
# If set (including empty): use that value; empty means explicit root.
resolve_evo_service_user() {
  local candidate
  if [[ -v EVO_SERVICE_USER ]]; then
    echo "${EVO_SERVICE_USER}"
    return
  fi
  candidate=""
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

# Systemd User=/Group=/HOME=, ownership of /var/lib/volumio-evo, audio group, optional sudoers for mount helpers.
configure_evo_runtime_user() {
  local u g home
  u="$(resolve_evo_service_user)"
  local drop_dir="/etc/systemd/system/volumio-evo.service.d"
  local drop_in="${drop_dir}/10-runtime-user.conf"
  local sudoers="/etc/sudoers.d/volumio-evo-mount"
  local mpd_sudoers="/etc/sudoers.d/volumio-evo-mpd"
  local rfkill_sudoers="/etc/sudoers.d/volumio-evo-rfkill"
  local nmcli_sudoers="/etc/sudoers.d/volumio-evo-nmcli"
  local iw_sudoers="/etc/sudoers.d/volumio-evo-iw"
  local hostname_timedate_sudoers="/etc/sudoers.d/volumio-evo-hostname-timedate"
  local rtcwake_sudoers="/etc/sudoers.d/volumio-evo-rtcwake"

  if [[ -z "${u}" ]]; then
    echo "Evo service user: (none) — volumio-evo runs as root (default)."
    rm -f "${drop_in}"
    rmdir "${drop_dir}" 2>/dev/null || true
    rm -f "${sudoers}" 2>/dev/null || true
    rm -f "${mpd_sudoers}" 2>/dev/null || true
    rm -f "${rfkill_sudoers}" 2>/dev/null || true
    rm -f "${nmcli_sudoers}" 2>/dev/null || true
    rm -f "${iw_sudoers}" 2>/dev/null || true
    rm -f "${hostname_timedate_sudoers}" 2>/dev/null || true
    rm -f "${rtcwake_sudoers}" 2>/dev/null || true
    rm -f "/etc/sudoers.d/volumio-evo-config-install" 2>/dev/null || true
    return 0
  fi

  # Must match what we pass to sudoers for `systemctl restart mpd` (see EVO_INSTALL_MPD_SUDOERS).
  local systemctl_bin
  systemctl_bin="$(command -v systemctl 2>/dev/null || true)"
  if [[ -z "${systemctl_bin}" || ! -x "${systemctl_bin}" ]]; then
    systemctl_bin="/usr/bin/systemctl"
  fi

  # Must match sudoers for `rfkill unblock wifi` and Environment=VOLUMIO_EVO_RFKILL (see EVO_INSTALL_RFKILL_SUDOERS).
  local rfkill_bin
  rfkill_bin="$(command -v rfkill 2>/dev/null || true)"
  if [[ -z "${rfkill_bin}" || ! -x "${rfkill_bin}" ]]; then
    rfkill_bin="/usr/sbin/rfkill"
  fi

  # Must match sudoers for nmcli and Environment=VOLUMIO_EVO_NMCLI (see EVO_INSTALL_NMCLI_SUDOERS).
  local nmcli_bin
  nmcli_bin="$(command -v nmcli 2>/dev/null || true)"
  if [[ -z "${nmcli_bin}" || ! -x "${nmcli_bin}" ]]; then
    nmcli_bin="/usr/bin/nmcli"
  fi

  # Must match sudoers for iw and Environment=VOLUMIO_EVO_IW (see EVO_INSTALL_IW_SUDOERS).
  # Needed for: phy capability probe (`iw phy info`, `iw dev`) and virtual AP vif management
  # (`iw dev <sta> interface add <ap> type __ap` / `iw dev <ap> del`) on single-PHY AP+STA chips.
  local iw_bin
  iw_bin="$(command -v iw 2>/dev/null || true)"
  if [[ -z "${iw_bin}" || ! -x "${iw_bin}" ]]; then
    iw_bin="/usr/sbin/iw"
  fi

  local hostnamectl_bin
  hostnamectl_bin="$(command -v hostnamectl 2>/dev/null || true)"
  if [[ -z "${hostnamectl_bin}" || ! -x "${hostnamectl_bin}" ]]; then
    hostnamectl_bin="/usr/bin/hostnamectl"
  fi

  local timedatectl_bin
  timedatectl_bin="$(command -v timedatectl 2>/dev/null || true)"
  if [[ -z "${timedatectl_bin}" || ! -x "${timedatectl_bin}" ]]; then
    timedatectl_bin="/usr/bin/timedatectl"
  fi

  # rtcwake from util-linux — RTC alarm for wake-from-suspend (alarm clock); must match sudoers / VOLUMIO_EVO_RTCWAKE.
  local rtcwake_bin
  rtcwake_bin="$(command -v rtcwake 2>/dev/null || true)"
  if [[ -z "${rtcwake_bin}" || ! -x "${rtcwake_bin}" ]]; then
    rtcwake_bin="/usr/sbin/rtcwake"
  fi

  if ! id -u "${u}" >/dev/null 2>&1; then
    echo "ERROR: EVO_SERVICE_USER='${u}' is not a valid login on this system. Create the user or fix EVO_SERVICE_USER."
    exit 1
  fi
  g="$(id -gn "${u}")"
  home="$(getent passwd "${u}" | cut -d: -f6)"
  if [[ -z "${home}" || ! -d "${home}" ]]; then
    echo "WARN: home for ${u} missing; systemd may still work with User=."
  fi

  echo "Evo service user: ${u} (group ${g}) — configuring systemd drop-in and data ownership."

  mkdir -p "${drop_dir}"
  cat > "${drop_in}" <<EOF
# Generated by bootstrap-volumio-evo-player.sh — do not hardcode uid; uses login name ${u}
[Service]
User=${u}
Group=${g}
SupplementaryGroups=audio
Environment=HOME=${home}
Environment=VOLUMIO_EVO_RUNTIME_USER=${u}
Environment=VOLUMIO_EVO_SYSTEMCTL=${systemctl_bin}
Environment=VOLUMIO_EVO_RFKILL=${rfkill_bin}
Environment=VOLUMIO_EVO_NMCLI=${nmcli_bin}
Environment=VOLUMIO_EVO_IW=${iw_bin}
Environment=VOLUMIO_EVO_HOSTNAMECTL=${hostnamectl_bin}
Environment=VOLUMIO_EVO_TIMEDATECTL=${timedatectl_bin}
Environment=VOLUMIO_EVO_RTCWAKE=${rtcwake_bin}
EOF

  usermod -aG audio "${u}" 2>/dev/null || true

  chown -R "${u}:${g}" /var/lib/volumio-evo
  chown -R "${u}:${g}" "${MUSIC_ROOT}" 2>/dev/null || true
  # NAS mount points (network drives UI): Evo creates /mnt/NAS/<alias> before sudo mount
  mkdir -p /mnt/NAS
  chown -R "${u}:${g}" /mnt/NAS 2>/dev/null || true
  # Plugin drops and read-only assets: user can add .wasm without sudo if desired
  chown -R "${u}:${g}" /usr/share/volumio-evo/plugins 2>/dev/null || true
  # MPD include snippet: Evo must rewrite this when mixer type / playback output changes (non-root service).
  if [[ -f "${EVO_MPD_FRAGMENT}" ]]; then
    chown "${u}:${g}" "${EVO_MPD_FRAGMENT}"
    echo "MPD fragment ownership: ${u}:${g} ${EVO_MPD_FRAGMENT}"
  fi

  if [[ "${EVO_INSTALL_MOUNT_SUDOERS:-1}" == "1" ]]; then
    local tmp
    tmp="$(mktemp)"
    cat > "${tmp}" <<EOF
# volumio-evo: allow runtime user to run mount/umount for future NAS/CIFS wiring (narrow paths only).
# Managed by bootstrap; remove if you use a different privilege model.
${u} ALL=(root) NOPASSWD: /usr/bin/mount, /usr/bin/umount, /bin/umount
EOF
    if command -v visudo >/dev/null 2>&1 && visudo -cf "${tmp}" 2>/dev/null; then
      install -m 0440 "${tmp}" "${sudoers}"
      echo "Installed ${sudoers} (mount/umount NOPASSWD for ${u})."
    else
      echo "WARN: visudo check failed or visudo missing; not installing ${sudoers}. Install sudoers manually if needed."
    fi
    rm -f "${tmp}"
  else
    rm -f "${sudoers}" 2>/dev/null || true
  fi

  if [[ "${EVO_INSTALL_MPD_SUDOERS:-1}" == "1" ]]; then
    local tmp_mpd
    tmp_mpd="$(mktemp)"
    cat > "${tmp_mpd}" <<EOF
# volumio-evo: allow runtime user to reload MPD after editing the Evo fragment (Playback Options).
# Managed by bootstrap; must match Environment=VOLUMIO_EVO_SYSTEMCTL in 10-runtime-user.conf.
${u} ALL=(root) NOPASSWD: ${systemctl_bin} restart mpd
EOF
    if command -v visudo >/dev/null 2>&1 && visudo -cf "${tmp_mpd}" 2>/dev/null; then
      install -m 0440 "${tmp_mpd}" "${mpd_sudoers}"
      echo "Installed ${mpd_sudoers} (systemctl restart mpd NOPASSWD for ${u})."
    else
      echo "WARN: visudo check failed or visudo missing; not installing ${mpd_sudoers}. Install sudoers manually if needed."
    fi
    rm -f "${tmp_mpd}"
  else
    rm -f "${mpd_sudoers}" 2>/dev/null || true
  fi

  if [[ "${EVO_INSTALL_RFKILL_SUDOERS:-1}" == "1" ]]; then
    local tmp_rf
    tmp_rf="$(mktemp)"
    cat > "${tmp_rf}" <<EOF
# volumio-evo: unblock Wi-Fi when soft-blocked (rfkill) before nmcli scan; narrow command only.
# Managed by bootstrap; must match Environment=VOLUMIO_EVO_RFKILL in 10-runtime-user.conf.
${u} ALL=(root) NOPASSWD: ${rfkill_bin} unblock wifi
EOF
    if command -v visudo >/dev/null 2>&1 && visudo -cf "${tmp_rf}" 2>/dev/null; then
      install -m 0440 "${tmp_rf}" "${rfkill_sudoers}"
      echo "Installed ${rfkill_sudoers} (rfkill unblock wifi NOPASSWD for ${u})."
    else
      echo "WARN: visudo check failed or visudo missing; not installing ${rfkill_sudoers}. Install sudoers manually if needed."
    fi
    rm -f "${tmp_rf}"
  else
    rm -f "${rfkill_sudoers}" 2>/dev/null || true
  fi

  if [[ "${EVO_INSTALL_NMCLI_SUDOERS:-1}" == "1" ]]; then
    local tmp_nm
    tmp_nm="$(mktemp)"
    cat > "${tmp_nm}" <<EOF
# volumio-evo: NetworkManager (nmcli) for apply/scan when service runs non-root.
# Managed by bootstrap; must match Environment=VOLUMIO_EVO_NMCLI in 10-runtime-user.conf.
${u} ALL=(root) NOPASSWD: ${nmcli_bin}
EOF
    if command -v visudo >/dev/null 2>&1 && visudo -cf "${tmp_nm}" 2>/dev/null; then
      install -m 0440 "${tmp_nm}" "${nmcli_sudoers}"
      echo "Installed ${nmcli_sudoers} (nmcli NOPASSWD for ${u})."
    else
      echo "WARN: visudo check failed or visudo missing; not installing ${nmcli_sudoers}. Install sudoers manually if needed."
    fi
    rm -f "${tmp_nm}"
  else
    rm -f "${nmcli_sudoers}" 2>/dev/null || true
  fi

  # `iw` NOPASSWD for phy capability + virtual AP vif lifecycle (single-PHY STA+AP; see docs/NETWORK_NM.md).
  if [[ "${EVO_INSTALL_IW_SUDOERS:-1}" == "1" ]]; then
    local tmp_iw
    tmp_iw="$(mktemp)"
    cat > "${tmp_iw}" <<EOF
# volumio-evo: iw for 'phy info', 'dev', and virtual AP vif add/del on single-PHY AP+STA chips.
# Managed by bootstrap; must match Environment=VOLUMIO_EVO_IW in 10-runtime-user.conf.
${u} ALL=(root) NOPASSWD: ${iw_bin}
EOF
    if command -v visudo >/dev/null 2>&1 && visudo -cf "${tmp_iw}" 2>/dev/null; then
      install -m 0440 "${tmp_iw}" "${iw_sudoers}"
      echo "Installed ${iw_sudoers} (iw NOPASSWD for ${u})."
    else
      echo "WARN: visudo check failed or visudo missing; not installing ${iw_sudoers}. Install sudoers manually if needed."
    fi
    rm -f "${tmp_iw}"
  else
    rm -f "${iw_sudoers}" 2>/dev/null || true
  fi

  # hostnamectl / timedatectl for Settings → System (non-root service hits polkit without NOPASSWD sudo).
  if [[ "${EVO_INSTALL_HOSTNAME_TIMEDATE_SUDOERS:-1}" == "1" ]]; then
    local tmp_ht
    tmp_ht="$(mktemp)"
    cat > "${tmp_ht}" <<EOF
# volumio-evo: hostname and timezone apply (Settings → System) when service runs non-root.
# Managed by bootstrap; must match Environment=VOLUMIO_EVO_HOSTNAMECTL / VOLUMIO_EVO_TIMEDATECTL.
${u} ALL=(root) NOPASSWD: ${hostnamectl_bin} set-hostname *
${u} ALL=(root) NOPASSWD: ${timedatectl_bin} set-timezone *
EOF
    if command -v visudo >/dev/null 2>&1 && visudo -cf "${tmp_ht}" 2>/dev/null; then
      install -m 0440 "${tmp_ht}" "${hostname_timedate_sudoers}"
      echo "Installed ${hostname_timedate_sudoers} (hostnamectl/timedatectl NOPASSWD for ${u})."
    else
      echo "WARN: visudo check failed or visudo missing; not installing ${hostname_timedate_sudoers}."
    fi
    rm -f "${tmp_ht}"
  else
    rm -f "${hostname_timedate_sudoers}" 2>/dev/null || true
  fi

  # rtcwake — program/clear RTC alarm (wake from suspend); full binary NOPASSWD like nmcli (see docs/ALARM_WAKE.md).
  if [[ "${EVO_INSTALL_RTCWAKE_SUDOERS:-1}" == "1" ]]; then
    local tmp_rw
    tmp_rw="$(mktemp)"
    cat > "${tmp_rw}" <<EOF
# volumio-evo: rtcwake for alarm wake-from-suspend (non-root Evo). Package: util-linux.
# Managed by bootstrap; must match Environment=VOLUMIO_EVO_RTCWAKE in 10-runtime-user.conf.
${u} ALL=(root) NOPASSWD: ${rtcwake_bin}
EOF
    if command -v visudo >/dev/null 2>&1 && visudo -cf "${tmp_rw}" 2>/dev/null; then
      install -m 0440 "${tmp_rw}" "${rtcwake_sudoers}"
      echo "Installed ${rtcwake_sudoers} (rtcwake NOPASSWD for ${u})."
    else
      echo "WARN: visudo check failed or visudo missing; not installing ${rtcwake_sudoers}."
    fi
    rm -f "${tmp_rw}"
  else
    rm -f "${rtcwake_sudoers}" 2>/dev/null || true
  fi

  # Narrow NOPASSWD: install merged config from pending path → /etc/volumio-evo/config.toml (preferred Wi-Fi iface UI).
  local config_install_sudoers="/etc/sudoers.d/volumio-evo-config-install"
  local pending_cfg="/var/lib/volumio-evo/settings/network/config.toml.pending"
  if [[ "${EVO_INSTALL_CONFIG_INSTALL_SUDOERS:-1}" == "1" ]]; then
    local tmp_ci
    tmp_ci="$(mktemp)"
    cat > "${tmp_ci}" <<EOF
# volumio-evo: non-root service merges \`wifi_iface\` into system config (must match Rust \`config_toml_pending_path\`).
# Managed by bootstrap; paths are fixed.
${u} ALL=(root) NOPASSWD: /usr/bin/install -o root -g root -m 644 ${pending_cfg} /etc/volumio-evo/config.toml
EOF
    if command -v visudo >/dev/null 2>&1 && visudo -cf "${tmp_ci}" 2>/dev/null; then
      install -m 0440 "${tmp_ci}" "${config_install_sudoers}"
      echo "Installed ${config_install_sudoers} (install config.toml NOPASSWD for ${u})."
    else
      echo "WARN: visudo check failed or visudo missing; not installing ${config_install_sudoers}."
    fi
    rm -f "${tmp_ci}"
  else
    rm -f "${config_install_sudoers}" 2>/dev/null || true
  fi
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

# Evo default when RUST_LOG is unset; older installs lack this key.
ensure_config_has_log_level() {
  local cfg="/etc/volumio-evo/config.toml"
  [[ -f "${cfg}" ]] || return 0
  grep -qE '^[[:space:]]*log_level[[:space:]]*=' "${cfg}" && return 0
  if grep -qE '^[[:space:]]*bind[[:space:]]*=' "${cfg}"; then
    sed -i '/^[[:space:]]*bind[[:space:]]*=/i log_level = "info"' "${cfg}"
  else
    sed -i '1i log_level = "info"' "${cfg}"
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
  local -a net_pkgs=()
  if [[ "${EVO_INSTALL_NETWORK_STORAGE_PKGS:-1}" == "1" ]]; then
    net_pkgs+=(cifs-utils nfs-common smbclient avahi-utils)
    echo "Installing network storage packages: cifs-utils nfs-common smbclient avahi-utils"
  fi
  local -a nm_pkgs=()
  if [[ "${EVO_INSTALL_NETWORK_MANAGER:-1}" == "1" ]]; then
    # `iw`: phy capability probe + virtual AP vif (single-PHY STA+AP). `rfkill`: Wi-Fi soft-block.
    nm_pkgs+=(network-manager rfkill iw)
    echo "Installing NetworkManager (nmcli): network-manager rfkill iw"
  fi
  apt-get update
  apt-get install -y \
    git curl ca-certificates nginx mpd python3 acl \
    build-essential pkg-config libssl-dev \
    rsync \
    libimage-exiftool-perl \
    "${net_pkgs[@]}" \
    "${nm_pkgs[@]}"
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

# True if /etc/mpd.conf already references EVO_MPD_FRAGMENT on an include / include_optional line (non-comment).
mpd_main_config_includes_evo_fragment() {
  local main="${1:-/etc/mpd.conf}"
  [[ -f "${main}" ]] || return 1
  local rel="${EVO_MPD_FRAGMENT#/etc/}"
  while IFS= read -r line || [[ -n "${line}" ]]; do
    [[ "${line}" =~ ^[[:space:]]*# ]] && continue
    [[ "${line}" != *"${EVO_MPD_FRAGMENT}"* && "${line}" != *"${rel}"* ]] && continue
    [[ "${line}" =~ ^[[:space:]]*include(_optional)?[[:space:]] ]] && return 0
  done < "${main}"
  return 1
}

# Ensure stock /etc/mpd.conf loads EVO_MPD_FRAGMENT (idempotent). Does not remove other includes (e.g. mpd_local.conf).
ensure_mpd_conf_includes_evo_fragment() {
  local main="/etc/mpd.conf"
  [[ -f "${main}" ]] || {
    echo "WARN: ${main} missing; install mpd or create config before relying on playback."
    return 0
  }
  if mpd_main_config_includes_evo_fragment "${main}"; then
    echo "MPD: ${main} already includes ${EVO_MPD_FRAGMENT}"
    return 0
  fi
  cat >> "${main}" <<EOF

# volumio-evo: playback output fragment (bootstrap + backend). Edit ${EVO_MPD_FRAGMENT}, not this line.
include_optional "${EVO_MPD_FRAGMENT}"
EOF
  echo "MPD: appended include_optional \"${EVO_MPD_FRAGMENT}\" to ${main}"
}

# Seed a minimal fragment once so include_optional succeeds before Evo writes ALSA/MPD pipeline on save.
ensure_default_evo_mpd_fragment() {
  mkdir -p "$(dirname "${EVO_MPD_FRAGMENT}")"
  if [[ -f "${EVO_MPD_FRAGMENT}" ]]; then
    echo "MPD: keeping existing ${EVO_MPD_FRAGMENT}"
    return 0
  fi
  cat > "${EVO_MPD_FRAGMENT}" <<'EOF'
# Managed by volumio-evo (bootstrap default; Evo may replace on Playback Options save).
# Evo uses pcm "volumio" for MPD only when `aplay -L` lists it; otherwise direct hw (see mpd_playback_device).
# If you also define audio_output in /etc/mpd_local.conf, remove one copy to avoid duplicate MPD outputs.

audio_output {
	type		"alsa"
	name		"volumio-evo"
	device		"default"
	mixer_type	"software"
}
EOF
  chmod 0644 "${EVO_MPD_FRAGMENT}"
  echo "MPD: wrote default ${EVO_MPD_FRAGMENT}"
}

configure_mpd() {
  mkdir -p "${MUSIC_ROOT}"/{INTERNAL,USB,NAS,SMB}
  if [[ -f /etc/mpd.conf ]]; then
    # Stock Debian/Raspberry Pi OS often ships only #music_directory (commented). Replace in place; avoid a duplicate at EOF.
    if grep -qE '^[[:space:]]*music_directory[[:space:]]' /etc/mpd.conf; then
      sed -i 's|^[[:space:]]*music_directory.*|music_directory "'"${MUSIC_ROOT}"'"|' /etc/mpd.conf
    elif grep -qE '^[[:space:]]*#[[:space:]]*music_directory[[:space:]]' /etc/mpd.conf; then
      sed -i 's|^[[:space:]]*#[[:space:]]*music_directory.*|music_directory "'"${MUSIC_ROOT}"'"|' /etc/mpd.conf
    else
      echo 'music_directory "'"${MUSIC_ROOT}"'"' >> /etc/mpd.conf
    fi
  else
    echo "WARN: /etc/mpd.conf not found; skipping music_directory and MPD include (install mpd first)."
  fi
  ensure_mpd_conf_includes_evo_fragment
  ensure_default_evo_mpd_fragment
  if [[ -f /etc/mpd_local.conf ]] && grep -E -q '^[[:space:]]*audio_output' /etc/mpd_local.conf 2>/dev/null; then
    echo "WARN: /etc/mpd_local.conf defines audio_output; Evo uses ${EVO_MPD_FRAGMENT}. Remove duplicate audio_output from mpd_local.conf to avoid two ALSA outputs."
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

# Stock Evo ships the same assets as Node's miscellanea/albumart (SVG/PNG under plugins/), not Font Awesome.
# GET /albumart?icon=music serves plugins/icons/music.svg. Install from repo when present; else minimal SVGs only.
# I2S DAC catalogue (same source as stock Volumio). Installed to /usr/share/volumio-evo/alsa/dacs.json.
# Always overwrites the install path on success so every bootstrap mode keeps the device in sync with the tree.
install_dacs_catalog() {
  mkdir -p /usr/share/volumio-evo/alsa
  local evo="${EVO_REPO_DIR}/layer/config/alsa/dacs.json"
  local script="${SCRIPT_REPO_DIR}/layer/config/alsa/dacs.json"
  local src=""
  # Prefer the git checkout (EVO_REPO_DIR); use the script tree only if its copy is strictly newer (local edit).
  if [[ -f "${evo}" && -f "${script}" ]]; then
    if [[ "${script}" -nt "${evo}" ]]; then
      src="${script}"
    else
      src="${evo}"
    fi
  elif [[ -f "${evo}" ]]; then
    src="${evo}"
  elif [[ -f "${script}" ]]; then
    src="${script}"
  fi
  if [[ -n "${src}" ]]; then
    echo "Updating I2S DAC catalogue (always): ${src} -> /usr/share/volumio-evo/alsa/dacs.json"
    cp -f "${src}" /usr/share/volumio-evo/alsa/dacs.json
    chmod 644 /usr/share/volumio-evo/alsa/dacs.json
    return 0
  fi
  echo "WARN: layer/config/alsa/dacs.json not found under EVO_REPO_DIR or script repo; I2S DAC list in Playback Options will be empty."
  echo "      Copy dacs.json to /usr/share/volumio-evo/alsa/dacs.json or set VOLUMIO_EVO_DACS_JSON in systemd."
}

# ALSA card name → pretty labels + I2S detection (Node `alsa_controller/cards.json` → cards.json).
install_alsa_cards_json() {
  mkdir -p /usr/share/volumio-evo/alsa
  local evo="${EVO_REPO_DIR}/layer/config/alsa/cards.json"
  local script="${SCRIPT_REPO_DIR}/layer/config/alsa/cards.json"
  local src=""
  if [[ -f "${evo}" && -f "${script}" ]]; then
    if [[ "${script}" -nt "${evo}" ]]; then
      src="${script}"
    else
      src="${evo}"
    fi
  elif [[ -f "${evo}" ]]; then
    src="${evo}"
  elif [[ -f "${script}" ]]; then
    src="${script}"
  fi
  if [[ -n "${src}" ]]; then
    echo "Updating ALSA card catalogue (always): ${src} -> /usr/share/volumio-evo/alsa/cards.json"
    cp -f "${src}" /usr/share/volumio-evo/alsa/cards.json
    chmod 644 /usr/share/volumio-evo/alsa/cards.json
    return 0
  fi
  echo "WARN: layer/config/alsa/cards.json not found; output device names may stay as raw aplay strings."
}

install_bundled_plugins_assets() {
  mkdir -p /usr/share/volumio-evo/plugins
  local src="" d
  for d in \
    "${EVO_REPO_DIR}/layer/bundled-plugins" \
    "${EVO_REPO_DIR}/crates/core/assets/bundled-plugins" \
    "${SCRIPT_REPO_DIR}/layer/bundled-plugins" \
    "${SCRIPT_REPO_DIR}/crates/core/assets/bundled-plugins"; do
    if [[ -d "${d}" ]]; then
      src="${d}"
      break
    fi
  done
  if [[ -n "${src}" ]]; then
    echo "Installing bundled plugin assets from ${src} -> /usr/share/volumio-evo/plugins/"
    cp -a "${src}/." /usr/share/volumio-evo/plugins/
    return 0
  fi
  echo "WARN: No bundled-plugins tree in repo checkout; writing minimal SVG fallbacks (music; folder-o / users / dot-circle-o from layer if present)."
  mkdir -p /usr/share/volumio-evo/plugins/icons
  local src_fo=""
  for src_fo in \
    "${SCRIPT_REPO_DIR}/layer/bundled-plugins/icons/folder-o.svg" \
    "${EVO_REPO_DIR}/layer/bundled-plugins/icons/folder-o.svg"; do
    if [[ -f "${src_fo}" ]]; then
      cp -f "${src_fo}" /usr/share/volumio-evo/plugins/icons/folder-o.svg
      src_fo="copied"
      break
    fi
  done
  if [[ "${src_fo}" != "copied" ]]; then
    cat > /usr/share/volumio-evo/plugins/icons/folder-o.svg <<'EOSVG'
<?xml version="1.0" encoding="UTF-8"?>
<!-- Minimal fallback; prefer layer/bundled-plugins/icons/folder-o.svg (Node albumart asset). -->
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">
  <path fill="#FFFFFF" d="M464 128H272l-64-64H48C21.5 64 0 85.5 0 112v288c0 26.5 21.5 48 48 48h416c26.5 0 48-21.5 48-48V176c0-26.5-21.5-48-48-48zm0 272H48V112h136l64 64h216v224z"/>
</svg>
EOSVG
  fi
  local src_users=""
  for src_users in \
    "${SCRIPT_REPO_DIR}/layer/bundled-plugins/icons/users.svg" \
    "${EVO_REPO_DIR}/layer/bundled-plugins/icons/users.svg"; do
    if [[ -f "${src_users}" ]]; then
      cp -f "${src_users}" /usr/share/volumio-evo/plugins/icons/users.svg
      src_users="copied"
      break
    fi
  done
  if [[ "${src_users}" != "copied" ]]; then
    echo "WARN: users.svg missing (artist browse fallback); install full layer/bundled-plugins."
  fi
  local src_dot=""
  for src_dot in \
    "${SCRIPT_REPO_DIR}/layer/bundled-plugins/icons/dot-circle-o.svg" \
    "${EVO_REPO_DIR}/layer/bundled-plugins/icons/dot-circle-o.svg"; do
    if [[ -f "${src_dot}" ]]; then
      cp -f "${src_dot}" /usr/share/volumio-evo/plugins/icons/dot-circle-o.svg
      src_dot="copied"
      break
    fi
  done
  if [[ "${src_dot}" != "copied" ]]; then
    echo "WARN: dot-circle-o.svg missing (albums browse fallback); install full layer/bundled-plugins."
  fi
  cat > /usr/share/volumio-evo/plugins/icons/music.svg <<'EOSVG'
<?xml version="1.0" encoding="UTF-8"?>
<!-- Note glyph for /albumart?icon=music (browse song rows without embedded art yet). -->
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">
  <path fill="#c8c8c8" d="M470.4 105.6c16.8 7.2 28.8 24 28.8 43.2v256c0 38.4-34.4 67.2-72 57.6-20.8-4.8-35.2-24-35.2-45.6V188.8L192 249.6v156.8c0 38.4-34.4 67.2-72 57.6-25.6-6.4-40-28.8-40-54.4V153.6c0-25.6 14.4-48 40-54.4l249.6-62.4c10.4-2.4 20.8-.8 30.4 4.8z"/>
</svg>
EOSVG
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

  mkdir -p /etc/volumio-evo /usr/share/volumio-evo/plugins /var/lib/volumio-evo/albumart \
    /var/lib/volumio-evo/settings/alsa /var/lib/volumio-evo/settings/mpd \
    /var/lib/volumio-evo/settings/mounts /var/lib/volumio-evo/settings/favourites \
    /var/lib/volumio-evo/settings/playlist /var/lib/volumio-evo/settings/network /mnt/NAS
  install_dacs_catalog
  install_alsa_cards_json
  install_bundled_plugins_assets
  if [[ "${EVO_SOURCE_AVAILABLE}" == "1" && -f "${EVO_REPO_DIR}/layer/config/volumio-evo.toml.example" ]]; then
    if [[ ! -f /etc/volumio-evo/config.toml ]]; then
      cp "${EVO_REPO_DIR}/layer/config/volumio-evo.toml.example" /etc/volumio-evo/config.toml
    fi
  elif [[ ! -f /etc/volumio-evo/config.toml ]]; then
    cat > /etc/volumio-evo/config.toml <<EOF
log_level = "info"
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
  ensure_config_has_log_level

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
Environment=VOLUMIO_EVO_ALSA_DIR=/usr/share/volumio-evo/alsa
Environment=VOLUMIO_EVO_SETTINGS_DIR=/var/lib/volumio-evo/settings
Environment=MODULAR_ALSA_PIPELINE=true

[Install]
WantedBy=multi-user.target
EOF
  fi

  configure_evo_runtime_user

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
  # Remove superseded site names from older bootstrap attempts to avoid
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
  local i max
  max="${EVO_BACKEND_WAIT_SECS:-60}"
  for ((i = 1; i <= max; i++)); do
    if curl -fsS --connect-timeout 2 --max-time 5 \
      "http://127.0.0.1:${EVO_HTTP_PORT}/api/health" >/dev/null 2>&1 \
      && curl -fsS --connect-timeout 2 --max-time 5 \
      "http://127.0.0.1:${EVO_HTTP_PORT}/api/v1/ping" >/dev/null 2>&1; then
      echo "Backend OK."
      return 0
    fi
    if [[ "${i}" -eq 1 ]]; then
      echo "Waiting for volumio-evo on 127.0.0.1:${EVO_HTTP_PORT} (up to ${max}s)..."
    fi
    if command -v systemctl >/dev/null 2>&1 && systemctl is-failed --quiet volumio-evo 2>/dev/null; then
      echo "ERROR: volumio-evo.service is in failed state (not listening on port ${EVO_HTTP_PORT})."
      echo "  journalctl -u volumio-evo -n 50 --no-pager"
      systemctl status volumio-evo --no-pager 2>&1 || true
      journalctl -u volumio-evo -n 50 --no-pager 2>&1 || true
      exit 1
    fi
    sleep 1
  done
  echo "ERROR: Backend did not respond on 127.0.0.1:${EVO_HTTP_PORT} within ${max}s."
  echo "  systemctl status volumio-evo"
  echo "  journalctl -u volumio-evo -n 50 --no-pager"
  if command -v systemctl >/dev/null 2>&1; then
    systemctl status volumio-evo --no-pager 2>&1 || true
  fi
  exit 1
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
    ensure_config_has_log_level
    install_dacs_catalog
    install_alsa_cards_json
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
