# ARDA Full Multitouch — 2026-08-04

This snapshot extends the proven privileged InputBridge path from the legacy
single-pointer line protocol to a generic multipointer protocol.

## Protocol

```text
DISPLAY <display_id>
TM <ACTION> <action_index> <count> <pointer_id> <x> <y> ...
K <android_keycode>
STOP
```

Supported actions:

```text
DOWN
POINTER_DOWN
MOVE
POINTER_UP
UP
CANCEL
```

The legacy `T DOWN|MOVE|UP ...` command remains accepted for compatibility.

## Limits and validation

- Up to 16 simultaneous pointers.
- Android pointer ids must be unique and in the range 0..31.
- `action_index` is preserved for `POINTER_DOWN` and `POINTER_UP`.
- Malformed transitions force `ACTION_CANCEL` so no stale touch remains.
- Disconnect and display change also cancel an active gesture.

## Scope

- Rust AAP input forwarding now preserves the complete `pointer_data` list.
- Every pointer is coordinate-mapped independently.
- The privileged ArdaInputBridge creates real multipointer `MotionEvent`s.
- The embedded app_process InputServer implements the same line-v2 protocol.
- GPS persistence remains unchanged.
- No focus candidate is included.
- No VHAL or Launcher source is changed.
