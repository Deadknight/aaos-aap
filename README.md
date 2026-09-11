# ARDA — AAOS / Android Auto Integration

> Canonical README merged on **2026-09-08** from the supplied README/handover archive plus the later runtime-validated work completed after the archive.
>
> This file is the **technical/project README**. Historical decision chronology, failures, root-cause investigations, checkpoint status, and continuation instructions live in `HANDOVER.md`.

---

## 1. Project purpose

ARDA is a Raspberry Pi 4 based Android Automotive OS / Android Auto integration stack. The project combines:

- AAOS on Raspberry Pi 4.
- AAP / Android Auto head-unit transport and media handling.
- H.264 display capture and Android Auto video streaming.
- Touch, full multitouch, key input, and platform injection.
- Vehicle property / VHAL / Wasm translation.
- GPS/GNSS bridge work.
- USB AOA and direct `/dev/usb_accessory` operation.
- Optional Wi-Fi handoff / proxying.
- Projection display geometry reconciliation.
- CarSystemUI safe-area-aware system bars.
- Custom vertical/horizontal dock behavior, including recent map and recent app slots.
- ArdaPlatform as the privileged configuration/control plane.

The project has gone through multiple architecture generations. Older versions are still documented below and in the preserved source appendices, but the canonical current architecture is described first.

---

## 2. Non-negotiable working rules

### 2.1 Never run `repo sync`

`repo sync` is intentionally forbidden for this working tree. The AOSP checkout contains targeted local/private-repository work that must not be casually reconciled against upstream.

Allowed targeted operations include:

```bash
git fetch
git pull --ff-only
git push
git fetch --unshallow --no-tags <remote>
```

when deliberately applied to an individual repository.

### 2.2 Keep ADB transport pinned during a transaction

Current preferred Ethernet ADB endpoint:

```text
192.168.1.184:5555
```

Older scripts/checkpoints may still mention:

```text
192.168.1.165:5555
```

Treat `192.168.1.184:5555` as the current default unless the device changes again.

USB serial historically used:

```text
1000000028c0b59c
```

Operational policy:

- Ethernet ADB for deploys, logs, system work.
- USB reserved for AAP/DHU where possible.
- Once a script selects an ADB transport, keep it pinned for the whole transaction.

### 2.3 Never `adb root` while DHU is live

On this platform, `adb root` can re-enumerate the USB gadget. With DHU/AAP active this can tear down the AOA session.

Deployment scripts therefore gate on:

```bash
pidof screenrecord
```

or equivalent AAP/DHU state before calling `adb root`.

### 2.4 Preserve hardware/direct capture goals

The project explicitly avoids falling back to unnecessary CPU copies.

Current design rule:

- exact source == sink: direct capture-owned / PCT path is allowed;
- source != sink: use normal VDS/HWC/GPU composition/scaling into codec-owned sink buffers;
- no CPU RGBA copy/scaling path as the production answer.

### 2.5 Do not commit/push experimental work before runtime approval

Build success alone does not close a feature. Runtime behavior must be observed first.

---

## 3. Current environment

### 3.1 Mac development machine

Common local source paths:

```text
/Users/edo/Documents/Code/aaos-aap
/Users/edo/Documents/Code/ArdaPlatform
```

The Mac is commonly used for:

- `aaos-aap` development.
- ArdaPlatform Android Studio builds.
- DHU.
- orchestration scripts.
- SSH into the Ubuntu AOSP host.
- Ethernet ADB deployment.

### 3.2 Ubuntu/AOSP build host

```text
host: edo@192.168.1.55
key:  ~/.ssh/id_ed25519_arda_build
tree: /home/edo/aosp-rpi-aaos16
lunch: aosp_rpi4_car-bp4a-userdebug
```

Recent build banner:

```text
PLATFORM_VERSION=16
TARGET_PRODUCT=aosp_rpi4_car
TARGET_BUILD_VARIANT=userdebug
TARGET_ARCH=arm64
TARGET_CPU_VARIANT=cortex-a72
BUILD_ID=BP4A.251205.006
```

### 3.3 Target

```text
Raspberry Pi 4
Android Automotive OS 16
Waveshare 7.9" display profile
rotation: 270° in the current landscape presentation
```

Observed display geometry in the current safe-area work:

```text
active WMS geometry: 1064x720
vertical system-bar girth: 62 px
```

---

## 4. Repository layout and Git remotes

### 4.1 Last all-six remote-contained baseline — 2026-08-13

The supplied archive records the following canonical all-six checkpoint:

```text
packages/apps/Car/Launcher
  branch: arda-poc-checkpoint
  remote: github -> Deadknight/arda-aaos-car-launcher
  commit: fe205c5870d01b395e4e3df2614e7c6d2b41c114

packages/apps/Car/MediaPrebuilt
  branch: arda-poc-checkpoint
  remote: github -> Deadknight/android_packages_apps_Car_MediaPrebuilt
  commit: 1342ba973e3c8e70342144c736b3a5eb94a6ddd7

packages/apps/Car/SystemUI
  branch: arda-aaos16
  remote: arda -> Deadknight/android_packages_apps_Car_SystemUI
  commit: c35ec54d6457c6801043838028f8ad66d422bf59

device/brcm/rpi4
  branch: arda-aaos16
  remote: arda -> Deadknight/android_device_brcm_rpi4
  commit: a0b6ae212be2f9815213519860ccb78d56b13734

vendor/arda
  branch: arda-aaos16
  remote: origin -> Deadknight/android_vendor_arda
  commit: e79d85effc39207a946825710383f2732f4d1c35

vendor/arda/ardaplatform
  branch: main
  remote: origin -> Deadknight/android_vendor_arda_ardaplatform
  commit: 63df5257e5667373470f97a0867607d8817cc9ba
```

These SHAs remain important recovery points, but Launcher and SystemUI now contain later **uncommitted** work described below.

### 4.2 Current Git target state — 2026-09-08

Audit confirmed these existing remotes:

```text
Launcher:
  repo:   packages/apps/Car/Launcher
  branch: arda-poc-checkpoint
  remote: github
  url:    https://github.com/Deadknight/arda-aaos-car-launcher.git

SystemUI:
  repo:   packages/apps/Car/SystemUI
  branch: arda-aaos16
  remote: arda
  url:    https://github.com/Deadknight/android_packages_apps_Car_SystemUI.git
```

Ubuntu GitHub CLI is authenticated as:

```text
Deadknight
protocol: HTTPS
```

GitHub SSH auth is not required for these repositories.

At the moment this README was assembled, the latest safe-area/dock work had **not yet been committed/pushed**. The prepared checkpoint script is:

```text
ARDA_CHECKPOINT_COMMIT_PUSH_V2_20260908.sh
```

Expected current modified files:

```text
packages/apps/Car/Launcher
  app/src/com/android/car/carlauncher/CarLauncherViewModel.java
  docklib/src/com/android/car/docklib/DockViewModel.kt
  docklib/src/com/android/car/docklib/view/DockView.kt

packages/apps/Car/SystemUI
  src/com/android/systemui/car/systembar/ArdaSafeAreaCarSystemBarView.java
  src/com/android/systemui/car/systembar/SystemBarConfigsImpl.java
```

Do not assume those changes are on GitHub until the commit/push script completes and remote SHA verification passes.

---

## 5. AAP / Android Auto architecture

The AAP stack evolved from protocol bring-up into a practical head-unit path.

### 5.1 Protocol/media bring-up lineage

Important milestones preserved from the original README:

```text
v24c  rust-protobuf optional Service field API correction
v24d  service-channel CHANNEL_OPEN_REQUEST fix
v25   media CONFIG decode + START
v26   MEDIA_MESSAGE_SETUP
v27   docs-based Annex-B H.264 file DATA probe
v28   native-capture live H.264 bridge
v29   stdout flushing + SDR auto config
v30   video pump/ACK behavior fix
v31   SDR resolution/density/inset-aware selection
```

The original detailed notes are preserved verbatim later in this README.

### 5.2 Current AAP transport policy

Normal current-image operation uses direct AAOS USB accessory support instead of recreating the gadget on every run.

Current baseline:

```text
/dev/usb_accessory
transport.mode = "usb-accessory"
```

Legacy manual gadget helper remains for recovery/older images only.

### 5.3 Startup sequencing caveats

Two startup races have been observed and must be kept conceptually separate.

#### Race A — display profile reconcile / system_server restart

AAP can select a new HU display profile, apply WM geometry, and request a display-profile reconcile that restarts framework/system processes. An AAP autostart session caught in that transition can exit.

This is not generic USB instability.

#### Race B — first native capture child EOF

The next AAP session may reach `MEDIA_MESSAGE_START`, spawn `screenrecord`, and see the first child terminate/EOF before first output. The built-in first-frame timeout/restart can recover; a second child has been observed to produce codec config, first video, and ACK normally.

Keep first-frame retry logic.

---

## 6. Video capture and H.264

### 6.1 Hardware encoder

Primary H.264 hardware codec:

```text
c2.v4l2.avc.encoder
```

Historical production capture used platform:

```text
/system/bin/screenrecord
```

with Annex-B H.264 piped into AAP media framing.

### 6.2 1080p30 policy

A 60-fps SDR advertisement does not imply the Pi 4 should intentionally attempt 1080p60.

The performance policy established in the archive:

```text
1920x1080
~30 real encoded fps
hardware encoder
no software fallback
no avoidable CPU conversion
AAP PTS cadence must agree with actual capture cadence
```

### 6.3 Later SurfaceFlinger/HWC capture architecture

After the August archive, capture work progressed further.

Current architectural state:

```text
Stable profile:
  exact-source direct path based on the proven V23 behavior
  product default

Performance profile:
  B2.1 lineage
  ~25–26 fps observed in prior testing
  rare artifact risk
  optional, not discarded
```

Runtime property design:

```text
persist.sys.arda.capture_profile=stable|performance
sys.arda.capture_profile.active=<active profile>
```

### 6.4 Exact source == sink versus scaling

Correct production rule:

```text
source size == codec sink size
  -> direct PCT / capture-owned direct path may be used

source size != codec sink size
  -> normal VDS HWC/GPU composition/scaling
  -> codec-owned sink buffers
  -> no CPU copy
```

A previous failure mode came from trying to direct-clone a physical 1920x1080 frame into a 1280x720 codec sink; this caused codec/V4L2 CPU-lock style failures. Do not reintroduce that architecture.

### 6.5 Future performance direction

If needed after runtime measurement:

```text
capture-owned triple-buffer DMABUF pool
```

is the preferred next direct-capture architecture, not CPU conversion.

---

## 7. Input

### 7.1 Single-touch / platform injection history

Historical modes included:

```text
uinput
app_process / InputManager.injectInputEvent
shell input fallback
```

The project later platformized input into a secured framework/platform bridge.

### 7.2 Full multitouch

Full multitouch is considered closed/passed in the historical handover.

Important behavior:

- pointer snapshots are reconciled rather than naively replayed;
- stale pointers are released;
- missing UP transitions are repaired;
- pointer IDs and gesture continuity are handled conservatively;
- platform input ownership is centralized.

The original changelogs are preserved in `HANDOVER.md`.

---

## 8. Vehicle property / VHAL / Wasm

The project progressed through:

```text
v42 sensor bridge start
v43 sensor OS/VHAL bridge
v55 explicit sensor gate + normalized event boundary
v56 Wasmtime VHAL translation/policy
v57 Phase 1.1 direct VehicleProperty + generic Wasm pass-through
Phase 2.0 / 2.1 / 2.2 vehicle manager + router/platformization
```

Important ownership rule established later:

```text
one canonical vehicle-property ownership path
```

Do not reintroduce duplicated manager/router ownership or translation logic in multiple layers.

See `HANDOVER.md` and the preserved `README_PHASE1_1_FIX.md` appendix for exact Phase 1.1 details and smoke markers.

---

## 9. GPS / GNSS

GPS/GNSS evolved through:

```text
Phase 3.0A fidelity repair
Phase 3.0B Android GPS provider bridge
Phase 3.0B2 GNSS status plane
Phase 3.0B3 vendor GNSS AIDL HAL migration
```

Standard GNSS AIDL location path was closed in the supplied historical plan.

Separate DHU GPS persistence work also ensured GPS state survives focus/session transitions correctly; exact change notes are preserved in `HANDOVER.md`.

---

## 10. Networking and RN-3B Wi-Fi handoff

Historical RN-3B work added:

```text
HELLO / HELLO_ACK
PING / PONG
INTERNET_OFFER
INTERNET_RESULT
INTERNET_STATUS
```

Supported conceptual modes:

- phone hotspot -> AAOS joins phone Wi-Fi;
- AAOS local hotspot / HTTP proxy path;
- keep RFCOMM bootstrap alive until direct Wi-Fi HELLO/PING/PONG succeeds.

The Android phone companion still owns companion-side hotspot orchestration where not otherwise superseded.

Original hybrid README is preserved verbatim later in this README.

---

## 11. Display geometry and projection safe area

This is one of the major post-archive additions.

### 11.1 User requirement

The safe-area requirement is **not** “pad the icons inward.”

The actual requirement:

- shorten the real left/right or top/bottom system-bar windows;
- bar background must stop at the projection safe-area boundary;
- unsafe edge segments should expose the underlying/global background;
- support both vertical and horizontal system-bar modes.

### 11.2 Failed approaches

Several attempts were useful diagnostics but were not the final solution.

#### Child view resize

Patching `ArdaSafeAreaCarSystemBarView` could not resize the WMS-owned outer bar window.

#### `getLayoutParamsByName()`

This was not on the real runtime attach path.

#### `InternalSystemBarWindow.getLayoutParams()`

The expected method contained the patch but was not the final runtime `LayoutParams` source in the active path.

#### `setRootView()` one-shot apply

This proved the real attach choke point but still suffered from startup ordering: bars were attached before projection overlay data existed.

#### `OnApplyWindowInsetsListener` V5

This reacted to late insets, but caused a feedback loop:

```text
0,8,0,14 -> shrink
0,0,0,0  -> grow
0,8,0,14 -> shrink
...
```

Reason: after shrinking, the bar window no longer intersected the top/bottom overlay regions, so its **local** window insets became zero. The bar then enlarged itself and repeated the cycle.

### 11.3 Final safe-area V6 architecture

The final stable source is a **global, window-independent atomic setting** generated by projection geometry:

```text
Settings.Global:
arda_projection_content_insets
```

Observed value in the current 1064x720 setup:

```text
0,8,0,14
```

Meaning:

```text
left   = 0
top    = 8
right  = 0
bottom = 14
```

SystemUI uses a `ContentObserver` rather than its own local `WindowInsets` intersection.

Conceptual flow:

```text
ArdaProjectionInsets
  -> writes arda_projection_content_insets
  -> SystemUI ContentObserver
  -> compute desired bar LP from stored full/base LP
  -> only if geometry changed:
       WindowManager.updateViewLayout()
```

This avoids self-feedback.

### 11.4 Runtime geometry

Correct vertical result:

```text
LeftCarSystemBar:
  [0,8][62,706]

RightCarSystemBar:
  [1002,8][1064,706]
```

Effective height:

```text
720 - 8 - 14 = 698 px
```

The safe-area behavior was visually confirmed stable after V6.

### 11.5 Safe-area toggle

SystemUI honors:

```text
arda_system_bar_respect_safe_area
```

V6 also observes the safe-area toggle, so switching the feature can reapply full versus safe geometry without relying on local window inset feedback.

---

## 12. ArdaPlatform

ArdaPlatform is the user-facing privileged configuration plane.

Local source:

```text
/Users/edo/Documents/Code/ArdaPlatform
```

Deployment model:

- user builds the app in Android Studio;
- do not run Gradle from source-patching automation;
- final privileged/system integration is through AOSP prebuilt packaging / `ArdaPlatformPriv`;
- avoid replacing the intended privileged deployment with ordinary `adb install`.

Current relevant settings include:

```text
arda_system_bar_respect_safe_area
arda_dock_show_recent_map
arda_dock_show_recent_app
arda_system_bar_layout_mode
arda_projection_content_insets
```

Older/raw stable inset keys may still exist for projection internals, but SystemUI safe-bar geometry should use the V6 atomic content inset source, not the old raw 120/120 values.

---

## 13. Dock / recent map / recent app

### 13.1 Desired behavior

Vertical dock order:

```text
pinned/static items
recent map
recent app
```

The recent map slot should immediately precede the general recent-app slot.

Both recent slots are controllable from ArdaPlatform.

### 13.2 Recent-map classifier crash

A critical SystemUI crash was found in the first recent-map implementation:

```text
IllegalArgumentException:
Can't set package name when selector is already set
```

Bad pattern:

```kotlin
Intent.makeMainSelectorActivity(
    Intent.ACTION_MAIN,
    Intent.CATEGORY_APP_MAPS,
).setPackage(component.packageName)
```

Correct safe construction:

```kotlin
Intent(Intent.ACTION_MAIN)
    .addCategory(Intent.CATEGORY_APP_MAPS)
    .setPackage(component.packageName)
```

The crash had caused repeated SystemUI restarts and transient disappearance of the left/right bars. This was a dock classifier bug, not a SurfaceFlinger/capture regression.

### 13.3 Persistent recents

Current Launcher source contains:

```text
ARDA_DOCK_PERSISTENT_RECENTS_V2
```

This is the current persistent recent-map/recent-app implementation marker.

### 13.4 Dock spacing

Multiple spacing layers were tested.

#### Parent divider — ineffective

`DockView` is a `FrameLayout`, so adding a LinearLayout divider to the outer XML did not affect RecyclerView children.

#### Item XML margin — not authoritative

`DockAdapter` replaces the inflated item `LayoutParams`, so XML margins are not a reliable runtime spacing source.

#### Final V1.3 spacing — correct layer

`DockView.kt` owns the real inner:

```text
RecyclerView
```

Current spacing is applied using:

```text
RecyclerView.ItemDecoration
```

Marker:

```text
ARDA_DOCK_REAL_RECYCLERVIEW_SPACING_V1_3
```

Current intended additional inter-item gap:

```text
6dp
```

Properties:

- applies uniformly to STATIC and DYNAMIC dock items;
- therefore includes recent map <-> recent app;
- preserves first/last outer gap;
- supports vertical/horizontal orientation;
- item XML margins are restored to stock `7dp/7dp`;
- visually confirmed working.

---

## 14. Build and deploy conventions

### 14.1 CarSystemUI build

Typical build-host sequence:

```bash
cd /home/edo/aosp-rpi-aaos16

set +u
source build/envsetup.sh
lunch aosp_rpi4_car-bp4a-userdebug
set -u

m CarSystemUI
```

Expected artifact:

```text
out/target/product/rpi4/system/system_ext/priv-app/CarSystemUI/CarSystemUI.apk
```

### 14.2 Device path

Current deployment path used successfully:

```text
/system/system_ext/priv-app/CarSystemUI/CarSystemUI.apk
```

AOSP/device mount presentation may show `system_ext` under `/system/system_ext`.

### 14.3 Atomic replacement

Preferred deploy pattern:

1. require DHU closed;
2. pin Ethernet ADB;
3. `adb root`;
4. `adb remount`;
5. backup current APK;
6. push to staging file in same filesystem;
7. verify SHA;
8. `chown/chmod/restorecon`;
9. atomic `mv`;
10. `sync`;
11. reboot;
12. wait for `sys.boot_completed=1`;
13. verify SystemUI/system_server process gates.

### 14.4 Build script reporting

Preferred final summary fields:

```text
SOURCE_WRITE=
GIT_WRITE=
BUILD=
DEPLOY=
REBOOT=
REPO_SYNC=
```

---

## 15. Shell/script traps already learned

### 15.1 `pipefail` and `grep | head`

Avoid pipelines that can return SIGPIPE/141 under `set -o pipefail`.

Especially avoid:

```bash
strings file | grep -qF marker
grep ... | head
```

when the producer can be terminated early.

Prefer temp files or non-short-circuiting checks.

### 15.2 `envsetup.sh` and `set -u`

Use:

```bash
set +u
source build/envsetup.sh
lunch ...
set -u
```

### 15.3 Screenshot stderr contamination

Some device `screencap` output was observed with MESA loader text before PNG bytes.

For binary screenshots:

- redirect stderr separately, or
- strip bytes before the PNG signature if necessary.

---

## 16. Current operational status — 2026-09-08

### Working / runtime-validated

```text
AAP media bring-up and live H.264 path lineage
direct USB accessory transport
full multitouch/platform input lineage
vehicle manager/router platformization
GNSS AIDL path lineage
projection display-profile reconcile
dual capture profile architecture
stable safe-area system bars via V6 atomic projection content insets
recent-map classifier crash fixed
persistent recent map/recent app source present
dock inter-item spacing V1.3 visually confirmed
```

### Important current caveats

- The latest Launcher/SystemUI changes were still uncommitted at merge time.
- Run the prepared V2 commit/push checkpoint and record the resulting SHAs.
- Real-HU boot-settle validation remained a separate deferred gate in the August checkpoint.
- Performance B2.1 remains optional; stable direct profile is the default.
- Do not regress mismatch scaling back into direct physical-frame clone logic.

---

## 17. Important paths

```text
Mac:
  /Users/edo/Documents/Code/aaos-aap
  /Users/edo/Documents/Code/ArdaPlatform

Ubuntu:
  /home/edo/aosp-rpi-aaos16

Launcher:
  packages/apps/Car/Launcher

SystemUI:
  packages/apps/Car/SystemUI

SystemUI safe-area core:
  packages/apps/Car/SystemUI/src/com/android/systemui/car/systembar/SystemBarConfigsImpl.java

Inner safe-area view lineage:
  packages/apps/Car/SystemUI/src/com/android/systemui/car/systembar/ArdaSafeAreaCarSystemBarView.java

Dock persistent recents:
  packages/apps/Car/Launcher/docklib/src/com/android/car/docklib/DockViewModel.kt

Dock real RecyclerView spacing:
  packages/apps/Car/Launcher/docklib/src/com/android/car/docklib/view/DockView.kt
```

---

## 18. Quick recovery checklist

If the system appears visually wrong after UI work:

1. Determine whether SystemUI is crashing before blaming capture.
2. Check:
   ```bash
   pidof com.android.systemui
   logcat | grep -E 'FATAL EXCEPTION|Process: com.android.systemui'
   ```
3. Inspect WMS:
   ```bash
   dumpsys window
   dumpsys window displays
   ```
4. For safe bars, compare:
   ```text
   arda_projection_content_insets
   statusBars/navigationBars provider frames
   Left/RightCarSystemBar frames
   ```
5. For dock issues, remember:
   ```text
   DockView = FrameLayout
   real child list = inner RecyclerView
   ```
6. Do not `repo sync`.

---

## 19. Historical version map

The following milestones are still useful when tracing why a subsystem exists:

```text
v24–v31  AAP channel/media/H.264 bring-up
v39–v41  input injector and first-frame behavior
v42–v57  sensor/VHAL/vehicle model architecture
v44–v52  USB AOA/direct accessory and config profile
Phase 2  vehicle manager/router platformization
Phase 3  GPS/GNSS bridge and AIDL GNSS HAL
RN-3B    Wi-Fi handoff
Aug 2026  multitouch, GPS persistence, projection safe-area POC
Aug/Sep  capture profile and direct/HWC performance work
Sep 2026  SystemUI real safe-window V6 + dock recents/spacing
```

---

# Appendix A — Original README sources, preserved verbatim

The canonical sections above supersede old statements such as “current task” or “current checkpoint” where later work exists. The source text below is intentionally preserved so no implementation detail is lost.


---

## Source Appendix 1: `README.md`


# arda-senderd

v24c: fixes rust-protobuf generated API usage for optional Service submessages.

The current generator exposes optional message fields as `Option<T>` public fields
(e.g. `svc.media_sink_service`) rather than `has_media_sink_service()` /
`media_sink_service()` helpers. ServiceDiscoveryResponse parsing now uses
`if let Some(...) = ...as_ref()` and `.is_some()` for marker services.

## v24d channel-open service-channel fix

DHU rejects CHANNEL_OPEN_REQUEST when it is sent as a channel-0 control message.
AAP traces use a service-channel control frame for channel open:

- channel = selected service_id
- flags = ENCRYPTED | CONTROL | FIRST | LAST (`0x0f`)

`aap-session-probe` now sends the selected video `CHANNEL_OPEN_REQUEST` on the
selected service channel with the CONTROL flag preserved before TLS encryption.


## v25 media start probe

- Decodes non-control media-channel payloads such as `MEDIA_MESSAGE_CONFIG`.
- After selected video channel opens, sends encrypted `MEDIA_MESSAGE_START` when HU reports `MEDIA_MESSAGE_CONFIG STATUS_READY`.
- Keeps ping response/session behavior unchanged.

## v26 media setup probe

After the video service channel is opened, `aap-session-probe` now sends a
`MEDIA_MESSAGE_SETUP` payload on the opened media channel using the generated
AAP protobuf structs. The current requested codec is H.264 baseline
(`MEDIA_CODEC_VIDEO_H264_BP`), matching DHU's advertised first video config.

Expected follow-up from DHU is `MEDIA_MESSAGE_CONFIG STATUS_READY`; existing
logic then sends `MEDIA_MESSAGE_START`.

## v27 - docs-based H264 file DATA probe

Adds optional `--video-file PATH` / `--video-frames N` to `aap-session-probe`.
After docs-confirmed AV sequence (`SETUP` -> `CONFIG STATUS_READY` -> `START`), the session can send Annex-B H264 using:

- `MEDIA_MESSAGE_CODEC_CONFIG` (`0x0001`): raw H264 SPS/PPS bytes
- `MEDIA_MESSAGE_DATA` (`0x0000`): `u64` big-endian PTS in microseconds + raw H264 NAL bytes
- standard AAP plaintext fragmentation before TLS encryption for large payloads
- `MEDIA_MESSAGE_ACK` (`0x8004`) tracking to respect `max_unacked`

Example:

```bash
adb shell /data/local/tmp/arda-senderd aap-session-probe \
  --addr 0.0.0.0:5277 \
  --keys /data/local/tmp/arda-certs \
  --dump /data/local/tmp/arda-aap-session.log \
  --response 1.7 \
  --max-frames 2000 \
  --video-file /data/local/tmp/arda-test.h264 \
  --video-frames 60
```

## v28 - native-capture live video bridge

Adds `--video-native` to `aap-session-probe`.

After the docs-confirmed media sequence succeeds (`CHANNEL_OPEN_RESPONSE` ->
`MEDIA_MESSAGE_SETUP` -> `MEDIA_MESSAGE_CONFIG STATUS_READY` ->
`MEDIA_MESSAGE_START`), the session now spawns this same binary as an internal
`native-capture --out -` child and streams its Annex-B H264 output into the
opened AAP video channel.

The live path reuses the v27 docs-based media packet format:

- `MEDIA_MESSAGE_CODEC_CONFIG` (`0x0001`) for SPS/PPS
- `MEDIA_MESSAGE_DATA` (`0x0000`) for `u64` big-endian PTS + raw H264 NAL bytes
- existing plaintext fragmentation-before-TLS for large video payloads
- existing `MEDIA_MESSAGE_ACK` flow control using HU `max_unacked`

Example:

```bash
adb shell /data/local/tmp/arda-senderd aap-session-probe \
  --addr 0.0.0.0:5277 \
  --keys /data/local/tmp/arda-certs \
  --dump /data/local/tmp/arda-aap-session.log \
  --response 1.7 \
  --max-frames 5000 \
  --video-native \
  --video-size 800x480 \
  --video-bit-rate 2500000 \
  --video-fps 30 \
  --video-frames 0
```

## v29 live native video notes

- Native stdout mode is now unbuffered and flushes every MediaCodec output buffer so the AAP live-video reader can see SPS/PPS and frame NALs immediately.
- `aap-session-probe --video-native` can auto-configure native capture size/fps from the selected SDR `VideoConfiguration`.
- Use `--no-video-auto-config` to force manual `--video-size` / `--video-fps` values.

## v30 live video pump fix

- After `MEDIA_MESSAGE_START`, the session receive timeout is reduced to 20ms so live native video can be pumped between HU frames instead of only when ping/focus frames arrive.
- `MEDIA_MESSAGE_ACK` is treated as releasing at least one in-flight media buffer. The DHU trace reports `ack=1` repeatedly, so treating it as a cumulative counter stalls after two frames.
- This fixes the symptom where the first IDR frame appears in DHU, then the display freezes while pings continue.

Test:

```bash
adb push target/aarch64-linux-android/release/arda-senderd /data/local/tmp/arda-senderd
adb shell chmod 755 /data/local/tmp/arda-senderd
adb shell /data/local/tmp/arda-senderd aap-session-probe \
  --addr 0.0.0.0:5277 \
  --keys /data/local/tmp/arda-certs \
  --dump /data/local/tmp/arda-aap-session.log \
  --response 1.7 \
  --max-frames 5000 \
  --video-native \
  --video-bit-rate 2500000 \
  --video-frames 0
```

## v31: SDR resolution/density/margin aware video config

- `aap-session-probe` can now select HU video config with `--video-config-policy highest|balanced|lowest|index:N`.
- SDR video config logs now include codec stream size, density, real_density, per-side margins, content insets, stable insets, content area and safe area.
- Optional `--apply-wm-config` applies selected SDR config to Android WindowManager before native capture starts.
  - `--wm-size-mode stream` uses codec resolution, for example 1920x1080.
  - `--wm-size-mode content` uses codec resolution minus HU margins.
  - `--wm-size-mode safe` uses content size minus content insets.
  - `--wm-density-mode density|real` selects SDR density or real_density.
- Default remains safe: no `wm` changes are applied unless `--apply-wm-config` is given.

Reset if needed:

```bash
adb shell wm size reset
adb shell wm density reset
adb reboot
```

### v39 app_process input injector

`--input-inject` defaults to the older safe `uinput` backend. The app_process backend is kept as an explicit experimental mode: the Rust binary is still the only process started manually, then it writes an embedded `arda-inputd.jar`, launches it with `app_process`, sends mapped AA touchscreen/key events to it, and stops it when the session exits.

Fallback modes are still available:

```bash
--input-inject-mode uinput   # default safe mode, old /dev/uinput virtual touchscreen
--input-inject-mode app      # experimental InputManager.injectInputEvent via app_process
--input-inject-mode shell    # slow /system/bin/input motionevent fallback
```

The Android Docker build installs SDK platform/build-tools because `build.rs` compiles the embedded input daemon with `javac` + `d8`.


## v39d emergency note

`--input-inject` no longer defaults to app_process injection. Default is back to `uinput` so the standard arda test command cannot start `arda-inputd` accidentally. Use `--input-inject-mode app` only for explicit experiments.

### v39e app-process input safety

`--input-inject-mode app` is kept available because drag/touch can work on AAOS, but it is no longer started during ServiceDiscovery. It starts lazily on first actual injected event, uses conservative Gearhead-like MotionEvent parameters, ignores stray MOVE/UP events without a live DOWN, and throttles MOVE events by default.

Useful knobs:

```bash
--input-inject-mode app
--input-app-move-min-ms 16   # default; use 0 to disable MOVE throttling, 32 to be safer
```

### v41j first-frame render kick modes

If native video waits for a screen change before producing the first slice NAL, use one of:

```bash
--video-first-frame-kick-cancel
--video-first-frame-kick-center
--video-first-frame-kick-wakeup
--video-first-frame-kick-mode keyevent --video-first-frame-kick-keycode WAKEUP
```

`touch-cancel` is intended to dirty the input/display pipeline without an ACTION_UP click. `center` is a real tap at stream center and can activate UI. `HOME` is supported but not recommended unless explicitly testing navigation behavior.

## v44 AAOS USB-C AOA gadget bootstrap — legacy/manual path

The original v44 code auto-created/rebound a custom configfs AOA gadget before every AAP probe. That behavior was useful on the older image, but the current AAOS image already initializes USB accessory support.

As of v52, normal AAP commands **do not mutate USB gadget state and do not run `su` preflight attempts**. The old helper remains only for manual recovery on legacy images:

```bash
arda-senderd aaos-aoa-gadget --check
arda-senderd aaos-aoa-gadget --unbind
arda-senderd aaos-aoa-gadget --udc fe980000.usb
```

For the current image use the read-only check:

```bash
arda-senderd usb-accessory-status
```

The current baseline consumes the HU-side `/dev/usb_accessory` gadget stream directly with `transport.mode = "usb-accessory"` in `config.toml`. See:

```text
docs/ARDA_TEST_PROFILES.md
docs/AAOS_AOA_USB_GADGET.md
```

## v52 config profile + direct USB AAP transport

The long bring-up command is now stored in repository-root `config.toml`. The baseline uses platform `screenrecord` H.264, Waveshare 7.9 geometry, app/framework input injection, unlimited video frames, and direct `/dev/usb_accessory` AAP transport.

Push the profile and run:

```bash
adb push config.toml /data/local/tmp/arda-config.toml
adb shell /data/local/tmp/arda-senderd usb-accessory-status
adb shell /data/local/tmp/arda-senderd config-check /data/local/tmp/arda-config.toml
adb shell /data/local/tmp/arda-senderd aap-run
```

For the normal Mac test workflow, use the repository helper instead of reconstructing the long command:

```bash
./scripts/arda-usb-run.sh
```

Pass `--build` to compile first, or append one-off config overrides after `--`:

```bash
./scripts/arda-usb-run.sh --build
./scripts/arda-usb-run.sh -- --log-level summary
```

Explicit CLI flags override config values for one-off A/B tests. TCP remains available as a debugging fallback with `--transport tcp --addr 0.0.0.0:5277`.

## v46 AAOS Google Maps Automotive notes

Adds documentation for the recovery-less Google Maps Automotive / GApps experiment on the ARDA Raspberry Pi 4 AAOS image:

```text
docs/AAOS_GOOGLE_MAPS_AUTOMOTIVE.md
```

Key result: Google Maps Automotive became usable only after keeping GMS/GSF enabled and adding explicit Dalvik heap settings to `/system/build.prop`. Before the heap fix, Maps was clamped around 16 MB Java heap and spammed blocking GC; after tuning, Maps CPU dropped to a reasonable startup load and the UI became responsive enough to open Maps settings/location.

The doc also records external push/download links for the tested MindTheGapps Android 11 ARM64 package and Google Maps Automotive APKMirror package, plus the exact push/install/rescue commands.

## v47 AAOS phone internet bridge bootstrap

Adds first-step AAOS-side helpers for getting the Raspberry Pi 4 AAOS image online through a phone.

Primary MVP mode: phone opens a normal mobile hotspot, AAOS joins it as a Wi-Fi client:

```bash
adb shell /data/local/tmp/arda-senderd net-join \
  --ssid ARDA-Phone \
  --psk 'change-me-1234' \
  --security wpa2

adb shell /data/local/tmp/arda-senderd net-status
```

Cleanup:

```bash
adb shell /data/local/tmp/arda-senderd net-disconnect --ssid ARDA-Phone
adb shell /data/local/tmp/arda-senderd net-proxy-clear
```

Proxy experiment mode: companion app exposes an HTTP proxy on the phone-side Wi-Fi address, then AAOS sets Android global HTTP proxy:

```bash
adb shell /data/local/tmp/arda-senderd net-proxy --host 192.168.43.1 --port 8080
```

Reverse mode is also sketched for experiments: AAOS opens a SoftAP, phone joins that AP, and the phone companion binds outbound sockets to cellular while accepting proxy traffic from AAOS. First try the saved Hotspot settings config shown in the AAOS UI:

```bash
adb shell /data/local/tmp/arda-senderd net-ap-start --force-unsafe-softap --use-saved-config --wait-ms 8000
adb shell /data/local/tmp/arda-senderd net-ap-stop
```

Custom SSID/password is still available when the image honors shell SoftAP config:

```bash
adb shell /data/local/tmp/arda-senderd net-ap-start --force-unsafe-softap --ssid ARDA-AAOS --psk arda-aaos-1234 --security wpa2 --wait-ms 8000
```

`net-ap-start` is intentionally experimental and disabled by default because shell SoftAP support depends on the AAOS Wi-Fi framework/HAL image; pass `--force-unsafe-softap` only when testing with ADB rescue ready. See:

```text
docs/AAOS_PHONE_INTERNET_BRIDGE.md
```

### v48 AAOS SoftAP/tether diagnostics

After testing on the Raspberry Pi AAOS image, `cmd wifi start-softap` can return success while the Settings Hotspot UI still appears disabled. `net-status` now prints SoftAP/tethering diagnostics from `dumpsys wifi`, `dumpsys connectivity`, and all Wi-Fi/AP-like interfaces so this can be verified from ADB.

`net-ap-start` has two unsafe test paths, both requiring `--force-unsafe-softap`:

```bash
adb shell /data/local/tmp/arda-senderd net-ap-start --force-unsafe-softap --use-saved-config --wait-ms 8000
adb shell /data/local/tmp/arda-senderd net-ap-start --force-unsafe-softap --ssid ARDA-AAOS --psk arda-aaos-1234 --security wpa2 --wait-ms 8000
```

The first path starts the Hotspot settings UI configuration through Connectivity tethering when the image supports it; the second path keeps the previous custom shell SoftAP experiment. `net-join` also no longer treats `Wifi is not connected` as a successful connection match.

### v49 SoftAP bootloop guard and Wi-Fi rescue

Testing showed that the Raspberry Pi AAOS image can accept `startSoftAp` but fail later at hostapd/HAL setup. In that state the device can enter an Android userspace restart loop until Wi-Fi config stores are moved out of the way.

To avoid accidental repeats, `net-ap-start` is now disabled by default. It only runs with an explicit unsafe flag:

```bash
adb shell /data/local/tmp/arda-senderd net-ap-start --force-unsafe-softap --use-saved-config --wait-ms 8000
```

Do not put this in init on the current image. The safe direction is still:

```text
phone hotspot -> AAOS net-join
```

Recovery helper if SoftAP/Wi-Fi state poisons boot again:

```bash
adb wait-for-device
adb root
adb wait-for-device
adb shell /data/local/tmp/arda-senderd net-wifi-rescue
adb reboot
```

or, with explicit stop plus rescue:

```bash
adb shell /data/local/tmp/arda-senderd net-ap-stop --rescue-configs
adb reboot
```

Reverse mode remains a valid future architecture once AP mode is fixed:

```text
AAOS AP always on -> phone joins AAOS -> companion proxy listens on Wi-Fi -> companion outbound binds to cellular -> AAOS uses HTTP/SOCKS/tun2socks
```



---

## Source Appendix 2: `README-RN3B-HYBRID.md`


# RN-3B hybrid senderd patch

Apply these changed/new files over the canonical `120720261432.zip` source.

Replaced:

```text
src/main.rs
```

New:

```text
src/bluetooth/mod.rs
scripts/run-rn3b-hybrid-bridge-android.sh
scripts/build-run-rn3b-hybrid-macos.sh
```

New senderd command:

```text
arda-senderd bt-bootstrapd
```

RN-3B debug bridge:

```text
127.0.0.1:5283 only
bridge token required
```

Production-ready transport option already exists in the command:

```text
--transport unix
--socket /dev/socket/arda_bt_bridge
--allow-uid UID
```

The Unix option is not deployed through init/SELinux until the RN-3B runtime
gate passes.

Defaults:

```text
keep_bootstrap_rfcomm_open=true
keep_bluetooth_profiles_connected=true
```

Build and run on macOS:

```bash
chmod +x scripts/build-run-rn3b-hybrid-macos.sh \
  scripts/run-rn3b-hybrid-bridge-android.sh

./scripts/build-run-rn3b-hybrid-macos.sh
```

To reuse an already-built Android binary:

```bash
SKIP_BUILD=1 ./scripts/build-run-rn3b-hybrid-macos.sh
```

## RN-3B Wi-Fi handoff MVP

The Bluetooth/RFCOMM bootstrap remains open, but bulk/control continuation can move to a direct token-authenticated TCP connection over the phone hotspot.

New senderd options:

```text
--enable-wifi-handoff
--wifi-handoff-addr 0.0.0.0:5284
```

The Android run script enables:

```text
local ArdaPlatform bridge: 127.0.0.1:5283
Wi-Fi handoff listener:    0.0.0.0:5284
```

Protocol additions, version 1:

```text
10 INTERNET_OFFER   companion -> senderd
11 INTERNET_RESULT  senderd -> companion
12 INTERNET_STATUS  companion -> senderd
```

Phone-hotspot offer payload:

```json
{
  "mode": "phone_hotspot",
  "ssid": "ARDA-Phone",
  "psk": "change-me-1234",
  "security": "wpa2",
  "wait_ms": 30000
}
```

Successful result:

```json
{
  "status": "ready",
  "mode": "phone_hotspot",
  "ssid": "ARDA-Phone",
  "transport": "wifi_tcp",
  "aaos_ip": "192.168.43.123",
  "port": 5284,
  "http_proxy": null,
  "keep_rfcomm_open": true
}
```

After this result, the phone companion connects to `aaos_ip:5284`, performs the same `ARDBR001` token authentication, then sends the normal `HELLO` and `PING` frames over Wi-Fi. ArdaPlatform does not need to proxy the new Wi-Fi connection; it remains the Bluetooth bootstrap bridge.

The current source archive does not contain the Android phone companion source. The companion must still be updated to start/inspect its hotspot, send `INTERNET_OFFER`, read `INTERNET_RESULT`, and reconnect to the returned Wi-Fi endpoint.

Mac senderd-side smoke helper:

```bash
DEVICE=1000000028c0b59c \
SSID='ARDA-Phone' \
PSK='change-me-1234' \
./scripts/test-rn3b-wifi-handoff-macos.sh
```

Use USB ADB for this smoke where possible. Network ADB can disappear when AAOS leaves the current Wi-Fi network and joins the phone hotspot; the real Bluetooth bridge does not have that limitation.

Direct Wi-Fi listener gate on the current LAN, before switching to a phone hotspot:

```bash
AAOS_HOST=192.168.1.21 \
./scripts/test-rn3b-wifi-listener-macos.sh
```

Expected:

```text
ARDA_RN3B_WIFI_DIRECT_GATE=PASS
```



---

## Source Appendix 3: `README_PHASE1_1_FIX.md`


# ARDA v57 Phase 1.1 — Direct VehicleProperty correction

Baseline: the user's current v57 Phase 1 working checkout, produced from
`arda-v57-wip-phase1-full-source-20260707.zip`.

This package is a correction over that CURRENT Phase 1 checkout. Do not replay
older v53/v54/v56 patches and do not restart from 070620261158 for this test.

## What changed

1. rust-protobuf v3 generated accessors are explicitly enabled in build.rs.
   Existing router code intentionally uses the accessor API consistently.

2. The extra ARDA generic vehicle signal-ID namespace was removed.

3. VehicleSignal and VehicleCommand now carry Android VehicleProperty IDs
   directly as int32 `property_id` fields.

4. HU/AAP sensor semantics are mapped to known standard VehicleProperty IDs in
   Rust only.

5. The builtin Wasm ABI v2 module is a generic typed pass-through:
   vehicle property -> VHAL event
   VHAL set -> vehicle property command

6. The static Rust property registry is validation/authorization metadata, not
   a mapping table. It holds known type, area, access/capability and range rules.

7. Source arbitration still occurs before policy processing.

8. Request and confirmed state remain separate.

## Intentionally unchanged

- AAP/TLS/SDR parser and protocol path
- secure platform input bridge
- capture/video source path

## Compile status

Static checks passed in the packaging environment:
- git diff --check
- no stale `signal_id` API remains in proto/src/wasm/docs except historical text
- no hardcoded VHAL property integer IDs remain in the builtin WAT
- builtin WAT branches only on four scalar value types in each direction
- build.rs explicitly requests generated protobuf accessors

The actual Android Rust Docker cross-build has not been executed in this
packaging environment. The user's `./scripts/docker-build.sh` result is the
authoritative compile validation.

## Expected smoke markers after a successful build

SENSOR_GATE_SMOKE_OK=YES
SENSOR_WASM_SMOKE_OK=YES
VEHICLE_POLICY_SMOKE_GENERIC_PROPERTY_PASS_THROUGH=YES
VEHICLE_POLICY_SMOKE_OK=YES
VEHICLE_ROUTER_SMOKE_ARBITRATION_OK=YES
VEHICLE_ROUTER_SMOKE_PROPERTY_ID_COMMAND_OK=YES
VEHICLE_ROUTER_SMOKE_OK=YES

## Build follow-up after the first Phase 1.1 compile

The first user-side Android build still produced the old 204 rust-protobuf accessor `E0599`
cascade. The generated-file shape showed that a no-accessor `arda_vehicle_router.rs` was being
compiled even though the current `build.rs` requests accessors.

The Docker target build now cleans only the `arda-senderd` package artifacts for the selected Android
target before compiling. This is intentional: Phase patches/checkpoints are commonly restored with
`rsync -av`, which preserves archive mtimes, and stale Cargo build-script/output fingerprints must not
survive across codegen contract changes.

`build.rs` also verifies the generated router source immediately after protobuf codegen. A successful
build must print an `ARDA_PROTO_ACCESSORS_OK=.../arda_vehicle_router.rs` warning before Rust reaches
the router sources. Missing accessors now fail as one build-script error instead of hundreds of
`E0599` messages.

## Runtime follow-up after the first successful Android build

The Android cross-build passed. The first runtime smokes then found two independent test-path issues:

1. `sensor_event_to_vehicle_signals()` emitted built-in AAP signals with reserved `source_id=0`.
   Router forwarding hid the bug because `SensorRouterPublisher` overwrote the source with 1, while
   dry-run policy validation correctly rejected source zero before Wasm. The mapping now emits
   `AAP_SENSOR_SOURCE_ID=1` directly and the router registration uses the same constant.

2. A combined ADB shell wrapper used `pkill -f "arda-senderd vehicle-routerd"`. That pattern can match
   the remote `sh -c` command line that contains the pattern itself and kill the test shell. Use
   `scripts/run-vehicle-router-smoke-android.sh`; it records the daemon PID, kills only that PID, and
   uses `/data/local/tmp/arda_vehicle_router_smoke.sock` so the normal router socket is untouched.

Remaining Phase 1.1 gate:

```text
SENSOR_WASM_SMOKE_OK=YES
VEHICLE_ROUTER_SMOKE_ARBITRATION_OK=YES
VEHICLE_ROUTER_SMOKE_PROPERTY_ID_COMMAND_OK=YES
VEHICLE_ROUTER_SMOKE_OK=YES
```



## Router smoke privilege follow-up

The corrected sensor smoke is now green:

```text
SENSOR_WASM_EVENT ... source=1 ...
SENSOR_WASM_TRAP=NONE
SENSOR_WASM_SMOKE_OK=YES
```

The next router run reached the daemon and exposed a separate startup privilege mismatch. The router
prints the Wasmtime ready line before binding/configuring its Unix socket, then requires the socket to
be owned by `root:system` (`0:1000`). Launching `vehicle-routerd` as the normal ADB shell user cannot
perform that ownership change and exits with `EPERM` before `VEHICLE_ROUTER_READY`.

Do not make socket ownership best-effort and do not special-case `/data/local/tmp` by ignoring `chown`
failures. The ownership check is part of the router's intended security boundary.

Use the root-aware runner:

```bash
DEVICE=192.168.1.15:5555 ./scripts/run-vehicle-router-smoke-android.sh
```

It requests `adb root`, reconnects, verifies `id -u == 0`, and only then launches the PID-scoped smoke
router. Expected startup marker:

```text
ROUTER_ADB_ROOT_OK device=... uid=0 ...
```

The Rust router startup path now annotates I/O failures with stage/path/EUID/EGID. An ownership failure
will identify `stage=chown_socket_root_system_requires_privilege` rather than returning only
`Operation not permitted (os error 1)`.
