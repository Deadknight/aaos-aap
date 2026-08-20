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