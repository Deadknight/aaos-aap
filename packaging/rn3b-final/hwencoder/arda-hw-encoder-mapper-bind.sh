#!/system/bin/sh
set -u

SOURCE="/vendor/lib64/arda/libminigbm_gralloc_gbm_mesa.so"
TARGET="/apex/com.android.hardware.graphics.allocator.minigbm_gbm_mesa/lib64/libminigbm_gralloc_gbm_mesa.so"
LOG="/data/local/tmp/arda-hw-encoder-mapper-bind.log"
WAIT_SECONDS=30

mkdir -p /data/local/tmp
: > "$LOG"
chmod 0600 "$LOG"

log_marker() {
    marker="$1"
    echo "$marker" >> "$LOG"
    /system/bin/log -t ARDA_HW_ENCODER "$marker" 2>/dev/null || true
}

waited=0
while [ ! -s "$TARGET" ] && [ "$waited" -lt "$WAIT_SECONDS" ]; do
    sleep 1
    waited=$((waited + 1))
done

if [ ! -s "$SOURCE" ]; then
    log_marker "ARDA_HW_ENCODER_PERSIST_BIND=FAIL source_missing=$SOURCE"
    exit 2
fi

if [ ! -s "$TARGET" ]; then
    log_marker "ARDA_HW_ENCODER_PERSIST_BIND=FAIL target_missing=$TARGET waited=$waited"
    exit 2
fi

source_hash="$(sha256sum "$SOURCE" | awk '{print $1}')"
target_hash_before="$(sha256sum "$TARGET" | awk '{print $1}')"

if [ "$source_hash" = "$target_hash_before" ]; then
    log_marker "ARDA_HW_ENCODER_PERSIST_BIND=PASS_ALREADY_ACTIVE waited=$waited"
    exit 0
fi

# The source is vendor-owned and ABI-audited against the stock APEX library.
# Bind only the core minigbm library; allocator, SurfaceFlinger and Codec2 are
# not restarted. The V4L2 Codec2 process lazy-loads this library on first use.
umount "$TARGET" 2>/dev/null || true
mount --bind "$SOURCE" "$TARGET"

target_hash_after="$(sha256sum "$TARGET" | awk '{print $1}')"
if [ "$target_hash_after" != "$source_hash" ]; then
    log_marker "ARDA_HW_ENCODER_PERSIST_BIND=FAIL hash_mismatch"
    umount "$TARGET" 2>/dev/null || true
    exit 2
fi

log_marker "ARDA_HW_ENCODER_PERSIST_BIND=PASS source=$SOURCE target=$TARGET waited=$waited"
exit 0
