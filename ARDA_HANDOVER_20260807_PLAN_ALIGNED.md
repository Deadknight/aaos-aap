# ARDA AAOS / Android Auto AAP — Plan-Aligned Current Handover
**Date:** 2026-08-07  
**Source plan:** `Archive(5).zip / ARDA_PLAN.md`  
**Purpose:** Continue the ARDA project in a new conversation from the current canonical state, without accidentally returning to an obsolete milestone.

---

# 0. PLAN INTERPRETATION RULE

`ARDA_PLAN.md` is a chronological working log that has grown over time.

The line near the top:

```text
Current milestone: TCP + TLS + auth + service discovery + ping response works.
```

is **no longer the active milestone**. It is an old heading left over from the v24 period.

The v52–v57, Phase 2.x, Phase 3.x, and RN-3B sections further down are newer and supersede that old heading.

In a new conversation:
- Do not return to old probe stages such as v24/v25/v27.
- Do not attempt to re-implement closed phases.
- The current active work is the **Projection Content Insets / CarSystemBar Safe Area** section at the end of this handover.

---

# 1. WORKING RULES

- **NEVER run `repo sync`.**
- Make only targeted file/module changes in the existing local AOSP/source tree.
- **Do not commit/push unless the user explicitly requests it.**
- Preserve existing unrelated dirty source files.
- Do not generate unnecessary SHA-256 hashes.
- Consider a framework patch only when the issue cannot be solved with a vendor/platform helper.
- ADB priority:
  1. USB `1000000028c0b59c`
  2. `192.168.1.185:5555`
  3. `192.168.1.15:5555`
- Once an ADB transport is selected, keep it pinned through root/remount/push/reboot/verify.
- Assume SELinux Enforcing.
- The user usually prefers a script/ZIP + runtime-gate workflow.

---

# 2. ENVIRONMENT

## Mac
```text
host/user:
edo@Edos-MacBook-Pro-3

aaos-aap canonical source:
/Users/edo/Documents/Code/aaos-aap

branch:
main

remote:
origin

build SSH key:
~/.ssh/id_ed25519_arda_build
```

## Ubuntu build host
```text
edo@192.168.1.55

AOSP:
/home/edo/aosp-rpi-aaos16

lunch:
aosp_rpi4_car-bp4a-userdebug

Android:
16 / AAOS 16

Build ID:
BP4A.251205.006
```

## Vendor repos
```text
/home/edo/aosp-rpi-aaos16/vendor/arda
branch arda-aaos16

/home/edo/aosp-rpi-aaos16/vendor/arda/ardaplatform
branch main
```

## Device paths
```text
/system_ext/bin/arda-connectivityd
/product/etc/arda/arda-aap.toml
/system_ext/priv-app/ArdaInputBridge/ArdaInputBridge.apk
```

---

# 3. PLAN HISTORY — WHAT MUST NOT BE REDONE

## v24–v31
AAP base transport/media bring-up period.

Completed baseline:

```text
TCP
TLS
auth
ServiceDiscovery
ChannelOpen
Media SETUP
Media CONFIG
Media START
H264 CODEC_CONFIG / DATA
ACK flow control
native capture
SDR config selection
margins/content-insets parsing
```

These are now historical baseline items.

---

# 4. v39–v54 INPUT / PLATFORM INPUT

Important evolution from the plan:

```text
v39:
app_process InputManager injector

v39d/e:
app injector emergency/non-default path
uinput becomes default again

v53:
platform-integrated input bridge

v54:
secured platform input bridge transport
```

The current canonical state is further ahead:

## Full multitouch — CLOSED/PASS

Rust:
- full pointer snapshot
- pointer IDs
- action index

Java:
```text
ArdaInputApplication.java
snapshot-reconcile-v1
```

Runtime gates:

```text
INPUT_BRIDGE_LINE_FAILURE=NONE
INPUT_MANAGER_INJECTION_FAILURE=NONE
NORMALIZED_DOWN=PASS
NORMALIZED_POINTER_DOWN=PASS
MULTIPOINTER_MOVE=PASS
NORMALIZED_POINTER_UP=PASS
NORMALIZED_UP=PASS
MAX_POINTERS_OBSERVED=2
REQUIRED_POINTER_COUNT=PASS
RELEASE_ACTION_NORMALIZED=PASS
ARDA_FULL_MULTITOUCH_RECONCILER_VERIFY=PASS
```

Maps pinch zoom was visually verified in DHU.

Do not redesign this area.

---

# 5. v42–v57 SENSOR / VEHICLE ARCHITECTURE

The sensor architecture evolved several times according to the plan.

## v55
Explicit sensor gate + normalized event boundary.

## v56
Wasmtime VHAL translation/policy layer.

## v57 Phase 1.1
Final canonical direction:

```text
AAP/HU sensor
  -> Rust known exact mapping
  -> Android VehicleProperty ID + area + typed value
  -> vehicle-routerd
  -> arbitration
  -> generic typed Wasmtime pass-through
  -> same VehicleProperty VHAL event
```

The Wasm default policy is no longer a signal-ID mapping table.

AAP sensor ID → Android VehicleProperty mapping is performed in Rust at the known exact semantic boundary.

Ambiguous semantics are not forced into VHAL mappings.

## Phase 1.1 — CLOSED

Runtime:

```text
SENSOR_WASM_TRAP=NONE
SENSOR_WASM_SMOKE_OK=YES
VEHICLE_POLICY_SMOKE_OK=YES
VEHICLE_ROUTER_SMOKE_ARBITRATION_OK=YES
VEHICLE_ROUTER_SMOKE_PROPERTY_ID_COMMAND_OK=YES
VEHICLE_ROUTER_SMOKE_OK=YES
```

Do not reopen this phase.

---

# 6. VEHICLE MANAGER — PHASE 2.0 / 2.1 / 2.2 CLOSED

Authoritative result from the plan:

```text
VHAL SET
  -> vehicle-routerd
  -> per-source Manager bridge
  -> one-way AIDL adapter command
  -> CommandAck OK
  -> adapter confirmed-state publish
  -> vehicle-routerd
  -> VHAL event
```

Runtime Enforcing gate:

```text
MANAGER_BRIDGE_MANAGER_DOMAIN=PASS
MANAGER_BRIDGE_ROUTER_REGISTRATION=PASS
VEHICLE_MANAGER_BRIDGE_SMOKE_SET_HANDLED=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_CONFIRMED_STATE=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_ACK_ROUND_TRIP=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_OK=YES
MANAGER_BRIDGE_ENFORCING_RUNTIME_GATE=PASS selinux=Enforcing
```

Platform contexts:

```text
/system_ext/bin/arda-senderd
  u:object_r:arda_vehicle_router_exec:s0

/dev/socket/arda_vehicle_router
  u:object_r:arda_vehicle_router_socket:s0

arda-senderd
  u:r:arda_vehicle_router:s0

com.arda.vehicle.manager
  u:r:arda_vehicle_manager:s0
```

## Canonical ownership

Android Vehicle Manager/AIDL/SEPolicy/init source:

```text
Deadknight/android_vendor_arda
branch arda-aaos16
vehicle/
canonical commit:
b53e5473d31de98573cb426c6618bdc9cc699e07
```

Device integration checkpoint:

```text
Deadknight/android_device_brcm_rpi4
branch arda-aaos16
commit:
0b13b2a565056f0b32a23abe9acc362401524b1a
```

The duplicate Android vehicle platform tree in `aaos-aap` is no longer canonical.

---

# 7. GPS / GNSS — PHASE 3.0B3 CLOSED

The Phase 3.0B/3.0B2 framework/provider experiments in the plan are superseded.

Final design:

```text
AAP SensorBatch.location_data
  -> exact Rust decode
  -> PlatformGnssHalBridge
  -> arda_gnss_hal socket
  -> vendor android.hardware.gnss.IGnss/default
  -> stock GnssNative
  -> stock GPS_PROVIDER
  -> GMS fused / Maps
```

Final platform identity:

```text
Soong:
android.hardware.gnss-service.arda

Executable:
/system_ext/bin/hw/android.hardware.gnss-service.arda

Init service:
arda.gnss-hal

AIDL:
android.hardware.gnss.IGnss/default

SELinux:
arda_gnss_hal
```

Location is:
- outside VehicleProperty
- outside VHAL
- outside Wasm

Runtime closure:

```text
ARDA_PHASE3_0B3_STANDARD_GNSS_AIDL_SERVICE=PASS
ARDA_PHASE3_0B3_STOCK_GPS_PROVIDER_PATH=PASS
ARDA_PHASE3_0B3_MAPS_LOCATION_DELIVERY=PASS
ARDA_PHASE3_0B3_GMS_FUSED_DELIVERY=PASS
ARDA_PHASE3_0B3_ACTIVE_FIX_REPLAY=PASS
ARDA_PHASE3_0B3_MAPS_WAITING_FOR_GPS_RESOLVED=YES
ARDA_PHASE3_0B3_REAL_AAP_LOCATION_RUNTIME=PASS
ARDA_PHASE3_0B3_LOCATION_CLOSED=YES
```

Canonical GNSS checkpoints:

```text
vendor GNSS:
72fe5c085ce32c1a43452c05b75eb92d888c9156

Ubuntu AOSP device integration:
2551dd2cdcabef976f3d8c66b9d56772034f2b82

canonical android_device_brcm_rpi4 GNSS:
787c5b67e54b31e22af70c918481417ef149fb2f
```

`GpsSatelliteData` → Android GnssSvInfo mapping is still intentionally not implemented:
AAP does not provide exact constellation/baseband C/N0 semantics.

---

# 8. PHASE 3.0C SENSOR MAPPING POLICY

Authoritative exact mappings from the plan:

AAP source `1`:

```text
SENSOR_SPEED(3)
 -> PERF_VEHICLE_SPEED

SENSOR_ODOMETER(5)
 -> PERF_ODOMETER

SENSOR_FUEL(6).low_fuel_warning
 -> FUEL_LEVEL_LOW
 only when optional proto field present

SENSOR_PARKING_BRAKE(7)
 -> PARKING_BRAKE_ON

SENSOR_GEAR(8)
 -> GEAR_SELECTION

SENSOR_NIGHT_MODE(10)
 -> NIGHT_MODE
 only when optional proto field present

SENSOR_TOLL_CARD(22)
 -> ELECTRONIC_TOLL_COLLECTION_CARD_STATUS
```

Intentionally not mapped to VehicleProperty:

```text
LOCATION
COMPASS
DRIVING_STATUS_DATA
ACCELEROMETER_DATA
GYROSCOPE_DATA
GPS_SATELLITE_DATA
raw ambiguous fuel/range
```

Do not fabricate units or semantics just to increase the mapping count.

---

# 9. RN-3B WI-FI HANDOFF

Plan state:

Validated base:

```text
127.0.0.1:5283 bridge
token auth
HELLO / HELLO_ACK
PING / PONG
```

Implemented in senderd source:

```text
INTERNET_OFFER = 10
INTERNET_RESULT = 11
INTERNET_STATUS = 12

direct Wi-Fi listener:
0.0.0.0:5284

phone hotspot SSID/PSK join
AAOS wlan0 IP/port result
RFCOMM fallback remains open
```

Still pending outside this source archive:
- phone companion starts/inspects hotspot
- sends INTERNET_OFFER
- parses INTERNET_RESULT
- reconnects over direct Wi-Fi
- repeats ARDBR001 + HELLO/PING/PONG
- RFCOMM is closed only after the direct Wi-Fi gate passes

No ArdaPlatform protocol change is required if it remains a transparent RFCOMM byte pump.

Do not mix this phase with the safe-area work.

---

# 10. VIDEO FOCUS

Source is included.

Markers:

```text
VIDEO_FOCUS_OUTPUT_SUSPEND
VIDEO_FOCUS_OUTPUT_RESUME
```

Intended behavior:

```text
NATIVE:
capture/output suspend
AAP/TLS session stays alive
video channel stays alive
host-liveness timeout suspended

PROJECTED:
same session/channel
capture restart
MEDIA_START
SPS/PPS
new IDR
resume
```

DHU side is good.
**Real-HU validation is still pending.**

---

# 11. AAP CURRENT COMBINED CHECKPOINT

Mac aaos-aap commit created:

```text
af596294b8d26fd089e62d44c86df96401c2bacd
arda-aap: persist GPS, full multitouch and video focus
```

The automatic Mac push stopped at a credentials prompt.

If no later push result is available in the new conversation:

```text
MAC_AAOS_AAP_PUSH=PENDING
```

Do not create another commit.
If needed, use only:

```bash
git push origin main
```

Ubuntu commits already pushed:

```text
ardaplatform:
53e61b4
arda: update AAP prebuilt with GPS, full multitouch and video focus

vendor/arda:
d46fc3d
arda: add snapshot-reconciled full multitouch input
```

Unrelated:

```text
vendor/arda/ardaplatform/ArdaPlatform.apk
```

Do not touch it.

---

# 12. DISPLAY GEOMETRY

Mercedes 720p:

```text
physical AAP stream:
1280x720

margins:
left   108
right  108
top    0
bottom 0

content_insets:
left   0
top    8
right  0
bottom 14
```

Logical Android display:

```text
1064x720
```

Architecture:

```text
margins
 = physical H264/display black bars

content_insets
 = Android safe-area / WindowInsets
```

Do not apply the left/right 108 again as WindowInsets.

Touch coordinates remain:

```text
1064x720
```

---

# 13. ARDA PROJECTION INSETS — VENDOR POC CLOSED/PASS

Vendor-owned helper:

```text
vendor/arda/projection_insets/
ArdaProjectionInsets.apk
```

Installation:

```text
/system_ext/priv-app/ArdaProjectionInsets/
```

Properties:

```text
platform signed
privileged
platform_apis: true
system_ext_specific: true
```

Permissions:

```text
android.permission.INTERNAL_SYSTEM_WINDOW
android.permission.STATUS_BAR_SERVICE
```

Insets provider:

```text
TYPE_SYSTEM_OVERLAY

LayoutParams.providedInsets

WindowInsets.Type.systemOverlays()

InsetsFrameProvider.SOURCE_DISPLAY
```

V4 final:

```text
REQUIRED_PERMISSION_RUNTIME_GATE=PASS
INSETS_WINDOW_RUNTIME_GATE=PASS
POC_40_60_APPLY_LOG_GATE=PASS
ARDA_PROJECTION_INSETS_VENDOR_POC_V4=PASS
```

Real 8/14 WMS proof:

```text
logical display=1064x720

systemOverlays TOP:
frame=[0,0][1064,8]

systemOverlays BOTTOM:
frame=[0,706][1064,720]
```

Left/right providers are zero.

Framework patch required:

```text
NO
```

---

# 14. 120/120 VISUAL SAFE-AREA TEST — PASS

Test:

```bash
./ARDA_PROJECTION_INSETS_SET_20260807.sh 0 120 0 120
```

WMS:

```text
TOP:
[0,0]-[1064,120]

BOTTOM:
[0,600]-[1064,720]
```

Important framework fact from the local Android 16 source:

```java
WindowInsets.Type.systemBars()
=
STATUS_BARS
| NAVIGATION_BARS
| CAPTION_BAR
| SYSTEM_OVERLAYS
```

Therefore `systemOverlays` is intentionally part of systemBars compatibility.

CarSystemUI source audit found direct `systemOverlays()` use in:

```text
TaskPanel.java
```

120/120 runtime result:

```text
old Maps:
Rect(68,40 - 686,660)

new computed Maps:
Rect(68,120 - 686,600)
```

Maps was then made visible and DHU visually showed the projection task constrained to the safe vertical region.

Conclusion:

```text
SYSTEM_OVERLAYS_PROVIDER=PASS
TASKPANEL_CONSUMPTION=PASS
DHU_VISUAL_SAFE_AREA=PASS
```

Do NOT change the provider to statusBars/navigationBars.

---

# 15. CURRENT REMAINING VISUAL PROBLEM

When the 120/120 safe area is applied:

- Maps / TaskPanel obeys the top/bottom safe area.
- Left/right CarSystemBar rail content does **not** obey it.

Existing bar providers/windows remain full-height:

```text
LeftCarSystemBar:
statusBars frame=[0,0][62,720]

RightCarSystemBar:
navigationBars frame=[1002,0][1064,720]
```

Desired:

```text
bar window/background:
0..720 remains full-height

bar icon/content:
safe vertical region only
```

For 120/120:

```text
icon content usable Y:
120..600
```

For the real Mercedes config:

```text
8..706
```

Do not resize the actual bar/provider windows.

---

# 16. CARSYSTEMUI FIRST SAFE-AREA PATCH ATTEMPT — DID NOT APPLY

Attempted target:

```text
packages/apps/Car/SystemUI/src/com/android/systemui/car/systembar/CarSystemBarView.java
```

Current repo:

```text
CARSYSTEMUI_GIT_HEAD=
88aba80b385617ded0dc245f5edbdb2ee8b226cb
```

The patch assumed upstream-style anchors:

```text
public void onFinishInflate()

mNavButtons = findViewById(R.id.nav_buttons);

mLockScreenButtons = findViewById(R.id.lock_screen_nav_buttons);

mOcclusionButtons = findViewById(R.id.occlusion_buttons);
```

The current local source does not contain those exact anchors.

Result:

```text
PATCH_ANCHOR_GATE=FAIL

MISSING_ANCHOR=public void onFinishInflate()
MISSING_ANCHOR=mNavButtons = findViewById(R.id.nav_buttons);
MISSING_ANCHOR=mLockScreenButtons = findViewById(R.id.lock_screen_nav_buttons);
MISSING_ANCHOR=mOcclusionButtons = findViewById(R.id.occlusion_buttons);
```

No safe-area source patch was applied.

A backup was created before the anchor gate:

```text
/home/edo/arda-source-backups/20260807_125435-carsystembar-safe-area-poc/CarSystemBarView.java
```

---

# 17. CURRENT CARSYSTEMUI DIRTY STATE — PRESERVE

Before the safe-area patch attempt:

```text
 M res/layout/car_right_system_bar_arda_status.xml
 M res/values/strings.xml

?? ARDA_LEFT_RIGHT_RAIL_README.md
?? ARDA_SYSTEMUI_V19_8_RUNTIME_STATE_README.md
?? ARDA_SYSTEMUI_V19_9_PROVIDER_BYPASS_README.md
?? res/drawable/arda_ic_widgets.xml
?? src/com/android/systemui/car/systembar/ArdaWidgetSystemBarButton.java
```

These are existing ARDA rail/widget changes.

Rules:

```text
DO NOT git reset
DO NOT git checkout these files
DO NOT delete them
DO NOT stage/commit them unless user explicitly requests
```

The safe-area patch must coexist with them.

---

# 18. NEXT ACTIVE CODING TASK

## First: inspect the REAL local CarSystemUI implementation

Do not reuse upstream anchors.

Read-only source inspection should include at minimum:

```bash
cd /home/edo/aosp-rpi-aaos16/packages/apps/Car/SystemUI

sed -n '1,340p' \
  src/com/android/systemui/car/systembar/CarSystemBarView.java

grep -RInE \
  "class CarSystemBarView|inflate|findViewById|nav_buttons|lock_screen|occlusion|setButtons|setPadding|WindowInsets|systemOverlays|onApplyWindowInsets" \
  src/com/android/systemui/car/systembar \
  res/layout \
  | head -400
```

Also inspect the currently modified ARDA right rail:

```bash
sed -n '1,320p' res/layout/car_right_system_bar_arda_status.xml
```

Find the actual left rail counterpart(s).

## Then patch the real content owner

Target behavior:

```text
WindowInsets.Type.systemOverlays()
   ↓
top / bottom only
   ↓
left/right vertical rail ICON/CONTENT container
```

Requirements:

- preserve original padding
- repeated inset dispatch must not accumulate padding
- inset listener must not consume the Insets
- left/right background/full bar window stays 720 high
- statusBars/navigationBars providers remain untouched
- top/bottom horizontal bars remain unaffected
- preserve current ARDA widget button layout
- dynamic 120/120 → 8/14 updates should work without reboot if framework dispatches insets
- add clear runtime marker:
  `ArdaCarSystemBarInsets`

Suggested logs:

```text
APPLY side=LEFT top=120 bottom=120
APPLY side=RIGHT top=120 bottom=120
```

Do not assume `CarSystemBarView` is the correct content owner until the actual local source/layout confirms it.

---

# 19. PROJECTION INSETS PRODUCTION WIRING — AFTER VISUAL BAR FIX

The PoC transport currently uses four Settings.Global keys:

```text
arda_projection_inset_left
arda_projection_inset_top
arda_projection_inset_right
arda_projection_inset_bottom
```

This causes transient intermediate states during sequential writes.

Production target:

```text
arda_projection_content_insets=0,8,0,14
```

as one atomic string/value.

Planned data flow:

```text
AAP selected SDR content_insets
        ↓
arda-connectivityd
        ↓
one atomic Settings.Global value
        ↓
ArdaProjectionInsets
        ↓
WindowInsets.systemOverlays()
```

AAP session close/disconnect:

```text
0,0,0,0
```

reset.

`stable_content_insets` remains log-only until a real HU proves distinct semantics.

---

# 20. PRODUCT INTEGRATION — PENDING

`ArdaProjectionInsets` is currently PoC/live deployed.

Still required after UI behavior is closed:

```text
PRODUCT_PACKAGES += ArdaProjectionInsets
```

through the existing ARDA vendor integration path where possible.

Do not edit unrelated upstream product sources if an existing vendor product makefile can own this.

Then:
- build coherent image/module
- boot verify
- remove dependency on manual APK push

---

# 21. CURRENT PROJECT STATUS SNAPSHOT

```text
AAP transport/TLS/media baseline               CLOSED
native video path                              CLOSED baseline
full multitouch                                PASS
GPS/GNSS AIDL standard path                    CLOSED
Vehicle Router Phase 1.1                       CLOSED
Vehicle Manager Phase 2.0/2.1/2.2             CLOSED
Phase 3.0C exact sensor mapping policy         ESTABLISHED
RN-3B senderd Wi-Fi handoff source             IMPLEMENTED
RN-3B phone companion direct Wi-Fi handoff     PENDING
video focus DHU                                IMPLEMENTED
video focus real HU                            PENDING

AAP margins left/right                         PASS
content_insets parser                          PASS
vendor ArdaProjectionInsets                    PASS
WMS systemOverlays top/bottom                  PASS
TaskPanel/Maps safe-area consumption           PASS
DHU 120/120 visual safe-area                   PASS
left/right CarSystemBar content safe-area      ACTIVE TODO
ProjectionInsets atomic SDR wiring             TODO
ProjectionInsets product integration           TODO
```

---

# 22. CONTINUATION PROMPT FOR A NEW CHAT

Use this text:

> Continue the ARDA AAOS/AAP project from this handover. `ARDA_PLAN.md` is a chronological log; the v24 "current milestone" at the top is obsolete and has been superseded. Vehicle Phase 1.1, Manager Phase 2.0/2.1/2.2, and GNSS Phase 3.0B3 are closed. The current active work is the AAP `content_insets` safe-area. The vendor-owned `ArdaProjectionInsets` successfully creates Android 16 `WindowInsets.Type.systemOverlays()` top/bottom providers. In the 120/120 test, Maps/TaskPanel becomes `Rect(...,120 - ...,600)` and this was visually verified in DHU. The remaining issue is that the left/right CarSystemBar icon/content columns do not follow the safe area. The bar/provider windows must remain full-height; only the rail content should receive top/bottom systemOverlays padding. The first CarSystemBarView patch failed at the anchor gate because the local source differs from upstream, and no patch was applied. First inspect the real local CarSystemBarView and ARDA rail layout sources. Preserve existing dirty SystemUI files. Do not run repo sync. Do not commit/push. Then prepare a minimal safe-area consumer patch for the actual owner.
