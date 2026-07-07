use crate::vehicle::client::SensorRouterPublisher;
use crate::vehicle::model::{
    property_name, sensor_event_to_vehicle_signals, VehicleSignal,
};
use crate::vehicle::policy::{ready_line, VehiclePolicyEngine};
use crate::vehicle::wire::DEFAULT_ROUTER_SOCKET;

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
pub struct SensorPipelineConfig {
    pub wasm_spec: String,
    pub fuel_per_event: u64,
    pub max_commands_per_event: usize,
    pub vhal_enabled: bool,
    pub vhal_min_interval_ms: u64,
}

enum SensorPipelineMode {
    Router(SensorRouterPublisher),
    DryRun(VehiclePolicyEngine),
}

pub struct SensorPipeline {
    mode: SensorPipelineMode,
    events_seen: u64,
    signals_seen: u64,
}

impl SensorPipeline {
    pub fn new(
        config: SensorPipelineConfig,
    ) -> Result<(Self, String), Box<dyn std::error::Error>> {
        if config.vhal_enabled {
            let publisher = SensorRouterPublisher::connect(DEFAULT_ROUTER_SOCKET, 1)?;
            let ready = format!(
                "SENSOR_VEHICLE_ROUTER ready socket={} source_id=1 source=AAP_SensorSource priority=10 publish=POWERTRAIN,HVAC_STATE direct_vhal_shell=false wasm_location=routerd legacy_vhal_min_ms_ignored={}",
                DEFAULT_ROUTER_SOCKET,
                config.vhal_min_interval_ms,
            );
            return Ok((
                Self {
                    mode: SensorPipelineMode::Router(publisher),
                    events_seen: 0,
                    signals_seen: 0,
                },
                ready,
            ));
        }

        let (policy, abi_version) = VehiclePolicyEngine::new(
            &config.wasm_spec,
            config.fuel_per_event,
            config.max_commands_per_event,
        )?;
        let ready = format!(
            "{} sensor_mode=dry_run direct_vhal_shell=false",
            ready_line(
                &policy,
                abi_version,
                config.fuel_per_event,
                config.max_commands_per_event,
            )
        );
        Ok((
            Self {
                mode: SensorPipelineMode::DryRun(policy),
                events_seen: 0,
                signals_seen: 0,
            },
            ready,
        ))
    }

    pub fn process_events(&mut self, events: &[ArdaSensorEvent]) -> Vec<String> {
        let mut lines = Vec::new();
        for event in events {
            self.events_seen = self.events_seen.saturating_add(1);
            let signals = sensor_event_to_vehicle_signals(event);
            if signals.is_empty() {
                lines.push(format!(
                    "SENSOR_VEHICLE_PROPERTY_SKIP event={} kind={} reason=no_vhal_property_mapping total_events={}",
                    event.name(),
                    event.kind_id(),
                    self.events_seen,
                ));
                continue;
            }
            self.signals_seen = self.signals_seen.saturating_add(signals.len() as u64);
            match &mut self.mode {
                SensorPipelineMode::Router(publisher) => {
                    for signal in &signals {
                        lines.push(format!(
                            "SENSOR_VEHICLE_PROPERTY event={} property={}({}) area={} value={}",
                            event.name(),
                            property_name(signal.property_id),
                            signal.property_id,
                            signal.area_id,
                            signal.value.summary(),
                        ));
                    }
                    match publisher.publish(&signals) {
                        Ok(()) => lines.push(format!(
                            "SENSOR_VEHICLE_ROUTER published event={} properties={} total_properties={}",
                            event.name(),
                            signals.len(),
                            self.signals_seen,
                        )),
                        Err(err) => lines.push(format!(
                            "SENSOR_VEHICLE_ROUTER failed event={} properties={} error={} fail_closed=true direct_vhal_fallback=false",
                            event.name(),
                            signals.len(),
                            err,
                        )),
                    }
                }
                SensorPipelineMode::DryRun(policy) => {
                    for signal in signals {
                        lines.extend(process_dry_run_signal(policy, event, &signal));
                    }
                }
            }
        }
        lines
    }
}

fn process_dry_run_signal(
    policy: &mut VehiclePolicyEngine,
    event: &ArdaSensorEvent,
    signal: &VehicleSignal,
) -> Vec<String> {
    let mut lines = vec![format!(
        "SENSOR_WASM_EVENT event={} kind={} property={}({}) area={} value={}",
        event.name(),
        event.kind_id(),
        property_name(signal.property_id),
        signal.property_id,
        signal.area_id,
        signal.value.summary(),
    )];
    match policy.process_vehicle_signal(signal) {
        Ok(output) => {
            if output.status != 0 {
                lines.push(format!(
                    "SENSOR_WASM_EVENT rejected event={} property={} guest_status={} outputs_discarded=true",
                    event.name(),
                    property_name(signal.property_id),
                    output.status,
                ));
                return lines;
            }
            if output.dropped_outputs > 0 {
                lines.push(format!(
                    "SENSOR_WASM_LIMIT event={} property={} dropped_outputs={} reason=max_outputs_per_call",
                    event.name(),
                    property_name(signal.property_id),
                    output.dropped_outputs,
                ));
            }
            for vhal_event in output.vhal_events {
                lines.push(format!(
                    "SENSOR_WASM_VHAL dry_run event={} property={} prop={}({}) area={} value={}",
                    event.name(),
                    property_name(signal.property_id),
                    property_name(vhal_event.property_id),
                    vhal_event.property_id,
                    vhal_event.area_id,
                    vhal_event.value.summary(),
                ));
            }
            for command in output.vehicle_commands {
                lines.push(format!(
                    "SENSOR_WASM_VEHICLE unexpected_command event={} property={} command_property={}({}) area={} value={}",
                    event.name(),
                    property_name(signal.property_id),
                    property_name(command.property_id),
                    command.property_id,
                    command.area_id,
                    command.value.summary(),
                ));
            }
        }
        Err(err) => lines.push(format!(
            "SENSOR_WASM_TRAP event={} kind={} property={} error={} outputs_discarded=true",
            event.name(),
            event.kind_id(),
            property_name(signal.property_id),
            err,
        )),
    }
    lines
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
