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
