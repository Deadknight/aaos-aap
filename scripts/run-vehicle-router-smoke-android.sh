#!/usr/bin/env bash
set -euo pipefail

DEVICE="${DEVICE:-192.168.1.15:5555}"
REMOTE_BIN="${REMOTE_BIN:-/data/local/tmp/arda-senderd}"
SOCKET="${SOCKET:-/data/local/tmp/arda_vehicle_router_smoke.sock}"
LOG="${LOG:-/data/local/tmp/arda-vehicle-router-smoke.log}"

adb connect "$DEVICE" >/dev/null

ADB_ROOT_RESULT="$(adb -s "$DEVICE" root 2>&1 | tr -d '\r' || true)"
REMOTE_UID=""
for _ in $(seq 1 30); do
  adb connect "$DEVICE" >/dev/null 2>&1 || true
  REMOTE_UID="$(adb -s "$DEVICE" shell 'id -u' 2>/dev/null | tr -d '\r' | tail -n 1 || true)"
  if [[ "$REMOTE_UID" == "0" ]]; then
    break
  fi
  sleep 1
done

if [[ "$REMOTE_UID" != "0" ]]; then
  printf 'ROUTER_ADB_ROOT_REQUIRED device=%s uid=%s adb_root_result=%q\n' \
    "$DEVICE" "${REMOTE_UID:-unknown}" "$ADB_ROOT_RESULT" >&2
  exit 1
fi

printf 'ROUTER_ADB_ROOT_OK device=%s uid=%s adb_root_result=%q\n' \
  "$DEVICE" "$REMOTE_UID" "$ADB_ROOT_RESULT"

ROUTER_PID="$({
  adb -s "$DEVICE" shell \
    "sh -c 'rm -f \"$SOCKET\" \"$LOG\"; nohup \"$REMOTE_BIN\" vehicle-routerd --socket \"$SOCKET\" >\"$LOG\" 2>&1 </dev/null & echo \$!'"
} | tr -d '\r' | tail -n 1)"

if [[ ! "$ROUTER_PID" =~ ^[0-9]+$ ]]; then
  printf 'ROUTER_START_FAILED invalid_pid=%s\n' "$ROUTER_PID" >&2
  adb -s "$DEVICE" shell "cat '$LOG' 2>/dev/null || true" >&2 || true
  exit 1
fi

cleanup() {
  adb -s "$DEVICE" shell \
    "kill '$ROUTER_PID' 2>/dev/null || true; rm -f '$SOCKET'" \
    >/dev/null 2>&1 || true
}
trap cleanup EXIT

sleep 2

if ! adb -s "$DEVICE" shell "kill -0 '$ROUTER_PID' 2>/dev/null"; then
  echo "ROUTER_START_FAILED pid=$ROUTER_PID exited_before_smoke=true" >&2
  adb -s "$DEVICE" shell "cat '$LOG' 2>/dev/null || true" >&2 || true
  exit 1
fi

echo
echo "================ ROUTER READY ================"
adb -s "$DEVICE" shell "cat '$LOG' 2>/dev/null || true"

echo
echo "================ ROUTER SMOKE ================"
if ! adb -s "$DEVICE" shell \
  "'$REMOTE_BIN' vehicle-router-smoke --socket '$SOCKET'"; then
  echo
  echo "================ ROUTER LOG AFTER FAILURE ================"
  adb -s "$DEVICE" shell "cat '$LOG' 2>/dev/null || true"
  exit 1
fi

echo
echo "================ ROUTER LOG FINAL ================"
adb -s "$DEVICE" shell "cat '$LOG' 2>/dev/null || true"
