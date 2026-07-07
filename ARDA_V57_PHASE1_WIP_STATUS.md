# ARDA v57 WIP — Phase 1.1 handoff

Date: 2026-07-07
Baseline/source-of-truth used: `070620261158.zip`
Previous checkpoint: v57 Phase 1

## This is real persisted code

This tree contains the current v57 Phase 1.1 implementation. It is not a design-only snapshot.

## Why Phase 1.1 exists

The first Phase 1 Docker compile exposed one repeated rust-protobuf v3 API-shape mistake: the router
used generated accessor methods but code generation did not explicitly enable them. The same mistake
expanded into the large `E0599` cascade.

The first Phase 1 builtin WAT policy also duplicated standard VHAL semantics as a long property switch.
Phase 1.1 simplifies that architecture before the first successful router checkpoint.

## Implemented in Phase 1.1

- Explicitly enable rust-protobuf field accessors in `build.rs`.
- Use Android `VehicleProperty` IDs directly in the new router model.
- Rename router protobuf `VehicleSignal.signal_id` and `VehicleCommand.signal_id` to `property_id`.
  Field numbers are unchanged.
- Remove the second ARDA generic signal-ID namespace.
- Hardcode only known AAP/HU SensorSource -> VehicleProperty mappings in Rust.
- Keep AAP location outside VHAL/Wasmtime.
- Keep AAP FuelData unmapped until its source units are established.
- Replace the builtin WAT property switch with a generic typed pass-through guest.
- Keep custom Wasm as optional remap/filter/stateful/OEM policy.
- Add declarative `SUPPORTED_PROPERTY_RULES` validation/authorization metadata for the current Phase 1
  subset. It is not a signal mapping table.
- Preserve source arbitration, command ACK/timeout, VHAL set error routing and the central router.
- Rewrite policy/router smokes for property-ID-preserving behavior.

## Current default semantics

```text
AAP/HU
  -> Rust known-sensor mapping
  -> VehicleProperty ID + area + typed value
                                      \
third-party/CAN property source -------> vehicle-routerd
                                         -> arbitration
                                         -> Wasmtime typed pass-through
                                         -> same VehicleProperty VHAL event

VHAL set
  -> vehicle-routerd
  -> Wasmtime typed pass-through
  -> same VehicleProperty command
  -> adapter
```

The adapter is responsible for OEM/CAN encoding when it receives a standard property command.

## Actually implemented components

- `proto/arda_vehicle_router.proto`
- `src/vehicle/model.rs`
- `src/vehicle/policy.rs`
- `src/vehicle/router.rs`
- `src/vehicle/client.rs`
- `src/vehicle/wire.rs`
- `src/vehicle/smoke.rs`
- AAP sensor publisher into the router
- `vehicle-routerd`
- `vehicle-policy-smoke`
- `vehicle-router-smoke`

## Not implemented yet

- Arda Vehicle Manager Android system app
- third-party adapter AIDL SDK
- PackageManager adapter discovery UI
- per-user grant database
- signing-lineage grant validation
- capability approval UI
- demo third-party adapter APK
- real AIDL `IVehicleHardware` backend
- `VehiclePropConfig`-driven runtime property registry
- init/SELinux/product packaging for vehicle-routerd
- dedicated ARDA GPS provider / Android fused-location integration

Do not describe these later phases as completed code.

## Compile status

The user has already run the first Phase 1 Docker build. That build reached Rust compilation but failed
on the rust-protobuf accessor cascade before router code could compile.

Phase 1.1 contains the codegen correction and architecture simplification, but has not yet passed the
user's real Android cross-build. The next compiler result is authoritative.

## Validation order

1. Apply Phase 1.1 changed files to the current Phase 1 tree.
2. Run `./scripts/docker-build.sh`.
3. Require the old protobuf `set_*` / `field, not a method` cascade to disappear.
4. Run `sensor-gate-smoke`.
5. Run `sensor-wasm-smoke`.
6. Run `vehicle-policy-smoke` and require:
   - `VEHICLE_POLICY_SMOKE_GENERIC_PROPERTY_PASS_THROUGH=YES`
   - `VEHICLE_POLICY_SMOKE_OK=YES`
7. Start `vehicle-routerd`.
8. Run `vehicle-router-smoke` and require:
   - `VEHICLE_ROUTER_SMOKE_ARBITRATION_OK=YES`
   - `VEHICLE_ROUTER_SMOKE_PROPERTY_ID_COMMAND_OK=YES`
   - `VEHICLE_ROUTER_SMOKE_OK=YES`
9. Only after Phase 1.1 passes, continue with Vehicle Manager / adapter AIDL.

## Preserved proven paths

Phase 1/1.1 does not intentionally rewrite:

- `src/input/mod.rs`
- `src/capture/mod.rs`
- `src/capture/screenrecord.rs`
- `src/aap/mod.rs`
- `src/aap/proto.rs`
- `src/aap/tls.rs`
- `src/aap/fragment.rs`

The proven AAP/TLS/SDR, capture and secure input paths remain the baseline.

## Phase 1.1 build follow-up — stale generated protobuf guard

The first Phase 1.1 Android build still showed the same 204 accessor-shaped `E0599` errors even though
`build.rs` requested `Customize::generate_accessors(true)`.

The failure shape is consistent with the Phase 1 generated `OUT_DIR/protos/arda_vehicle_router.rs`
being reused after a timestamp-preserving patch restore. The Phase 1.1 patch was applied with
`rsync -av`; source mtimes from the archive can therefore be older than Cargo's existing target
fingerprints even when file contents changed.

The Android Docker build is now hardened in two places:

- `scripts/docker-build-android.sh` runs `cargo clean -p arda-senderd --target "$TARGET"` before the
  package build. This removes only ARDA's selected-target artifacts and keeps dependency artifacts
  cached, forcing the current `build.rs` and protobuf generation to run.
- `build.rs` verifies the generated `arda_vehicle_router.rs` immediately after codegen. Required
  router accessors such as `set_protocol_version`, `has_hello`, `source_id`, `property_id` and
  `has_vhal_events` must be present. If not, the build stops in codegen with one explicit error instead
  of reaching the 204-error Rust accessor cascade.

The router/proto/property architecture is unchanged by this build fix. AAP/TLS/SDR, capture and the
secure platform input path remain untouched.

Next authoritative validation remains:

1. `./scripts/docker-build.sh`
2. Require `ARDA_PROTO_ACCESSORS_OK=.../arda_vehicle_router.rs` during `arda-senderd` build.
3. Require the old 204 `E0599` accessor cascade to be absent.
4. Continue with `sensor-gate-smoke`, `sensor-wasm-smoke`, `vehicle-policy-smoke`, then
   `vehicle-router-smoke` in the existing Phase 1.1 order.
