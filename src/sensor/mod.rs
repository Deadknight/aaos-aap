use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use wasmtime::{
    Caller, Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc,
};

pub const SENSOR_WASM_ABI_VERSION: i32 = 1;
const BUILTIN_WASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/arda_vhal_default.wasm"));
const MAX_WASM_MODULE_BYTES: usize = 4 * 1024 * 1024;
const MAX_WASM_LINEAR_MEMORY_BYTES: usize = 2 * 1024 * 1024;
const MAX_WASM_TABLE_ELEMENTS: usize = 1024;

#[derive(Debug, Clone)]
pub enum ArdaSensorEvent {
    Location {
        latitude_deg: f64,
        longitude_deg: f64,
        accuracy_m: f64,
        altitude_m: f64,
        speed_mps: f64,
        bearing_deg: f64,
    },
    Compass {
        bearing_deg: f64,
        pitch_deg: f64,
        roll_deg: f64,
    },
    Speed {
        speed_mps: f64,
        cruise_engaged: bool,
        cruise_set_speed_raw: i32,
    },
    Rpm {
        rpm: f64,
    },
    Odometer {
        kilometers: f64,
        trip_kilometers: f64,
    },
    Fuel {
        level_raw: i32,
        range_raw: i32,
        low_fuel_warning: bool,
    },
    ParkingBrake {
        engaged: bool,
    },
    Gear {
        aap_gear: i32,
        android_gear: Option<i32>,
    },
    Diagnostics {
        dtc: Vec<u8>,
    },
    NightMode {
        enabled: bool,
    },
    Environment {
        temperature_c: f64,
        pressure: f64,
        rain_raw: i32,
    },
    Hvac {
        target_temperature_c: f64,
        current_temperature_c: f64,
    },
    DrivingStatus {
        status: i32,
    },
    DeadReckoning {
        steering_angle_deg: f64,
        wheel_speed_mps: Vec<f64>,
    },
    Passenger {
        present: bool,
    },
    Door {
        hood_open: bool,
        trunk_open: bool,
        door_open: Vec<bool>,
    },
    Light {
        head_light_state: i32,
        turn_indicator_state: i32,
        hazard_lights_on: bool,
    },
    TirePressure {
        pressures: Vec<f64>,
    },
    Accelerometer {
        x: f64,
        y: f64,
        z: f64,
    },
    Gyroscope {
        x: f64,
        y: f64,
        z: f64,
    },
    GpsSatellite {
        in_use: i32,
        in_view: i32,
        listed: usize,
    },
    TollCard {
        present: bool,
    },
    EnergyModel {
        present: bool,
    },
}

impl ArdaSensorEvent {
    pub fn kind_id(&self) -> i32 {
        match self {
            Self::Location { .. } => 1,
            Self::Compass { .. } => 2,
            Self::Speed { .. } => 3,
            Self::Rpm { .. } => 4,
            Self::Odometer { .. } => 5,
            Self::Fuel { .. } => 6,
            Self::ParkingBrake { .. } => 7,
            Self::Gear { .. } => 8,
            Self::Diagnostics { .. } => 9,
            Self::NightMode { .. } => 10,
            Self::Environment { .. } => 11,
            Self::Hvac { .. } => 12,
            Self::DrivingStatus { .. } => 13,
            Self::DeadReckoning { .. } => 14,
            Self::Passenger { .. } => 15,
            Self::Door { .. } => 16,
            Self::Light { .. } => 17,
            Self::TirePressure { .. } => 18,
            Self::Accelerometer { .. } => 19,
            Self::Gyroscope { .. } => 20,
            Self::GpsSatellite { .. } => 21,
            Self::TollCard { .. } => 22,
            Self::EnergyModel { .. } => 23,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Location { .. } => "LOCATION",
            Self::Compass { .. } => "COMPASS",
            Self::Speed { .. } => "SPEED",
            Self::Rpm { .. } => "RPM",
            Self::Odometer { .. } => "ODOMETER",
            Self::Fuel { .. } => "FUEL",
            Self::ParkingBrake { .. } => "PARKING_BRAKE",
            Self::Gear { .. } => "GEAR",
            Self::Diagnostics { .. } => "OBDII_DIAGNOSTIC_CODE",
            Self::NightMode { .. } => "NIGHT_MODE",
            Self::Environment { .. } => "ENVIRONMENT_DATA",
            Self::Hvac { .. } => "HVAC_DATA",
            Self::DrivingStatus { .. } => "DRIVING_STATUS_DATA",
            Self::DeadReckoning { .. } => "DEAD_RECKONING_DATA",
            Self::Passenger { .. } => "PASSENGER_DATA",
            Self::Door { .. } => "DOOR_DATA",
            Self::Light { .. } => "LIGHT_DATA",
            Self::TirePressure { .. } => "TIRE_PRESSURE_DATA",
            Self::Accelerometer { .. } => "ACCELEROMETER_DATA",
            Self::Gyroscope { .. } => "GYROSCOPE_DATA",
            Self::GpsSatellite { .. } => "GPS_SATELLITE_DATA",
            Self::TollCard { .. } => "TOLL_CARD",
            Self::EnergyModel { .. } => "VEHICLE_ENERGY_MODEL_DATA",
        }
    }

    pub fn abi_values(&self) -> [f64; 6] {
        match self {
            Self::Location {
                latitude_deg,
                longitude_deg,
                accuracy_m,
                altitude_m,
                speed_mps,
                bearing_deg,
            } => [
                *latitude_deg,
                *longitude_deg,
                *accuracy_m,
                *altitude_m,
                *speed_mps,
                *bearing_deg,
            ],
            Self::Compass {
                bearing_deg,
                pitch_deg,
                roll_deg,
            } => [*bearing_deg, *pitch_deg, *roll_deg, 0.0, 0.0, 0.0],
            Self::Speed {
                speed_mps,
                cruise_engaged,
                cruise_set_speed_raw,
            } => [
                *speed_mps,
                bool_number(*cruise_engaged),
                *cruise_set_speed_raw as f64,
                0.0,
                0.0,
                0.0,
            ],
            Self::Rpm { rpm } => [*rpm, 0.0, 0.0, 0.0, 0.0, 0.0],
            Self::Odometer {
                kilometers,
                trip_kilometers,
            } => [*kilometers, *trip_kilometers, 0.0, 0.0, 0.0, 0.0],
            Self::Fuel {
                level_raw,
                range_raw,
                low_fuel_warning,
            } => [
                *level_raw as f64,
                *range_raw as f64,
                bool_number(*low_fuel_warning),
                0.0,
                0.0,
                0.0,
            ],
            Self::ParkingBrake { engaged } => {
                [bool_number(*engaged), 0.0, 0.0, 0.0, 0.0, 0.0]
            }
            Self::Gear {
                aap_gear,
                android_gear,
            } => [
                *aap_gear as f64,
                android_gear.unwrap_or(-1) as f64,
                bool_number(android_gear.is_some()),
                0.0,
                0.0,
                0.0,
            ],
            Self::Diagnostics { dtc } => [
                dtc.len() as f64,
                fnv1a32(dtc) as f64,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
            Self::NightMode { enabled } => {
                [bool_number(*enabled), 0.0, 0.0, 0.0, 0.0, 0.0]
            }
            Self::Environment {
                temperature_c,
                pressure,
                rain_raw,
            } => [
                *temperature_c,
                *pressure,
                *rain_raw as f64,
                0.0,
                0.0,
                0.0,
            ],
            Self::Hvac {
                target_temperature_c,
                current_temperature_c,
            } => [
                *target_temperature_c,
                *current_temperature_c,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
            Self::DrivingStatus { status } => [*status as f64, 0.0, 0.0, 0.0, 0.0, 0.0],
            Self::DeadReckoning {
                steering_angle_deg,
                wheel_speed_mps,
            } => [
                *steering_angle_deg,
                wheel_speed_mps.first().copied().unwrap_or(0.0),
                wheel_speed_mps.get(1).copied().unwrap_or(0.0),
                wheel_speed_mps.get(2).copied().unwrap_or(0.0),
                wheel_speed_mps.get(3).copied().unwrap_or(0.0),
                wheel_speed_mps.len() as f64,
            ],
            Self::Passenger { present } => {
                [bool_number(*present), 0.0, 0.0, 0.0, 0.0, 0.0]
            }
            Self::Door {
                hood_open,
                trunk_open,
                door_open,
            } => [
                bool_number(*hood_open),
                bool_number(*trunk_open),
                bool_number(door_open.first().copied().unwrap_or(false)),
                bool_number(door_open.get(1).copied().unwrap_or(false)),
                bool_number(door_open.get(2).copied().unwrap_or(false)),
                bool_number(door_open.get(3).copied().unwrap_or(false)),
            ],
            Self::Light {
                head_light_state,
                turn_indicator_state,
                hazard_lights_on,
            } => [
                *head_light_state as f64,
                *turn_indicator_state as f64,
                bool_number(*hazard_lights_on),
                0.0,
                0.0,
                0.0,
            ],
            Self::TirePressure { pressures } => [
                pressures.first().copied().unwrap_or(0.0),
                pressures.get(1).copied().unwrap_or(0.0),
                pressures.get(2).copied().unwrap_or(0.0),
                pressures.get(3).copied().unwrap_or(0.0),
                pressures.len() as f64,
                0.0,
            ],
            Self::Accelerometer { x, y, z } | Self::Gyroscope { x, y, z } => {
                [*x, *y, *z, 0.0, 0.0, 0.0]
            }
            Self::GpsSatellite {
                in_use,
                in_view,
                listed,
            } => [
                *in_use as f64,
                *in_view as f64,
                *listed as f64,
                0.0,
                0.0,
                0.0,
            ],
            Self::TollCard { present } | Self::EnergyModel { present } => {
                [bool_number(*present), 0.0, 0.0, 0.0, 0.0, 0.0]
            }
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Location {
                latitude_deg,
                longitude_deg,
                accuracy_m,
                altitude_m,
                speed_mps,
                bearing_deg,
            } => format!(
                "kind={}({}) lat={:.7} lon={:.7} accuracy_m={:.3} altitude_m={:.2} speed_mps={:.3} bearing_deg={:.3}",
                self.kind_id(), self.name(), latitude_deg, longitude_deg, accuracy_m, altitude_m, speed_mps, bearing_deg
            ),
            Self::Compass {
                bearing_deg,
                pitch_deg,
                roll_deg,
            } => format!(
                "kind={}({}) bearing_deg={:.3} pitch_deg={:.3} roll_deg={:.3}",
                self.kind_id(), self.name(), bearing_deg, pitch_deg, roll_deg
            ),
            Self::Speed {
                speed_mps,
                cruise_engaged,
                cruise_set_speed_raw,
            } => format!(
                "kind={}({}) speed_mps={:.3} cruise_engaged={} cruise_set_speed_raw={}",
                self.kind_id(), self.name(), speed_mps, cruise_engaged, cruise_set_speed_raw
            ),
            Self::Rpm { rpm } => format!("kind={}({}) rpm={:.3}", self.kind_id(), self.name(), rpm),
            Self::Odometer {
                kilometers,
                trip_kilometers,
            } => format!(
                "kind={}({}) kilometers={:.1} trip_kilometers={:.1}",
                self.kind_id(), self.name(), kilometers, trip_kilometers
            ),
            Self::Fuel {
                level_raw,
                range_raw,
                low_fuel_warning,
            } => format!(
                "kind={}({}) level_raw={} range_raw={} low_fuel_warning={}",
                self.kind_id(), self.name(), level_raw, range_raw, low_fuel_warning
            ),
            Self::ParkingBrake { engaged } => format!(
                "kind={}({}) engaged={}",
                self.kind_id(), self.name(), engaged
            ),
            Self::Gear {
                aap_gear,
                android_gear,
            } => format!(
                "kind={}({}) aap_gear={} android_gear={:?}",
                self.kind_id(), self.name(), aap_gear, android_gear
            ),
            Self::Diagnostics { dtc } => format!(
                "kind={}({}) dtc_len={} dtc_fnv1a32=0x{:08x}",
                self.kind_id(), self.name(), dtc.len(), fnv1a32(dtc)
            ),
            Self::NightMode { enabled } => format!(
                "kind={}({}) enabled={}",
                self.kind_id(), self.name(), enabled
            ),
            Self::Environment {
                temperature_c,
                pressure,
                rain_raw,
            } => format!(
                "kind={}({}) temperature_c={:.3} pressure={:.3} rain_raw={}",
                self.kind_id(), self.name(), temperature_c, pressure, rain_raw
            ),
            Self::Hvac {
                target_temperature_c,
                current_temperature_c,
            } => format!(
                "kind={}({}) target_temperature_c={:.3} current_temperature_c={:.3}",
                self.kind_id(), self.name(), target_temperature_c, current_temperature_c
            ),
            Self::DrivingStatus { status } => format!(
                "kind={}({}) status=0x{:x}",
                self.kind_id(), self.name(), status
            ),
            Self::DeadReckoning {
                steering_angle_deg,
                wheel_speed_mps,
            } => format!(
                "kind={}({}) steering_angle_deg={:.1} wheel_speed_mps={:?}",
                self.kind_id(), self.name(), steering_angle_deg, wheel_speed_mps
            ),
            Self::Passenger { present } => format!(
                "kind={}({}) present={}",
                self.kind_id(), self.name(), present
            ),
            Self::Door {
                hood_open,
                trunk_open,
                door_open,
            } => format!(
                "kind={}({}) hood_open={} trunk_open={} door_open={:?}",
                self.kind_id(), self.name(), hood_open, trunk_open, door_open
            ),
            Self::Light {
                head_light_state,
                turn_indicator_state,
                hazard_lights_on,
            } => format!(
                "kind={}({}) head_light_state={} turn_indicator_state={} hazard_lights_on={}",
                self.kind_id(), self.name(), head_light_state, turn_indicator_state, hazard_lights_on
            ),
            Self::TirePressure { pressures } => format!(
                "kind={}({}) pressures={:?}",
                self.kind_id(), self.name(), pressures
            ),
            Self::Accelerometer { x, y, z } => format!(
                "kind={}({}) x={:.3} y={:.3} z={:.3}",
                self.kind_id(), self.name(), x, y, z
            ),
            Self::Gyroscope { x, y, z } => format!(
                "kind={}({}) x={:.3} y={:.3} z={:.3}",
                self.kind_id(), self.name(), x, y, z
            ),
            Self::GpsSatellite {
                in_use,
                in_view,
                listed,
            } => format!(
                "kind={}({}) in_use={} in_view={} listed={}",
                self.kind_id(), self.name(), in_use, in_view, listed
            ),
            Self::TollCard { present } => format!(
                "kind={}({}) present={}",
                self.kind_id(), self.name(), present
            ),
            Self::EnergyModel { present } => format!(
                "kind={}({}) present={} note=opaque_proto_not_mapped_yet",
                self.kind_id(), self.name(), present
            ),
        }
    }
}

fn bool_number(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for byte in bytes {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

#[derive(Debug, Clone)]
enum RawVhalValue {
    Bool(bool),
    I32(i32),
    F32(f32),
}

#[derive(Debug, Clone)]
struct RawVhalCommand {
    property_id: i32,
    area_id: i32,
    value: RawVhalValue,
}

#[derive(Debug)]
struct WasmHostState {
    limits: StoreLimits,
    commands: Vec<RawVhalCommand>,
    max_commands: usize,
    dropped_commands: u64,
}

impl WasmHostState {
    fn new(max_commands: usize) -> Self {
        Self {
            limits: StoreLimitsBuilder::new()
                .memory_size(MAX_WASM_LINEAR_MEMORY_BYTES)
                .table_elements(MAX_WASM_TABLE_ELEMENTS)
                .instances(2)
                .tables(1)
                .memories(1)
                .trap_on_grow_failure(true)
                .build(),
            commands: Vec::new(),
            max_commands: max_commands.max(1),
            dropped_commands: 0,
        }
    }

    fn push(&mut self, command: RawVhalCommand) -> i32 {
        if self.commands.len() >= self.max_commands {
            self.dropped_commands = self.dropped_commands.saturating_add(1);
            return -2;
        }
        self.commands.push(command);
        0
    }
}

struct SensorWasmEngine {
    store: Store<WasmHostState>,
    on_sensor: TypedFunc<(i32, f64, f64, f64, f64, f64, f64), i32>,
    fuel_per_event: u64,
    module_label: String,
}

impl SensorWasmEngine {
    fn new(
        wasm_spec: &str,
        fuel_per_event: u64,
        max_commands: usize,
    ) -> Result<(Self, i32), Box<dyn std::error::Error>> {
        let (module_label, module_bytes) = load_module_bytes(wasm_spec)?;

        let mut engine_config = Config::new();
        engine_config.consume_fuel(true);
        let engine = Engine::new(&engine_config)?;
        let module = Module::from_binary(&engine, &module_bytes)?;
        let mut linker = Linker::new(&engine);

        linker.func_wrap(
            "arda_vhal",
            "emit_bool",
            |mut caller: Caller<'_, WasmHostState>, property_id: i32, area_id: i32, value: i32| -> i32 {
                caller.data_mut().push(RawVhalCommand {
                    property_id,
                    area_id,
                    value: RawVhalValue::Bool(value != 0),
                })
            },
        )?;
        linker.func_wrap(
            "arda_vhal",
            "emit_i32",
            |mut caller: Caller<'_, WasmHostState>, property_id: i32, area_id: i32, value: i32| -> i32 {
                caller.data_mut().push(RawVhalCommand {
                    property_id,
                    area_id,
                    value: RawVhalValue::I32(value),
                })
            },
        )?;
        linker.func_wrap(
            "arda_vhal",
            "emit_f32",
            |mut caller: Caller<'_, WasmHostState>, property_id: i32, area_id: i32, value: f32| -> i32 {
                caller.data_mut().push(RawVhalCommand {
                    property_id,
                    area_id,
                    value: RawVhalValue::F32(value),
                })
            },
        )?;

        let mut store = Store::new(&engine, WasmHostState::new(max_commands));
        store.limiter(|state| &mut state.limits);
        store.set_fuel(fuel_per_event.max(1))?;
        let instance = linker.instantiate(&mut store, &module)?;
        let abi_version = instance
            .get_typed_func::<(), i32>(&mut store, "arda_abi_version")?
            .call(&mut store, ())?;
        if abi_version != SENSOR_WASM_ABI_VERSION {
            return Err(format!(
                "sensor wasm ABI mismatch: module={} expected={} actual={}",
                module_label, SENSOR_WASM_ABI_VERSION, abi_version
            )
            .into());
        }
        let on_sensor = instance.get_typed_func::<
            (i32, f64, f64, f64, f64, f64, f64),
            i32,
        >(&mut store, "arda_on_sensor")?;

        Ok((
            Self {
                store,
                on_sensor,
                fuel_per_event: fuel_per_event.max(1),
                module_label,
            },
            abi_version,
        ))
    }

    fn process_event(
        &mut self,
        event: &ArdaSensorEvent,
    ) -> Result<(i32, Vec<RawVhalCommand>, u64), Box<dyn std::error::Error>> {
        {
            let state = self.store.data_mut();
            state.commands.clear();
            state.dropped_commands = 0;
        }
        self.store.set_fuel(self.fuel_per_event)?;
        let [a, b, c, d, e, f] = event.abi_values();
        let status = match self
            .on_sensor
            .call(&mut self.store, (event.kind_id(), a, b, c, d, e, f))
        {
            Ok(status) => status,
            Err(err) => {
                let state = self.store.data_mut();
                state.commands.clear();
                state.dropped_commands = 0;
                return Err(err.into());
            }
        };
        let state = self.store.data_mut();
        let commands = std::mem::take(&mut state.commands);
        let dropped = std::mem::take(&mut state.dropped_commands);
        Ok((status, commands, dropped))
    }
}

fn load_module_bytes(wasm_spec: &str) -> Result<(String, Vec<u8>), Box<dyn std::error::Error>> {
    let normalized = wasm_spec.trim();
    if normalized.is_empty()
        || normalized.eq_ignore_ascii_case("builtin")
        || normalized.eq_ignore_ascii_case("default")
        || normalized.eq_ignore_ascii_case("embedded")
    {
        return Ok(("builtin:arda_vhal_default".to_string(), BUILTIN_WASM.to_vec()));
    }

    let path = Path::new(normalized);
    let metadata = fs::metadata(path)
        .map_err(|err| format!("cannot stat sensor wasm {}: {err}", path.display()))?;
    let module_len = usize::try_from(metadata.len())
        .map_err(|_| format!("sensor wasm {} is too large for this platform", path.display()))?;
    if module_len > MAX_WASM_MODULE_BYTES {
        return Err(format!(
            "sensor wasm {} exceeds max module bytes: actual={} max={}",
            path.display(), module_len, MAX_WASM_MODULE_BYTES
        )
        .into());
    }
    let bytes = fs::read(path)
        .map_err(|err| format!("cannot read sensor wasm {}: {err}", path.display()))?;
    Ok((format!("file:{}", path.display()), bytes))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VhalValueType {
    Bool,
    I32,
    F32,
}

#[derive(Debug, Clone)]
struct VhalCommand {
    property_id: i32,
    property_name: &'static str,
    area_id: i32,
    value: RawVhalValue,
}

impl VhalCommand {
    fn value_string(&self) -> String {
        match self.value {
            RawVhalValue::Bool(value) => value.to_string(),
            RawVhalValue::I32(value) => value.to_string(),
            RawVhalValue::F32(value) => format!("{value:.3}"),
        }
    }
}

fn property_rule(property_id: i32) -> Option<(&'static str, VhalValueType, f64, f64)> {
    match property_id {
        291_504_647 => Some(("PERF_VEHICLE_SPEED", VhalValueType::F32, -1.0, 1_000.0)),
        291_504_648 => Some(("PERF_VEHICLE_SPEED_DISPLAY", VhalValueType::F32, -1.0, 1_000.0)),
        291_504_901 => Some(("ENGINE_RPM", VhalValueType::F32, 0.0, 50_000.0)),
        291_504_644 => Some(("PERF_ODOMETER", VhalValueType::F32, 0.0, 2_000_000_000.0)),
        291_504_903 => Some(("FUEL_LEVEL", VhalValueType::F32, 0.0, 2_000_000_000.0)),
        291_504_904 => Some(("RANGE_REMAINING", VhalValueType::F32, -1.0, 2_000_000_000.0)),
        287_310_850 => Some(("PARKING_BRAKE_ON", VhalValueType::Bool, 0.0, 1.0)),
        289_408_001 => Some(("CURRENT_GEAR", VhalValueType::I32, 0.0, 0x1000 as f64)),
        289_408_000 => Some(("GEAR_SELECTION", VhalValueType::I32, 0.0, 0x1000 as f64)),
        287_310_855 => Some(("NIGHT_MODE", VhalValueType::Bool, 0.0, 1.0)),
        291_505_923 => Some(("ENV_OUTSIDE_TEMPERATURE", VhalValueType::F32, -100.0, 200.0)),
        _ => None,
    }
}

fn validate_command(raw: RawVhalCommand) -> Result<VhalCommand, String> {
    let Some((property_name, expected_type, min_value, max_value)) = property_rule(raw.property_id) else {
        return Err(format!("property_not_allowed id={}", raw.property_id));
    };
    if raw.area_id != 0 {
        return Err(format!(
            "area_not_allowed property={}({}) area={}",
            property_name, raw.property_id, raw.area_id
        ));
    }

    let (actual_type, numeric_value) = match raw.value {
        RawVhalValue::Bool(value) => (VhalValueType::Bool, if value { 1.0 } else { 0.0 }),
        RawVhalValue::I32(value) => (VhalValueType::I32, value as f64),
        RawVhalValue::F32(value) => (VhalValueType::F32, value as f64),
    };
    if actual_type != expected_type {
        return Err(format!(
            "type_mismatch property={}({}) expected={:?} actual={:?}",
            property_name, raw.property_id, expected_type, actual_type
        ));
    }
    if !numeric_value.is_finite() {
        return Err(format!(
            "non_finite property={}({})",
            property_name, raw.property_id
        ));
    }
    if numeric_value < min_value || numeric_value > max_value {
        return Err(format!(
            "range_reject property={}({}) value={} allowed={}..{}",
            property_name, raw.property_id, numeric_value, min_value, max_value
        ));
    }

    Ok(VhalCommand {
        property_id: raw.property_id,
        property_name,
        area_id: raw.area_id,
        value: raw.value,
    })
}

#[derive(Debug)]
struct VhalLastPush {
    property_id: i32,
    area_id: i32,
    at: Instant,
}

struct VhalHostAdapter {
    enabled: bool,
    min_interval: Duration,
    last_pushes: Vec<VhalLastPush>,
}

impl VhalHostAdapter {
    fn new(enabled: bool, min_interval_ms: u64) -> Self {
        Self {
            enabled,
            min_interval: Duration::from_millis(min_interval_ms),
            last_pushes: Vec::new(),
        }
    }

    fn dispatch(&mut self, event: &ArdaSensorEvent, command: &VhalCommand) -> String {
        let value = command.value_string();
        if !self.enabled {
            return format!(
                "SENSOR_WASM_VHAL dry_run event={} prop={}({}) area={} value={}",
                event.name(), command.property_name, command.property_id, command.area_id, value
            );
        }
        if !self.should_push(command.property_id, command.area_id) {
            return format!(
                "SENSOR_WASM_VHAL rate_limited event={} prop={}({}) area={} value={} min_ms={}",
                event.name(), command.property_name, command.property_id, command.area_id, value, self.min_interval.as_millis()
            );
        }

        let prop_id = command.property_id.to_string();
        let area_id = command.area_id.to_string();
        let cmd_args = [
            "car_service",
            "inject-vhal-event",
            prop_id.as_str(),
            area_id.as_str(),
            value.as_str(),
        ];
        let primary = run_command_capture("/system/bin/cmd", &cmd_args);
        self.mark_attempt(command.property_id, command.area_id);
        if primary.success {
            return format!(
                "SENSOR_WASM_VHAL applied event={} prop={}({}) area={} value={} cmd='cmd {}' status={} stdout='{}' stderr='{}'",
                event.name(), command.property_name, command.property_id, command.area_id, value,
                cmd_args.join(" "), primary.status, primary.stdout, primary.stderr
            );
        }

        let fallback_args = [
            "activity",
            "service",
            "com.android.car",
            "inject-vhal-event",
            prop_id.as_str(),
            value.as_str(),
        ];
        let fallback = run_command_capture("/system/bin/dumpsys", &fallback_args);
        format!(
            "SENSOR_WASM_VHAL {} event={} prop={}({}) area={} value={} primary_status={} primary_stderr='{}' fallback_status={} fallback_stdout='{}' fallback_stderr='{}'",
            if fallback.success { "applied_fallback" } else { "failed" },
            event.name(), command.property_name, command.property_id, command.area_id, value,
            primary.status, primary.stderr, fallback.status, fallback.stdout, fallback.stderr
        )
    }

    fn should_push(&self, property_id: i32, area_id: i32) -> bool {
        if self.min_interval.is_zero() {
            return true;
        }
        let now = Instant::now();
        self.last_pushes
            .iter()
            .find(|last| last.property_id == property_id && last.area_id == area_id)
            .map(|last| now.duration_since(last.at) >= self.min_interval)
            .unwrap_or(true)
    }

    fn mark_attempt(&mut self, property_id: i32, area_id: i32) {
        let now = Instant::now();
        if let Some(last) = self
            .last_pushes
            .iter_mut()
            .find(|last| last.property_id == property_id && last.area_id == area_id)
        {
            last.at = now;
        } else {
            self.last_pushes.push(VhalLastPush {
                property_id,
                area_id,
                at: now,
            });
        }
    }
}

struct CommandResult {
    success: bool,
    status: String,
    stdout: String,
    stderr: String,
}

fn run_command_capture(program: &str, args: &[&str]) -> CommandResult {
    match Command::new(program).args(args).output() {
        Ok(output) => CommandResult {
            success: output.status.success(),
            status: output.status.to_string(),
            stdout: compact_output(&output.stdout),
            stderr: compact_output(&output.stderr),
        },
        Err(err) => CommandResult {
            success: false,
            status: "spawn-error".to_string(),
            stdout: String::new(),
            stderr: err.to_string(),
        },
    }
}

fn compact_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim()
        .replace('\n', " | ")
}

#[derive(Debug, Clone)]
pub struct SensorPipelineConfig {
    pub wasm_spec: String,
    pub fuel_per_event: u64,
    pub max_commands_per_event: usize,
    pub vhal_enabled: bool,
    pub vhal_min_interval_ms: u64,
}

pub struct SensorPipeline {
    wasm: SensorWasmEngine,
    vhal: VhalHostAdapter,
    events_seen: u64,
    commands_generated: u64,
    commands_rejected: u64,
}

impl SensorPipeline {
    pub fn new(
        config: SensorPipelineConfig,
    ) -> Result<(Self, String), Box<dyn std::error::Error>> {
        let (wasm, abi_version) = SensorWasmEngine::new(
            &config.wasm_spec,
            config.fuel_per_event,
            config.max_commands_per_event,
        )?;
        let ready = format!(
            "SENSOR_WASM_ENGINE ready abi={} module={} fuel_per_event={} max_commands_per_event={} memory_limit_bytes={} table_limit_elements={} vhal_enabled={} vhal_min_ms={} wasi=false imports=arda_vhal.emit_bool,emit_i32,emit_f32",
            abi_version,
            wasm.module_label,
            config.fuel_per_event.max(1),
            config.max_commands_per_event.max(1),
            MAX_WASM_LINEAR_MEMORY_BYTES,
            MAX_WASM_TABLE_ELEMENTS,
            config.vhal_enabled,
            config.vhal_min_interval_ms,
        );
        Ok((
            Self {
                wasm,
                vhal: VhalHostAdapter::new(config.vhal_enabled, config.vhal_min_interval_ms),
                events_seen: 0,
                commands_generated: 0,
                commands_rejected: 0,
            },
            ready,
        ))
    }

    pub fn process_events(&mut self, events: &[ArdaSensorEvent]) -> Vec<String> {
        let mut lines = Vec::new();
        for event in events {
            self.events_seen = self.events_seen.saturating_add(1);
            match self.wasm.process_event(event) {
                Ok((status, commands, dropped)) => {
                    if status != 0 {
                        lines.push(format!(
                            "SENSOR_WASM_EVENT rejected event={} kind={} guest_status={} commands_discarded={}",
                            event.name(), event.kind_id(), status, commands.len()
                        ));
                        continue;
                    }
                    lines.push(format!(
                        "SENSOR_WASM_EVENT event={} kind={} commands={} dropped_by_host={} total_events={}",
                        event.name(), event.kind_id(), commands.len(), dropped, self.events_seen
                    ));
                    if dropped > 0 {
                        lines.push(format!(
                            "SENSOR_WASM_LIMIT event={} dropped_commands={} reason=max_commands_per_event",
                            event.name(), dropped
                        ));
                    }
                    for raw in commands {
                        self.commands_generated = self.commands_generated.saturating_add(1);
                        match validate_command(raw) {
                            Ok(command) => {
                                lines.push(self.vhal.dispatch(event, &command));
                            }
                            Err(reason) => {
                                self.commands_rejected = self.commands_rejected.saturating_add(1);
                                lines.push(format!(
                                    "SENSOR_WASM_VHAL rejected event={} reason={} total_rejected={}",
                                    event.name(), reason, self.commands_rejected
                                ));
                            }
                        }
                    }
                }
                Err(err) => {
                    lines.push(format!(
                        "SENSOR_WASM_TRAP event={} kind={} error={} commands_discarded=true",
                        event.name(), event.kind_id(), err
                    ));
                }
            }
        }
        lines
    }
}

pub fn smoke_events() -> Vec<ArdaSensorEvent> {
    vec![
        ArdaSensorEvent::Speed {
            speed_mps: 13.8,
            cruise_engaged: false,
            cruise_set_speed_raw: 0,
        },
        ArdaSensorEvent::NightMode { enabled: false },
        ArdaSensorEvent::Gear {
            aap_gear: 100,
            android_gear: Some(0x0008),
        },
        ArdaSensorEvent::Environment {
            temperature_c: 21.5,
            pressure: 101.325,
            rain_raw: 0,
        },
    ]
}
