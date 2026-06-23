# Arda standalone plan

## Why standalone?

The existing AAP project should guide protocol sequencing, protobuf fields, and channel timing, but this project should stay minimal and controlled:

- one Rust daemon
- no Android app helper
- no Kotlin/Java dependency
- root/OS-level capture and input
- Docker build for Android aarch64

## Architecture

```text
arda-senderd
  capture/
    screenrecord_source -> raw H264 stdout
  h264/
    annexb parser -> SPS/PPS/IDR/non-IDR
  input/
    uinput touch/key injector later
  aap/
    TCP/DHU transport later
    version negotiation later
    service discovery later
    video channel later
```

## Current confirmed facts

- `screenrecord --output-format=h264 ... -` can be viewed live via `adb exec-out | mpv`.
- The stream is raw H264 / JVT NAL sequence, not MP4 container.
- `/dev/uinput` exists.
- USB mouse appeared as kernel input events, so the input stack is alive.
- `/config/usb_gadget` and `/sys/class/udc` are currently empty; USB gadget comes later.
