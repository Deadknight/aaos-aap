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
