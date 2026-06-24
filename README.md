# arda-senderd standalone

Standalone OS-level Android Auto sender experiment for the Arda setup.

This repo is intentionally clean-room/scratch:

- no dependency on the existing `aa-proxy` / `aap` repo
- existing AAP code can guide protocol behavior later
- first goal is reliable OS-level capture from AAOS
- later goals are AAP video channel, input injection, and USB transport

## Current MVP

`arda-senderd capture-test` starts Android's `screenrecord` as a child process:

```text
screenrecord --output-format=h264 --size 1280x720 --bit-rate 6000000 --time-limit 180 -
```

On the current Snapp AAOS/RPi4 image this outputs raw Annex-B H264/NAL data to stdout.
`arda-senderd` parses the NAL stream and writes it back to a file or stdout.

## Build with Docker

From this repo root:

```bash
./scripts/docker-build.sh
```

Output:

```text
target/aarch64-linux-android/release/arda-senderd
```

Manual Docker commands:

```bash
docker build -t arda-android-builder -f Dockerfile.android .
docker run --rm -v "$PWD":/work arda-android-builder
```

## Push to AAOS

```bash
adb connect 192.168.1.15:5555
adb push target/aarch64-linux-android/release/arda-senderd /data/local/tmp/arda-senderd
adb shell chmod 755 /data/local/tmp/arda-senderd
```

## Record to file on AAOS

```bash
adb shell /data/local/tmp/arda-senderd capture-test \
  --size 1280x720 \
  --bit-rate 6000000 \
  --seconds 10 \
  --out /sdcard/arda.h264

adb pull /sdcard/arda.h264 ./arda.h264
mpv --demuxer=lavf --demuxer-lavf-format=h264 arda.h264
```

## Live preview on Mac with mpv

```bash
adb exec-out /data/local/tmp/arda-senderd capture-test \
  --size 1280x720 \
  --bit-rate 6000000 \
  --seconds 180 \
  --out - \
  | mpv --demuxer=lavf --demuxer-lavf-format=h264 --profile=low-latency --untimed -
```

or use the helper script:

```bash
./scripts/push-and-live-mpv.sh
```

## Inspect a raw H264 stream

On Mac/Linux after building locally, or on Android if the binary is present:

```bash
./target/aarch64-linux-android/release/arda-senderd nal-info test.h264 --limit 80
```

Expected NAL types:

```text
Sps
Pps
SliceIdr
SliceNonIdr
```

## Next steps

1. Add `/dev/uinput` touch/key device.
2. Add AAP transport/session skeleton.
3. Feed parsed H264 access units into the AA video channel.
4. Restart `screenrecord` automatically before/after the 180 second limit.
5. Add USB gadget/accessory transport after TCP/DHU works.

### Apple Silicon / Docker Desktop note

The Docker build is forced to `linux/amd64` because the Android NDK linux package contains `linux-x86_64` clang toolchains. On M-series Macs, running the image as `linux/arm64` can fail during link with:

```text
rosetta error: failed to open elf at /lib64/ld-linux-x86-64.so.2
```

Use:

```bash
./scripts/docker-build.sh
```

The script runs both `docker buildx build` and `docker run` with `--platform linux/amd64`.


## v3 note: binary-clean stdout

`capture-test --out -` writes only raw H264 bytes to stdout. Diagnostics are suppressed in stdout mode because `adb exec-out` can pollute the host pipe if remote stderr is merged. Use file output when you want NAL stats/logs.

## v4 note

`capture-test --out -` now runs `screenrecord` in direct stdout pass-through mode.
This keeps `adb exec-out` binary-clean and avoids the 0-byte live pipe seen in v3.
`screenrecord` stderr is written to `/data/local/tmp/arda-screenrecord.stderr.log`.

## Embedded native C++ probe

This build now links a small C++ object directly into `arda-senderd` through `build.rs`.
It does **not** replace `screenrecord` yet; it probes whether the AAOS image exposes the native pieces needed for the later in-process capture path.

Build is still the same command:

```bash
./scripts/docker-build.sh
```

Run on AAOS:

```bash
adb push target/aarch64-linux-android/release/arda-senderd /data/local/tmp/arda-senderd
adb shell chmod 755 /data/local/tmp/arda-senderd
adb shell /data/local/tmp/arda-senderd native-probe
```

Good signs for the next native capture patch:

```text
libmediandk.so OK
AMediaCodec_createEncoderByType OK
AMediaCodec_createInputSurface OK
libgui.so OK
libbinder.so OK
libui.so OK
```

## Native encoder smoke test

`native-encoder-test` is the first embedded C++ MediaCodec test. It does not capture the AAOS screen yet; it feeds dummy black YUV420 frames into Android's H264 encoder and writes the encoded output. This separates "does native AMediaCodec H264 output work?" from the later SurfaceFlinger/display-capture work.

```bash
adb shell /data/local/tmp/arda-senderd native-encoder-test \
  --size 320x240 \
  --bit-rate 1000000 \
  --fps 15 \
  --seconds 3 \
  --out /sdcard/arda-native-encoder-test.h264

adb pull /sdcard/arda-native-encoder-test.h264 ./arda-native-encoder-test.h264
file arda-native-encoder-test.h264
xxd -l 64 arda-native-encoder-test.h264
```

## V7 native surface encoder smoke test

After `native-encoder-test` passes, run the input-surface path. This does not capture the AAOS screen yet; it creates an AMediaCodec H264 encoder input surface, attaches EGL/GLES to it, draws changing color frames, and writes the encoded H264 stream.

```bash
adb push target/aarch64-linux-android/release/arda-senderd /data/local/tmp/arda-senderd
adb shell chmod 755 /data/local/tmp/arda-senderd

adb shell /data/local/tmp/arda-senderd native-surface-encoder-test \
  --size 320x240 \
  --bit-rate 1000000 \
  --fps 15 \
  --seconds 3 \
  --out /sdcard/arda-native-surface-encoder-test.h264

adb pull /sdcard/arda-native-surface-encoder-test.h264 ./arda-native-surface-encoder-test.h264
file arda-native-surface-encoder-test.h264
xxd -l 64 arda-native-surface-encoder-test.h264
adb shell /data/local/tmp/arda-senderd nal-info /sdcard/arda-native-surface-encoder-test.h264
```

Expected key lines: `create input surface: OK`, `eglCreateWindowSurface: OK`, `eglSwapBuffers` without errors, `encoded_bytes > 0`, and NAL output containing SPS/PPS/IDR.


## V8 native display probe

Adds `native-display-probe`, a safe symbol/ABI check for Android 11 `libgui` display mirroring pieces: `SurfaceComposerClient::createDisplay`, `Transaction::setDisplaySurface`, `setDisplayProjection`, `setDisplayLayerStack`, `apply`, `getInternalDisplayToken`, and fallback `ScreenshotClient::capture` symbols. It does not yet call private constructors or route the screen into the encoder surface.

## V9 native-display-active-probe

`native-display-active-probe` calls the smallest private SurfaceComposerClient path before trying full display capture:

- constructs `android::String8`
- calls `SurfaceComposerClient::getInternalDisplayToken()`
- calls `SurfaceComposerClient::createDisplay()`
- calls `SurfaceComposerClient::destroyDisplay()`

This is an active ABI test. It does not route the display into the encoder surface yet.


## v10c

Adds an explicit v10c marker to native-display-capture-test logs and keeps the ANativeWindow -> Surface pointer adjustment before getIGraphicBufferProducer.


## V11 native display live stdout

`native-display-capture-test` now supports `--out -` for binary-clean stdout. In this mode Rust does not print the native report to stdout; diagnostics stay in `/data/local/tmp/arda-display-capture-test.log`.

Live native display stream:

```bash
adb exec-out /data/local/tmp/arda-senderd native-display-capture-test   --size 1280x720   --bit-rate 6000000   --fps 30   --seconds 180   --out -   | mpv --demuxer=lavf         --demuxer-lavf-format=h264         --no-correct-pts         --container-fps-override=30         --profile=low-latency -
```

If the live pipe fails, read:

```bash
adb shell cat /data/local/tmp/arda-display-capture-test.log
```
