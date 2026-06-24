# Arda standalone plan

## Why standalone?

The existing AAP project should guide protocol sequencing, protobuf fields, and channel timing, but this project should stay minimal and controlled:

- one Rust daemon
- no Android app helper
- no Kotlin/Java dependency
- root/OS-level capture and input
- Docker build for Android aarch64

## Architecture

```text
arda-senderd
  capture/
    screenrecord_source -> raw H264 stdout
  h264/
    annexb parser -> SPS/PPS/IDR/non-IDR
  input/
    uinput touch/key injector later
  aap/
    TCP/DHU transport later
    version negotiation later
    service discovery later
    video channel later
```

## Current confirmed facts

- `screenrecord --output-format=h264 ... -` can be viewed live via `adb exec-out | mpv`.
- The stream is raw H264 / JVT NAL sequence, not MP4 container.
- `/dev/uinput` exists.
- USB mouse appeared as kernel input events, so the input stack is alive.
- `/config/usb_gadget` and `/sys/class/udc` are currently empty; USB gadget comes later.


## V7 step

Add `native-surface-encoder-test`: AMediaCodec input-surface + EGL/GLES draw path. This verifies that a Surface-backed encoder can produce Annex-B H264 without `screenrecord`. It still draws synthetic frames; SurfaceFlinger/display capture is the next missing piece.


## V8 native display probe

Adds `native-display-probe`, a safe symbol/ABI check for Android 11 `libgui` display mirroring pieces: `SurfaceComposerClient::createDisplay`, `Transaction::setDisplaySurface`, `setDisplayProjection`, `setDisplayLayerStack`, `apply`, `getInternalDisplayToken`, and fallback `ScreenshotClient::capture` symbols. It does not yet call private constructors or route the screen into the encoder surface.

## V9 checkpoint

Native display active probe added. This validates whether the private `sp<IBinder>` and `String8` ABI calls are safe before attempting `Transaction::setDisplaySurface`.


## v10c-surface-offset

If native-display-capture-test logs do not show the v10c marker or `Surface this adjustment`, the patched native file did not make it into the built binary.


## V11

- Added binary-clean stdout support for `native-display-capture-test --out -`.
- This allows live native display capture without `screenrecord`:
  `SurfaceComposer virtual display -> MediaCodec input surface -> H264 stdout -> mpv`.
- Native reports are suppressed from stdout in live mode; use `/data/local/tmp/arda-display-capture-test.log` for diagnostics.


## v12 input test

Adds `input-test` using `/dev/uinput` temporary virtual direct touchscreen. Test with root:

```bash
adb shell su 0 /data/local/tmp/arda-senderd input-test --size 1280x720 tap 300 300
adb shell su 0 /data/local/tmp/arda-senderd input-test --size 1280x720 swipe 200 500 900 500 --duration-ms 300 --steps 20
```

### v12b input parser fix

- `input-test` now accepts `--duration-ms` and `--steps` either before or after the action coordinates.
- `swipe` default duration is now 300 ms; `tap` default remains 80 ms.

## v13 - Native capture command

- Added `native-capture` as a production-style alias around the working native display capture pipeline.
- `native-capture --out -` is binary-clean raw Annex-B H264 for `adb exec-out | mpv`.
- `native-capture --seconds 0` means run until the process is killed; this removes the old finite-test-only behavior for live work.
- Transaction setter logging now treats setters as returning `Transaction&`/pointer-like values instead of fake `status_t` values.
