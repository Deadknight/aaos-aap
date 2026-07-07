#!/usr/bin/env bash
set -euo pipefail

export ANDROID_HOME="${ANDROID_HOME:-/opt/android-sdk}"
export ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-/opt/android-ndk-r26d}"
export ANDROID_API="${ANDROID_API:-23}"
export ANDROID_BUILD_TOOLS="${ANDROID_BUILD_TOOLS:-35.0.0}"
export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/build-tools/$ANDROID_BUILD_TOOLS:$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin:$PATH"

TARGET="${TARGET:-aarch64-linux-android}"
FEATURES="${FEATURES:-}"

if [[ "$TARGET" == "aarch64-linux-android" ]]; then
  export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="aarch64-linux-android${ANDROID_API}-clang"
  export CC_aarch64_linux_android="aarch64-linux-android${ANDROID_API}-clang"
  export CXX_aarch64_linux_android="aarch64-linux-android${ANDROID_API}-clang++"
  export AR_aarch64_linux_android="llvm-ar"
elif [[ "$TARGET" == "armv7-linux-androideabi" ]]; then
  export CARGO_TARGET_ARMV7_LINUX_ANDROIDEABI_LINKER="armv7a-linux-androideabi${ANDROID_API}-clang"
  export CC_armv7_linux_androideabi="armv7a-linux-androideabi${ANDROID_API}-clang"
  export CXX_armv7_linux_androideabi="armv7a-linux-androideabi${ANDROID_API}-clang++"
  export AR_armv7_linux_androideabi="llvm-ar"
fi

if [[ -z "${ANDROID_JAR:-}" ]]; then
  ANDROID_JAR="$(find "$ANDROID_HOME/platforms" -name android.jar | sort -V | tail -n 1)"
  export ANDROID_JAR
fi

if [[ -z "${D8:-}" ]]; then
  D8="$(find "$ANDROID_HOME/build-tools" -name d8 | sort -V | tail -n 1)"
  export D8
fi

echo "Building Rust + embedded native C++ probe + app_process inputd for $TARGET (ANDROID_API=$ANDROID_API)"
echo "ANDROID_JAR=$ANDROID_JAR"
echo "D8=$D8"

# Patch/checkpoint archives are commonly applied with `rsync -av`, which preserves
# source mtimes. Cargo's build-script rerun tracking is mtime-based, so an older
# restored build.rs can otherwise leave a stale OUT_DIR/protos snapshot in target/.
# Clean only this package for the selected Android target; dependency artifacts stay
# cached, while build.rs/codegen/native/inputd are guaranteed to reflect this tree.
echo "Refreshing arda-senderd target artifacts before codegen"
cargo clean -p arda-senderd --target "$TARGET"

if [[ -n "$FEATURES" ]]; then
  cargo build --release --target "$TARGET" --features "$FEATURES"
else
  cargo build --release --target "$TARGET"
fi

OUT="target/$TARGET/release/arda-senderd"
file "$OUT" || true
ls -lh "$OUT"
