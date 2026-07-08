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

## Phase 2 implementation status

Implemented and runtime-proven in Phase 2.0:

- Arda Vehicle Manager Android system app
- third-party adapter AIDL SDK
- PackageManager adapter discovery
- per-user device-protected grant database
- signing-lineage grant validation
- explicit publish/command capability grants
- demo smoke adapter APK
- grant -> bind -> descriptor -> publish Binder runtime flow

Implemented and real-build proven in Phase 2.1:

- per-source Manager `ROLE_MANAGER` LocalSocket bridge
- protocol-v2 Hello/RegisterSource
- adapter `VehicleSignals` forwarding with manager-owned source ID
- router `VehicleCommands` delivery over one-way AIDL
- adapter `CommandAck` forwarding as router `CommandAcks`
- bounded latest-state retention while the router is unavailable
- end-to-end `vehicle-manager-bridge-smoke` command and Android runner
- Android `LocalSocket` connect-before-timeout ordering fix

The first real enforcing runtime connect reached the platform boundary and failed exactly on:

```text
scontext=u:r:system_app:s0
tcontext=u:object_r:socket_device:s0
tclass=sock_file
denied { write }
permissive=0
```

Phase 2.2 source therefore platformizes the router instead of granting broad generic socket access:

- init-managed `arda_vehicle_router` service in `system_ext`
- init-created `stream+listen` `/dev/socket/arda_vehicle_router`
- Rust consumption of `ANDROID_SOCKET_arda_vehicle_router` with listener validation
- manual `UnixListener::bind()` fallback for existing custom-socket/router smokes
- dedicated `arda_vehicle_router` core domain
- dedicated `arda_vehicle_router_socket` type
- package-specific `arda_vehicle_manager` app domain
- narrow `sock_file write` and `unix_stream_socket connectto` from `arda_vehicle_manager` to `arda_vehicle_router_socket`
- userdebug/eng-only `su` socket client allowance for the existing root VHAL smoke client

Still not implemented/closed:

- real AIDL `IVehicleHardware` backend
- `VehiclePropConfig`-driven runtime property registry
- dedicated ARDA GPS provider / Android fused-location integration
- production capability approval UI

Phase 2.2 is now runtime-complete under enforcing SELinux. The broad `allow system_app socket_device:sock_file write` rule remains forbidden.

## Compile and runtime validation status

The user's real Android Docker cross-build passed after forcing fresh package-specific protobuf codegen.
The old rust-protobuf accessor `E0599` cascade is closed.

User-side smoke results on 2026-07-07:
- `sensor-gate-smoke`: PASS.
- `vehicle-policy-smoke`: PASS, including generic VehicleProperty pass-through.
- `sensor-wasm-smoke`: PASS after the source-ID correction. All built-in AAP events now carry
  `source=1`, no `SENSOR_WASM_TRAP` remains, and `SENSOR_WASM_SMOKE_OK=YES` passes.
- the first router smoke wrapper did not actually reach daemon startup because its `pkill -f` pattern
  could match and terminate the remote `sh -c` test shell itself. The PID-scoped runner fixed that.
- the PID-scoped runner then exposed a privilege mismatch: the router reached Wasmtime initialization
  but exited before `VEHICLE_ROUTER_READY` with `EPERM`. The router intentionally chowns its socket to
  `root:system` (`0:1000`), while the runner had launched it as normal ADB shell UID 2000.
- the current runner now requests `adb root`, verifies remote UID 0 before daemon startup, and preserves
  the strict root:system socket ownership boundary. Router startup errors also include stage/path/EUID/EGID.

Phase 1.1 is therefore compile-green and sensor-policy runtime-green, but remains runtime-open until the
root-aware PID-scoped router smoke passes.

## Validation order

1. Run `./scripts/docker-build.sh`.
2. Push the rebuilt Android binary.
3. `sensor-wasm-smoke` is already PASS with `source=1`, no traps, and `SENSOR_WASM_SMOKE_OK=YES`.
4. Run `DEVICE=<network-adb-device> ./scripts/run-vehicle-router-smoke-android.sh`; the script must
   print `ROUTER_ADB_ROOT_OK ... uid=0` before router startup.
5. Require:
   - `VEHICLE_ROUTER_SMOKE_ARBITRATION_OK=YES`
   - `VEHICLE_ROUTER_SMOKE_PROPERTY_ID_COMMAND_OK=YES`
   - `VEHICLE_ROUTER_SMOKE_OK=YES`
6. Only after those two remaining runtime checks pass, continue with Vehicle Manager / adapter AIDL.

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

## 2026-07-07 Phase 1.1 runtime closure

Verified on Raspberry Pi:

```text
SENSOR_WASM_TRAP=NONE
SENSOR_WASM_SMOKE_OK=YES
VEHICLE_POLICY_SMOKE_OK=YES
VEHICLE_ROUTER_SMOKE_ARBITRATION_OK=YES
VEHICLE_ROUTER_SMOKE_PROPERTY_ID_COMMAND_OK=YES
VEHICLE_ROUTER_SMOKE_OK=YES
```

The router smoke also verified:

```text
lower-priority ENERGY source shadowing
higher-priority ENERGY takeover
source release on disconnect
VHAL set -> VehicleCommand dispatch by Android VehicleProperty ID
CommandAck handling
adapter-confirmed state publish as a separate VHAL event
```

Phase 1.1 status: CLOSED.

Phase 2.0+ platform source now lives in `Deadknight/android_vendor_arda` under `vehicle/` and is copied into AOSP as `vendor/arda/vehicle/`. See
`docs/ARDA_VEHICLE_MANAGER_PHASE2.md`.

### Phase 2.0 security review

- Adapter discovery now requires the exported service to be protected by `com.arda.vehicle.permission.BIND_ADAPTER` (`signature`).
- Manager validates manifest/Binder descriptor priority and capability identity before readiness.
- Closed/revoked adapter connections reject delayed service-connect and stale host callbacks.
- Grant/revoke/source-id persistence is synchronous and fail-closed.

## 2026-07-08 Phase 2.0 runtime closure and Phase 2.1 source state

Phase 2.0 is now runtime-closed on Raspberry Pi/user 10:

```text
ARDA_VM_GRANT_OK ... source=10000 priority=100 publish=[2, 3] commands=[16]
ARDA_VM_STATUS ... grants=1 ... granted=true signer_ok=true connected=true ready=true
ARDA_VM_RESCAN user=10
ARDA_VM_BIND_REQUEST ... source=10000
ARDA_VM_ADAPTER_READY ... source=10000
SMOKE_ADAPTER_HOST_CONNECTED
ARDA_VM_SIGNAL_ACCEPT ... property=291504903 area=0 type=4
ARDA_VM_SIGNAL_ACCEPT ... property=358614274 area=1 type=4
ARDA_PHASE2_0_RUNTIME_GATE=PASS
```

The previously listed Vehicle Manager/AIDL/grant/signing/demo-adapter items are implemented and must
no longer be described as future code. Remaining major platform gaps are the real AIDL
`IVehicleHardware` backend, `VehiclePropConfig`-driven runtime property registry,
vehicle-routerd init/SELinux/product daemon packaging, and the dedicated GPS/fused-location path.

Phase 2.1 source now adds:

```text
vendor/arda/vehicle/router-proto
vendor/arda/vehicle/manager/.../RouterBridge.java
scripts/run-vehicle-manager-bridge-smoke-android.sh
vehicle-manager-bridge-smoke
```

Bridge architecture is one `ROLE_MANAGER` LocalSocket per granted adapter/source. This is intentional:
protocol v2 has no `UnregisterSource`, while router source cleanup is connection-scoped. Closing one
source link therefore releases only that source and its pending commands through existing router
cleanup semantics.

The Manager now frames accepted adapter properties as protocol-v2 `VehicleSignals`, injects the
manager-owned source ID, receives `VehicleCommands`, delivers them over one-way AIDL, and forwards
adapter `CommandAck` values as `CommandAcks`. Router remains authoritative for property/type/area and
capability validation.

The bridge keeps at most the latest value for 512 distinct `(propertyId, areaId)` keys while the
router is unavailable, flushes in batches of 128 after reconnect, and intentionally does not replay
ACKs across router reconnects.

Validation completed in the artifact-preparation environment:

```text
SHELL_OK=YES
ROUTER_PROTO_SYNC=YES
JAVA_BRACE_OK=YES
AIDL_BRACE_OK=YES
JAVA_STUB_COMPILE_OK=YES classes=94 total_sources=56 actual_arda_java=16 fake_stubs=40
```

The four javac warnings are only serialVersionUID warnings in fake Android validation stubs, not ARDA
source compile errors.

Not yet claimed:
- real AOSP/Soong Phase 2.1 build;
- real Rust/Cargo Android build, because Cargo is unavailable in the artifact-preparation container;
- Raspberry Pi enforcing-SELinux Manager <-> router registration;
- end-to-end Phase 2.1 VHAL set -> AIDL adapter -> ACK + confirmed-state smoke.

The new Android smoke runner never calls `setenforce 0`. On registration failure it captures socket
and process contexts plus recent SELinux AVCs. If enforcing policy blocks the manual ADB-launched
router, the next implementation step is init-managed `vehicle-routerd` plus a dedicated narrow
SELinux domain/socket policy.

## 2026-07-08 Phase 2.1 enforcing boundary and Phase 2.2 platformization

The real Phase 2.1 Android Docker Rust build and canonical `vendor/arda/vehicle` Soong builds passed.
Phase 2.0 grant state survived APK replacement and reboot with source `10000` and the smoke adapter
remained `granted=true signer_ok=true connected=true ready=true`.

A local Android `LocalSocket` ordering bug was found before the first real SELinux gate. Calling
`setSoTimeout()` on a fresh `LocalSocket` failed with `IOException: socket not created` before
`connect()`. The Manager now connects first and sets the 5-second handshake timeout afterward.
Runtime behavior changed from the local pre-connect error to the expected OS results:

```text
router absent  -> No such file or directory
router present -> Permission denied
```

With SELinux explicitly `Enforcing`, the manually ADB-root-launched router created:

```text
/dev/socket/arda_vehicle_router
u:object_r:socket_device:s0
```

and the Manager reached `LocalSocket.connect()`. The exact current denial is:

```text
avc: denied { write }
scontext=u:r:system_app:s0
tcontext=u:object_r:socket_device:s0
tclass=sock_file
permissive=0
```

This is the first real Phase 2.1 enforcing platform boundary. The bridge/protobuf/AIDL path must not be
bypassed with permissive mode or a broad `system_app -> socket_device` allow rule.

Phase 2.2 source now introduces the production-shaped boundary:

```text
init
  -> creates + binds + listens /dev/socket/arda_vehicle_router
  -> socket type arda_vehicle_router_socket
  -> starts /system_ext/bin/arda-senderd vehicle-routerd
  -> domain transition to arda_vehicle_router
  -> exports ANDROID_SOCKET_arda_vehicle_router

arda-senderd vehicle-routerd
  -> validates inherited fd is open, SOCK_STREAM and SO_ACCEPTCONN=1
  -> validates local pathname is /dev/socket/arda_vehicle_router
  -> consumes the inherited listener

com.arda.vehicle.manager
  -> package-specific arda_vehicle_manager domain
  -> narrow unix_socket_connect to arda_vehicle_router
```

The old manual listener path remains available when no init control-socket environment variable is
present, preserving existing custom socket and router smoke workflows.

New product integration source:

```text
vendor/arda/vehicle/router-service
vendor/arda/vehicle/sepolicy/private
scripts/stage-arda-vehicle-router-prebuilt.sh
```

`arda_vehicle.mk` now includes both `ArdaVehicleManager` and `arda-vehicle-routerd`; the smoke adapter
remains opt-in.

Phase 2.2 completed the real Android Docker Rust rebuild, AOSP Soong/system_ext policy build,
coherent precompiled-policy/hash deployment, init-managed router startup, dedicated socket/router/Manager
domain validation, and full Manager command/ACK/confirmed-state smoke under Enforcing.

The exact runtime-proven additions were the socket-type `connectto` rule for the Manager and
`self:process execmem` only for the dedicated Wasmtime router domain. No broad generic app/socket rule
was added.

## 2026-07-08 Phase 2.1 / Phase 2.2 final closure

Final Enforcing gate:

```text
MANAGER_BRIDGE_MANAGER_DOMAIN=PASS
MANAGER_BRIDGE_ROUTER_REGISTRATION=PASS
VEHICLE_MANAGER_BRIDGE_SMOKE_SET_HANDLED=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_CONFIRMED_STATE=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_ACK_ROUND_TRIP=YES
VEHICLE_MANAGER_BRIDGE_SMOKE_OK=YES
MANAGER_BRIDGE_ENFORCING_RUNTIME_GATE=PASS selinux=Enforcing
```

Router startup is production-shaped:

```text
init-created /dev/socket/arda_vehicle_router
  -> u:object_r:arda_vehicle_router_socket:s0

/system_ext/bin/arda-senderd
  -> u:object_r:arda_vehicle_router_exec:s0
  -> u:r:arda_vehicle_router:s0
  -> VEHICLE_ROUTER_SOCKET_MODE=init_inherited
  -> VEHICLE_WASM_ENGINE ready

com.arda.vehicle.manager
  -> u:r:arda_vehicle_manager:s0
```

Phase 2.1: CLOSED.
Phase 2.2: CLOSED.

Git checkpoints:

```text
android_vendor_arda      b53e5473d31de98573cb426c6618bdc9cc699e07
android_device_brcm_rpi4 0b13b2a565056f0b32a23abe9acc362401524b1a
```

Canonical Android platform source is `android_vendor_arda/vehicle`; `aaos-aap` does not keep a
duplicate in-repo Vehicle platform tree.
