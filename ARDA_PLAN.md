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
