#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  AOSP_ROOT=/path/to/aosp \
  ARDA_VEHICLE_SOURCE=/path/to/android_vendor_arda/vehicle \
  PRODUCT_MK=device/.../product.mk \
  BOARD_CONFIG=device/.../BoardConfig.mk \
    ./scripts/apply-arda-vehicle-phase2-aosp.sh

The canonical ARDA Vehicle source is Deadknight/android_vendor_arda under vehicle/.
The complete Vehicle tree is copied to vendor/arda/vehicle by default.
ARDA_VEHICLE_SOURCE is optional only when a supported sibling or /tmp vendor clone is present.
The staged Android ARM64 router binary is required before copy.
PRODUCT_MK is optional and receives one canonical Vehicle product inherit.
BOARD_CONFIG is optional and receives one canonical system_ext private SEPolicy path.
The smoke adapter remains opt-in through arda_vehicle_smoke.mk.
USAGE
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
AOSP_ROOT="${AOSP_ROOT:-}"
SOURCE="${ARDA_VEHICLE_SOURCE:-}"

if [[ -z "$SOURCE" ]]; then
  for candidate in \
    "$REPO_ROOT/../android_vendor_arda/vehicle" \
    "/tmp/arda-android_vendor_arda-inspect/vehicle"
  do
    if [[ -d "$candidate" ]]; then
      SOURCE="$candidate"
      break
    fi
  done
fi
PRODUCT_MK="${PRODUCT_MK:-}"
BOARD_CONFIG="${BOARD_CONFIG:-}"
DEST_REL="${ARDA_VEHICLE_AOSP_DEST:-vendor/arda/vehicle}"
PREBUILT_REL="router-service/prebuilt/arda-senderd"

if [[ -z "$AOSP_ROOT" ]]; then
  usage >&2
  exit 2
fi

if [[ ! -d "$AOSP_ROOT/build/soong" ]]; then
  echo "ARDA_PHASE2_AOSP_ROOT_INVALID root=$AOSP_ROOT" >&2
  exit 2
fi

if [[ -z "$SOURCE" || ! -d "$SOURCE" ]]; then
  echo "ARDA_PHASE2_SOURCE_MISSING source=${SOURCE:-<unset>}" >&2
  echo "Set ARDA_VEHICLE_SOURCE=/path/to/android_vendor_arda/vehicle." >&2
  exit 2
fi

SOURCE="$(cd "$SOURCE" && pwd)"

cmp -s \
  "$REPO_ROOT/proto/arda_vehicle_router.proto" \
  "$SOURCE/router-proto/arda_vehicle_router.proto" \
  || {
    echo "ARDA_PHASE2_ROUTER_PROTO_DRIFT=YES" >&2
    exit 2
  }

echo "ARDA_PHASE2_ROUTER_PROTO_SYNC=YES"

PREBUILT="$SOURCE/$PREBUILT_REL"
if [[ ! -f "$PREBUILT" || ! -s "$PREBUILT" || ! -x "$PREBUILT" ]]; then
  echo "ARDA_PHASE2_ROUTER_PREBUILT_MISSING path=$PREBUILT" >&2
  echo "Run scripts/stage-arda-vehicle-router-prebuilt.sh after the Android Docker Rust build." >&2
  exit 2
fi

if ! command -v file >/dev/null 2>&1; then
  echo "ARDA_PHASE2_FILE_TOOL_MISSING" >&2
  exit 2
fi

PREBUILT_INFO="$(file "$PREBUILT")"
printf '%s\n' "$PREBUILT_INFO"
if [[ "$PREBUILT_INFO" != *"ELF 64-bit"* \
      || "$PREBUILT_INFO" != *"ARM aarch64"* \
      || "$PREBUILT_INFO" != *"/system/bin/linker64"* ]]; then
  echo "ARDA_PHASE2_ROUTER_PREBUILT_ARCH_INVALID path=$PREBUILT" >&2
  exit 2
fi

echo "ARDA_PHASE2_ROUTER_PREBUILT_OK=YES"

DEST="$AOSP_ROOT/$DEST_REL"
LEGACY_DEST="$AOSP_ROOT/packages/apps/ArdaVehicle"

DEST_PARENT="$(mkdir -p "$(dirname "$DEST")" && cd "$(dirname "$DEST")" && pwd)"
DEST_ABS="$DEST_PARENT/$(basename "$DEST")"

if [[ "$SOURCE" = "$DEST_ABS" ]]; then
  echo "ARDA_PHASE2_SOURCE_EQUALS_DEST source=$SOURCE dest=$DEST_ABS" >&2
  echo "Use the canonical android_vendor_arda/vehicle checkout as ARDA_VEHICLE_SOURCE." >&2
  exit 2
fi

if [[ "$DEST_REL" != "packages/apps/ArdaVehicle" \
      && -f "$LEGACY_DEST/manager/Android.bp" ]] \
    && grep -Fq 'name: "ArdaVehicleManager"' "$LEGACY_DEST/manager/Android.bp"; then
  rm -rf "$LEGACY_DEST"
  echo "ARDA_PHASE2_LEGACY_PACKAGES_APPS_REMOVED=YES path=$LEGACY_DEST"
fi

mkdir -p "$DEST"
rsync -a --delete --exclude='.git/' "$SOURCE/" "$DEST/"

echo "ARDA_PHASE2_AOSP_COPY_OK source=$SOURCE dest=$DEST"

for required in \
  aidl/Android.bp \
  sdk/Android.bp \
  manager/Android.bp \
  manager/AndroidManifest.xml \
  router-proto/Android.bp \
  router-service/Android.bp \
  router-service/arda-vehicle-routerd.rc \
  router-service/prebuilt/arda-senderd \
  sepolicy/private/arda_vehicle_manager.te \
  sepolicy/private/arda_vehicle_router.te \
  sepolicy/private/file_contexts \
  sepolicy/private/seapp_contexts
  do
  if [[ ! -e "$DEST/$required" ]]; then
    echo "ARDA_PHASE2_DEST_REQUIRED_MISSING path=$DEST/$required" >&2
    exit 2
  fi
done

echo "ARDA_PHASE2_DEST_TREE_GATE=PASS"

if [[ -n "$PRODUCT_MK" ]]; then
  if [[ "$PRODUCT_MK" != /* ]]; then
    PRODUCT_MK="$AOSP_ROOT/$PRODUCT_MK"
  fi
  if [[ ! -f "$PRODUCT_MK" ]]; then
    echo "ARDA_PHASE2_PRODUCT_MK_MISSING path=$PRODUCT_MK" >&2
    exit 2
  fi
  python3 - "$PRODUCT_MK" <<'PY'
from pathlib import Path
import re
import sys

path = Path(sys.argv[1])
text = path.read_text()

text = re.sub(
    r"\n?# BEGIN ARDA VEHICLE PHASE2\n.*?# END ARDA VEHICLE PHASE2\n?",
    "\n",
    text,
    flags=re.DOTALL,
)

text = re.sub(
    r"\n?# ARDA Vehicle Manager\n"
    r"\$\(call inherit-product-if-exists, vendor/arda/vehicle/arda_vehicle\.mk\)\n?",
    "\n",
    text,
)

block = """# ARDA Vehicle Manager
$(call inherit-product-if-exists, vendor/arda/vehicle/arda_vehicle.mk)
"""
gas_marker = "# ARDA_GAS_STAGE1_BEGIN"
if gas_marker in text:
    text = text.replace(gas_marker, block + "\n" + gas_marker, 1)
else:
    text = text.rstrip() + "\n\n" + block

path.write_text(text)
PY
  echo "ARDA_PHASE2_PRODUCT_INHERIT_OK file=$PRODUCT_MK"
fi

if [[ -n "$BOARD_CONFIG" ]]; then
  if [[ "$BOARD_CONFIG" != /* ]]; then
    BOARD_CONFIG="$AOSP_ROOT/$BOARD_CONFIG"
  fi
  if [[ ! -f "$BOARD_CONFIG" ]]; then
    echo "ARDA_PHASE2_BOARD_CONFIG_MISSING path=$BOARD_CONFIG" >&2
    exit 2
  fi
  python3 - "$BOARD_CONFIG" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
marker = "# ARDA Vehicle platform policy"
policy_line = "SYSTEM_EXT_PRIVATE_SEPOLICY_DIRS += vendor/arda/vehicle/sepolicy/private"
lines = path.read_text().splitlines()
lines = [line for line in lines if line.strip() not in {marker, policy_line}]
text = "\n".join(lines).rstrip()
if text:
    text += "\n\n"
text += marker + "\n" + policy_line + "\n"
path.write_text(text)
PY

  POLICY_COUNT="$(grep -Fxc 'SYSTEM_EXT_PRIVATE_SEPOLICY_DIRS += vendor/arda/vehicle/sepolicy/private' "$BOARD_CONFIG")"
  if [[ "$POLICY_COUNT" != "1" ]]; then
    echo "ARDA_PHASE2_SEPOLICY_DIR_COUNT_INVALID count=$POLICY_COUNT file=$BOARD_CONFIG" >&2
    exit 2
  fi
  echo "ARDA_PHASE2_SEPOLICY_INHERIT_OK file=$BOARD_CONFIG"
fi

cat <<EOF2
ARDA_PHASE2_2_APPLY_OK=YES
AOSP_DEST=$DEST_REL
NEXT_BUILD_COMMANDS:
  source build/envsetup.sh
  lunch aosp_rpi4_car-bp4a-userdebug
  m arda-vehicle-routerd ArdaVehicleManager ArdaVehicleSmokeAdapter
  m systemextimage
EOF2
