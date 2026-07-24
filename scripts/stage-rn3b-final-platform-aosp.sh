#!/usr/bin/env bash
set -euo pipefail

AOSP_ROOT="${AOSP_ROOT:-/home/edo/aosp-rpi-aaos16}"
PLATFORM_APK="${PLATFORM_APK:-}"
SENDERD_BIN="${SENDERD_BIN:-}"
PLATFORM_PACKAGE="${PLATFORM_PACKAGE:-com.github.deadknight.ardaplatform.debug}"
PRODUCT_MK="${PRODUCT_MK:-}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMPLATE_DIR="$REPO_ROOT/packaging/rn3b-final/ardaplatform"
HW_TEMPLATE_DIR="$REPO_ROOT/packaging/rn3b-final/hwencoder"
DEST="$AOSP_ROOT/vendor/arda/ardaplatform"
HW_DEST="$AOSP_ROOT/vendor/arda/hwencoder/minigbm"
INHERIT_LINE='$(call inherit-product-if-exists, vendor/arda/ardaplatform/arda_connectivity_product.mk)'
LEGACY_INHERIT_LINE='$(call inherit-product-if-exists, vendor/arda/ardaplatform/arda_rn3b_product.mk)'

fail() {
  echo "ARDA_RN3B_FINAL_STAGE=FAIL reason=$*" >&2
  exit 1
}

[[ -d "$AOSP_ROOT" ]] || fail "AOSP_ROOT_NOT_FOUND:$AOSP_ROOT"
[[ -n "$PLATFORM_APK" ]] || fail "PLATFORM_APK_REQUIRED"
[[ -f "$PLATFORM_APK" ]] || fail "PLATFORM_APK_NOT_FOUND:$PLATFORM_APK"
[[ -n "$SENDERD_BIN" ]] || fail "SENDERD_BIN_REQUIRED"
[[ -f "$SENDERD_BIN" ]] || fail "SENDERD_BIN_NOT_FOUND:$SENDERD_BIN"

if [[ -z "$PRODUCT_MK" ]]; then
  PRODUCT_MK="$(grep -RIl --include='*.mk' \
    -E 'PRODUCT_NAME[[:space:]]*:=[[:space:]]*aosp_rpi4_car' \
    "$AOSP_ROOT/device" 2>/dev/null | head -n 1 || true)"
fi
[[ -n "$PRODUCT_MK" && -f "$PRODUCT_MK" ]] || \
  fail "PRODUCT_MK_NOT_FOUND_SET_PRODUCT_MK_EXPLICITLY"

mkdir -p "$DEST"
cp "$PLATFORM_APK" "$DEST/ArdaPlatform.apk"
cp "$SENDERD_BIN" "$DEST/arda-connectivityd"
chmod 0755 "$DEST/arda-connectivityd"
cp "$TEMPLATE_DIR/Android.bp" "$DEST/Android.bp"
cp "$TEMPLATE_DIR/arda-aap.toml" "$DEST/arda-aap.toml"
chmod 0644 "$DEST/arda-aap.toml"

# ARDA_AAP_AUTOSTART_STAGE_BEGIN
sed "s|@PACKAGE@|$PLATFORM_PACKAGE|g" \
  "$TEMPLATE_DIR/arda-aap-autostart.sh.in" \
  > "$DEST/arda-aap-autostart.sh"
chmod 0755 "$DEST/arda-aap-autostart.sh"
cp "$TEMPLATE_DIR/arda-aap-autostart.rc" \
  "$DEST/arda-aap-autostart.rc"
chmod 0644 "$DEST/arda-aap-autostart.rc"
# ARDA_AAP_AUTOSTART_STAGE_END

sed "s|@PACKAGE@|$PLATFORM_PACKAGE|g" \
  "$TEMPLATE_DIR/privapp-permissions-ardaplatform.xml.in" \
  > "$DEST/privapp-permissions-ardaplatform.xml"
sed "s|@PACKAGE@|$PLATFORM_PACKAGE|g" \
  "$TEMPLATE_DIR/default-permissions-ardaplatform.xml.in" \
  > "$DEST/default-permissions-ardaplatform.xml"
sed "s|@PACKAGE@|$PLATFORM_PACKAGE|g" \
  "$TEMPLATE_DIR/sysconfig-ardaplatform.xml.in" \
  > "$DEST/sysconfig-ardaplatform.xml"
sed "s|@PACKAGE@|$PLATFORM_PACKAGE|g" \
  "$TEMPLATE_DIR/arda-rn3b-permission-bootstrap.sh.in" \
  > "$DEST/arda-rn3b-permission-bootstrap.sh"
chmod 0755 "$DEST/arda-rn3b-permission-bootstrap.sh"
cp "$TEMPLATE_DIR/arda-rn3b-permission-bootstrap.rc" \
  "$DEST/arda-rn3b-permission-bootstrap.rc"
sed "s|@PACKAGE@|$PLATFORM_PACKAGE|g" \
  "$TEMPLATE_DIR/arda-connectivity-supervisor.sh.in" \
  > "$DEST/arda-connectivity-supervisor.sh"
chmod 0755 "$DEST/arda-connectivity-supervisor.sh"
sed "s|@PACKAGE@|$PLATFORM_PACKAGE|g" \
  "$TEMPLATE_DIR/arda-connectivityd.rc.in" \
  > "$DEST/arda-connectivityd.rc"
cp "$TEMPLATE_DIR/arda_connectivity_product.mk.in" \
  "$DEST/arda_connectivity_product.mk"

# ARDA_RPI4_HW_ENCODER_EXPERIMENTAL_STAGE_BEGIN
# Do not copy or activate the mapper-bind helper during normal staging.
# The source remains under packaging/rn3b-final/hwencoder and
# vendor/arda/hwencoder/minigbm for controlled, explicit experiments.
if [[ "${ARDA_ENABLE_EXPERIMENTAL_HW_ENCODER:-0}" == "1" ]]; then
  [[ -f "$HW_DEST/Android.bp" ]] ||     fail "HW_ENCODER_VENDOR_MODULE_MISSING:$HW_DEST/Android.bp_RUN_V13_FIRST"
  cp "$HW_TEMPLATE_DIR/arda-hw-encoder-mapper-bind.sh"     "$HW_DEST/arda-hw-encoder-mapper-bind.sh"
  chmod 0755 "$HW_DEST/arda-hw-encoder-mapper-bind.sh"
  cp "$HW_TEMPLATE_DIR/arda-hw-encoder-mapper-bind.rc"     "$HW_DEST/arda-hw-encoder-mapper-bind.rc"
  chmod 0644 "$HW_DEST/arda-hw-encoder-mapper-bind.rc"
  echo "ARDA_RPI4_HW_ENCODER_EXPERIMENTAL_STAGE=ENABLED_MANUALLY"
else
  echo "ARDA_RPI4_HW_ENCODER_EXPERIMENTAL_STAGE=SKIPPED_DEFAULT"
fi
# ARDA_RPI4_HW_ENCODER_EXPERIMENTAL_STAGE_END

# Remove files left by earlier RN-3B runtime naming. The protocol/test markers
# remain RN-3B; only installed runtime names are migrated.
rm -f \
  "$DEST/arda-senderd" \
  "$DEST/arda-rn3b-bootstrap-supervisor.sh" \
  "$DEST/arda-rn3b-bt-bootstrapd.rc" \
  "$DEST/arda_rn3b_product.mk" \
  "$DEST/rn3b_wifi.conf"

if grep -Fqx "$LEGACY_INHERIT_LINE" "$PRODUCT_MK"; then
  fail "LEGACY_PRODUCT_INHERIT_PRESENT_REFUSE_AOSP_OWNED_EDIT:$PRODUCT_MK"
fi

if ! grep -Fqx "$INHERIT_LINE" "$PRODUCT_MK"; then
  fail "VENDOR_PRODUCT_INHERIT_MISSING_REFUSE_AOSP_OWNED_EDIT:$PRODUCT_MK"
fi

echo "ARDA_RN3B_FINAL_PRODUCT_INHERIT=ALREADY_PRESENT file=$PRODUCT_MK"
echo "ARDA_RN3B_FINAL_AOSP_OWNED_SOURCE_EDITED=NO"

echo "ARDA_RN3B_FINAL_STAGE=PASS"
echo "ARDA_RN3B_FINAL_AOSP_DEST=$DEST"
echo "ARDA_RN3B_FINAL_PLATFORM_PACKAGE=$PLATFORM_PACKAGE"
echo "ARDA_RN3B_FINAL_CREDENTIAL_MODE=FIRST_BOOT_DEVICE_GENERATED"
echo "ARDA_RN3B_FINAL_STATIC_WIFI_SECRET_IN_IMAGE=NO"
echo "ARDA_RN3B_FINAL_RUNTIME_PERMISSION_MODE=DEFAULT_GRANT_PLUS_BOOTSTRAP_USER0"
echo "ARDA_RN3B_FINAL_CONNECTIVITY_SERVICE=arda-connectivityd"
echo "ARDA_RN3B_FINAL_CONNECTIVITY_BINARY=/system_ext/bin/arda-connectivityd"
echo "ARDA_RN3B_FINAL_AAP_AUTOSTART=INIT_USB_ACCESSORY_PROPERTY_TRIGGER"
echo "ARDA_RN3B_FINAL_AAP_CONFIG=/product/etc/arda/arda-aap.toml"
echo "ARDA_RPI4_HW_ENCODER_PERSIST=DISABLED_APEX_LABEL_UNSAFE"
echo "ARDA_RPI4_HW_ENCODER_CODEC=EXPERIMENTAL_MANUAL_ONLY"
echo "ARDA_RN3B_FINAL_SENDERD_BOOT_MODE=INIT_SUPERVISED_TOML"
echo "ARDA_RN3B_FINAL_SENDERD_CONFIG=/data/user_de/0/$PLATFORM_PACKAGE/files/arda-connectivity.toml"
echo "ARDA_RN3B_FINAL_NEXT=lunch_aosp_rpi4_car-bp4a-userdebug_then_build_image"
