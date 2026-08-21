#!/system/bin/sh
set -u

BINARY="/system_ext/bin/arda-connectivityd"
CONFIG="/product/etc/arda/arda-aap.toml"
PLATFORM_PACKAGE="com.github.deadknight.ardaplatform.debug"
VIDEO_OVERRIDE_CONFIG="/data/user_de/0/$PLATFORM_PACKAGE/files/arda-aap-video.toml"
DEFAULT_VIDEO_POLICY="balanced"
DEFAULT_VIDEO_CODEC="c2.v4l2.avc.encoder"
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

video_policy="$DEFAULT_VIDEO_POLICY"
forced_resolution=""
if [ -s "$VIDEO_OVERRIDE_CONFIG" ]; then
    forced_resolution="$(
        sed -n 's/^[[:space:]]*force_resolution[[:space:]]*=[[:space:]]*"\([^"]*\)"[[:space:]]*$/\1/p' \
            "$VIDEO_OVERRIDE_CONFIG" | tail -n 1
    )"
    auto_policy="$(
        sed -n 's/^[[:space:]]*auto_policy[[:space:]]*=[[:space:]]*"\([^"]*\)"[[:space:]]*$/\1/p' \
            "$VIDEO_OVERRIDE_CONFIG" | tail -n 1
    )"
    if [ -n "$auto_policy" ]; then
        video_policy="$auto_policy"
    fi
fi
if [ -n "$forced_resolution" ]; then
    video_policy="resolution:$forced_resolution"
fi

video_codec_reason="EXPLICIT_ENV"
if [ -n "${ARDA_SCREENRECORD_CODEC:-}" ]; then
    video_codec="$ARDA_SCREENRECORD_CODEC"
else
    video_codec="$DEFAULT_VIDEO_CODEC"
    video_codec_reason="PRODUCTION_HWC_DMABUF_DEFAULT"
fi
log_marker "ARDA_AAP_AUTOSTART_LAUNCH=YES config=$CONFIG video_override=$VIDEO_OVERRIDE_CONFIG video_policy=$video_policy video_codec=$video_codec video_codec_reason=$video_codec_reason usb_node=$USB_NODE wait_seconds=$wait_seconds"

ARDA_PLATFORM_PACKAGE="$PLATFORM_PACKAGE" \
ARDA_SCREENRECORD_CODEC="$video_codec" \
"$BINARY" aap-run --video-config-policy "$video_policy" >> "$LOG" 2>&1
rc=$?

log_marker "ARDA_AAP_AUTOSTART_END session=$session_id rc=$rc usb_state=$(getprop sys.usb.state) usb_config=$(getprop sys.usb.config)"
exit "$rc"
