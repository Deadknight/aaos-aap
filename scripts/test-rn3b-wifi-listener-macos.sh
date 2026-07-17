#!/usr/bin/env bash
set -euo pipefail

AAOS_HOST="${AAOS_HOST:-192.168.1.21}"
WIFI_PORT="${WIFI_PORT:-5284}"
BRIDGE_TOKEN="${BRIDGE_TOKEN:-Arda-RN3B-Debug-Only-2026!}"

AAOS_HOST="$AAOS_HOST" \
WIFI_PORT="$WIFI_PORT" \
BRIDGE_TOKEN="$BRIDGE_TOKEN" \
python3 <<'PY'
import json
import os
import socket
import struct

HOST = os.environ["AAOS_HOST"]
PORT = int(os.environ["WIFI_PORT"])
TOKEN = os.environ["BRIDGE_TOKEN"].encode()


def read_exact(sock: socket.socket, size: int) -> bytes:
    data = bytearray()
    while len(data) < size:
        chunk = sock.recv(size - len(data))
        if not chunk:
            raise RuntimeError("socket closed early")
        data.extend(chunk)
    return bytes(data)


def send_frame(sock: socket.socket, op: int, request_id: int, payload: dict) -> None:
    raw = json.dumps(payload, separators=(",", ":")).encode()
    body = bytes([1, op]) + struct.pack("<Q", request_id) + raw
    sock.sendall(struct.pack("<I", len(body)) + body)


def read_frame(sock: socket.socket):
    size = struct.unpack("<I", read_exact(sock, 4))[0]
    body = read_exact(sock, size)
    return body[0], body[1], struct.unpack("<Q", body[2:10])[0], json.loads(body[10:])


with socket.create_connection((HOST, PORT), timeout=10) as sock:
    sock.settimeout(15)
    sock.sendall(b"ARDBR001" + struct.pack("<H", len(TOKEN)) + TOKEN)
    auth = read_exact(sock, 4)
    print("AUTH=", auth.decode(errors="replace"))
    if auth != b"OKAY":
        raise RuntimeError("Wi-Fi listener auth failed")

    send_frame(sock, 1, 1, {"role": "phone_companion", "client": "mac_wifi_direct_smoke"})
    hello = read_frame(sock)
    print("HELLO_ACK=", hello)
    if hello[1] != 2:
        raise RuntimeError(f"unexpected HELLO response: {hello}")

    send_frame(sock, 3, 2, {"ping": True})
    pong = read_frame(sock)
    print("PONG=", pong)
    if pong[1] != 4:
        raise RuntimeError(f"unexpected PING response: {pong}")

print("ARDA_RN3B_WIFI_DIRECT_GATE=PASS")
PY
