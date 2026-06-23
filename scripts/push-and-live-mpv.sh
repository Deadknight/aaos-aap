#!/usr/bin/env bash
set -euo pipefail

BIN="${BIN:-target/aarch64-linux-android/release/arda-senderd}"
ADDR="${ADDR:-192.168.1.15:5555}"
SIZE="${SIZE:-1280x720}"
BITRATE="${BITRATE:-6000000}"
SECONDS="${SECONDS:-180}"

adb connect "$ADDR" || true
adb push "$BIN" /data/local/tmp/arda-senderd
adb shell chmod 755 /data/local/tmp/arda-senderd

adb exec-out /data/local/tmp/arda-senderd capture-test \
  --size "$SIZE" \
  --bit-rate "$BITRATE" \
  --seconds "$SECONDS" \
  --out - \
  | mpv --demuxer=lavf --demuxer-lavf-format=h264 --profile=low-latency --untimed -
