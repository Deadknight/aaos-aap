#!/system/bin/sh
set -u

BINARY="/system_ext/bin/arda-connectivityd"
CONFIG="/product/etc/arda/arda-aap.toml"
PLATFORM_PACKAGE="com.github.deadknight.ardaplatform.debug"
VIDEO_OVERRIDE_CONFIG="/data/user_de/0/$PLATFORM_PACKAGE/files/arda-aap-video.toml"
DEFAULT_VIDEO_POLICY="balanced"
DEFAULT_VIDEO_HW_PROFILE="auto"
DEFAULT_VIDEO_FPS_MODE="auto"
PI4_OPTIMAL_FPS_CAP=30
PI5_OPTIMAL_FPS_CAP=60
PI4_DEFAULT_VIDEO_CODEC="c2.v4l2.avc.encoder"
PI5_DEFAULT_VIDEO_CODEC="c2.android.avc.encoder"
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

# ARDA_AAP_AUTOSTART_ACTIVE_CERTS_V1
# Mirror native aap_key_update::resolve_keys_dir(): prefer a complete active/
# pair; otherwise preserve compatibility with the legacy direct pair.
CERT_KEYS_DIR="$CERT_DIR"
if [ -s "$CERT_DIR/active/md_cert.pem" ] && [ -s "$CERT_DIR/active/md_key.pem" ]; then
    CERT_KEYS_DIR="$CERT_DIR/active"
fi

if [ ! -s "$CERT_KEYS_DIR/md_cert.pem" ] || [ ! -s "$CERT_KEYS_DIR/md_key.pem" ]; then
    log_marker "ARDA_AAP_AUTOSTART=FAIL_TLS_FILES_MISSING dir=$CERT_KEYS_DIR"
    exit 2
fi

log_marker "ARDA_AAP_AUTOSTART_TLS_KEYS=PASS dir=$CERT_KEYS_DIR"

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

normalize_hw_profile() {
    case "${1:-}" in
        pi4_hdmi|pi5_hwc_vds|auto)
            echo "$1"
            ;;
        *)
            echo "$DEFAULT_VIDEO_HW_PROFILE"
            ;;
    esac
}

resolve_hw_profile() {
    requested="$1"
    if [ "$requested" != "auto" ]; then
        echo "$requested"
        return
    fi

    hardware="$(getprop ro.hardware 2>/dev/null || true)"
    board="$(getprop ro.product.board 2>/dev/null || true)"

    case "$hardware:$board" in
        *rpi4*|*bcm2711*)
            echo "pi4_hdmi"
            ;;
        *rpi5*|*bcm2712*)
            echo "pi5_hwc_vds"
            ;;
        *)
            echo "pi5_hwc_vds"
            ;;
    esac
}

normalize_fps_mode() {
    case "${1:-}" in
        auto|optimal|10|15|30|40|60)
            echo "$1"
            ;;
        *)
            echo "$DEFAULT_VIDEO_FPS_MODE"
            ;;
    esac
}

requested_hw_profile="$(
    /system/bin/settings get global arda_video_hw_profile 2>/dev/null || echo null
)"
[ "$requested_hw_profile" = "null" ] && requested_hw_profile=""
requested_hw_profile="$(normalize_hw_profile "$requested_hw_profile")"
resolved_hw_profile="$(resolve_hw_profile "$requested_hw_profile")"

video_fps_mode="$(
    /system/bin/settings get global arda_video_fps_cap 2>/dev/null || echo null
)"
[ "$video_fps_mode" = "null" ] && video_fps_mode=""
video_fps_mode="$(normalize_fps_mode "$video_fps_mode")"

case "$video_fps_mode" in
    auto)
        video_fps_cap=60
        video_fps_reason="HU_SDR_NATIVE_UP_TO_ENGINE_60"
        ;;
    optimal)
        if [ "$resolved_hw_profile" = "pi4_hdmi" ]; then
            video_fps_cap="$PI4_OPTIMAL_FPS_CAP"
        else
            video_fps_cap="$PI5_OPTIMAL_FPS_CAP"
        fi
        video_fps_reason="BOARD_OPTIMAL"
        ;;
    10|15|30|40|60)
        video_fps_cap="$video_fps_mode"
        video_fps_reason="USER_CAP"
        ;;
esac

case "$resolved_hw_profile" in
    pi4_hdmi)
        hwc_vds=0
        default_profile_codec="$PI4_DEFAULT_VIDEO_CODEC"
        ;;
    pi5_hwc_vds)
        hwc_vds=1
        default_profile_codec="$PI5_DEFAULT_VIDEO_CODEC"
        ;;
esac

setprop vendor.hwc.drm.enable_virtual_display "$hwc_vds"
service call SurfaceFlinger 1021 i32 0 >/dev/null 2>&1 || true
sleep 1
service call SurfaceFlinger 1021 i32 1 >/dev/null 2>&1 || true

log_marker "ARDA_VIDEO_HW_PROFILE requested=$requested_hw_profile resolved=$resolved_hw_profile hardware=$(getprop ro.hardware) board=$(getprop ro.product.board) hwc_vds=$hwc_vds active_route=$(getprop sys.arda.video_route.active)"
log_marker "ARDA_VIDEO_FPS_MODE mode=$video_fps_mode cap=$video_fps_cap reason=$video_fps_reason"

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
    video_codec="$default_profile_codec"
    video_codec_reason="HW_PROFILE_DEFAULT"
fi
log_marker "ARDA_AAP_AUTOSTART_LAUNCH=YES config=$CONFIG video_override=$VIDEO_OVERRIDE_CONFIG video_policy=$video_policy video_hw_requested=$requested_hw_profile video_hw_resolved=$resolved_hw_profile video_fps_mode=$video_fps_mode video_fps_cap=$video_fps_cap video_codec=$video_codec video_codec_reason=$video_codec_reason usb_node=$USB_NODE wait_seconds=$wait_seconds"

ARDA_PLATFORM_PACKAGE="$PLATFORM_PACKAGE" \
ARDA_SCREENRECORD_CODEC="$video_codec" \
"$BINARY" aap-run \
    --video-config-policy "$video_policy" \
    --video-fps "$video_fps_cap" \
    >> "$LOG" 2>&1
rc=$?

log_marker "ARDA_AAP_AUTOSTART_END session=$session_id rc=$rc usb_state=$(getprop sys.usb.state) usb_config=$(getprop sys.usb.config)"
exit "$rc"
