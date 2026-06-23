#!/usr/bin/env bash
set -euo pipefail

# Android NDK linux.zip ships x86_64 Linux toolchains. On Apple Silicon,
# Docker would otherwise build/run an arm64 Linux container and the NDK clang
# wrapper fails with: rosetta error: failed to open elf /lib64/ld-linux-x86-64.so.2
PLATFORM="${PLATFORM:-linux/amd64}"
IMAGE="${IMAGE:-arda-android-builder:latest}"

docker buildx build \
  --platform "$PLATFORM" \
  --load \
  -t "$IMAGE" \
  -f Dockerfile.android \
  .

docker run --rm \
  --platform "$PLATFORM" \
  -v "$PWD":/work \
  "$IMAGE"
