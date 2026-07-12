use crate::sensor::ArdaSensorEvent;

pub const AAP_SENSOR_SOURCE_ID: u32 = 1;

pub const AREA_GLOBAL: i32 = 0;
// ARDA vehicle values now use Android VHAL area IDs directly. For front seats,
// VehicleAreaSeat::ROW_1_LEFT is 0x1 and ROW_1_RIGHT is 0x4.
pub const AREA_DRIVER: i32 = 1;
pub const AREA_PASSENGER: i32 = 4;

pub const VHAL_AREA_ROW_1_LEFT: i32 = AREA_DRIVER;
pub const VHAL_AREA_ROW_1_RIGHT: i32 = AREA_PASSENGER;

pub const PROP_PERF_VEHICLE_SPEED: i32 = 291_504_647;
pub const PROP_PERF_VEHICLE_SPEED_DISPLAY: i32 = 291_504_648;
pub const PROP_ENGINE_RPM: i32 = 291_504_901;
pub const PROP_PERF_ODOMETER: i32 = 291_504_644;
pub const PROP_FUEL_LEVEL: i32 = 291_504_903;
pub const PROP_RANGE_REMAINING: i32 = 291_504_904;
pub const PROP_FUEL_LEVEL_LOW: i32 = 287_310_853;
pub const PROP_EV_BATTERY_LEVEL: i32 = 291_504_905;
pub const PROP_EV_CURRENT_BATTERY_CAPACITY: i32 = 291_504_909;
pub const PROP_PARKING_BRAKE_ON: i32 = 287_310_850;
pub const PROP_CURRENT_GEAR: i32 = 289_408_001;
pub const PROP_GEAR_SELECTION: i32 = 289_408_000;
pub const PROP_NIGHT_MODE: i32 = 287_310_855;
pub const PROP_ELECTRONIC_TOLL_COLLECTION_CARD_STATUS: i32 = 289_410_874;
pub const ETC_CARD_STATUS_UNKNOWN: i32 = 0;
pub const ETC_CARD_STATUS_NOT_INSERTED: i32 = 3;
pub const PROP_ENV_OUTSIDE_TEMPERATURE: i32 = 291_505_923;
pub const PROP_HVAC_FAN_SPEED: i32 = 356_517_120;
pub const PROP_HVAC_TEMPERATURE_CURRENT: i32 = 358_614_274;
pub const PROP_HVAC_TEMPERATURE_SET: i32 = 358_614_275;
pub const PROP_HVAC_POWER_ON: i32 = 354_419_984;

pub const VALUE_BOOL: i32 = 1;
pub const VALUE_I32: i32 = 2;
pub const VALUE_I64: i32 = 3;
pub const VALUE_F32: i32 = 4;
pub const VALUE_F64: i32 = 5;
pub const VALUE_BYTES: i32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    Powertrain,
    Energy,
    HvacState,
    BodyState,
    ChargingState,
    HvacControl,
    BodyControl,
    ChargingControl,
}

impl Capability {
    pub fn id(self) -> i32 {
        match self {
            Self::Powertrain => 1,
            Self::Energy => 2,
            Self::HvacState => 3,
            Self::BodyState => 4,
            Self::ChargingState => 5,
            Self::HvacControl => 16,
            Self::BodyControl => 17,
            Self::ChargingControl => 18,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Powertrain => "POWERTRAIN",
            Self::Energy => "ENERGY",
            Self::HvacState => "HVAC_STATE",
            Self::BodyState => "BODY_STATE",
            Self::ChargingState => "CHARGING_STATE",
            Self::HvacControl => "HVAC_CONTROL",
            Self::BodyControl => "BODY_CONTROL",
            Self::ChargingControl => "CHARGING_CONTROL",
        }
    }

    pub fn from_id(value: i32) -> Option<Self> {
        match value {
            1 => Some(Self::Powertrain),
            2 => Some(Self::Energy),
            3 => Some(Self::HvacState),
            4 => Some(Self::BodyState),
            5 => Some(Self::ChargingState),
            16 => Some(Self::HvacControl),
            17 => Some(Self::BodyControl),
            18 => Some(Self::ChargingControl),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VehicleValue {
    Bool(bool),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Bytes(Vec<u8>),
}

impl VehicleValue {
    pub fn value_type_id(&self) -> i32 {
        match self {
            Self::Bool(_) => VALUE_BOOL,
            Self::I32(_) => VALUE_I32,
            Self::I64(_) => VALUE_I64,
            Self::F32(_) => VALUE_F32,
            Self::F64(_) => VALUE_F64,
            Self::Bytes(_) => VALUE_BYTES,
        }
    }

    pub fn i64_lane(&self) -> i64 {
        match self {
            Self::Bool(value) => i64::from(*value),
            Self::I32(value) => i64::from(*value),
            Self::I64(value) => *value,
            Self::F32(_) | Self::F64(_) | Self::Bytes(_) => 0,
        }
    }

    pub fn f64_lane(&self) -> f64 {
        match self {
            Self::F32(value) => f64::from(*value),
            Self::F64(value) => *value,
            Self::Bool(_) | Self::I32(_) | Self::I64(_) | Self::Bytes(_) => 0.0,
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::Bool(value) => value.to_string(),
            Self::I32(value) => value.to_string(),
            Self::I64(value) => value.to_string(),
            Self::F32(value) => format!("{value:.3}"),
            Self::F64(value) => format!("{value:.6}"),
            Self::Bytes(value) => format!("bytes(len={})", value.len()),
        }
    }

    pub fn is_finite(&self) -> bool {
        match self {
            Self::F32(value) => value.is_finite(),
            Self::F64(value) => value.is_finite(),
            _ => true,
        }
    }

    pub fn numeric_f64(&self) -> Option<f64> {
        match self {
            Self::Bool(value) => Some(if *value { 1.0 } else { 0.0 }),
            Self::I32(value) => Some(f64::from(*value)),
            Self::I64(value) => Some(*value as f64),
            Self::F32(value) => Some(f64::from(*value)),
            Self::F64(value) => Some(*value),
            Self::Bytes(_) => None,
        }
    }
}

// Vehicle sources publish Android VehicleProperty IDs directly. Third-party
// adapters do not need a second ARDA-specific signal-ID namespace.
#[derive(Debug, Clone, PartialEq)]
pub struct VehicleSignal {
    pub source_id: u32,
    pub property_id: i32,
    pub area_id: i32,
    pub value: VehicleValue,
    pub timestamp_nanos: i64,
}

impl VehicleSignal {
    pub fn with_source(mut self, source_id: u32) -> Self {
        self.source_id = source_id;
        self
    }

    pub fn summary(&self) -> String {
        format!(
            "source={} property={}({}) area={} type={} value={} ts={}",
            self.source_id,
            property_name(self.property_id),
            self.property_id,
            self.area_id,
            self.value.value_type_id(),
            self.value.summary(),
            self.timestamp_nanos,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VhalEvent {
    pub property_id: i32,
    pub area_id: i32,
    pub value: VehicleValue,
    pub timestamp_nanos: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VhalSetRequest {
    pub request_id: u64,
    pub property_id: i32,
    pub area_id: i32,
    pub value: VehicleValue,
    pub timestamp_nanos: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VehicleCommand {
    pub command_id: u64,
    pub source_id: u32,
    pub property_id: i32,
    pub area_id: i32,
    pub value: VehicleValue,
    pub timestamp_nanos: i64,
}

pub fn property_name(property_id: i32) -> &'static str {
    match property_id {
        PROP_PERF_VEHICLE_SPEED => "PERF_VEHICLE_SPEED",
        PROP_PERF_VEHICLE_SPEED_DISPLAY => "PERF_VEHICLE_SPEED_DISPLAY",
        PROP_ENGINE_RPM => "ENGINE_RPM",
        PROP_PERF_ODOMETER => "PERF_ODOMETER",
        PROP_FUEL_LEVEL => "FUEL_LEVEL",
        PROP_RANGE_REMAINING => "RANGE_REMAINING",
        PROP_FUEL_LEVEL_LOW => "FUEL_LEVEL_LOW",
        PROP_EV_BATTERY_LEVEL => "EV_BATTERY_LEVEL",
        PROP_EV_CURRENT_BATTERY_CAPACITY => "EV_CURRENT_BATTERY_CAPACITY",
        PROP_PARKING_BRAKE_ON => "PARKING_BRAKE_ON",
        PROP_CURRENT_GEAR => "CURRENT_GEAR",
        PROP_GEAR_SELECTION => "GEAR_SELECTION",
        PROP_NIGHT_MODE => "NIGHT_MODE",
        PROP_ELECTRONIC_TOLL_COLLECTION_CARD_STATUS => {
            "ELECTRONIC_TOLL_COLLECTION_CARD_STATUS"
        },
        PROP_ENV_OUTSIDE_TEMPERATURE => "ENV_OUTSIDE_TEMPERATURE",
        PROP_HVAC_FAN_SPEED => "HVAC_FAN_SPEED",
        PROP_HVAC_TEMPERATURE_CURRENT => "HVAC_TEMPERATURE_CURRENT",
        PROP_HVAC_TEMPERATURE_SET => "HVAC_TEMPERATURE_SET",
        PROP_HVAC_POWER_ON => "HVAC_POWER_ON",
        _ => "UNKNOWN_PROPERTY",
    }
}

#[derive(Clone, Copy)]
struct PropertyRule {
    property_id: i32,
    value_type: i32,
    publish_capability: Capability,
    command_capability: Option<Capability>,
    min_value: f64,
    max_value: f64,
    allow_global: bool,
    allow_seat: bool,
}

impl PropertyRule {
    const fn f32(
        property_id: i32,
        publish_capability: Capability,
        command_capability: Option<Capability>,
        min_value: f64,
        max_value: f64,
        allow_global: bool,
        allow_seat: bool,
    ) -> Self {
        Self {
            property_id,
            value_type: VALUE_F32,
            publish_capability,
            command_capability,
            min_value,
            max_value,
            allow_global,
            allow_seat,
        }
    }

    const fn bool(
        property_id: i32,
        publish_capability: Capability,
        command_capability: Option<Capability>,
        allow_global: bool,
        allow_seat: bool,
    ) -> Self {
        Self {
            property_id,
            value_type: VALUE_BOOL,
            publish_capability,
            command_capability,
            min_value: 0.0,
            max_value: 1.0,
            allow_global,
            allow_seat,
        }
    }

    const fn i32(
        property_id: i32,
        publish_capability: Capability,
        command_capability: Option<Capability>,
        min_value: f64,
        max_value: f64,
        allow_global: bool,
        allow_seat: bool,
    ) -> Self {
        Self {
            property_id,
            value_type: VALUE_I32,
            publish_capability,
            command_capability,
            min_value,
            max_value,
            allow_global,
            allow_seat,
        }
    }
}

// Phase 1 does not yet have the real IVehicleHardware backend from which to load
// VehiclePropConfig dynamically. Keep the currently supported property subset in
// one declarative registry. This is authorization/type/area metadata; it is not a
// vehicle-signal-to-VHAL mapping table. Once the AIDL VHAL backend lands, this
// registry can be built from the supported VehiclePropConfig set plus ARDA grants.
const SUPPORTED_PROPERTY_RULES: &[PropertyRule] = &[
    PropertyRule::f32(
        PROP_PERF_VEHICLE_SPEED,
        Capability::Powertrain,
        None,
        -1.0,
        1_000.0,
        true,
        false,
    ),
    PropertyRule::f32(
        PROP_PERF_VEHICLE_SPEED_DISPLAY,
        Capability::Powertrain,
        None,
        -1.0,
        1_000.0,
        true,
        false,
    ),
    PropertyRule::f32(
        PROP_ENGINE_RPM,
        Capability::Powertrain,
        None,
        0.0,
        50_000.0,
        true,
        false,
    ),
    PropertyRule::f32(
        PROP_PERF_ODOMETER,
        Capability::Powertrain,
        None,
        0.0,
        2_000_000_000.0,
        true,
        false,
    ),
    PropertyRule::f32(
        PROP_FUEL_LEVEL,
        Capability::Energy,
        None,
        0.0,
        2_000_000_000.0,
        true,
        false,
    ),
    PropertyRule::f32(
        PROP_RANGE_REMAINING,
        Capability::Energy,
        None,
        -1.0,
        2_000_000_000.0,
        true,
        false,
    ),
    PropertyRule::bool(
        PROP_FUEL_LEVEL_LOW,
        Capability::Energy,
        None,
        true,
        false,
    ),
    PropertyRule::f32(
        PROP_EV_BATTERY_LEVEL,
        Capability::Energy,
        None,
        0.0,
        10_000_000.0,
        true,
        false,
    ),
    PropertyRule::f32(
        PROP_EV_CURRENT_BATTERY_CAPACITY,
        Capability::Energy,
        None,
        0.0,
        10_000_000.0,
        true,
        false,
    ),
    PropertyRule::bool(
        PROP_PARKING_BRAKE_ON,
        Capability::Powertrain,
        None,
        true,
        false,
    ),
    PropertyRule::i32(
        PROP_CURRENT_GEAR,
        Capability::Powertrain,
        None,
        0.0,
        4096.0,
        true,
        false,
    ),
    PropertyRule::i32(
        PROP_GEAR_SELECTION,
        Capability::Powertrain,
        None,
        0.0,
        4096.0,
        true,
        false,
    ),
    PropertyRule::bool(
        PROP_NIGHT_MODE,
        Capability::Powertrain,
        None,
        true,
        false,
    ),
    PropertyRule::i32(
        PROP_ELECTRONIC_TOLL_COLLECTION_CARD_STATUS,
        Capability::BodyState,
        None,
        0.0,
        3.0,
        true,
        false,
    ),
    PropertyRule::f32(
        PROP_ENV_OUTSIDE_TEMPERATURE,
        Capability::Powertrain,
        None,
        -100.0,
        200.0,
        true,
        false,
    ),
    PropertyRule::i32(
        PROP_HVAC_FAN_SPEED,
        Capability::HvacState,
        Some(Capability::HvacControl),
        0.0,
        100.0,
        false,
        true,
    ),
    PropertyRule::f32(
        PROP_HVAC_TEMPERATURE_CURRENT,
        Capability::HvacState,
        None,
        -40.0,
        100.0,
        false,
        true,
    ),
    PropertyRule::f32(
        PROP_HVAC_TEMPERATURE_SET,
        Capability::HvacState,
        Some(Capability::HvacControl),
        -40.0,
        100.0,
        false,
        true,
    ),
    PropertyRule::bool(
        PROP_HVAC_POWER_ON,
        Capability::HvacState,
        Some(Capability::HvacControl),
        false,
        true,
    ),
];

fn property_rule(property_id: i32) -> Option<PropertyRule> {
    SUPPORTED_PROPERTY_RULES
        .iter()
        .copied()
        .find(|rule| rule.property_id == property_id)
}

pub fn publish_capability_for_property(property_id: i32) -> Option<Capability> {
    property_rule(property_id).map(|rule| rule.publish_capability)
}

pub fn command_capability_for_property(property_id: i32) -> Option<Capability> {
    property_rule(property_id).and_then(|rule| rule.command_capability)
}

pub fn validate_vehicle_signal(signal: &VehicleSignal) -> Result<Capability, String> {
    if signal.source_id == 0 {
        return Err("source_id 0 is reserved".to_string());
    }
    if signal.source_id > i32::MAX as u32 {
        return Err(format!(
            "source_id_out_of_wasm_range source_id={} max={}",
            signal.source_id,
            i32::MAX,
        ));
    }
    let Some(rule) = property_rule(signal.property_id) else {
        return Err(format!("vhal_property_not_allowed id={}", signal.property_id));
    };
    validate_property_rule(signal.property_id, signal.area_id, &signal.value, rule)?;
    Ok(rule.publish_capability)
}

pub fn validate_vehicle_command(command: &VehicleCommand) -> Result<Capability, String> {
    let Some(rule) = property_rule(command.property_id) else {
        return Err(format!("vhal_command_property_not_allowed id={}", command.property_id));
    };
    validate_property_rule(command.property_id, command.area_id, &command.value, rule)?;
    let Some(capability) = rule.command_capability else {
        return Err(format!(
            "vhal_property_not_writable property={}({})",
            property_name(command.property_id),
            command.property_id,
        ));
    };
    Ok(capability)
}

pub fn validate_vhal_event(event: &VhalEvent) -> Result<(), String> {
    let Some(rule) = property_rule(event.property_id) else {
        return Err(format!("vhal_property_not_allowed id={}", event.property_id));
    };
    validate_property_rule(event.property_id, event.area_id, &event.value, rule)
}

pub fn validate_vhal_set_request(request: &VhalSetRequest) -> Result<(), String> {
    let Some(rule) = property_rule(request.property_id) else {
        return Err(format!("vhal_set_property_not_allowed id={}", request.property_id));
    };
    if rule.command_capability.is_none() {
        return Err(format!(
            "vhal_property_not_writable property={}({})",
            property_name(request.property_id),
            request.property_id,
        ));
    }
    validate_property_rule(request.property_id, request.area_id, &request.value, rule)
}

fn validate_property_rule(
    property_id: i32,
    area_id: i32,
    value: &VehicleValue,
    rule: PropertyRule,
) -> Result<(), String> {
    if value.value_type_id() != rule.value_type {
        return Err(format!(
            "vhal_type_mismatch property={}({}) expected={} actual={}",
            property_name(property_id),
            property_id,
            rule.value_type,
            value.value_type_id(),
        ));
    }
    let area_ok = (rule.allow_global && area_id == AREA_GLOBAL)
        || (rule.allow_seat && matches!(area_id, AREA_DRIVER | AREA_PASSENGER));
    if !area_ok {
        return Err(format!(
            "vhal_area_reject property={}({}) area={}",
            property_name(property_id),
            property_id,
            area_id,
        ));
    }
    if !value.is_finite() {
        return Err(format!(
            "vhal_non_finite property={}",
            property_name(property_id),
        ));
    }
    let Some(numeric) = value.numeric_f64() else {
        return Err(format!(
            "vhal_numeric_required property={}",
            property_name(property_id),
        ));
    };
    if numeric < rule.min_value || numeric > rule.max_value {
        return Err(format!(
            "vhal_range_reject property={} value={} allowed={}..{}",
            property_name(property_id),
            numeric,
            rule.min_value,
            rule.max_value,
        ));
    }
    Ok(())
}

// AAP/HU SensorSource is a built-in adapter. Its documented, well-understood
// sensor types are translated to standard Android VehicleProperty IDs here in
// Rust. The default Wasm policy therefore does not contain a property-by-property
// if/else chain for HU sensors.
pub fn sensor_event_to_vehicle_signals(event: &ArdaSensorEvent) -> Vec<VehicleSignal> {
    let timestamp_nanos = monotonic_nanos();
    let make = |property_id, area_id, value| VehicleSignal {
        source_id: AAP_SENSOR_SOURCE_ID,
        property_id: property_id,
        area_id,
        value,
        timestamp_nanos,
    };

    match event {
        // Location intentionally stays outside VHAL/Wasmtime. It will be sent to
        // Android GPS_PROVIDER through the dedicated ARDA location bridge.
        ArdaSensorEvent::Location { .. } => Vec::new(),
        // AAP speed_e3 is normalized to m/s and represents actual vehicle
        // speed. It is not a speedometer/display-adjusted value.
        ArdaSensorEvent::Speed { speed_mps, .. } => vec![make(
            PROP_PERF_VEHICLE_SPEED,
            AREA_GLOBAL,
            VehicleValue::F32(*speed_mps as f32),
        )],
        ArdaSensorEvent::Rpm { rpm } => vec![make(
            PROP_ENGINE_RPM,
            AREA_GLOBAL,
            VehicleValue::F32(*rpm as f32),
        )],
        ArdaSensorEvent::Odometer { kilometers, .. } => vec![make(
            PROP_PERF_ODOMETER,
            AREA_GLOBAL,
            VehicleValue::F32(*kilometers as f32),
        )],
        // AAP FuelData does not define fuel_level/range units. Do not pretend
        // those raw values are VHAL milliliters/meters. The optional
        // low_fuel_warning field does exactly match Android's dashboard warning
        // semantics, but only publish it when the proto field is present.
        ArdaSensorEvent::Fuel {
            low_fuel_warning,
            low_fuel_warning_present,
            ..
        } => {
            if *low_fuel_warning_present {
                vec![make(
                    PROP_FUEL_LEVEL_LOW,
                    AREA_GLOBAL,
                    VehicleValue::Bool(*low_fuel_warning),
                )]
            } else {
                Vec::new()
            }
        }
        ArdaSensorEvent::ParkingBrake { engaged } => vec![make(
            PROP_PARKING_BRAKE_ON,
            AREA_GLOBAL,
            VehicleValue::Bool(*engaged),
        )],
        // AAP's legacy gear sensor semantics map to selected gear. A selected
        // DRIVE value does not identify the transmission's actual numbered gear.
        ArdaSensorEvent::Gear { android_gear, .. } => android_gear
            .map(|gear| {
                vec![make(
                    PROP_GEAR_SELECTION,
                    AREA_GLOBAL,
                    VehicleValue::I32(gear),
                )]
            })
            .unwrap_or_default(),
        ArdaSensorEvent::NightMode {
            enabled,
            enabled_present,
        } => {
            if *enabled_present {
                vec![make(
                    PROP_NIGHT_MODE,
                    AREA_GLOBAL,
                    VehicleValue::Bool(*enabled),
                )]
            } else {
                Vec::new()
            }
        }
        ArdaSensorEvent::Environment { temperature_c, .. } => vec![make(
            PROP_ENV_OUTSIDE_TEMPERATURE,
            AREA_GLOBAL,
            VehicleValue::F32(*temperature_c as f32),
        )],
        ArdaSensorEvent::Hvac {
            target_temperature_c,
            current_temperature_c,
        } => vec![
            make(
                PROP_HVAC_TEMPERATURE_SET,
                AREA_DRIVER,
                VehicleValue::F32(*target_temperature_c as f32),
            ),
            make(
                PROP_HVAC_TEMPERATURE_CURRENT,
                AREA_DRIVER,
                VehicleValue::F32(*current_temperature_c as f32),
            ),
        ],
        // AAP TollCardData only says whether a card is present. Presence does
        // not prove VALID or INVALID status. Map absent to NOT_INSERTED and
        // present to UNKNOWN without fabricating card validity.
        ArdaSensorEvent::TollCard { present } => vec![make(
            PROP_ELECTRONIC_TOLL_COLLECTION_CARD_STATUS,
            AREA_GLOBAL,
            VehicleValue::I32(if *present {
                ETC_CARD_STATUS_UNKNOWN
            } else {
                ETC_CARD_STATUS_NOT_INSERTED
            }),
        )],
        // Compass, driving restrictions, accelerometer, gyroscope and AAP GPS
        // satellite metadata do not have exact standard VehicleProperty
        // semantics. Keep them out of VHAL rather than inventing a property.
        _ => Vec::new(),
    }
}

pub fn monotonic_nanos() -> i64 {
    use std::time::Instant;
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    i64::try_from(start.elapsed().as_nanos()).unwrap_or(i64::MAX)
}

pub fn smoke_vehicle_signals() -> Vec<VehicleSignal> {
    let ts = monotonic_nanos();
    vec![
        VehicleSignal {
            source_id: AAP_SENSOR_SOURCE_ID,
            property_id: PROP_PERF_VEHICLE_SPEED,
            area_id: AREA_GLOBAL,
            value: VehicleValue::F32(13.8),
            timestamp_nanos: ts,
        },
        VehicleSignal {
            source_id: AAP_SENSOR_SOURCE_ID,
            property_id: PROP_FUEL_LEVEL,
            area_id: AREA_GLOBAL,
            // VehiclePropertyIds.FUEL_LEVEL is milliliters.
            value: VehicleValue::F32(32_500.0),
            timestamp_nanos: ts,
        },
        VehicleSignal {
            source_id: AAP_SENSOR_SOURCE_ID,
            property_id: PROP_EV_CURRENT_BATTERY_CAPACITY,
            area_id: AREA_GLOBAL,
            value: VehicleValue::F32(74_000.0),
            timestamp_nanos: ts,
        },
        VehicleSignal {
            source_id: AAP_SENSOR_SOURCE_ID,
            property_id: PROP_EV_BATTERY_LEVEL,
            area_id: AREA_GLOBAL,
            value: VehicleValue::F32(59_940.0),
            timestamp_nanos: ts,
        },
        VehicleSignal {
            source_id: AAP_SENSOR_SOURCE_ID,
            property_id: PROP_HVAC_TEMPERATURE_SET,
            area_id: AREA_DRIVER,
            value: VehicleValue::F32(21.5),
            timestamp_nanos: ts,
        },
    ]
}
