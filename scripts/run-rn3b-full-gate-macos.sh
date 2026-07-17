#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ADB="${ADB:-adb}"
AAOS_DEVICE="${AAOS_DEVICE:-1000000028c0b59c}"
PHONE_DEVICE="${PHONE_DEVICE:-554b52f6}"
AAOS_BLUETOOTH_ADDRESS="${AAOS_BLUETOOTH_ADDRESS:-}"
PLATFORM_PACKAGE="${PLATFORM_PACKAGE:-com.github.deadknight.ardaplatform.debug}"
PLATFORM_USER="${PLATFORM_USER:-auto}"
COMPANION_PACKAGE="${COMPANION_PACKAGE:-com.github.deadknight.ardacompanion.debug}"
BUILD_SENDERD="${BUILD_SENDERD:-false}"
RESTART_PLATFORM="${RESTART_PLATFORM:-false}"
RESTART_COMPANION_SESSION="${RESTART_COMPANION_SESSION:-false}"
COMPANION_TRIGGER_MODE="${COMPANION_TRIGGER_MODE:-bluetooth}"
REVERSE_PROXY_WAIT_SECONDS="${REVERSE_PROXY_WAIT_SECONDS:-240}"
RN3B_GATE_MODE="${RN3B_GATE_MODE:-reverse_proxy}"
LOCAL_WIFI_WAIT_SECONDS="${LOCAL_WIFI_WAIT_SECONDS:-180}"
FIXED_LOHS_SSID="${FIXED_LOHS_SSID:-ARDA-RN3B}"
FIXED_LOHS_PSK="${FIXED_LOHS_PSK:-ArdaRN3B2026Pass!}"
FIXED_LOHS_BAND="${FIXED_LOHS_BAND:-2}"
FIXED_LOHS_WAIT_SECONDS="${FIXED_LOHS_WAIT_SECONDS:-45}"
FIXED_LOHS_PID_FILE="/data/local/tmp/arda-rn3b-fixed-lohs.pid"
FIXED_LOHS_MODE_FILE="/data/local/tmp/arda-rn3b-fixed-lohs.mode"
FIXED_LOHS_LOG_FILE="/data/local/tmp/arda-rn3b-fixed-lohs.log"

fail() {
  echo "ARDA_RN3B_FULL_GATE_FAIL=$1" >&2
  exit 1
}

fixed_lohs_pid() {
  "$ADB" -s "$AAOS_DEVICE" shell "cat '$FIXED_LOHS_PID_FILE' 2>/dev/null || true" 2>/dev/null | tr -d '\r'
}

fixed_lohs_mode() {
  "$ADB" -s "$AAOS_DEVICE" shell "cat '$FIXED_LOHS_MODE_FILE' 2>/dev/null || true" 2>/dev/null | tr -d '\r\n'
}

fixed_ap_visible() {
  "$ADB" -s "$AAOS_DEVICE" shell dumpsys wifi 2>/dev/null | grep -Fq "$FIXED_LOHS_SSID"
}

fixed_lohs_alive() {
  local mode pid
  mode="$(fixed_lohs_mode)"
  case "$mode" in
    start-softap)
      fixed_ap_visible
      ;;
    start-lohs)
      pid="$(fixed_lohs_pid)"
      [ -n "$pid" ] || return 1
      "$ADB" -s "$AAOS_DEVICE" shell "kill -0 '$pid' 2>/dev/null" >/dev/null 2>&1 && \
        fixed_ap_visible
      ;;
    *)
      return 1
      ;;
  esac
}

stop_fixed_lohs() {
  local mode pid
  mode="$(fixed_lohs_mode)"
  pid="$(fixed_lohs_pid)"

  # stop-softap also stops the shell-created local-only SoftAP on releases that
  # expose start-lohs through WifiShellCommand. Ignore unsupported-command errors.
  "$ADB" -s "$AAOS_DEVICE" shell cmd wifi stop-softap >/dev/null 2>&1 || true
  if [ -n "$pid" ]; then
    "$ADB" -s "$AAOS_DEVICE" shell "kill '$pid' 2>/dev/null || true" >/dev/null 2>&1 || true
  fi
  "$ADB" -s "$AAOS_DEVICE" shell \
    "rm -f '$FIXED_LOHS_PID_FILE' '$FIXED_LOHS_MODE_FILE' '$FIXED_LOHS_LOG_FILE'" \
    >/dev/null 2>&1 || true
  [ -n "$mode" ] && echo "ARDA_RN3B_FIXED_LOHS_STOP_MODE=$mode"
  sleep 2
}

start_fixed_lohs() {
  if fixed_lohs_alive; then
    echo "ARDA_RN3B_FIXED_LOHS_LIFECYCLE=PRESERVED"
    echo "ARDA_RN3B_FIXED_LOHS_COMMAND=$(fixed_lohs_mode)"
    return 0
  fi

  local wifi_help command
  wifi_help="$("$ADB" -s "$AAOS_DEVICE" shell cmd wifi help 2>/dev/null | tr -d '\r' || true)"
  if printf '%s\n' "$wifi_help" | grep -q 'start-lohs'; then
    command=start-lohs
  elif printf '%s\n' "$wifi_help" | grep -q 'start-softap'; then
    # Older AAOS Wi-Fi modules do not expose start-lohs. AOSP start-softap
    # starts a configured SoftAP but does not itself activate internet
    # tethering, which is sufficient for the ARDA local saved-network path.
    command=start-softap
  else
    printf '%s\n' "$wifi_help" | tail -120 >&2
    fail AAOS_CMD_WIFI_FIXED_SOFTAP_UNAVAILABLE
  fi

  "$ADB" -s "$AAOS_DEVICE" shell \
    "rm -f '$FIXED_LOHS_LOG_FILE' '$FIXED_LOHS_PID_FILE' '$FIXED_LOHS_MODE_FILE'; \
     echo '$command' >'$FIXED_LOHS_MODE_FILE'; \
     nohup cmd wifi '$command' '$FIXED_LOHS_SSID' wpa2 '$FIXED_LOHS_PSK' -b '$FIXED_LOHS_BAND' \
       >'$FIXED_LOHS_LOG_FILE' 2>&1 </dev/null & echo \$! >'$FIXED_LOHS_PID_FILE'" \
    >/dev/null

  local ready=NO
  for _ in $(seq 1 "$FIXED_LOHS_WAIT_SECONDS"); do
    if fixed_ap_visible; then
      ready=YES
      break
    fi

    # start-lohs must keep its shell request alive. start-softap may return
    # immediately after registering the AP, so only treat a dead process as
    # fatal for start-lohs.
    if [ "$command" = "start-lohs" ]; then
      local pid
      pid="$(fixed_lohs_pid)"
      if [ -z "$pid" ] || ! "$ADB" -s "$AAOS_DEVICE" shell \
          "kill -0 '$pid' 2>/dev/null" >/dev/null 2>&1; then
        break
      fi
    fi
    sleep 1
  done

  if [ "$ready" != "YES" ]; then
    echo "ARDA_RN3B_FIXED_LOHS_COMMAND=$command" >&2
    "$ADB" -s "$AAOS_DEVICE" shell \
      "cat '$FIXED_LOHS_LOG_FILE' 2>/dev/null || true" | tr -d '\r' || true
    "$ADB" -s "$AAOS_DEVICE" shell dumpsys wifi 2>/dev/null | \
      grep -E -i -C 3 'softap|hotspot|ap state|ssid' | tail -160 || true
    fail AAOS_FIXED_LOHS_START_TIMEOUT
  fi

  echo "ARDA_RN3B_FIXED_LOHS_COMMAND=$command"
  if [ "$command" = "start-softap" ]; then
    echo "ARDA_RN3B_FIXED_LOHS_COMPAT_MODE=SOFTAP_WITHOUT_TETHERING"
  else
    echo "ARDA_RN3B_FIXED_LOHS_COMPAT_MODE=LOCAL_ONLY_HOTSPOT"
  fi
  echo "ARDA_RN3B_FIXED_LOHS_START=PASS"
}

cleanup_on_exit() {
  status=$?
  if [ "$status" -ne 0 ]; then
    "$ADB" -s "$AAOS_DEVICE" shell settings put global http_proxy :0 >/dev/null 2>&1 || true
    "$ADB" -s "$AAOS_DEVICE" shell settings delete global http_proxy >/dev/null 2>&1 || true
  fi
}
trap cleanup_on_exit EXIT

is_bt_address() {
  printf '%s' "$1" | grep -Eiq '^([0-9a-f]{2}:){5}[0-9a-f]{2}$'
}

resolve_aaos_bt_address() {
  local candidate=""

  candidate="$(
    "$ADB" -s "$AAOS_DEVICE" shell '
      for file in \
        /sys/class/bluetooth/hci0/address \
        /data/misc/apexdata/com.android.btservices/bt_config.conf \
        /data/misc/bluedroid/bt_config.conf \
        /data/misc/bluetooth/bt_config.conf; do
        if [ -r "$file" ]; then
          case "$file" in
            */address)
              cat "$file" 2>/dev/null
              ;;
            *)
              awk '\''
                /^\[Adapter\]$/ { in_adapter=1; next }
                /^\[/ { if (in_adapter) exit }
                in_adapter && /^Address[[:space:]]*=/ {
                  sub(/^[^=]*=[[:space:]]*/, "")
                  print
                  exit
                }
              '\'' "$file" 2>/dev/null
              ;;
          esac
        fi
      done
    ' 2>/dev/null | tr -d '\r' | grep -Eim1 '^([0-9a-f]{2}:){5}[0-9a-f]{2}$' || true
  )"

  printf '%s' "$candidate"
}

[ -n "$PHONE_DEVICE" ] || fail PHONE_DEVICE_REQUIRED
case "$RN3B_GATE_MODE" in
  reverse_proxy|local_wifi) ;;
  *) fail "RN3B_GATE_MODE_INVALID:$RN3B_GATE_MODE" ;;
esac
case "$COMPANION_TRIGGER_MODE" in
  bluetooth|activity) ;;
  *) fail "COMPANION_TRIGGER_MODE_INVALID:$COMPANION_TRIGGER_MODE" ;;
esac
case "$RESTART_COMPANION_SESSION" in
  true|TRUE|1|yes|YES|false|FALSE|0|no|NO) ;;
  *) fail "RESTART_COMPANION_SESSION_INVALID:$RESTART_COMPANION_SESSION" ;;
esac
case "$LOCAL_WIFI_WAIT_SECONDS" in
  ''|*[!0-9]*) fail "LOCAL_WIFI_WAIT_SECONDS_INVALID:$LOCAL_WIFI_WAIT_SECONDS" ;;
esac
[ "$LOCAL_WIFI_WAIT_SECONDS" -ge 30 ] || \
  fail "LOCAL_WIFI_WAIT_SECONDS_TOO_SMALL:$LOCAL_WIFI_WAIT_SECONDS"
case "$REVERSE_PROXY_WAIT_SECONDS" in
  ''|*[!0-9]*) fail "REVERSE_PROXY_WAIT_SECONDS_INVALID:$REVERSE_PROXY_WAIT_SECONDS" ;;
esac
[ "$REVERSE_PROXY_WAIT_SECONDS" -ge 30 ] || \
  fail "REVERSE_PROXY_WAIT_SECONDS_TOO_SMALL:$REVERSE_PROXY_WAIT_SECONDS"
case "$FIXED_LOHS_SSID" in
  ''|*[!A-Za-z0-9._-]*) fail "FIXED_LOHS_SSID_INVALID:$FIXED_LOHS_SSID" ;;
esac
[ "${#FIXED_LOHS_SSID}" -le 32 ] || fail FIXED_LOHS_SSID_TOO_LONG
case "$FIXED_LOHS_PSK" in
  *"'"*|*$'\n'*|*$'\r'*) fail FIXED_LOHS_PSK_INVALID ;;
esac
[ "${#FIXED_LOHS_PSK}" -ge 8 ] && [ "${#FIXED_LOHS_PSK}" -le 63 ] || \
  fail FIXED_LOHS_PSK_LENGTH_INVALID
case "$FIXED_LOHS_BAND" in 2|5|6|any) ;; *) fail "FIXED_LOHS_BAND_INVALID:$FIXED_LOHS_BAND" ;; esac
case "$FIXED_LOHS_WAIT_SECONDS" in
  ''|*[!0-9]*) fail "FIXED_LOHS_WAIT_SECONDS_INVALID:$FIXED_LOHS_WAIT_SECONDS" ;;
esac

"$ADB" start-server >/dev/null
"$ADB" -s "$AAOS_DEVICE" get-state >/dev/null 2>&1 || fail AAOS_ADB_NOT_REACHABLE
"$ADB" -s "$PHONE_DEVICE" get-state >/dev/null 2>&1 || fail PHONE_ADB_NOT_REACHABLE
"$ADB" -s "$AAOS_DEVICE" root >/dev/null 2>&1 || true
"$ADB" -s "$AAOS_DEVICE" wait-for-device

if [ "$PLATFORM_USER" = "auto" ]; then
  PLATFORM_USER="$("$ADB" -s "$AAOS_DEVICE" shell am get-current-user 2>/dev/null | tr -d '\r\n')"
fi
case "$PLATFORM_USER" in
  ''|*[!0-9]*) fail "PLATFORM_USER_INVALID:$PLATFORM_USER" ;;
esac
echo "ARDA_RN3B_PLATFORM_USER=$PLATFORM_USER"

# The demo owns transport only in the current foreground AAOS user. Stop any
# stale Platform instance in every other Android user before senderd is started.
# The final AOSP integration will replace this with one privileged singleUser
# core service in the system user.
for other_user in $(
  "$ADB" -s "$AAOS_DEVICE" shell pm list users 2>/dev/null |
    sed -n 's/.*UserInfo{\([0-9][0-9]*\):.*/\1/p'
); do
  [ "$other_user" = "$PLATFORM_USER" ] && continue
  "$ADB" -s "$AAOS_DEVICE" shell am force-stop \
    --user "$other_user" "$PLATFORM_PACKAGE" >/dev/null 2>&1 || true
  "$ADB" -s "$AAOS_DEVICE" shell am stopservice \
    --user "$other_user" \
    -n "$PLATFORM_PACKAGE/com.github.deadknight.ardaplatform.ArdaBootstrapServerService" \
    >/dev/null 2>&1 || true
  echo "ARDA_RN3B_NON_TARGET_PLATFORM_INSTANCE=STOPPED user=$other_user"
done

# APK ownership is external to this script. Verify only; never install/uninstall apps.
if ! "$ADB" -s "$AAOS_DEVICE" shell pm list packages --user "$PLATFORM_USER" 2>/dev/null | tr -d '\r' | grep -qx "package:$PLATFORM_PACKAGE"; then
  fail "PLATFORM_PACKAGE_NOT_INSTALLED_FOR_USER_${PLATFORM_USER}:$PLATFORM_PACKAGE"
fi
if ! "$ADB" -s "$PHONE_DEVICE" shell pm list packages 2>/dev/null | tr -d '\r' | grep -qx "package:$COMPANION_PACKAGE"; then
  fail "COMPANION_PACKAGE_NOT_INSTALLED:$COMPANION_PACKAGE"
fi

echo "ARDA_RN3B_APP_INSTALL_MODE=EXTERNAL_MANUAL"
echo "ARDA_RN3B_PLATFORM_PACKAGE_GATE=PASS"
echo "ARDA_RN3B_COMPANION_PACKAGE_GATE=PASS"

# Keep the validated MAP Client workaround active before Bluetooth comes up.
"$ADB" -s "$AAOS_DEVICE" shell setprop bluetooth.profile.map.client.enabled false
"$ADB" -s "$AAOS_DEVICE" shell cmd bluetooth_manager enable >/dev/null 2>&1 || true
"$ADB" -s "$PHONE_DEVICE" shell cmd bluetooth_manager enable >/dev/null 2>&1 || true
"$ADB" -s "$PHONE_DEVICE" shell svc wifi enable >/dev/null 2>&1 || true
"$ADB" -s "$PHONE_DEVICE" shell svc data enable >/dev/null 2>&1 || true

phone_sdk="$("$ADB" -s "$PHONE_DEVICE" shell getprop ro.build.version.sdk 2>/dev/null | tr -d '\r')"
case "$phone_sdk" in
  ''|*[!0-9]*) fail "PHONE_SDK_INVALID:$phone_sdk" ;;
esac
echo "ARDA_RN3B_PHONE_SDK=$phone_sdk"

for permission in \
  android.permission.BLUETOOTH_CONNECT \
  android.permission.BLUETOOTH_ADVERTISE \
  android.permission.POST_NOTIFICATIONS \
  android.permission.NEARBY_WIFI_DEVICES; do
  "$ADB" -s "$AAOS_DEVICE" shell pm grant --user "$PLATFORM_USER" "$PLATFORM_PACKAGE" "$permission" >/dev/null 2>&1 || true
done

phone_runtime_permissions=()
if [ "$phone_sdk" -ge 31 ]; then
  phone_runtime_permissions+=(
    android.permission.BLUETOOTH_CONNECT
    android.permission.BLUETOOTH_SCAN
  )
fi
if [ "$phone_sdk" -ge 33 ]; then
  phone_runtime_permissions+=(
    android.permission.POST_NOTIFICATIONS
    android.permission.NEARBY_WIFI_DEVICES
  )
else
  phone_runtime_permissions+=(android.permission.ACCESS_FINE_LOCATION)
fi

for permission in "${phone_runtime_permissions[@]}"; do
  "$ADB" -s "$PHONE_DEVICE" shell pm grant "$COMPANION_PACKAGE" "$permission" >/dev/null 2>&1 || true
done

phone_package_dump="$(
  "$ADB" -s "$PHONE_DEVICE" shell dumpsys package "$COMPANION_PACKAGE" 2>/dev/null | tr -d '\r'
)"
[ -n "$phone_package_dump" ] || fail PHONE_PACKAGE_DUMPSYS_EMPTY

phone_permission_is_declared() {
  local permission="$1"
  printf '%s\n' "$phone_package_dump" | awk -v permission="$permission" '
    {
      line=$0
      sub(/^[[:space:]]+/, "", line)
      sub(/[[:space:]]+$/, "", line)
      if (line == permission) found=1
    }
    END { exit(found ? 0 : 1) }
  '
}

phone_permission_is_granted() {
  local permission="$1"
  printf '%s\n' "$phone_package_dump" | awk -v permission="$permission" '
    index($0, permission ": granted=true") { found=1 }
    END { exit(found ? 0 : 1) }
  '
}

check_phone_declared_normal_permission() {
  local permission="$1"
  if phone_permission_is_declared "$permission"; then
    echo "ARDA_RN3B_PHONE_PERMISSION_${permission##*.}=DECLARED_NORMAL"
    return 0
  fi
  echo "ARDA_RN3B_PHONE_PERMISSION_${permission##*.}=NOT_DECLARED"
  return 1
}

check_phone_runtime_permission() {
  local permission="$1"
  if phone_permission_is_granted "$permission"; then
    echo "ARDA_RN3B_PHONE_PERMISSION_${permission##*.}=granted"
    return 0
  fi
  echo "ARDA_RN3B_PHONE_PERMISSION_${permission##*.}=denied"
  return 1
}

# BLUETOOTH is a normal install-time permission. On some Android 12 vendor
# Bluetooth stacks RFCOMM still checks it even when BLUETOOTH_CONNECT is granted.
# Verify that the installed APK declares it; cmd package check-permission is not
# available on every Android 12 build and must not be used as the gate.
check_phone_declared_normal_permission android.permission.BLUETOOTH ||   fail PHONE_LEGACY_BLUETOOTH_PERMISSION_NOT_DECLARED_IN_INSTALLED_APK
if [ "$phone_sdk" -ge 31 ]; then
  check_phone_runtime_permission android.permission.BLUETOOTH_CONNECT ||     fail PHONE_BLUETOOTH_CONNECT_PERMISSION_DENIED
  check_phone_runtime_permission android.permission.BLUETOOTH_SCAN ||     fail PHONE_BLUETOOTH_SCAN_PERMISSION_DENIED
fi

echo "ARDA_RN3B_PHONE_PERMISSION_GATE=PASS"

phone_bt="$("$ADB" -s "$PHONE_DEVICE" shell settings get global bluetooth_on 2>/dev/null | tr -d '\r')"
if [ "$phone_bt" != "1" ] && ! "$ADB" -s "$PHONE_DEVICE" shell dumpsys bluetooth_manager 2>/dev/null | grep -qE 'state: ON|mState=ON|enabled: true'; then
  fail PHONE_BLUETOOTH_MUST_BE_ON
fi

case "$BUILD_SENDERD" in
  true|TRUE|1|yes|YES)
    echo "ARDA_RN3B_SENDERD_BUILD=START"
    "$ROOT/scripts/docker-build.sh"
    echo "ARDA_RN3B_SENDERD_BUILD=PASS"
    ;;
  false|FALSE|0|no|NO)
    echo "ARDA_RN3B_SENDERD_BUILD=SKIPPED_USE_EXISTING_BINARY"
    ;;
  *)
    fail "BUILD_SENDERD_INVALID:$BUILD_SENDERD"
    ;;
esac

DEVICE="$AAOS_DEVICE" \
  "$ROOT/scripts/run-rn3b-hybrid-bridge-android.sh"

echo "ARDA_RN3B_SENDERD_DEPLOY_GATE=PASS"
echo "ARDA_RN3B_SENDERD_SESSION_GATE=WAITING_FOR_PLATFORM_AND_PHONE"

# PLATFORM_WIFI_INFO and phone HELLO are emitted only after both applications are
# running and the phone has connected over RFCOMM. Keep the existing Platform
# foreground service and its LocalOnlyHotspot reservation by default so the AP
# identity and Android approval remain reusable across repeated gate runs.
"$ADB" -s "$PHONE_DEVICE" logcat -c >/dev/null 2>&1 || true

platform_service_running=NO
if "$ADB" -s "$AAOS_DEVICE" shell \
    dumpsys activity services "$PLATFORM_PACKAGE" 2>/dev/null | \
    grep -q 'ArdaBootstrapServerService'; then
  platform_service_running=YES
fi

case "$RESTART_PLATFORM" in
  true|TRUE|1|yes|YES)
    "$ADB" -s "$AAOS_DEVICE" logcat -c >/dev/null 2>&1 || true
    "$ADB" -s "$AAOS_DEVICE" shell am force-stop --user "$PLATFORM_USER" "$PLATFORM_PACKAGE" || true
    stop_fixed_lohs
    platform_service_running=NO
    echo "ARDA_RN3B_PLATFORM_LIFECYCLE=FORCED_RESTART"
    ;;
  false|FALSE|0|no|NO)
    if [ "$platform_service_running" = "YES" ]; then
      echo "ARDA_RN3B_PLATFORM_LIFECYCLE=REUSE_RUNNING_SERVICE"
      echo "ARDA_RN3B_PLATFORM_HOTSPOT_RESERVATION=PRESERVED"
    else
      "$ADB" -s "$AAOS_DEVICE" logcat -c >/dev/null 2>&1 || true
      echo "ARDA_RN3B_PLATFORM_LIFECYCLE=START_NEW_SERVICE"
    fi
    ;;
  *) fail "RESTART_PLATFORM_INVALID:$RESTART_PLATFORM" ;;
esac

if ! fixed_lohs_alive && [ "$platform_service_running" = "YES" ]; then
  echo "ARDA_RN3B_PLATFORM_LIFECYCLE=RESTART_REQUIRED_FOR_FIXED_LOHS"
  "$ADB" -s "$AAOS_DEVICE" shell am force-stop --user "$PLATFORM_USER" "$PLATFORM_PACKAGE" || true
  platform_service_running=NO
fi
start_fixed_lohs
echo "ARDA_RN3B_FIXED_LOHS_SSID=$FIXED_LOHS_SSID"
echo "ARDA_RN3B_FIXED_LOHS_SECURITY=wpa2"
echo "ARDA_RN3B_FIXED_LOHS_GATE=PASS"

if [ "$platform_service_running" != "YES" ]; then
  "$ADB" -s "$AAOS_DEVICE" shell am start --user "$PLATFORM_USER" \
    -n "$PLATFORM_PACKAGE/com.github.deadknight.ardaplatform.MainActivity" \
    --ez arda_auto_start_bridge true \
    --ez arda_external_fixed_lohs true \
    --es arda_external_lohs_ssid "$FIXED_LOHS_SSID" \
    --es arda_external_lohs_psk "$FIXED_LOHS_PSK" \
    --es arda_external_lohs_security wpa2 >/dev/null
fi

echo "ARDA_RN3B_PLATFORM_STARTED=PASS"
echo "AAOS network ADB may now disappear because LocalOnlyHotspot owns Wi-Fi. USB ADB remains authoritative."

aaos_listener_ready=NO
if [ "$platform_service_running" = "YES" ]; then
  aaos_listener_ready=YES
  echo "ARDA_RN3B_AAOS_RFCOMM_LISTENER_REUSED=PASS"
else
  for _ in $(seq 1 45); do
    if "$ADB" -s "$AAOS_DEVICE" logcat -d -v brief -s ARDA_RN3B_AAOS:I '*:S' 2>/dev/null | \
        grep -q 'ARDA_RN3B_AAOS_RFCOMM_LISTEN=PASS'; then
      aaos_listener_ready=YES
      break
    fi
    sleep 1
  done
fi
if [ "$aaos_listener_ready" != "YES" ]; then
  "$ADB" -s "$AAOS_DEVICE" logcat -d -v threadtime -s ARDA_RN3B_AAOS:I '*:S' | tail -250 || true
  fail AAOS_RFCOMM_LISTENER_TIMEOUT
fi
echo "ARDA_RN3B_AAOS_RFCOMM_LISTENER_READY=PASS"

if [ -z "$AAOS_BLUETOOTH_ADDRESS" ]; then
  AAOS_BLUETOOTH_ADDRESS="$(resolve_aaos_bt_address)"
fi
if [ -n "$AAOS_BLUETOOTH_ADDRESS" ] && is_bt_address "$AAOS_BLUETOOTH_ADDRESS"; then
  AAOS_BLUETOOTH_ADDRESS="$(printf '%s' "$AAOS_BLUETOOTH_ADDRESS" | tr '[:lower:]' '[:upper:]')"
  echo "ARDA_RN3B_AAOS_BLUETOOTH_ADDRESS=$AAOS_BLUETOOTH_ADDRESS"
else
  AAOS_BLUETOOTH_ADDRESS=""
  echo "ARDA_RN3B_AAOS_BLUETOOTH_ADDRESS=AUTO_NOT_FOUND_APP_FALLBACK"
fi

case "$RESTART_COMPANION_SESSION" in
  true|TRUE|1|yes|YES)
    "$ADB" -s "$PHONE_DEVICE" shell am force-stop "$COMPANION_PACKAGE" || true
    echo "ARDA_RN3B_COMPANION_SESSION=FORCED_RESTART"
    ;;
  false|FALSE|0|no|NO)
    echo "ARDA_RN3B_COMPANION_SESSION=PRESERVE_IF_ACTIVE"
    ;;
esac

case "$COMPANION_TRIGGER_MODE" in
  bluetooth)
    # Production path: Android's ACL_CONNECTED / CompanionDeviceService callback
    # opens BluetoothBootstrapActivity. Do not bypass that path with adb am start.
    echo "ARDA_RN3B_COMPANION_TRIGGER=BLUETOOTH_BROADCAST"
    echo "Waiting for associated AAOS Bluetooth presence to open companion Activity..."
    ;;
  activity)
    # Explicit test-only escape hatch when the ACL link was already connected
    # before log capture or when validating without reconnecting Bluetooth.
    if [ -n "$AAOS_BLUETOOTH_ADDRESS" ]; then
      "$ADB" -s "$PHONE_DEVICE" shell am start \
        -n "$COMPANION_PACKAGE/com.github.deadknight.ardacompanion.BluetoothBootstrapActivity" \
        --ez arda_auto_connect true \
        --es arda_trigger_source GATE_ACTIVITY_TEST \
        --es bluetooth_address "$AAOS_BLUETOOTH_ADDRESS" >/dev/null
    else
      "$ADB" -s "$PHONE_DEVICE" shell am start \
        -n "$COMPANION_PACKAGE/com.github.deadknight.ardacompanion.BluetoothBootstrapActivity" \
        --ez arda_auto_connect true \
        --es arda_trigger_source GATE_ACTIVITY_TEST >/dev/null
    fi
    echo "ARDA_RN3B_COMPANION_TRIGGER=ACTIVITY_TEST_ONLY"
    ;;
esac

echo "ARDA_RN3B_COMPANION_TRIGGER_ARMED=PASS"
echo "Waiting for Platform Wi-Fi registration and Bluetooth HELLO gate..."

session_ready=NO
for _ in $(seq 1 60); do
  senderd_session_log="$(
    "$ADB" -s "$AAOS_DEVICE" shell       "cat /data/local/tmp/arda-rn3b-senderd.log 2>/dev/null || true" 2>/dev/null       | tr -d '\r'
  )"

  if printf '%s\n' "$senderd_session_log" | grep -q       'ARDA_RN3B_SENDERD_WIFI_LISTEN_GATE=PASS'; then
    printf '%s\n' "$senderd_session_log"
    fail SENDERD_ABANDONED_PHONE_HOTSPOT_BINARY_ACTIVE
  fi

  if printf '%s\n' "$senderd_session_log" | grep -q        'ARDA_RN3B_SENDERD_PLATFORM_WIFI_INFO=PASS' &&      printf '%s\n' "$senderd_session_log" | grep -q        'ARDA_RN3B_SENDERD_HELLO_ACK_TX=PASS'; then
    session_ready=YES
    break
  fi
  sleep 1
done

if [ "$session_ready" != "YES" ]; then
  "$ADB" -s "$AAOS_DEVICE" shell     cat /data/local/tmp/arda-rn3b-senderd.log 2>/dev/null | tr -d '\r' || true
  "$ADB" -s "$AAOS_DEVICE" logcat -d -v threadtime     -s ARDA_RN3B_AAOS:I '*:S' | tail -250 || true
  "$ADB" -s "$PHONE_DEVICE" logcat -d -v threadtime     -s ARDA_RN3B_PHONE:I '*:S' | tail -300 || true
  fail PLATFORM_WIFI_OR_HELLO_SESSION_TIMEOUT
fi

echo "ARDA_RN3B_SENDERD_PLATFORM_WIFI_INFO_GATE=PASS"
echo "ARDA_RN3B_SENDERD_HELLO_GATE=PASS"
echo "ARDA_RN3B_FULL_GATE_SENDERD_ARCHITECTURE=PASS"
echo "ARDA_RN3B_GATE_MODE=$RN3B_GATE_MODE"

if [ "$RN3B_GATE_MODE" = "local_wifi" ]; then
  echo "ARDA_RN3B_LOCAL_WIFI_WAIT_SECONDS=$LOCAL_WIFI_WAIT_SECONDS"
  echo "Waiting for phone saved-network autojoin gate..."

  for _ in $(seq 1 "$LOCAL_WIFI_WAIT_SECONDS"); do
    phone_gate_log="$(
      "$ADB" -s "$PHONE_DEVICE" logcat -d -v brief \
        -s ARDA_RN3B_PHONE:I '*:S' 2>/dev/null | tr -d '\r'
    )"
    if printf '%s\n' "$phone_gate_log" | grep -q \
        'ARDA_RN3B_PHONE_LOCAL_WIFI_GATE=PASS' && \
       printf '%s\n' "$phone_gate_log" | grep -q \
        'ARDA_RN3B_PHONE_LOCAL_ONLY_SESSION=PASS'; then
      printf '%s\n' "$phone_gate_log" | tail -120
      echo
      echo "ARDA_RN3B_LOCAL_WIFI_ONLY_GATE=PASS"
      exit 0
    fi
    sleep 1
  done

  "$ADB" -s "$AAOS_DEVICE" shell cat \
    /data/local/tmp/arda-rn3b-senderd.log 2>/dev/null | tr -d '\r' || true
  "$ADB" -s "$AAOS_DEVICE" logcat -d -v threadtime \
    -s ARDA_RN3B_AAOS:I '*:S' | tail -200 || true
  "$ADB" -s "$PHONE_DEVICE" logcat -d -v threadtime \
    -s ARDA_RN3B_PHONE:I '*:S' | tail -300 || true
  fail LOCAL_WIFI_ONLY_GATE_TIMEOUT
fi

echo "ARDA_RN3B_REVERSE_PROXY_WAIT_SECONDS=$REVERSE_PROXY_WAIT_SECONDS"
echo "Waiting for reverse proxy gate..."

for _ in $(seq 1 "$REVERSE_PROXY_WAIT_SECONDS"); do
  if "$ADB" -s "$AAOS_DEVICE" shell \
      "grep -q 'ARDA_RN3B_REVERSE_PROXY_GATE=PASS' /data/local/tmp/arda-rn3b-senderd.log 2>/dev/null"; then
    "$ADB" -s "$AAOS_DEVICE" shell cat /data/local/tmp/arda-rn3b-senderd.log | tr -d '\r'
    echo
    echo "ARDA_RN3B_FULL_REVERSE_PROXY_GATE=PASS"
    "$ADB" -s "$AAOS_DEVICE" shell settings get global http_proxy | tr -d '\r'
    exit 0
  fi
  sleep 1
done

"$ADB" -s "$AAOS_DEVICE" shell cat /data/local/tmp/arda-rn3b-senderd.log 2>/dev/null | tr -d '\r' || true
"$ADB" -s "$AAOS_DEVICE" logcat -d -v threadtime -s ARDA_RN3B_AAOS:I '*:S' | tail -200 || true
"$ADB" -s "$PHONE_DEVICE" logcat -d -v threadtime -s ARDA_RN3B_PHONE:I '*:S' | tail -300 || true
fail REVERSE_PROXY_GATE_TIMEOUT
