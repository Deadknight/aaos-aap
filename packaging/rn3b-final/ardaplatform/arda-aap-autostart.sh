#!/system/bin/sh
set -u

BINARY="/system_ext/bin/arda-connectivityd"
CONFIG="/product/etc/arda/arda-aap.toml"
CERT_DIR="/data/local/tmp/arda-certs"
USB_NODE="/dev/usb_accessory"
LOG="/data/local/tmp/arda-aap-auto.log"
PID_FILE="/data/local/tmp/arda-aap-auto.pid"

mkdir -p /data/local/tmp
umask 077

log_marker() {
    marker="$1"
    echo "$marker" >> "$LOG"
    /system/bin/log -t ARDA_AAP_AUTOSTART "$marker" 2>/dev/null || true
}

cleanup() {
    rm -f "$PID_FILE"
}

trap cleanup EXIT

if [ -s "$PID_FILE" ]; then
    old_pid="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "$old_pid" ] && kill -0 "$old_pid" 2>/dev/null; then
        log_marker "ARDA_AAP_AUTOSTART=SKIP_ALREADY_RUNNING pid=$old_pid"
        exit 0
    fi
fi

echo "$$" > "$PID_FILE"

session_id="$(date +%s 2>/dev/null || echo unknown)"
log_marker "ARDA_AAP_AUTOSTART_BEGIN session=$session_id pid=$$ usb_state=$(getprop sys.usb.state) usb_config=$(getprop sys.usb.config)"

if [ ! -x "$BINARY" ]; then
    log_marker "ARDA_AAP_AUTOSTART=FAIL_BINARY_MISSING path=$BINARY"
    exit 2
fi

if [ ! -s "$CONFIG" ]; then
    log_marker "ARDA_AAP_AUTOSTART=FAIL_CONFIG_MISSING path=$CONFIG"
    exit 2
fi

if [ ! -s "$CERT_DIR/md_cert.pem" ] || [ ! -s "$CERT_DIR/md_key.pem" ]; then
    log_marker "ARDA_AAP_AUTOSTART=FAIL_TLS_FILES_MISSING dir=$CERT_DIR"
    exit 2
fi

# AOAP property notification and /dev/usb_accessory creation can be a few
# hundred milliseconds apart. Wait for the node rather than racing it.
wait_seconds=0
while [ ! -e "$USB_NODE" ] && [ "$wait_seconds" -lt 10 ]; do
    sleep 1
    wait_seconds=$((wait_seconds + 1))
done

if [ ! -e "$USB_NODE" ]; then
    log_marker "ARDA_AAP_AUTOSTART=FAIL_USB_NODE_TIMEOUT path=$USB_NODE wait_seconds=$wait_seconds"
    exit 2
fi

log_marker "ARDA_AAP_AUTOSTART_LAUNCH=YES config=$CONFIG usb_node=$USB_NODE wait_seconds=$wait_seconds"

"$BINARY" aap-run >> "$LOG" 2>&1
rc=$?

log_marker "ARDA_AAP_AUTOSTART_END session=$session_id rc=$rc usb_state=$(getprop sys.usb.state) usb_config=$(getprop sys.usb.config)"
exit "$rc"
