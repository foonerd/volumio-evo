#!/usr/bin/env bash
# Narrow root-only operations for SMB named users (see docs/OS_PRIVILEGE_MODEL.md).
# Invoked as: sudo -n /usr/local/bin/volumio-evo-smb-user-sync.sh <add|delete> <username>
# add: password on first line of stdin.

set -euo pipefail

ACTION="${1:-}"
USERN="${2:-}"

die() {
  echo "volumio-evo-smb-user-sync: $*" >&2
  exit 1
}

[[ -n "${USERN}" ]] || die "missing username"

case "${ACTION}" in
  add)
    [[ "${USERN}" =~ ^[a-z_][a-z0-9_-]{0,31}$ ]] || die "invalid username — use [a-z_][a-z0-9_-]{0,31}"
    IFS= read -r PASS || true
    [[ -n "${PASS}" ]] || die "empty password on stdin"
    if ! id -u "${USERN}" &>/dev/null; then
      NOLOGIN="$(command -v nologin 2>/dev/null || true)"
      [[ -z "${NOLOGIN}" || -x "${NOLOGIN}" ]] || NOLOGIN="/usr/sbin/nologin"
      useradd -r -s "${NOLOGIN}" -d /nonexistent "${USERN}"
    fi
    printf '%s\n%s\n' "${PASS}" "${PASS}" | smbpasswd -a -s "${USERN}"
    ;;
  delete)
    [[ "${USERN}" =~ ^[a-z_][a-z0-9_-]{0,31}$ ]] || die "invalid username"
    smbpasswd -x "${USERN}" 2>/dev/null || true
    userdel "${USERN}" 2>/dev/null || true
    ;;
  *)
    die "usage: $0 <add|delete> <username> (password on stdin for add)"
    ;;
esac
