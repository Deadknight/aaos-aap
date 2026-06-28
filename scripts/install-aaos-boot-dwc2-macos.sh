#!/usr/bin/env bash
set -euo pipefail

BOOT_DIR="${1:-}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DWC2_DTBO="$REPO_ROOT/packaging/boot/overlays/dwc2.dtbo"

if [[ -z "$BOOT_DIR" ]]; then
  for candidate in /Volumes/boot /Volumes/BOOT /Volumes/bootfs /Volumes/NO\ NAME; do
    if [[ -d "$candidate" && -f "$candidate/config.txt" && -f "$candidate/cmdline.txt" ]]; then
      BOOT_DIR="$candidate"
      break
    fi
  done
fi

if [[ -z "$BOOT_DIR" || ! -d "$BOOT_DIR" ]]; then
  echo "ERROR: boot partition mount point not found." >&2
  echo "Usage: $0 /Volumes/boot" >&2
  exit 2
fi

if [[ ! -f "$BOOT_DIR/config.txt" || ! -f "$BOOT_DIR/cmdline.txt" ]]; then
  echo "ERROR: $BOOT_DIR does not look like a Raspberry Pi boot partition" >&2
  exit 2
fi

if [[ ! -f "$DWC2_DTBO" ]]; then
  echo "ERROR: missing $DWC2_DTBO" >&2
  exit 2
fi

TS="$(date +%Y%m%d-%H%M%S)"
cp "$BOOT_DIR/config.txt" "$BOOT_DIR/config.txt.arda-bak-$TS"
cp "$BOOT_DIR/cmdline.txt" "$BOOT_DIR/cmdline.txt.arda-bak-$TS"

mkdir -p "$BOOT_DIR/overlays"
cp "$DWC2_DTBO" "$BOOT_DIR/overlays/dwc2.dtbo"

if ! grep -Eq '^otg_mode=0$' "$BOOT_DIR/config.txt"; then
  printf '\n# ARDA AAOS AOA USB-C gadget\notg_mode=0\n' >> "$BOOT_DIR/config.txt"
fi

if ! grep -Eq '^dtoverlay=dwc2,dr_mode=peripheral$' "$BOOT_DIR/config.txt"; then
  printf 'dtoverlay=dwc2,dr_mode=peripheral\n' >> "$BOOT_DIR/config.txt"
fi

python3 - "$BOOT_DIR/cmdline.txt" <<'PY'
import pathlib
import sys
p = pathlib.Path(sys.argv[1])
text = p.read_text().replace('\n', ' ').strip()
args = text.split()
needle = 'androidboot.usbcontroller=fe980000.usb'
if needle not in args:
    args.append(needle)
p.write_text(' '.join(args) + '\n')
PY

echo "Patched boot partition: $BOOT_DIR"
echo "Backups:"
echo "  config.txt.arda-bak-$TS"
echo "  cmdline.txt.arda-bak-$TS"
echo
echo "Next: boot AAOS, connect adb, then run:"
echo "  scripts/install-aaos-aoa-gadget-adb.sh target/aarch64-linux-android/release/arda-senderd"
