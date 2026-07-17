# RN-3B final AOSP integration — per-device hotspot identity

This replaces the demo-owned shell hotspot and foreground-user Platform instance.

Final ownership:

```text
system user 0 / platform-signed privileged ArdaPlatform
  -> one device-wide LocalOnlyHotspot
  -> per-device stable SSID and WPA2 passphrase
  -> RFCOMM listener
  -> localhost senderd byte bridge

foreground users
  -> no second hotspot
  -> no second RFCOMM listener
```

## Credential lifecycle

The final image contains no hotspot SSID or passphrase.

On the first successful boot of each device, ArdaPlatform uses `SecureRandom` to
create:

```text
SSID: ARDA-XXXXXXXX
PSK:  24 random characters
```

The generated identity is stored in system-user device-protected storage. It is
available during Direct Boot and remains unchanged across:

```text
service restarts
normal reboots
APK/system-app updates
new product builds flashed while userdata is preserved
```

It is intentionally regenerated after factory reset, userdata wipe, or explicit
ArdaPlatform app-data deletion. The passphrase is sent only through the existing
authenticated Bluetooth bootstrap protocol and is never written to logcat.

Build-time random credentials are not used because one image may be flashed to
multiple devices. Per-boot random credentials are not used because they would
invalidate the phone's saved network after every reboot.

## Build the final Platform APK

```bash
./gradlew clean :app:assembleDebug
```

## Stage into AOSP

```bash
cd <aaos-aap>
AOSP_ROOT=/home/edo/aosp-rpi-aaos16 \
PLATFORM_APK=/path/to/ArdaPlatform/app/build/outputs/apk/debug/app-debug.apk \
PLATFORM_PACKAGE=com.github.deadknight.ardaplatform.debug \
./scripts/stage-rn3b-final-platform-aosp.sh
```

No SSID or PSK arguments are accepted or required.

Before flashing the new image, remove a manually installed debug-key Platform
package from current userdata to avoid a debug-key/platform-key signature
conflict:

```bash
adb -s 1000000028c0b59c uninstall com.github.deadknight.ardaplatform.debug || true
```

## Build

```bash
cd /home/edo/aosp-rpi-aaos16
source build/envsetup.sh
lunch aosp_rpi4_car-bp4a-userdebug
m -j"$(nproc)"
./rpi4-mkimg.sh
```

Do not run `repo sync`.

## First phone enrollment

Because the final device receives a new per-device SSID and passphrase, the phone
will request one saved-network enrollment after the first final-image boot. Later
Platform restarts and AAOS reboots reuse the same credentials and do not create a
new saved network.

## Runtime gate

```bash
AAOS_DEVICE=1000000028c0b59c \
PHONE_DEVICE=554b52f6 \
./scripts/run-rn3b-final-gate-macos.sh
```

The gate reads the actual generated SSID from AAOS logs and compares the phone's
received offer against it. It also rejects an image-baked
`/product/etc/arda/rn3b_wifi.conf` secret file.
