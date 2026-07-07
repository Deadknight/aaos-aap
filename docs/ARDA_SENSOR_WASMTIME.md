# ARDA vehicle-property policy / Wasmtime ABI v2

This document describes the v57 Phase 1.1 policy boundary.

## Position in the stack

Wasmtime is not the VHAL implementation and does not parse AAP transport frames.

```text
AAP SensorSource
    -> Rust HU sensor mapping
    -> standard Android VehicleProperty ID/value
                                      \
CAN / third-party adapter --------------> vehicle router
                                           -> source arbitration
                                           -> Wasmtime ABI v2
                                           -> VHAL property events

AAOS UI -> Car Service -> AIDL VHAL set
                              -> vehicle router
                              -> Wasmtime ABI v2
                              -> VehicleProperty command
                              -> approved adapter -> CAN
```

The proven AAP/TLS/SDR path stays in `arda-senderd`.

Location is deliberately excluded:

```text
AAP SENSOR_LOCATION -> ARDA GPS provider -> Android GPS_PROVIDER -> fused
```

Location must not be represented as a VHAL property or Wasm policy input.

## Property ID model

ARDA does not define a second generic signal-ID namespace.

`VehicleSignal.property_id`, `VhalEvent.property_id`, `VhalSetRequest.property_id`, and
`VehicleCommand.property_id` carry the Android `VehicleProperty` ID directly.

This is intentional. An adapter developer that already decoded a vehicle into standard AAOS
semantics may publish the standard property ID, area ID, type, value, and timestamp directly.
The default Wasm module does not contain a `NIGHT_MODE`, `RPM`, `GEAR`, ... switch table.

Examples:

```text
property_id = FUEL_LEVEL
area_id     = 0
value       = F32(32500.0)       # milliliters

property_id = EV_BATTERY_LEVEL
area_id     = 0
value       = F32(59940.0)       # watt-hours

property_id = HVAC_TEMPERATURE_SET
area_id     = ROW_1_LEFT
value       = F32(23.0)          # Celsius
```

The platform-internal protobuf keeps `property_id` as `int32` rather than a closed enum. This is
important because ARDA must not prevent vendor VehicleProperty IDs. The future adapter SDK may ship
convenience constants/generated wrappers, but the wire value remains the actual property ID.

## Built-in HU/AAP mapping

AAP/HU SensorSource is a built-in source, so its known sensor semantics are translated in Rust by
`src/vehicle/model.rs::sensor_event_to_vehicle_signals()`.

Current mappings:

```text
AAP SPEED        -> PERF_VEHICLE_SPEED
                 -> PERF_VEHICLE_SPEED_DISPLAY

AAP RPM          -> ENGINE_RPM
AAP ODOMETER     -> PERF_ODOMETER
AAP PARKING      -> PARKING_BRAKE_ON
AAP GEAR         -> CURRENT_GEAR
                 -> GEAR_SELECTION
AAP NIGHT_MODE   -> NIGHT_MODE
AAP ENV temp     -> ENV_OUTSIDE_TEMPERATURE
AAP HVAC target  -> HVAC_TEMPERATURE_SET / ROW_1_LEFT
AAP HVAC current -> HVAC_TEMPERATURE_CURRENT / ROW_1_LEFT
```

AAP location is not mapped to VHAL.

AAP `FuelData.fuel_level` and `range` remain unmapped because the current AAP protobuf does not
define source units. ARDA must not pretend those raw integers are VHAL milliliters/meters.

## ABI version

The guest must export:

```text
arda_abi_version() -> i32
```

The required value is `2`.

The guest also exports:

```text
arda_on_vehicle_signal(
    source_id: i32,
    property_id: i32,
    area_id: i32,
    value_type: i32,
    i64_lane: i64,
    f64_lane: f64
) -> i32

arda_on_vhal_set(
    property_id: i32,
    area_id: i32,
    value_type: i32,
    i64_lane: i64,
    f64_lane: f64
) -> i32

arda_reset_capability(
    capability_id: i32,
    area_id: i32
) -> i32
```

A zero return means the call completed. A non-zero status is fail-closed: outputs from that call are
discarded. A Wasm trap also discards all outputs from that call.

`source_id` must be in `1..=i32::MAX`. Source ID zero is reserved.

The built-in AAP/HU sensor adapter uses `AAP_SENSOR_SOURCE_ID=1`. The Rust sensor mapping creates
source-1 signals before either dry-run policy processing or router publication; source zero is not used
as an internal placeholder. This keeps the dry-run and live-router policy boundary identical.

## Typed value lanes

```text
1 BOOL   -> i64_lane = 0 or 1
2 I32    -> i64_lane
3 I64    -> i64_lane
4 F32    -> f64_lane
5 F64    -> f64_lane; no default ABI v2 emitter yet
6 BYTES  -> future memory/span ABI
```

## Host imports

Vehicle state to Android:

```text
arda_vhal.emit_bool(property_id, area_id, value)
arda_vhal.emit_i32(property_id, area_id, value)
arda_vhal.emit_i64(property_id, area_id, value)
arda_vhal.emit_f32(property_id, area_id, value)
```

Android set request to vehicle adapter:

```text
arda_vehicle.emit_bool(property_id, area_id, value)
arda_vehicle.emit_i32(property_id, area_id, value)
arda_vehicle.emit_i64(property_id, area_id, value)
arda_vehicle.emit_f32(property_id, area_id, value)
```

No WASI context is linked. The guest receives no filesystem, network, Binder, shell, AAP or direct
VHAL object.

## Built-in policy

`wasm/arda_vhal_default.wat` is a typed pass-through policy.

Vehicle direction:

```text
(property_id, area_id, BOOL) -> arda_vhal.emit_bool(same property, same area, same value)
(property_id, area_id, I32)  -> arda_vhal.emit_i32(...)
(property_id, area_id, I64)  -> arda_vhal.emit_i64(...)
(property_id, area_id, F32)  -> arda_vhal.emit_f32(...)
```

Reverse direction:

```text
VHAL set(property_id, area_id, value)
    -> arda_vehicle.emit_*(same property_id, same area_id, same value)
```

Therefore an adapter receiving `HVAC_TEMPERATURE_SET / ROW_1_LEFT / 23.0` decides how to encode that
request onto its own CAN/USB/BT protocol.

A custom Wasm module remains useful when policy is actually needed, for example:

```text
vendor property remap
unit conversion
filtering / clamping
stateful combination of several properties
OEM-specific HVAC encoding policy
one Android property -> several vehicle commands
several vehicle values -> one Android property
```

The default module should not become a giant property switch.

## Supported-property registry

Phase 1.1 does not yet have the real `IVehicleHardware` backend from which to consume the active
`VehiclePropConfig` set. `src/vehicle/model.rs` therefore contains one declarative
`SUPPORTED_PROPERTY_RULES` registry for the current test subset.

The registry holds only validation/authorization metadata:

```text
property ID
expected value type
publish capability
optional command capability
allowed area class
safety range
```

It is not a signal-to-property mapping table.

When the AIDL VHAL backend lands, the supported property/type/area metadata should be derived from
the actual VHAL property configs and combined with Arda Vehicle Manager grants. The small static
registry is the Phase 1 transition boundary, not the final source of VHAL truth.

## Source arbitration

The router selects state sources at `(capability, area)` scope.

Example:

```text
ENERGY / GLOBAL
  Mercedes CAN priority 100
  AAP/HU fallback priority 10
```

A higher-priority source can take ownership. A selected source can also be replaced after it is
stale for five seconds.

Before a capability source changes, the router calls:

```text
arda_reset_capability(capability_id, area_id)
```

The builtin pass-through policy is stateless, so its reset hook is currently a no-op. Custom
stateful policies must clear capability-scoped guest state there.

## Request versus confirmed state

A VHAL set request and confirmed vehicle state are separate.

```text
AAOS UI sets HVAC_TEMPERATURE_SET = 23
    -> VHAL set request
    -> Wasm reverse path
    -> adapter command property_id=HVAC_TEMPERATURE_SET value=23
    -> adapter writes CAN
```

That does not immediately create a confirmed 23 C VHAL event.

The adapter later reads actual vehicle state and publishes:

```text
HVAC_TEMPERATURE_SET = 23
```

Only that vehicle-originated publish becomes the confirmed VHAL property event.

## Limits / fail-closed behavior

Current policy host limits:

```text
Wasm file size          <= 4 MiB
linear memory           <= 2 MiB
memories                <= 1
tables                   <= 1
table elements          <= 1024
instances               <= 2
fuel                     configured per call
outputs                  configured per call
```

Host validation checks the current supported-property registry, exact type, allowed area, finite
floating-point values, numeric range, state-vs-command direction, and router capability ownership.

Guest trap, non-zero status, invalid output or output-limit overflow fails closed for that call.
