# ARDA sensor normalization and Wasmtime VHAL policy

## Scope

The AAP/TLS/SDR session remains in Rust. WebAssembly is not part of transport,
TLS, service discovery, protobuf framing, channel state, media, or input.

The sensor path is:

```text
AAP SensorSource protobuf
        |
        v
parse_sensor_batch_payload
        |
        v
ArdaSensorEvent (normalized Rust values)
        |
        v
Wasmtime policy module
        |
        v
typed VHAL command requests
        |
        v
Rust allow-list / type / area / range / rate validation
        |
        v
VHAL host adapter
```

The built-in policy is compiled from `wasm/arda_vhal_default.wat` at Rust build
time and embedded in `arda-senderd`. An external `.wasm` file can be selected
with `--sensor-wasm PATH` without rebuilding senderd.

## Master gate

Only `--sensor-open` opens the AAP SensorSource channel.

The following flags configure a future sensor session but must not implicitly
open one:

- `--sensor-request-all`
- `--sensor-max-type`
- `--sensor-period-ms`
- `--sensor-jsonl`
- `--sensor-location-provider`
- `--sensor-location-min-ms`
- `--sensor-vhal-min-ms`
- `--sensor-wasm`
- `--sensor-wasm-fuel`
- `--sensor-wasm-max-commands`

Likewise, `--sensor-forward-vhal` selects the VHAL destination but does not open
the SensorSource channel. `sensor-gate-smoke` is the regression test for this
contract.

The repository baseline keeps `sensor.open = false` and
`sensor.forward_vhal = false`.

## Normalization policy

`parse_sensor_batch_payload` continues to use the generated protobuf types. For
each sensor type present in an AAP `SensorBatch`, ARDA normalizes the latest
sample of that type into one `ArdaSensorEvent`. This intentionally collapses a
batch to current vehicle state before VHAL translation; the original batch
summary and JSONL capture remain available for diagnostics.

Current normalized sensor kinds match the AAP `SensorType` numeric ids:

| kind | event | ABI values `a..f` |
|---:|---|---|
| 1 | LOCATION | lat deg, lon deg, accuracy m, altitude m, speed m/s, bearing deg |
| 2 | COMPASS | bearing deg, pitch deg, roll deg, 0, 0, 0 |
| 3 | SPEED | speed m/s, cruise bool, cruise set speed raw, 0, 0, 0 |
| 4 | RPM | rpm, 0, 0, 0, 0, 0 |
| 5 | ODOMETER | km, trip km, 0, 0, 0, 0 |
| 6 | FUEL | level raw, range raw, low warning bool, 0, 0, 0 |
| 7 | PARKING_BRAKE | engaged bool, 0, 0, 0, 0, 0 |
| 8 | GEAR | AAP gear, Android VehicleGear, mapping-present bool, 0, 0, 0 |
| 9 | OBDII_DIAGNOSTIC_CODE | byte length, FNV-1a32 digest, 0, 0, 0, 0 |
| 10 | NIGHT_MODE | enabled bool, 0, 0, 0, 0, 0 |
| 11 | ENVIRONMENT_DATA | temperature C, pressure scaled by e3, rain raw, 0, 0, 0 |
| 12 | HVAC_DATA | target C, current C, 0, 0, 0, 0 |
| 13 | DRIVING_STATUS_DATA | status bitmask, 0, 0, 0, 0, 0 |
| 14 | DEAD_RECKONING_DATA | steering deg, wheel 0..3 m/s, wheel count |
| 15 | PASSENGER_DATA | present bool, 0, 0, 0, 0, 0 |
| 16 | DOOR_DATA | hood, trunk, door 0..3 bool |
| 17 | LIGHT_DATA | headlight enum, turn enum, hazard bool, 0, 0, 0 |
| 18 | TIRE_PRESSURE_DATA | tire 0..3 scaled by e2, count, 0 |
| 19 | ACCELEROMETER_DATA | x, y, z, 0, 0, 0 |
| 20 | GYROSCOPE_DATA | x, y, z, 0, 0, 0 |
| 21 | GPS_SATELLITE_DATA | in use, in view, listed, 0, 0, 0 |
| 22 | TOLL_CARD | present bool, 0, 0, 0, 0, 0 |
| 23 | VEHICLE_ENERGY_MODEL_DATA | present/opaque bool, 0, 0, 0, 0, 0 |

The current generated `SensorBatch` schema has no trailer payload field, so the
master request ceiling is 23 even though the AAP `SensorType` enum also names
`SENSOR_TRAILER_DATA(24)`. Do not silently request a payload the current proto
cannot normalize.

## Wasm ABI v1

Required exports:

```text
arda_abi_version() -> i32
arda_on_sensor(kind: i32,
               a: f64, b: f64, c: f64,
               d: f64, e: f64, f: f64) -> i32
```

`arda_abi_version()` must return `1`.

`arda_on_sensor()` returns `0` on success. A non-zero result discards all VHAL
commands emitted for that event.

Available imports are intentionally only:

```text
arda_vhal.emit_bool(property_id: i32, area_id: i32, value: i32) -> i32
arda_vhal.emit_i32(property_id: i32, area_id: i32, value: i32) -> i32
arda_vhal.emit_f32(property_id: i32, area_id: i32, value: f32) -> i32
```

No WASI linker is installed. The guest receives no filesystem, network, shell,
Binder, AAP session, or direct VHAL object.

## Runtime limits

Each event receives a fresh fixed fuel budget. A trap discards partially emitted
commands. The host also limits commands per event.

The current Wasmtime store limiter allows at most:

- 2 MiB linear memory per memory
- one linear memory
- one table
- 1024 table elements
- two instances

External Wasm module files are capped at 4 MiB before compilation.

These limits protect the host boundary; VHAL output validation is still applied
independently after guest execution.

## VHAL host validation

The guest does not write VHAL directly. Every command must pass Rust validation:

- property id allow-list
- exact value type (`bool`, `i32`, or `f32`)
- area id allow-list; currently global area `0` only
- finite floating-point value
- property range
- per-property/area minimum update interval

The built-in policy currently maps:

- SPEED -> `PERF_VEHICLE_SPEED`, `PERF_VEHICLE_SPEED_DISPLAY`
- RPM -> `ENGINE_RPM`
- ODOMETER -> `PERF_ODOMETER`
- PARKING_BRAKE -> `PARKING_BRAKE_ON`
- mapped GEAR -> `CURRENT_GEAR`, `GEAR_SELECTION`
- NIGHT_MODE -> `NIGHT_MODE`
- ENVIRONMENT_DATA temperature -> `ENV_OUTSIDE_TEMPERATURE`

FUEL is normalized but deliberately not mapped by the built-in policy because
the current AAP proto does not define the units of `fuel_level` and `range`.
An OEM/vehicle-specific policy can map those values after their source units are
established.

## Failure behavior

If Wasmtime cannot load, validate, instantiate, or ABI-check the selected module,
sensor decoding and JSONL continue but VHAL mapping is disabled. ARDA does not
fall back to the old direct protobuf-to-`cmd car_service` mapping.

If a guest call traps, all commands emitted by that call are discarded.

If VHAL forwarding is disabled, validated commands are logged as
`SENSOR_WASM_VHAL dry_run` and are not applied.

## Smoke tests

Run on the target Pi after building and pushing senderd:

```bash
/data/local/tmp/arda-senderd sensor-gate-smoke
/data/local/tmp/arda-senderd sensor-wasm-smoke
```

Required markers:

```text
SENSOR_GATE_SMOKE_OK=YES
SENSOR_WASM_ENGINE ready ...
SENSOR_WASM_SMOKE_OK=YES
```

The Wasm smoke test always uses VHAL dry-run mode.
