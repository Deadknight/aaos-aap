<# ARDA — Canonical Handover

> **Current pointer: 2026-09-08**
>
> This file is the canonical **handover/history/continuation document**. `README.md` is the cleaner project/architecture/runbook reference.
>
> Older handovers and plan documents are preserved verbatim at the end. When they disagree with the canonical current-state section below, the newer section wins.

---

## 0. How to use this handover

Priority order:

1. **Sections 1–15 of this file** — canonical current state.
2. `README.md` — architecture and operational reference.
3. Preserved source appendices — historical detail and forensic context.

Never interpret an old “current milestone” sentence in a preserved appendix as newer than this 2026-09-08 pointer.

---

## 1. Current checkpoint summary — 2026-09-08

The current runtime-validated state combines the August private-repo baseline with later capture/SystemUI/dock work.

### Core platform

```text
Target: Raspberry Pi 4
OS: Android Automotive OS 16 userdebug
Display: Waveshare 7.9 profile
Current active WMS geometry observed: 1064x720
Preferred Ethernet ADB: 192.168.1.184:5555
Build host: edo@192.168.1.55
AOSP: /home/edo/aosp-rpi-aaos16
Lunch: aosp_rpi4_car-bp4a-userdebug
```

### Rules

```text
NEVER repo sync
Ethernet ADB for system work
USB reserved for AAP/DHU
never adb root while DHU is live
no CPU-copy production capture
runtime approval before commit/push
```

### Current working UI state

- Left/right system bars now respect Android Auto projection safe area at the **real WMS window** level.
- Stable implementation is **V6**, driven by `Settings.Global arda_projection_content_insets`.
- Current observed projection content inset value: `0,8,0,14`.
- Correct vertical bar frames observed:
  - left `[0,8][62,706]`
  - right `[1002,8][1064,706]`
- Previous V5 `WindowInsets` feedback loop has been removed.
- Recent-map classifier crash is fixed.
- Persistent recent map / recent app source marker is `ARDA_DOCK_PERSISTENT_RECENTS_V2`.
- Dock spacing is applied at the real inner RecyclerView with marker `ARDA_DOCK_REAL_RECYCLERVIEW_SPACING_V1_3`.
- Additional inter-item spacing: 6dp; visually confirmed including the Maps/recent-app region.

### Current Git state

Latest safe-area/dock work is still **uncommitted** at the moment this handover is assembled.

Known dirty files expected by the prepared checkpoint script:

```text
Launcher:
  app/src/com/android/car/carlauncher/CarLauncherViewModel.java
  docklib/src/com/android/car/docklib/DockViewModel.kt
  docklib/src/com/android/car/docklib/view/DockView.kt

SystemUI:
  src/com/android/systemui/car/systembar/ArdaSafeAreaCarSystemBarView.java
  src/com/android/systemui/car/systembar/SystemBarConfigsImpl.java
```

Existing correct remotes:

```text
Launcher:
  remote github
  https://github.com/Deadknight/arda-aaos-car-launcher.git
  branch arda-poc-checkpoint

SystemUI:
  remote arda
  https://github.com/Deadknight/android_packages_apps_Car_SystemUI.git
  branch arda-aaos16
```

Ubuntu `gh` is authenticated as `Deadknight` over HTTPS.

Prepared commit/push script:

```text
ARDA_CHECKPOINT_COMMIT_PUSH_V2_20260908.sh
```

Do not mark the new checkpoint as remote-contained until that script finishes and remote SHA equality passes.

---

## 2. Last fully remote-contained baseline — 2026-08-13

The archive's all-six canonical checkpoint:

```text
Launcher
  fe205c5870d01b395e4e3df2614e7c6d2b41c114

MediaPrebuilt
  1342ba973e3c8e70342144c736b3a5eb94a6ddd7

SystemUI
  c35ec54d6457c6801043838028f8ad66d422bf59

device/brcm/rpi4
  a0b6ae212be2f9815213519860ccb78d56b13734

vendor/arda
  e79d85effc39207a946825710383f2732f4d1c35

vendor/arda/ardaplatform
  63df5257e5667373470f97a0867607d8817cc9ba
```

The Launcher and SystemUI branches have newer uncommitted work now. The baseline remains a valuable recovery anchor.

---

## 3. Closed / established historical platform areas

These were considered closed or sufficiently established in the supplied handovers:

### Input

- app/framework injection lineage.
- full multitouch snapshot reconciliation.
- pointer lifecycle repair.
- physical-key behavior.
- secured platform input bridge ownership.

### Vehicle / sensors

- explicit sensor gating.
- normalized event boundary.
- Wasmtime translation policy.
- direct VehicleProperty model correction.
- manager/router ownership split.
- enforcing/platformized router boundary.

### GNSS

- GPS provider bridge.
- GNSS status plane.
- vendor GNSS AIDL HAL migration.
- standard GNSS AIDL location path.

### Network

- USB accessory transport baseline.
- phone internet bridge work.
- RN-3B Wi-Fi handoff MVP architecture.

Details and exact markers remain in the preserved plan/handover appendices.

---

## 4. H.264 / SurfaceFlinger capture state

### 4.1 August 1080p30 direction

The archive established a hardware-aware goal:

```text
1920x1080
~30 actual encoded fps
c2.v4l2.avc.encoder
no software fallback
minimum CPU conversion
AAP PTS = actual cadence
```

At that point, production capture used platform `screenrecord`, and ARDA did not pass a frame-rate flag. That motivated auditing actual MediaCodec cadence rather than merely changing PTS.

### 4.2 Later direct/HWC work

Subsequent work established two capture profiles.

#### Stable

- based on exact V23 direct/HWC behavior.
- product default.
- preferred reliability path.

#### Performance B2.1

- branch lineage retained.
- ~25–26 fps seen in prior reference tests.
- rare artifact possibility.
- optional profile, not abandoned.

Property design:

```text
persist.sys.arda.capture_profile=stable|performance
sys.arda.capture_profile.active=<profile>
```

### 4.3 Direct path rule

Do not repeat the 1280x720 sink / 1920x1080 physical clone mistake.

Correct rule:

```text
exact source == sink:
  direct PCT / DMABUF-friendly path

mismatch:
  normal VDS HWC/GPU composition/scaling
  codec-owned output buffers
  no CPU copy
```

### 4.4 Future capture optimization

If performance work resumes:

```text
capture-owned triple-buffer DMABUF pool
```

is the intended next architectural direction.

---

## 5. AAP startup and display-profile sequencing

This deserves explicit preservation because it was initially mistaken for generic USB instability.

### 5.1 Race 1 — display profile reconcile

A failing startup can look like:

```text
AAP chooses 1920x1080
WM size/density/reset applied
DISPLAY_PROFILE_RECONCILE_RESTART requested=true
system_server/zygote restart occurs
autostart AAP session exits
```

This is expected startup sequencing around geometry reconciliation, not necessarily a transport defect.

### 5.2 Race 2 — first screenrecord child EOF

The next AAP session can reach:

```text
MEDIA_MESSAGE_START
spawn screenrecord
first native-capture child EOF
wait first-frame timeout
restart screenrecord once
second child succeeds
```

Observed later success timeline included codec config, first video, and ACK immediately after the restarted child began producing output.

Keep the retry behavior and do not collapse both races into one “USB flaky” diagnosis.

---

## 6. Dynamic resolution architecture

Current intended behavior:

```text
maximum HU target: 1080p
arbitrary <=1080p profiles supported
```

When selected HU resolution equals capture source:

- direct path may be used.

When they differ:

- fall back to VDS HWC/GPU scaling;
- do not attempt direct incompatible-buffer clone;
- do not add CPU conversion.

A previous direct mismatch produced V4L2/Codec2 locking failures. That failure is architectural evidence and should remain documented.

---

## 7. Recent-map SystemUI crash forensic

The first recent-map classifier implementation caused repeated SystemUI crashes after task transitions.

Exact failure class:

```text
java.lang.IllegalArgumentException:
Can't set package name when selector is already set
```

Bad code:

```kotlin
Intent.makeMainSelectorActivity(
    Intent.ACTION_MAIN,
    Intent.CATEGORY_APP_MAPS,
).setPackage(component.packageName)
```

Fixed code:

```kotlin
Intent(Intent.ACTION_MAIN)
    .addCategory(Intent.CATEGORY_APP_MAPS)
    .setPackage(component.packageName)
```

Impact of the crash loop:

- transient disappearance of left/right rails;
- apparent “freeze” after returning from apps;
- repeated SystemUI deaths;
- no corresponding codec/capture failure evidence.

Important diagnosis rule:

> If rails disappear after app transitions, check SystemUI crash state before blaming SurfaceFlinger/AAP.

---

## 8. Safe-area investigation chronology

This is the most important September UI investigation.

### 8.1 Requirement clarified

The requirement was not padding.

Desired result:

```text
actual bar window background ends at safe-area boundary
unsafe top/bottom or left/right segments expose global background
```

### 8.2 Child-level patch failed

`ArdaSafeAreaCarSystemBarView` was an inner/default layout in some bar configurations. Resizing it did not resize the real WMS bar window.

### 8.3 `getLayoutParamsByName()` failed

Runtime marker count was zero. The method was not on the active attach path.

### 8.4 `InternalSystemBarWindow.getLayoutParams()` failed

Source patch existed, APK contained it, but runtime marker remained zero in the relevant active path.

### 8.5 Final addView choke point found

Source audit identified:

```text
CarSystemBarControllerImpl
 -> getWindowForName()
 -> SystemUiWindow.setRootView(...)
 -> WindowManager.addView(...)
```

A one-shot late SystemUI restart proved the actual safe geometry was correct when projection overlays already existed.

### 8.6 Startup ordering root cause proven

With projection overlays live, restarting user-0 SystemUI produced:

```text
systemOverlays = 0,8,0,14
Left  = [0,8][62,706]
Right = [1002,8][1064,706]
```

Therefore the bar geometry math was correct; the issue was **when** the data became available.

### 8.7 V5 feedback loop

V5 used `OnApplyWindowInsetsListener`.

It successfully reacted late, but then looped:

```text
0,8,0,14 -> shrink to 698
0,0,0,0  -> expand to 720
repeat
```

Why:

- after shrinking, the local bar window no longer intersects overlay regions;
- its own local `WindowInsets` become zero;
- the code interprets zero as “safe area disappeared”;
- it expands;
- overlays intersect again.

The live proof recorded very high repeated update counts, matching visible continuous grow/shrink animation.

### 8.8 V6 final stable fix

Use the window-independent projection-owned atomic setting:

```text
arda_projection_content_insets
```

Observed:

```text
0,8,0,14
```

SystemUI:

- stores a base/full LP;
- registers a `ContentObserver`;
- reads global projection content insets;
- calculates desired safe LP;
- calls `updateViewLayout()` only when geometry differs.

The local WindowInsets feedback source is no longer used as the geometry authority.

Result: stable bars, no oscillation.

---

## 9. Dock spacing investigation chronology

### 9.1 User-visible problem

After the safe-area fix, dock icons were visually too close together, especially around map/recent-app.

### 9.2 Controller ItemDecoration attempt failed to compile

The first patch called:

```kotlin
dockView.addItemDecoration(...)
```

but `dockView` is a custom `DockView`, not a RecyclerView.

Build failed safely and source rollback passed.

### 9.3 Parent XML divider built but had no visible effect

A transparent `showDividers="middle"` patch compiled and deployed, but `DockView` is a:

```text
FrameLayout
```

with an internal RecyclerView, so the parent divider did not control actual item spacing.

### 9.4 Item XML margin was not authoritative

`dock_app_item_view.xml` had:

```text
top margin    7dp
bottom margin 7dp
```

A temporary change to 10/10 built/deployed, but adapter LayoutParams handling meant it was not the clean runtime authority.

### 9.5 Final V1.3 — real RecyclerView ItemDecoration

Source audit proved:

```text
DockView : FrameLayout
  -> private RecyclerView
  -> LinearLayoutManager
```

Final spacing patch is inside `DockView.kt` on that actual RecyclerView.

Marker:

```text
ARDA_DOCK_REAL_RECYCLERVIEW_SPACING_V1_3
```

Behavior:

```text
additional gap: 6dp
first/last outer gap: unchanged
vertical/horizontal aware
static/dynamic uniform
recent map/recent app included
XML margins restored to stock 7dp/7dp
```

Visual result was approved.

---

## 10. ArdaPlatform and settings

Current UI controls relevant to this work:

```text
arda_system_bar_respect_safe_area
arda_dock_show_recent_map
arda_dock_show_recent_app
arda_system_bar_layout_mode
```

Projection-owned global geometry:

```text
arda_projection_content_insets
```

Raw/legacy stable inset values may also exist, but SystemUI bar geometry must use the V6 atomic content setting.

ArdaPlatform deployment remains privileged/system through AOSP prebuilt integration, not ordinary user-app installation.

---

## 11. Current source locations

```text
SystemUI safe geometry:
packages/apps/Car/SystemUI/src/com/android/systemui/car/systembar/SystemBarConfigsImpl.java

SystemUI inner safe view:
packages/apps/Car/SystemUI/src/com/android/systemui/car/systembar/ArdaSafeAreaCarSystemBarView.java

Recent map/recent app:
packages/apps/Car/Launcher/docklib/src/com/android/car/docklib/DockViewModel.kt

Real dock RecyclerView spacing:
packages/apps/Car/Launcher/docklib/src/com/android/car/docklib/view/DockView.kt

Launcher behavior:
packages/apps/Car/Launcher/app/src/com/android/car/carlauncher/CarLauncherViewModel.java
```

---

## 12. Current recovery/checkpoint status

### Remote-contained recovery point

Use the 2026-08-13 six-repo SHAs if the latest uncommitted work must be discarded.

### Latest local validated state

Contains:

```text
V6 safe bars
persistent recents V2
dock RecyclerView spacing V1.3
recent-map classifier hotfix lineage
```

At handover creation time these latest changes still need the V2 checkpoint commit/push.

The remote-audit proved the correct remotes exist and `origin` is not required.

---

## 13. Next exact work order

1. Run:
   ```text
   ARDA_CHECKPOINT_COMMIT_PUSH_V2_20260908.sh
   ```
2. Record:
   - new Launcher commit SHA;
   - new SystemUI commit SHA.
3. Verify remote SHA equality.
4. Update this handover's checkpoint section with those SHAs.
5. After checkpointing, continue only with deliberately selected next work:
   - remaining recent-slot edge cases if any;
   - horizontal-mode visual parity if needed;
   - real-HU boot-settle validation;
   - capture performance/triple-buffer work if prioritized.

Do not mix those tasks into the checkpoint commit.

---

## 14. Things that must not be redone

- Do not reintroduce the bad `makeMainSelectorActivity(...).setPackage(...)` map classifier.
- Do not implement safe bars as child padding only.
- Do not use local bar `WindowInsets.systemOverlays` as the permanent geometry source.
- Do not use raw `120/120` projection values directly as SystemUI WMS pixel offsets.
- Do not direct-clone a mismatched physical frame into a smaller codec sink.
- Do not `adb root` with DHU active.
- Do not `repo sync`.
- Do not replace privileged ArdaPlatform deployment with a normal `adb install`.
- Do not assume build success equals runtime closure.

---

## 15. Continuation prompt

> Continue ARDA from the canonical `README.md` and `HANDOVER.md` dated 2026-09-08. Never run repo sync. Target is RPi4 AAOS16, build host `edo@192.168.1.55`, AOSP `/home/edo/aosp-rpi-aaos16`, preferred Ethernet ADB `192.168.1.184:5555`. Current validated local state includes V6 system-bar safe-area geometry driven by global `arda_projection_content_insets`, persistent recent map/recent app V2, and V1.3 dock spacing on the real inner RecyclerView. The safe-area V5 local-WindowInsets listener was rejected because it caused a 0,8,0,14 <-> 0,0,0,0 relayout feedback loop. Correct vertical frames are approximately Left `[0,8][62,706]`, Right `[1002,8][1064,706]`. Launcher remote is `github=https://github.com/Deadknight/arda-aaos-car-launcher.git` on `arda-poc-checkpoint`; SystemUI remote is `arda=https://github.com/Deadknight/android_packages_apps_Car_SystemUI.git` on `arda-aaos16`. The latest validated UI work is still uncommitted unless `ARDA_CHECKPOINT_COMMIT_PUSH_V2_20260908.sh` has since been run; first verify Git state and record the new remote-contained SHAs. Keep stable capture profile as default, B2.1 performance optional, and use direct capture only for exact source==sink; mismatches must use VDS/HWC/GPU scaling with no CPU copy.

---

# Appendix A — Original handovers, plan, status, and changelogs preserved verbatim

The sections below are historical source material. Their implementation details remain authoritative for the period they describe; their old “current” pointers are superseded by the canonical sections above.


---

## Source Appendix 1: `ARDA_HANDOVER_20260813_H264_1080P30.md`


# ARDA AAOS / Android Auto AAP — Current Handover
**Date:** 2026-08-13  
**Active target:** Raspberry Pi 4 H.264 1920x1080 @ ~30 real fps  
**Chronological source plan:** `ARDA_PLAN.md`

---

# 0. USE THIS AS THE CURRENT POINTER

Older handovers and the early `ARDA_PLAN.md` line saying `Current milestone: TCP + TLS + auth + service discovery + ping response works` are historical. Do not return to those milestones.

The current project is far beyond initial AAP bring-up. The active coding task is **1080p30 H.264 performance**. The first-AAP-session boot-settle gate is separately pending real-HU cold-boot validation and is not the current task.

---

# 1. WORKING RULES

- **Never run `repo sync`.**
- Work only in the existing local source trees with targeted edits.
- Do not commit/push new H.264 work unless the user explicitly requests it after runtime validation.
- Preserve unrelated source state.
- Do not generate unnecessary SHA-256 output.
- Prefer vendor/project-owned implementation boundaries over generic framework patches when practical.
- ADB transport order for deployment: USB `1000000028c0b59c` -> `192.168.1.185:5555` -> `192.168.1.15:5555`; pin the selected transport for the whole transaction.
- Assume SELinux Enforcing.

---

# 2. ENVIRONMENT

## Mac

```text
edo@Edos-MacBook-Pro-3
/Users/edo/Documents/Code/aaos-aap
```

The Mac repo is user-managed separately. The Ubuntu finalization described below does **not** assert that the Mac repo is pushed/clean.

## Ubuntu/AOSP

```text
host: edo@192.168.1.55
AOSP root: /home/edo/aosp-rpi-aaos16
lunch: aosp_rpi4_car-bp4a-userdebug
Android/AAOS: 16
Build ID: BP4A.251205.006
```

---

# 3. CANONICAL PRIVATE GIT CHECKPOINT — 2026-08-13

All six ARDA GitHub repositories below were verified **PRIVATE** and remote-contained.

```text
packages/apps/Car/Launcher
  branch: arda-poc-checkpoint
  remote: github
  repo: Deadknight/arda-aaos-car-launcher
  SHA: fe205c5870d01b395e4e3df2614e7c6d2b41c114

packages/apps/Car/MediaPrebuilt
  branch: arda-poc-checkpoint
  remote: github
  repo: Deadknight/android_packages_apps_Car_MediaPrebuilt
  SHA: 1342ba973e3c8e70342144c736b3a5eb94a6ddd7

packages/apps/Car/SystemUI
  branch: arda-aaos16
  remote: arda
  repo: Deadknight/android_packages_apps_Car_SystemUI
  SHA: c35ec54d6457c6801043838028f8ad66d422bf59

device/brcm/rpi4
  branch: arda-aaos16
  remote: arda
  repo: Deadknight/android_device_brcm_rpi4
  SHA: a0b6ae212be2f9815213519860ccb78d56b13734

vendor/arda
  branch: arda-aaos16
  remote: origin
  repo: Deadknight/android_vendor_arda
  SHA: e79d85effc39207a946825710383f2732f4d1c35

vendor/arda/ardaplatform
  branch: main
  remote: origin
  repo: Deadknight/android_vendor_arda_ardaplatform
  SHA: 63df5257e5667373470f97a0867607d8817cc9ba
```

Final gates from `ARDA_FINALIZE_UNSHALLOW_PUSH_V6`:

```text
ALL_SIX_PINNED_HEADS_PRESERVED=PASS
ALL_SELECTED_GITHUB_REPOS_PRIVATE=PASS
ALL_SIX_ARDA_TARGET_REPOS_REMOTE_CONTAINED=PASS
UBUNTU_DIRTY_REPOS_AFTER=0
GLOBAL_AOSP_WORKTREES_CLEAN=PASS
AOSP_REMOTE_PUSHED=NO
REPO_SYNC_CALLED=NO
ARDA_FINALIZE_UNSHALLOW_PUSH_V6=PASS
```

Important Git detail: Launcher and MediaPrebuilt were shallow AOSP checkouts. Both were targeted with `git fetch --unshallow --no-tags aosp` before their first push to empty private GitHub repos. HEAD/source/worktree remained unchanged. This explains the earlier `remote unpack failed / did not receive expected object` failure; it was not source corruption.

---

# 4. CLOSED / ESTABLISHED PLATFORM BASELINE

Do not reopen these areas unless a new regression proves otherwise:

```text
AAP TCP/TLS/auth/service discovery/media baseline     CLOSED
AAP H.264 transport + ACK flow                        CLOSED baseline
full multitouch snapshot reconciler                   PASS
Maps pinch zoom                                       PASS
GPS/GNSS standard platform path                       CLOSED
Vehicle Router Phase 1.1                              CLOSED
Vehicle Manager Phase 2.0 / 2.1 / 2.2                CLOSED
projection content-insets/systemOverlays path          PASS
cross-profile display-profile reconcile               PASS
physical AAP keys / semantic NAV/TEL                  PASS
CarSystemUI vertical QC placement/recreation          runtime fixed
horizontal notification + widget controls             runtime PASS
```

Current CarSystemUI and related platform work are captured by the private Git checkpoints above.

---

# 5. DEFERRED REAL-HU BOOT-SETTLE VALIDATION

Current `vendor/arda/ardaplatform` includes the first-session AAP boot-settle gate in `arda-aap-autostart.sh`.

Design:

```text
first AAP session per boot waits for:
  sys.boot_completed=1
  Activity service
  Window service
  com.android.systemui
  com.android.car.carlauncher
  stable for 5 seconds

max wait: 60 s fail-open
boot-scoped marker: /dev/.arda-aap-boot-settled-v1
same-boot reconnect: no settle delay
arda-connectivityd supervisor itself: not delayed
```

Real-HU cold-boot validation is still **PENDING** because access to the vehicle takes time. Do not mark this closed without the real-HU test.

---

# 6. ACTIVE H.264 PERFORMANCE GOAL

Target:

```text
Raspberry Pi 4
1920x1080
~30 real encoded fps
hardware encoder: c2.v4l2.avc.encoder
software fallback: NO
```

The goal is not to chase 1080p60. If the selected SDR advertises `VIDEO_FPS_60`, treat 30 fps as the intended Pi-4 hardware-aware effective workload for 1080p and validate that the HU accepts the lower actual cadence.

Do not state that the protocol formally defines 60 as a maximum unless runtime/spec evidence proves it. Operationally, ARDA will attempt lower actual frame cadence while retaining the selected video configuration unless the HU rejects it.

---

# 7. CURRENT VIDEO SOURCE FACTS

In current `aaos-aap`:

```text
src/aap/proto.rs
  VIDEO_FPS_60 -> 60
  VIDEO_FPS_30 -> 30

src/aap/mod.rs
  configure_from_sdr():
    self.config.width  = selection.width
    self.config.height = selection.height
    self.config.fps    = selection.fps

production capture backend:
  /system/bin/screenrecord

screenrecord arguments from ARDA:
  --output-format=h264
  --size WIDTHxHEIGHT
  --bit-rate BITRATE
  --time-limit 0
  --codec-name c2.v4l2.avc.encoder
  -

AAP video PTS:
  frame_index * 1_000_000 / self.config.fps
```

Crucially, ARDA currently does **not** pass a frame-rate argument to platform `screenrecord`.

Therefore changing only `self.config.fps` to 30 would fix AAP PTS cadence but would not prove the actual MediaCodec/SurfaceFlinger workload was reduced from 60 to 30. The actual Android 16 `screenrecord` source must be audited before patching.

---

# 8. PERFORMANCE ARCHITECTURE TO PROVE

Current likely production path:

```text
SurfaceFlinger / virtual display
  -> encoder input Surface
  -> Codec2 c2.v4l2.avc.encoder
  -> V4L2 H.264 hardware
  -> Annex-B
  -> AAP media framing
```

The two questions are separate:

1. **Cadence:** Is the encoder actually being fed/configured at 60 when SDR says 60? We want a real 1080p30 workload.
2. **Pixel conversion:** Does the Pi path still do a costly CPU RGBA -> NV12 conversion before V4L2 encode? Only optimize this after the real 30-fps workload is established and measured.

The first optimization priority is cadence, not speculative GPU conversion work.

---

# 9. NEXT EXACT WORK ORDER

1. Read-only inspect the build-host Android 16 `screenrecord` source and any wrapper/config used by this image.
2. Locate exact `MediaFormat` keys used for H.264 encode: `KEY_FRAME_RATE`, bitrate mode, profile/level, color format, operating rate, I-frame cadence.
3. Locate display/virtual-display creation and whether screenrecord throttles frames or simply follows display refresh.
4. Inspect the active V4L2 Codec2 encoder path for supported 1080p limits and current format-conversion behavior.
5. Create a no-source-change runtime profiler for current 1080p behavior if source alone cannot prove actual output cadence.
6. Implement the narrowest 1080p30 effective-fps control so **actual capture/encoder cadence and AAP PTS agree at 30**.
7. Runtime success target:

```text
resolution = 1920x1080
encoded fps >= 29 preferred
frame interval ~33.3 ms
codec = c2.v4l2.avc.encoder
software fallback = NO
thermal throttling = NO
major frame drops/backpressure = NO
```

8. Only if 1080p30 still misses target, profile/optimize RGBA->NV12 / dmabuf/GPU conversion.

Do not commit/push this new H.264 work until runtime behavior is approved.

---

# 10. NEW-CHAT CONTINUATION PROMPT

> Continue ARDA from `ARDA_HANDOVER_20260813_H264_1080P30.md` and the latest section of `ARDA_PLAN.md`. Ubuntu/AOSP is clean and the six ARDA repos are private + remote-contained at the exact SHAs in the handover; never run repo sync. The active task is Pi 4 H.264 performance at 1920x1080 near 30 real fps using `c2.v4l2.avc.encoder`. SDR may advertise 60, but do not intentionally target 1080p60; we want a hardware-aware effective 30-fps workload and correct 30-fps AAP PTS. Current ARDA production capture uses `/system/bin/screenrecord`; `configure_from_sdr()` copies SDR fps directly, but the screenrecord command has no fps argument. First inspect the actual Android 16 build-host screenrecord/MediaCodec source and V4L2 Codec2 path read-only. Determine how to make the actual encoder input cadence 30, not merely change PTS. Keep the real-HU boot-settle gate pending and separate.



---

## Source Appendix 2: `ARDA_HANDOVER_20260807_PLAN_ALIGNED.md`


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



---

## Source Appendix 3: `ARDA_PLAN.md`


# ARDA plan

> **Current pointer (2026-08-13):** The historical `Current milestone` line below is obsolete. The canonical current checkpoint and active work are documented in the `2026-08-13 canonical private-repo checkpoint + 1080p30 H.264 performance` section at the end of this file and in `ARDA_HANDOVER_20260813_H264_1080P30.md`.

Current milestone: TCP + TLS + auth + service discovery + ping response works.

v24c patch:
- Keep v24 channel-open probe behavior.
- Fix ServiceDiscoveryResponse parsing for rust-protobuf optional fields.
- Generated `Service` exposes optional submessages as public `Option<T>` fields,
  not `has_*()` / `*_service()` methods.

Next after test:
- Select video MediaSinkService from ServiceDiscoveryResponse.
- Send ChannelOpenRequest.
- Observe ChannelOpenResponse.

### v24d
- Fixed video `CHANNEL_OPEN_REQUEST` wire shape.
- Previous v24 sent it on channel 0; DHU answered `MESSAGE_UNEXPECTED_MESSAGE`.
- Now it is sent on `channel = service_id` with `CONTROL` bit, producing `0x0f`
  after ENCRYPTED is added.


## v25 media start probe

- Decodes non-control media-channel payloads such as `MEDIA_MESSAGE_CONFIG`.
- After selected video channel opens, sends encrypted `MEDIA_MESSAGE_START` when HU reports `MEDIA_MESSAGE_CONFIG STATUS_READY`.
- Keeps ping response/session behavior unchanged.

### v26 media setup

- Channel open succeeds on service channel 0x02.
- Added explicit `MEDIA_MESSAGE_SETUP` after `CHANNEL_OPEN_RESPONSE opened=true`.
- Improved `MEDIA_MESSAGE_VIDEO_FOCUS_NOTIFICATION` / request logging.
- Next: after CONFIG/START, send first H.264 codec config / IDR data.

## v27

Docs-based AV DATA path:
- Do not advance by guessing media states.
- Verified from docs: video channel uses specific AV messages, not control.
- `0x8000 SETUP`, `0x8003 CONFIG`, `0x8001 START` are already observed working.
- Next DATA sender uses documented `0x0001 CODEC_CONFIG` and `0x0000 DATA` with 8-byte big-endian PTS header.
- ACK-based flow control added for `0x8004 MEDIA_MESSAGE_ACK` and `max_unacked` from CONFIG.

## v28

Wire the working native capture path into the docs-based AAP video path.

- Keep v27 H264 packet format unchanged because it already rendered in DHU.
- Add `--video-native` to spawn `native-capture --out -` only after video START.
- Parse child stdout as Annex-B H264 NALs.
- Send SPS/PPS as `MEDIA_MESSAGE_CODEC_CONFIG`.
- Send slice NALs as `MEDIA_MESSAGE_DATA` with monotonic 30fps PTS.
- Continue respecting `MEDIA_MESSAGE_ACK` / `max_unacked`.

Next after test:
- Convert probe into cleaner long-running session mode.
- Add touch/input channel opening and map uinput injection.

### v29
- Fix native-capture child stdout buffering for live AAP video.
- Log first live stdout reads and first parsed NALs.
- Select SDR video config with docs-backed resolution/fps/density/margin logging; native live capture follows SDR size/fps by default.

### v30 live video pump fix

Current issue: first native H264 IDR frame reaches DHU, but the display freezes. Logs show the video pipeline sends frame 0, receives ACKs, then only sends more frames when inbound HU frames wake the receive loop.

Fix:
- Enable a 20ms receive timeout after MEDIA_MESSAGE_START and pump video on timeout.
- Interpret repeated ACK value 1 as one buffer released, not as cumulative high-watermark.
- Keep service discovery / channel open / setup / start behavior unchanged.

## v31

- Add SDR video config policy selection: highest/balanced/lowest/index:N.
- Parse and log margins/content-insets/stable-insets from VideoConfiguration.
- Add optional WM application: stream/content/safe size modes and density/real_density modes.
- Keep default non-invasive: no WM change unless explicitly requested.

## v39 app_process input injector

- Default `--input-inject` backend is now `--input-inject-mode app`.
- `arda-senderd` embeds a small Android Java input daemon jar at build time.
- Runtime flow is automatic: `arda-senderd` writes `/data/local/tmp/arda-inputd.jar`, starts it with `app_process`, streams touch/key commands to its stdin, and stops it on normal exit / Ctrl-C path.
- The Java child creates `MotionEvent` / `KeyEvent` and calls `InputManager.injectInputEvent()` scrcpy-style, bypassing the unreliable `/dev/uinput` display-routing path.
- Fallbacks remain: `--input-inject-mode uinput` and `--input-inject-mode shell`.
- Build now needs Android SDK platform `android.jar` and build-tools `d8`; Dockerfile.android installs them.


## v39d emergency input safety

App-process InputManager injection stays in tree, but it is no longer default. Default input injection mode is `uinput`; app mode requires explicit `--input-inject-mode app`. This prevents accidental AAOS input/focus disruption during normal video tests.

## v39e note

App-process InputManager injection is no longer treated as the default path. It is kept as explicit `--input-inject-mode app`, starts lazily on the first injected event, and throttles MOVE events with `--input-app-move-min-ms` (default 16ms). This keeps the working drag path available while reducing the chance of AAOS focus/input stack corruption.

## v41j notes
- First-frame render kick modes were expanded after observing that a real touch immediately wakes the first encoder frame.
- New safer modes:
  - `--video-first-frame-kick-cancel`: sends touchscreen DOWN then CANCEL at the kick point, avoiding ACTION_UP/click.
  - `--video-first-frame-kick-center`: taps stream center automatically using configured video width/height.
  - `--video-first-frame-kick-wakeup`: sends KEYCODE_WAKEUP (224), lowest UI risk but may not dirty the projection surface on all builds.
  - `--video-first-frame-kick-mode keyevent --video-first-frame-kick-keycode WAKEUP|SEARCH|HOME|...` accepts simple names.
- Recommended order for testing: `touch-cancel`, then `wakeup`, then edge tap, then center tap only if safe.

## bluetoothaaos park note

- AAP mic receive is solved: MIC_DATA arrives, session_id=0 is accepted, and `--mic-ack-mode delta` keeps HU/DHU flow-control open.
- Bluetooth call routing is parked under the name `bluetoothaaos`.
- Current blocker: AAOS Bluetooth HFP client sees `AG_CALL_CHANGED`, then `com.android.bluetooth` crashes in `HfpClientDeviceBlock.handleCall()` because `TelecomManager.addNewUnknownCall()` throws `SecurityException: This PhoneAccountHandle is not registered for this user!`.
- Follow-up direction for bluetoothaaos: add/enable a proper Telecom `PhoneAccount`/`ConnectionService`/`InCallService` path, or patch Bluetooth `HfpClientConnectionService` so the HFP client account is registered for the active user before unknown-call insertion. HAL restart crashes (`H4Protocol::OnDataReady: Unimplemented packet type`) are likely a secondary restart-chain problem and should be revisited after the Telecom account crash is fixed.

## v42 sensor bridge start

Goal: move from mic to the AAP SensorSource channel and capture all HU-provided sensors up to `SENSOR_GPS_SATELLITE_DATA = 21`, then start forwarding usable data into AAOS.

Implemented in this patch:
- Parse SensorSourceService from ServiceDiscoveryResponse, including the advertised sensor type list.
- Add sensor channel open support with `--sensor-open`.
- Add subscription request support for advertised sensor types `1..21` using generated proto structs:
  - `SENSOR_MESSAGE_REQUEST = 0x8001`
  - `SENSOR_MESSAGE_RESPONSE = 0x8002`
  - `SENSOR_MESSAGE_BATCH = 0x8003`
  - `SENSOR_MESSAGE_ERROR = 0x8004`
- Add `--sensor-request-all` default behavior when sensor channel is open.
- Add automatic request periods unless overridden by `--sensor-period-ms`:
  - location/compass/gps satellite: 1000ms
  - speed: 500ms
  - gear/parking/dead-reckoning/accel/gyro: 500ms
  - fuel/rpm/odometer/environment/hvac/tire: 10000ms
  - night/driving/passenger/door/light: 0ms event-driven
- Decode and log SensorBatch for sensor types 1..21.
- Add optional JSONL capture for all decoded batches: `--sensor-jsonl /data/local/tmp/arda-sensors.jsonl`.
- Add first OS-forward path for location: `--sensor-forward-os` uses `cmd location providers set-test-provider-location` with provider `arda_gps` by default. This is intentionally best-effort; if `cmd location` syntax/permission differs on this AAOS image, logs will show the failure and the next step is an app_process LocationManager injector.

Next after test:
- Verify whether the HU/DHU accepts the batch of sensor requests and which sensors actually stream.
- If `cmd location` fails, replace shell command forwarding with a small app_process/system LocationManager helper.
- After GPS/location works, map remaining vehicle sensor data either into AAOS car property/VHAL where available, or keep it on the arda JSON/socket bridge for companion consumers.

## v43 sensor OS/VHAL bridge

Goal: turn the AAP SensorSource batches into AAOS-visible vehicle state, not just JSONL/log output.

Implemented in this patch:
- Keep `--sensor-forward-os` as the convenient umbrella flag, but make it forward both:
  - location fixes through `cmd location providers set-test-provider-location`
  - supported vehicle values through `cmd car_service inject-vhal-event`
- Add split flags for safer tests:
  - `--sensor-forward-location` / `--no-sensor-forward-location`
  - `--sensor-forward-vhal` / `--no-sensor-forward-vhal`
  - `--sensor-vhal-min-ms N` to rate-limit repeated VHAL injections per signal
- Add first AAP SensorBatch to VehiclePropertyIds mapping:
  - speed -> `PERF_VEHICLE_SPEED` and `PERF_VEHICLE_SPEED_DISPLAY`
  - rpm -> `ENGINE_RPM`
  - odometer -> `PERF_ODOMETER`
  - fuel/range -> `FUEL_LEVEL` and `RANGE_REMAINING` using raw AAP values for now
  - parking brake -> `PARKING_BRAKE_ON`
  - gear -> `CURRENT_GEAR` and `GEAR_SELECTION`, translating AAP gear enum into `android.car.VehicleGear` bit values
  - night mode -> `NIGHT_MODE`
  - environment temperature -> `ENV_OUTSIDE_TEMPERATURE`
- Add fallback injection command for older AAOS images:
  - primary: `cmd car_service inject-vhal-event <propertyId> <areaId> <value>`
  - fallback: `dumpsys activity service com.android.car inject-vhal-event <propertyId> <value>`

Notes from the first test log:
- HU advertises only location, speed, odometer, night mode, and driving status on this DHU/emulator path.
- Requests for all advertised sensor types are accepted.
- The captured stream only showed `NIGHT_MODE` and `DRIVING_STATUS` batches so far; `NIGHT_MODE` can now be injected immediately, while `DRIVING_STATUS` stays log/JSONL-only because it does not map cleanly to a current public `VehiclePropertyIds` entry.

Next after test:
- Run on the real car/HU and check whether speed/odometer/location batches actually arrive.
- If `cmd car_service inject-vhal-event` is permission-blocked on the AAOS image, replace shell VHAL injection with a small privileged/app_process car-service helper.
- For fuel/range, verify actual AAP units from real packets before treating them as Android milliliters/meters in consumers.

## v44 AAOS AOA USB gadget bootstrap

Goal: make the working Raspberry Pi 4 AAOS USB-C Android Open Accessory gadget setup reproducible and bootable.

Verified manually:
- `dwc2` overlay enables `/sys/class/udc/fe980000.usb`.
- `configfs` gadget path works on AAOS.
- `functions/accessory.usb0` can be created.
- `/proc/misc` exposes `usb_accessory` with minor `122` on the tested image.
- `/dev/usb_accessory` exists after setup.
- macOS detects the device as Accessory and AAOS USB debugging still works in this setup.

Implemented in this patch:
- Add Rust `aaos-aoa-gadget` / alias `usb-gadget-aoa` as a manual diagnostic command, but make normal AAP daemon commands auto-run the gadget preflight first.
- The daemon now checks whether `/config/usb_gadget/arda` is already bound and complete; if missing, it creates `0x18D1:0x2D00`, `accessory.usb0`, the required absolute configfs symlink, binds to `fe980000.usb`, and ensures `/dev/usb_accessory` from `/proc/misc` when needed. This is idempotent and skips changes when already configured.
- Add `--check`, `--unbind`, `--no-bind`, and basic identity/UDC options.
- Keep an optional boot init rc that can run the gadget preflight during `late-init`, while the preferred runtime path is automatic preflight inside normal AAP daemon commands.
- Add macOS boot partition patch helper for `dwc2.dtbo`, `config.txt`, and `cmdline.txt`.
- Add ADB installer for `/vendor/bin/arda-senderd` and `/vendor/etc/init/arda-aoa-gadget.rc`.
- Add `docs/AAOS_AOA_USB_GADGET.md` with the verified setup, install flow, and troubleshooting.

Compatibility note:
- Treat this as an ARDA Raspberry Pi 4 AAOS image patch, not a universal AAOS patch. Other images may need a different UDC name, a different partition layout, or a kernel with missing accessory gadget support.

## v45 native capture graceful teardown

Goal: stop repeated AAP sessions from leaving native virtual-display / MediaCodec state half-torn-down on AAOS.

Observation from tests:
- Second run could freeze the physical monitor and DHU for ~20-30s even though the internal AA video timer later showed a fast first encoded frame.
- Logcat around repeated native capture showed `BufferQueue has been abandoned`, `CompositionEngine ... arda-native-display-capture ... dequeueBuffer failed`, and `v3d_bo_from_cache(): wait failed: EINVAL`, which points at the native virtual-display capture being killed before its cleanup path ran.

Implemented:
- Do not use immediate `Child::kill()` for the live native-capture process during normal session cleanup or first-frame retry.
- Send SIGTERM first and wait briefly, falling back to SIGKILL only if the child does not exit.
- Add SIGTERM/SIGINT/SIGHUP handling inside native display capture:
  - the native-capture parent forwards the signal to the forked display-capture child,
  - the child exits its drain loop cleanly,
  - `destroyDisplay`, `signalEndOfInputStream`, output close, and private handle cleanup still run.
- Shorten EOS wait on requested stop so Ctrl-C/session-end does not hang for a long time.

Next after test:
- Repeat three back-to-back runs and check for `stop requested: leaving drain loop for graceful cleanup` and `destroyDisplay: OK` in `/data/local/tmp/arda-display-capture-test.log`.
- If the monitor still freezes, add an even safer mode that stops live video by sending AA channel-close before stopping native capture and/or disables first-frame retry hard restarts.

## v46 AOA preflight root elevation

Goal: keep normal `aap-session-probe` runs clean by doing the USB accessory/configfs preflight as root instead of trying privileged file operations as the shell user.

Observation from test:
- `aap-session-probe` auto-preflight ran as `shell` and printed:
  - `mount: bad /etc/fstab: No such file or directory`
  - `chown: '/dev/usb_accessory' to 'root:usb': Operation not permitted`
  - `chmod: chmod '/dev/usb_accessory' to 20660: Operation not permitted`
- The session still worked, but those errors are noisy and could hide real setup failures.

Implemented:
- Auto AOA preflight now checks whether the gadget is ready and whether `/dev/usb_accessory` has root-owned `0660` permissions.
- If a fix is needed and the current process is not root, it re-runs only the `aaos-aoa-gadget` setup through `su -c ... --quiet`.
- Manual `aaos-aoa-gadget` / `usb-gadget-aoa` also self-elevates through `su -c` when called from a non-root shell, while `--check` remains a read-only status command.
- Configfs mount now avoids the Android `mount`/fstab noise when `/config/usb_gadget` already exists, and uses `mount -t configfs configfs /config` only if needed.
- Best-effort helper commands (`mount`, `mknod`, `chown`, `chmod`) no longer inherit stdout/stderr, so expected no-op failures do not pollute the AAP session log.

Next after test:
- Run `aap-session-probe` from the normal `adb shell` path and verify the old mount/chown/chmod permission lines are gone.
- Confirm `aaos-aoa-gadget --check` still prints status without requiring root.

## v47 AAOS phone internet bridge bootstrap

Goal: make the ARDA AAOS device usable without built-in internet by using the driver's phone.

Decision:
- Use Bluetooth only as a control/bootstrap channel later; do not carry bulk internet over BT.
- First MVP: phone normal hotspot/tethering, AAOS joins as Wi-Fi client.
- Second mode: phone LocalOnlyHotspot or AAOS SoftAP plus companion app proxy. For full-device internet, add tun2socks later; Android global HTTP proxy is only a partial bridge.
- Gearhead/AA Wireless bootstrap is useful as a model for exchanging SSID/key/IP/port, but it is projection transport bootstrap, not generic internet sharing.

Implemented now:
- `net-status` / `internet-status`: prints `cmd wifi status`, configured networks, SoftAP/tether diagnostics, Wi-Fi/AP interfaces, wlan0 address, routes, DNS props and Android global proxy.
- `net-join` / `internet-join`: enables Wi-Fi, starts scan, calls `cmd wifi connect-network <ssid> open|owe|wpa2|wpa3 [passphrase]`, waits briefly, then prints status.
- `net-disconnect` / `internet-disconnect`: forgets a network by SSID, or disables Wi-Fi if no SSID is supplied.
- `net-proxy` / `internet-proxy`: sets Android global HTTP proxy for companion HTTP-proxy experiments.
- `net-proxy-clear` / `internet-proxy-clear`: clears Android global HTTP proxy.
- `net-ap-start` / `internet-ap-start`: experimental AAOS SoftAP/reverse-mode helper; supports `--use-saved-config` to start the Settings Hotspot config through Connectivity tethering, or custom `cmd wifi start-softap`/LOHS when supported.
- `net-ap-stop` / `internet-ap-stop`: experimental SoftAP stop helper.
- Add `docs/AAOS_PHONE_INTERNET_BRIDGE.md` with mode comparison and test commands.

Next:
- Test `net-status` on the current AAOS image and capture exact `cmd wifi` support.
- Test MVP with phone hotspot and verify Maps/GMS internet.
- If phone hotspot cannot be automated, implement companion LocalOnlyHotspot + HTTP/SOCKS proxy offer over BT.
- If HTTP proxy is insufficient for Maps/GMS, add AAOS tun2socks helper and route default through the phone proxy.

## v48 AAOS SoftAP/tether diagnostics and saved-config start

Observed test result:
- Phone-hotspot/client mode did not connect to the requested SSID yet, but the device still had internet through `eth0`; the old wait logic incorrectly matched `Wifi is not connected` because it contained the word `connected`. Fixed by requiring a real connected status or an IPv4 address on `wlan0`.
- Reverse mode `cmd wifi start-softap ARDA-AAOS ...` returned success, but Settings still showed Hotspot off / saved config name. This is now treated as inconclusive rather than trusted.

Implemented:
- `net-status` now includes `dumpsys wifi` and `dumpsys connectivity` SoftAP/tether snippets plus Wi-Fi/AP-like interfaces.
- `net-ap-start --use-saved-config` starts the Settings Hotspot configuration through `cmd connectivity tether start wifi` first. This is the recommended reverse-mode test on the RPi AAOS image.
- Custom `net-ap-start --ssid ...` remains available, with Connectivity saved-config fallback.
- `net-ap-stop` now also calls `cmd connectivity tether stop wifi`.

## v49 SoftAP bootloop guard and Wi-Fi rescue

Observed test result:
- `net-ap-start --use-saved-config` reached `WifiService: startSoftAp uid=0`, but SoftAP failed at hostapd/HAL setup: `Cannot find entry android.hardware.wifi.hostapd@1.0::IHostapd/default`, `Failed to connect to hostapd`, `setup failure when creating ap interface`.
- After that, the device entered an Android userspace restart loop. Recovery was possible over ADB by stopping SoftAP/Wi-Fi and moving `/data/misc/apexdata/com.android.wifi/WifiConfigStore.xml` and `WifiConfigStoreSoftAp.xml` to `/data/local/tmp/arda-wifi-rescue`.

Implemented:
- `net-ap-start` is disabled by default on this image. It prints a warning and exits unless `--force-unsafe-softap` is explicitly supplied.
- `net-ap-stop` now works without attempting automatic `su -c`; add `--rescue-configs` to run the Wi-Fi rescue cleanup after stopping AP.
- New `net-wifi-rescue [--backup-dir DIR]` command performs the ADB recovery sequence: stop SoftAP, disable Wi-Fi, set `wifi_on=0`, back up Wi-Fi config stores, and sync.
- `run_self_command_as_root` now tries multiple Android `su` forms (`su -c`, `su 0 sh -c`, `su root sh -c`) because this image rejects `su -c`.

Decision:
- Do not auto-start AAOS SoftAP from init until the image has working hostapd/HAL/firmware support or a separate known-good AP path.
- The preferred internet route remains phone hotspot -> AAOS `net-join` first.
- Reverse mode is still architecturally valid: AAOS AP -> phone joins -> companion proxy bound to cellular. It needs a working AP backend before it can be made persistent.

## v50 local native capture repeat self-test

Goal: validate the unfinished v45 graceful-teardown work locally before returning to AAP/DHU rendering or AAOS image changes.

Implemented:
- Add `native-capture-repeat-test` / `native-repeat-test`.
- Default test runs native display capture 3 times back-to-back.
- Each run uses the real long-running `native-capture --seconds 0` path, waits briefly, sends SIGTERM, and waits up to 5 seconds before a SIGKILL fallback.
- Verify the native capture log contains all graceful teardown markers:
  - `stop requested: leaving drain loop for graceful cleanup`
  - `after destroyDisplay`
  - `after signalEndOfInputStream`
  - `after cleanup`
  - `child returned rc=0`
- Verify every run produces a non-empty H.264 file.
- Fail immediately on a missing marker, empty output, or non-zero child exit.

Next after test:
- Push the local Android binary and run the 3-pass repeat self-test on AAOS.
- Only after `NATIVE_CAPTURE_REPEAT_TEST_OK=YES`, return to the AAP session and measure first-frame/render latency.
- Do not touch AAOS framework/image code for rendering unless the local native capture path itself is clean and repeatable.

## v52 current baseline profile + direct USB AAP transport

Status: implemented in source; device compile/runtime verification pending.

- Added repository-root `config.toml` as the current bring-up source of truth.
- Added dependency-free ARDA config loader; unknown keys fail loudly.
- Added `config-check [PATH]` to print resolved config-derived CLI args.
- Added `aap-run`, which loads `/data/local/tmp/arda-config.toml`; later CLI flags override config.
- Removed automatic AOA gadget creation/rebind from normal command startup.
- Kept `aaos-aoa-gadget` only as a manual/legacy recovery command.
- Added read-only `usb-accessory-status` for `/proc/misc`, `/dev/usb_accessory`, UDC state, and configfs accessory function visibility.
- Added real HU-side `--transport usb-accessory --usb-path /dev/usb_accessory` support to the full AAP session path; `usb` remains a compatibility alias.
- aa-proxy source comparison confirmed the role: its HU/car branch opens `/dev/usb_accessory` directly, while its `nusb` AOA switch/bulk endpoint code is only for a wired phone/MD. ARDA is the phone-like AAP endpoint, so the HU-side accessory char device is the correct transport.
- USB accessory transport uses one blocking reader thread on a cloned accessory fd. The kernel accessory read path is interruptible; the session consumes reader events with timed channel waits, so the existing 20 ms video/audio pump cadence still runs between HU frames; cleanup interrupts and joins the reader thread.
- TCP remains available as an explicit debugging fallback.
- Current baseline keeps video + app/framework input only; audio and sensor/VHAL forwarding remain disabled until USB live-video refresh is stable.

Immediate v52 test order:

1. Build/push `arda-senderd` and push `config.toml` as `/data/local/tmp/arda-config.toml`.
2. Run `usb-accessory-status`; require `USB_ACCESSORY_READY=YES`.
3. Run `config-check /data/local/tmp/arda-config.toml` and verify `--transport usb-accessory`, `--video-frames 0`, Waveshare profile, and app input mode.
4. Start DHU/USB host side.
5. Run `aap-run` and verify VERSION/TLS/SDR over `/dev/usb_accessory` without TCP listener/client logs.
6. Exercise Maps/launcher continuously and verify SliceNonIdr + MEDIA DATA continue without a frame-limit stop.
7. Only after video refresh is stable, re-enable sensor/VHAL with CLI overrides or a dedicated future profile.
8. Use `scripts/arda-usb-run.sh` as the normal Mac workflow; stop reconstructing the baseline from shell history.

## v53 platform-integrated input bridge client

Observed platform hot-deploy result:
- `/system_ext/priv-app/ArdaInputBridge/ArdaInputBridge.apk` is scanned as privileged/system_ext, persistent, installed for users 0 and 10.
- `com.arda.inputbridge` starts at boot and logs `READY addr=127.0.0.1:5276`.
- The platform bridge is signed with the platform certificate, requests `android.permission.INJECT_EVENTS`, and injects via `InputManagerGlobal.injectInputEvent()`.
- Exact loopback protocol is line based: `DISPLAY <id>`, `T DOWN|MOVE|UP <x> <y> <pointer_id>`, `K <android_keycode>`, `STOP`/`BYE`, and optional `PING`.
- The V4L2 C2 APEX hot-deploy is also active after reboot: `com.android.hardware.media.c2.v4l2` is mounted active/factory, `vendor.media.c2.v4l2` is running, and `IComponentStore/v4l2` is registered.

Implemented in senderd:
- Add `--input-inject-mode bridge` plus aliases `platform`, `platform-bridge`, `platform_bridge`, `system-app`, and `system_app`.
- Add a long-lived TCP client for `127.0.0.1:5276`; it sends `DISPLAY` once on connect and forwards the already mapped AAP touch/key events using the bridge's exact protocol.
- Bridge connection is lazy on the first injected touch/key event. A broken socket is dropped so the next event can reconnect; an interrupted active gesture may be lost until the next DOWN, while the bridge itself cancels active touch on disconnect.
- Keep the existing 16 ms MOVE throttle for the platform bridge to avoid flooding framework injection.
- Keep `app_process`, `/dev/uinput`, and shell injection as explicit A/B fallbacks; do not remove the proven paths yet.
- Change repository `config.toml` baseline from `inject_mode = "app"` to `inject_mode = "bridge"`.

Immediate test:
1. Build/push the changed senderd and baseline config only; no new AAOS image burn is required because ArdaInputBridge is already hot-deployed and boot-active.
2. Run `config-check` and require `--input-inject-mode bridge`.
3. Start DHU, run `aap-run`, then tap/swipe continuously.
4. Require sender logs `INPUT_PLATFORM_BRIDGE addr=127.0.0.1:5276` and `INPUT_PLATFORM_TOUCH_INJECT`; require Android logcat `ArdaInputBridge: client connected` with no `injectInputEvent failed`.
5. Verify no new `app_process ... com.arda.input.InputServer` child exists during the session.
6. After input is stable, separately force `c2.v4l2.avc.encoder` in the capture path and prove the active encoder from Codec2 runtime logs.

## v54 secured platform input bridge transport

Observed security issue in v53:
- The platform-signed persistent InputBridge owned `INJECT_EVENTS` but accepted unauthenticated TCP clients on localhost. Loopback was only a routing boundary, not an authorization boundary.
- The bridge also requested `INTERNET` solely for the TCP transport.
- A local client could connect to the single blocking server and hold the connection, preventing senderd from reaching the injector.

Implemented:
- Replace `127.0.0.1:5276` TCP with the Linux abstract UNIX-domain socket `arda_input_bridge`.
- The bridge starts the socket server only for Android user 0.
- Do not create a filesystem socket node; this removes stale inode cleanup, chmod, and app-private directory traversal.
- Read kernel peer credentials immediately after `accept()` and accept only UID 0, matching the current root `arda-senderd` runtime required by `/dev/usb_accessory`. Unauthorized peers are closed before the line protocol is read.
- Remove `android.permission.INTERNET` from ArdaInputBridge.
- Keep the proven line protocol and AAP touch/key mapping unchanged: `DISPLAY`, `T`, `K`, `STOP`/`BYE`, `PING`.
- Senderd connects to the abstract namespace with `AF_UNIX`/`sockaddr_un`; reconnect behavior remains lazy/event-driven.

Security boundary for the current PoC/runtime:
- SELinux grants abstract-socket `connectto` only to the dedicated `arda_vehicle_router` domain, with shell/su permitted only in userdebug builds.
- Kernel peer credentials are checked by the privileged bridge and only root UID is authorized.
- Root remains trusted because the current senderd session itself must run as root to open `/dev/usb_accessory`.

Future hardening when senderd becomes an init-managed platform daemon:
- Give senderd a dedicated Android UID/SELinux domain.
- Move the socket to an init-created path with a dedicated SELinux type.
- Replace the temporary root UID allow-list with the dedicated daemon UID/domain.
- Keep Binder/AIDL as an optional later replacement; it is not required for the current transport hardening.

Immediate test:
1. Build and hot-deploy `ArdaInputBridge`; verify the old TCP listener on `5276` is gone.
2. Require no `arda-input.sock` filesystem node and require bridge log `READY transport=unix-abstract name=arda_input_bridge`.
3. Run senderd as root with `inject_mode = bridge`; require `INPUT_PLATFORM_BRIDGE transport=unix-abstract`, `socket_name=arda_input_bridge`, and `peer_auth=SO_PEERCRED/root-uid`.
4. Require bridge log `client accepted uid=0` and successful tap/swipe injection.
5. Attempt a connection from a normal app UID only if a controlled test helper is available; the bridge must log `rejected peer uid=...`.
6. After secure input is stable, return to sensor/VHAL work. The next architecture target is a Wasmtime/WebAssembly VHAL translation layer while preserving the proven AAP/TLS/SDR path.

## v55 explicit sensor gate and normalized event boundary

Observed bug in the v54 source-of-truth tree:
- Repository config correctly emitted `--no-sensor-open` and `--no-sensor-forward-vhal`.
- Later sensor parameter flags such as `--sensor-request-all`, `--sensor-max-type`, `--sensor-jsonl`, and `--sensor-vhal-min-ms` mutated the parser state and implicitly re-enabled SensorSource and/or VHAL forwarding.
- This explained runtime `SENSOR_SOURCE_OPEN`, sensor requests, and direct VHAL writes while the baseline config claimed sensors were disabled.

Implemented:
- Make `--sensor-open` / `--no-sensor-open` the only master SensorSource gate.
- Sensor parameter flags configure a future session but never enable the master gate.
- `--sensor-forward-vhal` selects VHAL output but does not implicitly open the sensor channel.
- Add `sensor-gate-smoke` as a no-HU regression test for the exact config-order bug.
- Replace protobuf-layer `DecodedVehicleProperty` generation with a normalized `ArdaSensorEvent` boundary.
- Preserve generated protobuf parsing, batch summary, JSONL, and location forwarding.
- Normalize the latest sample per sensor type in each AAP SensorBatch into stable Rust units/values.
- Extend the current normalized schema through SensorType 23, including toll card and opaque energy-model presence.
- Keep the request ceiling at 23 because the current generated SensorBatch schema has no trailer payload field for SensorType 24.

Resulting boundary:

```text
AAP SensorBatch protobuf
        -> parse generated proto
        -> ArdaSensorEvent
        -> policy / mapping layer
```

The AAP/TLS/SDR path is unchanged.

## v56 Wasmtime VHAL translation and policy layer

Goal:
- Move sensor-to-VHAL translation/policy out of the AAP session and protobuf parser.
- Allow OEM/vehicle mappings to change as a `.wasm` policy module without rewriting AAP/TLS/SDR or rebuilding the protocol path.
- Keep Android/VHAL authority in the Rust host rather than granting it to the guest.

Implemented:
- Pin Wasmtime `36.0.11` with a reduced `cranelift + runtime + std` feature set.
- Compile `wasm/arda_vhal_default.wat` at build time and embed the resulting Wasm module in senderd.
- Add external policy selection with `--sensor-wasm builtin|PATH`.
- Add Wasm ABI v1: `arda_abi_version()` and fixed `arda_on_sensor(kind, a..f)` normalized-event entrypoint.
- Expose only three typed guest imports: `arda_vhal.emit_bool`, `emit_i32`, and `emit_f32`.
- Do not install WASI; the guest receives no filesystem, network, shell, Binder, AAP session, or direct VHAL object.
- Add fixed fuel per sensor event and fail-closed command discard on guest trap.
- Add Wasmtime store resource limits: 2 MiB linear memory, one memory, one table, 1024 table elements, two instances; cap external module files at 4 MiB.
- Add host command-count limit per event.
- Validate every guest VHAL request in Rust by property allow-list, exact type, global area, finite/range checks, and per-property/area rate limiting.
- Isolate the old `cmd car_service inject-vhal-event` plus userdebug dumpsys fallback inside the VHAL host adapter; remove direct VHAL mapping/writes from `src/aap/mod.rs` and `src/aap/proto.rs`.
- Fail closed if the Wasm engine cannot initialize: normalized sensor decode and JSONL continue, but VHAL mapping remains disabled and does not fall back to the old direct path.
- Add `sensor-wasm-smoke`, which exercises deterministic normalized events through the embedded Wasm ABI and host validator in VHAL dry-run mode.
- Built-in policy maps speed, RPM, odometer, parking brake, mapped gear, night mode, and outside temperature.
- Fuel/range stay normalized but are deliberately not mapped by the built-in policy until the AAP source units are established.
- Add `docs/ARDA_SENSOR_WASMTIME.md` as the ABI, safety-boundary, normalized-value, and test reference.

Repository baseline remains intentionally off:

```toml
[sensor]
open = false
forward_vhal = false
wasm = "builtin"
wasm_fuel = 50000
wasm_max_commands = 16
```

Immediate combined test order:
1. Build senderd; the first build resolves Wasmtime and updates `Cargo.lock`.
2. Push senderd/config over network ADB because DHU AOAP mode can remove USB ADB.
3. Run `sensor-gate-smoke`; require `SENSOR_GATE_SMOKE_OK=YES`.
4. Run `sensor-wasm-smoke`; require `SENSOR_WASM_ENGINE ready` and `SENSOR_WASM_SMOKE_OK=YES` with only `dry_run` VHAL output.
5. Run normal baseline AAP with repository config and require no `SENSOR_SOURCE_OPEN`, no `SENSOR_WASM_ENGINE`, and no VHAL output.
6. Run AAP with explicit `--sensor-open --no-sensor-forward-vhal`; collect real-HU `SENSOR_BATCH`, `SENSOR_NORMALIZED`, `SENSOR_WASM_EVENT`, and `SENSOR_WASM_VHAL dry_run` lines.
7. Only after inspecting real values, run explicit `--sensor-open --sensor-forward-vhal` and verify validated `SENSOR_WASM_VHAL applied` lines plus AAOS property behavior.
8. Do not accept any `SENSOR_OS_VHAL` line; that direct session/proto mapping path has been removed.


## v57 Phase 1.1 direct VehicleProperty model + generic Wasm pass-through

Reason for the Phase 1.1 correction:
- The first v57 router checkpoint used an ARDA-specific generic signal namespace and implemented the
  default WAT policy as a property-by-property `if` chain.
- That shape duplicated standard AAOS/VHAL semantics inside the policy guest and would grow into a
  giant switch table.
- The first Docker compile also exposed a rust-protobuf v3 codegen mismatch: router code consistently
  used generated accessors, while protobuf v3 does not generate public-field accessors by default.

Implemented in the current Phase 1.1 source:
- Explicitly request rust-protobuf generated field accessors in `build.rs`.
- Rename the new router protocol fields from `signal_id` to `property_id`; protobuf field numbers stay
  unchanged.
- Remove the second ARDA signal-ID namespace. `VehicleSignal.property_id` and
  `VehicleCommand.property_id` now carry Android `VehicleProperty` IDs directly.
- Map the known built-in AAP/HU sensor types to standard VHAL properties in Rust before the router.
- Keep AAP location out of VHAL/Wasmtime for the future GPS_PROVIDER -> fused path.
- Keep AAP FuelData unmapped because the source protobuf does not define fuel/range units.
- Replace the builtin WAT property switch with a value-type-only pass-through policy. The default
  guest preserves property ID, area ID and value in both vehicle->VHAL and VHAL-set->vehicle
  directions.
- Keep custom Wasm modules as optional policy/remap/stateful logic only where an OEM integration
  actually needs them.
- Rename the platform-internal protobuf property fields to `property_id`. Keep the wire field type as
  `int32`, rather than a closed property enum, so vendor VehicleProperty IDs remain representable.
- Replace the Rust `property_rule()` match chain with declarative `SUPPORTED_PROPERTY_RULES` metadata.
  This registry is only the Phase 1 validation/authorization subset; it is not a signal mapping table.
- The future real AIDL VHAL backend should derive supported property/type/area metadata from the active
  VehiclePropConfig set and combine it with Arda Vehicle Manager grants.

Current default flow:

```text
AAP/HU SensorSource
    -> Rust built-in sensor mapping
    -> VehicleProperty ID + area + typed value
                                      \
third-party/CAN adapter ---------------> router / source arbitration
                                         -> generic Wasm policy
                                         -> same VehicleProperty event
                                         -> VHAL

VHAL set request
    -> router
    -> generic Wasm policy
    -> same VehicleProperty command
    -> approved adapter
```

Immediate Phase 1.1 validation order:
1. Rebuild senderd with the Phase 1.1 changed files.
2. Require the old rust-protobuf `no method named set_*` / `field, not a method` cascade to disappear.
3. Run `sensor-gate-smoke`.
4. Run `sensor-wasm-smoke` and verify the known HU mappings pass through unchanged.
5. Run `vehicle-policy-smoke` and require `VEHICLE_POLICY_SMOKE_GENERIC_PROPERTY_PASS_THROUGH=YES`.
6. Start `vehicle-routerd`, then run `vehicle-router-smoke` and require property-ID-preserving HVAC
   command routing plus capability-scoped source arbitration.
7. Only after Phase 1.1 compile/smoke success, continue with Vehicle Manager/AIDL adapter API.

### v57 Phase 1.1 build determinism follow-up

- The first Phase 1.1 Android build still compiled against a no-accessor generated
  `arda_vehicle_router.rs`, producing the same 204 `E0599` accessor cascade.
- `generate_accessors(true)` remains the intended rust-protobuf API contract for the router.
- Android Docker builds now clean only the `arda-senderd` artifacts for the selected target before
  compiling so timestamp-preserving `rsync -av` patch restores cannot reuse stale `OUT_DIR/protos`.
- `build.rs` now validates the generated router source for required accessors immediately after
  codegen and emits `ARDA_PROTO_ACCESSORS_OK=...` on success.
- This is a build/codegen determinism fix only; VehicleProperty IDs, Rust AAP/HU mappings, generic
  Wasmtime typed pass-through, router arbitration and command routing remain unchanged.

## v57 Phase 1.1 runtime validation correction — 2026-07-07

Authoritative user-side results after the protobuf codegen refresh fix:
- Android Docker cross-build passed.
- `sensor-gate-smoke` passed with `SENSOR_GATE_SMOKE_OK=YES`.
- `vehicle-policy-smoke` passed with generic property pass-through and `VEHICLE_POLICY_SMOKE_OK=YES`.
- `sensor-wasm-smoke` exposed one source-identity bug: `sensor_event_to_vehicle_signals()` created
  otherwise valid built-in AAP VehicleProperty signals with reserved `source_id=0`. The live router
  publisher rewrote those signals to source 1 before publish, so the bug was hidden on that path; the
  direct dry-run policy path correctly rejected them before Wasm.
- The first combined router shell command produced no router output because
  `pkill -f "arda-senderd vehicle-routerd"` can match the remote shell's own `sh -c` command line,
  which contains that exact text, and terminate the test shell before daemon startup.

Runtime correction in the current tree:
- Define one `AAP_SENSOR_SOURCE_ID=1` constant in `src/vehicle/model.rs`.
- Built-in AAP/HU sensor mapping now creates valid source-1 `VehicleSignal` values immediately.
- `SensorPipeline` uses the same constant when registering `AAP SensorSource` with the router.
- Dry-run `SENSOR_WASM_EVENT` logging includes the source ID, so the policy boundary is visible.
- Add `scripts/run-vehicle-router-smoke-android.sh`, which captures the PID of the router it starts and
  kills only that process. It intentionally does not use `pkill -f` and defaults to the dedicated
  `/data/local/tmp/arda_vehicle_router_smoke.sock` path so an active normal router socket is untouched.

Immediate validation order after this correction:
1. Rebuild with `./scripts/docker-build.sh`.
2. Push the rebuilt `arda-senderd`.
3. Run `sensor-wasm-smoke`; require `source=1`, no `SENSOR_WASM_TRAP`, and
   `SENSOR_WASM_SMOKE_OK=YES`.
4. Run `DEVICE=<network-adb-device> ./scripts/run-vehicle-router-smoke-android.sh` and require all three
   `VEHICLE_ROUTER_SMOKE_*_OK=YES` markers.
5. If both pass, close v57 Phase 1.1 and start Phase 2 Vehicle Manager / adapter AIDL work.



## v57 Phase 1.1 router privilege smoke correction — 2026-07-07

Authoritative user-side runtime result after the source-ID correction:
- `sensor-wasm-smoke` passed with all built-in AAP events carrying `source=1`.
- No `SENSOR_WASM_TRAP` remained.
- `SENSOR_WASM_SMOKE_OK=YES` passed.
- The PID-scoped router runner reached daemon startup, but the daemon exited after Wasmtime init with
  `Operation not permitted (os error 1)` before `VEHICLE_ROUTER_READY`.

The router startup order makes this deterministic: after the Wasmtime ready line it binds the Unix
socket, applies mode `0660`, then explicitly changes the socket owner to `root:system` (`0:1000`). The
runner was launching through a normal ADB shell (`AID_SHELL`/UID 2000), so the explicit `chown(0,1000)`
required by the router security boundary failed with `EPERM`.

Current correction:
- Do not weaken or ignore the router socket ownership requirement for `/data/local/tmp` smoke paths.
- `scripts/run-vehicle-router-smoke-android.sh` now requests `adb root`, reconnects until the device
  shell reports UID 0, and fails with `ROUTER_ADB_ROOT_REQUIRED` if root is unavailable.
- The runner emits `ROUTER_ADB_ROOT_OK ... uid=0` before daemon startup.
- Router startup I/O now reports the exact failing stage, path, EUID and EGID. A future ownership
  failure is reported as `stage=chown_socket_root_system_requires_privilege` instead of a bare OS error.

Remaining Phase 1.1 gate:
1. Rebuild and push the corrected binary.
2. Run the root-aware PID-scoped router smoke runner over network ADB.
3. Require `VEHICLE_ROUTER_SMOKE_ARBITRATION_OK=YES`,
   `VEHICLE_ROUTER_SMOKE_PROPERTY_ID_COMMAND_OK=YES`, and `VEHICLE_ROUTER_SMOKE_OK=YES`.
4. Only then close Phase 1.1 and begin Vehicle Manager / adapter AIDL Phase 2.

## v57 Phase 1.1 closed / Phase 2.0 started

Raspberry Pi runtime validation completed on 2026-07-07:

```text
SENSOR_GATE_SMOKE_OK=YES
SENSOR_WASM_SMOKE_OK=YES
VEHICLE_POLICY_SMOKE_GENERIC_PROPERTY_PASS_THROUGH=YES
VEHICLE_POLICY_SMOKE_OK=YES
VEHICLE_ROUTER_SMOKE_ARBITRATION_OK=YES
VEHICLE_ROUTER_SMOKE_PROPERTY_ID_COMMAND_OK=YES
VEHICLE_ROUTER_SMOKE_OK=YES
```

Phase 1.1 is closed.

Phase 2.0 adds the platform-side third-party adapter boundary. Canonical source is `Deadknight/android_vendor_arda` under `vehicle/`; the AOSP destination is `vendor/arda/vehicle/`:

```text
arda.vehicle.adapter AIDL
arda-vehicle-adapter-sdk
ArdaVehicleManager
ArdaVehicleSmokeAdapter
```

Security model:

```text
adapter cannot choose source_id
manager allocates source_id and priority
capability grants are explicit subsets
service ComponentName + Android signing lineage bind the grant
single-signer certificate rotation follows verified SigningInfo history
multi-signer packages require an exact signer set
```

Phase 2.0 discovers, grants, binds and validates adapter callbacks. It intentionally logs accepted
signals/acks rather than talking to `vehicle-routerd` yet.

Next: Phase 2.1 manager-to-router bridge using the existing protocol v2 without changing the proven
AAP/TLS/SDR path.

## Phase 2.0 runtime closed / Phase 2.1 manager-router bridge source — 2026-07-08

Authoritative Raspberry Pi Phase 2.0 runtime result:

```text
ARDA_VM_GRANT_OK ... source=10000 priority=100 publish=[2, 3] commands=[16]
ARDA_VM_STATUS ... grants=1 ... granted=true signer_ok=true connected=true ready=true
ARDA_VM_RESCAN user=10
ARDA_VM_BIND_REQUEST ... source=10000
ARDA_VM_ADAPTER_READY ... source=10000
SMOKE_ADAPTER_HOST_CONNECTED
ARDA_VM_SIGNAL_ACCEPT ... property=291504903 area=0 type=4
ARDA_VM_SIGNAL_ACCEPT ... property=358614274 area=1 type=4
ARDA_PHASE2_0_RUNTIME_GATE=PASS
```

Phase 2.0 status: CLOSED.

Phase 2.1 current source implements the real `ArdaVehicleManager` <-> `vehicle-routerd` protocol-v2
bridge without changing the proven AAP/TLS/SDR path or adding a second signal namespace.

Key architecture:

```text
one granted adapter/source
    -> one ROLE_MANAGER LocalSocket
    -> Hello protocol 2
    -> RegisterSource from stored manager grant
    -> VehicleSignals with manager-injected source_id
    <- VehicleCommands
    -> one-way AIDL adapter delivery
    <- CommandAck
    -> CommandAcks
```

Use one router connection per source rather than multiplexing all adapters on one manager socket.
Protocol v2 has no `UnregisterSource`; router cleanup is client-connection scoped. Per-source sockets
make revoke, user-switch and adapter-disconnect cleanup fail closed using the existing proven
`cleanup_client()` behavior, with no router protocol change.

Additional Phase 2.1 hardening:
- bounded latest-state retention per `(propertyId, areaId)` while the router is unavailable;
- no ACK replay across router reconnects;
- finite handshake read timeout;
- command boundary validation before Binder delivery;
- current-connection checks around adapter-ready activation;
- stale rejected connections cannot remove a newer replacement binding;
- product source remains canonical under `vendor/arda/vehicle`;
- production product includes only `ArdaVehicleManager`; smoke adapter remains opt-in.

New end-to-end smoke command:

```text
vehicle-manager-bridge-smoke
```

It verifies:

```text
VHAL set
 -> router command dispatch
 -> Manager per-source bridge
 -> AIDL smoke adapter
 -> CommandAck
 -> adapter-confirmed state publish
 -> router
 -> VHAL event
```

and waits beyond the router's command timeout before a Ping/Pong drain so a late matching
`VhalSetError` proves the ACK round-trip failed.

Phase 2.1 status is SOURCE IMPLEMENTED / REAL BUILD + ENFORCING PI RUNTIME PENDING. The artifact
preparation environment has no Cargo; do not mark Rust compile passed until the existing Android
Docker build succeeds. Real AOSP/Soong build is also required for the Java protobuf-lite module.

Do not solve a Manager -> router SELinux denial by switching the product to permissive. If the
new enforcing smoke runner captures AVC denials, package `vehicle-routerd` as an init-managed daemon
with a dedicated SELinux domain/socket policy next.

## Phase 2.1 enforcing boundary / Phase 2.2 router platformization — 2026-07-08

Real Phase 2.1 Android Docker Rust and canonical vendor-path Soong builds passed. The Manager's
`LocalSocket` ordering bug was closed by connecting before applying the handshake timeout.

The first actual enforcing Pi connect now fails on the exact platform boundary:

```text
avc: denied { write }
scontext=u:r:system_app:s0
tcontext=u:object_r:socket_device:s0
tclass=sock_file
permissive=0
```

The router is ready and the generic `/dev/socket/arda_vehicle_router` inode exists, but the generic
`system_app` Manager is correctly denied access to generic `socket_device`. Do not add a broad
`system_app -> socket_device` allow rule and do not use permissive mode as a pass condition.

Phase 2.2 source platformizes this boundary:

```text
init
  -> creates + binds + listens arda_vehicle_router stream socket
  -> labels socket arda_vehicle_router_socket
  -> starts /system_ext/bin/arda-senderd vehicle-routerd
  -> arda_vehicle_router domain
  -> ANDROID_SOCKET_arda_vehicle_router inherited fd

com.arda.vehicle.manager
  -> package-specific arda_vehicle_manager domain
  -> narrow unix_socket_connect to arda_vehicle_router
```

Rust validates the inherited fd is open, `SOCK_STREAM`, already listening, and bound to the expected
default pathname before taking ownership. The existing manual `UnixListener::bind()` path remains as a
fallback when the init socket environment variable is absent or a custom `--socket` path is used.

Product integration now includes `arda-vehicle-routerd` with `ArdaVehicleManager`; the smoke adapter
remains opt-in. The generated Android ARM64 `arda-senderd` binary is staged into the Soong prebuilt
module by `scripts/stage-arda-vehicle-router-prebuilt.sh` and is intentionally Git-ignored.

The exact Enforcing AVC closure added only the permissions proven by runtime:

```text
allow arda_vehicle_manager arda_vehicle_router_socket:sock_file write;
allow arda_vehicle_manager arda_vehicle_router_socket:unix_stream_socket connectto;
allow arda_vehicle_router self:process execmem;
```

The userdebug/eng `su` smoke client receives the same narrow socket-type `connectto` allowance; no
generic `system_app -> socket_device` permission was added.

## Phase 2.2 enforcing AVC closure — 2026-07-08

The canonical system_ext executable label fix passed on the Pi. Under Enforcing, init now creates the
`arda_vehicle_router_socket`, starts `arda-senderd` in `u:r:arda_vehicle_router:s0`, and the Manager
runs in `u:r:arda_vehicle_manager:s0`.

The next exact runtime AVCs were:

```text
avc: denied { connectto }
scontext=u:r:arda_vehicle_manager:s0
tcontext=u:object_r:arda_vehicle_router_socket:s0
tclass=unix_stream_socket
permissive=0

avc: denied { execmem }
scontext=u:r:arda_vehicle_router:s0
tcontext=u:r:arda_vehicle_router:s0
tclass=process
permissive=0
```

The init-created listening socket carries the dedicated socket object type, so the generic
`unix_socket_connect(..., serverdomain)` macro's server-domain `connectto` rule does not match this
endpoint. Phase 2.2 now grants only `sock_file write` plus `unix_stream_socket connectto` from the
Manager to `arda_vehicle_router_socket`. The same narrow socket-type rule is available to `su` only in
userdebug/eng for the root smoke client.

Wasmtime JIT is allowed `self:process execmem` only in the dedicated `arda_vehicle_router` domain. Do
not copy this permission to Manager, apps, `system_app`, or generic domains. Do not pre-authorize any
further Wasmtime permission; rerun the Enforcing gate and add only permissions proven by exact AVCs.

Next authoritative gate: rebuild the exact system_ext policy/hash/precompiled-policy set, deploy only
the coherent SELinux artifacts, reboot, require router service/socket/domain plus Manager domain, then
rerun the Manager bridge VHAL SET -> AIDL adapter -> ACK -> confirmed-state smoke under Enforcing.

## Phase 2.1 / Phase 2.2 closed — 2026-07-08

Authoritative final Raspberry Pi Enforcing runtime result:

```text
MANAGER_BRIDGE_MANAGER_DOMAIN=PASS
MANAGER_BRIDGE_ROUTER_REGISTRATION=PASS
VEHICLE_MANAGER_BRIDGE_SMOKE_SET_HANDLED=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_CONFIRMED_STATE=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_ACK_ROUND_TRIP=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_OK=YES
MANAGER_BRIDGE_ENFORCING_RUNTIME_GATE=PASS selinux=Enforcing
```

The proven command path is:

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

Platform contexts are proven under Enforcing:

```text
/system_ext/bin/arda-senderd        u:object_r:arda_vehicle_router_exec:s0
/dev/socket/arda_vehicle_router     u:object_r:arda_vehicle_router_socket:s0
arda-senderd                        u:r:arda_vehicle_router:s0
com.arda.vehicle.manager            u:r:arda_vehicle_manager:s0
```

The router uses the init-created listening socket through
`ANDROID_SOCKET_arda_vehicle_router`, reports `VEHICLE_ROUTER_SOCKET_MODE=init_inherited`, and starts
the builtin Wasmtime vehicle policy engine successfully.

Phase 2.1 status: CLOSED.
Phase 2.2 status: CLOSED.

Canonical platform source ownership:

```text
Deadknight/android_vendor_arda
  branch arda-aaos16
  vehicle/
  commit b53e5473d31de98573cb426c6618bdc9cc699e07

Deadknight/android_device_brcm_rpi4
  branch arda-aaos16
  commit 0b13b2a565056f0b32a23abe9acc362401524b1a
```

`aaos-aap` no longer owns a duplicate in-repo Vehicle platform tree. Its Rust router, proto, Wasmtime
policy and smoke tooling remain here; Android Vehicle Manager/AIDL/SEPolicy/init source lives in
`android_vendor_arda/vehicle`.

## Phase 3.0A fidelity repair + Phase 3.0B Android GPS provider bridge — 2026-07-09

The 2026-07-09 source snapshot was older than the previously characterized Phase 3.0A sensor tree.
The current Phase 3 patch therefore folds the required SensorBatch fidelity repair into the real
location bridge instead of rebuilding a patch from memory.

Rust now preserves:

```text
LocationData.timestamp presence
LocationData latitude/longitude
optional accuracy/altitude/speed/bearing presence
GpsSatelliteData.number_in_view presence
per-satellite PRN/SNR/used-in-fix/azimuth/elevation presence and values
SensorSourceService.location_characterization presence
all 23 SensorBatch event kinds
```

The exact protobuf regression gate is:

```text
sensor-proto-smoke
```

Required markers:

```text
SENSOR_PROTO_ALL_23_EVENT_KINDS=YES
SENSOR_PROTO_LOCATION_OPTIONAL_PRESENCE=YES
SENSOR_PROTO_GPS_SATELLITE_FIDELITY=YES
SENSOR_PROTO_SMOKE_OK=YES
```

AAP `LocationData` does not enter VehicleSignals, VehicleProperty, the vehicle router, or Wasmtime.
The location path is now deliberately separate:

```text
AAP SensorBatch.location_data
  -> exact Rust protobuf decode
  -> PlatformLocationBridge
  -> abstract AF_UNIX socket arda_location_provider
  -> platform ArdaLocationProvider
  -> LocationProviderBase.reportLocation()
  -> Android GPS_PROVIDER
  -> fused/location consumers
```

The old `cmd location providers add-test-provider` / `set-test-provider-location` path is removed from
sensor runtime forwarding. `--sensor-location-provider` remains only as a legacy CLI/config field and
is ignored with a warning when location forwarding uses the dedicated platform bridge.

The platform GNSS override is selected through:

```text
config_useGnssHardwareProvider=false
config_gnssLocationProviderPackageName=com.arda.location.provider
```

A narrow LocationManagerService patch moves configured GNSS override discovery outside the
`GnssNative.isSupported()` hardware gate. Hardware GNSS behavior is preserved when a HAL exists: the
override owns `GPS_PROVIDER` and hardware remains available as `GPS_HARDWARE_PROVIDER`. With no GNSS
HAL, the configured override can still own `GPS_PROVIDER`.

Satellite metadata fidelity is retained in Rust but is not fabricated into framework `GnssStatus`.
Phase 3.0B closes the location-fix provider path only.

Platform apply helper:

```text
scripts/apply-arda-location-phase3-aosp.sh
```

Provider bridge smoke helper:

```text
scripts/run-arda-location-phase3-smoke.sh
```

Phase 3.0B is SOURCE IMPLEMENTED / BUILD + PI RUNTIME PENDING. Do not mark it closed until the exact
protobuf markers, Android/Soong build, provider bind/socket gate, and real AAP runtime report through
`GPS_PROVIDER` are proven.

Do not force ambiguous AAP fuel, wheel-speed ordering, tire/door area, cruise, or satellite semantics
into Android VehicleProperty IDs merely to increase the mapping count. Continue Phase 3.0C only from
semantics that are exact in the authoritative AAP proto/runtime evidence.

### Phase 3.0B2 - ARDA GNSS status plane (2026-07-09)

Status: SOURCE IMPLEMENTED / BUILD + REAL AAP RUNTIME PENDING

Runtime evidence from Phase 3.0B proved real AAP `LocationData` reaches the ARDA provider,
`GPS_PROVIDER`, GMS fused input, and Maps direct GPS registrations with exact coordinate fidelity.
Maps still showed `Searching for GPS...`. The frozen BP4A framework creates
`GnssManagerService` only when `GnssNative.isSupported()`; with the ARDA GPS provider override
and no hardware GNSS HAL, `registerGnssStatusCallback()` otherwise has no backend.

Phase 3.0B2 adds a narrow ARDA-only GNSS status fallback in `system_server`:

- real AAP `LocationData` published to Android GPS -> GNSS started + first-fix status
- real AAP `GpsSatelliteData` -> `GnssStatus` only from listed satellite details
- AAP PRN/SNR/used-in-fix/azimuth/elevation fidelity is preserved
- constellation remains `CONSTELLATION_UNKNOWN` because AAP does not identify constellation
- ephemeris, almanac, carrier frequency, and baseband C/N0 are never invented
- satellite entries lacking required Android `GnssStatus` azimuth/elevation are skipped, not filled
- location remains outside VehicleProperty/router Wasm/VHAL

Required close markers include real Maps GNSS status listener registration plus:

- `ARDA_GNSS_STATUS_FIX_PUBLISHED`
- `ARDA_GNSS_STATUS_PROVIDER_STARTED`
- `ARDA_GNSS_STATUS_PROVIDER_FIRST_FIX`
- Maps no longer remaining indefinitely in `Searching for GPS...` after a real AAP fix


### Phase 3.0B3 - Vendor GNSS AIDL HAL migration (2026-07-09)

Status: SOURCE MIGRATION / BUILD + SYNTHETIC RUNTIME PENDING

The Phase 3.0B2 framework GNSS-status fallback proved the missing Maps semantic layer, but it is
diagnostic-only and superseded. Final integration moves location into a vendor GNSS AIDL HAL so the
frozen stock Android location/GNSS services remain unmodified.

```text
AAP SensorBatch.location_data
  -> exact Rust protobuf decode
  -> PlatformGnssHalBridge
  -> abstract AF_UNIX socket arda_gnss_hal
  -> vendor android.hardware.gnss.IGnss/default
  -> stock GnssNative / GnssManagerService / GnssLocationProvider / GnssStatusProvider
  -> Android GPS_PROVIDER and consumers
```

`GpsSatelliteData` fidelity remains preserved in Rust. It is not forwarded to AIDL `GnssSvInfo` yet
because AAP does not provide constellation or mandatory baseband C/N0 semantics. No constellation,
baseband C/N0, NMEA, raw measurement, ephemeris, almanac, or carrier-frequency values are invented.

Required source/runtime markers include:

```text
ARDA_GNSS_HAL_BRIDGE_CONNECT
ARDA_GNSS_HAL_FIX_PUBLISHED
ARDA_GNSS_HAL_SV_NOT_FORWARDED reason=aap_lacks_constellation_and_baseband_cn0
ARDA_GNSS_HAL_READY socket=arda_gnss_hal
ARDA_GNSS_HAL_CLIENT uid=0
ARDA_GNSS_HAL_FIX_RECEIVED
ARDA_GNSS_HAL_FIX_REPORTED
```

The old Java `ArdaLocationProvider`, its GNSS resource overlay, the Phase 3.0B LMS override gate, and
the Phase 3.0B2 `ArdaGnssStatusProvider` LMS fallback are removed by the AOSP migration. No `repo sync`
is part of this phase. Phase 3.0B3 remains open until Android/Soong build, policy coherence, HAL service
registration, synthetic bridge runtime, and real AAP location delivery through stock `GPS_PROVIDER`
are proven.

## Phase 3.0B3 — Standard GNSS AIDL location path closed (2026-07-10)

Runtime path:

`AAP SensorBatch.location_data -> arda-senderd -> ARDA GNSS bridge -> android.hardware.gnss.IGnss/default -> stock GnssNative / GPS_PROVIDER -> Maps and GMS fused`

Final platform identity:

- Soong module: `android.hardware.gnss-service.arda`
- Executable: `/system_ext/bin/hw/android.hardware.gnss-service.arda`
- Init service: `arda.gnss-hal`
- AIDL instance: `android.hardware.gnss.IGnss/default`
- SELinux domain: `arda_gnss_hal`
- Location remains outside VehicleProperty, VHAL, and Wasm.
- No fake boot/default location is generated.
- The latest real AAP fix is replayed only while the AAP GNSS bridge client remains connected, with fresh callback timestamps.
- Per-replay info logging is disabled; replay behavior remains active.
- Real runtime observation: Maps no longer remains in `Waiting for GPS`; subsequent real AAP location commands move Maps.
- `GpsSatelliteData` Android GNSS SV mapping remains pending rather than fabricating constellation/baseband semantics.

Checkpoint markers:

`ARDA_PHASE3_0B3_STANDARD_GNSS_AIDL_SERVICE=PASS`

`ARDA_PHASE3_0B3_STOCK_GPS_PROVIDER_PATH=PASS`

`ARDA_PHASE3_0B3_MAPS_LOCATION_DELIVERY=PASS`

`ARDA_PHASE3_0B3_GMS_FUSED_DELIVERY=PASS`

`ARDA_PHASE3_0B3_ACTIVE_FIX_REPLAY=PASS`

`ARDA_PHASE3_0B3_MAPS_WAITING_FOR_GPS_RESOLVED=YES`

`ARDA_PHASE3_0B3_REAL_AAP_LOCATION_RUNTIME=PASS`

`ARDA_PHASE3_0B3_LOCATION_CLOSED=YES`

Canonical vendor GNSS checkpoint: `72fe5c085ce32c1a43452c05b75eb92d888c9156`
Ubuntu AOSP device GNSS checkpoint: `2551dd2cdcabef976f3d8c66b9d56772034f2b82`
Canonical device GNSS integration checkpoint: `787c5b67e54b31e22af70c918481417ef149fb2f`

## Phase 3.0C — exhaustive safe advertised HU sensor mapping

The current HU advertises 13 SensorSource kinds. Phase 3.0C maps every advertised kind that has an exact standard Android VehicleProperty semantic and leaves the rest outside VHAL rather than fabricating meaning.

Mapped from AAP source `1`:

- `SENSOR_SPEED(3)` -> `PERF_VEHICLE_SPEED` only. `PERF_VEHICLE_SPEED_DISPLAY` is intentionally not synthesized.
- `SENSOR_ODOMETER(5)` -> `PERF_ODOMETER`.
- `SENSOR_FUEL(6).low_fuel_warning` -> `FUEL_LEVEL_LOW` only when the optional proto field is present. Raw fuel level/range remain unmapped because AAP does not define the VHAL-required milliliter/meter units.
- `SENSOR_PARKING_BRAKE(7)` -> `PARKING_BRAKE_ON`.
- `SENSOR_GEAR(8)` -> `GEAR_SELECTION` only. `CURRENT_GEAR` is intentionally not synthesized from selected gear.
- `SENSOR_NIGHT_MODE(10)` -> `NIGHT_MODE` only when the optional proto field is present.
- `SENSOR_TOLL_CARD(22)` -> `ELECTRONIC_TOLL_COLLECTION_CARD_STATUS`: absent -> `NOT_INSERTED`, present -> `UNKNOWN`; card validity is never fabricated.

Advertised but intentionally outside VehicleProperty mapping:

- `SENSOR_LOCATION(1)` remains on the closed standard GNSS AIDL path.
- `SENSOR_COMPASS(2)` has no exact standard heading/pitch/roll VehicleProperty.
- `SENSOR_DRIVING_STATUS_DATA(13)` is Android Auto projection restriction state, not a physical VHAL property.
- `SENSOR_ACCELEROMETER_DATA(19)` has no exact standard three-axis acceleration VehicleProperty.
- `SENSOR_GYROSCOPE_DATA(20)` has no exact standard three-axis angular-velocity VehicleProperty.
- `SENSOR_GPS_SATELLITE_DATA(21)` remains outside VHAL and GNSS status because AAP lacks exact Android constellation/baseband-CN0 semantics.

Phase 3.0C preserves the boundary `AAP sensor ID -> Android VehicleProperty` in Rust and keeps the default Wasm policy generic/typed.

## RN-3B Wi-Fi handoff MVP — 2026-07-14

Validated before implementation:
- senderd loopback bridge listener `127.0.0.1:5283` passed.
- token auth passed.
- HELLO / HELLO_ACK passed.
- PING / PONG passed.

Implemented in the current source:
- Add operation 10 `INTERNET_OFFER`.
- Add operation 11 `INTERNET_RESULT`.
- Add operation 12 `INTERNET_STATUS`.
- Add opt-in token-authenticated Wi-Fi listener, default test endpoint `0.0.0.0:5284`.
- Add phone-hotspot mode: exact SSID/PSK/security -> AAOS `net-join` -> require target SSID plus wlan0 IPv4 -> return AAOS IP/port over the still-open Bluetooth bridge.
- Add local-hotspot HTTP-proxy mode using the existing `net-proxy` helper.
- Keep bootstrap RFCOMM open as fallback while the companion performs a second ARDBR001 + HELLO/PING gate over Wi-Fi.
- Tighten `net-join`: an unrelated existing wlan0 connection no longer counts as success.
- Add Mac smoke script `scripts/test-rn3b-wifi-handoff-macos.sh`.

Still required outside this source archive:
- Patch phone companion to start/inspect hotspot, send INTERNET_OFFER, read INTERNET_RESULT, then connect to returned AAOS Wi-Fi endpoint.
- No ArdaPlatform protocol change is required if it remains a transparent RFCOMM byte pump.
- Do not close RFCOMM until the direct Wi-Fi HELLO/PING/PONG gate passes.


## 2026-08-13 canonical private-repo checkpoint + 1080p30 H.264 performance

This section supersedes older "current milestone" pointers above. The chronological history remains useful, but the canonical working state is now the checkpoint below.

### Ubuntu/AOSP source checkpoint

Build host and tree:

```text
edo@192.168.1.55
/home/edo/aosp-rpi-aaos16
```

The following ARDA-owned repositories are the current remote-contained checkpoints. All GitHub repositories listed here are **PRIVATE** for the current temporary development period.

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

Checkpoint finalization gates:

```text
ALL_SIX_PINNED_HEADS_PRESERVED=PASS
ALL_SELECTED_GITHUB_REPOS_PRIVATE=PASS
ALL_SIX_ARDA_TARGET_REPOS_REMOTE_CONTAINED=PASS
UBUNTU_DIRTY_REPOS_AFTER=0
GLOBAL_AOSP_WORKTREES_CLEAN=PASS
AOSP_REMOTE_PUSHED=NO
REPO_SYNC_CALLED=NO
ARDA_FINALIZE_UNSHALLOW_PUSH_V6=PASS
```

`packages/apps/Car/Launcher` and `packages/apps/Car/MediaPrebuilt` were shallow AOSP checkouts. They were targeted with:

```text
git fetch --unshallow --no-tags aosp
```

only so their current branches could be pushed to empty/private GitHub repositories. Their HEADs and worktrees were verified unchanged. This was **not** `repo sync`.

The Mac `aaos-aap` repository remains separately user-managed:

```text
/Users/edo/Documents/Code/aaos-aap
```

Do not infer its push/clean status from the Ubuntu checkpoint above.

### Current platform/UI state relevant to continuation

The earlier projection-insets/SystemUI bring-up is no longer the active coding task. Current runtime-validated platform work includes the projection display-profile reconcile, full multitouch, physical-key handling, and the later CarSystemUI vertical quick-control/system-bar recreation fixes. Horizontal notification/widget controls were also runtime-tested successfully before this checkpoint.

The boot-settle gate for the first AAP session is present in the current `vendor/arda/ardaplatform` checkpoint, but the real-HU cold-boot validation was intentionally deferred because it requires access to the vehicle. Do not treat that real-HU gate as closed yet.

### Active H.264 performance target

The active performance goal is:

```text
1920x1080
~30 real encoded fps
Raspberry Pi 4
hardware H.264: c2.v4l2.avc.encoder
minimum avoidable ARM/CPU conversion cost
```

Operational policy: an SDR profile advertising 60 fps does not force ARDA to intentionally overload the Pi 4 encoder at 1080p60. For the 1080p path, use a hardware-aware effective target of 30 fps and verify that the HU accepts the lower actual frame cadence. Do not claim protocol semantics beyond runtime evidence; validate on DHU/real HU.

Current `aaos-aap` source facts that matter:

```text
VideoConfigSelection.fps comes directly from SDR (30 or 60).
LiveVideoSender::configure_from_sdr currently assigns:
  self.config.fps = selection.fps

Production video backend is platform /system/bin/screenrecord.
The ARDA screenrecord command currently passes:
  --output-format=h264
  --size WIDTHxHEIGHT
  --bit-rate BITRATE
  --time-limit 0
  --codec-name c2.v4l2.avc.encoder

It does NOT pass a frame-rate argument.
AAP video PTS currently uses:
  pts_us = frame_index * 1_000_000 / self.config.fps
```

Therefore merely changing ARDA's PTS/config fps from 60 to 30 is not sufficient if platform `screenrecord` still configures MediaCodec from a 60 Hz display. The next implementation must control the actual encoder/capture cadence as well as AAP PTS.

### Next H.264 work order

1. Read-only audit the **actual Android 16 build-host `screenrecord` source** and current V4L2 Codec2 encoder configuration path.
2. Confirm exactly where `KEY_FRAME_RATE`, operating rate, repeat/refresh cadence, color-format conversion, and encoder surface are configured.
3. Measure/identify whether current 1080p path is attempting 60-fps encoder input when SDR advertises 60.
4. Implement the narrowest hardware-aware 1080p30 control. Prefer a vendor-owned/capture-owned solution; patch generic AOSP only if the actual source proves there is no clean vendor boundary.
5. Keep AAP PTS at the same effective fps as the actual produced cadence.
6. Runtime gate target: stable encoded output as close to 30 fps as practical (target >=29 fps), no software codec fallback, no thermal throttling, and no avoidable CPU RGBA->NV12 bottleneck.
7. Only after the 30-fps workload is real should GPU/dmabuf/NV12 conversion optimization be judged necessary.

Do not run `repo sync`. Do not commit/push H.264 changes until runtime behavior is approved.



---

## Source Appendix 4: `ARDA_V57_PHASE1_WIP_STATUS.md`


# ARDA AAOS / RPi4 H.264 1080p30 HANDOVER
Current checkpoint: 2026-08-19 ~22:20 TRT

## PRIMARY GOAL

Raspberry Pi 4 üzerindeki AAOS 16 sisteminde gerçek ScreenRecorder / virtual-display capture yolunu:

1920x1080
>=30 fps sustainable
H.264 hardware encode
no artificial frame cap
no GPU/CPU format conversion bottleneck
no SWIOTLB/scatterlist failures

seviyesine çıkarmak.

Durable image build ancak gerçek runtime >=30fps + stability PASS sonrasında yapılacak.


============================================================
HARD WORKFLOW CONSTRAINTS
============================================================

- NEVER run `repo sync`.
- Git commit/push ancak kullanıcı açıkça isterse.
- Generated helper scripts için SHA-256 yazma.
- ADB transport order:
    1. USB serial 1000000028c0b59c
    2. 192.168.1.185:5555
    3. 192.168.1.15:5555

  Bir transaction transport seçince root/remount/push/reboot/verify boyunca pinned kalmalı.

- Build host:
    edo@192.168.1.55

- SSH key:
    ~/.ssh/id_ed25519_arda_build

- AOSP:
    /home/edo/aosp-rpi-aaos16

- Lunch:
    aosp_rpi4_car-bp4a-userdebug

- Studio screen mirroring / Studio screenrecord test sırasında kapalı.
  HWC virtual display slotunu tüketebiliyor.

- Do not manipulate Mac terminal state with stty/tput/escape hacks.

- No durable image yet.

- Unrelated dirty files resetlenmemeli.

- Failed source experiment only restores its own touched files from its own backup.


============================================================
CLOSED ROOT CAUSES / DO NOT REOPEN RANDOMLY
============================================================

1. Original CPU FormatConverter
   ABGR → NV12 CPU conversion çok yavaştı.
   Bu primary production direction değil artık.

2. Direct HWC/ISP path works.

3. `/dev/video12` hardware ISP has ample isolated capacity.
   Native staging proof ~49 fps.

4. GPU/system DMA-BUF direct video12 import:
   fails due scatterlist/SWIOTLB.
   This route closed.

5. Reserved/CMA allocator path:
   GPU RGBA surface için contiguous reserved heap allocation proven.

6. Stock HWC virtual display buffer:
   HAL IMPLEMENTATION_DEFINED / XB24
   direct video12 DMABUF import PROVEN.
   No reserved-minigbm extension required for HWC.

7. HWC colors:
   XB24/BGR4 direct test visually correct after startup;
   no obvious red/blue swap.

8. C2 output NV12 block:
   directly importable to video12 capture DMABUF.
   Full-DMABUF path proven.

9. C2 buffer count:
   not bottleneck.
   8 blocks observed, no waits.
   Do NOT change kInputBufferCount 2→4.

10. FrameDropper / max-fps:
    explicit 60 vs omitted --frame-rate produced same ~58ms input cadence.
    Not primary bottleneck.

11. AIDL InputSurface files:
    instrumented implementation was not runtime owner.
    Dead path. Do not return there.

12. GraphicBufferSource:
    runtime implementation is DEPRECATED path.
    Original measurements:
      onFrame ~61ms
      free buffers 14–16
      blocked=0
      submit ~0.28ms
    GBS itself wasn't original bottleneck.

13. video11:
    typically mostly idle at original cadence.
    Not original 15–20fps cause.

14. SWIOTLB:
    Current HWC full-DMABUF runs have:
      SWIOTLB_RUNTIME_COUNT=0

15. CMA:
    no current runtime pressure/leak explaining FPS.


============================================================
CURRENT PRODUCTION-DIRECTION PIPELINE
============================================================

Screen / SurfaceFlinger
    ↓
HWC virtual display
    buffer:
      HAL format = IMPLEMENTATION_DEFINED
      DRM fourcc = XB24
      1920x1080
      stride=1920
    ↓ DMABUF
FormatConverter direct candidate
    accepts:
      RGBA_8888
      OR IMPLEMENTATION_DEFINED
    ↓
ArdaIspFullDmabufConverter
    video12 OUTPUT:
      BGR4 DMABUF
    ISP
    video12 CAPTURE:
      NV12 directly into existing C2 output DMABUF
    ↓
video11
    hardware H264
    ↓
ScreenRecorder H264

No CPU input copy.
No CPU output copy in full-DMABUF path.


============================================================
IMPORTANT HWC FACT
============================================================

HWC Virtual Display is structurally active when:

vendor.hwc.drm.enable_virtual_display=1

then:

service call SurfaceFlinger 1021 i32 1

dumpsys SurfaceFlinger should show:

name="ScreenRecorder"
isGpu=false

Current pacesetter:

renderRate=30.00 Hz

physical active mode:

vsyncRate=60.00 Hz


============================================================
LOW-LOG HWC VS GPU RESULT
============================================================

HWC:
  frames=470 / 24 s
  requested-window=19.583 fps
  wall=18.800

GPU:
  frames=382
  requested-window=15.917
  wall=15.134

HWC ~23% faster.

Therefore HWC is current production direction.


============================================================
SURFACEFLINGER PIPELINE COUNTER RESULT
============================================================

Initial low-overhead SF counter test showed approximately:

Scheduler signals:
  ~21.74/s

commit true:
  ~18.7/s

VDS queue:
  ~19.6/s

HWC present:
  ~0.945 ms average

VDS refresh:
  ~1.93 ms

VDS queueBuffer call:
  ~1.75 ms

Conclusion:
HWC present / VDS queue call itself was not blocking for tens of ms.


============================================================
MESSAGEQUEUE / REQUEST CADENCE TEST
============================================================

MessageQueue exact source established:

SurfaceFlinger is event-driven.

`scheduleFrame()` does NOT mean:
"run continuously at renderRate=30".

Instead it arms a next VSync callback.

MessageQueue also coalesces callbacks if mFramePending is already true.

Baseline counter result:

MQ scheduleFrame rate ≈ 21.019/s
MQ callback rate      ≈ 20.695/s
MQ accepted           ≈ 20.695/s
MQ handle             ≈ 20.695/s
coalesced             = 0

Scheduler signal      ≈ 20.690/s

Thus baseline:
frame request itself was only ~21Hz.
VSyncDispatch and main Looper were not dropping a 30Hz request stream.


============================================================
INTERNAL FORCE-COMPOSITE CONTROL
============================================================

Diagnostic patch in SurfaceFlinger.cpp:

property:
  debug.arda.force_composite_loop

When true, composite() schedules another:

  scheduleComposite(FrameHint::kNone);

This maintains an internal request stream instead of relying only on app/content transaction cadence.

Seed:
  service call SurfaceFlinger 1004

(1004 exact source = force composite ahead of next VSYNC)


============================================================
FORCE LOOP RESULT BEFORE VDS FORCE
============================================================

Force loop increased scheduling strongly:

MQ schedule:
  ~43.46/s

MQ callback:
  ~25.61/s

Scheduler signal:
  ~24.23/s

But:

VDS begin:
  570

original mustRecompose true:
  439

queueBuffer:
  439

cancel:
  131

H264:
  439 ~= 18.29 fps

This proved another gate:

LegacyVirtualDisplaySurface was cancelling frames when incoming
mustRecompose=false.

Therefore VDS dirty/must gate became a separate source of frame loss.


============================================================
FORCE VDS RECOMPOSE CONTROL
============================================================

Diagnostic property:

  debug.arda.force_vds_recompose

LegacyVirtualDisplaySurface::beginFrame:

for ScreenRecorder only:

  mMustRecompose =
      mustRecompose ||
      debug.arda.force_vds_recompose

This is diagnostic only.


============================================================
LATEST AUTHORITATIVE RUNTIME TEST
============================================================

Archive:
ARDA_H264_HWC_FORCE_VDS_RECOMPOSE_CONTROL_RESUME_V1_1_20260819_221241

Latest uploaded archive:
Archive(20260819-191840).zip

Build reused successful 21:48 SurfaceFlinger build.

USB transport:
1000000028c0b59c

Fresh reboot:
PASS

settle uptime:
180s

HWC:
PASS

C2 full-DMABUF:
PASS

isGpu:
false

SWIOTLB:
0


Exact counters:

MQ_SCHEDULE=1015
MQ_ARMED=366
MQ_CALLBACK=651
MQ_ACCEPTED=609
MQ_COALESCED=42
MQ_HANDLE=609
MQ_ELAPSED_US=24634702

MQ_SCHEDULE_RATE_FPS=41.202
MQ_CALLBACK_RATE_FPS=26.426
MQ_HANDLE_RATE_FPS=24.721

Scheduler:

PIPE_SIGNAL=609
PIPE_COMMIT_TRUE=556
PIPE_COMMIT_FALSE=53
PIPE_COMPOSITE=556
PIPE_ELAPSED_US=24631738

PIPE_SIGNAL_RATE_FPS=24.724

VDS:

VDS_BEGIN=545
VDS_MUST=360
VDS_COMMITTED=545
VDS_QUEUE_OK=545
VDS_QUEUE_ERROR=0
VDS_CANCEL=0

VDS_BEGIN_RATE_FPS=22.718
VDS_QUEUE_OK_RATE_FPS=22.718

H264:

FRAME_COUNT=543
FPS_REQUESTED_WINDOW=22.625
FPS_WALL=21.789

CAPTURE_RC=0
SF_VIRTUAL_IS_GPU=false
SWIOTLB_RUNTIME_COUNT=0


============================================================
CRITICAL ANALYZER CORRECTION
============================================================

The test analyzer printed:

FORCE_VDS_RECOMPOSE_EFFECTIVE_GATE=FAIL
FORCE_VDS_CONTROL_STRUCTURAL_GATE=FAIL

THIS IS A FALSE NEGATIVE.

Reason:

The existing VDS instrumentation increments its `must` counter based on the
original `mustRecompose` argument BEFORE the new diagnostic force is applied.

Therefore:

VDS_MUST=360

does NOT represent final:

mMustRecompose = mustRecompose || ardaForceRecompose

The actual force-recompose effectiveness is proven by:

VDS_BEGIN      = 545
VDS_QUEUE_OK   = 545
VDS_CANCEL     = 0

Hence every VDS begin frame reached queueBuffer.

H264=543 is also essentially 1:1 with VDS queue=545.

Correct classification:

FORCE_VDS_RECOMPOSE=PASS

VDS dirty/mustRecompose filtering is no longer the current FPS bottleneck.


============================================================
CURRENT FIRST MAJOR DROP
============================================================

With both controls enabled:

MQ callbacks:
  651

accepted/handled:
  609

Scheduler signals:
  609

commit true:
  556

commit false:
  53

VDS begin:
  545

VDS queue:
  545

H264:
  543


The largest currently actionable explicit frame rejection is:

Scheduler signal
  609
      ↓
commit()
  true  = 556
  false = 53


VDS after successful commit is effectively clean:

545 begin
545 queue
0 cancel

Encoder tracks VDS almost 1:1:

545 queue
543 H264


============================================================
NEXT EXPERIMENT
============================================================

Prepared script:

ARDA_H264_SF_COMMIT_FALSE_REASON_COUNTER_V1_20260819.sh

Purpose:

Instrument exact SurfaceFlinger::commit() false exits while running the
same:

- internal force-composite control
- HWC ScreenRecorder
- force VDS recompose
- full-DMABUF C2 path

Reasons counted:

PACESETTER_REMOVED
MODESET_FRAME_PENDING
MODESET_FINALIZE_PENDING
HWC_BACKPRESSURE
NO_MUST_COMPOSITE
BOOT_STAGE


Primary decision:

If most commit=false are:

HWC_BACKPRESSURE
→ physical pacesetter/frame-target backpressure is the next target.
→ likely investigate 30Hz renderRate vs 60Hz hardware VSync,
  or decouple capture VDS from physical-display backpressure.

If most are:

NO_MUST_COMPOSITE
→ duplicate/extra scheduled callbacks are arriving without a true composite need.
→ redesign force loop as one outstanding self-schedule request,
  rather than over-requesting.

If modeset reasons:
→ stabilize display mode handling first.

Do NOT optimize encoder/C2 again before this commit-false result.


============================================================
CURRENT SOURCE STATE
============================================================

frameworks/native current dirty files:

 M services/surfaceflinger/CompositionEngine/src/Display.cpp
 M services/surfaceflinger/DisplayHardware/VirtualDisplay/LegacyVirtualDisplaySurface.cpp
 M services/surfaceflinger/Scheduler/MessageQueue.cpp
 M services/surfaceflinger/Scheduler/Scheduler.cpp
 M services/surfaceflinger/SurfaceFlinger.cpp

These collectively contain:

- ARDA_SF_PIPE_TL_V1 counters
- ARDA_MQ_REQ_TL_V1 counters
- ARDA_FORCE_COMPOSITE_CONTROL_V1
- debug.arda.force_composite_loop
- debug.arda.force_vds_recompose
- VDS force-recompose diagnostic

Latest relevant backup before force-VDS modification:

/home/edo/aosp-rpi-aaos16/arda-source-backups/
20260819_214822-before-force-vds-recompose-v1

Do not broad-reset these files.


============================================================
OTHER EXISTING DIRTY TREES TO PRESERVE
============================================================

frameworks/av likely still contains:

 M cmds/screenrecord/screenrecord.cpp
 M media/module/bqhelper/flagged_files/DeprecatedGraphicBufferSource.inc
 M media/module/bqhelper/flagged_files/GraphicBufferSource.inc

external/v4l2_codec2 contains existing project changes including:

common/FormatConverter.cpp
components/V4L2EncodeComponent.cpp
components/V4L2EncodeInterface.cpp
components/V4L2Encoder.cpp

FormatConverter includes:
- full-DMABUF ISP path
- HWC IMPLEMENTATION_DEFINED direct candidate
- ARDA_HWC_INPUT_DMABUF_PROBE_V1

external/minigbm also contains reserved allocator work.

Do not reset unrelated work.


============================================================
RUNTIME CURRENT STATE AFTER TEST
============================================================

Cleanup executed:

debug.arda.force_vds_recompose=0
debug.arda.force_composite_loop=0

HWC virtual display runtime switch restored/disabled.

vendor.hwc.drm.enable_virtual_display restored to previous value.

Runtime deployed SurfaceFlinger still contains diagnostic code.

C2 APEX still contains current:
- HWC direct import
- full-DMABUF
- prior C2 instrumentation

Fresh reboot may recreate stale minigbm allocator bind; every test must drain it
after reboot before capture.


============================================================
PERFORMANCE INTERPRETATION
============================================================

Current 22.6fps is NOT proof that hardware encoder capacity is 22.6fps.

Known isolated ISP pipeline capacity was ~49fps.

Current forced VDS result shows:

VDS queue:
545

H264:
543

Thus encoder is consuming almost every frame it is given at this current input
rate.

Current bottleneck remains upstream in SurfaceFlinger scheduling / commit pacing.

The latest run materially improved real output:

baseline HWC low-log:
~19.58fps

forced VDS/current:
~22.63fps

and eliminated VDS cancel/drop entirely.


============================================================
IMMEDIATE EXECUTION
============================================================

Run:

cd ~/Downloads

chmod +x ARDA_H264_SF_COMMIT_FALSE_REASON_COUNTER_V1_20260819.sh

/bin/bash -n ARDA_H264_SF_COMMIT_FALSE_REASON_COUNTER_V1_20260819.sh \
  || exit 1

/bin/bash ARDA_H264_SF_COMMIT_FALSE_REASON_COUNTER_V1_20260819.sh

Studio mirroring must remain OFF.

When result ZIP is produced:
inspect it automatically.

First inspect:

test/analysis.txt
test/focused-logcat.txt
test/logcat-live.txt
test/sf-case.txt
run.log
source/status-final.txt

Validate reason accounting independently:

sum(commit-false reason counts)
must equal or explain PIPE_COMMIT_FALSE.

Do not trust automatic classification blindly.

Then continue only at the first proven rate/drop stage.



---

## Source Appendix 5: `COMBINED_GPS_MULTITOUCH_FOCUS_CHANGELOG_20260805.md`


# Combined GPS Persistence, Full Multitouch, and Video Focus

Date: 2026-08-05

This canonical source snapshot combines:

- persistent AAP SENSOR_LOCATION forwarding to Android `arda_gps`;
- line-v2 generic multipointer transport (up to 16 pointers);
- snapshot-to-Android-MotionEvent reconciliation in ArdaInputBridge;
- video-focus handling that treats focus as display ownership rather than
  connection ownership.

Video focus behavior:

- NATIVE focus stops the H.264 capture/output path only;
- the AAP/TLS session and already-open video channel remain alive;
- host-liveness timeout is suspended while projection output is hidden;
- PROJECTED focus re-arms capture, media start, codec config, and a fresh IDR
  on the existing session/channel.

Validation status at this checkpoint:

- GPS persistence: runtime PASS.
- Full multitouch: runtime PASS; Maps pinch zoom in/out works.
- Video focus: integrated candidate; final real-HU validation remains pending.



---

## Source Appendix 6: `FULL_MULTITOUCH_CHANGELOG_20260804.md`


# ARDA Full Multitouch — 2026-08-04

This snapshot extends the proven privileged InputBridge path from the legacy
single-pointer line protocol to a generic multipointer protocol.

## Protocol

```text
DISPLAY <display_id>
TM <ACTION> <action_index> <count> <pointer_id> <x> <y> ...
K <android_keycode>
STOP
```

Supported actions:

```text
DOWN
POINTER_DOWN
MOVE
POINTER_UP
UP
CANCEL
```

The legacy `T DOWN|MOVE|UP ...` command remains accepted for compatibility.

## Limits and validation

- Up to 16 simultaneous pointers.
- Android pointer ids must be unique and in the range 0..31.
- `action_index` is preserved for `POINTER_DOWN` and `POINTER_UP`.
- Malformed transitions force `ACTION_CANCEL` so no stale touch remains.
- Disconnect and display change also cancel an active gesture.

## Scope

- Rust AAP input forwarding now preserves the complete `pointer_data` list.
- Every pointer is coordinate-mapped independently.
- The privileged ArdaInputBridge creates real multipointer `MotionEvent`s.
- The embedded app_process InputServer implements the same line-v2 protocol.
- GPS persistence remains unchanged.
- No focus candidate is included.
- No VHAL or Launcher source is changed.



---

## Source Appendix 7: `FULL_MULTITOUCH_RECONCILER_CHANGELOG_20260805.md`


# Full Multitouch Snapshot Reconciler v1 — 2026-08-05

## Root cause

The line-v2 transport was carrying valid multi-pointer snapshots such as:

```text
TM MOVE 0 2 0 388 279 1 263 448
```

The platform bridge still enforced a strict Android transition order. DHU/AAP can
emit a MOVE that is the first frame containing an additional pointer, and can
emit final releases as `UP(count > 1)` followed by `POINTER_UP`. The strict
validator rejected those snapshots, cancelled the active gesture, and every
following MOVE failed.

## Fix

The platform bridge now treats each AAP frame as a complete pointer snapshot and
reconciles it into a valid Android MotionEvent sequence:

- Missing DOWN is synthesized.
- Missing POINTER_DOWN events are synthesized for every newly appearing ID.
- Missing POINTER_UP events are synthesized for disappearing IDs.
- `UP(count > 1)` is normalized to POINTER_UP.
- A final POINTER_UP is normalized to ACTION_UP.
- Pointer IDs and all coordinates are preserved.
- Up to 16 pointers remain supported.
- Stale pointers present only in release snapshots are not re-added.
- Unrecoverable mismatches cancel and bootstrap a clean gesture.

Only `ArdaInputBridge` changes. `arda-connectivityd` and the GPS configuration
remain untouched by the build/install package.



---

## Source Appendix 8: `GPS_PERSISTENCE_CHANGELOG_20260804.md`


# ARDA DHU GPS persistence — 2026-08-04

Based on `aaosaap04.zip`.

Changes:

- `src/config.rs` accepts and resolves:
  - `sensor.period_ms`
  - `sensor.forward_os`
  - `sensor.location_provider`
  - `sensor.location_min_ms`
- `sensor.forward_os` resolves to the location-only CLI flags, so it does not
  implicitly enable VHAL forwarding.
- Root and packaged `arda-aap.toml` profiles now persist the live-tested
  location-only settings:
  - sensor channel enabled
  - request only `SENSOR_LOCATION` (type 1)
  - 200 ms request/minimum forwarding interval
  - forward to `arda_gps`
  - VHAL forwarding disabled
  - JSONL sensor dump disabled

No `repo sync`, commit, or push is performed by the deployment package.
