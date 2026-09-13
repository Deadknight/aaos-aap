use flate2::read::GzDecoder;
use rustls::crypto::aws_lc_rs;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::sign::CertifiedKey;
use rustls::version::{TLS12, TLS13};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Cursor, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_CONTROL_URL: &str = "https://aap.opencharge.top/control.php";
pub const DEFAULT_STATE_DIR: &str = "/data/local/tmp/arda-aap-key-update";
pub const DEFAULT_CERT_DIR: &str = "/data/local/tmp/arda-certs";
const ANDROID_CA_DIR: &str = "/system/etc/security/cacerts";
const ACTIVE_LINK_NAME: &str = "active";
const MAX_CERT_BYTES: usize = 512 * 1024;
const MAX_KEY_BYTES: usize = 512 * 1024;
const MAX_REDIRECTS: usize = 3;
const MAX_PENDING_ATTEMPTS: u64 = 2;
static AAP_ATTEMPT_PENDING_SHA: OnceLock<Mutex<Option<String>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct Config {
    pub enabled: bool,
    pub control_url: String,
    pub allow_direct_internet: bool,
    pub check_on_start: bool,
    pub check_on_direct_ready: bool,
    pub check_on_proxy_ready: bool,
    pub check_interval_seconds: u64,
    pub connect_debounce_seconds: u64,
    pub retry_interval_seconds: u64,
    pub download_timeout_seconds: u64,
    pub max_manifest_bytes: usize,
    pub max_archive_bytes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            control_url: DEFAULT_CONTROL_URL.to_string(),
            allow_direct_internet: true,
            check_on_start: true,
            check_on_direct_ready: true,
            check_on_proxy_ready: true,
            check_interval_seconds: 86_400,
            connect_debounce_seconds: 300,
            retry_interval_seconds: 900,
            download_timeout_seconds: 30,
            max_manifest_bytes: 64 * 1024,
            max_archive_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyEndpoint {
    pub ip: IpAddr,
    pub port: u16,
}

#[derive(Debug)]
struct RuntimeState {
    proxy: Option<ProxyEndpoint>,
    generation: u64,
}

#[derive(Clone)]
pub struct Runtime {
    shared: Arc<(Mutex<RuntimeState>, Condvar)>,
}

#[derive(Debug)]
struct Manifest {
    sha: String,
    url: String,
}

#[derive(Debug)]
struct HttpsUrl {
    host: String,
    port: u16,
    path: String,
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Debug)]
struct CandidateKeys {
    cert: Vec<u8>,
    key: Vec<u8>,
}

impl Runtime {
    pub fn start(config: Config) -> Option<Self> {
        if !config.enabled {
            println!("ARDA_AAP_KEY_UPDATE=DISABLED");
            return None;
        }

        let shared = Arc::new((
            Mutex::new(RuntimeState {
                proxy: None,
                generation: 0,
            }),
            Condvar::new(),
        ));
        let worker_shared = shared.clone();
        let worker_config = config.clone();
        thread::Builder::new()
            .name("arda-aap-key-update".to_string())
            .spawn(move || worker_loop(worker_shared, worker_config))
            .map_err(|err| {
                eprintln!("ARDA_AAP_KEY_UPDATE_WORKER=FAIL error={err}");
                err
            })
            .ok()?;

        println!(
            "ARDA_AAP_KEY_UPDATE=ENABLED control_url={} interval_seconds={} direct_internet={} on_start={} on_direct_ready={} on_proxy_ready={}",
            config.control_url,
            config.check_interval_seconds,
            config.allow_direct_internet,
            config.check_on_start,
            config.check_on_direct_ready,
            config.check_on_proxy_ready
        );
        Some(Self { shared })
    }

    pub fn proxy_ready(&self, ip: IpAddr, port: u16) {
        let (lock, wake) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let endpoint = ProxyEndpoint { ip, port };
        if state.proxy.as_ref() != Some(&endpoint) {
            state.generation = state.generation.wrapping_add(1);
        }
        state.proxy = Some(endpoint.clone());
        println!(
            "ARDA_AAP_KEY_UPDATE_PROXY=READY endpoint={}:{} generation={}",
            endpoint.ip, endpoint.port, state.generation
        );
        wake.notify_all();
    }

    pub fn proxy_gone(&self) {
        let (lock, wake) = &*self.shared;
        let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.proxy.take().is_some() {
            println!("ARDA_AAP_KEY_UPDATE_PROXY=GONE");
        }
        wake.notify_all();
    }
}

pub fn run_cli(args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::default();
    let mut proxy: Option<ProxyEndpoint> = None;
    let mut force = false;
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--proxy" => {
                index += 1;
                let value = args.get(index).ok_or("--proxy requires IP:PORT")?;
                let addr: SocketAddr = value.parse()?;
                proxy = Some(ProxyEndpoint {
                    ip: addr.ip(),
                    port: addr.port(),
                });
            }
            "--control-url" => {
                index += 1;
                config.control_url = args
                    .get(index)
                    .ok_or("--control-url requires URL")?
                    .clone();
            }
            "--timeout-seconds" => {
                index += 1;
                config.download_timeout_seconds = args
                    .get(index)
                    .ok_or("--timeout-seconds requires N")?
                    .parse()?;
            }
            "--max-archive-bytes" => {
                index += 1;
                config.max_archive_bytes = args
                    .get(index)
                    .ok_or("--max-archive-bytes requires N")?
                    .parse()?;
            }
            "--direct" => config.allow_direct_internet = true,
            "--no-direct" => config.allow_direct_internet = false,
            "--force" => force = true,
            "--help" | "-h" => {
                println!(
                    "aap-key-update [--proxy IP:PORT] [--direct|--no-direct] [--control-url HTTPS_URL] [--timeout-seconds N] [--max-archive-bytes N] [--force]"
                );
                return Ok(());
            }
            other => return Err(format!("unknown aap-key-update argument: {other}").into()),
        }
        index += 1;
    }

    if !config.allow_direct_internet && proxy.is_none() {
        return Err("aap-key-update requires direct internet or --proxy IP:PORT".into());
    }
    ensure_state_layout()?;
    let outcome = check_once(&config, proxy.as_ref(), force)?;
    println!("ARDA_AAP_KEY_UPDATE_MANUAL={outcome}");
    Ok(())
}

fn worker_loop(shared: Arc<(Mutex<RuntimeState>, Condvar)>, config: Config) {
    if let Err(err) = ensure_state_layout() {
        eprintln!("ARDA_AAP_KEY_UPDATE_STATE=FAIL error={err}");
    }

    let mut observed_generation = 0u64;
    let mut first_iteration = true;
    let mut direct_online: Option<bool> = None;
    loop {
        let (proxy, generation) = {
            let (lock, _) = &*shared;
            let state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            (state.proxy.clone(), state.generation)
        };

        let now = unix_seconds();
        let current_direct_online = if config.allow_direct_internet {
            probe_direct_connectivity(&config.control_url, Duration::from_secs(3))
        } else {
            false
        };
        let direct_connection_trigger = config.check_on_direct_ready
            && current_direct_online
            && direct_online == Some(false);
        if direct_online != Some(current_direct_online) {
            println!(
                "ARDA_AAP_KEY_UPDATE_DIRECT_STATE={}",
                if current_direct_online { "READY" } else { "GONE" }
            );
        }
        direct_online = Some(current_direct_online);

        let last_success = read_u64_file(&state_path("last_success_unix")).unwrap_or(0);
        let last_attempt = read_u64_file(&state_path("last_attempt_unix")).unwrap_or(0);

        let startup_trigger = first_iteration && config.check_on_start;
        let connection_trigger = config.check_on_proxy_ready
            && proxy.is_some()
            && generation != observed_generation
            && now.saturating_sub(last_attempt) >= config.connect_debounce_seconds;
        let daily_due = (last_success == 0 && last_attempt == 0)
            || (last_success > 0
                && now.saturating_sub(last_success) >= config.check_interval_seconds);
        let retry_due = last_attempt > last_success
            && now.saturating_sub(last_attempt) >= config.retry_interval_seconds;

        first_iteration = false;

        if startup_trigger
            || direct_connection_trigger
            || connection_trigger
            || daily_due
            || retry_due
        {
            if startup_trigger || connection_trigger {
                observed_generation = generation;
            }
            let _ = write_u64_atomic(&state_path("last_attempt_unix"), now);
            let trigger = if startup_trigger {
                "startup"
            } else if direct_connection_trigger {
                "direct-ready"
            } else if connection_trigger {
                "proxy-ready"
            } else if daily_due {
                "daily"
            } else {
                "retry"
            };
            println!(
                "ARDA_AAP_KEY_UPDATE_CHECK=BEGIN trigger={} direct_internet={} proxy={}",
                trigger,
                config.allow_direct_internet,
                proxy
                    .as_ref()
                    .map(|endpoint| format!("{}:{}", endpoint.ip, endpoint.port))
                    .unwrap_or_else(|| "none".to_string())
            );
            match check_once(&config, proxy.as_ref(), false) {
                Ok(outcome) => {
                    let _ = write_u64_atomic(&state_path("last_success_unix"), unix_seconds());
                    println!("ARDA_AAP_KEY_UPDATE_CHECK=PASS outcome={outcome}");
                }
                Err(err) => {
                    eprintln!("ARDA_AAP_KEY_UPDATE_CHECK=FAIL error={err}");
                }
            }
        }

        let (lock, wake) = &*shared;
        let state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = wake
            .wait_timeout(state, Duration::from_secs(60))
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

fn check_once(
    config: &Config,
    proxy: Option<&ProxyEndpoint>,
    force: bool,
) -> Result<&'static str, Box<dyn std::error::Error>> {
    let mut direct_error: Option<String> = None;

    if config.allow_direct_internet {
        println!("ARDA_AAP_KEY_UPDATE_TRANSPORT=BEGIN mode=direct");
        match check_once_via(config, None, force) {
            Ok(outcome) => {
                println!("ARDA_AAP_KEY_UPDATE_TRANSPORT=PASS mode=direct outcome={outcome}");
                return Ok(outcome);
            }
            Err(err) => {
                let message = err.to_string();
                eprintln!("ARDA_AAP_KEY_UPDATE_TRANSPORT=FAIL mode=direct error={message}");
                direct_error = Some(message);
            }
        }
    }

    if let Some(proxy) = proxy {
        println!(
            "ARDA_AAP_KEY_UPDATE_TRANSPORT=BEGIN mode=proxy endpoint={}:{}",
            proxy.ip, proxy.port
        );
        match check_once_via(config, Some(proxy), force) {
            Ok(outcome) => {
                println!("ARDA_AAP_KEY_UPDATE_TRANSPORT=PASS mode=proxy outcome={outcome}");
                return Ok(outcome);
            }
            Err(proxy_error) => {
                if let Some(direct_error) = direct_error {
                    return Err(format!(
                        "direct update failed: {direct_error}; proxy update failed: {proxy_error}"
                    )
                    .into());
                }
                return Err(proxy_error);
            }
        }
    }

    Err(direct_error
        .unwrap_or_else(|| "no AAP key-update network path is available".to_string())
        .into())
}

fn check_once_via(
    config: &Config,
    proxy: Option<&ProxyEndpoint>,
    force: bool,
) -> Result<&'static str, Box<dyn std::error::Error>> {
    ensure_state_layout()?;
    let manifest_bytes = https_get(
        proxy,
        &config.control_url,
        config.download_timeout_seconds,
        config.max_manifest_bytes,
    )?;
    let manifest_text = std::str::from_utf8(&manifest_bytes)?;
    let manifest = parse_manifest(manifest_text)?;
    validate_sha256_hex(&manifest.sha)?;
    parse_https_url(&manifest.url)?;

    println!(
        "ARDA_AAP_KEY_UPDATE_MANIFEST=PASS sha={} url={}",
        manifest.sha, manifest.url
    );

    let active_sha = read_trimmed(&state_path("active.sha")).unwrap_or_default();
    let pending_sha = read_trimmed(&state_path("pending.sha")).unwrap_or_default();
    if !force && (manifest.sha == active_sha || manifest.sha == pending_sha) {
        return Ok("UNCHANGED");
    }

    let archive = https_get(
        proxy,
        &manifest.url,
        config.download_timeout_seconds,
        config.max_archive_bytes,
    )?;
    let actual_sha = hex_lower(&Sha256::digest(&archive));
    if actual_sha != manifest.sha {
        return Err(format!(
            "archive sha256 mismatch: expected {}, got {}",
            manifest.sha, actual_sha
        )
        .into());
    }
    println!(
        "ARDA_AAP_KEY_UPDATE_ARCHIVE_SHA=PASS sha={} bytes={}",
        actual_sha,
        archive.len()
    );

    let candidate = extract_candidate_keys(&archive)?;
    validate_cert_key_pair(&candidate.cert, &candidate.key)?;
    println!("ARDA_AAP_KEY_UPDATE_CERT_KEY=PASS");

    activate_candidate(&manifest.sha, &candidate)?;
    Ok("UPDATED_PENDING_AAP_AUTH")
}

fn ensure_state_layout() -> io::Result<()> {
    fs::create_dir_all(Path::new(DEFAULT_STATE_DIR).join("versions"))?;
    fs::create_dir_all(DEFAULT_CERT_DIR)?;
    fs::set_permissions(DEFAULT_STATE_DIR, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn activate_candidate(sha: &str, candidate: &CandidateKeys) -> io::Result<()> {
    ensure_state_layout()?;
    let versions = Path::new(DEFAULT_STATE_DIR).join("versions");
    let version_dir = versions.join(sha);
    if !version_dir.exists() {
        let temp_dir = versions.join(format!(".{sha}.new.{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir)?;
        write_secure_file(&temp_dir.join("md_cert.pem"), &candidate.cert)?;
        write_secure_file(&temp_dir.join("md_key.pem"), &candidate.key)?;
        fs::rename(&temp_dir, &version_dir)?;
    }

    match capture_current_target()? {
        Some(previous_target) => {
            write_text_atomic(&state_path("previous.target"), &previous_target)?;
            let previous_sha =
                read_trimmed(&state_path("active.sha")).unwrap_or_else(|| "legacy".into());
            write_text_atomic(&state_path("previous.sha"), &previous_sha)?;
        }
        None => {
            // Genuine first install: there is no working certificate pair to preserve.
            // Keep the candidate pending until AAP authentication confirms it, but do not
            // invent a rollback target.
            let _ = fs::remove_file(state_path("previous.target"));
            let _ = fs::remove_file(state_path("previous.sha"));
            println!("ARDA_AAP_KEY_UPDATE_FIRST_INSTALL=YES sha={sha}");
        }
    }

    switch_active_link(&version_dir)?;
    write_text_atomic(&state_path("pending.sha"), sha)?;
    write_u64_atomic(&state_path("pending.attempts"), 0)?;
    println!(
        "ARDA_AAP_KEY_UPDATE_ACTIVATE=PASS sha={} active_link={}",
        sha,
        active_link_path().display()
    );
    Ok(())
}

fn capture_current_target() -> io::Result<Option<String>> {
    let active = active_link_path();
    if let Ok(target) = fs::read_link(&active) {
        return Ok(Some(target.to_string_lossy().into_owned()));
    }

    let legacy_cert = Path::new(DEFAULT_CERT_DIR).join("md_cert.pem");
    let legacy_key = Path::new(DEFAULT_CERT_DIR).join("md_key.pem");
    let legacy_cert_exists = legacy_cert.is_file();
    let legacy_key_exists = legacy_key.is_file();

    match (legacy_cert_exists, legacy_key_exists) {
        (false, false) => return Ok(None),
        (true, true) => {}
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "existing AAP certificate pair is incomplete",
            ));
        }
    }

    let cert = fs::read(&legacy_cert)?;
    let key = fs::read(&legacy_key)?;
    validate_cert_key_pair(&cert, &key)?;
    let digest = hex_lower(&Sha256::digest([cert.as_slice(), key.as_slice()].concat()));
    let legacy_dir = Path::new(DEFAULT_STATE_DIR)
        .join("versions")
        .join(format!("legacy-{digest}"));
    if !legacy_dir.exists() {
        fs::create_dir_all(&legacy_dir)?;
        write_secure_file(&legacy_dir.join("md_cert.pem"), &cert)?;
        write_secure_file(&legacy_dir.join("md_key.pem"), &key)?;
    }
    Ok(Some(legacy_dir.to_string_lossy().into_owned()))
}

fn switch_active_link(target: &Path) -> io::Result<()> {
    let active = active_link_path();
    let temp = active.with_extension(format!("new.{}", std::process::id()));
    let _ = fs::remove_file(&temp);
    symlink(target, &temp)?;
    fs::rename(&temp, &active)?;
    Ok(())
}

fn active_link_path() -> PathBuf {
    Path::new(DEFAULT_CERT_DIR).join(ACTIVE_LINK_NAME)
}

pub fn resolve_keys_dir(base: &Path) -> PathBuf {
    let active = base.join(ACTIVE_LINK_NAME);
    if active.join("md_cert.pem").is_file() && active.join("md_key.pem").is_file() {
        active
    } else {
        base.to_path_buf()
    }
}

pub fn prepare_aap_attempt() {
    if let Err(err) = rollback_if_attempts_exhausted() {
        eprintln!("ARDA_AAP_KEY_UPDATE_ROLLBACK_PRECHECK=FAIL error={err}");
    }
    let pending = read_trimmed(&state_path("pending.sha")).unwrap_or_default();
    let attempt_sha = if pending.is_empty() {
        None
    } else {
        Some(pending.clone())
    };
    *aap_attempt_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = attempt_sha;
    if pending.is_empty() {
        return;
    }
    let attempts = read_u64_file(&state_path("pending.attempts")).unwrap_or(0) + 1;
    if let Err(err) = write_u64_atomic(&state_path("pending.attempts"), attempts) {
        eprintln!("ARDA_AAP_KEY_UPDATE_PENDING_ATTEMPT=FAIL error={err}");
    } else {
        println!(
            "ARDA_AAP_KEY_UPDATE_PENDING_ATTEMPT sha={} attempt={} max={}",
            pending, attempts, MAX_PENDING_ATTEMPTS
        );
    }
}

pub fn confirm_aap_authenticated() {
    let attempted = aap_attempt_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    let pending = read_trimmed(&state_path("pending.sha")).unwrap_or_default();
    if pending.is_empty() || attempted.as_deref() != Some(pending.as_str()) {
        if !pending.is_empty() {
            println!(
                "ARDA_AAP_KEY_UPDATE_CONFIRM=SKIP pending_sha={} attempted_sha={}",
                pending,
                attempted.as_deref().unwrap_or("none")
            );
        }
        return;
    }
    if let Err(err) = write_text_atomic(&state_path("active.sha"), &pending) {
        eprintln!("ARDA_AAP_KEY_UPDATE_CONFIRM=FAIL error={err}");
        return;
    }
    let _ = fs::remove_file(state_path("pending.sha"));
    let _ = fs::remove_file(state_path("pending.attempts"));
    let _ = fs::remove_file(state_path("previous.target"));
    let _ = fs::remove_file(state_path("previous.sha"));
    println!("ARDA_AAP_KEY_UPDATE_CONFIRM=PASS sha={pending}");
}

fn aap_attempt_slot() -> &'static Mutex<Option<String>> {
    AAP_ATTEMPT_PENDING_SHA.get_or_init(|| Mutex::new(None))
}

fn rollback_if_attempts_exhausted() -> io::Result<()> {
    let pending = read_trimmed(&state_path("pending.sha")).unwrap_or_default();
    if pending.is_empty() {
        return Ok(());
    }
    let attempts = read_u64_file(&state_path("pending.attempts")).unwrap_or(0);
    if attempts < MAX_PENDING_ATTEMPTS {
        return Ok(());
    }

    let previous_target = match read_trimmed(&state_path("previous.target")) {
        Some(previous_target) => previous_target,
        None => {
            // A first install has no prior working pair. There is nothing safe to roll
            // back to, so keep the validated candidate pending for later AAP auth.
            println!(
                "ARDA_AAP_KEY_UPDATE_ROLLBACK=SKIP rejected_sha={} attempts={} reason=first_install_no_previous_target",
                pending, attempts
            );
            return Ok(());
        }
    };

    let previous_path = PathBuf::from(previous_target);
    if !previous_path.join("md_cert.pem").is_file()
        || !previous_path.join("md_key.pem").is_file()
    {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("previous key directory is incomplete: {}", previous_path.display()),
        ));
    }
    switch_active_link(&previous_path)?;
    let previous_sha = read_trimmed(&state_path("previous.sha")).unwrap_or_else(|| "legacy".into());
    write_text_atomic(&state_path("active.sha"), &previous_sha)?;
    let _ = fs::remove_file(state_path("pending.sha"));
    let _ = fs::remove_file(state_path("pending.attempts"));
    let _ = fs::remove_file(state_path("previous.target"));
    let _ = fs::remove_file(state_path("previous.sha"));
    println!(
        "ARDA_AAP_KEY_UPDATE_ROLLBACK=PASS rejected_sha={} restored_sha={} attempts={}",
        pending, previous_sha, attempts
    );
    Ok(())
}

fn https_get(
    proxy: Option<&ProxyEndpoint>,
    url: &str,
    timeout_seconds: u64,
    max_bytes: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut current = url.to_string();
    for redirect in 0..=MAX_REDIRECTS {
        let parsed = parse_https_url(&current)?;
        let response = https_request_once(proxy, &parsed, timeout_seconds, max_bytes)?;
        if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            if redirect == MAX_REDIRECTS {
                return Err("too many HTTPS redirects".into());
            }
            let location = header_value(&response.headers, "location")
                .ok_or("redirect response has no Location header")?;
            current = resolve_redirect(&parsed, location)?;
            continue;
        }
        if !(200..300).contains(&response.status) {
            return Err(format!("HTTPS GET {current} returned HTTP {}", response.status).into());
        }
        return Ok(response.body);
    }
    Err("unreachable redirect loop".into())
}

fn https_request_once(
    proxy: Option<&ProxyEndpoint>,
    url: &HttpsUrl,
    timeout_seconds: u64,
    max_bytes: usize,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    let timeout = Duration::from_secs(timeout_seconds.max(1));
    let socket = if let Some(proxy) = proxy {
        let proxy_addr = SocketAddr::new(proxy.ip, proxy.port);
        let mut socket = TcpStream::connect_timeout(&proxy_addr, timeout)?;
        socket.set_read_timeout(Some(timeout))?;
        socket.set_write_timeout(Some(timeout))?;

        let authority = if url.port == 443 {
            format!("{}:443", url.host)
        } else {
            format!("{}:{}", url.host, url.port)
        };
        socket.write_all(
            format!(
                "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Connection: keep-alive\r\nUser-Agent: ARDA-AAP-Key-Updater/1\r\n\r\n"
            )
            .as_bytes(),
        )?;
        socket.flush()?;
        let connect_headers = read_headers_only(&mut socket, 32 * 1024)?;
        let connect_status = parse_status_code(&connect_headers)?;
        if connect_status != 200 {
            return Err(format!("proxy CONNECT {authority} returned HTTP {connect_status}").into());
        }
        socket
    } else {
        connect_direct(url, timeout)?
    };

    let roots = load_android_roots()?;
    let provider = Arc::new(aws_lc_rs::default_provider());
    let tls_config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&TLS13, &TLS12])?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server_name = ServerName::try_from(url.host.clone())?;
    let connection = ClientConnection::new(Arc::new(tls_config), server_name)?;
    let mut tls = StreamOwned::new(connection, socket);
    let host_header = if url.port == 443 {
        url.host.clone()
    } else {
        format!("{}:{}", url.host, url.port)
    };
    tls.write_all(
        format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: */*\r\nAccept-Encoding: identity\r\nUser-Agent: ARDA-AAP-Key-Updater/1\r\n\r\n",
            url.path, host_header
        )
        .as_bytes(),
    )?;
    tls.flush()?;

    let mut raw = Vec::new();
    let mut limited = tls.take((max_bytes + 128 * 1024 + 1) as u64);
    limited.read_to_end(&mut raw)?;
    if raw.len() > max_bytes + 128 * 1024 {
        return Err(format!("HTTP response exceeds configured limit for {}", url.host).into());
    }
    parse_http_response(&raw, max_bytes)
}

fn probe_direct_connectivity(url: &str, timeout: Duration) -> bool {
    let Ok(parsed) = parse_https_url(url) else {
        return false;
    };
    let Ok(addresses) = (parsed.host.as_str(), parsed.port).to_socket_addrs() else {
        return false;
    };
    for address in addresses {
        if TcpStream::connect_timeout(&address, timeout).is_ok() {
            return true;
        }
    }
    false
}

fn connect_direct(
    url: &HttpsUrl,
    timeout: Duration,
) -> Result<TcpStream, Box<dyn std::error::Error>> {
    let addresses = (url.host.as_str(), url.port)
        .to_socket_addrs()?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(format!("DNS returned no addresses for {}:{}", url.host, url.port).into());
    }

    let mut last_error: Option<io::Error> = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(socket) => {
                socket.set_read_timeout(Some(timeout))?;
                socket.set_write_timeout(Some(timeout))?;
                println!(
                    "ARDA_AAP_KEY_UPDATE_DIRECT_CONNECT=PASS host={} address={}",
                    url.host, address
                );
                return Ok(socket);
            }
            Err(err) => last_error = Some(err),
        }
    }

    Err(format!(
        "direct TCP connect failed for {}:{}: {}",
        url.host,
        url.port,
        last_error
            .map(|err| err.to_string())
            .unwrap_or_else(|| "unknown error".to_string())
    )
    .into())
}

fn load_android_roots() -> Result<RootCertStore, Box<dyn std::error::Error>> {
    let mut store = RootCertStore::empty();
    let mut certs: Vec<CertificateDer<'static>> = Vec::new();
    for item in fs::read_dir(ANDROID_CA_DIR)? {
        let path = item?.path();
        if !path.is_file() {
            continue;
        }
        let bytes = fs::read(&path)?;
        let mut reader = Cursor::new(bytes.as_slice());
        let parsed = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
        if parsed.is_empty() {
            certs.push(CertificateDer::from(bytes));
        } else {
            certs.extend(parsed);
        }
    }
    let (added, ignored) = store.add_parsable_certificates(certs);
    if added == 0 {
        return Err(format!("no usable Android CA certificates; ignored={ignored}").into());
    }
    Ok(store)
}

fn read_headers_only(stream: &mut TcpStream, max_bytes: usize) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut byte = [0u8; 1];
    while out.len() < max_bytes {
        let count = stream.read(&mut byte)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed before HTTP headers completed",
            ));
        }
        out.push(byte[0]);
        if out.ends_with(b"\r\n\r\n") {
            return Ok(out);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "HTTP headers exceed limit",
    ))
}

fn parse_http_response(
    raw: &[u8],
    max_body_bytes: usize,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    let header_end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("HTTP response has no complete header block")?;
    let header_bytes = &raw[..header_end + 4];
    let body_bytes = &raw[header_end + 4..];
    let status = parse_status_code(header_bytes)?;
    let header_text = std::str::from_utf8(header_bytes)?;
    let mut headers = Vec::new();
    for line in header_text.split("\r\n").skip(1) {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }

    let body = if header_value(&headers, "transfer-encoding")
        .map(|value| value.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false)
    {
        decode_chunked(body_bytes, max_body_bytes)?
    } else {
        let expected = header_value(&headers, "content-length")
            .and_then(|value| value.parse::<usize>().ok());
        if let Some(expected) = expected {
            if expected > max_body_bytes || body_bytes.len() < expected {
                return Err(format!(
                    "HTTP body length invalid: expected {expected}, received {}",
                    body_bytes.len()
                )
                .into());
            }
            body_bytes[..expected].to_vec()
        } else {
            if body_bytes.len() > max_body_bytes {
                return Err("HTTP body exceeds configured limit".into());
            }
            body_bytes.to_vec()
        }
    };

    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

fn parse_status_code(headers: &[u8]) -> Result<u16, Box<dyn std::error::Error>> {
    let line_end = headers
        .windows(2)
        .position(|window| window == b"\r\n")
        .unwrap_or(headers.len());
    let line = std::str::from_utf8(&headers[..line_end])?;
    let code = line
        .split_whitespace()
        .nth(1)
        .ok_or("HTTP status line has no status code")?
        .parse::<u16>()?;
    Ok(code)
}

fn decode_chunked(input: &[u8], max_bytes: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut pos = 0usize;
    let mut out = Vec::new();
    loop {
        let line_end = input[pos..]
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or("invalid chunked body: missing size terminator")?
            + pos;
        let size_text = std::str::from_utf8(&input[pos..line_end])?;
        let size_hex = size_text.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16)?;
        pos = line_end + 2;
        if size == 0 {
            return Ok(out);
        }
        if out.len().saturating_add(size) > max_bytes {
            return Err("chunked HTTP body exceeds configured limit".into());
        }
        if input.len() < pos + size + 2 || &input[pos + size..pos + size + 2] != b"\r\n" {
            return Err("invalid chunked body: truncated chunk".into());
        }
        out.extend_from_slice(&input[pos..pos + size]);
        pos += size + 2;
    }
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn parse_https_url(url: &str) -> Result<HttpsUrl, Box<dyn std::error::Error>> {
    let remainder = url.strip_prefix("https://").ok_or("URL must use https://")?;
    let (authority, path) = remainder
        .split_once('/')
        .map(|(authority, path)| (authority, format!("/{path}")))
        .unwrap_or((remainder, "/".to_string()));
    if authority.is_empty() || authority.contains('@') {
        return Err("HTTPS URL has invalid authority".into());
    }
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']').ok_or("invalid IPv6 URL authority")?;
        let host = authority[1..end].to_string();
        let port = authority[end + 1..]
            .strip_prefix(':')
            .map(str::parse)
            .transpose()?
            .unwrap_or(443);
        (host, port)
    } else if let Some((host, port_text)) = authority.rsplit_once(':') {
        if port_text.chars().all(|ch| ch.is_ascii_digit()) {
            (host.to_string(), port_text.parse()?)
        } else {
            (authority.to_string(), 443)
        }
    } else {
        (authority.to_string(), 443)
    };
    if host.is_empty() || port == 0 {
        return Err("HTTPS URL host/port is invalid".into());
    }
    Ok(HttpsUrl { host, port, path })
}

fn resolve_redirect(base: &HttpsUrl, location: &str) -> Result<String, Box<dyn std::error::Error>> {
    if location.starts_with("https://") {
        parse_https_url(location)?;
        return Ok(location.to_string());
    }
    if location.starts_with('/') {
        let authority = if base.port == 443 {
            base.host.clone()
        } else {
            format!("{}:{}", base.host, base.port)
        };
        return Ok(format!("https://{authority}{location}"));
    }
    Err("redirect Location must be absolute HTTPS or root-relative".into())
}

fn parse_manifest(text: &str) -> Result<Manifest, Box<dyn std::error::Error>> {
    let sha = json_string_field(text, "sha")?;
    let url = json_string_field(text, "url")?;
    Ok(Manifest { sha, url })
}

fn json_string_field(text: &str, key: &str) -> Result<String, Box<dyn std::error::Error>> {
    let needle = format!("\"{key}\"");
    let key_pos = text.find(&needle).ok_or_else(|| format!("manifest missing {key}"))?;
    let after_key = &text[key_pos + needle.len()..];
    let colon = after_key.find(':').ok_or_else(|| format!("manifest {key} has no colon"))?;
    let mut chars = after_key[colon + 1..].chars().peekable();
    while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
        chars.next();
    }
    if chars.next() != Some('"') {
        return Err(format!("manifest {key} must be a JSON string").into());
    }
    let mut out = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Ok(out),
            '\\' => {
                let escaped = chars.next().ok_or("truncated JSON escape")?;
                match escaped {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\u{0008}'),
                    'f' => out.push('\u{000c}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    _ => return Err("unsupported JSON escape in manifest".into()),
                }
            }
            other => out.push(other),
        }
    }
    Err(format!("manifest {key} string is unterminated").into())
}

fn validate_sha256_hex(value: &str) -> Result<(), Box<dyn std::error::Error>> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("manifest sha must be exactly 64 hexadecimal characters".into());
    }
    if value.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Err("manifest sha must use lowercase hexadecimal".into());
    }
    Ok(())
}

fn extract_candidate_keys(archive_bytes: &[u8]) -> Result<CandidateKeys, Box<dyn std::error::Error>> {
    let decoder = GzDecoder::new(Cursor::new(archive_bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut exact_cert: Option<Vec<u8>> = None;
    let mut exact_key: Option<Vec<u8>> = None;
    let mut discovered_certs: Vec<(String, Vec<u8>)> = Vec::new();
    let mut discovered_keys: Vec<(String, Vec<u8>)> = Vec::new();

    for item in archive.entries()? {
        let mut entry = item?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            continue;
        }
        if !entry_type.is_file() {
            return Err(format!("archive contains unsupported entry type at {:?}", entry.path()?).into());
        }
        let path = entry.path()?.into_owned();
        for component in path.components() {
            if !matches!(component, Component::Normal(_) | Component::CurDir) {
                return Err(format!("archive contains unsafe path {}", path.display()).into());
            }
        }
        let declared_size = entry.header().size()? as usize;
        if declared_size > MAX_CERT_BYTES.max(MAX_KEY_BYTES) {
            continue;
        }
        let bytes = read_entry_limited(&mut entry, MAX_CERT_BYTES.max(MAX_KEY_BYTES))?;
        let basename = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
        let display = path.to_string_lossy().into_owned();
        let cert_like = contains_ascii(&bytes, b"-----BEGIN CERTIFICATE-----");
        let key_like = contains_ascii(&bytes, b"-----BEGIN PRIVATE KEY-----")
            || contains_ascii(&bytes, b"-----BEGIN RSA PRIVATE KEY-----")
            || contains_ascii(&bytes, b"-----BEGIN EC PRIVATE KEY-----");

        if basename == "md_cert.pem" {
            if exact_cert.is_some() {
                return Err("archive contains multiple md_cert.pem files".into());
            }
            exact_cert = Some(bytes.clone());
        }
        if basename == "md_key.pem" {
            if exact_key.is_some() {
                return Err("archive contains multiple md_key.pem files".into());
            }
            exact_key = Some(bytes.clone());
        }
        if cert_like {
            discovered_certs.push((display.clone(), bytes.clone()));
        }
        if key_like {
            discovered_keys.push((display, bytes));
        }
    }

    let cert = match exact_cert {
        Some(value) => value,
        None if discovered_certs.len() == 1 => discovered_certs.remove(0).1,
        None => {
            let names = discovered_certs
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(",");
            return Err(format!(
                "archive needs exactly one certificate PEM when md_cert.pem is absent; found {} [{}]",
                discovered_certs.len(), names
            )
            .into());
        }
    };
    let key = match exact_key {
        Some(value) => value,
        None if discovered_keys.len() == 1 => discovered_keys.remove(0).1,
        None => {
            let names = discovered_keys
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(",");
            return Err(format!(
                "archive needs exactly one private-key PEM when md_key.pem is absent; found {} [{}]",
                discovered_keys.len(), names
            )
            .into());
        }
    };

    Ok(CandidateKeys { cert, key })
}

fn contains_ascii(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

fn read_entry_limited<R: Read>(reader: &mut R, max: usize) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    reader.take((max + 1) as u64).read_to_end(&mut out)?;
    if out.len() > max {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "archive entry exceeds size limit"));
    }
    Ok(out)
}

fn validate_cert_key_pair(cert_pem: &[u8], key_pem: &[u8]) -> io::Result<()> {
    let mut cert_reader = Cursor::new(cert_pem);
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(io_other)?;
    if certs.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "md_cert.pem contains no certificate"));
    }
    let key = load_private_key_bytes(key_pem)?;
    let provider = aws_lc_rs::default_provider();
    let signing_key = provider.key_provider.load_private_key(key).map_err(io_other)?;
    let certified = CertifiedKey::new(certs, signing_key);
    certified.keys_match().map_err(io_other)?;
    Ok(())
}

fn load_private_key_bytes(bytes: &[u8]) -> io::Result<PrivateKeyDer<'static>> {
    let mut reader = Cursor::new(bytes);
    if let Some(key) = rustls_pemfile::pkcs8_private_keys(&mut reader)
        .next()
        .transpose()
        .map_err(io_other)?
    {
        return Ok(PrivateKeyDer::Pkcs8(key));
    }
    let mut reader = Cursor::new(bytes);
    if let Some(key) = rustls_pemfile::rsa_private_keys(&mut reader)
        .next()
        .transpose()
        .map_err(io_other)?
    {
        return Ok(PrivateKeyDer::Pkcs1(key));
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "md_key.pem contains no supported private key",
    ))
}

fn state_path(name: &str) -> PathBuf {
    Path::new(DEFAULT_STATE_DIR).join(name)
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|value| value.trim().to_string())
}

fn read_u64_file(path: &Path) -> Option<u64> {
    read_trimmed(path)?.parse().ok()
}

fn write_u64_atomic(path: &Path, value: u64) -> io::Result<()> {
    write_text_atomic(path, &value.to_string())
}

fn write_text_atomic(path: &Path, value: &str) -> io::Result<()> {
    let temp = path.with_extension(format!("new.{}", std::process::id()));
    write_secure_file(&temp, format!("{value}\n").as_bytes())?;
    fs::rename(temp, path)?;
    Ok(())
}

fn write_secure_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true).mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn io_other<E: std::fmt::Display>(err: E) -> io::Error {
    io::Error::new(io::ErrorKind::Other, err.to_string())
}
