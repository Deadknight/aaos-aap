use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_TCP_ADDR: &str = "127.0.0.1:5283";
const DEFAULT_UNIX_SOCKET: &str = "/dev/socket/arda_bt_bridge";
const INIT_SOCKET_ENV: &str = "ANDROID_SOCKET_arda_bt_bridge";
const BRIDGE_MAGIC: &[u8; 8] = b"ARDBR001";
const BRIDGE_OK: &[u8; 4] = b"OKAY";
const BRIDGE_DENY: &[u8; 4] = b"DENY";
const MAX_BRIDGE_TOKEN_BYTES: usize = 512;
const MAX_FRAME_BYTES: usize = 64 * 1024;
const PROTOCOL_VERSION: u8 = 1;
const DEFAULT_PHONE_PROXY_PORT: u16 = 5285;
const DEFAULT_PROXY_TEST_URL: &str = "http://connectivitycheck.gstatic.com/generate_204";
const DEFAULT_PROXY_CONNECT_AUTHORITY: &str = "connectivitycheck.gstatic.com:443";

const OP_HELLO: u8 = 1;
const OP_HELLO_ACK: u8 = 2;
const OP_PING: u8 = 3;
const OP_PONG: u8 = 4;
const OP_SESSION_STOP: u8 = 5;
const OP_WIFI_REQUEST: u8 = 10;
const OP_WIFI_OFFER: u8 = 11;
const OP_PROXY_READY: u8 = 12;
const OP_PROXY_RESULT: u8 = 13;
const OP_PROXY_STATUS: u8 = 14;
const OP_PROXY_STOP: u8 = 15;
const OP_PLATFORM_WIFI_INFO: u8 = 20;
const OP_PLATFORM_WIFI_ACK: u8 = 21;
const OP_ERROR: u8 = 255;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BridgeTransport {
    Tcp,
    Unix,
}

#[derive(Debug)]
struct Config {
    transport: BridgeTransport,
    tcp_addr: SocketAddr,
    unix_socket: PathBuf,
    bridge_token: String,
    allowed_uids: Vec<u32>,
    once: bool,
    keep_bootstrap_rfcomm_open: bool,
    keep_bluetooth_profiles_connected: bool,
}

#[derive(Debug)]
struct Frame {
    version: u8,
    operation: u8,
    request_id: u64,
    payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlatformWifiInfo {
    ssid: String,
    psk: Option<String>,
    security: String,
    bssid: Option<String>,
    ap_ip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyEndpoint {
    phone_ip: IpAddr,
    port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum JsonScalar {
    String(String),
    Bool(bool),
    Number(u64),
    Null,
}

pub fn run(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_args(args)?;

    println!("ARDA_RN3B_SENDERD_ROLE=PROTOCOL_STATE_MACHINE");
    println!("ARDA_RN3B_SENDERD_TRANSPORT={:?}", config.transport);
    println!("ARDA_RN3B_SENDERD_ONCE={}", config.once);
    println!(
        "ARDA_RN3B_KEEP_BOOTSTRAP_RFCOMM_OPEN={}",
        config.keep_bootstrap_rfcomm_open
    );
    println!(
        "ARDA_RN3B_KEEP_BLUETOOTH_PROFILES_CONNECTED={}",
        config.keep_bluetooth_profiles_connected
    );
    println!("ARDA_RN3B_PROTOCOL_REV=AAOS_LOHS_CELLULAR_PROXY_V1");
    println!("ARDA_RN3B_WIFI_ROLE=AAOS_LOCAL_ONLY_AP");
    println!("ARDA_RN3B_PROXY_ROLE=PHONE_CELLULAR_HTTP_CONNECT_PROXY");
    println!("ARDA_RN3B_DATA_PATH=AAOS_WIFI_TO_PHONE_CELLULAR");

    match config.transport {
        BridgeTransport::Tcp => run_tcp(&config),
        BridgeTransport::Unix => run_unix(&config),
    }
}

fn parse_args(args: Vec<String>) -> Result<Config, Box<dyn std::error::Error>> {
    let mut transport = BridgeTransport::Tcp;
    let mut tcp_addr: SocketAddr = DEFAULT_TCP_ADDR.parse()?;
    let mut unix_socket = PathBuf::from(DEFAULT_UNIX_SOCKET);
    let mut bridge_token: Option<String> = None;
    let mut bridge_token_file: Option<PathBuf> = None;
    let mut allowed_uids = Vec::new();
    let mut once = false;
    let mut keep_bootstrap_rfcomm_open = true;
    let mut keep_bluetooth_profiles_connected = true;

    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--transport" => {
                index += 1;
                transport = match args.get(index).map(String::as_str) {
                    Some("tcp") => BridgeTransport::Tcp,
                    Some("unix") => BridgeTransport::Unix,
                    Some(other) => return Err(format!("unsupported --transport {other}").into()),
                    None => return Err("--transport requires tcp|unix".into()),
                };
            }
            "--addr" | "--bridge-addr" => {
                index += 1;
                tcp_addr = args.get(index).ok_or("--addr requires HOST:PORT")?.parse()?;
            }
            "--socket" | "--bridge-socket" => {
                index += 1;
                unix_socket = PathBuf::from(args.get(index).ok_or("--socket requires PATH")?);
            }
            "--bridge-token" => {
                index += 1;
                bridge_token = Some(args.get(index).ok_or("--bridge-token requires VALUE")?.clone());
            }
            "--bridge-token-file" => {
                index += 1;
                bridge_token_file = Some(PathBuf::from(
                    args.get(index).ok_or("--bridge-token-file requires PATH")?,
                ));
            }
            "--allow-uid" => {
                index += 1;
                allowed_uids.push(args.get(index).ok_or("--allow-uid requires UID")?.parse()?);
            }
            "--once" => once = true,
            "--keep-bootstrap-rfcomm-open" => keep_bootstrap_rfcomm_open = true,
            "--no-keep-bootstrap-rfcomm-open" => keep_bootstrap_rfcomm_open = false,
            "--keep-bluetooth-profiles-connected" => keep_bluetooth_profiles_connected = true,
            "--no-keep-bluetooth-profiles-connected" => {
                keep_bluetooth_profiles_connected = false
            }
            // Compatibility with the abandoned phone-hotspot prototype. The new design has no
            // senderd Wi-Fi listener: AAOS owns a local-only AP and the phone owns the proxy.
            "--enable-wifi-handoff" | "--no-wifi-handoff" => {}
            "--wifi-handoff-addr" => {
                index += 1;
                let _ = args.get(index).ok_or("--wifi-handoff-addr requires HOST:PORT")?;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("unknown bt-bootstrapd argument: {other}").into()),
        }
        index += 1;
    }

    if bridge_token.is_some() && bridge_token_file.is_some() {
        return Err("use only one of --bridge-token or --bridge-token-file".into());
    }
    if let Some(path) = bridge_token_file {
        bridge_token = Some(fs::read_to_string(&path).map_err(|err| {
            format!("cannot read bridge token file {}: {err}", path.display())
        })?);
    }

    let bridge_token = bridge_token.unwrap_or_default().trim().to_string();
    if bridge_token.len() > MAX_BRIDGE_TOKEN_BYTES {
        return Err(format!(
            "bridge token is too long: {} > {} bytes",
            bridge_token.len(),
            MAX_BRIDGE_TOKEN_BYTES
        )
        .into());
    }

    match transport {
        BridgeTransport::Tcp => {
            if !tcp_addr.ip().is_loopback() {
                return Err(format!("RN-3B TCP bridge must bind loopback only, got {tcp_addr}").into());
            }
            if bridge_token.is_empty() {
                return Err("RN-3B TCP bridge requires a bridge token".into());
            }
        }
        BridgeTransport::Unix => {
            if bridge_token.is_empty() && allowed_uids.is_empty() {
                return Err("Unix bridge requires --allow-uid and/or bridge token".into());
            }
        }
    }

    Ok(Config {
        transport,
        tcp_addr,
        unix_socket,
        bridge_token,
        allowed_uids,
        once,
        keep_bootstrap_rfcomm_open,
        keep_bluetooth_profiles_connected,
    })
}

fn print_help() {
    eprintln!(
        r#"arda-senderd bt-bootstrapd

  Debug bridge:
    bt-bootstrapd --transport tcp --addr 127.0.0.1:5283 --bridge-token VALUE

  Production bridge:
    bt-bootstrapd --transport unix --socket /dev/socket/arda_bt_bridge \
      --allow-uid UID [--bridge-token-file PATH]

ArdaPlatform owns Bluetooth RFCOMM and the AAOS local-only hotspot. It registers
hotspot credentials with senderd over the authenticated loopback bridge before
pumping phone frames. The phone companion joins that local-only Wi-Fi network,
opens an HTTP/CONNECT proxy bound to the Wi-Fi address, and binds every outbound
socket to Android's cellular Network. senderd verifies the proxy and applies it
as the AAOS global HTTP proxy while RFCOMM remains connected as control/fallback."#
    );
}

fn run_tcp(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(config.tcp_addr)?;
    println!("ARDA_RN3B_SENDERD_BRIDGE_LISTEN={}", listener.local_addr()?);
    println!("ARDA_RN3B_SENDERD_BRIDGE_LISTEN_GATE=PASS");

    for incoming in listener.incoming() {
        match incoming {
            Ok(mut stream) => {
                let peer = stream.peer_addr()?;
                if !peer.ip().is_loopback() {
                    eprintln!("ARDA_RN3B_SENDERD_BRIDGE_REJECT=NON_LOOPBACK peer={peer}");
                    continue;
                }
                stream.set_nodelay(true)?;
                println!("ARDA_RN3B_SENDERD_BRIDGE_ACCEPT={peer}");
                if let Err(err) = serve_bridge(
                    &mut stream,
                    &config.bridge_token,
                    config.keep_bootstrap_rfcomm_open,
                    config.keep_bluetooth_profiles_connected,
                ) {
                    eprintln!("ARDA_RN3B_SENDERD_SESSION_ERROR={err}");
                }
                if config.once {
                    break;
                }
            }
            Err(err) => eprintln!("ARDA_RN3B_SENDERD_ACCEPT_ERROR={err}"),
        }
    }
    Ok(())
}

fn run_unix(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let (listener, origin) = open_unix_listener(&config.unix_socket)?;
    println!(
        "ARDA_RN3B_SENDERD_BRIDGE_LISTEN={} origin={origin}",
        config.unix_socket.display()
    );
    println!("ARDA_RN3B_SENDERD_BRIDGE_LISTEN_GATE=PASS");

    for incoming in listener.incoming() {
        match incoming {
            Ok(mut stream) => {
                let (uid, gid, pid) = peer_credentials(&stream)?;
                println!("ARDA_RN3B_SENDERD_BRIDGE_PEER uid={uid} gid={gid} pid={pid}");
                if !config.allowed_uids.is_empty() && !config.allowed_uids.contains(&uid) {
                    eprintln!("ARDA_RN3B_SENDERD_BRIDGE_REJECT=UID_NOT_ALLOWED uid={uid}");
                    let _ = stream.write_all(BRIDGE_DENY);
                    continue;
                }
                if let Err(err) = serve_bridge(
                    &mut stream,
                    &config.bridge_token,
                    config.keep_bootstrap_rfcomm_open,
                    config.keep_bluetooth_profiles_connected,
                ) {
                    eprintln!("ARDA_RN3B_SENDERD_SESSION_ERROR={err}");
                }
                if config.once {
                    break;
                }
            }
            Err(err) => eprintln!("ARDA_RN3B_SENDERD_ACCEPT_ERROR={err}"),
        }
    }
    Ok(())
}

fn open_unix_listener(path: &Path) -> io::Result<(UnixListener, &'static str)> {
    if let Ok(value) = env::var(INIT_SOCKET_ENV) {
        let fd: RawFd = value.parse().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid {INIT_SOCKET_ENV}={value}"),
            )
        })?;
        // SAFETY: Android init transfers ownership of this listening descriptor.
        let listener = unsafe { UnixListener::from_raw_fd(fd) };
        return Ok((listener, "init"));
    }

    if path.exists() {
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o660))?;
    Ok((listener, "filesystem"))
}

fn serve_bridge<S: Read + Write>(
    stream: &mut S,
    expected_token: &str,
    keep_bootstrap_rfcomm_open: bool,
    keep_bluetooth_profiles_connected: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    authenticate_bridge(stream, expected_token)?;
    println!("ARDA_RN3B_SENDERD_BRIDGE_AUTH=PASS");

    let mut hello_seen = false;
    let mut platform_wifi: Option<PlatformWifiInfo> = None;
    let mut proxy_endpoint: Option<ProxyEndpoint> = None;

    loop {
        let Some(frame) = read_frame(stream)? else {
            if proxy_endpoint.is_some() {
                clear_android_proxy();
                println!("ARDA_RN3B_REVERSE_PROXY_CLEARED_ON_EOF=PASS");
            }
            println!("ARDA_RN3B_SENDERD_BRIDGE_EOF=YES");
            return Ok(());
        };

        println!(
            "ARDA_RN3B_SENDERD_RX={} request_id={} payload_bytes={}",
            operation_name(frame.operation),
            frame.request_id,
            frame.payload.len()
        );

        if frame.version != PROTOCOL_VERSION {
            write_error(stream, frame.request_id, "unsupported_version")?;
            continue;
        }

        match frame.operation {
            OP_PLATFORM_WIFI_INFO => {
                match parse_platform_wifi_info(&frame.payload) {
                    Ok(Some(info)) => {
                        println!(
                            "ARDA_RN3B_SENDERD_PLATFORM_WIFI_INFO=PASS ssid={} security={} bssid={}",
                            info.ssid,
                            info.security,
                            info.bssid.as_deref().unwrap_or("unreported")
                        );
                        platform_wifi = Some(info);
                        write_frame(
                            stream,
                            OP_PLATFORM_WIFI_ACK,
                            frame.request_id,
                            br#"{"status":"ok","wifi_available":true}"#,
                        )?;
                    }
                    Ok(None) => {
                        platform_wifi = None;
                        println!("ARDA_RN3B_SENDERD_PLATFORM_WIFI_INFO=UNAVAILABLE");
                        write_frame(
                            stream,
                            OP_PLATFORM_WIFI_ACK,
                            frame.request_id,
                            br#"{"status":"ok","wifi_available":false}"#,
                        )?;
                    }
                    Err(err) => write_error(
                        stream,
                        frame.request_id,
                        &format!("invalid_platform_wifi_info:{err}"),
                    )?,
                }
            }
            OP_HELLO => {
                let payload = String::from_utf8_lossy(&frame.payload);
                if !payload.contains("\"role\":\"phone_companion\"") {
                    write_error(stream, frame.request_id, "invalid_hello_role")?;
                    continue;
                }
                hello_seen = true;
                println!("ARDA_RN3B_SENDERD_HELLO_RX=PASS");
                let response = format!(
                    "{{\"role\":\"aaos_senderd\",\"service\":\"arda_bt_bootstrap\",\"keep_rfcomm_open\":{},\"keep_bluetooth_profiles_connected\":{},\"wifi_bootstrap_supported\":{},\"phone_proxy_port\":{},\"server_unix_ms\":{}}}",
                    keep_bootstrap_rfcomm_open,
                    keep_bluetooth_profiles_connected,
                    platform_wifi.is_some(),
                    DEFAULT_PHONE_PROXY_PORT,
                    unix_time_ms()
                );
                write_frame(stream, OP_HELLO_ACK, frame.request_id, response.as_bytes())?;
                println!("ARDA_RN3B_SENDERD_HELLO_ACK_TX=PASS");
            }
            OP_PING => {
                if !hello_seen {
                    write_error(stream, frame.request_id, "hello_required")?;
                    continue;
                }
                let response = format!(
                    "{{\"status\":\"ok\",\"proxy_active\":{},\"server_unix_ms\":{}}}",
                    proxy_endpoint.is_some(),
                    unix_time_ms()
                );
                write_frame(stream, OP_PONG, frame.request_id, response.as_bytes())?;
                println!("ARDA_RN3B_SENDERD_PONG_TX=PASS");
                println!("ARDA_RN3B_SENDERD_HELLO_PING_PONG_GATE=PASS");
                if !keep_bootstrap_rfcomm_open {
                    return Ok(());
                }
            }
            OP_WIFI_REQUEST => {
                if !hello_seen {
                    write_error(stream, frame.request_id, "hello_required")?;
                    continue;
                }
                println!("ARDA_RN3B_SENDERD_WIFI_REQUEST_RX=PASS");
                match &platform_wifi {
                    Some(info) => {
                        let psk_json = info
                            .psk
                            .as_deref()
                            .map(|value| format!("\"{}\"", json_escape(value)))
                            .unwrap_or_else(|| "null".to_string());
                        let bssid_json = info
                            .bssid
                            .as_deref()
                            .map(|value| format!("\"{}\"", json_escape(value)))
                            .unwrap_or_else(|| "null".to_string());
                        let ap_ip_json = info
                            .ap_ip
                            .as_deref()
                            .map(|value| format!("\"{}\"", json_escape(value)))
                            .unwrap_or_else(|| "null".to_string());
                        let response = format!(
                            "{{\"status\":\"ready\",\"mode\":\"aaos_local_only_hotspot\",\"ssid\":\"{}\",\"psk\":{},\"security\":\"{}\",\"bssid\":{},\"ap_ip\":{},\"proxy_port\":{},\"keep_rfcomm_open\":true}}",
                            json_escape(&info.ssid),
                            psk_json,
                            json_escape(&info.security),
                            bssid_json,
                            ap_ip_json,
                            DEFAULT_PHONE_PROXY_PORT
                        );
                        write_frame(stream, OP_WIFI_OFFER, frame.request_id, response.as_bytes())?;
                        println!("ARDA_RN3B_SENDERD_WIFI_OFFER_TX=PASS");
                    }
                    None => {
                        write_frame(
                            stream,
                            OP_WIFI_OFFER,
                            frame.request_id,
                            br#"{"status":"error","error":"aaos_local_only_hotspot_unavailable"}"#,
                        )?;
                        eprintln!("ARDA_RN3B_SENDERD_WIFI_OFFER_TX=FAIL_NO_PLATFORM_INFO");
                    }
                }
            }
            OP_PROXY_READY => {
                if !hello_seen {
                    write_error(stream, frame.request_id, "hello_required")?;
                    continue;
                }
                match parse_proxy_ready(&frame.payload) {
                    Ok((endpoint, test_url)) => {
                        println!(
                            "ARDA_RN3B_SENDERD_PHONE_PROXY_READY_RX={} port={}",
                            endpoint.phone_ip, endpoint.port
                        );
                        match probe_http_proxy(&endpoint, &test_url) {
                            Ok(status_line) => {
                                let connect_status = match probe_connect_proxy(
                                    &endpoint,
                                    DEFAULT_PROXY_CONNECT_AUTHORITY,
                                ) {
                                    Ok(status) => {
                                        println!("ARDA_RN3B_REVERSE_PROXY_CONNECT_GATE=PASS");
                                        status
                                    }
                                    Err(err) => {
                                        let response = format!(
                                            "{{\"status\":\"error\",\"error\":\"{}\",\"global_proxy_applied\":false}}",
                                            json_escape(&err.to_string())
                                        );
                                        write_frame(
                                            stream,
                                            OP_PROXY_RESULT,
                                            frame.request_id,
                                            response.as_bytes(),
                                        )?;
                                        eprintln!(
                                            "ARDA_RN3B_REVERSE_PROXY_GATE=FAIL error={err}"
                                        );
                                        continue;
                                    }
                                };
                                crate::net_proxy(vec![
                                    "--host".to_string(),
                                    endpoint.phone_ip.to_string(),
                                    "--port".to_string(),
                                    endpoint.port.to_string(),
                                ])?;
                                let expected_proxy =
                                    format!("{}:{}", endpoint.phone_ip, endpoint.port);
                                let applied_proxy = read_android_proxy();
                                if applied_proxy != expected_proxy {
                                    clear_android_proxy();
                                    let error = format!(
                                        "Android global proxy verification failed: expected {expected_proxy}, got {applied_proxy}"
                                    );
                                    let response = format!(
                                        "{{\"status\":\"error\",\"error\":\"{}\",\"global_proxy_applied\":false}}",
                                        json_escape(&error)
                                    );
                                    write_frame(
                                        stream,
                                        OP_PROXY_RESULT,
                                        frame.request_id,
                                        response.as_bytes(),
                                    )?;
                                    eprintln!("ARDA_RN3B_REVERSE_PROXY_GATE=FAIL error={error}");
                                } else {
                                    proxy_endpoint = Some(endpoint.clone());
                                    let response = format!(
                                        "{{\"status\":\"ready\",\"proxy\":\"{}:{}\",\"test_url\":\"{}\",\"test_status\":\"{}\",\"connect_test_status\":\"{}\",\"global_proxy_applied\":true}}",
                                        endpoint.phone_ip,
                                        endpoint.port,
                                        json_escape(&test_url),
                                        json_escape(&status_line),
                                        json_escape(&connect_status)
                                    );
                                    write_frame(
                                        stream,
                                        OP_PROXY_RESULT,
                                        frame.request_id,
                                        response.as_bytes(),
                                    )?;
                                    println!("ARDA_RN3B_REVERSE_PROXY_GATE=PASS");
                                }
                            }
                            Err(err) => {
                                let response = format!(
                                    "{{\"status\":\"error\",\"error\":\"{}\",\"global_proxy_applied\":false}}",
                                    json_escape(&err.to_string())
                                );
                                write_frame(
                                    stream,
                                    OP_PROXY_RESULT,
                                    frame.request_id,
                                    response.as_bytes(),
                                )?;
                                eprintln!("ARDA_RN3B_REVERSE_PROXY_GATE=FAIL error={err}");
                            }
                        }
                    }
                    Err(err) => write_error(
                        stream,
                        frame.request_id,
                        &format!("invalid_proxy_ready:{err}"),
                    )?,
                }
            }
            OP_PROXY_STATUS => {
                let current = read_android_proxy();
                let active = proxy_endpoint.is_some();
                let response = format!(
                    "{{\"status\":\"ok\",\"active\":{},\"android_http_proxy\":\"{}\"}}",
                    active,
                    json_escape(&current)
                );
                write_frame(stream, OP_PROXY_RESULT, frame.request_id, response.as_bytes())?;
            }
            OP_PROXY_STOP => {
                clear_android_proxy();
                proxy_endpoint = None;
                write_frame(
                    stream,
                    OP_PROXY_RESULT,
                    frame.request_id,
                    br#"{"status":"stopped","global_proxy_applied":false}"#,
                )?;
                println!("ARDA_RN3B_REVERSE_PROXY_STOP=PASS");
            }
            OP_SESSION_STOP => {
                if proxy_endpoint.is_some() {
                    clear_android_proxy();
                    proxy_endpoint = None;
                    println!("ARDA_RN3B_REVERSE_PROXY_STOP=PASS");
                }
                println!("ARDA_RN3B_SENDERD_SESSION_STOP_RX=YES");
                return Ok(());
            }
            _ => write_error(stream, frame.request_id, "unsupported_operation")?,
        }
    }
}

fn parse_platform_wifi_info(
    payload: &[u8],
) -> Result<Option<PlatformWifiInfo>, Box<dyn std::error::Error>> {
    let object = parse_json_object(std::str::from_utf8(payload)?)?;
    let available = optional_json_bool(&object, "available")?.unwrap_or(true);
    if !available {
        return Ok(None);
    }

    let ssid = required_json_string(&object, "ssid")?;
    if ssid.is_empty() || ssid.len() > 64 {
        return Err("ssid length must be 1..64 bytes".into());
    }
    let psk = optional_json_string(&object, "psk")?;
    let security = optional_json_string(&object, "security")?
        .unwrap_or_else(|| if psk.is_some() { "wpa2".into() } else { "open".into() });
    if !matches!(security.as_str(), "open" | "owe" | "wpa2" | "wpa3") {
        return Err(format!("unsupported Wi-Fi security '{security}'").into());
    }
    let bssid = optional_json_string(&object, "bssid")?;
    if let Some(value) = bssid.as_deref() {
        validate_bssid(value)?;
    }
    let ap_ip = optional_json_string(&object, "ap_ip")?;

    Ok(Some(PlatformWifiInfo {
        ssid,
        psk,
        security,
        bssid,
        ap_ip,
    }))
}

fn validate_bssid(value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let bytes = value.as_bytes();
    if bytes.len() != 17 {
        return Err("bssid must be 17 characters".into());
    }
    for (index, byte) in bytes.iter().enumerate() {
        if index % 3 == 2 {
            if *byte != b':' {
                return Err("bssid separators must be ':'".into());
            }
        } else if !byte.is_ascii_hexdigit() {
            return Err("bssid contains non-hex characters".into());
        }
    }
    Ok(())
}

fn parse_proxy_ready(
    payload: &[u8],
) -> Result<(ProxyEndpoint, String), Box<dyn std::error::Error>> {
    let object = parse_json_object(std::str::from_utf8(payload)?)?;
    let phone_ip: IpAddr = required_json_string(&object, "phone_ip")?.parse()?;
    if phone_ip.is_unspecified() || phone_ip.is_loopback() || phone_ip.is_multicast() {
        return Err("phone_ip is not a usable local address".into());
    }
    let port_value = optional_json_number(&object, "proxy_port")?
        .unwrap_or(DEFAULT_PHONE_PROXY_PORT as u64);
    let port = u16::try_from(port_value).map_err(|_| "proxy_port must fit u16")?;
    if port == 0 {
        return Err("proxy_port must be non-zero".into());
    }
    let test_url = optional_json_string(&object, "test_url")?
        .unwrap_or_else(|| DEFAULT_PROXY_TEST_URL.to_string());
    if !test_url.starts_with("http://") {
        return Err("proxy test_url must use http:// for the bootstrap gate".into());
    }
    Ok((ProxyEndpoint { phone_ip, port }, test_url))
}

fn probe_http_proxy(
    endpoint: &ProxyEndpoint,
    test_url: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let (host, _path) = parse_http_url(test_url)?;
    let addr = SocketAddr::new(endpoint.phone_ip, endpoint.port);
    let mut socket = TcpStream::connect_timeout(&addr, Duration::from_secs(8))?;
    socket.set_read_timeout(Some(Duration::from_secs(12)))?;
    socket.set_write_timeout(Some(Duration::from_secs(8)))?;
    socket.write_all(
        format!(
            "GET {test_url} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: ARDA-senderd-RN3B\r\n\r\n"
        )
        .as_bytes(),
    )?;
    socket.flush()?;

    let mut reader = BufReader::new(socket);
    let mut status = String::new();
    reader.read_line(&mut status)?;
    let status = status.trim().to_string();
    let code = status
        .split_whitespace()
        .nth(1)
        .ok_or("proxy returned an invalid HTTP status line")?
        .parse::<u16>()?;
    if !(200..400).contains(&code) {
        return Err(format!("proxy test returned HTTP {code}: {status}").into());
    }
    Ok(status)
}

fn probe_connect_proxy(
    endpoint: &ProxyEndpoint,
    authority: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let addr = SocketAddr::new(endpoint.phone_ip, endpoint.port);
    let mut socket = TcpStream::connect_timeout(&addr, Duration::from_secs(8))?;
    socket.set_read_timeout(Some(Duration::from_secs(12)))?;
    socket.set_write_timeout(Some(Duration::from_secs(8)))?;
    socket.write_all(
        format!(
            "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: close\r\nUser-Agent: ARDA-senderd-RN3B\r\n\r\n"
        )
        .as_bytes(),
    )?;
    socket.flush()?;

    let mut reader = BufReader::new(socket);
    let mut status = String::new();
    reader.read_line(&mut status)?;
    let status = status.trim().to_string();
    let code = status
        .split_whitespace()
        .nth(1)
        .ok_or("proxy returned an invalid CONNECT status line")?
        .parse::<u16>()?;
    if code != 200 {
        return Err(format!("proxy CONNECT test returned HTTP {code}: {status}").into());
    }
    Ok(status)
}

fn parse_http_url(url: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let remainder = url.strip_prefix("http://").ok_or("URL must start with http://")?;
    let (authority, path) = remainder
        .split_once('/')
        .map(|(authority, path)| (authority, format!("/{path}")))
        .unwrap_or((remainder, "/".to_string()));
    if authority.is_empty() {
        return Err("URL host is empty".into());
    }
    Ok((authority.to_string(), path))
}

fn read_android_proxy() -> String {
    Command::new("settings")
        .args(["get", "global", "http_proxy"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

fn clear_android_proxy() {
    let _ = Command::new("settings")
        .args(["put", "global", "http_proxy", ":0"])
        .status();
    let _ = Command::new("settings")
        .args(["delete", "global", "http_proxy"])
        .status();
}

fn authenticate_bridge<S: Read + Write>(stream: &mut S, expected_token: &str) -> io::Result<()> {
    let mut magic = [0u8; 8];
    stream.read_exact(&mut magic)?;
    if &magic != BRIDGE_MAGIC {
        let _ = stream.write_all(BRIDGE_DENY);
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "invalid bridge magic"));
    }

    let mut length = [0u8; 2];
    stream.read_exact(&mut length)?;
    let token_len = u16::from_le_bytes(length) as usize;
    if token_len > MAX_BRIDGE_TOKEN_BYTES {
        let _ = stream.write_all(BRIDGE_DENY);
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bridge token exceeds limit"));
    }

    let mut token = vec![0u8; token_len];
    stream.read_exact(&mut token)?;
    let accepted = constant_time_eq(&token, expected_token.as_bytes());
    stream.write_all(if accepted { BRIDGE_OK } else { BRIDGE_DENY })?;
    stream.flush()?;
    if !accepted {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "bridge token rejected"));
    }
    Ok(())
}

fn read_frame<S: Read>(stream: &mut S) -> io::Result<Option<Frame>> {
    let mut length = [0u8; 4];
    let mut read = 0usize;
    while read < length.len() {
        match stream.read(&mut length[read..])? {
            0 if read == 0 => return Ok(None),
            0 => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "partial frame length")),
            count => read += count,
        }
    }

    let body_len = u32::from_le_bytes(length) as usize;
    if !(10..=MAX_FRAME_BYTES).contains(&body_len) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid ARDA frame body length {body_len}"),
        ));
    }

    let mut body = vec![0u8; body_len];
    stream.read_exact(&mut body)?;
    Ok(Some(Frame {
        version: body[0],
        operation: body[1],
        request_id: u64::from_le_bytes(body[2..10].try_into().expect("fixed request id")),
        payload: body[10..].to_vec(),
    }))
}

fn write_frame<S: Write>(
    stream: &mut S,
    operation: u8,
    request_id: u64,
    payload: &[u8],
) -> io::Result<()> {
    let body_len = 1usize + 1 + 8 + payload.len();
    if body_len > MAX_FRAME_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "response frame exceeds limit"));
    }
    stream.write_all(&(body_len as u32).to_le_bytes())?;
    stream.write_all(&[PROTOCOL_VERSION, operation])?;
    stream.write_all(&request_id.to_le_bytes())?;
    stream.write_all(payload)?;
    stream.flush()
}

fn write_error<S: Write>(stream: &mut S, request_id: u64, error: &str) -> io::Result<()> {
    let payload = format!("{{\"error\":\"{}\"}}", json_escape(error));
    write_frame(stream, OP_ERROR, request_id, payload.as_bytes())
}

fn operation_name(operation: u8) -> &'static str {
    match operation {
        OP_HELLO => "HELLO",
        OP_HELLO_ACK => "HELLO_ACK",
        OP_PING => "PING",
        OP_PONG => "PONG",
        OP_SESSION_STOP => "SESSION_STOP",
        OP_WIFI_REQUEST => "WIFI_REQUEST",
        OP_WIFI_OFFER => "WIFI_OFFER",
        OP_PROXY_READY => "PROXY_READY",
        OP_PROXY_RESULT => "PROXY_RESULT",
        OP_PROXY_STATUS => "PROXY_STATUS",
        OP_PROXY_STOP => "PROXY_STOP",
        OP_PLATFORM_WIFI_INFO => "PLATFORM_WIFI_INFO",
        OP_PLATFORM_WIFI_ACK => "PLATFORM_WIFI_ACK",
        OP_ERROR => "ERROR",
        _ => "UNKNOWN",
    }
}

fn peer_credentials(stream: &UnixStream) -> io::Result<(u32, u32, u32)> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: cred and len are valid writable storage for getsockopt.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((cred.uid, cred.gid, cred.pid as u32))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let max_len = left.len().max(right.len());
    for index in 0..max_len {
        let l = left.get(index).copied().unwrap_or(0);
        let r = right.get(index).copied().unwrap_or(0);
        difference |= (l ^ r) as usize;
    }
    difference == 0
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn parse_json_object(input: &str) -> Result<HashMap<String, JsonScalar>, String> {
    let bytes = input.as_bytes();
    let mut index = 0usize;
    skip_json_ws(bytes, &mut index);
    expect_json_byte(bytes, &mut index, b'{')?;
    let mut result = HashMap::new();

    loop {
        skip_json_ws(bytes, &mut index);
        if bytes.get(index) == Some(&b'}') {
            index += 1;
            break;
        }
        let key = parse_json_string(bytes, &mut index)?;
        skip_json_ws(bytes, &mut index);
        expect_json_byte(bytes, &mut index, b':')?;
        skip_json_ws(bytes, &mut index);
        let value = parse_json_scalar(bytes, &mut index)?;
        if result.insert(key.clone(), value).is_some() {
            return Err(format!("duplicate JSON key '{key}'"));
        }
        skip_json_ws(bytes, &mut index);
        match bytes.get(index) {
            Some(b',') => index += 1,
            Some(b'}') => {
                index += 1;
                break;
            }
            _ => return Err("expected ',' or '}' in JSON object".to_string()),
        }
    }

    skip_json_ws(bytes, &mut index);
    if index != bytes.len() {
        return Err("trailing bytes after JSON object".to_string());
    }
    Ok(result)
}

fn parse_json_scalar(bytes: &[u8], index: &mut usize) -> Result<JsonScalar, String> {
    match bytes.get(*index).copied() {
        Some(b'\"') => Ok(JsonScalar::String(parse_json_string(bytes, index)?)),
        Some(b't') if json_keyword_at(bytes, *index, b"true") => {
            *index += 4;
            Ok(JsonScalar::Bool(true))
        }
        Some(b'f') if json_keyword_at(bytes, *index, b"false") => {
            *index += 5;
            Ok(JsonScalar::Bool(false))
        }
        Some(b'n') if json_keyword_at(bytes, *index, b"null") => {
            *index += 4;
            Ok(JsonScalar::Null)
        }
        Some(b'0'..=b'9') => {
            let start = *index;
            while matches!(bytes.get(*index), Some(b'0'..=b'9')) {
                *index += 1;
            }
            let number = std::str::from_utf8(&bytes[start..*index])
                .map_err(|_| "invalid JSON number")?
                .parse::<u64>()
                .map_err(|_| "invalid JSON number")?;
            Ok(JsonScalar::Number(number))
        }
        _ => Err("unsupported JSON value".to_string()),
    }
}

fn json_keyword_at(bytes: &[u8], index: usize, keyword: &[u8]) -> bool {
    bytes.get(index..).map(|rest| rest.starts_with(keyword)).unwrap_or(false)
}

fn parse_json_string(bytes: &[u8], index: &mut usize) -> Result<String, String> {
    expect_json_byte(bytes, index, b'\"')?;
    let mut out = String::new();
    let mut raw_start = *index;
    while *index < bytes.len() {
        match bytes[*index] {
            b'\"' => {
                out.push_str(
                    std::str::from_utf8(&bytes[raw_start..*index])
                        .map_err(|_| "invalid UTF-8 in JSON string")?,
                );
                *index += 1;
                return Ok(out);
            }
            b'\\' => {
                out.push_str(
                    std::str::from_utf8(&bytes[raw_start..*index])
                        .map_err(|_| "invalid UTF-8 in JSON string")?,
                );
                *index += 1;
                let escaped = *bytes.get(*index).ok_or("truncated JSON escape")?;
                *index += 1;
                match escaped {
                    b'\"' => out.push('\"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    _ => return Err("unsupported JSON escape".to_string()),
                }
                raw_start = *index;
            }
            0x00..=0x1f => return Err("control byte in JSON string".to_string()),
            _ => *index += 1,
        }
    }
    Err("unterminated JSON string".to_string())
}

fn expect_json_byte(bytes: &[u8], index: &mut usize, expected: u8) -> Result<(), String> {
    if bytes.get(*index).copied() != Some(expected) {
        return Err(format!("expected JSON byte '{}'", expected as char));
    }
    *index += 1;
    Ok(())
}

fn skip_json_ws(bytes: &[u8], index: &mut usize) {
    while matches!(bytes.get(*index), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        *index += 1;
    }
}

fn required_json_string(
    object: &HashMap<String, JsonScalar>,
    key: &str,
) -> Result<String, String> {
    optional_json_string(object, key)?.ok_or_else(|| format!("missing string field '{key}'"))
}

fn optional_json_string(
    object: &HashMap<String, JsonScalar>,
    key: &str,
) -> Result<Option<String>, String> {
    match object.get(key) {
        None | Some(JsonScalar::Null) => Ok(None),
        Some(JsonScalar::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("field '{key}' must be a string")),
    }
}

fn optional_json_bool(
    object: &HashMap<String, JsonScalar>,
    key: &str,
) -> Result<Option<bool>, String> {
    match object.get(key) {
        None | Some(JsonScalar::Null) => Ok(None),
        Some(JsonScalar::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("field '{key}' must be a boolean")),
    }
}

fn optional_json_number(
    object: &HashMap<String, JsonScalar>,
    key: &str,
) -> Result<Option<u64>, String> {
    match object.get(key) {
        None | Some(JsonScalar::Null) => Ok(None),
        Some(JsonScalar::Number(value)) => Ok(Some(*value)),
        Some(_) => Err(format!("field '{key}' must be a non-negative integer")),
    }
}

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push('?'),
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_round_trip() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, OP_PING, 42, br#"{"x":1}"#).unwrap();
        let mut cursor = Cursor::new(bytes);
        let frame = read_frame(&mut cursor).unwrap().unwrap();
        assert_eq!(frame.operation, OP_PING);
        assert_eq!(frame.request_id, 42);
    }

    #[test]
    fn parses_platform_wifi_info() {
        let info = parse_platform_wifi_info(
            br#"{"available":true,"ssid":"ARDA-RN3B","psk":"abcdefgh","security":"wpa2","bssid":"02:11:22:33:44:55"}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(info.ssid, "ARDA-RN3B");
        assert_eq!(info.psk.as_deref(), Some("abcdefgh"));
        assert_eq!(info.bssid.as_deref(), Some("02:11:22:33:44:55"));
    }

    #[test]
    fn parses_proxy_ready() {
        let (endpoint, url) = parse_proxy_ready(
            br#"{"phone_ip":"192.168.43.2","proxy_port":5285,"test_url":"http://example.com/"}"#,
        )
        .unwrap();
        assert_eq!(endpoint.port, 5285);
        assert_eq!(url, "http://example.com/");
    }

    #[test]
    fn bridge_auth_accepts_matching_token() {
        let token = "secret";
        let mut input = Vec::new();
        input.extend_from_slice(BRIDGE_MAGIC);
        input.extend_from_slice(&(token.len() as u16).to_le_bytes());
        input.extend_from_slice(token.as_bytes());
        let mut duplex = TestDuplex::new(input);
        authenticate_bridge(&mut duplex, token).unwrap();
        assert_eq!(duplex.output, BRIDGE_OK);
    }

    struct TestDuplex {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl TestDuplex {
        fn new(input: Vec<u8>) -> Self {
            Self { input: Cursor::new(input), output: Vec::new() }
        }
    }

    impl Read for TestDuplex {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.input.read(buf)
        }
    }

    impl Write for TestDuplex {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
