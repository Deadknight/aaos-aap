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
