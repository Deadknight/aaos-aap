# Combined GPS Persistence, Full Multitouch, and Video Focus

Date: 2026-08-05

This canonical source snapshot combines:

- persistent AAP SENSOR_LOCATION forwarding to Android `arda_gps`;
- line-v2 generic multipointer transport (up to 16 pointers);
- snapshot-to-Android-MotionEvent reconciliation in ArdaInputBridge;
- video-focus handling that treats focus as display ownership rather than
  connection ownership.

Video focus behavior:

- NATIVE focus stops the H.264 capture/output path only;
- the AAP/TLS session and already-open video channel remain alive;
- host-liveness timeout is suspended while projection output is hidden;
- PROJECTED focus re-arms capture, media start, codec config, and a fresh IDR
  on the existing session/channel.

Validation status at this checkpoint:

- GPS persistence: runtime PASS.
- Full multitouch: runtime PASS; Maps pinch zoom in/out works.
- Video focus: integrated candidate; final real-HU validation remains pending.
