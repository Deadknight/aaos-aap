#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEVICE="${DEVICE:-1000000028c0b59c}"
ADB="${ADB:-adb}"
BIN="${ARDA_BIN:-$ROOT/target/aarch64-linux-android/release/arda-senderd}"
REMOTE_BIN="${REMOTE_BIN:-/data/local/tmp/arda-senderd-rn3b}"
REMOTE_LOG="${REMOTE_LOG:-/data/local/tmp/arda-rn3b-senderd.log}"
REMOTE_PID="${REMOTE_PID:-/data/local/tmp/arda-rn3b-senderd.pid}"
REMOTE_TOKEN_FILE="${REMOTE_TOKEN_FILE:-/data/local/tmp/arda-rn3b-bridge.token}"
BRIDGE_ADDR="${BRIDGE_ADDR:-127.0.0.1:5283}"
BRIDGE_TOKEN="${BRIDGE_TOKEN:-Arda-RN3B-Debug-Only-2026!}"
LOCAL_TOKEN_FILE="$(mktemp -t arda-rn3b-token.XXXXXX)"
trap 'rm -f "$LOCAL_TOKEN_FILE"' EXIT

fail() {
  echo "ARDA_RN3B_HYBRID_RUN_FAIL=$1" >&2
  exit 1
}

[ -f "$BIN" ] || fail "BINARY_NOT_FOUND:$BIN"

# Validate the generated artifact structurally. Do not grep Rust log strings from a
# stripped/optimized ELF: compiler string merging and host grep behavior make that
# check unreliable. The authoritative architecture/protocol gate is the runtime log
# emitted by this exact binary after it is deployed to AAOS below.
local_binary_info="$(file "$BIN" 2>/dev/null || true)"
echo "ARDA_RN3B_LOCAL_BINARY_FILE=$local_binary_info"
printf '%s\n' "$local_binary_info" | grep -Eiq   'ELF 64-bit.*(ARM aarch64|AArch64)' ||   fail "LOCAL_BINARY_NOT_AARCH64_ANDROID_ELF:$local_binary_info"

# Because the full gate may build immediately before this script, also verify that
# the checked-out source contains the current Platform Wi-Fi control protocol. This
# catches accidentally running the deploy script from an old source tree without
# relying on strings surviving linker optimization in the final ELF.
BLUETOOTH_SOURCE="${BLUETOOTH_SOURCE:-$ROOT/src/bluetooth/mod.rs}"
[ -f "$BLUETOOTH_SOURCE" ] || fail "BLUETOOTH_SOURCE_NOT_FOUND:$BLUETOOTH_SOURCE"
grep -Fq 'ARDA_RN3B_PROTOCOL_REV=AAOS_LOHS_CELLULAR_PROXY_V1' "$BLUETOOTH_SOURCE" ||   fail "SOURCE_MISSING_CURRENT_PROTOCOL_REV"
grep -Fq 'ARDA_RN3B_WIFI_ROLE=AAOS_LOCAL_ONLY_AP' "$BLUETOOTH_SOURCE" ||   fail "SOURCE_MISSING_AAOS_LOCAL_ONLY_AP_ROLE"
grep -Fq 'ARDA_RN3B_SENDERD_PLATFORM_WIFI_INFO=PASS' "$BLUETOOTH_SOURCE" ||   fail "SOURCE_MISSING_PLATFORM_WIFI_INFO_PROTOCOL"
grep -Fq 'ARDA_RN3B_PROXY_ROLE=PHONE_CELLULAR_HTTP_CONNECT_PROXY' "$BLUETOOTH_SOURCE" ||   fail "SOURCE_MISSING_PHONE_CELLULAR_PROXY_ROLE"

echo "ARDA_RN3B_LOCAL_SOURCE_PROTOCOL_GATE=PASS"
echo "ARDA_RN3B_LOCAL_BINARY_ARCHITECTURE_GATE=PASS"
echo "ARDA_RN3B_LOCAL_BINARY=$BIN"

printf '%s' "$BRIDGE_TOKEN" > "$LOCAL_TOKEN_FILE"

"$ADB" start-server >/dev/null
"$ADB" -s "$DEVICE" get-state >/dev/null 2>&1 || fail AAOS_ADB_NOT_REACHABLE
"$ADB" -s "$DEVICE" root >/dev/null 2>&1 || true
sleep 1
"$ADB" -s "$DEVICE" wait-for-device

"$ADB" -s "$DEVICE" push "$BIN" "$REMOTE_BIN" >/dev/null
"$ADB" -s "$DEVICE" push "$LOCAL_TOKEN_FILE" "$REMOTE_TOKEN_FILE" >/dev/null
"$ADB" -s "$DEVICE" shell chmod 0755 "$REMOTE_BIN"
"$ADB" -s "$DEVICE" shell chmod 0600 "$REMOTE_TOKEN_FILE"

# Stop only known senderd processes. Do not use `pkill -f` here: the remote
# adb shell command line itself contains "$REMOTE_BIN bt-bootstrapd", so a
# full-command-line match can kill the shell that is about to launch senderd.
remote_start_output="$(
  "$ADB" -s "$DEVICE" shell "
    old_pid=''
    if [ -f '$REMOTE_PID' ]; then
      old_pid=\$(cat '$REMOTE_PID' 2>/dev/null || true)
      if [ -n \"\$old_pid\" ]; then
        kill \"\$old_pid\" 2>/dev/null || true
      fi
    fi

    if command -v pidof >/dev/null 2>&1; then
      for candidate_pid in \$(pidof arda-senderd-rn3b 2>/dev/null || true); do
        if [ -n \"\$candidate_pid\" ] && [ \"\$candidate_pid\" != \"\$old_pid\" ]; then
          kill \"\$candidate_pid\" 2>/dev/null || true
        fi
      done
    fi

    sleep 1
    rm -f '$REMOTE_LOG' '$REMOTE_PID'
    nohup '$REMOTE_BIN' bt-bootstrapd \\
      --transport tcp \\
      --addr '$BRIDGE_ADDR' \\
      --bridge-token-file '$REMOTE_TOKEN_FILE' \\
      --keep-bootstrap-rfcomm-open \\
      --keep-bluetooth-profiles-connected \\
      >'$REMOTE_LOG' 2>&1 </dev/null &
    new_pid=\$!
    echo \"\$new_pid\" >'$REMOTE_PID'
    sleep 1
    kill -0 \"\$new_pid\" 2>/dev/null || {
      cat '$REMOTE_LOG' 2>/dev/null || true
      exit 1
    }
    echo ARDA_RN3B_REMOTE_SENDERD_STARTED=PASS
    echo ARDA_RN3B_REMOTE_SENDERD_PID=\$new_pid
  " | tr -d '\r'
)" || fail REMOTE_SENDERD_START_FAILED
printf '%s\n' "$remote_start_output"

ready=NO
for _ in $(seq 1 20); do
  remote_log="$(
    "$ADB" -s "$DEVICE" shell "cat '$REMOTE_LOG' 2>/dev/null || true" 2>/dev/null \
      | tr -d '\r'
  )"

  if printf '%s\n' "$remote_log" | grep -q 'ARDA_RN3B_SENDERD_WIFI_LISTEN_GATE=PASS'; then
    printf '%s\n' "$remote_log"
    fail REMOTE_BINARY_IS_ABANDONED_PHONE_HOTSPOT_BUILD
  fi

  # The exact runtime revision is authoritative. Source inspection alone cannot prove
  # that Cargo replaced an older target artifact, especially after archive restores
  # that preserve mtimes. Reject the pre-PLATFORM_WIFI_INFO binary before apps start.
  if printf '%s\n' "$remote_log" | grep -q 'ARDA_RN3B_SENDERD_BRIDGE_LISTEN_GATE=PASS'; then
    if printf '%s\n' "$remote_log" | grep -q \
        'ARDA_RN3B_PROTOCOL_REV=AAOS_LOHS_CELLULAR_PROXY_V1' && \
       printf '%s\n' "$remote_log" | grep -q \
        'ARDA_RN3B_WIFI_ROLE=AAOS_LOCAL_ONLY_AP' && \
       printf '%s\n' "$remote_log" | grep -q \
        'ARDA_RN3B_PROXY_ROLE=PHONE_CELLULAR_HTTP_CONNECT_PROXY'; then
      ready=YES
      break
    fi

    if printf '%s\n' "$remote_log" | grep -qE \
        'ARDA_RN3B_REMOTE_AUTH=DEFERRED_RN3B_TRANSPORT_GATE|ARDA_RN3B_WIFI_HANDOFF_CONFIG=DISABLED'; then
      printf '%s\n' "$remote_log"
      fail REMOTE_BINARY_STALE_PRE_PLATFORM_WIFI_INFO_REBUILD_REQUIRED
    fi
  fi
  sleep 1
done

"$ADB" -s "$DEVICE" shell "cat '$REMOTE_LOG' 2>/dev/null || true" | tr -d '\r'
[ "$ready" = YES ] || fail SENDERD_BRIDGE_LISTEN_TIMEOUT

echo
echo "ARDA_RN3B_SENDERD_BRIDGE_READY=PASS"
echo "ARDA_RN3B_SENDERD_SESSION_GATE=DEFERRED_UNTIL_APPS_START"
echo "ARDA_RN3B_BRIDGE_ADDR=$BRIDGE_ADDR"
echo "ARDA_RN3B_AAOS_APP_TOKEN=$BRIDGE_TOKEN"
echo
echo "Next on AAOS app:"
echo "  bridge host: 127.0.0.1"
echo "  bridge port: ${BRIDGE_ADDR##*:}"
echo "  bridge token: $BRIDGE_TOKEN"
echo "  Start ArdaPlatform: LocalOnlyHotspot + RFCOMM listener"
echo
echo "Then on phone companion:"
echo "  RFCOMM HELLO / PING -> WIFI_REQUEST"
echo "  join AAOS LocalOnlyHotspot"
echo "  expose cellular-bound HTTP/HTTPS CONNECT proxy to AAOS"
echo
echo "Collect senderd log:"
echo "  adb -s '$DEVICE' shell cat '$REMOTE_LOG'"
