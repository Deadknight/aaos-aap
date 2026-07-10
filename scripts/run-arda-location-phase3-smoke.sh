#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PI="${PI:-192.168.1.21:5555}"
BIN="${ARDA_BIN:-$ROOT/target/aarch64-linux-android/release/arda-senderd}"
REMOTE_BIN="${REMOTE_BIN:-/data/local/tmp/arda-senderd}"

fail() {
  echo "ARDA_GNSS_HAL_SMOKE_FAIL=$*" >&2
  exit 2
}

test -s "$BIN" || fail "BINARY_MISSING_$BIN"

adb connect "$PI" >/dev/null 2>&1 || true
adb -s "$PI" root >/dev/null 2>&1 || true
adb -s "$PI" wait-for-device

UID_NOW="$(adb -s "$PI" shell -n id -u 2>/dev/null | tr -d '\r\n')"
[[ "$UID_NOW" == "0" ]] || fail "ADB_ROOT_REQUIRED_uid_${UID_NOW:-unknown}"

echo "ARDA_GNSS_HAL_SMOKE_ADB_ROOT=PASS device=$PI"

SERVICE_CHECK="$(adb -s "$PI" shell -n 'service check android.hardware.gnss.IGnss/default 2>/dev/null || true' | tr -d '\r')"
printf '%s\n' "$SERVICE_CHECK"
printf '%s\n' "$SERVICE_CHECK" | grep -Fq 'found' || fail "GNSS_AIDL_SERVICE_MISSING"

OLD_PACKAGE="$(adb -s "$PI" shell -n 'pm path com.arda.location.provider 2>/dev/null || true' | tr -d '\r')"
[[ -z "$OLD_PACKAGE" ]] || fail "OLD_LOCATION_PROVIDER_STILL_INSTALLED"

HAL_PS="$(adb -s "$PI" shell -n "ps -AZ | grep -F 'android.hardware.gnss-service.example' || true" | tr -d '\r')"
printf '%s\n' "$HAL_PS"
printf '%s\n' "$HAL_PS" \
  | grep -Eq '^u:r:arda_gnss_hal:s0[[:space:]].*android\.hardware\.gnss-service\.example' \
  || fail "GNSS_HAL_DOMAIN_INVALID"

adb -s "$PI" push "$BIN" "$REMOTE_BIN" >/dev/null
adb -s "$PI" shell -n chmod 0755 "$REMOTE_BIN"

PROTO_OUT="$(adb -s "$PI" shell -n "$REMOTE_BIN sensor-proto-smoke" 2>&1 | tr -d '\r')"
printf '%s\n' "$PROTO_OUT"

for marker in \
  SENSOR_PROTO_ALL_23_EVENT_KINDS=YES \
  SENSOR_PROTO_LOCATION_OPTIONAL_PRESENCE=YES \
  SENSOR_PROTO_GPS_SATELLITE_FIDELITY=YES \
  SENSOR_PROTO_SMOKE_OK=YES
do
  printf '%s\n' "$PROTO_OUT" | grep -Fq "$marker" \
    || fail "PROTO_MARKER_MISSING_$marker"
done

adb -s "$PI" logcat -c
adb -s "$PI" shell -n 'dmesg -c >/dev/null 2>&1 || true'
adb -s "$PI" shell -n 'am force-stop com.google.android.apps.maps 2>/dev/null || true'
adb -s "$PI" shell -n 'monkey -p com.google.android.apps.maps 1 >/dev/null 2>&1 || true'

READY=0

for attempt in $(seq 1 120); do
  HAL_LOG="$(adb -s "$PI" logcat -d -v brief -s ArdaGnssAidl:I '*:S' 2>/dev/null | tr -d '\r')"

  if printf '%s\n' "$HAL_LOG" | grep -Fq 'ARDA_GNSS_HAL_READY socket=arda_gnss_hal'; then
    READY=1
    echo "ARDA_GNSS_HAL_READY_GATE=PASS attempt=$attempt"
    break
  fi

  sleep 0.5
done

[[ "$READY" == "1" ]] || fail "GNSS_HAL_SOCKET_NOT_READY"

BRIDGE_OUT="$(
  adb -s "$PI" shell -n \
    "$REMOTE_BIN location-bridge-smoke --lat 39.9000000 --lon 32.8000000 --accuracy 3.0 --altitude 900.0 --speed 0.0 --bearing 0.0" \
    2>&1 \
    | tr -d '\r'
)"

printf '%s\n' "$BRIDGE_OUT"

printf '%s\n' "$BRIDGE_OUT" \
  | grep -Fq 'ARDA_GNSS_HAL_BRIDGE_CONNECT socket=arda_gnss_hal' \
  || fail "RUST_GNSS_HAL_CONNECT_MARKER_MISSING"

printf '%s\n' "$BRIDGE_OUT" \
  | grep -Fq 'ARDA_GNSS_HAL_FIX_PUBLISHED' \
  || fail "RUST_GNSS_HAL_FIX_MARKER_MISSING"

printf '%s\n' "$BRIDGE_OUT" \
  | grep -Fq 'ARDA_LOCATION_BRIDGE_SMOKE_OK=YES' \
  || fail "LOCATION_SMOKE_MARKER_MISSING"

sleep 2

HAL_LOG="$(adb -s "$PI" logcat -d -v threadtime -s ArdaGnssAidl:I '*:S' 2>/dev/null | tr -d '\r')"
printf '%s\n' "$HAL_LOG"

for marker in \
  'ARDA_GNSS_HAL_READY socket=arda_gnss_hal' \
  'ARDA_GNSS_HAL_CLIENT uid=0' \
  'ARDA_GNSS_HAL_FIX_RECEIVED lat=39.9000000 lon=32.8000000' \
  'ARDA_GNSS_HAL_FIX_REPORTED lat=39.9000000 lon=32.8000000'
do
  printf '%s\n' "$HAL_LOG" | grep -Fq "$marker" \
    || fail "HAL_MARKER_MISSING_$marker"
done

DUMP="$(adb -s "$PI" shell -n dumpsys location 2>/dev/null | tr -d '\r')"

printf '%s\n' "$DUMP" \
  | grep -i -E \
    'last location=Location\[gps|gps provider|android\.hardware\.gnss|com\.google\.android\.apps\.maps|fused_location_provider' \
  | tail -160 \
  || true

printf '%s\n' "$DUMP" \
  | grep -Fq 'last location=Location[gps 39.900000,32.800000' \
  || fail "GPS_PROVIDER_SYNTHETIC_FIX_MISSING"

DENIALS="$(
  adb -s "$PI" shell -n \
    "dmesg 2>/dev/null | grep -E 'avc: *denied.*(hal_gnss_default|arda_vehicle_router|arda_gnss_hal)' | tail -120 || true" \
    | tr -d '\r'
)"

if [[ -n "$DENIALS" ]]; then
  printf '%s\n' "$DENIALS"
  fail "GNSS_HAL_AVC_DENIAL_PRESENT"
fi

echo "ARDA_PHASE3_0B3_PROTO_FIDELITY=PASS"
echo "ARDA_PHASE3_0B3_GNSS_AIDL_SERVICE=PASS"
echo "ARDA_PHASE3_0B3_OLD_JAVA_PROVIDER_REMOVED=PASS"
echo "ARDA_PHASE3_0B3_GNSS_HAL_DOMAIN=PASS"
echo "ARDA_PHASE3_0B3_GNSS_HAL_BRIDGE=PASS"
echo "ARDA_PHASE3_0B3_GNSS_HAL_FIX_REPORT=PASS"
echo "ARDA_PHASE3_0B3_GPS_PROVIDER_SYNTHETIC=PASS"
echo "ARDA_PHASE3_0B3_SYNTHETIC_RUNTIME_GATE=PASS"

