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
