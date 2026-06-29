# ARDA AAOS Google Maps Automotive / GApps notes

This note documents the current **recovery-less** Google Maps Automotive experiment on the ARDA Raspberry Pi 4 AAOS image.

The short version:

```text
GMS/GSF should stay enabled.
Disable unnecessary Google apps.
Fix Dalvik heap before judging Google Maps stability.
Treat Play Store / Phonesky as a separate task.
```

## Tested baseline

Observed AAOS image:

```bash
adb shell getprop ro.build.version.release   # 11
adb shell getprop ro.build.version.sdk       # 30
adb shell getprop ro.product.cpu.abi         # arm64-v8a
adb shell getprop ro.build.type              # userdebug
adb shell getprop ro.build.tags              # test-keys
```

Observed partition/layout behavior:

```text
/system      writable after adb root + adb remount
/product     -> /system/product
/system_ext  -> /system/system_ext
```

Active AAOS user during tests:

```bash
adb shell am get-current-user
# 10

adb shell cmd user list
# UserInfo{0:Driver:813} running
# UserInfo{10:Driver:412} running
```

Existing non-Google maps package on this image:

```text
com.snappautomotive.maps -> /system/app/osmdroid/osmdroid.apk
```

Google Maps Automotive package:

```text
com.google.android.apps.maps
```

## External push files / download links

Do not commit these proprietary APK/ZIP files into the repository. Keep them in `~/Downloads` or another local staging directory and push/extract them manually.

| Purpose | File | Link |
|---|---|---|
| Android 11 ARM64 GApps base used for manual extraction | `MindTheGapps-11.0.0-arm64-20220217_100228.zip` | `https://androidfilehost.com/?fid=17825722713688288529` |
| Android 11 ARM64 GApps alternative | `MindTheGapps-11.0.0-arm64-20230201.zip` | `https://sourceforge.net/projects/wsa-mtg/files/arm64/20230201/MindTheGapps-11.0.0-arm64-20230201.zip/` |
| Google Maps Android Automotive release page | `Google Maps (Android Automotive) 26.06.080002.E` | `https://www.apkmirror.com/apk/google-inc/google-maps-android-automotive/google-maps-android-automotive-26-06-080002-e-release/` |
| Google Maps Android Automotive APK page used in the current test | `Google Maps (Android Automotive) 26.06.080002.E arm64-v8a+x86+x86_64 Android 11+` | `https://www.apkmirror.com/apk/google-inc/google-maps-android-automotive/google-maps-android-automotive-26-06-080002-e-release/google-maps-android-automotive-26-06-080002-e-android-apk-download/` |
| Google Maps Android Automotive Android 11+ variant listing | Android 11+ / API 30 variants | `https://www.apkmirror.com/apk/google-inc/google-maps-android-automotive/variant-%7B%22minapi_slug%22%3A%22minapi-30%22%7D/` |

Local file name used in the session for Maps may vary. Example:

```bash
MAPS_APK="$HOME/Downloads/com.google.android.apps.maps_26.06.080002.E-1068622130_minAPI30(arm64-v8a,x86,x86_64)(nodpi)_apkmirror.com.apk"
```

## Manual GApps staging

Extract the GApps ZIP into a temporary staging directory:

```bash
ZIP=MindTheGapps-11.0.0-arm64-20220217_100228.zip
WORK=/tmp/mtg11

rm -rf "$WORK"
mkdir -p "$WORK"
unzip -q "$ZIP" -d "$WORK"

find "$WORK" -maxdepth 4 -type d | head -120
```

Remove setup/restore components that are risky on this AAOS image:

```bash
find "$WORK" -depth \( \
  -iname '*SetupWizard*' -o \
  -iname '*GoogleRestore*' -o \
  -iname '*OneTimeInitializer*' -o \
  -iname '*AndroidAutoStub*' \
\) -print -exec rm -rf {} +
```

Add Google Maps Automotive to the staged product app tree:

```bash
mkdir -p "$WORK/system/product/app/GoogleMapsAutomotive"
cp "$MAPS_APK" "$WORK/system/product/app/GoogleMapsAutomotive/GoogleMapsAutomotive.apk"
ls -lh "$WORK/system/product/app/GoogleMapsAutomotive/"
```

Keep Maps under `product/app`, not `product/priv-app`, to avoid privileged permission whitelist boot issues.

## Push staged files to AAOS

```bash
adb root
adb remount

if [ -d "$WORK/system" ]; then
  adb push "$WORK/system/." /system/
fi
if [ -d "$WORK/product" ]; then
  adb push "$WORK/product/." /product/
fi
if [ -d "$WORK/system_ext" ]; then
  adb push "$WORK/system_ext/." /system_ext/
fi
```

Fix ownership, modes, and SELinux contexts:

```bash
adb shell <<'EOF'
set -e

chown -R root:root \
  /system/product/app \
  /system/product/priv-app \
  /system/product/etc \
  /system/system_ext/priv-app \
  /system/system_ext/etc 2>/dev/null || true

find /system/product/app /system/product/priv-app /system/system_ext/priv-app \
  -type d -exec chmod 755 {} \; 2>/dev/null || true

find /system/product/app /system/product/priv-app /system/system_ext/priv-app \
  -type f -exec chmod 644 {} \; 2>/dev/null || true

find /system/product/etc /system/system_ext/etc \
  -name '*.xml' -type f -exec chmod 644 {} \; 2>/dev/null || true

restorecon -RF /system/product/app/GoogleMapsAutomotive /system/product /system/system_ext 2>/dev/null || true
sync
EOF

adb reboot
```

## Initial package checks

After reboot:

```bash
adb connect 192.168.1.15
adb root

adb shell pm path com.google.android.gms
adb shell pm path com.google.android.gsf
adb shell pm path com.android.vending
adb shell pm path com.google.android.apps.maps
adb shell pm path com.snappautomotive.maps
```

Expected before Play Store/Phonesky is fixed:

```text
com.google.android.gms       -> present
com.google.android.gsf       -> present
com.google.android.apps.maps -> present
com.android.vending          -> may be missing
com.snappautomotive.maps     -> still present
```

Install existing packages for both AAOS users:

```bash
for u in 0 10; do
  adb shell cmd package install-existing --user $u com.google.android.gms || true
  adb shell cmd package install-existing --user $u com.google.android.gsf || true
  adb shell cmd package install-existing --user $u com.google.android.apps.maps || true
  adb shell cmd package install-existing --user $u com.android.vending || true
done
```

`com.android.vending` failure is acceptable while Maps is being tested. Debug Play Store separately.

## Keep GMS/GSF enabled

Do not globally disable GMS/GSF while testing Maps. The current stable-ish setup keeps GMS and GSF enabled for both users:

```bash
for u in 0 10; do
  adb shell pm enable --user $u com.google.android.gms || true
  adb shell pm enable --user $u com.google.android.gsf || true
done
```

Disable only the extra Google apps that are not required for the current Maps test:

```bash
for u in 0 10; do
  for p in \
    com.google.android.googlequicksearchbox \
    com.google.android.gm.exchange \
    com.google.android.partnersetup \
    com.google.android.syncadapters.contacts \
    com.google.android.syncadapters.calendar \
    com.google.android.tts \
    com.google.android.marvin.talkback \
    com.google.android.feedback
  do
    adb shell pm disable-user --user $u "$p" || true
    adb shell am force-stop --user $u "$p" || true
  done
done
```

## Important fix: Dalvik heap tuning

### Symptom before heap tuning

Google Maps could open but became extremely slow or froze the display. Logcat showed repeated GC pressure:

```text
Clamp target GC heap from 21MB to 16MB
0% free, 15MB/16MB
Waiting for a blocking GC Alloc
RenderEngine fence wait timed out
Failed to flush RenderEngine commands
```

This means Maps was effectively trapped in a tiny Java heap and spent most of its time in blocking GC.

### Check current heap properties

```bash
adb shell 'getprop | grep -Ei "dalvik.vm.heap|low_ram|config.low_ram"'
```

On the tested image, this initially returned nothing relevant.

### Apply heap tuning

Backup:

```bash
adb root
adb remount
adb shell 'cp /system/build.prop /system/build.prop.arda-before-heap'
```

Append ARDA heap settings:

```bash
adb shell 'cat >> /system/build.prop' <<'EOF'

# ARDA Google Maps / GMS heap tuning
ro.config.low_ram=false
dalvik.vm.heapstartsize=16m
dalvik.vm.heapgrowthlimit=256m
dalvik.vm.heapsize=512m
dalvik.vm.heaptargetutilization=0.5
dalvik.vm.heapminfree=8m
dalvik.vm.heapmaxfree=32m
EOF

adb shell 'chmod 644 /system/build.prop'
adb shell 'restorecon /system/build.prop 2>/dev/null || true'
adb shell sync
adb reboot
```

Verify after reboot:

```bash
adb connect 192.168.1.15
adb root
adb shell 'getprop | grep -Ei "dalvik.vm.heap|low_ram|config.low_ram"'
```

Expected:

```text
[dalvik.vm.heapgrowthlimit]: [256m]
[dalvik.vm.heapmaxfree]: [32m]
[dalvik.vm.heapminfree]: [8m]
[dalvik.vm.heapsize]: [512m]
[dalvik.vm.heapstartsize]: [16m]
[dalvik.vm.heaptargetutilization]: [0.5]
[ro.config.low_ram]: [false]
```

Rollback if needed:

```bash
adb root
adb remount
adb shell 'cp /system/build.prop.arda-before-heap /system/build.prop'
adb shell 'chmod 644 /system/build.prop'
adb shell 'restorecon /system/build.prop 2>/dev/null || true'
adb reboot
```

## Enable and launch Maps after heap tuning

Enable only for the active AAOS user first:

```bash
adb shell pm enable --user 10 com.google.android.apps.maps || true
adb shell cmd package install-existing --user 10 com.google.android.apps.maps || true
adb shell pm clear --user 10 com.google.android.apps.maps
```

Launch:

```bash
adb logcat -c
adb shell monkey --pct-syskeys 0 -p com.google.android.apps.maps -c android.intent.category.LAUNCHER 1
```

Observe:

```bash
adb shell 'ps -A -o USER,PID,PPID,RSS,ARGS | grep -Ei "maps|gms|gservices|gapps|unstable"'
adb shell top -m 20
adb logcat -d | grep -Ei 'google.android.apps.maps|Clamp target GC heap|blocking GC|RenderEngine|DroidGuard|FATAL|AndroidRuntime|lowmemory|watchdog' | tail -200
```

Healthy-ish observed result after heap tuning:

```text
Maps opens faster.
Maps settings can be opened.
Location can be enabled.
Maps CPU drops from ~100%+ to roughly low double digits after startup.
RAM still has about 2 GB free.
Swap is still disabled.
```

## Disable Maps without disabling GMS

If Maps freezes the UI, disable Maps only:

```bash
adb shell am force-stop --user 10 com.google.android.apps.maps || true
adb shell pm disable-user --user 10 com.google.android.apps.maps || true
adb shell pm disable-user --user 0 com.google.android.apps.maps || true
adb reboot
```

Then keep GMS/GSF enabled:

```bash
for u in 0 10; do
  adb shell pm enable --user $u com.google.android.gms || true
  adb shell pm enable --user $u com.google.android.gsf || true
done
```

## Full Maps rescue: move APK out of scan path

Use this only if Maps keeps respawning or the launcher keeps a stale Maps tile.

Disable by moving the directory out of Android's app scan path:

```bash
adb root
adb remount

adb shell 'mkdir -p /system/product/_disabled_apps'
adb shell 'mv /system/product/app/GoogleMapsAutomotive /system/product/_disabled_apps/GoogleMapsAutomotive'
adb shell 'sync'

adb reboot
```

Restore later:

```bash
adb root
adb remount

adb shell 'mv /system/product/_disabled_apps/GoogleMapsAutomotive /system/product/app/GoogleMapsAutomotive'
adb shell 'restorecon -RF /system/product/app/GoogleMapsAutomotive 2>/dev/null || true'
adb shell 'sync'

adb reboot
```

Re-enable for user 10:

```bash
adb shell cmd package install-existing --user 10 com.google.android.apps.maps || true
adb shell pm enable --user 10 com.google.android.apps.maps || true
```

## Notes

- Do not debug Play Store / Phonesky and Maps at the same time.
- `com.android.vending` may fail to parse/install while Maps still works.
- Swap/zram was not required for the first successful Maps stabilization; the key fix was Dalvik heap tuning.
- If Maps still loops after heap tuning, try an older Android 11-era Maps Automotive APK before changing GMS package strategy.
