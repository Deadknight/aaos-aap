#!/usr/bin/env bash
set -euo pipefail

BIN="${1:-target/aarch64-linux-android/release/arda-senderd}"
RC_LOCAL="packaging/vendor/etc/init/arda-aoa-gadget.rc"

if [[ ! -f "$BIN" ]]; then
  echo "ERROR: arda-senderd binary not found: $BIN" >&2
  echo "Usage: $0 path/to/arda-senderd" >&2
  exit 2
fi

if [[ ! -f "$RC_LOCAL" ]]; then
  echo "ERROR: missing $RC_LOCAL; run from repo root" >&2
  exit 2
fi

adb wait-for-device
adb push "$BIN" /data/local/tmp/arda-senderd
adb push "$RC_LOCAL" /data/local/tmp/arda-aoa-gadget.rc

adb shell su -c 'mount -o rw,remount /vendor 2>/dev/null || mount -o rw,remount / 2>/dev/null || true'
adb shell su -c 'mkdir -p /vendor/bin /vendor/etc/init'
adb shell su -c 'cp /data/local/tmp/arda-senderd /vendor/bin/arda-senderd && chmod 755 /vendor/bin/arda-senderd'
adb shell su -c 'cp /data/local/tmp/arda-aoa-gadget.rc /vendor/etc/init/arda-aoa-gadget.rc && chmod 644 /vendor/etc/init/arda-aoa-gadget.rc'

adb shell su -c '/vendor/bin/arda-senderd aaos-aoa-gadget --udc fe980000.usb'

echo
echo "Installed ARDA AAOS AOA support. The daemon now auto-checks/creates the gadget before AAP commands."
echo "Manual verification:"
echo '  adb shell su -c "/vendor/bin/arda-senderd aaos-aoa-gadget --check"'
echo "Normal usage auto-runs the preflight first, for example:"
echo '  adb shell su -c "/vendor/bin/arda-senderd aap-session-probe --addr 0.0.0.0:5277 --keys /data/local/tmp/arda-certs"'
echo "Reboot test:"
echo "  adb reboot"
