#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PI="${PI:-1000000028c0b59c}"
DHU_SERIAL="${DHU_SERIAL:-1000000028c0b59c}"
REMOTE_BIN="/data/local/tmp/arda-senderd"
REMOTE_CONFIG="/data/local/tmp/arda-config.toml"
CONFIG="${ARDA_CONFIG:-$ROOT/config.toml}"
CERT_DIR="${ARDA_CERT_DIR:-$HOME/Downloads/certs}"
DO_BUILD=0

usage() {
  cat <<'EOF'
Usage:
  scripts/arda-usb-run.sh [--build] [--] [aap-run CLI overrides...]

Environment:
  PI=1000000028c0b59c
  DHU_SERIAL=1000000028c0b59c
  ARDA_CONFIG=/path/to/config.toml
  ARDA_CERT_DIR=$HOME/Downloads/certs

Examples:
  ./scripts/arda-usb-run.sh
  ./scripts/arda-usb-run.sh --build
  PI=192.168.1.15:5555 ./scripts/arda-usb-run.sh
  PI=192.168.1.15:5555 ./scripts/arda-usb-run.sh -- --sensor-open --no-sensor-forward-vhal
  ./scripts/arda-usb-run.sh -- --log-level summary
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build)
      DO_BUILD=1
      shift
      ;;
    --)
      shift
      break
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      break
      ;;
  esac
done

if [[ "$DO_BUILD" == "1" ]]; then
  "$ROOT/scripts/docker-build.sh"
fi

BIN="$ROOT/target/aarch64-linux-android/release/arda-senderd"

[[ -x "$BIN" ]] || {
  echo "arda-usb-run: binary missing: $BIN" >&2
  echo "arda-usb-run: run ./scripts/docker-build.sh or pass --build" >&2
  exit 1
}

[[ -f "$CONFIG" ]] || {
  echo "arda-usb-run: config missing: $CONFIG" >&2
  exit 1
}

step() {
  printf '\n[%s/7] %s\n' "$1" "$2"
}

wait_for_adb_device() {
  local attempt state
  for attempt in $(seq 1 60); do
    state="$(adb -s "$PI" get-state 2>/dev/null || true)"
    if [[ "$state" == "device" ]]; then
      return 0
    fi
    sleep 0.25
  done

  echo "arda-usb-run: ADB device did not become ready: $PI" >&2
  adb devices -l >&2 || true
  return 1
}

step 1 "checking ADB device (no adb root restart)"
if [[ "$PI" == *:* ]]; then
  adb connect "$PI" >/dev/null 2>&1 || true
fi
wait_for_adb_device

echo "ADB_READY=YES serial=$PI"

step 2 "pushing arda-senderd and config"
adb -s "$PI" push "$BIN" "$REMOTE_BIN"
adb -s "$PI" shell chmod 755 "$REMOTE_BIN"
adb -s "$PI" push "$CONFIG" "$REMOTE_CONFIG"

step 3 "checking AAP certificates"
REMOTE_CERTS_OK="$(
  adb -s "$PI" shell '
    test -s /data/local/tmp/arda-certs/md_cert.pem &&
    test -s /data/local/tmp/arda-certs/md_key.pem &&
    echo YES || echo NO
  ' | tr -d '\r'
)"

if [[ "$REMOTE_CERTS_OK" != "YES" ]]; then
  if [[ -s "$CERT_DIR/md_cert.pem" && -s "$CERT_DIR/md_key.pem" ]]; then
    adb -s "$PI" shell 'mkdir -p /data/local/tmp/arda-certs'
    adb -s "$PI" push "$CERT_DIR/md_cert.pem" /data/local/tmp/arda-certs/md_cert.pem
    adb -s "$PI" push "$CERT_DIR/md_key.pem" /data/local/tmp/arda-certs/md_key.pem
    adb -s "$PI" shell \
      'chmod 600 /data/local/tmp/arda-certs/md_cert.pem /data/local/tmp/arda-certs/md_key.pem'
  else
    echo "arda-usb-run: remote AAP certs are missing." >&2
    echo "arda-usb-run: expected local fallback in: $CERT_DIR" >&2
    exit 1
  fi
fi

step 4 "stopping stale ARDA/screenrecord processes"
adb -s "$PI" shell '
  pkill -INT arda-senderd 2>/dev/null || true
  sleep 1
  pkill -9 arda-senderd 2>/dev/null || true
  pkill -INT screenrecord 2>/dev/null || true
  sleep 1
'

step 5 "checking USB accessory state"
adb -s "$PI" shell "$REMOTE_BIN" usb-accessory-status

echo
step 6 "USB baseline ready; start DHU from another terminal"
echo "ARDA_USB_BASELINE_READY=YES"
echo "ADB target: $PI"
echo "DHU USB serial: $DHU_SERIAL"
echo "Start DHU from another Mac terminal with:"
echo "  desktop-head-unit --usb=$DHU_SERIAL"
echo
echo
step 7 "starting ARDA AAP USB accessory session"
echo "Starting ARDA AAP USB accessory session..."
echo

adb -s "$PI" shell "$REMOTE_BIN" aap-run "$@"
