#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  AOSP_ROOT=/path/to/aosp \
  ARDA_LOCATION_SOURCE=/path/to/android_vendor_arda/location_provider \
  PRODUCT_MK=device/brcm/rpi4/aosp_rpi4_car.mk \
  BOARD_CONFIG=device/brcm/rpi4/BoardConfig.mk \
    ./scripts/apply-arda-location-phase3-aosp.sh

ARDA_LOCATION_SOURCE is optional when ../android_vendor_arda/location_provider exists.
PRODUCT_MK and BOARD_CONFIG are auto-detected for the RPi4 tree when omitted.
The script copies the ARDA provider into vendor/arda/location_provider, adds product/SEPolicy integration,
and narrowly patches LocationManagerService so a configured GNSS provider override can become GPS_PROVIDER
even when no hardware GNSS HAL is available.
USAGE
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
AOSP_ROOT="${AOSP_ROOT:-}"
SOURCE="${ARDA_LOCATION_SOURCE:-}"
PRODUCT_MK="${PRODUCT_MK:-}"
BOARD_CONFIG="${BOARD_CONFIG:-}"
DEST_REL="${ARDA_LOCATION_AOSP_DEST:-vendor/arda/location_provider}"
LMS_REL="frameworks/base/services/core/java/com/android/server/location/LocationManagerService.java"

if [[ -z "$AOSP_ROOT" ]]; then
  usage >&2
  exit 2
fi

if [[ ! -d "$AOSP_ROOT/build/soong" || ! -f "$AOSP_ROOT/$LMS_REL" ]]; then
  echo "ARDA_PHASE3_0B_AOSP_ROOT_INVALID root=$AOSP_ROOT" >&2
  exit 2
fi

if [[ -z "$SOURCE" ]]; then
  for candidate in \
    "$REPO_ROOT/../android_vendor_arda/location_provider" \
    "/tmp/arda-location-provider"
  do
    if [[ -d "$candidate" ]]; then
      SOURCE="$candidate"
      break
    fi
  done
fi

if [[ -z "$SOURCE" || ! -d "$SOURCE" ]]; then
  echo "ARDA_PHASE3_0B_SOURCE_MISSING source=${SOURCE:-<unset>}" >&2
  echo "Set ARDA_LOCATION_SOURCE=/path/to/android_vendor_arda/location_provider." >&2
  exit 2
fi

SOURCE="$(cd "$SOURCE" && pwd)"

if [[ -z "$PRODUCT_MK" ]]; then
  for candidate in \
    device/brcm/rpi4/aosp_rpi4_car.mk \
    device/brcm/rpi4/aosp_rpi4.mk
  do
    if [[ -f "$AOSP_ROOT/$candidate" ]]; then
      PRODUCT_MK="$candidate"
      break
    fi
  done
fi

if [[ -z "$BOARD_CONFIG" && -f "$AOSP_ROOT/device/brcm/rpi4/BoardConfig.mk" ]]; then
  BOARD_CONFIG="device/brcm/rpi4/BoardConfig.mk"
fi

if [[ -z "$PRODUCT_MK" || -z "$BOARD_CONFIG" ]]; then
  echo "ARDA_PHASE3_0B_DEVICE_INTEGRATION_PATH_MISSING product=${PRODUCT_MK:-<unset>} board=${BOARD_CONFIG:-<unset>}" >&2
  echo "Set PRODUCT_MK and BOARD_CONFIG explicitly." >&2
  exit 2
fi

if [[ "$PRODUCT_MK" != /* ]]; then
  PRODUCT_MK="$AOSP_ROOT/$PRODUCT_MK"
fi
if [[ "$BOARD_CONFIG" != /* ]]; then
  BOARD_CONFIG="$AOSP_ROOT/$BOARD_CONFIG"
fi

if [[ ! -f "$PRODUCT_MK" || ! -f "$BOARD_CONFIG" ]]; then
  echo "ARDA_PHASE3_0B_DEVICE_INTEGRATION_FILE_MISSING product=$PRODUCT_MK board=$BOARD_CONFIG" >&2
  exit 2
fi

for required in \
  Android.bp \
  AndroidManifest.xml \
  arda_location.mk \
  overlay/frameworks/base/core/res/res/values/config.xml \
  sepolicy/private/arda_location_provider.te \
  sepolicy/private/seapp_contexts \
  src/com/arda/location/provider/ArdaGnssLocationService.java
  do
  if [[ ! -f "$SOURCE/$required" ]]; then
    echo "ARDA_PHASE3_0B_SOURCE_REQUIRED_MISSING path=$SOURCE/$required" >&2
    exit 2
  fi
done

DEST="$AOSP_ROOT/$DEST_REL"
DEST_PARENT="$(mkdir -p "$(dirname "$DEST")" && cd "$(dirname "$DEST")" && pwd)"
DEST_ABS="$DEST_PARENT/$(basename "$DEST")"
if [[ "$SOURCE" = "$DEST_ABS" ]]; then
  echo "ARDA_PHASE3_0B_SOURCE_EQUALS_DEST source=$SOURCE dest=$DEST_ABS" >&2
  exit 2
fi

mkdir -p "$DEST"
rsync -a --delete --exclude='.git/' "$SOURCE/" "$DEST/"
echo "ARDA_PHASE3_0B_AOSP_COPY_OK source=$SOURCE dest=$DEST"

python3 - "$PRODUCT_MK" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
marker = "# ARDA Location Provider Phase 3.0B"
inherit = "$(call inherit-product-if-exists, vendor/arda/location_provider/arda_location.mk)"
lines = path.read_text().splitlines()
lines = [line for line in lines if line.strip() not in {marker, inherit}]
text = "\n".join(lines).rstrip()
if text:
    text += "\n\n"
text += marker + "\n" + inherit + "\n"
path.write_text(text)
PY

PRODUCT_COUNT="$(grep -Fxc '$(call inherit-product-if-exists, vendor/arda/location_provider/arda_location.mk)' "$PRODUCT_MK")"
if [[ "$PRODUCT_COUNT" != "1" ]]; then
  echo "ARDA_PHASE3_0B_PRODUCT_INHERIT_COUNT_INVALID count=$PRODUCT_COUNT file=$PRODUCT_MK" >&2
  exit 2
fi
echo "ARDA_PHASE3_0B_PRODUCT_INHERIT_OK file=$PRODUCT_MK"

python3 - "$BOARD_CONFIG" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
marker = "# ARDA Location Provider policy"
policy = "SYSTEM_EXT_PRIVATE_SEPOLICY_DIRS += vendor/arda/location_provider/sepolicy/private"
lines = path.read_text().splitlines()
lines = [line for line in lines if line.strip() not in {marker, policy}]
text = "\n".join(lines).rstrip()
if text:
    text += "\n\n"
text += marker + "\n" + policy + "\n"
path.write_text(text)
PY

POLICY_COUNT="$(grep -Fxc 'SYSTEM_EXT_PRIVATE_SEPOLICY_DIRS += vendor/arda/location_provider/sepolicy/private' "$BOARD_CONFIG")"
if [[ "$POLICY_COUNT" != "1" ]]; then
  echo "ARDA_PHASE3_0B_SEPOLICY_INHERIT_COUNT_INVALID count=$POLICY_COUNT file=$BOARD_CONFIG" >&2
  exit 2
fi
echo "ARDA_PHASE3_0B_SEPOLICY_INHERIT_OK file=$BOARD_CONFIG"

python3 - "$AOSP_ROOT/$LMS_REL" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text()
marker = "// ARDA Phase 3.0B: allow GNSS provider override without a hardware GNSS HAL."
if marker in text:
    if text.count(marker) != 1:
        raise SystemExit(f"ARDA_PHASE3_0B_LMS_MARKER_COUNT_INVALID count={text.count(marker)}")
    print("ARDA_PHASE3_0B_LMS_GNSS_OVERRIDE_GATE=PASS state=already_patched")
    raise SystemExit(0)

start_token = "        // initialize gnss last because it has no awareness of boot phases and blindly assumes that\n"
end_token = "        // bind to geocoder provider\n"
if text.count(start_token) != 1 or text.count(end_token) != 1:
    raise SystemExit(
        "ARDA_PHASE3_0B_LMS_ANCHOR_COUNT_INVALID "
        f"start={text.count(start_token)} end={text.count(end_token)}"
    )
start = text.index(start_token)
end = text.index(end_token, start)
old = text[start:end]
required = [
    "if (hasLocationFeature && GnssNative.isSupported()) {",
    "config_useGnssHardwareProvider",
    "ProxyLocationProvider.create(",
    "ACTION_GNSS_PROVIDER",
    "GPS_HARDWARE_PROVIDER",
    "mGnssManagerService.getGnssLocationProvider()",
]
missing = [token for token in required if token not in old]
if missing:
    raise SystemExit("ARDA_PHASE3_0B_LMS_EXPECTED_BLOCK_DRIFT missing=" + repr(missing))

new = '''        // initialize gnss last because it has no awareness of boot phases and blindly assumes that
        // all other location providers are loaded at initialization
        boolean hasLocationFeature = mContext.getPackageManager().hasSystemFeature(
                PackageManager.FEATURE_LOCATION);
        if (hasLocationFeature) {
            // ARDA Phase 3.0B: allow GNSS provider override without a hardware GNSS HAL.
            boolean useGnssHardwareProvider = mContext.getResources().getBoolean(
                    com.android.internal.R.bool.config_useGnssHardwareProvider);
            AbstractLocationProvider gnssProvider = null;
            if (!useGnssHardwareProvider) {
                // TODO: Create a separate config_enableGnssLocationOverlay config resource
                // if we want to selectively enable a GNSS overlay but disable a fused overlay.
                gnssProvider = ProxyLocationProvider.create(
                        mContext,
                        GPS_PROVIDER,
                        ACTION_GNSS_PROVIDER,
                        com.android.internal.R.bool.config_enableFusedLocationOverlay,
                        com.android.internal.R.string.config_gnssLocationProviderPackageName);
            }

            if (GnssNative.isSupported()) {
                GnssConfiguration gnssConfiguration = new GnssConfiguration(mContext);
                GnssNative gnssNative = GnssNative.create(mInjector, gnssConfiguration);
                mGnssManagerService = new GnssManagerService(mContext, mInjector, gnssNative);
                mGnssManagerService.onSystemReady();
                if (gnssProvider == null) {
                    gnssProvider = mGnssManagerService.getGnssLocationProvider();
                } else {
                    // If we have a GNSS provider override, add the hardware provider as a standalone
                    // option for use by apps with the correct permission. Note the GNSS HAL can only
                    // support a single client, so mGnssManagerService.getGnssLocationProvider() can
                    // only be installed with a single provider. Locations from this provider won't
                    // be reported through the passive provider.
                    LocationProviderManager gnssHardwareManager =
                            new LocationProviderManager(
                                    mContext,
                                    mInjector,
                                    GPS_HARDWARE_PROVIDER,
                                    /*passiveManager=*/ null,
                                    Collections.singletonList(Manifest.permission.LOCATION_HARDWARE));
                    addLocationProviderManager(
                            gnssHardwareManager, mGnssManagerService.getGnssLocationProvider());
                }
            } else if (useGnssHardwareProvider) {
                Log.w(TAG, "no GNSS hardware location provider found");
            }

            if (gnssProvider != null) {
                LocationProviderManager gnssManager = new LocationProviderManager(
                        mContext, mInjector, GPS_PROVIDER, mPassiveManager);
                addLocationProviderManager(gnssManager, gnssProvider);
            } else {
                Log.w(TAG, "no GNSS location provider found");
            }
        }
'''
path.write_text(text[:start] + new + text[end:])
print("ARDA_PHASE3_0B_LMS_GNSS_OVERRIDE_GATE=PASS state=patched")
PY

grep -Fq '// ARDA Phase 3.0B: allow GNSS provider override without a hardware GNSS HAL.' "$AOSP_ROOT/$LMS_REL" || {
  echo "ARDA_PHASE3_0B_LMS_MARKER_MISSING file=$AOSP_ROOT/$LMS_REL" >&2
  exit 2
}

cat <<EOF
ARDA_PHASE3_0B_AOSP_APPLY_OK=YES
AOSP_DEST=$DEST_REL
PRODUCT_MK=$PRODUCT_MK
BOARD_CONFIG=$BOARD_CONFIG
LMS=$LMS_REL
NEXT_BUILD_COMMANDS:
  source build/envsetup.sh
  lunch aosp_rpi4_car-bp4a-userdebug
  m ArdaLocationProvider framework-res services
  m systemimage systemextimage
EOF
