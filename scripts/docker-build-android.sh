#!/usr/bin/env bash
set -euo pipefail

export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-/opt/android-ndk-r26d}"
export ANDROID_API="${ANDROID_API:-23}"
export PATH="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"

TARGET="${TARGET:-aarch64-linux-android}"
FEATURES="${FEATURES:-}"

if [[ "$TARGET" == "aarch64-linux-android" ]]; then
  export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="aarch64-linux-android${ANDROID_API}-clang"
  export CXX_aarch64_linux_android="aarch64-linux-android${ANDROID_API}-clang++"
elif [[ "$TARGET" == "armv7-linux-androideabi" ]]; then
  export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="armv7a-linux-androideabi${ANDROID_API}-clang"
  export CXX_armv7_linux_androideabi="armv7a-linux-androideabi${ANDROID_API}-clang++"
fi

echo "Building Rust + embedded native C++ probe for $TARGET (ANDROID_API=$ANDROID_API)"

if [[ -n "$FEATURES" ]]; then
  cargo build --release --target "$TARGET" --features "$FEATURES"
else
  cargo build --release --target "$TARGET"
fi

OUT="target/$TARGET/release/arda-senderd"
file "$OUT" || true
ls -lh "$OUT"
