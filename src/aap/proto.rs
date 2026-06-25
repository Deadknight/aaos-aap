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
        Some(ControlMessageType::MESSAGE_SERVICE_DISCOVERY_RESPONSE) => {
            let summary = summarize_service_discovery_response_body(body).ok()?;
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
        Some(other) => Some(format!("{other:?} body_len={}", body.len())),
        None => Some(format!("UNKNOWN_MEDIA_MESSAGE id=0x{id:04x} body_len={}", body.len())),
    }
}

/// Build MEDIA_MESSAGE_SETUP for the opened video sink channel.
/// Returned bytes include the 2-byte media message id followed by the protobuf body.
/// For DHU's first video sink we currently request H.264 baseline, matching the
/// advertised first video config in ServiceDiscoveryResponse.
pub fn build_video_media_setup_payload() -> Result<(Vec<u8>, String), protobuf::Error> {
    let mut setup = Setup::new();
    setup.set_type(MediaCodecType::MEDIA_CODEC_VIDEO_H264_BP);

    let mut out = Vec::new();
    out.extend_from_slice(&(MediaMessageId::MEDIA_MESSAGE_SETUP.value() as u16).to_be_bytes());
    out.extend_from_slice(&setup.write_to_bytes()?);

    Ok((
        out,
        "MEDIA_MESSAGE_SETUP type=MEDIA_CODEC_VIDEO_H264_BP".to_string(),
    ))
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
}

#[derive(Debug, Clone)]
pub struct MediaSinkSelection {
    pub service_id: i32,
    pub available_type: String,
    pub display_id: u32,
    pub display_type: String,
    pub video_configs: usize,
    pub first_video_config: Option<String>,
    pub selected_video_config: Option<VideoConfigSelection>,
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
    pub codec: String,
    pub resolution: String,
    pub summary: String,
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
    format!(
        "index={} resolution={:?} {}x{} fps={:?}/{} margins={}x{} density={} real_density={} video_codec={:?}",
        index,
        vc.codec_resolution(),
        width,
        height,
        vc.frame_rate(),
        video_fps_value(vc.frame_rate()),
        vc.width_margin(),
        vc.height_margin(),
        vc.density(),
        vc.real_density(),
        vc.video_codec_type()
    )
}

fn select_best_video_config(configs: &[VideoConfiguration]) -> Option<VideoConfigSelection> {
    configs
        .iter()
        .enumerate()
        .filter_map(|(index, vc)| {
            let (width, height) = video_resolution_dimensions(vc.codec_resolution())?;
            let fps = video_fps_value(vc.frame_rate());
            let codec = format!("{:?}", vc.video_codec_type());
            let resolution = format!("{:?}", vc.codec_resolution());
            let summary = video_config_summary(index, vc);
            let area = (width as i64) * (height as i64);
            let score = area * 100 + fps as i64;
            Some((
                score,
                VideoConfigSelection {
                    config_index: index,
                    width,
                    height,
                    fps,
                    density: vc.density(),
                    real_density: vc.real_density(),
                    width_margin: vc.width_margin(),
                    height_margin: vc.height_margin(),
                    codec,
                    resolution,
                    summary,
                },
            ))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, selection)| selection)
}

/// Parse a MESSAGE_SERVICE_DISCOVERY_RESPONSE control payload, select the first
/// HU video MediaSinkService, and return human-readable details for logs.
/// `payload` includes the 2-byte control message id.
pub fn parse_service_discovery_response_control_payload(
    payload: &[u8],
) -> Result<Option<ServiceDiscoverySummary>, protobuf::Error> {
    if payload.len() < 2 {
        return Ok(None);
    }
    let id = u16::from_be_bytes([payload[0], payload[1]]);
    if id != ControlMessageType::MESSAGE_SERVICE_DISCOVERY_RESPONSE.value() as u16 {
        return Ok(None);
    }
    summarize_service_discovery_response_body(&payload[2..]).map(Some)
}

fn summarize_service_discovery_response_body(
    body: &[u8],
) -> Result<ServiceDiscoverySummary, protobuf::Error> {
    let msg = ServiceDiscoveryResponse::parse_from_bytes(body)?;
    let mut details = Vec::new();
    let mut video_media_sink = None;

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
                let selected_video_config = select_best_video_config(&sink.video_configs);
                video_media_sink = Some(MediaSinkSelection {
                    service_id: svc.id(),
                    available_type,
                    display_id: sink.display_id(),
                    display_type,
                    video_configs: video_count,
                    first_video_config,
                    selected_video_config,
                });
            }
        }
        if let Some(input) = svc.input_source_service.as_ref() {
            kinds.push(format!(
                "input_source touchscreens={} touchpads={} keycodes={} display_id={}",
                input.touchscreen.len(),
                input.touchpad.len(),
                input.keycodes_supported.len(),
                input.display_id()
            ));
        }
        if let Some(sensor) = svc.sensor_source_service.as_ref() {
            kinds.push(format!("sensor_source sensors={}", sensor.sensors.len()));
        }
        if let Some(source) = svc.media_source_service.as_ref() {
            kinds.push(format!("media_source type={:?}", source.available_type()));
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
