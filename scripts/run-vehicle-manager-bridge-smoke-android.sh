#!/usr/bin/env bash
set -euo pipefail

DEVICE="${DEVICE:-192.168.1.21:5555}"
REMOTE_BIN="${REMOTE_BIN:-/data/local/tmp/arda-senderd}"
SOCKET="${SOCKET:-/dev/socket/arda_vehicle_router}"
INIT_SERVICE="${INIT_SERVICE:-arda_vehicle_router}"
STATUS_LOG="${STATUS_LOG:-/data/local/tmp/arda-vehicle-manager-bridge-status.log}"
READY_TIMEOUT_SEC="${READY_TIMEOUT_SEC:-30}"
MANAGER_COMPONENT="${MANAGER_COMPONENT:-com.arda.vehicle.manager/.ShellReceiver}"
MANAGER_ACTION="${MANAGER_ACTION:-com.arda.vehicle.manager.SHELL}"

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
  printf 'MANAGER_BRIDGE_ADB_ROOT_REQUIRED device=%s uid=%s adb_root_result=%q\n' \
    "$DEVICE" "${REMOTE_UID:-unknown}" "$ADB_ROOT_RESULT" >&2
  exit 1
fi

printf 'MANAGER_BRIDGE_ADB_ROOT_OK device=%s uid=%s adb_root_result=%q\n' \
  "$DEVICE" "$REMOTE_UID" "$ADB_ROOT_RESULT"

SELINUX_MODE="$(adb -s "$DEVICE" shell getenforce 2>/dev/null | tr -d '\r' | tail -n 1 || true)"
echo "MANAGER_BRIDGE_SELINUX_MODE=${SELINUX_MODE:-unknown}"

if [[ "$SELINUX_MODE" != "Enforcing" ]]; then
  echo "MANAGER_BRIDGE_SELINUX_ENFORCING_REQUIRED mode=${SELINUX_MODE:-unknown}" >&2
  exit 1
fi

echo "MANAGER_BRIDGE_SELINUX_GATE=PASS"

PRE_STATUS="$(adb -s "$DEVICE" shell am broadcast \
  -n "$MANAGER_COMPONENT" \
  -a "$MANAGER_ACTION" \
  --es command status 2>&1 | tr -d '\r')"
printf '%s\n' "$PRE_STATUS"

if ! printf '%s\n' "$PRE_STATUS" | grep -q 'granted=true.*signer_ok=true.*connected=true.*ready=true'; then
  echo "MANAGER_BRIDGE_MANAGER_PRECONDITION=FAIL" >&2
  echo "Expected at least one granted, signer-valid, connected, ready adapter." >&2
  exit 1
fi

echo "MANAGER_BRIDGE_MANAGER_PRECONDITION=PASS"

adb -s "$DEVICE" logcat -c

SERVICE_STATE="$(adb -s "$DEVICE" shell "getprop init.svc.$INIT_SERVICE" 2>/dev/null | tr -d '\r' | tail -n 1 || true)"
echo "MANAGER_BRIDGE_ROUTER_SERVICE_STATE=${SERVICE_STATE:-unknown} service=$INIT_SERVICE"

if [[ "$SERVICE_STATE" != "running" ]]; then
  echo "MANAGER_BRIDGE_ROUTER_SERVICE=FAIL service=$INIT_SERVICE state=${SERVICE_STATE:-unknown}" >&2
  adb -s "$DEVICE" shell "dmesg 2>/dev/null | grep -E 'arda_vehicle_router|VEHICLE_ROUTER_|VEHICLE_WASM_ENGINE|incorrect label|no domain transition|Unable to start service|cannot execv' | tail -160 || true" >&2 || true
  exit 1
fi

echo "MANAGER_BRIDGE_ROUTER_SERVICE=PASS service=$INIT_SERVICE"

for _ in $(seq 1 30); do
  if adb -s "$DEVICE" shell "test -S '$SOCKET'" 2>/dev/null; then
    break
  fi
  sleep 1
done

if ! adb -s "$DEVICE" shell "test -S '$SOCKET'" 2>/dev/null; then
  echo "MANAGER_BRIDGE_ROUTER_SOCKET=FAIL path=$SOCKET" >&2
  exit 1
fi

echo "MANAGER_BRIDGE_ROUTER_SOCKET=PASS path=$SOCKET"

SECURITY_CONTEXTS="$(adb -s "$DEVICE" shell "ls -lZ '$SOCKET' 2>/dev/null || true; ps -AZ | grep -E 'arda-senderd|com\\.arda\\.vehicle\\.manager' || true" | tr -d '\r')"
printf '%s\n' "$SECURITY_CONTEXTS"

if ! printf '%s\n' "$SECURITY_CONTEXTS" | grep -q 'u:object_r:arda_vehicle_router_socket:s0'; then
  echo "MANAGER_BRIDGE_ROUTER_SOCKET_CONTEXT=FAIL" >&2
  exit 1
fi

echo "MANAGER_BRIDGE_ROUTER_SOCKET_CONTEXT=PASS"

if ! printf '%s\n' "$SECURITY_CONTEXTS" | grep -q '^u:r:arda_vehicle_router:s0 .*arda-senderd'; then
  echo "MANAGER_BRIDGE_ROUTER_DOMAIN=FAIL" >&2
  exit 1
fi

echo "MANAGER_BRIDGE_ROUTER_DOMAIN=PASS"

if ! printf '%s\n' "$SECURITY_CONTEXTS" | grep -Eq '^u:r:arda_vehicle_manager:s0(:[^[:space:]]+)?[[:space:]].*com\.arda\.vehicle\.manager'; then
  echo "MANAGER_BRIDGE_MANAGER_DOMAIN=FAIL" >&2
  exit 1
fi

echo "MANAGER_BRIDGE_MANAGER_DOMAIN=PASS"

DEADLINE=$((SECONDS + READY_TIMEOUT_SEC))
REGISTERED=0
LAST_STATUS=""
while (( SECONDS < DEADLINE )); do
  LAST_STATUS="$(adb -s "$DEVICE" shell am broadcast \
    -n "$MANAGER_COMPONENT" \
    -a "$MANAGER_ACTION" \
    --es command status 2>&1 | tr -d '\r')"
  if printf '%s\n' "$LAST_STATUS" | grep -q 'ROUTER sources=[1-9].*state=REGISTERED registered=true'; then
    REGISTERED=1
    break
  fi
  sleep 1
done

adb -s "$DEVICE" shell "cat > '$STATUS_LOG'" <<<"$LAST_STATUS" >/dev/null

print_diagnostics() {
  echo
  echo "================ MANAGER STATUS ================"
  printf '%s\n' "$LAST_STATUS"
  echo
  echo "================ SOCKET / PROCESS CONTEXT ================"
  adb -s "$DEVICE" shell "ls -lZ '$SOCKET' 2>/dev/null || true; ps -AZ | grep -E 'arda-senderd|com\\.arda\\.vehicle\\.manager' || true"
  echo
  echo "================ ROUTER KMSG ================"
  adb -s "$DEVICE" shell "dmesg 2>/dev/null | grep -E 'VEHICLE_ROUTER_|VEHICLE_WASM_ENGINE|arda_vehicle_router|execmem|connectto' | tail -160 || true"
  echo
  echo "================ MANAGER LOG ================"
  adb -s "$DEVICE" logcat -d -v threadtime \
    -s ArdaVehicleManager:I ArdaVehicleSmoke:I '*:S' \
    | tee /tmp/arda-vehicle-manager-bridge-logcat.txt
  echo
  echo "================ SELINUX AVC ================"
  adb -s "$DEVICE" shell \
    "dmesg 2>/dev/null | grep -E 'avc: *denied' | grep -E 'arda_vehicle_router|arda_vehicle_manager|ArdaVehicle|execmem|connectto' | tail -160 || true"
  adb -s "$DEVICE" logcat -b main -b system -b crash -d 2>/dev/null \
    | grep -E 'avc: *denied' \
    | grep -E 'arda_vehicle_router|arda_vehicle_manager|ArdaVehicle' \
    | tail -160 \
    || true
}

if [[ "$REGISTERED" -ne 1 ]]; then
  print_diagnostics
  echo
  echo "MANAGER_BRIDGE_ROUTER_REGISTRATION=FAIL enforcing_mode=${SELINUX_MODE:-unknown}" >&2
  exit 1
fi

echo
echo "================ MANAGER STATUS ================"
printf '%s\n' "$LAST_STATUS"
echo "MANAGER_BRIDGE_ROUTER_REGISTRATION=PASS"

echo
echo "================ MANAGER BRIDGE SMOKE ================"
if ! adb -s "$DEVICE" shell \
  "'$REMOTE_BIN' vehicle-manager-bridge-smoke --socket '$SOCKET'"; then
  print_diagnostics
  exit 1
fi

echo
echo "================ ROUTER KMSG FINAL ================"
adb -s "$DEVICE" shell "dmesg 2>/dev/null | grep -E 'VEHICLE_ROUTER_|VEHICLE_WASM_ENGINE|arda_vehicle_router|execmem|connectto' | tail -160 || true"

echo
echo "================ MANAGER LOG FINAL ================"
adb -s "$DEVICE" logcat -d -v threadtime \
  -s ArdaVehicleManager:I ArdaVehicleSmoke:I '*:S' \
  | tee /tmp/arda-vehicle-manager-bridge-logcat.txt

echo
echo "MANAGER_BRIDGE_ENFORCING_RUNTIME_GATE=PASS selinux=${SELINUX_MODE:-unknown}"
