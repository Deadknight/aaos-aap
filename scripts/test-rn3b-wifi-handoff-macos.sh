#!/usr/bin/env bash
set -euo pipefail

DEVICE="${DEVICE:-192.168.1.21:5555}"
ADB="${ADB:-adb}"
LOCAL_PORT="${LOCAL_PORT:-15283}"
BRIDGE_TOKEN="${BRIDGE_TOKEN:-Arda-RN3B-Debug-Only-2026!}"
SSID="${SSID:?set SSID to the phone hotspot name}"
PSK="${PSK:-}"
SECURITY="${SECURITY:-wpa2}"
WAIT_MS="${WAIT_MS:-30000}"
MODE="${MODE:-phone_hotspot}"
PHONE_IP="${PHONE_IP:-}"
HTTP_PROXY_PORT="${HTTP_PROXY_PORT:-}"

"$ADB" connect "$DEVICE" >/dev/null
"$ADB" -s "$DEVICE" forward --remove "tcp:$LOCAL_PORT" >/dev/null 2>&1 || true
"$ADB" -s "$DEVICE" forward "tcp:$LOCAL_PORT" tcp:5283 >/dev/null

BRIDGE_TOKEN="$BRIDGE_TOKEN" \
SSID="$SSID" \
PSK="$PSK" \
SECURITY="$SECURITY" \
WAIT_MS="$WAIT_MS" \
MODE="$MODE" \
PHONE_IP="$PHONE_IP" \
HTTP_PROXY_PORT="$HTTP_PROXY_PORT" \
LOCAL_PORT="$LOCAL_PORT" \
python3 <<'PY'
import json
import os
import socket
import struct

TOKEN = os.environ["BRIDGE_TOKEN"].encode()
PORT = int(os.environ["LOCAL_PORT"])


def read_exact(sock: socket.socket, size: int) -> bytes:
    data = bytearray()
    while len(data) < size:
        chunk = sock.recv(size - len(data))
        if not chunk:
            raise RuntimeError("socket closed early")
        data.extend(chunk)
    return bytes(data)


def send_frame(sock: socket.socket, op: int, request_id: int, payload: dict) -> None:
    raw = json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode()
    body = bytes([1, op]) + struct.pack("<Q", request_id) + raw
    sock.sendall(struct.pack("<I", len(body)) + body)


def read_frame(sock: socket.socket):
    size = struct.unpack("<I", read_exact(sock, 4))[0]
    body = read_exact(sock, size)
    return body[0], body[1], struct.unpack("<Q", body[2:10])[0], json.loads(body[10:])


offer = {
    "mode": os.environ["MODE"],
    "ssid": os.environ["SSID"],
    "security": os.environ["SECURITY"],
    "wait_ms": int(os.environ["WAIT_MS"]),
}
if os.environ.get("PSK"):
    offer["psk"] = os.environ["PSK"]
if os.environ.get("PHONE_IP"):
    offer["phone_ip"] = os.environ["PHONE_IP"]
if os.environ.get("HTTP_PROXY_PORT"):
    offer["http_proxy_port"] = int(os.environ["HTTP_PROXY_PORT"])

with socket.create_connection(("127.0.0.1", PORT), timeout=10) as sock:
    sock.settimeout(90)
    sock.sendall(b"ARDBR001" + struct.pack("<H", len(TOKEN)) + TOKEN)
    auth = read_exact(sock, 4)
    print("AUTH=", auth.decode(errors="replace"))
    if auth != b"OKAY":
        raise RuntimeError("bridge auth failed")

    send_frame(sock, 1, 1, {"role": "phone_companion", "client": "mac_wifi_handoff_smoke"})
    print("HELLO_ACK=", read_frame(sock))

    send_frame(sock, 10, 2, offer)
    result = read_frame(sock)
    print("INTERNET_RESULT=", result)
    if result[1] != 11 or result[3].get("status") != "ready":
        raise RuntimeError(f"internet offer failed: {result}")

print("ARDA_RN3B_WIFI_OFFER_GATE=PASS")
PY
