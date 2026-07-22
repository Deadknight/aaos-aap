use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

use protobuf::{Enum, Message};

use super::model::{VehicleCommand, VehicleSignal, VehicleValue, VhalEvent, VhalSetRequest};

pub const ROUTER_PROTOCOL_VERSION: u32 = 3;
pub const DEFAULT_ROUTER_SOCKET: &str = "/dev/socket/arda_vehicle_router";
pub const MAX_ROUTER_FRAME_BYTES: usize = 1024 * 1024;

pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/protos/mod.rs"));
}

pub use generated::arda_vehicle_router as pb;

pub fn read_envelope(stream: &mut UnixStream) -> io::Result<pb::Envelope> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > MAX_ROUTER_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid router frame length: {len}"),
        ));
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body)?;
    pb::Envelope::parse_from_bytes(&body)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
}

pub fn write_envelope(stream: &mut UnixStream, envelope: &pb::Envelope) -> io::Result<()> {
    let body = envelope
        .write_to_bytes()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
    if body.is_empty() || body.len() > MAX_ROUTER_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid outgoing router frame length: {}", body.len()),
        ));
    }
    let len = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "router frame too large"))?;
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

pub fn envelope() -> pb::Envelope {
    let mut envelope = pb::Envelope::new();
    envelope.set_protocol_version(ROUTER_PROTOCOL_VERSION);
    envelope
}

pub fn peer_credentials(stream: &UnixStream) -> io::Result<(u32, u32, u32)> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((cred.uid, cred.gid, u32::try_from(cred.pid).unwrap_or(0)))
}

pub fn value_from_wire(
    value_type: i32,
    bool_value: bool,
    int32_value: i32,
    int64_value: i64,
    float_value: f32,
    double_value: f64,
    bytes_value: &[u8],
) -> Result<VehicleValue, String> {
    match value_type {
        super::model::VALUE_BOOL => Ok(VehicleValue::Bool(bool_value)),
        super::model::VALUE_I32 => Ok(VehicleValue::I32(int32_value)),
        super::model::VALUE_I64 => Ok(VehicleValue::I64(int64_value)),
        super::model::VALUE_F32 => Ok(VehicleValue::F32(float_value)),
        super::model::VALUE_F64 => Ok(VehicleValue::F64(double_value)),
        super::model::VALUE_BYTES => Ok(VehicleValue::Bytes(bytes_value.to_vec())),
        other => Err(format!("unknown value_type={other}")),
    }
}

pub fn vehicle_signal_from_pb(message: &pb::VehicleSignal) -> Result<VehicleSignal, String> {
    Ok(VehicleSignal {
        source_id: message.source_id(),
        property_id: message.property_id(),
        area_id: message.area_id(),
        value: value_from_wire(
            message.value_type().value(),
            message.bool_value(),
            message.int32_value(),
            message.int64_value(),
            message.float_value(),
            message.double_value(),
            message.bytes_value(),
        )?,
        timestamp_nanos: message.timestamp_nanos(),
    })
}

pub fn vhal_set_from_pb(message: &pb::VhalSetRequest) -> Result<VhalSetRequest, String> {
    Ok(VhalSetRequest {
        request_id: message.request_id(),
        property_id: message.property_id(),
        area_id: message.area_id(),
        value: value_from_wire(
            message.value_type().value(),
            message.bool_value(),
            message.int32_value(),
            message.int64_value(),
            message.float_value(),
            message.double_value(),
            message.bytes_value(),
        )?,
        timestamp_nanos: message.timestamp_nanos(),
    })
}

pub fn vehicle_signal_to_pb(signal: &VehicleSignal) -> pb::VehicleSignal {
    let mut message = pb::VehicleSignal::new();
    message.set_source_id(signal.source_id);
    message.set_property_id(signal.property_id);
    message.set_area_id(signal.area_id);
    set_value_on_signal(&mut message, &signal.value);
    message.set_timestamp_nanos(signal.timestamp_nanos);
    message
}

pub fn vhal_event_to_pb(event: &VhalEvent) -> pb::VhalEvent {
    let mut message = pb::VhalEvent::new();
    message.set_property_id(event.property_id);
    message.set_area_id(event.area_id);
    set_value_on_vhal_event(&mut message, &event.value);
    message.set_timestamp_nanos(event.timestamp_nanos);
    message
}

pub fn vehicle_command_to_pb(command: &VehicleCommand) -> pb::VehicleCommand {
    let mut message = pb::VehicleCommand::new();
    message.set_command_id(command.command_id);
    message.set_source_id(command.source_id);
    message.set_property_id(command.property_id);
    message.set_area_id(command.area_id);
    set_value_on_command(&mut message, &command.value);
    message.set_timestamp_nanos(command.timestamp_nanos);
    message
}

fn value_type(value: &VehicleValue) -> pb::ValueType {
    pb::ValueType::from_i32(value.value_type_id()).unwrap_or(pb::ValueType::VALUE_UNSPECIFIED)
}

fn set_value_on_signal(message: &mut pb::VehicleSignal, value: &VehicleValue) {
    message.set_value_type(value_type(value));
    match value {
        VehicleValue::Bool(value) => message.set_bool_value(*value),
        VehicleValue::I32(value) => message.set_int32_value(*value),
        VehicleValue::I64(value) => message.set_int64_value(*value),
        VehicleValue::F32(value) => message.set_float_value(*value),
        VehicleValue::F64(value) => message.set_double_value(*value),
        VehicleValue::Bytes(value) => message.set_bytes_value(value.clone()),
    }
}

fn set_value_on_vhal_event(message: &mut pb::VhalEvent, value: &VehicleValue) {
    message.set_value_type(value_type(value));
    match value {
        VehicleValue::Bool(value) => message.set_bool_value(*value),
        VehicleValue::I32(value) => message.set_int32_value(*value),
        VehicleValue::I64(value) => message.set_int64_value(*value),
        VehicleValue::F32(value) => message.set_float_value(*value),
        VehicleValue::F64(value) => message.set_double_value(*value),
        VehicleValue::Bytes(value) => message.set_bytes_value(value.clone()),
    }
}

fn set_value_on_command(message: &mut pb::VehicleCommand, value: &VehicleValue) {
    message.set_value_type(value_type(value));
    match value {
        VehicleValue::Bool(value) => message.set_bool_value(*value),
        VehicleValue::I32(value) => message.set_int32_value(*value),
        VehicleValue::I64(value) => message.set_int64_value(*value),
        VehicleValue::F32(value) => message.set_float_value(*value),
        VehicleValue::F64(value) => message.set_double_value(*value),
        VehicleValue::Bytes(value) => message.set_bytes_value(value.clone()),
    }
}
