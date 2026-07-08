use std::thread;
use std::time::Duration;

use super::client::{make_vhal_set_f32, RouterWireClient};
use super::model::{
    monotonic_nanos, property_name, smoke_vehicle_signals, Capability, VehicleSignal,
    VehicleValue, VhalSetRequest, AREA_DRIVER, AREA_GLOBAL, PROP_EV_BATTERY_LEVEL,
    PROP_EV_CURRENT_BATTERY_CAPACITY, PROP_FUEL_LEVEL, PROP_HVAC_TEMPERATURE_SET,
    PROP_PERF_VEHICLE_SPEED,
};
use super::policy::{ready_line, VehiclePolicyEngine};
use super::wire::pb;

pub fn policy_smoke(
    wasm_spec: &str,
    fuel_per_call: u64,
    max_outputs: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut policy, abi_version) =
        VehiclePolicyEngine::new(wasm_spec, fuel_per_call, max_outputs)?;
    println!(
        "{}",
        ready_line(&policy, abi_version, fuel_per_call, max_outputs)
    );

    let mut saw_speed = false;
    let mut saw_fuel = false;
    let mut saw_capacity = false;
    let mut saw_battery = false;
    let mut saw_hvac_state = false;

    for signal in smoke_vehicle_signals() {
        println!("VEHICLE_POLICY_SMOKE_PROPERTY {}", signal.summary());
        let output = policy.process_vehicle_signal(&signal)?;
        println!(
            "VEHICLE_POLICY_SMOKE_OUTPUT property={}({}) guest_status={} vhal_events={} vehicle_commands={} dropped={}",
            property_name(signal.property_id),
            signal.property_id,
            output.status,
            output.vhal_events.len(),
            output.vehicle_commands.len(),
            output.dropped_outputs,
        );
        if output.status != 0 || output.vhal_events.len() != 1 {
            return Err(format!(
                "default property policy did not pass through {}({}): status={} events={}",
                property_name(signal.property_id),
                signal.property_id,
                output.status,
                output.vhal_events.len(),
            )
            .into());
        }

        let event = &output.vhal_events[0];
        if event.property_id != signal.property_id
            || event.area_id != signal.area_id
            || event.value != signal.value
        {
            return Err(format!(
                "default property policy changed property payload: input={} output_prop={} output_area={} output_value={}",
                signal.summary(),
                event.property_id,
                event.area_id,
                event.value.summary(),
            )
            .into());
        }

        println!(
            "VEHICLE_POLICY_SMOKE_VHAL prop={}({}) area={} value={}",
            property_name(event.property_id),
            event.property_id,
            event.area_id,
            event.value.summary(),
        );

        match event.property_id {
            PROP_PERF_VEHICLE_SPEED => {
                saw_speed = matches!(event.value, VehicleValue::F32(v) if (v - 13.8).abs() < 0.001)
            }
            PROP_FUEL_LEVEL => {
                saw_fuel = matches!(event.value, VehicleValue::F32(v) if (v - 32_500.0).abs() < 0.5)
            }
            PROP_EV_CURRENT_BATTERY_CAPACITY => {
                saw_capacity = matches!(event.value, VehicleValue::F32(v) if (v - 74_000.0).abs() < 0.5)
            }
            PROP_EV_BATTERY_LEVEL => {
                saw_battery = matches!(event.value, VehicleValue::F32(v) if (v - 59_940.0).abs() < 1.0)
            }
            PROP_HVAC_TEMPERATURE_SET => {
                saw_hvac_state = event.area_id == AREA_DRIVER
                    && matches!(event.value, VehicleValue::F32(v) if (v - 21.5).abs() < 0.001)
            }
            _ => {}
        }
    }

    let request = VhalSetRequest {
        request_id: 77,
        property_id: PROP_HVAC_TEMPERATURE_SET,
        area_id: AREA_DRIVER,
        value: VehicleValue::F32(23.0),
        timestamp_nanos: monotonic_nanos(),
    };
    let output = policy.process_vhal_set(&request)?;
    if output.status != 0 || output.vehicle_commands.len() != 1 {
        return Err(format!(
            "HVAC set smoke failed: status={} commands={}",
            output.status,
            output.vehicle_commands.len()
        )
        .into());
    }
    let command = &output.vehicle_commands[0];
    println!(
        "VEHICLE_POLICY_SMOKE_COMMAND command={} property={}({}) area={} value={}",
        command.command_id,
        property_name(command.property_id),
        command.property_id,
        command.area_id,
        command.value.summary(),
    );
    let saw_hvac_command = command.property_id == PROP_HVAC_TEMPERATURE_SET
        && command.area_id == AREA_DRIVER
        && matches!(command.value, VehicleValue::F32(v) if (v - 23.0).abs() < 0.001);

    if !(saw_speed
        && saw_fuel
        && saw_capacity
        && saw_battery
        && saw_hvac_state
        && saw_hvac_command)
    {
        return Err(format!(
            "vehicle policy smoke markers missing speed={} fuel={} capacity={} battery={} hvac_state={} hvac_command={}",
            saw_speed, saw_fuel, saw_capacity, saw_battery, saw_hvac_state, saw_hvac_command
        )
        .into());
    }

    println!("VEHICLE_POLICY_SMOKE_GENERIC_PROPERTY_PASS_THROUGH=YES");
    println!("VEHICLE_POLICY_SMOKE_OK=YES");
    Ok(())
}

pub fn router_smoke(socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let timeout = Duration::from_secs(3);
    let mut vhal = RouterWireClient::connect(
        socket_path,
        pb::ClientRole::ROLE_VHAL,
        "arda-router-smoke-vhal",
        timeout,
    )?;
    let mut primary = RouterWireClient::connect(
        socket_path,
        pb::ClientRole::ROLE_MANAGER,
        "arda-router-smoke-manager-primary",
        timeout,
    )?;
    let mut lower_energy = RouterWireClient::connect(
        socket_path,
        pb::ClientRole::ROLE_MANAGER,
        "arda-router-smoke-manager-lower-energy",
        timeout,
    )?;
    let mut takeover_energy = RouterWireClient::connect(
        socket_path,
        pb::ClientRole::ROLE_MANAGER,
        "arda-router-smoke-manager-takeover-energy",
        timeout,
    )?;

    primary.register_source(
        9001,
        "ARDA smoke primary adapter",
        100,
        &[Capability::Energy, Capability::HvacState],
        &[Capability::HvacControl],
    )?;
    lower_energy.register_source(
        9002,
        "ARDA smoke lower-priority energy source",
        10,
        &[Capability::Energy],
        &[],
    )?;
    takeover_energy.register_source(
        9003,
        "ARDA smoke higher-priority energy source",
        200,
        &[Capability::Energy],
        &[],
    )?;

    // Primary takes ENERGY ownership.
    primary.publish_signals(&[smoke_f32(
        9001,
        PROP_FUEL_LEVEL,
        AREA_GLOBAL,
        32_500.0,
    )])?;
    require_f32_event(&mut vhal, PROP_FUEL_LEVEL, AREA_GLOBAL, 32_500.0)?;

    // Lower priority tries to publish a different fuel value. It must be shadowed.
    // Publish a primary capacity event immediately after it so the VHAL client has a
    // deterministic frame to receive and inspect.
    lower_energy.publish_signals(&[smoke_f32(
        9002,
        PROP_FUEL_LEVEL,
        AREA_GLOBAL,
        20_000.0,
    )])?;
    primary.publish_signals(&[smoke_f32(
        9001,
        PROP_EV_CURRENT_BATTERY_CAPACITY,
        AREA_GLOBAL,
        74_000.0,
    )])?;
    let events = recv_vhal_events(&mut vhal)?;
    if events.iter().any(|event| {
        event.property_id() == PROP_FUEL_LEVEL
            && (event.float_value() - 20_000.0).abs() < 0.5
    }) {
        return Err("lower-priority ENERGY source escaped arbitration".into());
    }
    if !events.iter().any(|event| {
        event.property_id() == PROP_EV_CURRENT_BATTERY_CAPACITY
            && (event.float_value() - 74_000.0).abs() < 0.5
    }) {
        return Err("primary ENERGY source did not remain selected".into());
    }

    // Higher priority source takes ENERGY ownership.
    takeover_energy.publish_signals(&[smoke_f32(
        9003,
        PROP_FUEL_LEVEL,
        AREA_GLOBAL,
        40_000.0,
    )])?;
    require_f32_event(&mut vhal, PROP_FUEL_LEVEL, AREA_GLOBAL, 40_000.0)?;

    // Old primary is now shadowed. A selected takeover battery event provides the
    // deterministic VHAL frame used to assert that the old fuel update did not leak.
    primary.publish_signals(&[smoke_f32(
        9001,
        PROP_FUEL_LEVEL,
        AREA_GLOBAL,
        30_000.0,
    )])?;
    takeover_energy.publish_signals(&[smoke_f32(
        9003,
        PROP_EV_BATTERY_LEVEL,
        AREA_GLOBAL,
        59_940.0,
    )])?;
    let events = recv_vhal_events(&mut vhal)?;
    if events.iter().any(|event| {
        event.property_id() == PROP_FUEL_LEVEL
            && (event.float_value() - 30_000.0).abs() < 0.5
    }) {
        return Err("previous ENERGY source published after takeover".into());
    }
    if !events.iter().any(|event| {
        event.property_id() == PROP_EV_BATTERY_LEVEL
            && (event.float_value() - 59_940.0).abs() < 1.0
    }) {
        return Err("higher-priority ENERGY source did not remain selected".into());
    }

    // Vehicle state from the adapter is a confirmed property event.
    primary.publish_signals(&[VehicleSignal {
        source_id: 9001,
        property_id: PROP_HVAC_TEMPERATURE_SET,
        area_id: AREA_DRIVER,
        value: VehicleValue::F32(21.5),
        timestamp_nanos: monotonic_nanos(),
    }])?;
    require_f32_event(
        &mut vhal,
        PROP_HVAC_TEMPERATURE_SET,
        AREA_DRIVER,
        21.5,
    )?;

    // AAOS set request is routed back to the adapter with the same standard
    // VehicleProperty ID. The adapter is responsible for CAN/OEM encoding.
    let set_request = make_vhal_set_f32(
        7001,
        PROP_HVAC_TEMPERATURE_SET,
        AREA_DRIVER,
        23.0,
        monotonic_nanos(),
    );
    let result = vhal.send_vhal_set(set_request)?;
    println!(
        "VEHICLE_ROUTER_SMOKE_SET_RESULT request={} handled={} status={} error={:?}",
        result.request_id(),
        result.handled(),
        result.status(),
        result.error(),
    );
    if !result.handled() || result.status() != 0 {
        return Err("router did not handle HVAC set request".into());
    }

    let command_message = primary.recv()?;
    if !command_message.has_vehicle_commands()
        || command_message.vehicle_commands().commands().len() != 1
    {
        return Err("adapter did not receive exactly one vehicle command".into());
    }
    let command = &command_message.vehicle_commands().commands()[0];
    println!(
        "VEHICLE_ROUTER_SMOKE_COMMAND command={} source={} property={}({}) area={} value={:.3}",
        command.command_id(),
        command.source_id(),
        property_name(command.property_id()),
        command.property_id(),
        command.area_id(),
        command.float_value(),
    );
    if command.property_id() != PROP_HVAC_TEMPERATURE_SET
        || command.area_id() != AREA_DRIVER
        || (command.float_value() - 23.0).abs() > 0.001
    {
        return Err("router produced the wrong HVAC vehicle command".into());
    }
    primary.ack_command(command.command_id(), pb::CommandStatus::COMMAND_OK, "")?;

    // Request != confirmed state. Only the adapter's later property publish is
    // surfaced as the confirmed 23 C state event.
    primary.publish_signals(&[VehicleSignal {
        source_id: 9001,
        property_id: PROP_HVAC_TEMPERATURE_SET,
        area_id: AREA_DRIVER,
        value: VehicleValue::F32(23.0),
        timestamp_nanos: monotonic_nanos(),
    }])?;
    require_f32_event(
        &mut vhal,
        PROP_HVAC_TEMPERATURE_SET,
        AREA_DRIVER,
        23.0,
    )?;

    // Give the router writer thread a small window to flush logs before process exit.
    thread::sleep(Duration::from_millis(50));

    println!("VEHICLE_ROUTER_SMOKE_ARBITRATION_OK=YES");
    println!("VEHICLE_ROUTER_SMOKE_PROPERTY_ID_COMMAND_OK=YES");
    println!("VEHICLE_ROUTER_SMOKE_OK=YES");
    Ok(())
}


pub fn manager_bridge_smoke(socket_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let timeout = Duration::from_secs(4);
    let mut vhal = RouterWireClient::connect(
        socket_path,
        pb::ClientRole::ROLE_VHAL,
        "arda-manager-bridge-smoke-vhal",
        timeout,
    )?;

    let request_id = 7_002;
    let set_request = make_vhal_set_f32(
        request_id,
        PROP_HVAC_TEMPERATURE_SET,
        AREA_DRIVER,
        23.0,
        monotonic_nanos(),
    );
    let result = vhal.send_vhal_set(set_request)?;
    println!(
        "VEHICLE_MANAGER_BRIDGE_SMOKE_SET_RESULT request={} handled={} status={} error={:?}",
        result.request_id(),
        result.handled(),
        result.status(),
        result.error(),
    );
    if !result.handled() || result.status() != 0 {
        return Err(format!(
            "manager bridge set request was not handled: status={} error={:?}",
            result.status(),
            result.error(),
        )
        .into());
    }
    println!("VEHICLE_MANAGER_BRIDGE_SMOKE_SET_HANDLED=YES");

    let mut confirmed = false;
    for _ in 0..8 {
        let message = vhal.recv()?;
        if message.has_vhal_set_error()
            && message.vhal_set_error().request_id() == request_id
        {
            let error = message.vhal_set_error();
            return Err(format!(
                "manager bridge command failed request={} status={} error={:?}",
                error.request_id(),
                error.status(),
                error.error(),
            )
            .into());
        }
        if !message.has_vhal_events() {
            continue;
        }
        for event in message.vhal_events().events() {
            if event.property_id() == PROP_HVAC_TEMPERATURE_SET
                && event.area_id() == AREA_DRIVER
                && event.value_type() == pb::ValueType::VALUE_F32
                && (event.float_value() - 23.0).abs() < 0.001
            {
                confirmed = true;
                println!(
                    "VEHICLE_MANAGER_BRIDGE_SMOKE_CONFIRMED property={}({}) area={} value={:.3}",
                    property_name(event.property_id()),
                    event.property_id(),
                    event.area_id(),
                    event.float_value(),
                );
                break;
            }
        }
        if confirmed {
            break;
        }
    }
    if !confirmed {
        return Err("manager bridge did not return adapter-confirmed HVAC state".into());
    }
    println!("VEHICLE_MANAGER_BRIDGE_SMOKE_CONFIRMED_STATE=YES");

    thread::sleep(Duration::from_millis(2_250));
    let nonce = 0xA2_0001u64;
    let mut ping = pb::Ping::new();
    ping.set_nonce(nonce);
    let mut ping_envelope = super::wire::envelope();
    ping_envelope.set_ping(ping);
    vhal.send(&ping_envelope)?;

    let mut saw_pong = false;
    for _ in 0..8 {
        let message = vhal.recv()?;
        if message.has_vhal_set_error()
            && message.vhal_set_error().request_id() == request_id
        {
            let error = message.vhal_set_error();
            return Err(format!(
                "manager bridge ACK did not clear router command request={} status={} error={:?}",
                error.request_id(),
                error.status(),
                error.error(),
            )
            .into());
        }
        if message.has_pong() && message.pong().nonce() == nonce {
            saw_pong = true;
            break;
        }
    }
    if !saw_pong {
        return Err("manager bridge smoke did not receive router Pong".into());
    }
    println!("VEHICLE_MANAGER_BRIDGE_SMOKE_ACK_ROUND_TRIP=YES");
    println!("VEHICLE_MANAGER_BRIDGE_SMOKE_OK=YES");
    Ok(())
}

fn recv_vhal_events(
    client: &mut RouterWireClient,
) -> Result<Vec<pb::VhalEvent>, Box<dyn std::error::Error>> {
    for _ in 0..8 {
        let message = client.recv()?;
        if message.has_vhal_events() {
            return Ok(message.vhal_events().events().to_vec());
        }
    }
    Err("router smoke did not receive VHAL events".into())
}

fn require_f32_event(
    client: &mut RouterWireClient,
    property_id: i32,
    area_id: i32,
    expected: f32,
) -> Result<(), Box<dyn std::error::Error>> {
    let events = recv_vhal_events(client)?;
    for event in &events {
        println!(
            "VEHICLE_ROUTER_SMOKE_VHAL_EVENT prop={}({}) area={} type={:?} value={:.3}",
            property_name(event.property_id()),
            event.property_id(),
            event.area_id(),
            event.value_type(),
            event.float_value(),
        );
    }
    if events.iter().any(|event| {
        event.property_id() == property_id
            && event.area_id() == area_id
            && (event.float_value() - expected).abs() < 0.001
    }) {
        Ok(())
    } else {
        Err(format!(
            "missing expected VHAL f32 event property={}({}) area={} value={}",
            property_name(property_id),
            property_id,
            area_id,
            expected,
        )
        .into())
    }
}

fn smoke_f32(source_id: u32, property_id: i32, area_id: i32, value: f32) -> VehicleSignal {
    VehicleSignal {
        source_id,
        property_id: property_id,
        area_id,
        value: VehicleValue::F32(value),
        timestamp_nanos: monotonic_nanos(),
    }
}
