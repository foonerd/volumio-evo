#!/bin/bash
#
# Generate Volumio Evo Plymouth overlay images (VOL:v1 token set).
#
# Lives one level above the theme dir so bootstrap copying
# layer/plymouth/volumio-adaptive/ to the running system does not ship
# this dev tool. Theme dir is the generation target (BASE_DIR below).
#
# 19 tokens x 4 rotations x 2 sizes = 152 PNGs
# Output basenames: overlay-vol-<domain>-<code>{,-compact}.png
# Placed directly in sequence{0,90,180,270}/ alongside logo animations.
#
# Per-message text color (default white, warning yellow) via COLORS array.
# Font, pointsize, and dimensions match the legacy volumio-os generator
# so overlay sprite geometry in volumio-adaptive.script is preserved.
#
# Dependencies: imagemagick (convert), bc, bash 4+ (associative arrays).
#
# Usage:
#   ./generate-overlays.sh              # generate only
#   ./generate-overlays.sh --prune      # remove legacy overlay-*.png then generate
#   ./generate-overlays.sh --prune-only # remove legacy only, do not generate
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
THEME_DIR="$SCRIPT_DIR/volumio-adaptive"
BASE_DIR="$THEME_DIR"

# --------------------------------- VOL:v1 REGISTRY -------------------------------
# Key format: vol-<domain>-<code>  (becomes overlay-<key>{,-compact}.png)
# Keep in sync with GetOverlayFilename() in volumio-adaptive.script and the
# token table in layer/plymouth/README.md.

MESSAGE_ORDER=(
  vol-initrd-early
  vol-fs-checking
  vol-fs-root-ready
  vol-fs-data
  vol-sys-base
  vol-app-starting
  vol-app-listening
  vol-ui-ready
  vol-net-link
  vol-net-configured
  vol-maint-resize-start
  vol-maint-resize-done
  vol-maint-first-boot
  vol-maint-usb-update-start
  vol-maint-usb-update-done
  vol-maint-factory-reset
  vol-maint-network-recovery
  vol-diag-degraded
  vol-diag-fail
)

declare -A MESSAGES
MESSAGES["vol-initrd-early"]="Starting Volumio"
MESSAGES["vol-fs-checking"]="Checking storage"
MESSAGES["vol-fs-root-ready"]="Root filesystem ready"
MESSAGES["vol-fs-data"]="Data volume ready"
MESSAGES["vol-sys-base"]="Preparing system services"
MESSAGES["vol-app-starting"]="Starting Volumio Evo"
MESSAGES["vol-app-listening"]="Volumio Evo is running"
MESSAGES["vol-ui-ready"]="Ready to play"
MESSAGES["vol-net-link"]="Network link up"
MESSAGES["vol-net-configured"]="Network configured"
MESSAGES["vol-maint-resize-start"]="Expanding storage - please wait"
MESSAGES["vol-maint-resize-done"]="Storage expansion complete"
MESSAGES["vol-maint-first-boot"]="First boot setup in progress"
MESSAGES["vol-maint-usb-update-start"]="Receiving update from USB - DO NOT POWER OFF"
MESSAGES["vol-maint-usb-update-done"]="USB update complete - restarting"
MESSAGES["vol-maint-factory-reset"]="Factory reset in progress - DO NOT POWER OFF"
MESSAGES["vol-maint-network-recovery"]="Network recovery in progress"
MESSAGES["vol-diag-degraded"]="System degraded - continuing"
MESSAGES["vol-diag-fail"]="System error - check logs"

# Per-message text color (default white)
declare -A COLORS
COLORS["vol-maint-resize-start"]="yellow"
COLORS["vol-maint-resize-done"]="yellow"
COLORS["vol-maint-first-boot"]="yellow"
COLORS["vol-maint-usb-update-start"]="yellow"
COLORS["vol-maint-usb-update-done"]="yellow"
COLORS["vol-maint-factory-reset"]="yellow"
COLORS["vol-maint-network-recovery"]="yellow"
COLORS["vol-diag-degraded"]="yellow"
COLORS["vol-diag-fail"]="yellow"

# Legacy (pre-VOL) basenames removed by --prune.
# Node-OS English overlays replaced by overlay-vol-* set above.
LEGACY_BASENAMES=(
  overlay-expanding-storage
  overlay-factory-reset
  overlay-finishing-storage
  overlay-internal-update
  overlay-performing-update
  overlay-player-prepared
  overlay-player-preparing
  overlay-player-restarting
  overlay-receiving-update
  overlay-remove-usb
  overlay-success-restart
  overlay-system-ready
  overlay-system-warning
  overlay-update-complete
  overlay-waiting-usb
)

# ---------------------------------- ARG PARSING ----------------------------------

DO_PRUNE=0
DO_GENERATE=1
for arg in "$@"; do
  case "$arg" in
    --prune)      DO_PRUNE=1 ;;
    --prune-only) DO_PRUNE=1; DO_GENERATE=0 ;;
    -h|--help)    sed -n '1,34p' "$0"; exit 0 ;;
    *)            echo "Unknown argument: $arg" >&2; exit 2 ;;
  esac
done

# ------------------------------ THEME DIR + SEQ CHECK ----------------------------

if [ ! -d "$THEME_DIR" ]; then
  echo "Error: theme dir $THEME_DIR not found" >&2
  exit 1
fi

for seq in 0 90 180 270; do
  if [ ! -d "$BASE_DIR/sequence${seq}" ]; then
    echo "Error: $BASE_DIR/sequence${seq} not found" >&2
    exit 1
  fi
done

# -------------------------------- PRUNE LEGACY -----------------------------------

prune_legacy() {
  local removed=0
  local f
  for seq in 0 90 180 270; do
    for base in "${LEGACY_BASENAMES[@]}"; do
      for variant in "" "-compact"; do
        f="$BASE_DIR/sequence${seq}/${base}${variant}.png"
        if [ -f "$f" ]; then
          rm -f "$f"
          removed=$((removed + 1))
        fi
      done
    done
  done
  echo "Pruned legacy PNGs: $removed removed"
}

# ----------------------------- CREATE SINGLE OVERLAY -----------------------------

create_overlay() {
  local sequence=$1
  local size=$2
  local message_id=$3
  local message_text="${MESSAGES[$message_id]}"
  local fill_color="${COLORS[$message_id]:-white}"
  local output_file="$BASE_DIR/sequence${sequence}/overlay-${message_id}${size}.png"

  # Font size: full vs compact
  if [ "$size" = "" ]; then
    FONT_SIZE=16
  else
    FONT_SIZE=12
  fi

  # Approximate text width (pt * 0.6 * chars) + margin
  text_length=${#message_text}
  text_width_needed=$(echo "$FONT_SIZE * 0.6 * $text_length" | bc)
  text_width_needed=${text_width_needed%.*}
  text_width_with_margin=$((text_width_needed + 40))

  # Dimensions per rotation (match legacy geometry; .script centers overlay
  # on the framebuffer so exact size is flexible, but parity avoids visual
  # regression against the upstream composition).
  case "$sequence" in
    0|180)
      if [ $text_width_with_margin -gt 480 ]; then
        WIDTH=$text_width_with_margin
      else
        WIDTH=480
      fi
      if [ "$size" = "" ]; then
        HEIGHT=380
      else
        HEIGHT=322
      fi
      ;;
    90|270)
      if [ $text_width_with_margin -gt 480 ]; then
        HEIGHT=$text_width_with_margin
      else
        HEIGHT=480
      fi
      if [ "$size" = "" ]; then
        WIDTH=380
      else
        WIDTH=320
      fi
      ;;
  esac

  convert -size ${WIDTH}x${HEIGHT} xc:none /tmp/base_$$.png

  case "$sequence" in
    0)
      convert -background none -fill "$fill_color" \
              -font Liberation-Sans -pointsize $FONT_SIZE \
              -gravity center label:"$message_text" \
              /tmp/text_$$.png
      convert /tmp/base_$$.png /tmp/text_$$.png \
              -gravity south -geometry +0+20 -composite \
              "$output_file"
      ;;
    90)
      convert -background none -fill "$fill_color" \
              -font Liberation-Sans -pointsize $FONT_SIZE \
              -gravity center label:"$message_text" -rotate 90 \
              /tmp/text_$$.png
      convert /tmp/base_$$.png /tmp/text_$$.png \
              -gravity west -geometry +20+0 -composite \
              "$output_file"
      ;;
    180)
      convert -background none -fill "$fill_color" \
              -font Liberation-Sans -pointsize $FONT_SIZE \
              -gravity center label:"$message_text" -rotate 180 \
              /tmp/text_$$.png
      convert /tmp/base_$$.png /tmp/text_$$.png \
              -gravity north -geometry +0+20 -composite \
              "$output_file"
      ;;
    270)
      convert -background none -fill "$fill_color" \
              -font Liberation-Sans -pointsize $FONT_SIZE \
              -gravity center label:"$message_text" -rotate 270 \
              /tmp/text_$$.png
      convert /tmp/base_$$.png /tmp/text_$$.png \
              -gravity east -geometry +20+0 -composite \
              "$output_file"
      ;;
  esac

  rm -f /tmp/base_$$.png /tmp/text_$$.png
  echo "Created: $output_file (${WIDTH}x${HEIGHT}) [${fill_color}]"
}

# ----------------------------------- MAIN ----------------------------------------

if [ $DO_PRUNE -eq 1 ]; then
  echo "Pruning legacy overlays..."
  prune_legacy
  echo
fi

if [ $DO_GENERATE -eq 0 ]; then
  exit 0
fi

echo "Generating VOL:v1 overlays..."
echo "Output: $BASE_DIR/sequence{0,90,180,270}/"
echo

for message_id in "${MESSAGE_ORDER[@]}"; do
  color="${COLORS[$message_id]:-white}"
  echo "[$message_id] \"${MESSAGES[$message_id]}\" [${color}]"

  create_overlay 0   ""         "$message_id"
  create_overlay 0   "-compact" "$message_id"
  create_overlay 90  ""         "$message_id"
  create_overlay 90  "-compact" "$message_id"
  create_overlay 180 ""         "$message_id"
  create_overlay 180 "-compact" "$message_id"
  create_overlay 270 ""         "$message_id"
  create_overlay 270 "-compact" "$message_id"
done

echo
echo "Generation complete."
echo "Overlay PNGs per sequence directory:"
for seq in 0 90 180 270; do
  count=$(ls -1 "$BASE_DIR/sequence${seq}"/overlay-*.png 2>/dev/null | wc -l)
  echo "  sequence${seq}/: $count"
done

