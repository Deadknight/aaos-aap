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
use crate::sensor::{ArdaSensorEvent, GpsSatelliteInfo};

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
        Some(ControlMessageType::MESSAGE_SERVICE_DISCOVERY_RESPONSE) => {
            let summary = summarize_service_discovery_response_body(body, "highest").ok()?;
            Some(summary.one_line)
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


/// If `payload` is MESSAGE_PING_REQUEST, build a matching MESSAGE_PING_RESPONSE.
/// Returned bytes include the 2-byte AAP control message id followed by the
/// protobuf body. The timestamp is echoed back, and optional ping data is
/// mirrored when present.
pub fn maybe_build_ping_response_control_payload(
    payload: &[u8],
) -> Result<Option<Vec<u8>>, protobuf::Error> {
    if payload.len() < 2 {
        return Ok(None);
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    if id != ControlMessageType::MESSAGE_PING_REQUEST.value() as u16 {
        return Ok(None);
    }

    let req = PingRequest::parse_from_bytes(&payload[2..])?;
    let mut resp = PingResponse::new();
    resp.set_timestamp(req.timestamp());
    if !req.data().is_empty() {
        resp.set_data(req.data().to_vec());
    }

    let mut out = Vec::new();
    out.extend_from_slice(&(ControlMessageType::MESSAGE_PING_RESPONSE.value() as u16).to_be_bytes());
    out.extend_from_slice(&resp.write_to_bytes()?);
    Ok(Some(out))
}


pub fn input_message_name(id: u16) -> &'static str {
    match InputMessageId::from_i32(id as i32) {
        Some(InputMessageId::INPUT_MESSAGE_INPUT_REPORT) => "INPUT_MESSAGE_INPUT_REPORT",
        Some(InputMessageId::INPUT_MESSAGE_KEY_BINDING_REQUEST) => "INPUT_MESSAGE_KEY_BINDING_REQUEST",
        Some(InputMessageId::INPUT_MESSAGE_KEY_BINDING_RESPONSE) => "INPUT_MESSAGE_KEY_BINDING_RESPONSE",
        Some(InputMessageId::INPUT_MESSAGE_INPUT_FEEDBACK) => "INPUT_MESSAGE_INPUT_FEEDBACK",
        None => "UNKNOWN_INPUT_MESSAGE",
    }
}

pub fn keycode_name(code: u32) -> &'static str {
    match code {
        3 => "KEYCODE_HOME",
        4 => "KEYCODE_BACK",
        5 => "KEYCODE_CALL",
        6 => "KEYCODE_ENDCALL",
        19 => "KEYCODE_DPAD_UP",
        20 => "KEYCODE_DPAD_DOWN",
        21 => "KEYCODE_DPAD_LEFT",
        22 => "KEYCODE_DPAD_RIGHT",
        23 => "KEYCODE_DPAD_CENTER",
        24 => "KEYCODE_VOLUME_UP",
        25 => "KEYCODE_VOLUME_DOWN",
        79 => "KEYCODE_HEADSETHOOK",
        82 => "KEYCODE_MENU",
        84 => "KEYCODE_SEARCH",
        85 => "KEYCODE_MEDIA_PLAY_PAUSE",
        86 => "KEYCODE_MEDIA_STOP",
        87 => "KEYCODE_MEDIA_NEXT",
        88 => "KEYCODE_MEDIA_PREVIOUS",
        89 => "KEYCODE_MEDIA_REWIND",
        90 => "KEYCODE_MEDIA_FAST_FORWARD",
        126 => "KEYCODE_MEDIA_PLAY",
        127 => "KEYCODE_MEDIA_PAUSE",
        164 => "KEYCODE_VOLUME_MUTE",
        219 => "KEYCODE_ASSIST",
        231 => "KEYCODE_VOICE_ASSIST",
        260 => "KEYCODE_NAVIGATE_PREVIOUS",
        261 => "KEYCODE_NAVIGATE_NEXT",
        262 => "KEYCODE_NAVIGATE_IN",
        263 => "KEYCODE_NAVIGATE_OUT",
        65535 => "AA_SENTINEL",
        65536 => "AA_ROTARY_CONTROLLER",
        65537 => "AA_MEDIA",
        65538 => "AA_NAVIGATION",
        65539 => "AA_RADIO",
        65540 => "AA_TEL",
        65541 => "AA_PRIMARY_BUTTON",
        65542 => "AA_SECONDARY_BUTTON",
        65543 => "AA_TERTIARY_BUTTON",
        65544 => "AA_TURN_CARD",
        _ => "KEYCODE_UNKNOWN_OR_OEM",
    }
}



pub fn sensor_message_name(id: u16) -> &'static str {
    match SensorMessageId::from_i32(id as i32) {
        Some(SensorMessageId::SENSOR_MESSAGE_REQUEST) => "SENSOR_MESSAGE_REQUEST",
        Some(SensorMessageId::SENSOR_MESSAGE_RESPONSE) => "SENSOR_MESSAGE_RESPONSE",
        Some(SensorMessageId::SENSOR_MESSAGE_BATCH) => "SENSOR_MESSAGE_BATCH",
        Some(SensorMessageId::SENSOR_MESSAGE_ERROR) => "SENSOR_MESSAGE_ERROR",
        None => "UNKNOWN_SENSOR_MESSAGE",
    }
}

pub fn sensor_type_name(value: i32) -> &'static str {
    match SensorType::from_i32(value) {
        Some(SensorType::SENSOR_LOCATION) => "SENSOR_LOCATION",
        Some(SensorType::SENSOR_COMPASS) => "SENSOR_COMPASS",
        Some(SensorType::SENSOR_SPEED) => "SENSOR_SPEED",
        Some(SensorType::SENSOR_RPM) => "SENSOR_RPM",
        Some(SensorType::SENSOR_ODOMETER) => "SENSOR_ODOMETER",
        Some(SensorType::SENSOR_FUEL) => "SENSOR_FUEL",
        Some(SensorType::SENSOR_PARKING_BRAKE) => "SENSOR_PARKING_BRAKE",
        Some(SensorType::SENSOR_GEAR) => "SENSOR_GEAR",
        Some(SensorType::SENSOR_OBDII_DIAGNOSTIC_CODE) => "SENSOR_OBDII_DIAGNOSTIC_CODE",
        Some(SensorType::SENSOR_NIGHT_MODE) => "SENSOR_NIGHT_MODE",
        Some(SensorType::SENSOR_ENVIRONMENT_DATA) => "SENSOR_ENVIRONMENT_DATA",
        Some(SensorType::SENSOR_HVAC_DATA) => "SENSOR_HVAC_DATA",
        Some(SensorType::SENSOR_DRIVING_STATUS_DATA) => "SENSOR_DRIVING_STATUS_DATA",
        Some(SensorType::SENSOR_DEAD_RECKONING_DATA) => "SENSOR_DEAD_RECKONING_DATA",
        Some(SensorType::SENSOR_PASSENGER_DATA) => "SENSOR_PASSENGER_DATA",
        Some(SensorType::SENSOR_DOOR_DATA) => "SENSOR_DOOR_DATA",
        Some(SensorType::SENSOR_LIGHT_DATA) => "SENSOR_LIGHT_DATA",
        Some(SensorType::SENSOR_TIRE_PRESSURE_DATA) => "SENSOR_TIRE_PRESSURE_DATA",
        Some(SensorType::SENSOR_ACCELEROMETER_DATA) => "SENSOR_ACCELEROMETER_DATA",
        Some(SensorType::SENSOR_GYROSCOPE_DATA) => "SENSOR_GYROSCOPE_DATA",
        Some(SensorType::SENSOR_GPS_SATELLITE_DATA) => "SENSOR_GPS_SATELLITE_DATA",
        Some(SensorType::SENSOR_TOLL_CARD) => "SENSOR_TOLL_CARD",
        Some(SensorType::SENSOR_VEHICLE_ENERGY_MODEL_DATA) => "SENSOR_VEHICLE_ENERGY_MODEL_DATA",
        Some(SensorType::SENSOR_TRAILER_DATA) => "SENSOR_TRAILER_DATA",
        None => "UNKNOWN_SENSOR_TYPE",
    }
}

pub fn media_message_name(id: u16) -> &'static str {
    match MediaMessageId::from_i32(id as i32) {
        Some(MediaMessageId::MEDIA_MESSAGE_DATA) => "MEDIA_MESSAGE_DATA",
        Some(MediaMessageId::MEDIA_MESSAGE_CODEC_CONFIG) => "MEDIA_MESSAGE_CODEC_CONFIG",
        Some(MediaMessageId::MEDIA_MESSAGE_SETUP) => "MEDIA_MESSAGE_SETUP",
        Some(MediaMessageId::MEDIA_MESSAGE_START) => "MEDIA_MESSAGE_START",
        Some(MediaMessageId::MEDIA_MESSAGE_STOP) => "MEDIA_MESSAGE_STOP",
        Some(MediaMessageId::MEDIA_MESSAGE_CONFIG) => "MEDIA_MESSAGE_CONFIG",
        Some(MediaMessageId::MEDIA_MESSAGE_ACK) => "MEDIA_MESSAGE_ACK",
        Some(MediaMessageId::MEDIA_MESSAGE_MICROPHONE_REQUEST) => "MEDIA_MESSAGE_MICROPHONE_REQUEST",
        Some(MediaMessageId::MEDIA_MESSAGE_MICROPHONE_RESPONSE) => "MEDIA_MESSAGE_MICROPHONE_RESPONSE",
        Some(MediaMessageId::MEDIA_MESSAGE_VIDEO_FOCUS_REQUEST) => "MEDIA_MESSAGE_VIDEO_FOCUS_REQUEST",
        Some(MediaMessageId::MEDIA_MESSAGE_VIDEO_FOCUS_NOTIFICATION) => "MEDIA_MESSAGE_VIDEO_FOCUS_NOTIFICATION",
        Some(MediaMessageId::MEDIA_MESSAGE_UPDATE_UI_CONFIG_REQUEST) => "MEDIA_MESSAGE_UPDATE_UI_CONFIG_REQUEST",
        Some(MediaMessageId::MEDIA_MESSAGE_UPDATE_UI_CONFIG_REPLY) => "MEDIA_MESSAGE_UPDATE_UI_CONFIG_REPLY",
        Some(MediaMessageId::MEDIA_MESSAGE_AUDIO_UNDERFLOW_NOTIFICATION) => "MEDIA_MESSAGE_AUDIO_UNDERFLOW_NOTIFICATION",
        Some(MediaMessageId::MEDIA_MESSAGE_ACTION_TAKEN_NOTIFICATION) => "MEDIA_MESSAGE_ACTION_TAKEN_NOTIFICATION",
        Some(MediaMessageId::MEDIA_MESSAGE_MEDIA_STATS) => "MEDIA_MESSAGE_MEDIA_STATS",
        Some(MediaMessageId::MEDIA_MESSAGE_MEDIA_OPTIONS) => "MEDIA_MESSAGE_MEDIA_OPTIONS",
        _ => "UNKNOWN_MEDIA_MESSAGE",
    }
}

/// Describe a non-control media-channel payload. `payload` includes the 2-byte
/// MediaMessageId followed by the protobuf body, except MEDIA_MESSAGE_DATA where
/// the body is raw media bytes.
pub fn describe_media_payload(payload: &[u8]) -> Option<String> {
    if payload.len() < 2 {
        return None;
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    let body = &payload[2..];
    let name = media_message_name(id);

    match MediaMessageId::from_i32(id as i32) {
        Some(MediaMessageId::MEDIA_MESSAGE_CONFIG) => {
            let msg = Config::parse_from_bytes(body).ok()?;
            Some(format!(
                "{name} status={:?} max_unacked={} configuration_indices={:?}",
                msg.status(),
                msg.max_unacked(),
                msg.configuration_indices
            ))
        }
        Some(MediaMessageId::MEDIA_MESSAGE_START) => {
            let msg = Start::parse_from_bytes(body).ok()?;
            Some(format!(
                "{name} session_id={} configuration_index={}",
                msg.session_id(),
                msg.configuration_index()
            ))
        }
        Some(MediaMessageId::MEDIA_MESSAGE_SETUP) => {
            let msg = Setup::parse_from_bytes(body).ok()?;
            Some(format!("{name} type={:?}", msg.type_()))
        }
        Some(MediaMessageId::MEDIA_MESSAGE_ACK) => {
            let msg = Ack::parse_from_bytes(body).ok()?;
            Some(format!(
                "{name} session_id={} ack={} receive_ts_count={}",
                msg.session_id(),
                msg.ack(),
                msg.receive_timestamp_ns.len()
            ))
        }
        Some(MediaMessageId::MEDIA_MESSAGE_CODEC_CONFIG) => {
            Some(format!("{name} bytes={}", body.len()))
        }
        Some(MediaMessageId::MEDIA_MESSAGE_DATA) => {
            Some(format!("{name} bytes={}", body.len()))
        }
        Some(MediaMessageId::MEDIA_MESSAGE_VIDEO_FOCUS_NOTIFICATION) => {
            let msg = VideoFocusNotification::parse_from_bytes(body).ok()?;
            Some(format!(
                "{name} focus={:?} unsolicited={}",
                msg.focus(),
                msg.unsolicited()
            ))
        }
        Some(MediaMessageId::MEDIA_MESSAGE_VIDEO_FOCUS_REQUEST) => {
            let msg = VideoFocusRequestNotification::parse_from_bytes(body).ok()?;
            Some(format!(
                "{name} mode={:?} reason={:?}",
                msg.mode(),
                msg.reason()
            ))
        }
        Some(MediaMessageId::MEDIA_MESSAGE_MICROPHONE_REQUEST) => {
            let msg = MicrophoneRequest::parse_from_bytes(body).ok()?;
            Some(format!(
                "{name} open={} anc={} ec={} max_unacked={}",
                msg.open(),
                msg.anc_enabled(),
                msg.ec_enabled(),
                msg.max_unacked()
            ))
        }
        Some(MediaMessageId::MEDIA_MESSAGE_MICROPHONE_RESPONSE) => {
            let msg = MicrophoneResponse::parse_from_bytes(body).ok()?;
            Some(format!(
                "{name} status={} session_id={}",
                msg.status(),
                msg.session_id()
            ))
        }
        Some(other) => Some(format!("{other:?} body_len={}", body.len())),
        None => Some(format!("UNKNOWN_MEDIA_MESSAGE id=0x{id:04x} body_len={}", body.len())),
    }
}


pub fn describe_input_payload(payload: &[u8]) -> Option<String> {
    if payload.len() < 2 {
        return None;
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    let body = &payload[2..];
    let name = input_message_name(id);

    match InputMessageId::from_i32(id as i32) {
        Some(InputMessageId::INPUT_MESSAGE_INPUT_REPORT) => {
            let msg = InputReport::parse_from_bytes(body).ok()?;
            let mut parts = Vec::new();
            parts.push(format!("{name} timestamp={} disp_channel_id={}", msg.timestamp(), msg.disp_channel_id()));
            if let Some(touch) = msg.touch_event.as_ref() {
                parts.push(format!("touchscreen={}", summarize_touch_event(touch)));
            }
            if let Some(touchpad) = msg.touchpad_event.as_ref() {
                parts.push(format!("touchpad={}", summarize_touch_event(touchpad)));
            }
            if let Some(keys) = msg.key_event.as_ref() {
                parts.push(format!("keys={}", summarize_key_event(keys)));
            }
            if let Some(abs) = msg.absolute_event.as_ref() {
                parts.push(format!("absolute={}", summarize_absolute_event(abs)));
            }
            if let Some(rel) = msg.relative_event.as_ref() {
                parts.push(format!("relative={}", summarize_relative_event(rel)));
            }
            Some(parts.join(" "))
        }
        Some(InputMessageId::INPUT_MESSAGE_KEY_BINDING_REQUEST) => {
            let msg = KeyBindingRequest::parse_from_bytes(body).ok()?;
            Some(format!("{name} keycodes={}", summarize_keycodes(&msg.keycodes)))
        }
        Some(InputMessageId::INPUT_MESSAGE_KEY_BINDING_RESPONSE) => {
            let msg = KeyBindingResponse::parse_from_bytes(body).ok()?;
            Some(format!("{name} status={}", msg.status()))
        }
        Some(InputMessageId::INPUT_MESSAGE_INPUT_FEEDBACK) => {
            let msg = InputFeedback::parse_from_bytes(body).ok()?;
            Some(format!("{name} event={:?}", msg.event()))
        }
        None => Some(format!("UNKNOWN_INPUT_MESSAGE id=0x{id:04x} body_len={}", body.len())),
    }
}

fn summarize_touch_event(touch: &TouchEvent) -> String {
    let mut points = Vec::new();
    for p in &touch.pointer_data {
        points.push(format!("id{}@{},{}", p.pointer_id(), p.x(), p.y()));
    }
    format!(
        "action={:?} action_index={} pointers=[{}]",
        touch.action(),
        touch.action_index(),
        points.join(" ")
    )
}

fn summarize_key_event(keys: &KeyEvent) -> String {
    let mut items = Vec::new();
    for key in &keys.keys {
        let code = key.keycode();
        items.push(format!(
            "{}({}) down={} meta={} longpress={}",
            code,
            keycode_name(code),
            key.down(),
            key.metastate(),
            key.longpress()
        ));
    }
    format!("[{}]", items.join("; "))
}

fn summarize_absolute_event(abs: &AbsoluteEvent) -> String {
    let mut items = Vec::new();
    for a in &abs.data {
        let code = a.keycode();
        items.push(format!("{}({})={}", code, keycode_name(code), a.value()));
    }
    format!("[{}]", items.join("; "))
}

fn summarize_relative_event(rel: &RelativeEvent) -> String {
    let mut items = Vec::new();
    for r in &rel.data {
        let code = r.keycode();
        items.push(format!("{}({}) delta={}", code, keycode_name(code), r.delta()));
    }
    format!("[{}]", items.join("; "))
}


#[derive(Debug, Clone)]
pub struct DecodedInputReport {
    pub timestamp: u64,
    pub disp_channel_id: i32,
    pub touch: Option<DecodedTouchEvent>,
    pub touchpad: Option<DecodedTouchEvent>,
    pub keys: Vec<DecodedKeyEvent>,
}

#[derive(Debug, Clone)]
pub struct DecodedTouchEvent {
    pub action: DecodedPointerAction,
    pub action_index: u32,
    pub pointers: Vec<DecodedPointer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodedPointerAction {
    Down,
    Up,
    Moved,
    PointerDown,
    PointerUp,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct DecodedPointer {
    pub x: u32,
    pub y: u32,
    pub pointer_id: u32,
}

#[derive(Debug, Clone)]
pub struct DecodedKeyEvent {
    pub keycode: u32,
    pub down: bool,
    pub metastate: u32,
    pub longpress: bool,
}

pub fn decode_input_report_payload(payload: &[u8]) -> Option<DecodedInputReport> {
    if payload.len() < 2 {
        return None;
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    if id != InputMessageId::INPUT_MESSAGE_INPUT_REPORT.value() as u16 {
        return None;
    }

    let msg = InputReport::parse_from_bytes(&payload[2..]).ok()?;
    let touch = msg.touch_event.as_ref().map(decode_touch_event);
    let touchpad = msg.touchpad_event.as_ref().map(decode_touch_event);
    let keys = msg
        .key_event
        .as_ref()
        .map(|k| {
            k.keys
                .iter()
                .map(|key| DecodedKeyEvent {
                    keycode: key.keycode(),
                    down: key.down(),
                    metastate: key.metastate(),
                    longpress: key.longpress(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(DecodedInputReport {
        timestamp: msg.timestamp(),
        disp_channel_id: msg.disp_channel_id(),
        touch,
        touchpad,
        keys,
    })
}

fn decode_touch_event(touch: &TouchEvent) -> DecodedTouchEvent {
    let action = match touch.action() {
        PointerAction::ACTION_DOWN => DecodedPointerAction::Down,
        PointerAction::ACTION_UP => DecodedPointerAction::Up,
        PointerAction::ACTION_MOVED => DecodedPointerAction::Moved,
        PointerAction::ACTION_POINTER_DOWN => DecodedPointerAction::PointerDown,
        PointerAction::ACTION_POINTER_UP => DecodedPointerAction::PointerUp,
    };
    let pointers = touch
        .pointer_data
        .iter()
        .map(|p| DecodedPointer {
            x: p.x(),
            y: p.y(),
            pointer_id: p.pointer_id(),
        })
        .collect::<Vec<_>>();
    DecodedTouchEvent {
        action,
        action_index: touch.action_index(),
        pointers,
    }
}

pub fn build_input_key_binding_request_payload(keycodes: &[i32]) -> Result<(Vec<u8>, String), protobuf::Error> {
    let mut req = KeyBindingRequest::new();
    req.keycodes = keycodes.to_vec();

    let mut out = Vec::new();
    out.extend_from_slice(&(InputMessageId::INPUT_MESSAGE_KEY_BINDING_REQUEST.value() as u16).to_be_bytes());
    out.extend_from_slice(&req.write_to_bytes()?);
    Ok((
        out,
        format!("INPUT_MESSAGE_KEY_BINDING_REQUEST keycodes={}", summarize_keycodes(keycodes)),
    ))
}

fn summarize_keycodes(keycodes: &[i32]) -> String {
    let mut items = Vec::new();
    for code in keycodes {
        items.push(format!("{}({})", code, keycode_name(*code as u32)));
    }
    format!("[{}]", items.join(", "))
}

/// Build MEDIA_MESSAGE_SETUP for the opened video sink channel.
/// Returned bytes include the 2-byte media message id followed by the protobuf body.
/// For DHU's first video sink we currently request H.264 baseline, matching the
/// advertised first video config in ServiceDiscoveryResponse.
pub fn build_media_setup_payload(codec_type: MediaCodecType) -> Result<(Vec<u8>, String), protobuf::Error> {
    let summary = format!("MEDIA_MESSAGE_SETUP type={:?}", codec_type);
    let mut setup = Setup::new();
    setup.set_type(codec_type);

    let mut out = Vec::new();
    out.extend_from_slice(&(MediaMessageId::MEDIA_MESSAGE_SETUP.value() as u16).to_be_bytes());
    out.extend_from_slice(&setup.write_to_bytes()?);

    Ok((out, summary))
}

pub fn build_video_media_setup_payload() -> Result<(Vec<u8>, String), protobuf::Error> {
    build_media_setup_payload(MediaCodecType::MEDIA_CODEC_VIDEO_H264_BP)
}

pub fn build_audio_pcm_media_setup_payload() -> Result<(Vec<u8>, String), protobuf::Error> {
    build_media_setup_payload(MediaCodecType::MEDIA_CODEC_AUDIO_PCM)
}

/// If a video media channel sends MEDIA_MESSAGE_CONFIG/STATUS_READY, build the
/// matching MEDIA_MESSAGE_START payload. Returned bytes include the 2-byte media
/// message id followed by the protobuf body.
pub fn maybe_build_media_start_from_config_payload(
    payload: &[u8],
    session_id: i32,
) -> Result<Option<(Vec<u8>, u32, u32, String)>, protobuf::Error> {
    if payload.len() < 2 {
        return Ok(None);
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    if id != MediaMessageId::MEDIA_MESSAGE_CONFIG.value() as u16 {
        return Ok(None);
    }

    let cfg = Config::parse_from_bytes(&payload[2..])?;
    if cfg.status() != self::protos::protos::config::Status::STATUS_READY {
        return Ok(None);
    }

    let configuration_index = cfg.configuration_indices.first().copied().unwrap_or(0);
    let mut start = Start::new();
    start.set_session_id(session_id);
    start.set_configuration_index(configuration_index);

    let mut out = Vec::new();
    out.extend_from_slice(&(MediaMessageId::MEDIA_MESSAGE_START.value() as u16).to_be_bytes());
    out.extend_from_slice(&start.write_to_bytes()?);
    let summary = format!(
        "MEDIA_MESSAGE_START session_id={} configuration_index={} max_unacked={}",
        session_id,
        configuration_index,
        cfg.max_unacked()
    );
    Ok(Some((out, configuration_index, cfg.max_unacked(), summary)))
}


/// Build MEDIA_MESSAGE_CODEC_CONFIG. Docs and aa-proxy media_tap both treat
/// codec config as: 2-byte media message id + raw codec bytes (H.264 SPS/PPS).
pub fn build_media_codec_config_payload(codec_config: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + codec_config.len());
    out.extend_from_slice(&(MediaMessageId::MEDIA_MESSAGE_CODEC_CONFIG.value() as u16).to_be_bytes());
    out.extend_from_slice(codec_config);
    out
}

/// Build MEDIA_MESSAGE_DATA with timestamp. Docs and aa-proxy media_tap use:
/// 2-byte id + uint64 big-endian PTS in microseconds + encoded media bytes.
pub fn build_media_data_payload(pts_us: u64, media_data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + 8 + media_data.len());
    out.extend_from_slice(&(MediaMessageId::MEDIA_MESSAGE_DATA.value() as u16).to_be_bytes());
    out.extend_from_slice(&pts_us.to_be_bytes());
    out.extend_from_slice(media_data);
    out
}

/// If payload is MEDIA_MESSAGE_ACK, return (session_id, ack_counter, summary).
pub fn parse_media_ack_payload(payload: &[u8]) -> Option<(i32, u32, String)> {
    if payload.len() < 2 {
        return None;
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    if id != MediaMessageId::MEDIA_MESSAGE_ACK.value() as u16 {
        return None;
    }
    let msg = Ack::parse_from_bytes(&payload[2..]).ok()?;
    let summary = format!(
        "MEDIA_MESSAGE_ACK session_id={} ack={} receive_ts_count={}",
        msg.session_id(),
        msg.ack(),
        msg.receive_timestamp_ns.len()
    );
    Some((msg.session_id(), msg.ack(), summary))
}


/// Parse inbound MEDIA_MESSAGE_DATA as: 2-byte media id + u64 BE PTS + bytes.
pub fn parse_media_data_payload(payload: &[u8]) -> Option<(u64, &[u8])> {
    if payload.len() < 10 {
        return None;
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    if id != MediaMessageId::MEDIA_MESSAGE_DATA.value() as u16 {
        return None;
    }
    let pts_us = u64::from_be_bytes([
        payload[2], payload[3], payload[4], payload[5],
        payload[6], payload[7], payload[8], payload[9],
    ]);
    Some((pts_us, &payload[10..]))
}

pub fn build_media_ack_payload(session_id: i32, ack_counter: u32) -> Result<(Vec<u8>, String), protobuf::Error> {
    let mut ack = Ack::new();
    ack.set_session_id(session_id);
    ack.set_ack(ack_counter);

    let mut out = Vec::new();
    out.extend_from_slice(&(MediaMessageId::MEDIA_MESSAGE_ACK.value() as u16).to_be_bytes());
    out.extend_from_slice(&ack.write_to_bytes()?);
    Ok((
        out,
        format!("MEDIA_MESSAGE_ACK session_id={} ack={}", session_id, ack_counter),
    ))
}

pub fn build_microphone_request_payload(
    open: bool,
    max_unacked: u32,
    anc_enabled: bool,
    ec_enabled: bool,
) -> Result<(Vec<u8>, String), protobuf::Error> {
    let mut req = MicrophoneRequest::new();
    req.set_open(open);
    req.set_anc_enabled(anc_enabled);
    req.set_ec_enabled(ec_enabled);
    req.set_max_unacked(max_unacked as i32);

    let mut out = Vec::new();
    out.extend_from_slice(&(MediaMessageId::MEDIA_MESSAGE_MICROPHONE_REQUEST.value() as u16).to_be_bytes());
    out.extend_from_slice(&req.write_to_bytes()?);

    Ok((
        out,
        format!(
            "MEDIA_MESSAGE_MICROPHONE_REQUEST open={} anc={} ec={} max_unacked={}",
            open, anc_enabled, ec_enabled, max_unacked
        ),
    ))
}

pub fn parse_microphone_response_payload(payload: &[u8]) -> Option<(i32, i32, String)> {
    if payload.len() < 2 {
        return None;
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    if id != MediaMessageId::MEDIA_MESSAGE_MICROPHONE_RESPONSE.value() as u16 {
        return None;
    }
    let msg = MicrophoneResponse::parse_from_bytes(&payload[2..]).ok()?;
    let summary = format!(
        "MEDIA_MESSAGE_MICROPHONE_RESPONSE status={} session_id={}",
        msg.status(),
        msg.session_id()
    );
    Some((msg.status(), msg.session_id(), summary))
}

pub fn build_microphone_response_payload(status: i32, session_id: i32) -> Result<(Vec<u8>, String), protobuf::Error> {
    let mut resp = MicrophoneResponse::new();
    resp.set_status(status);
    resp.set_session_id(session_id);

    let mut out = Vec::new();
    out.extend_from_slice(&(MediaMessageId::MEDIA_MESSAGE_MICROPHONE_RESPONSE.value() as u16).to_be_bytes());
    out.extend_from_slice(&resp.write_to_bytes()?);

    Ok((
        out,
        format!("MEDIA_MESSAGE_MICROPHONE_RESPONSE status={} session_id={}", status, session_id),
    ))
}

pub fn parse_microphone_request_payload(payload: &[u8]) -> Option<(bool, u32, String)> {
    if payload.len() < 2 {
        return None;
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    if id != MediaMessageId::MEDIA_MESSAGE_MICROPHONE_REQUEST.value() as u16 {
        return None;
    }
    let msg = MicrophoneRequest::parse_from_bytes(&payload[2..]).ok()?;
    let summary = format!(
        "MEDIA_MESSAGE_MICROPHONE_REQUEST open={} anc={} ec={} max_unacked={}",
        msg.open(),
        msg.anc_enabled(),
        msg.ec_enabled(),
        msg.max_unacked()
    );
    Some((msg.open(), msg.max_unacked().max(0) as u32, summary))
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


#[derive(Debug, Clone)]
pub struct ServiceDiscoverySummary {
    pub one_line: String,
    pub details: Vec<String>,
    pub video_media_sink: Option<MediaSinkSelection>,
    pub audio_media_sinks: Vec<AudioSinkSelection>,
    pub input_source: Option<InputSourceSelection>,
    pub media_source: Option<MediaSourceSelection>,
    pub sensor_source: Option<SensorSourceSelection>,
}

#[derive(Debug, Clone)]
pub struct MediaSinkSelection {
    pub service_id: i32,
    pub available_type: String,
    pub display_id: u32,
    pub display_type: String,
    pub video_configs: usize,
    pub first_video_config: Option<String>,
    pub video_config_selections: Vec<VideoConfigSelection>,
    pub selected_video_config: Option<VideoConfigSelection>,
}

#[derive(Debug, Clone)]
pub struct AudioSinkSelection {
    pub service_id: i32,
    pub available_type: String,
    pub audio_stream_type: String,
    pub display_id: u32,
    pub display_type: String,
    pub configs: Vec<AudioConfigSelection>,
}

#[derive(Debug, Clone)]
pub struct AudioConfigSelection {
    pub config_index: usize,
    pub sampling_rate: u32,
    pub bits: u32,
    pub channels: u32,
}

#[derive(Debug, Clone)]
pub struct MediaSourceSelection {
    pub service_id: i32,
    pub available_type: String,
    pub config: Option<AudioConfigSelection>,
}

#[derive(Debug, Clone)]
pub struct SensorSourceSelection {
    pub service_id: i32,
    pub sensors: Vec<SensorTypeInfo>,
    pub location_characterization: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct SensorTypeInfo {
    pub value: i32,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct InputSourceSelection {
    pub service_id: i32,
    pub display_id: u32,
    pub keycodes_supported: Vec<i32>,
    pub feedback_events_supported: Vec<String>,
    pub touchscreens: Vec<InputTouchscreenInfo>,
    pub touchpads: Vec<InputTouchpadInfo>,
}

#[derive(Debug, Clone)]
pub struct InputTouchscreenInfo {
    pub index: usize,
    pub width: i32,
    pub height: i32,
    pub type_name: String,
    pub is_secondary: bool,
}

#[derive(Debug, Clone)]
pub struct InputTouchpadInfo {
    pub index: usize,
    pub width: i32,
    pub height: i32,
    pub physical_width: i32,
    pub physical_height: i32,
    pub ui_navigation: bool,
    pub ui_absolute: bool,
    pub tap_as_select: bool,
    pub sensitivity: i32,
}

#[derive(Debug, Clone)]
pub struct VideoConfigSelection {
    pub config_index: usize,
    pub width: i32,
    pub height: i32,
    pub fps: i32,
    pub density: u32,
    pub real_density: u32,
    pub width_margin: u32,
    pub height_margin: u32,
    pub margin_left: u32,
    pub margin_right: u32,
    pub margin_top: u32,
    pub margin_bottom: u32,
    pub content_inset_left: u32,
    pub content_inset_right: u32,
    pub content_inset_top: u32,
    pub content_inset_bottom: u32,
    pub stable_inset_left: u32,
    pub stable_inset_right: u32,
    pub stable_inset_top: u32,
    pub stable_inset_bottom: u32,
    pub content_width: i32,
    pub content_height: i32,
    pub safe_width: i32,
    pub safe_height: i32,
    pub codec: String,
    pub resolution: String,
    pub summary: String,
}

fn clamp_dim(v: i32) -> i32 {
    v.max(1)
}

fn insets_tuple(insets: Option<&Insets>) -> (u32, u32, u32, u32) {
    match insets {
        Some(i) => (i.left(), i.right(), i.top(), i.bottom()),
        None => (0, 0, 0, 0),
    }
}

fn video_config_layout(vc: &VideoConfiguration, width: i32, height: i32) -> (u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, u32, i32, i32, i32, i32) {
    let ui = vc.ui_config.as_ref();
    let (mut margin_left, mut margin_right, mut margin_top, mut margin_bottom) =
        insets_tuple(ui.and_then(|u| u.margins.as_ref()));

    // Older/leaner HU configs sometimes only populate total width_margin/height_margin.
    // Split those totals symmetrically when per-side UiConfig margins are absent.
    if margin_left == 0 && margin_right == 0 && vc.width_margin() > 0 {
        margin_left = vc.width_margin() / 2;
        margin_right = vc.width_margin() - margin_left;
    }
    if margin_top == 0 && margin_bottom == 0 && vc.height_margin() > 0 {
        margin_top = vc.height_margin() / 2;
        margin_bottom = vc.height_margin() - margin_top;
    }

    let (content_inset_left, content_inset_right, content_inset_top, content_inset_bottom) =
        insets_tuple(ui.and_then(|u| u.content_insets.as_ref()));
    let (stable_inset_left, stable_inset_right, stable_inset_top, stable_inset_bottom) =
        insets_tuple(ui.and_then(|u| u.stable_content_insets.as_ref()));

    let content_width = clamp_dim(width - (margin_left + margin_right) as i32);
    let content_height = clamp_dim(height - (margin_top + margin_bottom) as i32);
    let safe_width = clamp_dim(content_width - (content_inset_left + content_inset_right) as i32);
    let safe_height = clamp_dim(content_height - (content_inset_top + content_inset_bottom) as i32);

    (
        margin_left, margin_right, margin_top, margin_bottom,
        content_inset_left, content_inset_right, content_inset_top, content_inset_bottom,
        stable_inset_left, stable_inset_right, stable_inset_top, stable_inset_bottom,
        content_width, content_height, safe_width, safe_height,
    )
}


fn video_resolution_dimensions(res: VideoCodecResolutionType) -> Option<(i32, i32)> {
    match res {
        VideoCodecResolutionType::VIDEO_800x480 => Some((800, 480)),
        VideoCodecResolutionType::VIDEO_1280x720 => Some((1280, 720)),
        VideoCodecResolutionType::VIDEO_1920x1080 => Some((1920, 1080)),
        VideoCodecResolutionType::VIDEO_2560x1440 => Some((2560, 1440)),
        VideoCodecResolutionType::VIDEO_3840x2160 => Some((3840, 2160)),
        VideoCodecResolutionType::VIDEO_720x1280 => Some((720, 1280)),
        VideoCodecResolutionType::VIDEO_1080x1920 => Some((1080, 1920)),
        VideoCodecResolutionType::VIDEO_1440x2560 => Some((1440, 2560)),
        VideoCodecResolutionType::VIDEO_2160x3840 => Some((2160, 3840)),
    }
}

fn video_fps_value(fps: VideoFrameRateType) -> i32 {
    match fps {
        VideoFrameRateType::VIDEO_FPS_60 => 60,
        VideoFrameRateType::VIDEO_FPS_30 => 30,
    }
}

fn video_config_summary(index: usize, vc: &VideoConfiguration) -> String {
    let (width, height) = video_resolution_dimensions(vc.codec_resolution()).unwrap_or((0, 0));
    let (
        margin_left, margin_right, margin_top, margin_bottom,
        content_inset_left, content_inset_right, content_inset_top, content_inset_bottom,
        stable_inset_left, stable_inset_right, stable_inset_top, stable_inset_bottom,
        content_width, content_height, safe_width, safe_height,
    ) = video_config_layout(vc, width, height);
    format!(
        "index={} resolution={:?} {}x{} fps={:?}/{} density={} real_density={} total_margins={}x{} margins=L{} R{} T{} B{} content={}x{} content_insets=L{} R{} T{} B{} stable_insets=L{} R{} T{} B{} safe={}x{} video_codec={:?}",
        index,
        vc.codec_resolution(),
        width,
        height,
        vc.frame_rate(),
        video_fps_value(vc.frame_rate()),
        vc.density(),
        vc.real_density(),
        vc.width_margin(),
        vc.height_margin(),
        margin_left, margin_right, margin_top, margin_bottom,
        content_width, content_height,
        content_inset_left, content_inset_right, content_inset_top, content_inset_bottom,
        stable_inset_left, stable_inset_right, stable_inset_top, stable_inset_bottom,
        safe_width, safe_height,
        vc.video_codec_type()
    )
}

fn make_video_config_selection(index: usize, vc: &VideoConfiguration) -> Option<VideoConfigSelection> {
    let (width, height) = video_resolution_dimensions(vc.codec_resolution())?;
    let fps = video_fps_value(vc.frame_rate());
    let codec = format!("{:?}", vc.video_codec_type());
    let resolution = format!("{:?}", vc.codec_resolution());
    let summary = video_config_summary(index, vc);
    let (
        margin_left, margin_right, margin_top, margin_bottom,
        content_inset_left, content_inset_right, content_inset_top, content_inset_bottom,
        stable_inset_left, stable_inset_right, stable_inset_top, stable_inset_bottom,
        content_width, content_height, safe_width, safe_height,
    ) = video_config_layout(vc, width, height);
    Some(VideoConfigSelection {
        config_index: index,
        width,
        height,
        fps,
        density: vc.density(),
        real_density: vc.real_density(),
        width_margin: vc.width_margin(),
        height_margin: vc.height_margin(),
        margin_left,
        margin_right,
        margin_top,
        margin_bottom,
        content_inset_left,
        content_inset_right,
        content_inset_top,
        content_inset_bottom,
        stable_inset_left,
        stable_inset_right,
        stable_inset_top,
        stable_inset_bottom,
        content_width,
        content_height,
        safe_width,
        safe_height,
        codec,
        resolution,
        summary,
    })
}

pub fn normalize_video_config_policy(policy: &str) -> String {
    let p = policy.trim();
    if p.is_empty() {
        return "balanced".to_string();
    }
    let lower = p
        .to_ascii_lowercase()
        .replace('×', "x")
        .replace("video_", "");
    if let Some(rest) = lower.strip_prefix("index:") {
        if rest.parse::<usize>().is_ok() {
            return lower;
        }
    }
    for prefix in ["resolution:", "size:"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if parse_video_size(rest).is_some() {
                return format!("resolution:{rest}");
            }
        }
    }
    if parse_video_size(&lower).is_some() {
        return format!("resolution:{lower}");
    }
    match lower.as_str() {
        "highest" | "max" | "best" => "highest".to_string(),
        "balanced" | "medium" | "mid" | "auto" | "compatible" => {
            "balanced".to_string()
        }
        "lowest" | "min" | "safe" => "lowest".to_string(),
        _ => "balanced".to_string(),
    }
}

fn parse_video_size(value: &str) -> Option<(i32, i32)> {
    let (width, height) = value.split_once('x')?;
    let width = width.parse::<i32>().ok()?;
    let height = height.parse::<i32>().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

fn select_video_config(configs: &[VideoConfiguration], policy: &str) -> Option<VideoConfigSelection> {
    let normalized = normalize_video_config_policy(policy);
    let mut selections: Vec<VideoConfigSelection> = configs
        .iter()
        .enumerate()
        .filter_map(|(index, vc)| make_video_config_selection(index, vc))
        .collect();
    if selections.is_empty() {
        return None;
    }

    if let Some(rest) = normalized.strip_prefix("index:") {
        if let Ok(index) = rest.parse::<usize>() {
            if let Some(found) = selections.iter().find(|s| s.config_index == index) {
                return Some(found.clone());
            }
        }
    }

    if let Some(rest) = normalized.strip_prefix("resolution:") {
        if let Some((width, height)) = parse_video_size(rest) {
            if let Some(found) = selections
                .iter()
                .filter(|selection| selection.width == width && selection.height == height)
                .max_by_key(|selection| selection.fps)
            {
                return Some(found.clone());
            }
        }
        // A forced resolution missing from the HU response falls back to the
        // highest locally compatible quality rather than accidentally taking
        // an expensive 1080p/4K profile.
        return select_video_config(configs, "balanced");
    }

    selections.sort_by_key(|s| ((s.width as i64) * (s.height as i64), s.fps as i64));
    match normalized.as_str() {
        "lowest" => selections.into_iter().next(),
        "balanced" => {
            let mut candidates: Vec<VideoConfigSelection> = selections
                .iter()
                .cloned()
                .filter(|s| s.width <= 1280 && s.height <= 720)
                .collect();
            if candidates.is_empty() {
                selections.into_iter().next()
            } else {
                candidates.sort_by_key(|s| ((s.width as i64) * (s.height as i64), s.fps as i64));
                candidates.into_iter().last()
            }
        }
        _ => selections.into_iter().last(),
    }
}

/// Parse a MESSAGE_SERVICE_DISCOVERY_RESPONSE control payload, select the first
/// HU video MediaSinkService, and return human-readable details for logs.
/// `payload` includes the 2-byte control message id.
pub fn parse_service_discovery_response_control_payload(
    payload: &[u8],
    video_config_policy: &str,
) -> Result<Option<ServiceDiscoverySummary>, protobuf::Error> {
    if payload.len() < 2 {
        return Ok(None);
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    if id != ControlMessageType::MESSAGE_SERVICE_DISCOVERY_RESPONSE.value() as u16 {
        return Ok(None);
    }
    summarize_service_discovery_response_body(&payload[2..], video_config_policy).map(Some)
}

fn summarize_service_discovery_response_body(
    body: &[u8],
    video_config_policy: &str,
) -> Result<ServiceDiscoverySummary, protobuf::Error> {
    let msg = ServiceDiscoveryResponse::parse_from_bytes(body)?;
    let mut details = Vec::new();
    let mut video_media_sink = None;
    let mut audio_media_sinks: Vec<AudioSinkSelection> = Vec::new();
    let mut input_source = None;
    let mut media_source = None;
    let mut sensor_source = None;

    details.push(format!(
        "SDR headunit display_name={:?} driver_position={:?} services={}",
        msg.display_name(),
        msg.driver_position(),
        msg.services.len()
    ));

    for svc in &msg.services {
        let mut kinds = Vec::new();

        if let Some(sink) = svc.media_sink_service.as_ref() {
            let video_count = sink.video_configs.len();
            let audio_count = sink.audio_configs.len();
            let available_type = format!("{:?}", sink.available_type());
            let display_type = format!("{:?}", sink.display_type());
            kinds.push(format!(
                "media_sink type={} audio_type={:?} audio_configs={} video_configs={} display_id={} display_type={}",
                available_type,
                sink.audio_type(),
                audio_count,
                video_count,
                sink.display_id(),
                display_type
            ));

            let looks_like_video = video_count > 0
                || matches!(
                    sink.available_type(),
                    MediaCodecType::MEDIA_CODEC_VIDEO_H264_BP
                        | MediaCodecType::MEDIA_CODEC_VIDEO_VP9
                        | MediaCodecType::MEDIA_CODEC_VIDEO_AV1
                        | MediaCodecType::MEDIA_CODEC_VIDEO_H265
                );

            if looks_like_video && video_media_sink.is_none() {
                let first_video_config = sink
                    .video_configs
                    .first()
                    .map(|vc| video_config_summary(0, vc));
                let video_config_selections = sink
                    .video_configs
                    .iter()
                    .enumerate()
                    .filter_map(|(index, vc)| make_video_config_selection(index, vc))
                    .collect::<Vec<_>>();
                let selected_video_config =
                    select_video_config(&sink.video_configs, video_config_policy);
                video_media_sink = Some(MediaSinkSelection {
                    service_id: svc.id(),
                    available_type: available_type.clone(),
                    display_id: sink.display_id(),
                    display_type: display_type.clone(),
                    video_configs: video_count,
                    first_video_config,
                    video_config_selections,
                    selected_video_config,
                });
            }

            if audio_count > 0 {
                let configs = sink
                    .audio_configs
                    .iter()
                    .enumerate()
                    .map(|(index, cfg)| AudioConfigSelection {
                        config_index: index,
                        sampling_rate: cfg.sampling_rate(),
                        bits: cfg.number_of_bits(),
                        channels: cfg.number_of_channels(),
                    })
                    .collect::<Vec<_>>();
                for cfg in &configs {
                    details.push(format!(
                        "    audio sink config[{}] service_id={} stream={:?} rate={} bits={} channels={}",
                        cfg.config_index,
                        svc.id(),
                        sink.audio_type(),
                        cfg.sampling_rate,
                        cfg.bits,
                        cfg.channels
                    ));
                }
                audio_media_sinks.push(AudioSinkSelection {
                    service_id: svc.id(),
                    available_type: available_type.clone(),
                    audio_stream_type: format!("{:?}", sink.audio_type()),
                    display_id: sink.display_id(),
                    display_type: display_type.clone(),
                    configs,
                });
            }
        }
        if let Some(input) = svc.input_source_service.as_ref() {
            let feedback_events_supported: Vec<String> = input
                .feedback_events_supported
                .iter()
                .map(|event| format!("{:?}", event))
                .collect();
            let touchscreens: Vec<InputTouchscreenInfo> = input
                .touchscreen
                .iter()
                .enumerate()
                .map(|(index, ts)| InputTouchscreenInfo {
                    index,
                    width: ts.width(),
                    height: ts.height(),
                    type_name: format!("{:?}", ts.type_()),
                    is_secondary: ts.is_secondary(),
                })
                .collect();
            let touchpads: Vec<InputTouchpadInfo> = input
                .touchpad
                .iter()
                .enumerate()
                .map(|(index, tp)| InputTouchpadInfo {
                    index,
                    width: tp.width(),
                    height: tp.height(),
                    physical_width: tp.physical_width(),
                    physical_height: tp.physical_height(),
                    ui_navigation: tp.ui_navigation(),
                    ui_absolute: tp.ui_absolute(),
                    tap_as_select: tp.tap_as_select(),
                    sensitivity: tp.sensitivity(),
                })
                .collect();

            kinds.push(format!(
                "input_source touchscreens={} touchpads={} keycodes={} display_id={} feedback_events={}",
                touchscreens.len(),
                touchpads.len(),
                input.keycodes_supported.len(),
                input.display_id(),
                feedback_events_supported.join(",")
            ));
            if !input.keycodes_supported.is_empty() {
                kinds.push(format!("input_keycodes={}", summarize_keycodes(&input.keycodes_supported)));
            }
            for ts in &touchscreens {
                details.push(format!(
                    "    input touchscreen[{}] {}x{} type={} secondary={}",
                    ts.index,
                    ts.width,
                    ts.height,
                    ts.type_name,
                    ts.is_secondary
                ));
            }
            for tp in &touchpads {
                details.push(format!(
                    "    input touchpad[{}] {}x{} physical={}x{} ui_navigation={} ui_absolute={} tap_as_select={} sensitivity={}",
                    tp.index,
                    tp.width,
                    tp.height,
                    tp.physical_width,
                    tp.physical_height,
                    tp.ui_navigation,
                    tp.ui_absolute,
                    tp.tap_as_select,
                    tp.sensitivity
                ));
            }
            if input_source.is_none() {
                input_source = Some(InputSourceSelection {
                    service_id: svc.id(),
                    display_id: input.display_id(),
                    keycodes_supported: input.keycodes_supported.clone(),
                    feedback_events_supported,
                    touchscreens,
                    touchpads,
                });
            }
        }
        if let Some(sensor) = svc.sensor_source_service.as_ref() {
            let sensors = sensor
                .sensors
                .iter()
                .map(|entry| {
                    let ty = entry.sensor_type();
                    SensorTypeInfo {
                        value: ty.value(),
                        name: format!("{:?}", ty),
                    }
                })
                .collect::<Vec<_>>();
            let location_characterization = sensor
                .has_location_characterization()
                .then(|| sensor.location_characterization());
            kinds.push(format!(
                "sensor_source sensors={} [{}] location_characterization={:?}",
                sensors.len(),
                sensors
                    .iter()
                    .map(|sensor| format!("{}({})", sensor.name, sensor.value))
                    .collect::<Vec<_>>()
                    .join(","),
                location_characterization,
            ));
            if sensor_source.is_none() {
                sensor_source = Some(SensorSourceSelection {
                    service_id: svc.id(),
                    sensors,
                    location_characterization,
                });
            }
        }
        if let Some(source) = svc.media_source_service.as_ref() {
            kinds.push(format!("media_source type={:?}", source.available_type()));
            if let Some(cfg) = source.audio_config.as_ref() {
                details.push(format!(
                    "    media_source audio config service_id={} type={:?} rate={} bits={} channels={}",
                    svc.id(),
                    source.available_type(),
                    cfg.sampling_rate(),
                    cfg.number_of_bits(),
                    cfg.number_of_channels()
                ));
            }
            if media_source.is_none() {
                media_source = Some(MediaSourceSelection {
                    service_id: svc.id(),
                    available_type: format!("{:?}", source.available_type()),
                    config: source.audio_config.as_ref().map(|cfg| AudioConfigSelection {
                        config_index: 0,
                        sampling_rate: cfg.sampling_rate(),
                        bits: cfg.number_of_bits(),
                        channels: cfg.number_of_channels(),
                    }),
                });
            }
        }
        if svc.bluetooth_service.is_some() {
            kinds.push("bluetooth".to_string());
        }
        if let Some(nav) = svc.navigation_status_service.as_ref() {
            kinds.push(format!(
                "navigation_status type={:?} min_interval_ms={}",
                nav.type_(),
                nav.minimum_interval_ms()
            ));
        }
        if svc.media_playback_service.is_some() {
            kinds.push("media_playback".to_string());
        }
        if svc.phone_status_service.is_some() {
            kinds.push("phone_status".to_string());
        }
        if svc.media_browser_service.is_some() {
            kinds.push("media_browser".to_string());
        }
        if let Some(vendor) = svc.vendor_extension_service.as_ref() {
            kinds.push(format!("vendor_extension name={:?}", vendor.service_name()));
        }
        if svc.generic_notification_service.is_some() {
            kinds.push("generic_notification".to_string());
        }
        if svc.wifi_projection_service.is_some() {
            kinds.push("wifi_projection".to_string());
        }

        let kinds = if kinds.is_empty() {
            "unknown".to_string()
        } else {
            kinds.join("; ")
        };
        details.push(format!("  service id={} {kinds}", svc.id()));
    }

    let one_line = if let Some(video) = &video_media_sink {
        format!(
            "MESSAGE_SERVICE_DISCOVERY_RESPONSE services={} selected_video_service={} type={} display_id={} display_type={} video_configs={}",
            msg.services.len(),
            video.service_id,
            video.available_type,
            video.display_id,
            video.display_type,
            video.video_configs
        )
    } else {
        format!(
            "MESSAGE_SERVICE_DISCOVERY_RESPONSE services={} selected_video_service=none",
            msg.services.len()
        )
    };

    Ok(ServiceDiscoverySummary {
        one_line,
        details,
        video_media_sink,
        audio_media_sinks,
        input_source,
        media_source,
        sensor_source,
    })
}

/// Build a MESSAGE_CHANNEL_OPEN_REQUEST control payload for a HU service id.
/// Returned bytes include the 2-byte AAP control message id followed by the
/// protobuf body.
pub fn build_channel_open_request_control_payload(
    service_id: i32,
    priority: i32,
) -> Result<Vec<u8>, protobuf::Error> {
    let mut req = ChannelOpenRequest::new();
    req.set_priority(priority);
    req.set_service_id(service_id);

    let mut payload = Vec::new();
    payload.extend_from_slice(&(ControlMessageType::MESSAGE_CHANNEL_OPEN_REQUEST.value() as u16).to_be_bytes());
    payload.extend_from_slice(&req.write_to_bytes()?);
    Ok(payload)
}

pub fn build_sensor_request_payload(
    sensor_type: i32,
    min_update_period_ms: i64,
) -> Result<(Vec<u8>, String), protobuf::Error> {
    let mut req = SensorRequest::new();
    let sensor_enum = SensorType::from_i32(sensor_type).unwrap_or(SensorType::SENSOR_LOCATION);
    req.set_type(sensor_enum);
    req.set_min_update_period(min_update_period_ms);

    let mut payload = Vec::new();
    payload.extend_from_slice(&(SensorMessageId::SENSOR_MESSAGE_REQUEST.value() as u16).to_be_bytes());
    payload.extend_from_slice(&req.write_to_bytes()?);
    Ok((
        payload,
        format!(
            "SENSOR_MESSAGE_REQUEST type={}({}) min_update_period_ms={}",
            sensor_type_name(sensor_type),
            sensor_type,
            min_update_period_ms
        ),
    ))
}

pub fn parse_sensor_response_payload(payload: &[u8]) -> Option<String> {
    if payload.len() < 2 {
        return None;
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    if id != SensorMessageId::SENSOR_MESSAGE_RESPONSE.value() as u16 {
        return None;
    }
    let msg = SensorResponse::parse_from_bytes(&payload[2..]).ok()?;
    Some(format!("SENSOR_MESSAGE_RESPONSE status={:?}", msg.status()))
}

pub fn parse_sensor_error_payload(payload: &[u8]) -> Option<String> {
    if payload.len() < 2 {
        return None;
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    if id != SensorMessageId::SENSOR_MESSAGE_ERROR.value() as u16 {
        return None;
    }
    let msg = SensorError::parse_from_bytes(&payload[2..]).ok()?;
    Some(format!(
        "SENSOR_MESSAGE_ERROR type={}({}) error={:?}",
        sensor_type_name(msg.sensor_type().value()),
        msg.sensor_type().value(),
        msg.sensor_error_type()
    ))
}

#[derive(Debug, Clone)]
pub struct DecodedLocationFix {
    pub timestamp_raw: Option<u64>,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub accuracy_m: Option<f64>,
    pub altitude_m: Option<f64>,
    pub speed_mps: Option<f64>,
    pub bearing_deg: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct SensorBatchSummary {
    pub summary: String,
    pub json_line: String,
    pub location: Option<DecodedLocationFix>,
    pub events: Vec<ArdaSensorEvent>,
}

fn android_vehicle_gear_from_aap(gear: Gear) -> Option<i32> {
    // android.car.VehicleGear constants are bit flags:
    // UNKNOWN=0, NEUTRAL=1, REVERSE=2, PARK=4, DRIVE=8, FIRST=16, SECOND=32, ...
    match gear {
        Gear::GEAR_NEUTRAL => Some(0x0001),
        Gear::GEAR_REVERSE => Some(0x0002),
        Gear::GEAR_PARK => Some(0x0004),
        Gear::GEAR_DRIVE => Some(0x0008),
        Gear::GEAR_1 => Some(0x0010),
        Gear::GEAR_2 => Some(0x0020),
        Gear::GEAR_3 => Some(0x0040),
        Gear::GEAR_4 => Some(0x0080),
        Gear::GEAR_5 => Some(0x0100),
        Gear::GEAR_6 => Some(0x0200),
        Gear::GEAR_7 => Some(0x0400),
        Gear::GEAR_8 => Some(0x0800),
        Gear::GEAR_9 => Some(0x1000),
        // Android currently exposes up to GEAR_NINTH in android.car.VehicleGear.
        Gear::GEAR_10 => None,
    }
}

fn json_u64_option(value: Option<u64>) -> String {
    value.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string())
}

fn json_f64_option(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.6}")).unwrap_or_else(|| "null".to_string())
}

pub fn parse_sensor_batch_payload(payload: &[u8]) -> Option<SensorBatchSummary> {
    if payload.len() < 2 {
        return None;
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    if id != SensorMessageId::SENSOR_MESSAGE_BATCH.value() as u16 {
        return None;
    }
    let msg = SensorBatch::parse_from_bytes(&payload[2..]).ok()?;
    let counts = [
        (1, "location", msg.location_data.len()),
        (2, "compass", msg.compass_data.len()),
        (3, "speed", msg.speed_data.len()),
        (4, "rpm", msg.rpm_data.len()),
        (5, "odometer", msg.odometer_data.len()),
        (6, "fuel", msg.fuel_data.len()),
        (7, "parking_brake", msg.parking_brake_data.len()),
        (8, "gear", msg.gear_data.len()),
        (9, "obdii", msg.diagnostics_data.len()),
        (10, "night", msg.night_mode_data.len()),
        (11, "environment", msg.environment_data.len()),
        (12, "hvac", msg.hvac_data.len()),
        (13, "driving_status", msg.driving_status_data.len()),
        (14, "dead_reckoning", msg.dead_reckoning_data.len()),
        (15, "passenger", msg.passenger_data.len()),
        (16, "door", msg.door_data.len()),
        (17, "light", msg.light_data.len()),
        (18, "tire_pressure", msg.tire_pressure_data.len()),
        (19, "accelerometer", msg.accelerometer_data.len()),
        (20, "gyroscope", msg.gyroscope_data.len()),
        (21, "gps_satellite", msg.gps_satellite_data.len()),
        (22, "toll_card", msg.toll_card_data.len()),
        (23, "energy_model", msg.energy_model_control.len()),
    ];
    let populated = counts
        .iter()
        .filter(|(_, _, count)| *count > 0)
        .map(|(ty, name, count)| format!("{}:{}={}", ty, name, count))
        .collect::<Vec<_>>();
    if populated.is_empty() {
        return Some(SensorBatchSummary {
            summary: "SENSOR_MESSAGE_BATCH empty".to_string(),
            json_line: "{\"type\":\"sensor_batch\",\"empty\":true}".to_string(),
            location: None,
            events: Vec::new(),
        });
    }

    let mut details = Vec::new();
    let mut json_fields = Vec::new();
    let mut events = Vec::new();
    json_fields.push("\"type\":\"sensor_batch\"".to_string());

    let location = msg.location_data.last().map(|loc| {
        let fix = DecodedLocationFix {
            timestamp_raw: loc.has_timestamp().then(|| loc.timestamp()),
            latitude_deg: loc.latitude_e7() as f64 / 10_000_000.0,
            longitude_deg: loc.longitude_e7() as f64 / 10_000_000.0,
            accuracy_m: loc.has_accuracy_e3().then(|| loc.accuracy_e3() as f64 / 1000.0),
            altitude_m: loc.has_altitude_e2().then(|| loc.altitude_e2() as f64 / 100.0),
            speed_mps: loc.has_speed_e3().then(|| loc.speed_e3() as f64 / 1000.0),
            bearing_deg: loc.has_bearing_e6().then(|| loc.bearing_e6() as f64 / 1_000_000.0),
        };
        details.push(format!(
            "location(timestamp_raw={:?},lat={:.7},lon={:.7},acc_m={:?},alt_m={:?},speed_mps={:?},bearing={:?})",
            fix.timestamp_raw, fix.latitude_deg, fix.longitude_deg, fix.accuracy_m,
            fix.altitude_m, fix.speed_mps, fix.bearing_deg,
        ));
        json_fields.push(format!(
            "\"location\":{{\"timestamp_raw\":{},\"lat\":{:.7},\"lon\":{:.7},\"accuracy_m\":{},\"altitude_m\":{},\"speed_mps\":{},\"bearing_deg\":{}}}",
            json_u64_option(fix.timestamp_raw), fix.latitude_deg, fix.longitude_deg,
            json_f64_option(fix.accuracy_m), json_f64_option(fix.altitude_m),
            json_f64_option(fix.speed_mps), json_f64_option(fix.bearing_deg),
        ));
        events.push(ArdaSensorEvent::Location {
            timestamp_raw: fix.timestamp_raw,
            latitude_deg: fix.latitude_deg,
            longitude_deg: fix.longitude_deg,
            accuracy_m: fix.accuracy_m,
            altitude_m: fix.altitude_m,
            speed_mps: fix.speed_mps,
            bearing_deg: fix.bearing_deg,
        });
        fix
    });
    if let Some(compass) = msg.compass_data.last() {
        let bearing_deg = compass.bearing_e6() as f64 / 1_000_000.0;
        let pitch_deg = compass.pitch_e6() as f64 / 1_000_000.0;
        let roll_deg = compass.roll_e6() as f64 / 1_000_000.0;
        details.push(format!("compass_bearing={:.1} pitch={:.1} roll={:.1}", bearing_deg, pitch_deg, roll_deg));
        json_fields.push(format!("\"compass\":{{\"bearing_deg\":{:.3},\"pitch_deg\":{:.3},\"roll_deg\":{:.3}}}", bearing_deg, pitch_deg, roll_deg));
        events.push(ArdaSensorEvent::Compass { bearing_deg, pitch_deg, roll_deg });
    }
    if let Some(speed) = msg.speed_data.last() {
        let speed_mps = speed.speed_e3() as f64 / 1000.0;
        details.push(format!("speed_mps={:.2}", speed_mps));
        json_fields.push(format!("\"speed_mps\":{:.3}", speed_mps));
        events.push(ArdaSensorEvent::Speed {
            speed_mps,
            cruise_engaged: speed.cruise_engaged(),
            cruise_set_speed_raw: speed.cruise_set_speed(),
        });
    }
    if let Some(rpm) = msg.rpm_data.last() {
        let rpm_value = rpm.rpm_e3() as f64 / 1000.0;
        details.push(format!("rpm={:.0}", rpm_value));
        json_fields.push(format!("\"rpm\":{:.3}", rpm_value));
        events.push(ArdaSensorEvent::Rpm { rpm: rpm_value });
    }
    if let Some(odo) = msg.odometer_data.last() {
        let odometer_km = odo.kms_e1() as f64 / 10.0;
        let trip_km = odo.trip_kms_e1() as f64 / 10.0;
        details.push(format!("odometer_km={:.1} trip_km={:.1}", odometer_km, trip_km));
        json_fields.push(format!("\"odometer\":{{\"km\":{:.1},\"trip_km\":{:.1}}}", odometer_km, trip_km));
        events.push(ArdaSensorEvent::Odometer { kilometers: odometer_km, trip_kilometers: trip_km });
    }
    if let Some(fuel) = msg.fuel_data.last() {
        let low_fuel_warning_present = fuel.has_low_fuel_warning();
        details.push(format!(
            "fuel={} range={} low={} low_present={}",
            fuel.fuel_level(),
            fuel.range(),
            fuel.low_fuel_warning(),
            low_fuel_warning_present,
        ));
        json_fields.push(format!(
            "\"fuel\":{{\"level\":{},\"range\":{},\"low\":{},\"low_present\":{}}}",
            fuel.fuel_level(),
            fuel.range(),
            fuel.low_fuel_warning(),
            low_fuel_warning_present,
        ));
        events.push(ArdaSensorEvent::Fuel {
            level_raw: fuel.fuel_level(),
            range_raw: fuel.range(),
            low_fuel_warning: fuel.low_fuel_warning(),
            low_fuel_warning_present,
        });
    }
    if let Some(pb) = msg.parking_brake_data.last() {
        let value = pb.parking_brake();
        details.push(format!("parking_brake={}", value));
        json_fields.push(format!("\"parking_brake\":{}", value));
        events.push(ArdaSensorEvent::ParkingBrake { engaged: value });
    }
    if let Some(gear) = msg.gear_data.last() {
        let aap_gear = gear.gear();
        let android_gear = android_vehicle_gear_from_aap(aap_gear);
        details.push(format!("gear={:?} android_gear={:?}", aap_gear, android_gear));
        json_fields.push(format!("\"gear\":\"{:?}\"", aap_gear));
        events.push(ArdaSensorEvent::Gear {
            aap_gear: aap_gear.value(),
            android_gear,
        });
    }
    if let Some(diagnostics) = msg.diagnostics_data.last() {
        let dtc = diagnostics.dtc().to_vec();
        details.push(format!("diagnostics_dtc_bytes={}", dtc.len()));
        json_fields.push(format!("\"diagnostics\":{{\"dtc_bytes\":{}}}", dtc.len()));
        events.push(ArdaSensorEvent::Diagnostics { dtc });
    }
    if let Some(night) = msg.night_mode_data.last() {
        let value = night.night_mode();
        let enabled_present = night.has_night_mode();
        details.push(format!(
            "night_mode={} night_mode_present={}",
            value, enabled_present
        ));
        json_fields.push(format!(
            "\"night_mode\":{},\"night_mode_present\":{}",
            value, enabled_present
        ));
        events.push(ArdaSensorEvent::NightMode {
            enabled: value,
            enabled_present,
        });
    }
    if let Some(env) = msg.environment_data.last() {
        let temperature_c = env.temperature_e3() as f64 / 1000.0;
        let pressure = env.pressure_e3() as f64 / 1000.0;
        details.push(format!("env_temp_c={:.1} pressure={:.3} rain={}", temperature_c, pressure, env.rain()));
        json_fields.push(format!("\"environment\":{{\"temperature_c\":{:.3},\"pressure\":{:.3},\"rain\":{}}}", temperature_c, pressure, env.rain()));
        events.push(ArdaSensorEvent::Environment { temperature_c, pressure, rain_raw: env.rain() });
    }
    if let Some(hvac) = msg.hvac_data.last() {
        let target_temperature_c = hvac.target_temperature_e3() as f64 / 1000.0;
        let current_temperature_c = hvac.current_temperature_e3() as f64 / 1000.0;
        details.push(format!("hvac_target_c={:.1} hvac_current_c={:.1}", target_temperature_c, current_temperature_c));
        json_fields.push(format!("\"hvac\":{{\"target_temperature_c\":{:.3},\"current_temperature_c\":{:.3}}}", target_temperature_c, current_temperature_c));
        events.push(ArdaSensorEvent::Hvac { target_temperature_c, current_temperature_c });
    }
    if let Some(status) = msg.driving_status_data.last() {
        details.push(format!("driving_status=0x{:x}", status.status()));
        json_fields.push(format!("\"driving_status\":{}", status.status()));
        events.push(ArdaSensorEvent::DrivingStatus { status: status.status() });
    }
    if let Some(dead) = msg.dead_reckoning_data.last() {
        let steering_angle_deg = dead.steering_angle_e1() as f64 / 10.0;
        let wheel_speed_mps = dead.wheel_speed_e3.iter().map(|v| *v as f64 / 1000.0).collect::<Vec<_>>();
        details.push(format!("dead_reckoning_steering={:.1} wheel_speed_mps={:?}", steering_angle_deg, wheel_speed_mps));
        json_fields.push(format!("\"dead_reckoning\":{{\"steering_angle_deg\":{:.1},\"wheel_count\":{}}}", steering_angle_deg, wheel_speed_mps.len()));
        events.push(ArdaSensorEvent::DeadReckoning { steering_angle_deg, wheel_speed_mps });
    }
    if let Some(passenger) = msg.passenger_data.last() {
        let present = passenger.passenger_present();
        details.push(format!("passenger_present={}", present));
        json_fields.push(format!("\"passenger_present\":{}", present));
        events.push(ArdaSensorEvent::Passenger { present });
    }
    if let Some(door) = msg.door_data.last() {
        let door_open = door.door_open.iter().copied().collect::<Vec<_>>();
        details.push(format!("hood_open={} trunk_open={} door_open={:?}", door.hood_open(), door.trunk_open(), door_open));
        json_fields.push(format!("\"door\":{{\"hood_open\":{},\"trunk_open\":{},\"door_count\":{}}}", door.hood_open(), door.trunk_open(), door_open.len()));
        events.push(ArdaSensorEvent::Door { hood_open: door.hood_open(), trunk_open: door.trunk_open(), door_open });
    }
    if let Some(light) = msg.light_data.last() {
        let head_light_state = light.head_light_state().value();
        let turn_indicator_state = light.turn_indicator_state().value();
        details.push(format!("head_light={:?} turn_indicator={:?} hazard={}", light.head_light_state(), light.turn_indicator_state(), light.hazard_lights_on()));
        json_fields.push(format!("\"light\":{{\"head_light_state\":{},\"turn_indicator_state\":{},\"hazard\":{}}}", head_light_state, turn_indicator_state, light.hazard_lights_on()));
        events.push(ArdaSensorEvent::Light { head_light_state, turn_indicator_state, hazard_lights_on: light.hazard_lights_on() });
    }
    if let Some(tire) = msg.tire_pressure_data.last() {
        let pressures = tire.tire_pressures_e2.iter().map(|v| *v as f64 / 100.0).collect::<Vec<_>>();
        details.push(format!("tire_pressure={:?}", pressures));
        json_fields.push(format!("\"tire_pressure\":{{\"count\":{}}}", pressures.len()));
        events.push(ArdaSensorEvent::TirePressure { pressures });
    }
    if let Some(acc) = msg.accelerometer_data.last() {
        let x = acc.acceleration_x_e3() as f64 / 1000.0;
        let y = acc.acceleration_y_e3() as f64 / 1000.0;
        let z = acc.acceleration_z_e3() as f64 / 1000.0;
        details.push(format!("accel=({:.3},{:.3},{:.3})", x, y, z));
        json_fields.push(format!("\"accelerometer\":{{\"x\":{:.3},\"y\":{:.3},\"z\":{:.3}}}", x, y, z));
        events.push(ArdaSensorEvent::Accelerometer { x, y, z });
    }
    if let Some(gyro) = msg.gyroscope_data.last() {
        let x = gyro.rotation_speed_x_e3() as f64 / 1000.0;
        let y = gyro.rotation_speed_y_e3() as f64 / 1000.0;
        let z = gyro.rotation_speed_z_e3() as f64 / 1000.0;
        details.push(format!("gyro=({:.3},{:.3},{:.3})", x, y, z));
        json_fields.push(format!("\"gyroscope\":{{\"x\":{:.3},\"y\":{:.3},\"z\":{:.3}}}", x, y, z));
        events.push(ArdaSensorEvent::Gyroscope { x, y, z });
    }
    if let Some(sat) = msg.gps_satellite_data.last() {
        let in_view = sat.has_number_in_view().then(|| sat.number_in_view());
        let satellites = sat.satellites.iter().map(|entry| GpsSatelliteInfo {
            prn: entry.prn(),
            snr: entry.snr_e3() as f64 / 1000.0,
            used_in_fix: entry.used_in_fix(),
            azimuth: entry.has_azimuth_e3().then(|| entry.azimuth_e3() as f64 / 1000.0),
            elevation: entry.has_elevation_e3().then(|| entry.elevation_e3() as f64 / 1000.0),
        }).collect::<Vec<_>>();
        details.push(format!("gps_satellites use={} view={:?} satellites={:?}", sat.number_in_use(), in_view, satellites));
        json_fields.push(format!(
            "\"gps_satellite\":{{\"in_use\":{},\"in_view\":{},\"listed\":{}}}",
            sat.number_in_use(),
            in_view.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
            satellites.len(),
        ));
        events.push(ArdaSensorEvent::GpsSatellite {
            in_use: sat.number_in_use(),
            in_view,
            satellites,
        });
    }
    if let Some(toll) = msg.toll_card_data.last() {
        let present = toll.is_card_present();
        details.push(format!("toll_card_present={}", present));
        json_fields.push(format!("\"toll_card_present\":{}", present));
        events.push(ArdaSensorEvent::TollCard { present });
    }
    if !msg.energy_model_control.is_empty() {
        details.push("energy_model=present(opaque)".to_string());
        json_fields.push("\"energy_model\":{\"present\":true,\"opaque\":true}".to_string());
        events.push(ArdaSensorEvent::EnergyModel { present: true });
    }

    Some(SensorBatchSummary {
        summary: format!(
            "SENSOR_MESSAGE_BATCH counts=[{}]{}{}",
            populated.join(","),
            if details.is_empty() { "" } else { " " },
            details.join(" ")
        ),
        json_line: format!("{{{}}}", json_fields.join(",")),
        location,
        events,
    })
}


fn smoke_encode_varint(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn smoke_field_varint(field: u32, value: u64, out: &mut Vec<u8>) {
    smoke_encode_varint((field as u64) << 3, out);
    smoke_encode_varint(value, out);
}

fn smoke_field_message(field: u32, body: &[u8], out: &mut Vec<u8>) {
    smoke_encode_varint(((field as u64) << 3) | 2, out);
    smoke_encode_varint(body.len() as u64, out);
    out.extend_from_slice(body);
}

pub fn sensor_proto_smoke() -> Result<(), String> {
    let mut batch = Vec::new();

    let mut location = Vec::new();
    smoke_field_varint(1, 123_456_789, &mut location);
    smoke_field_varint(2, 399_000_000, &mut location);
    smoke_field_varint(3, 328_000_000, &mut location);
    smoke_field_message(1, &location, &mut batch);

    let mut compass = Vec::new();
    smoke_field_varint(1, 123_000_000, &mut compass);
    smoke_field_message(2, &compass, &mut batch);
    let mut speed = Vec::new();
    smoke_field_varint(1, 13_800, &mut speed);
    smoke_field_message(3, &speed, &mut batch);
    let mut rpm = Vec::new();
    smoke_field_varint(1, 1_500_000, &mut rpm);
    smoke_field_message(4, &rpm, &mut batch);
    let mut odometer = Vec::new();
    smoke_field_varint(1, 123_456, &mut odometer);
    smoke_field_message(5, &odometer, &mut batch);
    smoke_field_message(6, &[], &mut batch);
    let mut parking = Vec::new();
    smoke_field_varint(1, 1, &mut parking);
    smoke_field_message(7, &parking, &mut batch);
    let mut gear = Vec::new();
    smoke_field_varint(1, 100, &mut gear);
    smoke_field_message(8, &gear, &mut batch);
    smoke_field_message(9, &[], &mut batch);
    smoke_field_message(10, &[], &mut batch);
    smoke_field_message(11, &[], &mut batch);
    smoke_field_message(12, &[], &mut batch);
    let mut driving = Vec::new();
    smoke_field_varint(1, 1, &mut driving);
    smoke_field_message(13, &driving, &mut batch);
    for field in 14..=20 {
        smoke_field_message(field, &[], &mut batch);
    }

    let mut satellite = Vec::new();
    smoke_field_varint(1, 7, &mut satellite);
    smoke_field_varint(2, 42_500, &mut satellite);
    smoke_field_varint(3, 1, &mut satellite);
    smoke_field_varint(4, 120_000, &mut satellite);
    let mut gps = Vec::new();
    smoke_field_varint(1, 1, &mut gps);
    smoke_field_message(3, &satellite, &mut gps);
    smoke_field_message(21, &gps, &mut batch);
    let mut toll = Vec::new();
    smoke_field_varint(1, 1, &mut toll);
    smoke_field_message(22, &toll, &mut batch);
    smoke_field_message(23, &[], &mut batch);

    let mut payload = Vec::new();
    payload.extend_from_slice(&(SensorMessageId::SENSOR_MESSAGE_BATCH.value() as u16).to_be_bytes());
    payload.extend_from_slice(&batch);
    let parsed = parse_sensor_batch_payload(&payload)
        .ok_or_else(|| "sensor proto smoke could not parse SensorBatch".to_string())?;
    let kinds = parsed.events.iter().map(ArdaSensorEvent::kind_id).collect::<Vec<_>>();
    let expected = (1..=23).collect::<Vec<_>>();
    if kinds != expected {
        return Err(format!("sensor proto smoke kind mismatch expected={expected:?} actual={kinds:?}"));
    }
    println!("SENSOR_PROTO_ALL_23_EVENT_KINDS=YES");

    let location = parsed.events.iter().find_map(|event| match event {
        ArdaSensorEvent::Location {
            timestamp_raw, latitude_deg, longitude_deg, accuracy_m,
            altitude_m, speed_mps, bearing_deg,
        } => Some((*timestamp_raw, *latitude_deg, *longitude_deg, *accuracy_m,
                   *altitude_m, *speed_mps, *bearing_deg)),
        _ => None,
    }).ok_or_else(|| "sensor proto smoke missing location event".to_string())?;
    if location.0 != Some(123_456_789)
        || (location.1 - 39.9).abs() > 0.000_000_1
        || (location.2 - 32.8).abs() > 0.000_000_1
        || location.3.is_some() || location.4.is_some()
        || location.5.is_some() || location.6.is_some()
    {
        return Err(format!("sensor proto smoke location presence mismatch: {location:?}"));
    }
    println!("SENSOR_PROTO_LOCATION_OPTIONAL_PRESENCE=YES");

    let gps = parsed.events.iter().find_map(|event| match event {
        ArdaSensorEvent::GpsSatellite { in_use, in_view, satellites } => Some((*in_use, *in_view, satellites)),
        _ => None,
    }).ok_or_else(|| "sensor proto smoke missing gps satellite event".to_string())?;
    let first = gps.2.first().ok_or_else(|| "sensor proto smoke missing satellite detail".to_string())?;
    if gps.0 != 1 || gps.1.is_some() || gps.2.len() != 1 || first.prn != 7
        || (first.snr - 42.5).abs() > 0.000_001 || !first.used_in_fix
        || first.azimuth != Some(120.0) || first.elevation.is_some()
    {
        return Err(format!("sensor proto smoke gps fidelity mismatch: in_use={} in_view={:?} satellites={:?}", gps.0, gps.1, gps.2));
    }
    println!("SENSOR_PROTO_GPS_SATELLITE_FIDELITY=YES");
    println!("SENSOR_PROTO_SMOKE_OK=YES");
    Ok(())
}


/// Build a MESSAGE_CHANNEL_CLOSE_NOTIFICATION control payload.
/// Returned bytes include the 2-byte AAP control message id followed by the
/// empty ChannelCloseNotification protobuf body.
pub fn build_channel_close_notification_control_payload() -> Result<Vec<u8>, protobuf::Error> {
    let msg = ChannelCloseNotification::new();
    let mut payload = Vec::new();
    payload.extend_from_slice(&(ControlMessageType::MESSAGE_CHANNEL_CLOSE_NOTIFICATION.value() as u16).to_be_bytes());
    payload.extend_from_slice(&msg.write_to_bytes()?);
    Ok(payload)
}

/// Build a phone-like ServiceDiscoveryRequest control payload.
/// Returned bytes include the 2-byte AAP control message id followed by the
/// protobuf body.
pub fn build_service_discovery_request_control_payload(
    label_text: &str,
    device_name: &str,
) -> Result<Vec<u8>, protobuf::Error> {
    let mut req = ServiceDiscoveryRequest::new();
    req.set_label_text(label_text.to_string());
    req.set_device_name(device_name.to_string());

    // Keep the first generated request minimal for compatibility.
    // The rust-protobuf generator used in this tree does not expose a
    // set_phone_info(...) helper for this field shape on every build; once the
    // service-discovery round-trip is confirmed we can populate phone_info via
    // the generated field API that this exact proto version exposes.

    let mut payload = Vec::new();
    payload.extend_from_slice(&(ControlMessageType::MESSAGE_SERVICE_DISCOVERY_REQUEST.value() as u16).to_be_bytes());
    payload.extend_from_slice(&req.write_to_bytes()?);
    Ok(payload)
}
