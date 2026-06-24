//! Generated AAP protobuf bindings + tiny helpers.
//!
//! The .proto files are copied from the aa-proxy-rs reference tree into
//! `proto/`. build.rs generates the Rust module at compile time. Going forward,
//! service discovery/channel messages should be built through these generated
//! structs instead of manual byte payloads.

pub mod protos {
    include!(concat!(env!("OUT_DIR"), "/protos/mod.rs"));
}

use protobuf::{Enum, Message};
use self::protos::protos::*;

pub fn control_message_name(id: u16) -> &'static str {
    match ControlMessageType::from_i32(id as i32) {
        Some(ControlMessageType::MESSAGE_VERSION_REQUEST) => "MESSAGE_VERSION_REQUEST",
        Some(ControlMessageType::MESSAGE_VERSION_RESPONSE) => "MESSAGE_VERSION_RESPONSE",
        Some(ControlMessageType::MESSAGE_ENCAPSULATED_SSL) => "MESSAGE_ENCAPSULATED_SSL",
        Some(ControlMessageType::MESSAGE_AUTH_COMPLETE) => "MESSAGE_AUTH_COMPLETE",
        Some(ControlMessageType::MESSAGE_SERVICE_DISCOVERY_REQUEST) => "MESSAGE_SERVICE_DISCOVERY_REQUEST",
        Some(ControlMessageType::MESSAGE_SERVICE_DISCOVERY_RESPONSE) => "MESSAGE_SERVICE_DISCOVERY_RESPONSE",
        Some(ControlMessageType::MESSAGE_CHANNEL_OPEN_REQUEST) => "MESSAGE_CHANNEL_OPEN_REQUEST",
        Some(ControlMessageType::MESSAGE_CHANNEL_OPEN_RESPONSE) => "MESSAGE_CHANNEL_OPEN_RESPONSE",
        Some(ControlMessageType::MESSAGE_CHANNEL_CLOSE_NOTIFICATION) => "MESSAGE_CHANNEL_CLOSE_NOTIFICATION",
        Some(ControlMessageType::MESSAGE_PING_REQUEST) => "MESSAGE_PING_REQUEST",
        Some(ControlMessageType::MESSAGE_PING_RESPONSE) => "MESSAGE_PING_RESPONSE",
        Some(ControlMessageType::MESSAGE_NAV_FOCUS_REQUEST) => "MESSAGE_NAV_FOCUS_REQUEST",
        Some(ControlMessageType::MESSAGE_NAV_FOCUS_NOTIFICATION) => "MESSAGE_NAV_FOCUS_NOTIFICATION",
        Some(ControlMessageType::MESSAGE_BYEBYE_REQUEST) => "MESSAGE_BYEBYE_REQUEST",
        Some(ControlMessageType::MESSAGE_BYEBYE_RESPONSE) => "MESSAGE_BYEBYE_RESPONSE",
        Some(ControlMessageType::MESSAGE_VOICE_SESSION_NOTIFICATION) => "MESSAGE_VOICE_SESSION_NOTIFICATION",
        Some(ControlMessageType::MESSAGE_AUDIO_FOCUS_REQUEST) => "MESSAGE_AUDIO_FOCUS_REQUEST",
        Some(ControlMessageType::MESSAGE_AUDIO_FOCUS_NOTIFICATION) => "MESSAGE_AUDIO_FOCUS_NOTIFICATION",
        Some(ControlMessageType::MESSAGE_CAR_CONNECTED_DEVICES_REQUEST) => "MESSAGE_CAR_CONNECTED_DEVICES_REQUEST",
        Some(ControlMessageType::MESSAGE_CAR_CONNECTED_DEVICES_RESPONSE) => "MESSAGE_CAR_CONNECTED_DEVICES_RESPONSE",
        Some(ControlMessageType::MESSAGE_USER_SWITCH_REQUEST) => "MESSAGE_USER_SWITCH_REQUEST",
        Some(ControlMessageType::MESSAGE_BATTERY_STATUS_NOTIFICATION) => "MESSAGE_BATTERY_STATUS_NOTIFICATION",
        Some(ControlMessageType::MESSAGE_CALL_AVAILABILITY_STATUS) => "MESSAGE_CALL_AVAILABILITY_STATUS",
        Some(ControlMessageType::MESSAGE_USER_SWITCH_RESPONSE) => "MESSAGE_USER_SWITCH_RESPONSE",
        Some(ControlMessageType::MESSAGE_SERVICE_DISCOVERY_UPDATE) => "MESSAGE_SERVICE_DISCOVERY_UPDATE",
        Some(ControlMessageType::MESSAGE_UNEXPECTED_MESSAGE) => "MESSAGE_UNEXPECTED_MESSAGE",
        Some(ControlMessageType::MESSAGE_FRAMING_ERROR) => "MESSAGE_FRAMING_ERROR",
        None => "UNKNOWN_CONTROL",
    }
}

/// Describe a control-message payload body using the generated protobuf types.
/// `body` is the payload after the 2-byte control message id.
pub fn describe_control_body(id: u16, body: &[u8]) -> Option<String> {
    match ControlMessageType::from_i32(id as i32) {
        Some(ControlMessageType::MESSAGE_AUTH_COMPLETE) => {
            let msg = AuthResponse::parse_from_bytes(body).ok()?;
            Some(format!("MESSAGE_AUTH_COMPLETE status={}", msg.status()))
        }
        Some(ControlMessageType::MESSAGE_SERVICE_DISCOVERY_REQUEST) => {
            let msg = ServiceDiscoveryRequest::parse_from_bytes(body).ok()?;
            Some(format!(
                "MESSAGE_SERVICE_DISCOVERY_REQUEST label={:?} device={:?} small_icon={} medium_icon={} large_icon={}",
                msg.label_text(),
                msg.device_name(),
                msg.small_icon().len(),
                msg.medium_icon().len(),
                msg.large_icon().len()
            ))
        }
        Some(ControlMessageType::MESSAGE_CHANNEL_OPEN_REQUEST) => {
            let msg = ChannelOpenRequest::parse_from_bytes(body).ok()?;
            Some(format!(
                "MESSAGE_CHANNEL_OPEN_REQUEST service_id={} priority={}",
                msg.service_id(),
                msg.priority()
            ))
        }
        Some(ControlMessageType::MESSAGE_CHANNEL_OPEN_RESPONSE) => {
            let msg = ChannelOpenResponse::parse_from_bytes(body).ok()?;
            Some(format!("MESSAGE_CHANNEL_OPEN_RESPONSE status={:?}", msg.status()))
        }
        Some(ControlMessageType::MESSAGE_PING_REQUEST) => {
            let msg = PingRequest::parse_from_bytes(body).ok()?;
            Some(format!(
                "MESSAGE_PING_REQUEST timestamp={} bug_report={} data_len={}",
                msg.timestamp(),
                msg.bug_report(),
                msg.data().len()
            ))
        }
        Some(ControlMessageType::MESSAGE_PING_RESPONSE) => {
            let msg = PingResponse::parse_from_bytes(body).ok()?;
            Some(format!(
                "MESSAGE_PING_RESPONSE timestamp={} data_len={}",
                msg.timestamp(),
                msg.data().len()
            ))
        }
        Some(ControlMessageType::MESSAGE_BYEBYE_REQUEST) => {
            let msg = ByeByeRequest::parse_from_bytes(body).ok()?;
            Some(format!("MESSAGE_BYEBYE_REQUEST reason={:?}", msg.reason()))
        }
        _ => None,
    }
}

pub fn smoke_summary() -> String {
    let auth_payload = [0x08, 0x00];
    let auth = describe_control_body(
        ControlMessageType::MESSAGE_AUTH_COMPLETE.value() as u16,
        &auth_payload,
    )
    .unwrap_or_else(|| "AUTH decode failed".to_string());
    format!("proto OK: {auth}")
}
