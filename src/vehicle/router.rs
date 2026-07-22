use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::CString;
use std::fs;
use std::io;
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use protobuf::Enum;

use super::model::{
    property_name, validate_vehicle_command, validate_vehicle_signal, Capability,
    VehicleCommand, VehicleSignal, VhalSetRequest,
};
use super::policy::{ready_line, VehiclePolicyEngine};
use super::wire::{
    self, envelope, pb, peer_credentials, read_envelope, vehicle_command_to_pb,
    vehicle_signal_from_pb, vhal_event_to_pb, vhal_set_from_pb, write_envelope,
    DEFAULT_ROUTER_SOCKET, ROUTER_PROTOCOL_VERSION,
};

const UID_ROOT: u32 = 0;
const UID_SYSTEM: u32 = 1000;
const ANDROID_PER_USER_RANGE: u32 = 100_000;
const UID_VEHICLE_NETWORK: u32 = 1054;
const ROUTER_SOCKET_GID_SYSTEM: u32 = 1000;
const INIT_ROUTER_SOCKET_ENV: &str = "ANDROID_SOCKET_arda_vehicle_router";
const SIGNAL_SOURCE_STALE_AFTER: Duration = Duration::from_secs(5);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
const VEHICLE_INFO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    SignalSource,
    Vhal,
    Manager,
}

impl Role {
    fn from_pb(value: pb::ClientRole) -> Option<Self> {
        match value {
            pb::ClientRole::ROLE_SIGNAL_SOURCE => Some(Self::SignalSource),
            pb::ClientRole::ROLE_VHAL => Some(Self::Vhal),
            pb::ClientRole::ROLE_MANAGER => Some(Self::Manager),
            pb::ClientRole::ROLE_UNSPECIFIED => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::SignalSource => "SIGNAL_SOURCE",
            Self::Vhal => "VHAL",
            Self::Manager => "MANAGER",
        }
    }
}

#[derive(Clone)]
struct ClientHandle {
    id: u64,
    role: Role,
    uid: u32,
    name: String,
    tx: mpsc::Sender<pb::Envelope>,
}

#[derive(Clone)]
struct SourceState {
    source_id: u32,
    client_id: u64,
    name: String,
    priority: i32,
    publish_capabilities: HashSet<Capability>,
    command_capabilities: HashSet<Capability>,
    tx: mpsc::Sender<pb::Envelope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SelectionKey {
    capability: Capability,
    area_id: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceSelectionDecision {
    Accepted { previous_source_id: Option<u32> },
    Shadowed,
}

struct SignalSelection {
    source_id: u32,
    priority: i32,
    last_seen: Instant,
}

struct PendingCommand {
    command_id: u64,
    source_id: u32,
    request_id: u64,
    property_id: i32,
    area_id: i32,
    deadline: Instant,
}

struct PendingVehicleInfo {
    request_id: u64,
    manager_client_id: u64,
    deadline: Instant,
}

struct RouterState {
    clients: HashMap<u64, ClientHandle>,
    sources: HashMap<u32, SourceState>,
    selections: HashMap<SelectionKey, SignalSelection>,
    pending_commands: HashMap<u64, PendingCommand>,
    pending_vehicle_info: HashMap<u64, PendingVehicleInfo>,
    policy: VehiclePolicyEngine,
}

pub struct VehicleRouterConfig {
    pub socket_path: String,
    pub wasm_spec: String,
    pub fuel_per_call: u64,
    pub max_outputs_per_call: usize,
}

pub fn run_vehicle_router(config: VehicleRouterConfig) -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = Path::new(&config.socket_path);
    let euid = unsafe { libc::geteuid() };
    let egid = unsafe { libc::getegid() };
    println!(
        "VEHICLE_ROUTER_START socket={} euid={} egid={} socket_uid={} socket_gid={}",
        config.socket_path,
        euid,
        egid,
        UID_ROOT,
        ROUTER_SOCKET_GID_SYSTEM,
    );

    let (policy, abi_version) = VehiclePolicyEngine::new(
        &config.wasm_spec,
        config.fuel_per_call,
        config.max_outputs_per_call,
    )?;
    println!(
        "{}",
        ready_line(
            &policy,
            abi_version,
            config.fuel_per_call,
            config.max_outputs_per_call,
        )
    );

    let (listener, socket_mode) = open_router_listener(socket_path)?;

    println!(
        "VEHICLE_ROUTER_READY socket={} mode=0660 uid={} gid={} protocol={} socket_mode={} allowed_roles=root:SIGNAL_SOURCE|VHAL|MANAGER,system_appid:MANAGER,vehicle_network:VHAL",
        config.socket_path,
        UID_ROOT,
        ROUTER_SOCKET_GID_SYSTEM,
        ROUTER_PROTOCOL_VERSION,
        socket_mode,
    );

    let state = Arc::new(Mutex::new(RouterState {
        clients: HashMap::new(),
        sources: HashMap::new(),
        selections: HashMap::new(),
        pending_commands: HashMap::new(),
        pending_vehicle_info: HashMap::new(),
        policy,
    }));
    let next_client_id = Arc::new(AtomicU64::new(1));
    let next_command_id = Arc::new(AtomicU64::new(1));

    spawn_timeout_worker(Arc::clone(&state));

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let state = Arc::clone(&state);
                let next_client_id = Arc::clone(&next_client_id);
                let next_command_id = Arc::clone(&next_command_id);
                thread::spawn(move || {
                    if let Err(err) = handle_client(
                        stream,
                        state,
                        next_client_id,
                        next_command_id,
                    ) {
                        eprintln!("VEHICLE_ROUTER_CLIENT_ERROR error={err}");
                    }
                });
            }
            Err(err) => eprintln!("VEHICLE_ROUTER_ACCEPT_ERROR error={err}"),
        }
    }
    Ok(())
}

fn open_router_listener(socket_path: &Path) -> io::Result<(UnixListener, &'static str)> {
    if socket_path == Path::new(DEFAULT_ROUTER_SOCKET) {
        if let Some(raw_value) = env::var_os(INIT_ROUTER_SOCKET_ENV) {
            let value = raw_value.into_string().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{INIT_ROUTER_SOCKET_ENV} is not valid UTF-8"),
                )
            })?;
            let fd: RawFd = value.parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid {INIT_ROUTER_SOCKET_ENV} fd: {value}"),
                )
            })?;
            validate_inherited_listener_fd(fd)?;

            let listener = unsafe { UnixListener::from_raw_fd(fd) };
            let local_addr = listener.local_addr().map_err(|err| {
                io::Error::new(
                    err.kind(),
                    format!("inherited router socket local_addr failed fd={fd}: {err}"),
                )
            })?;
            let actual_path = local_addr.as_pathname().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("inherited router socket fd={fd} is unnamed"),
                )
            })?;
            if actual_path != socket_path {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "inherited router socket path mismatch fd={fd} expected={} actual={}",
                        socket_path.display(),
                        actual_path.display(),
                    ),
                ));
            }

            println!(
                "VEHICLE_ROUTER_SOCKET_MODE=init_inherited env={} fd={} socket={} listening=true",
                INIT_ROUTER_SOCKET_ENV,
                fd,
                socket_path.display(),
            );
            return Ok((listener, "init_inherited"));
        }
    } else if env::var_os(INIT_ROUTER_SOCKET_ENV).is_some() {
        println!(
            "VEHICLE_ROUTER_INIT_SOCKET_IGNORED reason=custom_socket_path env={} socket={}",
            INIT_ROUTER_SOCKET_ENV,
            socket_path.display(),
        );
    }

    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| startup_io_error("create_parent", parent, err))?;
    }
    if socket_path.exists() {
        fs::remove_file(socket_path)
            .map_err(|err| startup_io_error("remove_stale_socket", socket_path, err))?;
    }

    let listener = UnixListener::bind(socket_path)
        .map_err(|err| startup_io_error("bind_socket", socket_path, err))?;
    fs::set_permissions(socket_path, fs::Permissions::from_mode(0o660))
        .map_err(|err| startup_io_error("chmod_socket_0660", socket_path, err))?;
    set_socket_owner(socket_path, UID_ROOT, ROUTER_SOCKET_GID_SYSTEM).map_err(|err| {
        startup_io_error(
            "chown_socket_root_system_requires_privilege",
            socket_path,
            err,
        )
    })?;

    println!(
        "VEHICLE_ROUTER_SOCKET_MODE=manual_bind socket={} mode=0660 uid={} gid={}",
        socket_path.display(),
        UID_ROOT,
        ROUTER_SOCKET_GID_SYSTEM,
    );
    Ok((listener, "manual_bind"))
}

fn validate_inherited_listener_fd(fd: RawFd) -> io::Result<()> {
    if fd < 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("negative inherited router socket fd={fd}"),
        ));
    }
    if unsafe { libc::fcntl(fd, libc::F_GETFD) } < 0 {
        let err = io::Error::last_os_error();
        return Err(io::Error::new(
            err.kind(),
            format!("inherited router socket fd={fd} is not open: {err}"),
        ));
    }

    let socket_type = socket_option_i32(fd, libc::SO_TYPE)?;
    if socket_type != libc::SOCK_STREAM {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "inherited router socket fd={fd} has type={socket_type}, expected SOCK_STREAM={}",
                libc::SOCK_STREAM,
            ),
        ));
    }

    let accepting = socket_option_i32(fd, libc::SO_ACCEPTCONN)?;
    if accepting != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("inherited router socket fd={fd} is not listening SO_ACCEPTCONN={accepting}"),
        ));
    }

    Ok(())
}

fn socket_option_i32(fd: RawFd, option: libc::c_int) -> io::Result<i32> {
    let mut value: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            option,
            &mut value as *mut libc::c_int as *mut libc::c_void,
            &mut len,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if len as usize != std::mem::size_of::<libc::c_int>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unexpected getsockopt length fd={fd} option={option} len={len}"),
        ));
    }
    Ok(value)
}


fn startup_io_error(stage: &str, path: &Path, err: io::Error) -> io::Error {
    let euid = unsafe { libc::geteuid() };
    let egid = unsafe { libc::getegid() };
    io::Error::new(
        err.kind(),
        format!(
            "vehicle router startup stage={} path={} euid={} egid={} error={}",
            stage,
            path.display(),
            euid,
            egid,
            err,
        ),
    )
}

fn set_socket_owner(path: &Path, uid: u32, gid: u32) -> io::Result<()> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "router socket path contains NUL"))?;
    let rc = unsafe { libc::chown(path.as_ptr(), uid, gid) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn handle_client(
    mut stream: UnixStream,
    state: Arc<Mutex<RouterState>>,
    next_client_id: Arc<AtomicU64>,
    next_command_id: Arc<AtomicU64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (uid, gid, pid) = peer_credentials(&stream)?;
    let hello_envelope = read_envelope(&mut stream)?;
    if hello_envelope.protocol_version() != ROUTER_PROTOCOL_VERSION || !hello_envelope.has_hello() {
        send_hello_reject(&mut stream, uid, "first frame must be current-protocol Hello")?;
        return Ok(());
    }
    let hello = hello_envelope.hello();
    if hello.protocol_version() != ROUTER_PROTOCOL_VERSION {
        send_hello_reject(&mut stream, uid, "unsupported protocol version")?;
        return Ok(());
    }
    let Some(role) = Role::from_pb(hello.role()) else {
        send_hello_reject(&mut stream, uid, "invalid client role")?;
        return Ok(());
    };
    if !role_allowed(uid, role) {
        send_hello_reject(
            &mut stream,
            uid,
            &format!("uid {uid} is not allowed for role {}", role.name()),
        )?;
        eprintln!(
            "VEHICLE_ROUTER_PEER_REJECT uid={} gid={} pid={} role={} name={:?}",
            uid,
            gid,
            pid,
            role.name(),
            hello.client_name(),
        );
        return Ok(());
    }

    let client_id = next_client_id.fetch_add(1, Ordering::Relaxed);
    let client_name = hello.client_name().to_string();
    let (tx, rx) = mpsc::channel::<pb::Envelope>();
    let mut write_stream = stream.try_clone()?;

    let mut ack = pb::HelloAck::new();
    ack.set_accepted(true);
    ack.set_protocol_version(ROUTER_PROTOCOL_VERSION);
    ack.set_peer_uid(uid);
    let mut ack_envelope = envelope();
    ack_envelope.set_hello_ack(ack);
    write_envelope(&mut stream, &ack_envelope)?;

    thread::spawn(move || {
        while let Ok(message) = rx.recv() {
            if let Err(err) = write_envelope(&mut write_stream, &message) {
                eprintln!("VEHICLE_ROUTER_WRITE_ERROR client={} error={}", client_id, err);
                break;
            }
        }
    });

    {
        let mut locked = state.lock().map_err(|_| "vehicle router state poisoned")?;
        locked.clients.insert(
            client_id,
            ClientHandle {
                id: client_id,
                role,
                uid,
                name: client_name.clone(),
                tx: tx.clone(),
            },
        );
    }

    println!(
        "VEHICLE_ROUTER_CLIENT_ACCEPT client={} uid={} gid={} pid={} role={} name={:?}",
        client_id,
        uid,
        gid,
        pid,
        role.name(),
        client_name,
    );

    // After the client is inserted into RouterState, every exit path must release
    // its registered sources. Capture all loop errors first so `?` and unexpected
    // socket errors cannot bypass cleanup_client().
    let session_result = (|| -> Result<(), Box<dyn std::error::Error>> {
        loop {
            let message = match read_envelope(&mut stream) {
                Ok(message) => message,
                Err(err) if matches!(err.kind(), io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset | io::ErrorKind::BrokenPipe) => break,
                Err(err) => return Err(err.into()),
            };
            if message.protocol_version() != ROUTER_PROTOCOL_VERSION {
                eprintln!(
                    "VEHICLE_ROUTER_PROTOCOL_REJECT client={} version={}",
                    client_id,
                    message.protocol_version(),
                );
                break;
            }

            if message.has_register_source() {
                handle_register_source(client_id, role, &tx, message.register_source(), &state)?;
            } else if message.has_vehicle_signals() {
                handle_vehicle_signals(client_id, role, message.vehicle_signals(), &state)?;
            } else if message.has_vhal_set_request() {
                handle_vhal_set_request(
                    client_id,
                    role,
                    &tx,
                    message.vhal_set_request(),
                    &state,
                    &next_command_id,
                )?;
            } else if message.has_command_acks() {
                handle_command_acks(client_id, role, message.command_acks(), &state)?;
            } else if message.has_vehicle_info_apply_request() {
                handle_vehicle_info_apply_request(
                    client_id,
                    role,
                    message.vehicle_info_apply_request(),
                    &state,
                )?;
            } else if message.has_vehicle_info_apply_result() {
                handle_vehicle_info_apply_result(
                    client_id,
                    role,
                    message.vehicle_info_apply_result(),
                    &state,
                )?;
            } else if message.has_ping() {
                let mut pong = pb::Pong::new();
                pong.set_nonce(message.ping().nonce());
                let mut response = envelope();
                response.set_pong(pong);
                let _ = tx.send(response);
            } else {
                eprintln!("VEHICLE_ROUTER_MESSAGE_REJECT client={} reason=unexpected_payload", client_id);
            }
        }
        Ok(())
    })();

    cleanup_client(client_id, &state);
    println!(
        "VEHICLE_ROUTER_CLIENT_DISCONNECT client={} uid={} role={} name={:?}",
        client_id,
        uid,
        role.name(),
        client_name,
    );
    session_result
}

fn send_hello_reject(stream: &mut UnixStream, uid: u32, error: &str) -> io::Result<()> {
    let mut ack = pb::HelloAck::new();
    ack.set_accepted(false);
    ack.set_error(error.to_string());
    ack.set_protocol_version(ROUTER_PROTOCOL_VERSION);
    ack.set_peer_uid(uid);
    let mut response = envelope();
    response.set_hello_ack(ack);
    write_envelope(stream, &response)
}

fn role_allowed(uid: u32, role: Role) -> bool {
    if uid == UID_ROOT {
        return true;
    }
    if uid == UID_VEHICLE_NETWORK {
        return role == Role::Vhal;
    }

    // Arda Vehicle Manager is platform-signed and runs with android.uid.system.
    // Android derives per-user UIDs by adding userId * PER_USER_RANGE, so user 10
    // is 1001000 rather than 1000. Accept only the system appId for MANAGER.
    let app_id = uid % ANDROID_PER_USER_RANGE;
    app_id == UID_SYSTEM && role == Role::Manager
}

fn handle_register_source(
    client_id: u64,
    role: Role,
    tx: &mpsc::Sender<pb::Envelope>,
    request: &pb::RegisterSource,
    state: &Arc<Mutex<RouterState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut result = pb::RegisterSourceResult::new();
    result.set_source_id(request.source_id());

    let validation = (|| -> Result<SourceState, String> {
        if !matches!(role, Role::SignalSource | Role::Manager) {
            return Err(format!("role {} cannot register sources", role.name()));
        }
        if request.source_id() == 0 {
            return Err("source_id 0 is reserved".to_string());
        }
        if request.source_id() > i32::MAX as u32 {
            return Err(format!(
                "source_id {} exceeds Wasm ABI i32 source lane max {}",
                request.source_id(),
                i32::MAX,
            ));
        }
        if request.source_name().trim().is_empty() {
            return Err("source_name must not be empty".to_string());
        }

        let publish_capabilities = parse_capabilities(request.publish_capabilities(), false)?;
        let command_capabilities = parse_capabilities(request.command_capabilities(), true)?;
        if role == Role::SignalSource && !command_capabilities.is_empty() {
            return Err(
                "SIGNAL_SOURCE cannot declare command capabilities; use an approved MANAGER adapter"
                    .to_string(),
            );
        }
        if publish_capabilities.is_empty() && command_capabilities.is_empty() {
            return Err("source must declare at least one capability".to_string());
        }

        let locked = state.lock().map_err(|_| "vehicle router state poisoned".to_string())?;
        if locked.sources.contains_key(&request.source_id()) {
            return Err(format!("source_id {} already registered", request.source_id()));
        }
        let client = locked
            .clients
            .get(&client_id)
            .ok_or_else(|| "client disappeared during registration".to_string())?;
        Ok(SourceState {
            source_id: request.source_id(),
            client_id,
            name: request.source_name().to_string(),
            priority: request.priority(),
            publish_capabilities,
            command_capabilities,
            tx: client.tx.clone(),
        })
    })();

    match validation {
        Ok(source) => {
            println!(
                "VEHICLE_ROUTER_SOURCE_REGISTER client={} source={} name={:?} priority={} publish={:?} commands={:?}",
                client_id,
                source.source_id,
                source.name,
                source.priority,
                source.publish_capabilities.iter().map(|cap| cap.name()).collect::<Vec<_>>(),
                source.command_capabilities.iter().map(|cap| cap.name()).collect::<Vec<_>>(),
            );
            state
                .lock()
                .map_err(|_| "vehicle router state poisoned")?
                .sources
                .insert(source.source_id, source);
            result.set_accepted(true);
        }
        Err(error) => {
            result.set_accepted(false);
            result.set_error(error.clone());
            eprintln!(
                "VEHICLE_ROUTER_SOURCE_REJECT client={} source={} error={}",
                client_id,
                request.source_id(),
                error,
            );
        }
    }

    let mut response = envelope();
    response.set_register_source_result(result);
    tx.send(response)?;
    Ok(())
}

fn parse_capabilities(values: &[i32], commands: bool) -> Result<HashSet<Capability>, String> {
    let mut result = HashSet::new();
    for value in values {
        let Some(capability) = Capability::from_id(*value) else {
            return Err(format!("unknown capability id={value}"));
        };
        let is_command = matches!(
            capability,
            Capability::HvacControl | Capability::BodyControl | Capability::ChargingControl
        );
        if is_command != commands {
            return Err(format!(
                "capability {} is in the wrong capability set",
                capability.name()
            ));
        }
        result.insert(capability);
    }
    Ok(result)
}

fn handle_vehicle_signals(
    client_id: u64,
    role: Role,
    messages: &pb::VehicleSignals,
    state: &Arc<Mutex<RouterState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(role, Role::SignalSource | Role::Manager) {
        return Err(format!("role {} cannot publish vehicle signals", role.name()).into());
    }

    for message in messages.signals() {
        let signal = match vehicle_signal_from_pb(message) {
            Ok(signal) => signal,
            Err(reason) => {
                eprintln!(
                    "VEHICLE_ROUTER_SIGNAL_REJECT client={} source={} property={} reason={}",
                    client_id,
                    message.source_id(),
                    message.property_id(),
                    reason,
                );
                continue;
            }
        };

        let mut locked = state.lock().map_err(|_| "vehicle router state poisoned")?;
        let Some(source) = locked.sources.get(&signal.source_id).cloned() else {
            eprintln!(
                "VEHICLE_ROUTER_SIGNAL_REJECT client={} source={} property={} reason=source_not_registered",
                client_id, signal.source_id, signal.property_id
            );
            continue;
        };
        if source.client_id != client_id {
            eprintln!(
                "VEHICLE_ROUTER_SIGNAL_REJECT client={} source={} property={} reason=source_owned_by_other_client",
                client_id, signal.source_id, signal.property_id
            );
            continue;
        }
        let capability = match validate_vehicle_signal(&signal) {
            Ok(capability) => capability,
            Err(reason) => {
                eprintln!(
                    "VEHICLE_ROUTER_SIGNAL_REJECT client={} {} reason={}",
                    client_id,
                    signal.summary(),
                    reason,
                );
                continue;
            }
        };
        if !source.publish_capabilities.contains(&capability) {
            eprintln!(
                "VEHICLE_ROUTER_SIGNAL_REJECT client={} source={} property={} capability={} reason=capability_denied",
                client_id,
                source.source_id,
                property_name(signal.property_id),
                capability.name(),
            );
            continue;
        }

        let decision = evaluate_signal_source(&locked, &source, capability, signal.area_id);
        let previous_source_id = match decision {
            SourceSelectionDecision::Accepted { previous_source_id } => previous_source_id,
            SourceSelectionDecision::Shadowed => {
                println!(
                    "VEHICLE_ROUTER_SIGNAL_SHADOWED source={} property={} capability={} area={} priority={}",
                    source.source_id,
                    property_name(signal.property_id),
                    capability.name(),
                    signal.area_id,
                    source.priority,
                );
                continue;
            }
        };

        if previous_source_id.is_some() {
            if let Err(err) = locked.policy.reset_capability(capability.id(), signal.area_id) {
                eprintln!(
                    "VEHICLE_WASM_RESET_ERROR capability={} area={} from={:?} to={} error={}",
                    capability.name(),
                    signal.area_id,
                    previous_source_id,
                    source.source_id,
                    err,
                );
                continue;
            }
        }

        let output = match locked.policy.process_vehicle_signal(&signal) {
            Ok(output) => output,
            Err(err) => {
                eprintln!(
                    "VEHICLE_WASM_TRAP direction=vehicle_to_vhal source={} property={} capability={} error={}",
                    source.source_id,
                    property_name(signal.property_id),
                    capability.name(),
                    err,
                );
                reset_capability_after_policy_failure(
                    &mut locked,
                    capability,
                    signal.area_id,
                    source.source_id,
                );
                continue;
            }
        };
        if output.status != 0 {
            eprintln!(
                "VEHICLE_WASM_EVENT_REJECT source={} property={} capability={} guest_status={} outputs_discarded=true",
                source.source_id,
                property_name(signal.property_id),
                capability.name(),
                output.status,
            );
            reset_capability_after_policy_failure(
                &mut locked,
                capability,
                signal.area_id,
                source.source_id,
            );
            continue;
        }

        commit_signal_source(&mut locked, &source, capability, signal.area_id);
        if let Some(previous) = previous_source_id {
            println!(
                "VEHICLE_ROUTER_SOURCE_SWITCH capability={} area={} from={} to={} priority={}",
                capability.name(),
                signal.area_id,
                previous,
                source.source_id,
                source.priority,
            );
        }

        if output.dropped_outputs > 0 {
            eprintln!(
                "VEHICLE_WASM_LIMIT direction=vehicle_to_vhal source={} property={} dropped_outputs={}",
                source.source_id,
                property_name(signal.property_id),
                output.dropped_outputs,
            );
        }
        if !output.vhal_events.is_empty() {
            println!(
                "VEHICLE_ROUTER_SIGNAL_APPLY source={} property={} capability={} area={} vhal_events={}",
                source.source_id,
                property_name(signal.property_id),
                capability.name(),
                signal.area_id,
                output.vhal_events.len(),
            );
            broadcast_vhal_events_locked(&locked, &output.vhal_events);
        }
    }
    Ok(())
}

fn evaluate_signal_source(
    state: &RouterState,
    source: &SourceState,
    capability: Capability,
    area_id: i32,
) -> SourceSelectionDecision {
    let key = SelectionKey { capability, area_id };
    let now = Instant::now();
    match state.selections.get(&key) {
        None => SourceSelectionDecision::Accepted {
            previous_source_id: None,
        },
        Some(selected) if selected.source_id == source.source_id => {
            SourceSelectionDecision::Accepted {
                previous_source_id: None,
            }
        }
        Some(selected) if source.priority > selected.priority => {
            SourceSelectionDecision::Accepted {
                previous_source_id: Some(selected.source_id),
            }
        }
        Some(selected)
            if now.duration_since(selected.last_seen) >= SIGNAL_SOURCE_STALE_AFTER =>
        {
            SourceSelectionDecision::Accepted {
                previous_source_id: Some(selected.source_id),
            }
        }
        Some(_) => SourceSelectionDecision::Shadowed,
    }
}

fn commit_signal_source(
    state: &mut RouterState,
    source: &SourceState,
    capability: Capability,
    area_id: i32,
) {
    state.selections.insert(
        SelectionKey { capability, area_id },
        SignalSelection {
            source_id: source.source_id,
            priority: source.priority,
            last_seen: Instant::now(),
        },
    );
}

fn reset_capability_after_policy_failure(
    state: &mut RouterState,
    capability: Capability,
    area_id: i32,
    source_id: u32,
) {
    if let Err(err) = state.policy.reset_capability(capability.id(), area_id) {
        eprintln!(
            "VEHICLE_WASM_RESET_ERROR capability={} area={} source={} reason=policy_failure error={}",
            capability.name(),
            area_id,
            source_id,
            err,
        );
    }
}

fn handle_vhal_set_request(
    client_id: u64,
    role: Role,
    tx: &mpsc::Sender<pb::Envelope>,
    message: &pb::VhalSetRequest,
    state: &Arc<Mutex<RouterState>>,
    next_command_id: &AtomicU64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut result = pb::VhalSetResult::new();
    result.set_request_id(message.request_id());
    if role != Role::Vhal {
        result.set_handled(false);
        result.set_status(5);
        result.set_error(format!("role {} cannot submit VHAL set requests", role.name()));
        send_vhal_set_result(tx, result)?;
        return Ok(());
    }

    let request = match vhal_set_from_pb(message) {
        Ok(request) => request,
        Err(reason) => {
            result.set_handled(false);
            result.set_status(5);
            result.set_error(reason);
            send_vhal_set_result(tx, result)?;
            return Ok(());
        }
    };

    let mut locked = state.lock().map_err(|_| "vehicle router state poisoned")?;
    let output = match locked.policy.process_vhal_set(&request) {
        Ok(output) => output,
        Err(err) => {
            result.set_handled(false);
            result.set_status(5);
            result.set_error(err.to_string());
            send_vhal_set_result(tx, result)?;
            return Ok(());
        }
    };
    if output.status != 0 || output.vehicle_commands.is_empty() {
        result.set_handled(false);
        result.set_status(5);
        result.set_error(format!(
            "policy did not map set request property={}({}) guest_status={}",
            property_name(request.property_id),
            request.property_id,
            output.status,
        ));
        send_vhal_set_result(tx, result)?;
        return Ok(());
    }

    let mut routed = Vec::new();
    for mut command in output.vehicle_commands {
        let capability = match validate_vehicle_command(&command) {
            Ok(capability) => capability,
            Err(reason) => {
                result.set_handled(false);
                result.set_status(5);
                result.set_error(reason);
                send_vhal_set_result(tx, result)?;
                return Ok(());
            }
        };
        let Some(source) = choose_command_source(&locked, capability) else {
            result.set_handled(false);
            result.set_status(5);
            result.set_error(format!("no active adapter for capability {}", capability.name()));
            send_vhal_set_result(tx, result)?;
            return Ok(());
        };
        command.command_id = next_command_id.fetch_add(1, Ordering::Relaxed);
        command.source_id = source.source_id;
        routed.push((source, command));
    }

    for (source, command) in &routed {
        let mut commands = pb::VehicleCommands::new();
        commands.set_commands(vec![vehicle_command_to_pb(command)]);
        let mut outgoing = envelope();
        outgoing.set_vehicle_commands(commands);
        source.tx.send(outgoing)?;
        locked.pending_commands.insert(
            command.command_id,
            PendingCommand {
                command_id: command.command_id,
                source_id: source.source_id,
                request_id: request.request_id,
                property_id: request.property_id,
                area_id: request.area_id,
                deadline: Instant::now() + COMMAND_TIMEOUT,
            },
        );
        println!(
            "VEHICLE_ROUTER_COMMAND_DISPATCH request={} command={} source={} property={} area={} value={}",
            request.request_id,
            command.command_id,
            source.source_id,
            property_name(command.property_id),
            command.area_id,
            command.value.summary(),
        );
    }

    result.set_handled(true);
    result.set_status(0);
    send_vhal_set_result(tx, result)?;
    println!(
        "VEHICLE_ROUTER_VHAL_SET_HANDLED client={} request={} property={}({}) area={} commands={}",
        client_id,
        request.request_id,
        property_name(request.property_id),
        request.property_id,
        request.area_id,
        routed.len(),
    );
    Ok(())
}

fn choose_command_source(state: &RouterState, capability: Capability) -> Option<SourceState> {
    state
        .sources
        .values()
        .filter(|source| source.command_capabilities.contains(&capability))
        .max_by(|left, right| {
            left.priority
                .cmp(&right.priority)
                // Equal priority is deterministic: the lower stable source id wins.
                .then_with(|| right.source_id.cmp(&left.source_id))
        })
        .cloned()
}

fn send_vhal_set_result(
    tx: &mpsc::Sender<pb::Envelope>,
    result: pb::VhalSetResult,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut response = envelope();
    response.set_vhal_set_result(result);
    tx.send(response)?;
    Ok(())
}

fn handle_vehicle_info_apply_request(
    client_id: u64,
    role: Role,
    request: &pb::VehicleInfoApplyRequest,
    state: &Arc<Mutex<RouterState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    if role != Role::Manager {
        return Err(format!("role {} cannot apply vehicle info", role.name()).into());
    }
    if request.request_id() == 0 {
        return Err("vehicle info request_id must be non-zero".into());
    }

    let mut locked = state.lock().map_err(|_| "vehicle router state poisoned")?;
    if locked.pending_vehicle_info.contains_key(&request.request_id()) {
        return Err(format!(
            "duplicate vehicle info request_id={}",
            request.request_id(),
        )
        .into());
    }

    let vhal_clients = locked
        .clients
        .values()
        .filter(|client| client.role == Role::Vhal)
        .cloned()
        .collect::<Vec<_>>();
    if vhal_clients.len() != 1 {
        let error = format!("expected one VHAL client, found {}", vhal_clients.len());
        if let Some(manager) = locked.clients.get(&client_id) {
            let mut result = pb::VehicleInfoApplyResult::new();
            result.set_request_id(request.request_id());
            result.set_applied(false);
            result.set_error(error.clone());
            let mut outgoing = envelope();
            outgoing.set_vehicle_info_apply_result(result);
            let _ = manager.tx.send(outgoing);
        }
        return Ok(());
    }

    let manager_id = locked
        .clients
        .get(&client_id)
        .ok_or("manager client disappeared")?
        .id;
    let vhal_id = vhal_clients[0].id;
    let vhal_tx = vhal_clients[0].tx.clone();
    let mut outgoing = envelope();
    outgoing.set_vehicle_info_apply_request(request.clone());
    vhal_tx.send(outgoing)?;
    locked.pending_vehicle_info.insert(
        request.request_id(),
        PendingVehicleInfo {
            request_id: request.request_id(),
            manager_client_id: manager_id,
            deadline: Instant::now() + VEHICLE_INFO_TIMEOUT,
        },
    );
    println!(
        "VEHICLE_ROUTER_VEHICLE_INFO_FORWARD request={} manager_client={} vhal_client={} fuel={:?} connectors={:?}",
        request.request_id(),
        manager_id,
        vhal_id,
        request.fuel_types(),
        request.connector_types(),
    );
    Ok(())
}

fn handle_vehicle_info_apply_result(
    client_id: u64,
    role: Role,
    result: &pb::VehicleInfoApplyResult,
    state: &Arc<Mutex<RouterState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    if role != Role::Vhal {
        return Err(format!("role {} cannot return vehicle info result", role.name()).into());
    }
    let mut locked = state.lock().map_err(|_| "vehicle router state poisoned")?;
    let Some(pending) = locked.pending_vehicle_info.remove(&result.request_id()) else {
        eprintln!(
            "VEHICLE_ROUTER_VEHICLE_INFO_RESULT_DROP client={} request={} reason=unknown_request",
            client_id,
            result.request_id(),
        );
        return Ok(());
    };
    let Some(manager) = locked.clients.get(&pending.manager_client_id) else {
        eprintln!(
            "VEHICLE_ROUTER_VEHICLE_INFO_RESULT_DROP client={} request={} reason=manager_disconnected",
            client_id,
            result.request_id(),
        );
        return Ok(());
    };
    let mut outgoing = envelope();
    outgoing.set_vehicle_info_apply_result(result.clone());
    manager.tx.send(outgoing)?;
    println!(
        "VEHICLE_ROUTER_VEHICLE_INFO_RESULT request={} applied={} error={:?}",
        result.request_id(),
        result.applied(),
        result.error(),
    );
    Ok(())
}

fn handle_command_acks(
    client_id: u64,
    role: Role,
    messages: &pb::CommandAcks,
    state: &Arc<Mutex<RouterState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    if role != Role::Manager {
        return Err(format!("role {} cannot acknowledge vehicle commands", role.name()).into());
    }
    let mut locked = state.lock().map_err(|_| "vehicle router state poisoned")?;
    for ack in messages.acknowledgements() {
        let command_id = ack.command_id();
        let Some(pending) = locked.pending_commands.get(&command_id) else {
            eprintln!(
                "VEHICLE_ROUTER_COMMAND_ACK_REJECT client={} command={} reason=unknown_command",
                client_id, command_id
            );
            continue;
        };
        let source_owned = locked
            .sources
            .get(&pending.source_id)
            .map(|source| source.client_id == client_id)
            .unwrap_or(false);
        if !source_owned {
            eprintln!(
                "VEHICLE_ROUTER_COMMAND_ACK_REJECT client={} command={} reason=wrong_client",
                client_id, command_id
            );
            continue;
        }
        let pending = locked.pending_commands.remove(&command_id).expect("pending command existed");
        println!(
            "VEHICLE_ROUTER_COMMAND_ACK command={} source={} status={:?} error={:?}",
            command_id,
            pending.source_id,
            ack.status(),
            ack.error(),
        );
        if ack.status() != pb::CommandStatus::COMMAND_OK {
            broadcast_vhal_set_error_locked(
                &locked,
                &pending,
                ack.status().value(),
                ack.error(),
            );
        }
    }
    Ok(())
}

fn broadcast_vhal_events_locked(state: &RouterState, events: &[super::model::VhalEvent]) {
    let mut message = pb::VhalEvents::new();
    message.set_events(events.iter().map(vhal_event_to_pb).collect());
    let mut outgoing = envelope();
    outgoing.set_vhal_events(message);
    for client in state.clients.values().filter(|client| client.role == Role::Vhal) {
        if let Err(err) = client.tx.send(outgoing.clone()) {
            eprintln!(
                "VEHICLE_ROUTER_VHAL_EVENT_SEND_ERROR client={} uid={} name={:?} error={}",
                client.id, client.uid, client.name, err
            );
        }
    }
}

fn broadcast_vhal_set_error_locked(
    state: &RouterState,
    pending: &PendingCommand,
    status: i32,
    error: &str,
) {
    let mut message = pb::VhalSetError::new();
    message.set_request_id(pending.request_id);
    message.set_property_id(pending.property_id);
    message.set_area_id(pending.area_id);
    message.set_status(status);
    message.set_error(error.to_string());
    let mut outgoing = envelope();
    outgoing.set_vhal_set_error(message);
    for client in state.clients.values().filter(|client| client.role == Role::Vhal) {
        let _ = client.tx.send(outgoing.clone());
    }
}

fn cleanup_client(client_id: u64, state: &Arc<Mutex<RouterState>>) {
    let Ok(mut locked) = state.lock() else {
        return;
    };
    locked.clients.remove(&client_id);
    let removed_sources = locked
        .sources
        .values()
        .filter(|source| source.client_id == client_id)
        .map(|source| source.source_id)
        .collect::<HashSet<_>>();
    locked.sources.retain(|_, source| source.client_id != client_id);

    // Stateful Wasm mappings are capability-scoped. When the selected source disappears,
    // reset that capability before making it unowned so the next source cannot combine its
    // first signal with state retained from the disconnected adapter.
    let removed_selections = locked
        .selections
        .iter()
        .filter(|(_, selected)| removed_sources.contains(&selected.source_id))
        .map(|(key, selected)| (*key, selected.source_id))
        .collect::<Vec<_>>();
    for (key, source_id) in &removed_selections {
        if let Err(err) = locked.policy.reset_capability(key.capability.id(), key.area_id) {
            eprintln!(
                "VEHICLE_WASM_RESET_ERROR capability={} area={} source={} reason=source_disconnect error={}",
                key.capability.name(),
                key.area_id,
                source_id,
                err,
            );
        }
    }
    locked
        .selections
        .retain(|_, selected| !removed_sources.contains(&selected.source_id));
    for (key, source_id) in removed_selections {
        println!(
            "VEHICLE_ROUTER_SOURCE_RELEASE capability={} area={} source={} reason=client_disconnect",
            key.capability.name(),
            key.area_id,
            source_id,
        );
    }

    let failed = locked
        .pending_commands
        .values()
        .filter(|pending| removed_sources.contains(&pending.source_id))
        .map(|pending| pending.command_id)
        .collect::<Vec<_>>();
    for command_id in failed {
        if let Some(pending) = locked.pending_commands.remove(&command_id) {
            broadcast_vhal_set_error_locked(&locked, &pending, 3, "adapter disconnected");
        }
    }

    let disconnected_was_vhal = !locked.clients.values().any(|client| client.role == Role::Vhal);
    let affected_vehicle_info = locked
        .pending_vehicle_info
        .values()
        .filter(|pending| {
            pending.manager_client_id == client_id || disconnected_was_vhal
        })
        .map(|pending| pending.request_id)
        .collect::<Vec<_>>();
    for request_id in affected_vehicle_info {
        if let Some(pending) = locked.pending_vehicle_info.remove(&request_id) {
            if pending.manager_client_id != client_id {
                if let Some(manager) = locked.clients.get(&pending.manager_client_id) {
                    let mut result = pb::VehicleInfoApplyResult::new();
                    result.set_request_id(request_id);
                    result.set_applied(false);
                    result.set_error("VHAL client disconnected".to_string());
                    let mut outgoing = envelope();
                    outgoing.set_vehicle_info_apply_result(result);
                    let _ = manager.tx.send(outgoing);
                }
            }
        }
    }
}

fn spawn_timeout_worker(state: Arc<Mutex<RouterState>>) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(250));
        let Ok(mut locked) = state.lock() else {
            return;
        };
        let now = Instant::now();
        let expired = locked
            .pending_commands
            .values()
            .filter(|pending| pending.deadline <= now)
            .map(|pending| pending.command_id)
            .collect::<Vec<_>>();
        for command_id in expired {
            if let Some(pending) = locked.pending_commands.remove(&command_id) {
                eprintln!(
                    "VEHICLE_ROUTER_COMMAND_TIMEOUT command={} source={} request={} property={}({}) area={}",
                    pending.command_id,
                    pending.source_id,
                    pending.request_id,
                    property_name(pending.property_id),
                    pending.property_id,
                    pending.area_id,
                );
                broadcast_vhal_set_error_locked(&locked, &pending, 4, "vehicle command timeout");
            }
        }

        let expired_vehicle_info = locked
            .pending_vehicle_info
            .values()
            .filter(|pending| pending.deadline <= now)
            .map(|pending| pending.request_id)
            .collect::<Vec<_>>();
        for request_id in expired_vehicle_info {
            if let Some(pending) = locked.pending_vehicle_info.remove(&request_id) {
                eprintln!(
                    "VEHICLE_ROUTER_VEHICLE_INFO_TIMEOUT request={} manager_client={}",
                    pending.request_id,
                    pending.manager_client_id,
                );
                if let Some(manager) = locked.clients.get(&pending.manager_client_id) {
                    let mut result = pb::VehicleInfoApplyResult::new();
                    result.set_request_id(request_id);
                    result.set_applied(false);
                    result.set_error("vehicle info apply timeout".to_string());
                    let mut outgoing = envelope();
                    outgoing.set_vehicle_info_apply_result(result);
                    let _ = manager.tx.send(outgoing);
                }
            }
        }
    });
}

pub fn router_state_summary(socket_path: &str) -> String {
    format!(
        "router_socket={} protocol={} source_stale_ms={} command_timeout_ms={} vehicle_info_timeout_ms={}",
        socket_path,
        ROUTER_PROTOCOL_VERSION,
        SIGNAL_SOURCE_STALE_AFTER.as_millis(),
        COMMAND_TIMEOUT.as_millis(),
        VEHICLE_INFO_TIMEOUT.as_millis(),
    )
}
