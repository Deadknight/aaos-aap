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
