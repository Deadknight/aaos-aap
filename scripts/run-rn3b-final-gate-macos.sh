#!/usr/bin/env bash
set -euo pipefail

AAOS_DEVICE="${AAOS_DEVICE:-1000000028c0b59c}"
PHONE_DEVICE="${PHONE_DEVICE:-}"
PLATFORM_PACKAGE="${PLATFORM_PACKAGE:-com.github.deadknight.ardaplatform.debug}"
EXPECTED_SSID="${EXPECTED_SSID:-}"
ADB="${ADB:-adb}"

fail() {
  echo "ARDA_RN3B_FINAL_GATE=FAIL reason=$*" >&2
  exit 1
}

shell() {
  "$ADB" -s "$AAOS_DEVICE" shell "$@"
}

"$ADB" -s "$AAOS_DEVICE" get-state >/dev/null 2>&1 || fail "AAOS_ADB_UNAVAILABLE"

PACKAGE_PATH="$(shell pm path "$PLATFORM_PACKAGE" | tr -d '\r' | head -n 1)"
[[ "$PACKAGE_PATH" == package:/product/priv-app/* || "$PACKAGE_PATH" == package:/system/priv-app/* || "$PACKAGE_PATH" == package:/system_ext/priv-app/* ]] || \
  fail "PLATFORM_NOT_PRIV_APP:$PACKAGE_PATH"
echo "ARDA_RN3B_FINAL_PRIV_APP_GATE=PASS path=${PACKAGE_PATH#package:}"

DUMP="$(shell dumpsys package "$PLATFORM_PACKAGE" | tr -d '\r')"
grep -qE 'SYSTEM|PRIVILEGED' <<<"$DUMP" || fail "PLATFORM_FLAGS_MISSING_SYSTEM_PRIVILEGED"
echo "ARDA_RN3B_FINAL_PLATFORM_FLAGS_GATE=PASS"

PROC_LINES="$(shell ps -A -o USER,PID,NAME,ARGS | grep -F "$PLATFORM_PACKAGE" || true)"
PROC_COUNT="$(grep -c . <<<"$PROC_LINES" || true)"
[[ "$PROC_COUNT" -eq 1 ]] || {
  echo "$PROC_LINES" >&2
  fail "EXPECTED_ONE_PLATFORM_PROCESS_GOT_$PROC_COUNT"
}
echo "ARDA_RN3B_FINAL_SINGLE_PROCESS_GATE=PASS"

CORE_USER="$(shell logcat -d -s ARDA_RN3B_AAOS:I '*:S' | grep 'ARDA_RN3B_AAOS_CORE_USER=' | tail -n 1 | sed -E 's/.*CORE_USER=([0-9]+).*/\1/' || true)"
[[ "$CORE_USER" == "0" ]] || fail "CORE_NOT_RUNNING_AS_USER_0:$CORE_USER"
echo "ARDA_RN3B_FINAL_CORE_USER_GATE=PASS user=0"

AAOS_LOG="$(shell logcat -d -s ARDA_RN3B_AAOS:I '*:S' | tr -d '\r')"
grep -q 'ARDA_RN3B_AAOS_CORE_OWNERSHIP=SINGLE_USER_SYSTEM' <<<"$AAOS_LOG" || fail "SINGLE_USER_MARKER_MISSING"
grep -q 'ARDA_RN3B_AAOS_LOHS_CONFIG_MODE=PLATFORM_SINGLE_USER_FIXED' <<<"$AAOS_LOG" || fail "FINAL_FIXED_LOHS_MARKER_MISSING"
grep -q 'ARDA_RN3B_AAOS_LOHS_STABLE_CONFIG_GATE=PASS' <<<"$AAOS_LOG" || fail "STABLE_CONFIG_GATE_NOT_PASS"
grep -qE 'ARDA_RN3B_AAOS_LOHS_CREDENTIAL_SOURCE=(FIRST_BOOT_DEVICE_GENERATED|DEVICE_PROTECTED_STORAGE)' <<<"$AAOS_LOG" || \
  fail "DEVICE_CREDENTIAL_SOURCE_MISSING"
grep -q 'ARDA_RN3B_AAOS_LOHS_CREDENTIAL_VERSION=1' <<<"$AAOS_LOG" || fail "CREDENTIAL_VERSION_MISMATCH"
grep -q 'ARDA_RN3B_AAOS_RFCOMM_LISTEN=PASS' <<<"$AAOS_LOG" || fail "RFCOMM_LISTENER_NOT_READY"

ACTUAL_SSID="$(grep 'ARDA_RN3B_AAOS_LOHS_SSID=' <<<"$AAOS_LOG" | tail -n 1 | sed -E 's/.*LOHS_SSID=([^[:space:]]+).*/\1/' || true)"
[[ "$ACTUAL_SSID" =~ ^ARDA-[ABCDEFGHJKLMNPQRSTUVWXYZ23456789]{8}$ ]] || \
  fail "INVALID_DEVICE_UNIQUE_SSID:$ACTUAL_SSID"
if [[ -n "$EXPECTED_SSID" && "$ACTUAL_SSID" != "$EXPECTED_SSID" ]]; then
  fail "SSID_MISMATCH expected=$EXPECTED_SSID actual=$ACTUAL_SSID"
fi
if grep -qE 'FRAMEWORK_RANDOM|AndroidShare_|ArdaShare_' <<<"$AAOS_LOG"; then
  fail "RANDOM_LOHS_MARKER_PRESENT"
fi
if shell test -e /product/etc/arda/rn3b_wifi.conf; then
  fail "STATIC_WIFI_SECRET_FILE_PRESENT"
fi
echo "ARDA_RN3B_FINAL_DEVICE_SSID_GATE=PASS ssid=$ACTUAL_SSID"
echo "ARDA_RN3B_FINAL_AAOS_TRANSPORT_GATE=PASS"

SENDERD_INIT_STATE="$(shell getprop init.svc.arda-connectivityd | tr -d '\r')"
[[ "$SENDERD_INIT_STATE" == "running" ]] || fail "SENDERD_INIT_SERVICE_NOT_RUNNING:$SENDERD_INIT_STATE"
CONFIG_PATH="/data/user_de/0/$PLATFORM_PACKAGE/files/arda-connectivity.toml"
shell test -s "$CONFIG_PATH" || fail "SENDERD_CONFIG_MISSING:$CONFIG_PATH"
echo "ARDA_RN3B_FINAL_SENDERD_INIT_GATE=PASS config=$CONFIG_PATH"

SENDERD_PROCESS="$(shell ps -A -o PID,ARGS | grep 'arda-connectivityd bt-bootstrapd' | grep -v grep | head -n 1 | tr -d '\r' || true)"
[[ -n "$SENDERD_PROCESS" ]] || fail "CONNECTIVITYD_NOT_RUNNING"
SENDERD_PID="$(awk '{print $1}' <<<"$SENDERD_PROCESS")"
if ! shell 'ss -lnt 2>/dev/null || netstat -lnt 2>/dev/null' | grep -q '127.0.0.1:5283'; then
  fail "SENDERD_BRIDGE_NOT_LISTENING"
fi
echo "ARDA_RN3B_FINAL_SENDERD_GATE=PASS pid=$SENDERD_PID"

if [[ -n "$PHONE_DEVICE" ]]; then
  "$ADB" -s "$PHONE_DEVICE" get-state >/dev/null 2>&1 || fail "PHONE_ADB_UNAVAILABLE"
  PHONE_LOG="$("$ADB" -s "$PHONE_DEVICE" logcat -d -s ARDA_RN3B_PHONE:I '*:S' | tr -d '\r')"
  grep -q 'ARDA_RN3B_PHONE_HELLO_PING_PONG_GATE=PASS' <<<"$PHONE_LOG" || fail "PHONE_HELLO_GATE_MISSING"
  grep -q "ARDA_RN3B_PHONE_WIFI_SSID=$ACTUAL_SSID" <<<"$PHONE_LOG" || fail "PHONE_WIFI_SSID_MISMATCH"
  grep -q 'ARDA_RN3B_PHONE_LOCAL_WIFI_GATE=PASS' <<<"$PHONE_LOG" || fail "PHONE_LOCAL_WIFI_GATE_MISSING"
  echo "ARDA_RN3B_FINAL_PHONE_GATE=PASS"
fi

echo "ARDA_RN3B_FINAL_GATE=PASS"
