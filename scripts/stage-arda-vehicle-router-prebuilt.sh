#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SOURCE_BIN="${1:-${ARDA_ROUTER_BINARY:-$REPO_ROOT/target/aarch64-linux-android/release/arda-senderd}}"
VEHICLE_SOURCE="${ARDA_VEHICLE_SOURCE:-}"

if [[ -z "$VEHICLE_SOURCE" ]]; then
  for candidate in \
    "$REPO_ROOT/../android_vendor_arda/vehicle" \
    "/tmp/arda-android_vendor_arda-inspect/vehicle"
  do
    if [[ -d "$candidate" ]]; then
      VEHICLE_SOURCE="$candidate"
      break
    fi
  done
fi

if [[ -z "$VEHICLE_SOURCE" || ! -d "$VEHICLE_SOURCE" ]]; then
  echo "ARDA_ROUTER_PREBUILT_VEHICLE_SOURCE_MISSING source=${VEHICLE_SOURCE:-<unset>}" >&2
  echo "Set ARDA_VEHICLE_SOURCE=/path/to/android_vendor_arda/vehicle." >&2
  exit 2
fi

VEHICLE_SOURCE="$(cd "$VEHICLE_SOURCE" && pwd)"
DEST_BIN="${ARDA_ROUTER_PREBUILT_DEST:-$VEHICLE_SOURCE/router-service/prebuilt/arda-senderd}"

if [[ ! -f "$SOURCE_BIN" || ! -s "$SOURCE_BIN" ]]; then
  echo "ARDA_ROUTER_PREBUILT_SOURCE_MISSING path=$SOURCE_BIN" >&2
  exit 2
fi

if [[ ! -x "$SOURCE_BIN" ]]; then
  echo "ARDA_ROUTER_PREBUILT_SOURCE_NOT_EXECUTABLE path=$SOURCE_BIN" >&2
  exit 2
fi

if ! command -v file >/dev/null 2>&1; then
  echo "ARDA_ROUTER_PREBUILT_FILE_TOOL_MISSING" >&2
  exit 2
fi

FILE_INFO="$(file "$SOURCE_BIN")"
printf '%s\n' "$FILE_INFO"

if [[ "$FILE_INFO" != *"ELF 64-bit"* \
      || "$FILE_INFO" != *"ARM aarch64"* \
      || "$FILE_INFO" != *"/system/bin/linker64"* ]]; then
  echo "ARDA_ROUTER_PREBUILT_ARCH_INVALID path=$SOURCE_BIN" >&2
  exit 2
fi

mkdir -p "$(dirname "$DEST_BIN")"
install -m 0755 "$SOURCE_BIN" "$DEST_BIN"

if command -v sha256sum >/dev/null 2>&1; then
  SHA256="$(sha256sum "$DEST_BIN" | awk '{print $1}')"
else
  SHA256="$(shasum -a 256 "$DEST_BIN" | awk '{print $1}')"
fi

printf 'ARDA_ROUTER_PREBUILT_SOURCE=%s\n' "$SOURCE_BIN"
printf 'ARDA_ROUTER_PREBUILT_DEST=%s\n' "$DEST_BIN"
printf 'ARDA_ROUTER_PREBUILT_SHA256=%s\n' "$SHA256"
printf 'ARDA_ROUTER_PREBUILT_STAGE=PASS\n'
