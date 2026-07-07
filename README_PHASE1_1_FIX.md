# ARDA v57 Phase 1.1 — Direct VehicleProperty correction

Baseline: the user's current v57 Phase 1 working checkout, produced from
`arda-v57-wip-phase1-full-source-20260707.zip`.

This package is a correction over that CURRENT Phase 1 checkout. Do not replay
older v53/v54/v56 patches and do not restart from 070620261158 for this test.

## What changed

1. rust-protobuf v3 generated accessors are explicitly enabled in build.rs.
   Existing router code intentionally uses the accessor API consistently.

2. The extra ARDA generic vehicle signal-ID namespace was removed.

3. VehicleSignal and VehicleCommand now carry Android VehicleProperty IDs
   directly as int32 `property_id` fields.

4. HU/AAP sensor semantics are mapped to known standard VehicleProperty IDs in
   Rust only.

5. The builtin Wasm ABI v2 module is a generic typed pass-through:
   vehicle property -> VHAL event
   VHAL set -> vehicle property command

6. The static Rust property registry is validation/authorization metadata, not
   a mapping table. It holds known type, area, access/capability and range rules.

7. Source arbitration still occurs before policy processing.

8. Request and confirmed state remain separate.

## Intentionally unchanged

- AAP/TLS/SDR parser and protocol path
- secure platform input bridge
- capture/video source path

## Compile status

Static checks passed in the packaging environment:
- git diff --check
- no stale `signal_id` API remains in proto/src/wasm/docs except historical text
- no hardcoded VHAL property integer IDs remain in the builtin WAT
- builtin WAT branches only on four scalar value types in each direction
- build.rs explicitly requests generated protobuf accessors

The actual Android Rust Docker cross-build has not been executed in this
packaging environment. The user's `./scripts/docker-build.sh` result is the
authoritative compile validation.

## Expected smoke markers after a successful build

SENSOR_GATE_SMOKE_OK=YES
SENSOR_WASM_SMOKE_OK=YES
VEHICLE_POLICY_SMOKE_GENERIC_PROPERTY_PASS_THROUGH=YES
VEHICLE_POLICY_SMOKE_OK=YES
VEHICLE_ROUTER_SMOKE_ARBITRATION_OK=YES
VEHICLE_ROUTER_SMOKE_PROPERTY_ID_COMMAND_OK=YES
VEHICLE_ROUTER_SMOKE_OK=YES

## Build follow-up after the first Phase 1.1 compile

The first user-side Android build still produced the old 204 rust-protobuf accessor `E0599`
cascade. The generated-file shape showed that a no-accessor `arda_vehicle_router.rs` was being
compiled even though the current `build.rs` requests accessors.

The Docker target build now cleans only the `arda-senderd` package artifacts for the selected Android
target before compiling. This is intentional: Phase patches/checkpoints are commonly restored with
`rsync -av`, which preserves archive mtimes, and stale Cargo build-script/output fingerprints must not
survive across codegen contract changes.

`build.rs` also verifies the generated router source immediately after protobuf codegen. A successful
build must print an `ARDA_PROTO_ACCESSORS_OK=.../arda_vehicle_router.rs` warning before Rust reaches
the router sources. Missing accessors now fail as one build-script error instead of hundreds of
`E0599` messages.

## Runtime follow-up after the first successful Android build

The Android cross-build passed. The first runtime smokes then found two independent test-path issues:

1. `sensor_event_to_vehicle_signals()` emitted built-in AAP signals with reserved `source_id=0`.
   Router forwarding hid the bug because `SensorRouterPublisher` overwrote the source with 1, while
   dry-run policy validation correctly rejected source zero before Wasm. The mapping now emits
   `AAP_SENSOR_SOURCE_ID=1` directly and the router registration uses the same constant.

2. A combined ADB shell wrapper used `pkill -f "arda-senderd vehicle-routerd"`. That pattern can match
   the remote `sh -c` command line that contains the pattern itself and kill the test shell. Use
   `scripts/run-vehicle-router-smoke-android.sh`; it records the daemon PID, kills only that PID, and
   uses `/data/local/tmp/arda_vehicle_router_smoke.sock` so the normal router socket is untouched.

Remaining Phase 1.1 gate:

```text
SENSOR_WASM_SMOKE_OK=YES
VEHICLE_ROUTER_SMOKE_ARBITRATION_OK=YES
VEHICLE_ROUTER_SMOKE_PROPERTY_ID_COMMAND_OK=YES
VEHICLE_ROUTER_SMOKE_OK=YES
```



## Router smoke privilege follow-up

The corrected sensor smoke is now green:

```text
SENSOR_WASM_EVENT ... source=1 ...
SENSOR_WASM_TRAP=NONE
SENSOR_WASM_SMOKE_OK=YES
```

The next router run reached the daemon and exposed a separate startup privilege mismatch. The router
prints the Wasmtime ready line before binding/configuring its Unix socket, then requires the socket to
be owned by `root:system` (`0:1000`). Launching `vehicle-routerd` as the normal ADB shell user cannot
perform that ownership change and exits with `EPERM` before `VEHICLE_ROUTER_READY`.

Do not make socket ownership best-effort and do not special-case `/data/local/tmp` by ignoring `chown`
failures. The ownership check is part of the router's intended security boundary.

Use the root-aware runner:

```bash
DEVICE=192.168.1.15:5555 ./scripts/run-vehicle-router-smoke-android.sh
```

It requests `adb root`, reconnects, verifies `id -u == 0`, and only then launches the PID-scoped smoke
router. Expected startup marker:

```text
ROUTER_ADB_ROOT_OK device=... uid=0 ...
```

The Rust router startup path now annotates I/O failures with stage/path/EUID/EGID. An ownership failure
will identify `stage=chown_socket_root_system_requires_privilege` rather than returning only
`Operation not permitted (os error 1)`.
