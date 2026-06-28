# ARDA plan

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
