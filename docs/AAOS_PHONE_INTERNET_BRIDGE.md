# ARDA AAOS phone internet bridge

Goal: get the Raspberry Pi 4 AAOS image online in the car through the driver's Android phone.

This document separates the bootstrap channel from the data path:

- Bootstrap/control: Bluetooth RFCOMM or an existing companion channel can exchange SSID, password, phone IP, proxy port and mode.
- Data path: Wi-Fi should carry the actual traffic. Bluetooth should not carry bulk internet traffic.

## Mode A: phone normal hotspot / tethering, AAOS joins Wi-Fi

This is the MVP and the most likely to make Maps/GMS work without special routing.

Phone:

1. Enable mobile data.
2. Enable Personal Hotspot / Mobile Hotspot.
3. Use a stable SSID/password, for example `ARDA-Phone`.

AAOS:

```bash
adb shell /data/local/tmp/arda-senderd net-join \
  --ssid ARDA-Phone \
  --psk 'change-me-1234' \
  --security wpa2

adb shell /data/local/tmp/arda-senderd net-status
```

Disconnect/cleanup:

```bash
adb shell /data/local/tmp/arda-senderd net-disconnect --ssid ARDA-Phone
```

Notes:

- This uses Android's `cmd wifi connect-network` shell command.
- The companion app can later send an `internet_offer` over Bluetooth containing the same SSID/PSK so AAOS can run `net-join` without manual typing.

Suggested future BT frame:

```json
{
  "op": "internet_offer",
  "mode": "phone_hotspot",
  "ssid": "ARDA-Phone",
  "psk": "change-me-1234",
  "security": "wpa2"
}
```

## Mode B: phone LocalOnlyHotspot + phone proxy

Use this if the phone app can create a local Wi-Fi link but cannot enable normal carrier tethering automatically.

Phone companion app:

1. Starts LocalOnlyHotspot.
2. Starts HTTP or SOCKS proxy.
3. Binds outbound proxy sockets to the cellular network with Android `ConnectivityManager`, so traffic from AAOS exits via mobile data.
4. Sends SSID/PSK/phone IP/proxy port to AAOS over Bluetooth.

AAOS first-stage HTTP proxy test:

```bash
adb shell /data/local/tmp/arda-senderd net-join \
  --ssid ARDA-Link \
  --psk 'from-companion' \
  --security wpa2

adb shell /data/local/tmp/arda-senderd net-proxy --host 192.168.43.1 --port 8080
adb shell /data/local/tmp/arda-senderd net-status
```

Cleanup:

```bash
adb shell /data/local/tmp/arda-senderd net-proxy-clear
adb shell /data/local/tmp/arda-senderd net-disconnect --ssid ARDA-Link
```

Limit:

- Android global HTTP proxy is only a partial bridge. Apps that ignore Android proxy settings, raw sockets, and some DNS paths may not work.
- For full-device internet, add a small tun2socks binary on AAOS and route default traffic through the phone SOCKS proxy.

Suggested future BT frame:

```json
{
  "op": "internet_offer",
  "mode": "local_hotspot_http_proxy",
  "ssid": "ARDA-Link",
  "psk": "from-companion",
  "security": "wpa2",
  "phone_ip": "192.168.43.1",
  "http_proxy_port": 8080
}
```

Future full-device mode:

```json
{
  "op": "internet_offer",
  "mode": "local_hotspot_socks_tun",
  "ssid": "ARDA-Link",
  "psk": "from-companion",
  "security": "wpa2",
  "phone_ip": "192.168.43.1",
  "socks_port": 1080,
  "dns_mode": "proxy"
}
```

## Mode C: AAOS SoftAP + phone joins AAOS + phone proxy

This is the reverse direction the project considered: AAOS creates the Wi-Fi network, phone joins it, then the phone companion app proxies AAOS traffic out through cellular.

This can be useful when AAOS SoftAP works reliably and phone hotspot automation is blocked.

AAOS experimental helper, using the Hotspot settings UI config. On the tested Raspberry Pi AAOS image this is disabled by default because hostapd/HAL failure caused Wi-Fi config state to poison boot. Only retry with ADB rescue ready:

```bash
adb shell /data/local/tmp/arda-senderd net-ap-start --force-unsafe-softap --use-saved-config --wait-ms 8000
```

Custom SSID/password helper, if the image honors shell SoftAP config:

```bash
adb shell /data/local/tmp/arda-senderd net-ap-start --force-unsafe-softap --ssid ARDA-AAOS --psk arda-aaos-1234 --security wpa2 --wait-ms 8000
```

Phone companion app:

1. Joins `ARDA-AAOS`.
2. Keeps mobile data available.
3. Starts HTTP/SOCKS proxy bound/listening on the Wi-Fi interface.
4. Binds outbound proxy sockets to cellular.
5. Sends `phone_ip` and proxy port back over Bluetooth or the Wi-Fi control socket.

AAOS HTTP proxy step:

```bash
adb shell /data/local/tmp/arda-senderd net-proxy --host PHONE_WIFI_IP --port 8080
```

Stop:

```bash
adb shell /data/local/tmp/arda-senderd net-ap-stop
adb shell /data/local/tmp/arda-senderd net-proxy-clear
```

Recovery helper if Android starts looping after a failed SoftAP attempt:

```bash
adb wait-for-device
adb root
adb wait-for-device
adb shell /data/local/tmp/arda-senderd net-wifi-rescue
adb reboot
```

Caveats:

- `net-ap-start` is experimental and disabled by default on the tested image. It requires `--force-unsafe-softap`.
- Do not auto-start AAOS SoftAP from init until hostapd/HAL support is known-good.
- Prefer `--use-saved-config` when the Settings UI already shows a valid Hotspot name/password. It starts tethering through Connectivity using the saved Settings config.
- Custom `cmd wifi start-softap <ssid> ...` may return success even if the Settings UI does not show the AP as enabled; use `net-status` diagnostics and a phone scan to verify.
- AAOS SoftAP requires framework, HAL, firmware and regulatory support.
- Starting a SoftAP may disconnect AAOS from any existing Wi-Fi network.
- This still needs phone-side proxy work; AAOS SoftAP alone does not give AAOS internet.

## Recommended implementation order

1. Test `net-status` on AAOS to see exact Wi-Fi shell support.
2. Test Mode A manually with normal phone hotspot.
3. Add companion app UI/button: "Offer internet" over BT with SSID/PSK.
4. Add Mode B HTTP proxy in companion app.
5. Add tun2socks full-device mode if Maps/GMS ignores HTTP proxy.
6. Test Mode C only if phone hotspot/local-only mode is blocked or inconvenient.

## Implemented RN-3B transport handoff

The senderd RN-3B state machine now accepts `INTERNET_OFFER` as operation 10 after `HELLO`. For `phone_hotspot`, senderd:

1. Enables AAOS Wi-Fi.
2. Connects to the exact offered SSID with `cmd wifi connect-network`.
3. Requires the target SSID to be reported connected and `wlan0` to have IPv4.
4. Returns `INTERNET_RESULT` operation 11 with the AAOS Wi-Fi IP and port 5284.
5. Keeps RFCOMM open as the fallback/bootstrap channel.
6. Accepts a new token-authenticated direct Wi-Fi session on port 5284.

The companion should then open a TCP connection to the returned `aaos_ip:port`, perform the existing `ARDBR001` authentication, and send `HELLO`/`PING` again. Only after the Wi-Fi `PONG` gate passes should it move later bulk traffic off RFCOMM.

`local_hotspot_http_proxy` is also accepted. Its payload additionally requires:

```json
{
  "phone_ip": "192.168.43.1",
  "http_proxy_port": 8080
}
```

The global Android HTTP proxy remains a partial internet bridge; it is not a substitute for future tun2socks full-device routing.
