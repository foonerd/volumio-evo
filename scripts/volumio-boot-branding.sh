#!/bin/bash
# Install and enable Volumio Evo Plymouth boot branding (packages, theme, cmdline, initramfs, vol-branding units).
# Run as root:
#   sudo EVO_REPO_DIR=/path/to/volumio-evo ./scripts/volumio-boot-branding.sh
# Optional: PLYMOUTH_ROTATION=0|90|180|270 (kernel cmdline plymouth=N for volumio-adaptive asset rotation).
#
# Progress for Evo UI: lines "::BRANDING <pct> <message>"
set -euo pipefail

EVO_REPO_DIR="${EVO_REPO_DIR:-}"
PLYMOUTH_ROTATION="${PLYMOUTH_ROTATION:-0}"

pct() {
  echo "::BRANDING $1 $2"
}

die() {
  pct 0 "ERROR: $*"
  exit 1
}

[[ "$(id -u)" == "0" ]] || die "must run as root"

[[ -n "${EVO_REPO_DIR}" && -d "${EVO_REPO_DIR}/layer/plymouth/volumio-adaptive" ]] || \
  die "set EVO_REPO_DIR to a volumio-evo checkout containing layer/plymouth/volumio-adaptive"

case "${PLYMOUTH_ROTATION}" in
  0|90|180|270) ;;
  *) die "PLYMOUTH_ROTATION must be 0, 90, 180, or 270" ;;
esac

THEME_SRC="${EVO_REPO_DIR}/layer/plymouth/volumio-adaptive"
SYS_UNITS_SRC="${EVO_REPO_DIR}/layer/systemd"

pct 3 "Installing Debian packages (plymouth)…"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y plymouth plymouth-themes

pct 12 "Installing volumio-adaptive theme…"
install -d /usr/share/plymouth/themes
rm -rf /usr/share/plymouth/themes/volumio-adaptive
cp -a "${THEME_SRC}" /usr/share/plymouth/themes/volumio-adaptive

pct 22 "Setting default Plymouth theme…"
if command -v plymouth-set-default-theme >/dev/null 2>&1; then
  plymouth-set-default-theme volumio-adaptive --rebuild-initrd 2>/dev/null || \
  plymouth-set-default-theme volumio-adaptive -R 2>/dev/null || true
fi
install -d /etc/plymouth
if [[ ! -f /etc/plymouth/plymouthd.conf ]] || ! grep -q '^Theme=volumio-adaptive' /etc/plymouth/plymouthd.conf 2>/dev/null; then
  cat > /etc/plymouth/plymouthd.conf <<'EOF'
[Daemon]
Theme=volumio-adaptive
ShowDelay=0
EOF
fi

pct 28 "Installing VOL branding systemd units…"
# Remove superseded vol-milestone-v1-* names (same VOL tokens; renamed to vol-branding-v1-*).
if [[ -d /etc/systemd/system ]]; then
  for old in vol-milestones-v1.target vol-milestone-v1-fs-root-ready.service vol-milestone-v1-sys-base.service \
             vol-milestone-v1-fs-data.service vol-milestone-v1-app-starting.service vol-milestone-v1-app-listening.service; do
    systemctl disable --now "${old}" 2>/dev/null || true
    rm -f "/etc/systemd/system/${old}"
  done
fi

for u in vol-branding-v1-fs-root-ready.service vol-branding-v1-sys-base.service \
         vol-branding-v1-fs-data.service vol-branding-v1-app-starting.service \
         vol-branding-v1-app-listening.service vol-branding-v1.target; do
  [[ -f "${SYS_UNITS_SRC}/${u}" ]] || die "missing ${SYS_UNITS_SRC}/${u}"
  install -m 0644 "${SYS_UNITS_SRC}/${u}" "/etc/systemd/system/${u}"
done
systemctl daemon-reload
systemctl enable vol-branding-v1.target

pct 38 "Updating kernel command line…"

patch_oneline_cmdline() {
  local f="$1"
  [[ -f "${f}" ]] || return 0
  [[ -w "${f}" ]] || return 0
  sed -i 's/[[:space:]]\{1,\}/ /g;s/^[[:space:]]*//;s/[[:space:]]*$//' "${f}"
  sed -i 's/[[:space:]]\+plymouth=[^[:space:]]*//g' "${f}"
  local line
  line="$(cat "${f}" | tr '\n' ' ' | sed 's/[[:space:]]\{1,\}/ /g')"
  [[ "${line}" == *"splash"* ]] || line="${line} splash"
  [[ "${line}" == *"plymouth.ignore-serial-consoles"* ]] || line="${line} plymouth.ignore-serial-consoles"
  line="${line} plymouth=${PLYMOUTH_ROTATION}"
  echo "${line}" > "${f}"
}

if [[ -f /boot/firmware/cmdline.txt ]]; then
  pct 42 "Configuring /boot/firmware/cmdline.txt (Raspberry Pi)…"
  patch_oneline_cmdline /boot/firmware/cmdline.txt
elif [[ -d /etc/default/grub.d ]]; then
  pct 42 "Configuring GRUB (/etc/default/grub.d/50-volumio-evo-plymouth.cfg)…"
  install -d /etc/default/grub.d
  cat > /etc/default/grub.d/50-volumio-evo-plymouth.cfg <<EOF
# Volumio Evo — Plymouth splash (sourced by grub)
EXTRA_PLY="splash plymouth.ignore-serial-consoles plymouth=${PLYMOUTH_ROTATION}"
GRUB_CMDLINE_LINUX_DEFAULT="\$(echo "\${GRUB_CMDLINE_LINUX_DEFAULT}" | sed 's/[[:space:]]*plymouth=[^[:space:]]*//g')"
for tok in \$EXTRA_PLY; do
  [[ " \$GRUB_CMDLINE_LINUX_DEFAULT " != *" \$tok "* ]] && GRUB_CMDLINE_LINUX_DEFAULT="\$GRUB_CMDLINE_LINUX_DEFAULT \$tok"
done
EOF
  chmod 0644 /etc/default/grub.d/50-volumio-evo-plymouth.cfg
  if command -v update-grub >/dev/null 2>&1; then
    update-grub
  fi
else
  pct 42 "WARN: No Pi cmdline.txt or /etc/default/grub.d — add splash plymouth.ignore-serial-consoles plymouth=${PLYMOUTH_ROTATION} manually."
fi

pct 55 "Rebuilding initramfs…"
if command -v update-initramfs >/dev/null 2>&1; then
  update-initramfs -u || die "update-initramfs failed"
fi

pct 92 "Reloading systemd…"
systemctl daemon-reload

pct 100 "Boot branding enabled. Reboot to apply."
exit 0
