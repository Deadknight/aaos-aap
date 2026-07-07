use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::model::{Capability, VehicleSignal};
use super::wire::{
    envelope, pb, read_envelope, vehicle_signal_to_pb, write_envelope, ROUTER_PROTOCOL_VERSION,
};

pub struct RouterWireClient {
    stream: UnixStream,
    pub role: pb::ClientRole,
    pub name: String,
}

impl RouterWireClient {
    pub fn connect(
        socket_path: &str,
        role: pb::ClientRole,
        name: &str,
        timeout: Duration,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut stream = UnixStream::connect(socket_path)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;

        let mut hello = pb::Hello::new();
        hello.set_protocol_version(ROUTER_PROTOCOL_VERSION);
        hello.set_role(role);
        hello.set_client_name(name.to_string());
        let mut message = envelope();
        message.set_hello(hello);
        write_envelope(&mut stream, &message)?;

        let response = read_envelope(&mut stream)?;
        if !response.has_hello_ack() {
            return Err("vehicle router did not return HelloAck".into());
        }
        let ack = response.hello_ack();
        if !ack.accepted() {
            return Err(format!("vehicle router rejected {}: {}", name, ack.error()).into());
        }
        if ack.protocol_version() != ROUTER_PROTOCOL_VERSION {
            return Err(format!(
                "vehicle router protocol mismatch expected={} actual={}",
                ROUTER_PROTOCOL_VERSION,
                ack.protocol_version(),
            )
            .into());
        }

        Ok(Self {
            stream,
            role,
            name: name.to_string(),
        })
    }

    pub fn register_source(
        &mut self,
        source_id: u32,
        source_name: &str,
        priority: i32,
        publish_capabilities: &[Capability],
        command_capabilities: &[Capability],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut request = pb::RegisterSource::new();
        request.set_source_id(source_id);
        request.set_source_name(source_name.to_string());
        request.set_priority(priority);
        request.set_publish_capabilities(
            publish_capabilities.iter().map(|capability| capability.id()).collect(),
        );
        request.set_command_capabilities(
            command_capabilities.iter().map(|capability| capability.id()).collect(),
        );
        let mut message = envelope();
        message.set_register_source(request);
        self.send(&message)?;

        let response = self.recv()?;
        if !response.has_register_source_result() {
            return Err("vehicle router did not return RegisterSourceResult".into());
        }
        let result = response.register_source_result();
        if !result.accepted() {
            return Err(format!(
                "vehicle router rejected source {}: {}",
                source_id,
                result.error(),
            )
            .into());
        }
        if result.source_id() != source_id {
            return Err(format!(
                "vehicle router source registration mismatch expected={} actual={}",
                source_id,
                result.source_id(),
            )
            .into());
        }
        Ok(())
    }

    pub fn publish_signals(
        &mut self,
        signals: &[VehicleSignal],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut batch = pb::VehicleSignals::new();
        batch.set_signals(signals.iter().map(vehicle_signal_to_pb).collect());
        let mut message = envelope();
        message.set_vehicle_signals(batch);
        self.send(&message)?;
        Ok(())
    }

    pub fn send_vhal_set(
        &mut self,
        request: pb::VhalSetRequest,
    ) -> Result<pb::VhalSetResult, Box<dyn std::error::Error>> {
        let request_id = request.request_id();
        let mut message = envelope();
        message.set_vhal_set_request(request);
        self.send(&message)?;
        loop {
            let response = self.recv()?;
            if response.has_vhal_set_result()
                && response.vhal_set_result().request_id() == request_id
            {
                return Ok(response.vhal_set_result().clone());
            }
        }
    }

    pub fn ack_command(
        &mut self,
        command_id: u64,
        status: pb::CommandStatus,
        error: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut ack = pb::CommandAck::new();
        ack.set_command_id(command_id);
        ack.set_status(status);
        ack.set_error(error.to_string());
        let mut acks = pb::CommandAcks::new();
        acks.set_acknowledgements(vec![ack]);
        let mut message = envelope();
        message.set_command_acks(acks);
        self.send(&message)?;
        Ok(())
    }

    pub fn send(&mut self, message: &pb::Envelope) -> Result<(), Box<dyn std::error::Error>> {
        write_envelope(&mut self.stream, message)?;
        Ok(())
    }

    pub fn recv(&mut self) -> Result<pb::Envelope, Box<dyn std::error::Error>> {
        Ok(read_envelope(&mut self.stream)?)
    }
}

pub struct SensorRouterPublisher {
    client: RouterWireClient,
    source_id: u32,
}

impl SensorRouterPublisher {
    pub fn connect(
        socket_path: &str,
        source_id: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut client = RouterWireClient::connect(
            socket_path,
            pb::ClientRole::ROLE_SIGNAL_SOURCE,
            "arda-aap-sensor-source",
            Duration::from_secs(2),
        )?;
        client.register_source(
            source_id,
            "AAP SensorSource",
            10,
            &[Capability::Powertrain, Capability::HvacState],
            &[],
        )?;
        Ok(Self { client, source_id })
    }

    pub fn publish(
        &mut self,
        signals: &[VehicleSignal],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sourced = signals
            .iter()
            .cloned()
            .map(|signal| signal.with_source(self.source_id))
            .collect::<Vec<_>>();
        self.client.publish_signals(&sourced)
    }
}

pub fn make_vhal_set_f32(
    request_id: u64,
    property_id: i32,
    area_id: i32,
    value: f32,
    timestamp_nanos: i64,
) -> pb::VhalSetRequest {
    let mut request = pb::VhalSetRequest::new();
    request.set_request_id(request_id);
    request.set_property_id(property_id);
    request.set_area_id(area_id);
    request.set_value_type(pb::ValueType::VALUE_F32);
    request.set_float_value(value);
    request.set_timestamp_nanos(timestamp_nanos);
    request
}

pub fn describe_envelope(message: &pb::Envelope) -> String {
    if message.has_vhal_events() {
        return format!("VHAL_EVENTS count={}", message.vhal_events().events().len());
    }
    if message.has_vehicle_commands() {
        return format!(
            "VEHICLE_COMMANDS count={}",
            message.vehicle_commands().commands().len()
        );
    }
    if message.has_vhal_set_error() {
        let error = message.vhal_set_error();
        return format!(
            "VHAL_SET_ERROR request={} property={} area={} status={} error={:?}",
            error.request_id(),
            error.property_id(),
            error.area_id(),
            error.status(),
            error.error(),
        );
    }
    if message.has_pong() {
        return format!("PONG nonce={}", message.pong().nonce());
    }
    "UNKNOWN_ENVELOPE".to_string()
}
