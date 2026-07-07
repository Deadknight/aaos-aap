# ARDA vehicle router v57 — Phase 1.1

## Status

This document describes the code that actually exists in the v57 Phase 1.1 Rust checkpoint.

Implemented now:

```text
generic VehicleProperty-based state model
platform-internal router protobuf
vehicle-routerd
SO_PEERCRED role gate
source registration
capability-scoped source arbitration
Wasmtime ABI v2 host
VHAL-event stream toward a future VHAL client
VHAL-set request stream toward an adapter source
vehicle command ACK / timeout / error routing
AAP/HU sensor publisher into the router
policy and router smoke tests
```

Not implemented yet:

```text
Arda Vehicle Manager system app
third-party adapter AIDL SDK
user approval / signing-lineage grant database
demo adapter APK
real AIDL IVehicleHardware backend
init / SELinux / product packaging for routerd
GPS provider / fused-location integration
```

Do not describe those later phases as completed code.

## Core property model

ARDA no longer defines a second generic signal-ID namespace.

The router protocol uses Android `VehicleProperty` IDs directly:

```text
VehicleSignal.property_id
VhalEvent.property_id
VhalSetRequest.property_id
VehicleCommand.property_id
```

The field type is `int32`, not a closed protobuf enum. This keeps standard and vendor property IDs
representable.

A source that already knows VHAL semantics publishes the property in the VHAL unit and type expected
for that property.

Examples:

```text
FUEL_LEVEL                 F32 32500.0   # milliliters
EV_BATTERY_LEVEL           F32 59940.0   # Wh
PERF_VEHICLE_SPEED         F32 13.8      # m/s
HVAC_TEMPERATURE_SET       F32 23.0      # Celsius, seat area
NIGHT_MODE                 BOOL true
```

The future adapter SDK may provide generated constants/helpers, but the router wire value remains the
real property ID.

## Built-in AAP/HU source

AAP SensorSource is built into ARDA and is the one place where AAP sensor semantics are hardcoded.

```text
AAP SensorBatch
    -> ArdaSensorEvent
    -> Rust sensor_event_to_vehicle_signals()
    -> VehicleProperty state values
    -> vehicle router
```

Known mappings are in `src/vehicle/model.rs`.

AAP location is deliberately omitted because it belongs on the future Android
`GPS_PROVIDER -> fused` path.

AAP fuel/range values are also omitted until their source units are established.

## Internal router protocol

Default socket:

```text
/dev/socket/arda_vehicle_router
```

Framing:

```text
u32 big-endian protobuf byte length
arda.vehicle.router.v2.Envelope
```

Maximum frame size is 1 MiB. Protocol version is 2.

Messages:

```text
Hello / HelloAck
RegisterSource / RegisterSourceResult
VehicleSignal / VehicleSignals
VhalEvent / VhalEvents
VhalSetRequest / VhalSetResult
VehicleCommand / VehicleCommands
CommandAck / CommandAcks
VhalSetError
Ping / Pong
```

This protobuf is platform-internal. The planned third-party app boundary remains Binder/AIDL.

## Router roles

```text
SIGNAL_SOURCE  built-in source such as AAP SensorSource
MANAGER        future Arda Vehicle Manager broker
VHAL           future ArdaVehicleHardware client
```

The Phase 1.1 router checks Linux peer credentials. Root may use all roles. The Android system appId
may use MANAGER and `vehicle_network` may use VHAL.

This is a transition boundary for development. The final init/SELinux packaging has not landed yet.

## Source registration and arbitration

A source registers:

```text
source_id
source_name
priority
publish capabilities
command capabilities
```

State-source selection is `(capability, area)` scoped.

Example intended priority:

```text
approved adapter     100
AAP SensorSource      10
```

A higher-priority source can take ownership. A selected source can also be replaced when stale for
five seconds or when it disconnects.

The current capability groups are:

```text
POWERTRAIN
ENERGY
HVAC_STATE
BODY_STATE
CHARGING_STATE

HVAC_CONTROL
BODY_CONTROL
CHARGING_CONTROL
```

## Supported-property registry

Until the real AIDL VHAL backend is attached, `src/vehicle/model.rs` keeps a declarative
`SUPPORTED_PROPERTY_RULES` subset.

For each current test property the registry records:

```text
property ID
expected value type
publish capability
optional command capability
allowed global/seat area class
sanity range
```

This registry is validation/authorization metadata. It does not translate one property into another.

Once `ArdaVehicleHardware` exists, the target is to build the supported property/type/area view from
the active VHAL `VehiclePropConfig` set and combine it with Vehicle Manager grants.

## Default Wasm policy

The embedded `wasm/arda_vhal_default.wat` is deliberately generic.

It branches only on the typed ABI lane:

```text
BOOL
I32
I64
F32
```

Vehicle to VHAL:

```text
(property_id, area_id, typed value)
    -> same property_id
    -> same area_id
    -> same value
```

VHAL set to vehicle command:

```text
(property_id, area_id, typed value)
    -> same property_id
    -> same area_id
    -> same value
```

There is no property-specific `if NIGHT_MODE`, `if RPM`, `if GEAR`, ... chain in the default WAT.

Custom Wasm remains available for an integration that really needs remap, filtering, stateful
combination or OEM policy.

## Request versus confirmed state

A VHAL set request and confirmed vehicle state remain separate.

```text
AAOS requests HVAC_TEMPERATURE_SET = 23
    -> VhalSetRequest
    -> Wasm reverse pass-through
    -> VehicleCommand property_id=HVAC_TEMPERATURE_SET value=23
    -> selected adapter
```

A successful router dispatch means the command was routed. It does not create a confirmed VHAL state
value.

The adapter later publishes actual vehicle state:

```text
VehicleSignal property_id=HVAC_TEMPERATURE_SET value=23
```

Only that vehicle-originated publish becomes the confirmed VHAL property event.

## Current tests

Policy smoke:

```bash
/data/local/tmp/arda-senderd vehicle-policy-smoke
```

Expected:

```text
VEHICLE_POLICY_SMOKE_GENERIC_PROPERTY_PASS_THROUGH=YES
VEHICLE_POLICY_SMOKE_OK=YES
```

Router terminal:

```bash
/data/local/tmp/arda-senderd vehicle-routerd
```

Second terminal:

```bash
/data/local/tmp/arda-senderd vehicle-router-smoke
```

Expected:

```text
VEHICLE_ROUTER_SMOKE_ARBITRATION_OK=YES
VEHICLE_ROUTER_SMOKE_PROPERTY_ID_COMMAND_OK=YES
VEHICLE_ROUTER_SMOKE_OK=YES
```

The router smoke checks capability-scoped ENERGY source priority/takeover and property-ID-preserving
HVAC command routing.

## Next phase

After Phase 1.1 compiles and these smoke tests pass:

```text
Phase 2
  Arda Vehicle Manager
  adapter AIDL SDK
  user grants
  signing-lineage validation
  capability approval UI
  demo third-party adapter

Phase 3
  real AIDL IVehicleHardware backend
  VehiclePropConfig-driven property registry
  router init/SELinux/product packaging

Then
  AAP location -> ARDA GPS provider -> GPS_PROVIDER -> fused
```
