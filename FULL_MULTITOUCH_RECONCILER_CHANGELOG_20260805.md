# Full Multitouch Snapshot Reconciler v1 — 2026-08-05

## Root cause

The line-v2 transport was carrying valid multi-pointer snapshots such as:

```text
TM MOVE 0 2 0 388 279 1 263 448
```

The platform bridge still enforced a strict Android transition order. DHU/AAP can
emit a MOVE that is the first frame containing an additional pointer, and can
emit final releases as `UP(count > 1)` followed by `POINTER_UP`. The strict
validator rejected those snapshots, cancelled the active gesture, and every
following MOVE failed.

## Fix

The platform bridge now treats each AAP frame as a complete pointer snapshot and
reconciles it into a valid Android MotionEvent sequence:

- Missing DOWN is synthesized.
- Missing POINTER_DOWN events are synthesized for every newly appearing ID.
- Missing POINTER_UP events are synthesized for disappearing IDs.
- `UP(count > 1)` is normalized to POINTER_UP.
- A final POINTER_UP is normalized to ACTION_UP.
- Pointer IDs and all coordinates are preserved.
- Up to 16 pointers remain supported.
- Stale pointers present only in release snapshots are not re-added.
- Unrecoverable mismatches cancel and bootstrap a clean gesture.

Only `ArdaInputBridge` changes. `arda-connectivityd` and the GPS configuration
remain untouched by the build/install package.
