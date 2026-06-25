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
