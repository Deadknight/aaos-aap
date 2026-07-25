//! ARDA Android Auto Projection (AAP) transport/session implementation.
//!
//! The full session can use either the historical TCP debug listener or the
//! AAOS device-side Android Open Accessory character device exposed by the
//! platform USB gadget (`/dev/usb_accessory`).

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::raw::{c_int, c_long};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, atomic::{AtomicBool, AtomicU8, Ordering}};
use std::thread;
use std::time::{Duration, Instant};

use crate::capture::screenrecord::preferred_codec_name;
use crate::h264::annexb::{AnnexBStreamParser, NalType};
use crate::input::{self, AppProcessAudioCapture, AppProcessInputInjector, PlatformInputBridgeInjector, TouchInjectAction, TouchInjector};
use crate::gnss_hal::{
    GnssHalPublishOutcome, PlatformGnssHalBridge, GNSS_HAL_SOCKET_NAME,
};
use crate::sensor::{ArdaSensorEvent, SensorPipeline, SensorPipelineConfig};

static SESSION_SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

/// True after this process receives a valid AAP MESSAGE_VERSION_REQUEST.
/// Used by the outer USB cleanup path so an idle receiver timeout does not
/// reset the USB gadget every minute while no HU is connected.
static AAP_PEER_SESSION_STARTED: AtomicBool = AtomicBool::new(false);

pub fn take_peer_session_started() -> bool {
    AAP_PEER_SESSION_STARTED.swap(false, Ordering::SeqCst)
}

static SESSION_LOG_LEVEL: AtomicU8 = AtomicU8::new(LOG_NORMAL);
const LOG_QUIET: u8 = 0;
const LOG_SUMMARY: u8 = 1;
const LOG_NORMAL: u8 = 2;
const LOG_VERBOSE: u8 = 3;

fn normalize_session_log_level(value: &str) -> &'static str {
    match value.to_ascii_lowercase().as_str() {
        "quiet" | "off" | "none" | "fast" => "quiet",
        "summary" | "minimal" | "min" => "summary",
        "verbose" | "trace" | "debug" | "wire" => "verbose",
        _ => "normal",
    }
}

fn session_log_level_value(value: &str) -> u8 {
    match normalize_session_log_level(value) {
        "quiet" => LOG_QUIET,
        "summary" => LOG_SUMMARY,
        "verbose" => LOG_VERBOSE,
        _ => LOG_NORMAL,
    }
}

fn set_session_log_level(value: &str) {
    SESSION_LOG_LEVEL.store(session_log_level_value(value), Ordering::SeqCst);
}

fn session_log_enabled(min_level: u8) -> bool {
    SESSION_LOG_LEVEL.load(Ordering::Relaxed) >= min_level
}

fn session_log(dump: &mut Option<Box<dyn Write>>, min_level: u8, line: &str) -> io::Result<()> {
    if session_log_enabled(min_level) {
        println!("{line}");
        log_line(dump, line)?;
    }
    Ok(())
}

fn session_log_format(dump: &mut Option<Box<dyn Write>>, min_level: u8, line: String) -> io::Result<()> {
    session_log(dump, min_level, &line)
}



const SIGHUP: c_int = 1;
const SIGINT: c_int = 2;
const SIGQUIT: c_int = 3;
const SIGUSR1: c_int = 10;
const SIGTERM: c_int = 15;
const USB_ACCESSORY_BULK_BUFFER_SIZE: usize = 16 * 1024;

type PthreadT = c_long;

unsafe extern "C" {
    fn signal(signum: c_int, handler: extern "C" fn(c_int)) -> usize;
    fn siginterrupt(signum: c_int, flag: c_int) -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    fn pthread_self() -> PthreadT;
    fn pthread_kill(thread: PthreadT, sig: c_int) -> c_int;
}

extern "C" fn handle_session_signal(_signum: c_int) {
    SESSION_SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

extern "C" fn handle_usb_reader_interrupt(_signum: c_int) {}

fn install_session_signal_handlers() {
    SESSION_SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    unsafe {
        let _ = signal(SIGHUP, handle_session_signal);
        let _ = signal(SIGINT, handle_session_signal);
        let _ = signal(SIGQUIT, handle_session_signal);
        let _ = signal(SIGTERM, handle_session_signal);
    }
}

fn install_usb_reader_interrupt_handler() -> io::Result<()> {
    unsafe {
        let _ = signal(SIGUSR1, handle_usb_reader_interrupt);
        if siginterrupt(SIGUSR1, 1) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn session_shutdown_requested() -> bool {
    SESSION_SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

fn request_session_shutdown() {
    SESSION_SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

pub mod proto;
mod fragment;
mod tls;

#[derive(Debug)]
enum UsbReadEvent {
    Data(Vec<u8>),
    Eof,
    Error(io::Error),
}

#[derive(Debug)]
struct UsbAccessoryStream {
    file: File,
    read_timeout: Option<Duration>,
    read_rx: mpsc::Receiver<UsbReadEvent>,
    read_pending: Vec<u8>,
    read_pending_offset: usize,
    reader_stop: Arc<AtomicBool>,
    reader_pthread: PthreadT,
    reader_thread: Option<thread::JoinHandle<()>>,
}

impl UsbAccessoryStream {
    fn open(path: &PathBuf) -> io::Result<Self> {
        install_usb_reader_interrupt_handler()?;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|err| {
                io::Error::new(
                    err.kind(),
                    format!("cannot open USB accessory {} read/write: {err}", path.display()),
                )
            })?;
        let mut reader_file = file.try_clone().map_err(|err| {
            io::Error::new(
                err.kind(),
                format!("cannot clone USB accessory fd {} for reader: {err}", path.display()),
            )
        })?;

        let (read_tx, read_rx) = mpsc::channel::<UsbReadEvent>();
        let (tid_tx, tid_rx) = mpsc::sync_channel::<PthreadT>(1);
        let reader_stop = Arc::new(AtomicBool::new(false));
        let reader_stop_thread = Arc::clone(&reader_stop);

        let reader_thread = thread::Builder::new()
            .name("arda-usb-rx".to_string())
            .spawn(move || {
                let tid = unsafe { pthread_self() };
                if tid_tx.send(tid).is_err() {
                    return;
                }

                let mut buf = vec![0u8; USB_ACCESSORY_BULK_BUFFER_SIZE];
                loop {
                    if reader_stop_thread.load(Ordering::SeqCst) {
                        break;
                    }
                    match reader_file.read(&mut buf) {
                        Ok(0) => {
                            let _ = read_tx.send(UsbReadEvent::Eof);
                            break;
                        }
                        Ok(size) => {
                            if read_tx.send(UsbReadEvent::Data(buf[..size].to_vec())).is_err() {
                                break;
                            }
                        }
                        Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                            if reader_stop_thread.load(Ordering::SeqCst) {
                                break;
                            }
                        }
                        Err(err) => {
                            let _ = read_tx.send(UsbReadEvent::Error(err));
                            break;
                        }
                    }
                }
            })?;

        let reader_pthread = tid_rx.recv().map_err(|_| {
            io::Error::new(
                io::ErrorKind::Other,
                "USB accessory reader thread exited before publishing pthread id",
            )
        })?;

        Ok(Self {
            file,
            read_timeout: None,
            read_rx,
            read_pending: Vec::new(),
            read_pending_offset: 0,
            reader_stop,
            reader_pthread,
            reader_thread: Some(reader_thread),
        })
    }

    fn set_read_timeout(&mut self, timeout: Option<Duration>) {
        self.read_timeout = timeout;
    }

    fn next_read_event(&self) -> io::Result<UsbReadEvent> {
        match self.read_timeout {
            Some(timeout) => match self.read_rx.recv_timeout(timeout) {
                Ok(event) => Ok(event),
                Err(mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "USB accessory read timed out",
                )),
                Err(mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "USB accessory reader thread disconnected",
                )),
            },
            None => self.read_rx.recv().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "USB accessory reader thread disconnected",
                )
            }),
        }
    }

    fn copy_pending(&mut self, buf: &mut [u8]) -> usize {
        let available = self.read_pending.len().saturating_sub(self.read_pending_offset);
        let size = available.min(buf.len());
        buf[..size].copy_from_slice(
            &self.read_pending[self.read_pending_offset..self.read_pending_offset + size],
        );
        self.read_pending_offset += size;
        if self.read_pending_offset == self.read_pending.len() {
            self.read_pending.clear();
            self.read_pending_offset = 0;
        }
        size
    }

    fn stop_reader(&mut self) {
        self.reader_stop.store(true, Ordering::SeqCst);
        unsafe {
            let _ = pthread_kill(self.reader_pthread, SIGUSR1);
        }
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for UsbAccessoryStream {
    fn drop(&mut self) {
        self.stop_reader();
    }
}

impl Read for UsbAccessoryStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.read_pending_offset < self.read_pending.len() {
            return Ok(self.copy_pending(buf));
        }

        match self.next_read_event()? {
            UsbReadEvent::Data(data) => {
                self.read_pending = data;
                self.read_pending_offset = 0;
                Ok(self.copy_pending(buf))
            }
            UsbReadEvent::Eof => Ok(0),
            UsbReadEvent::Error(err) => Err(err),
        }
    }
}

impl Write for UsbAccessoryStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[derive(Debug)]
enum SessionStream {
    Tcp(TcpStream),
    Usb(UsbAccessoryStream),
}

impl SessionStream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.set_read_timeout(timeout),
            Self::Usb(stream) => {
                stream.set_read_timeout(timeout);
                Ok(())
            }
        }
    }

    fn set_write_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.set_write_timeout(timeout),
            // /dev/usb_accessory is a character device and has no socket timeout
            // option. A blocking reader thread feeds timed reads to the session loop; accessory writes stay blocking.
            Self::Usb(_) => {
                let _ = timeout;
                Ok(())
            }
        }
    }
}

impl Read for SessionStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buf),
            Self::Usb(stream) => stream.read(buf),
        }
    }
}

impl Write for SessionStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buf),
            Self::Usb(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            Self::Usb(stream) => stream.flush(),
        }
    }
}

fn open_session_stream(config: &TlsHandshakeProbeConfig) -> io::Result<(SessionStream, String, String)> {
    match config.transport.as_str() {
        "tcp" => {
            let listener = TcpListener::bind(&config.addr)?;
            println!("listening on {}", listener.local_addr()?);
            println!("waiting for one client...");
            let (stream, peer) = listener.accept()?;
            let local = listener.local_addr()?.to_string();
            Ok((SessionStream::Tcp(stream), local, peer.to_string()))
        }
        "usb" | "usb-accessory" | "accessory" | "hu-usb" => {
            println!("opening HU-side USB accessory transport: {}", config.usb_path.display());
            println!("waiting for AAP bytes from USB host/head unit...");
            let stream = UsbAccessoryStream::open(&config.usb_path)?;
            Ok((
                SessionStream::Usb(stream),
                format!("usb:{}", config.usb_path.display()),
                "usb-host".to_string(),
            ))
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported AAP transport {other}; use tcp or usb-accessory"),
        )),
    }
}

#[derive(Debug, Clone)]
pub struct TcpProbeConfig {
    pub addr: String,
    pub dump_path: Option<PathBuf>,
    pub timeout_ms: u64,
    pub max_bytes: usize,
}

impl Default for TcpProbeConfig {
    fn default() -> Self {
        Self {
            addr: "0.0.0.0:5277".to_string(),
            dump_path: Some(PathBuf::from("/data/local/tmp/arda-aap-tcp.log")),
            timeout_ms: 60_000,
            max_bytes: 1024 * 1024,
        }
    }
}

pub fn tcp_probe(config: TcpProbeConfig) -> io::Result<()> {
    println!("arda aap tcp probe");
    println!("addr={}", config.addr);
    println!("timeout_ms={}", config.timeout_ms);
    println!("max_bytes={}", config.max_bytes);
    if let Some(path) = &config.dump_path {
        println!("dump={}", path.display());
    } else {
        println!("dump=none");
    }
    println!();

    let listener = TcpListener::bind(&config.addr)?;
    println!("listening on {}", listener.local_addr()?);
    println!("waiting for one client...");

    let (mut stream, peer) = listener.accept()?;
    println!("client connected: {peer}");
    stream.set_read_timeout(Some(Duration::from_millis(config.timeout_ms)))?;

    let mut dump: Option<Box<dyn Write>> = match &config.dump_path {
        Some(path) => Some(Box::new(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)?,
        )),
        None => None,
    };

    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "arda aap tcp probe")?;
        writeln!(dump, "local={}", listener.local_addr()?)?;
        writeln!(dump, "peer={peer}")?;
        writeln!(dump)?;
        dump.flush()?;
    }

    read_and_dump(&mut stream, peer, &mut dump, &config)?;
    println!("done");
    Ok(())
}

fn read_and_dump(
    stream: &mut TcpStream,
    peer: SocketAddr,
    dump: &mut Option<Box<dyn Write>>,
    config: &TcpProbeConfig,
) -> io::Result<()> {
    let mut buf = [0u8; 4096];
    let mut total = 0usize;
    let started = Instant::now();

    loop {
        if total >= config.max_bytes {
            println!("max_bytes reached: {total}");
            if let Some(dump) = dump.as_mut() {
                writeln!(dump, "max_bytes reached: {total}")?;
            }
            break;
        }

        let to_read = buf.len().min(config.max_bytes - total);
        let read = match stream.read(&mut buf[..to_read]) {
            Ok(0) => {
                println!("client disconnected after {total} bytes");
                if let Some(dump) = dump.as_mut() {
                    writeln!(dump, "client disconnected after {total} bytes")?;
                }
                break;
            }
            Ok(n) => n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted && session_shutdown_requested() => {
                println!("read interrupted by shutdown request; stopping AAP session");
                log_line(dump, "read interrupted by shutdown request; stopping AAP session")?;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.kind() == io::ErrorKind::TimedOut =>
            {
                println!("read timeout after {total} bytes");
                if let Some(dump) = dump.as_mut() {
                    writeln!(dump, "read timeout after {total} bytes")?;
                }
                break;
            }
            Err(err) => return Err(err),
        };

        println!("recv peer={peer} offset={total} len={read}");
        print_hex_dump(total, &buf[..read], &mut io::stdout())?;
        io::stdout().flush()?;

        if let Some(dump) = dump.as_mut() {
            writeln!(dump, "recv peer={peer} offset={total} len={read}")?;
            print_hex_dump(total, &buf[..read], dump)?;
            dump.flush()?;
        }

        total += read;
    }

    let elapsed = started.elapsed();
    println!("summary: bytes={total} elapsed_ms={}", elapsed.as_millis());
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "summary: bytes={total} elapsed_ms={}", elapsed.as_millis())?;
        dump.flush()?;
    }
    Ok(())
}

fn print_hex_dump(offset_base: usize, data: &[u8], out: &mut dyn Write) -> io::Result<()> {
    for (row_index, chunk) in data.chunks(16).enumerate() {
        let offset = offset_base + row_index * 16;
        write!(out, "{offset:08x}  ")?;
        for i in 0..16 {
            if let Some(byte) = chunk.get(i) {
                write!(out, "{byte:02x} ")?;
            } else {
                write!(out, "   ")?;
            }
            if i == 7 {
                write!(out, " ")?;
            }
        }
        write!(out, " |")?;
        for byte in chunk {
            let ch = if byte.is_ascii_graphic() || *byte == b' ' {
                *byte as char
            } else {
                '.'
            };
            write!(out, "{ch}")?;
        }
        writeln!(out, "|")?;
    }
    Ok(())
}


// ---------------------------------------------------------------------------
// AAP frame probe
// ---------------------------------------------------------------------------

const HEADER_LENGTH: usize = 4;
const FRAME_TYPE_FIRST: u8 = 1 << 0;
const FRAME_TYPE_LAST: u8 = 1 << 1;
const FRAME_TYPE_MASK: u8 = FRAME_TYPE_FIRST | FRAME_TYPE_LAST;
const FLAG_CONTROL: u8 = 1 << 2;
const FLAG_ENCRYPTED: u8 = 1 << 3;

#[derive(Debug, Clone)]
pub struct FrameProbeConfig {
    pub addr: String,
    pub dump_path: Option<PathBuf>,
    pub timeout_ms: u64,
    pub max_bytes: usize,
    pub max_frames: usize,
}

impl Default for FrameProbeConfig {
    fn default() -> Self {
        Self {
            addr: "0.0.0.0:5277".to_string(),
            dump_path: Some(PathBuf::from("/data/local/tmp/arda-aap-frames.log")),
            timeout_ms: 60_000,
            max_bytes: 4 * 1024 * 1024,
            max_frames: 256,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AapFrame {
    pub channel: u8,
    pub flags: u8,
    pub final_length: Option<u32>,
    pub payload: Vec<u8>,
}

pub fn frame_probe(config: FrameProbeConfig) -> io::Result<()> {
    println!("arda aap frame probe");
    println!("addr={}", config.addr);
    println!("timeout_ms={}", config.timeout_ms);
    println!("max_bytes={}", config.max_bytes);
    println!("max_frames={}", config.max_frames);
    if let Some(path) = &config.dump_path {
        println!("dump={}", path.display());
    } else {
        println!("dump=none");
    }
    println!();

    let listener = TcpListener::bind(&config.addr)?;
    println!("listening on {}", listener.local_addr()?);
    println!("waiting for one client...");

    let (mut stream, peer) = listener.accept()?;
    println!("client connected: {peer}");
    stream.set_read_timeout(Some(Duration::from_millis(config.timeout_ms)))?;

    let mut dump: Option<Box<dyn Write>> = match &config.dump_path {
        Some(path) => Some(Box::new(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)?,
        )),
        None => None,
    };

    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "arda aap frame probe")?;
        writeln!(dump, "local={}", listener.local_addr()?)?;
        writeln!(dump, "peer={peer}")?;
        writeln!(dump)?;
        dump.flush()?;
    }

    let started = Instant::now();
    let mut reader = FrameReader::new(config.max_bytes);
    let mut frames = 0usize;
    let mut bytes = 0usize;

    loop {
        if frames >= config.max_frames {
            log_line(&mut dump, &format!("max_frames reached: {frames}"))?;
            println!("max_frames reached: {frames}");
            break;
        }

        match reader.read_frame(&mut stream) {
            Ok(Some(frame)) => {
                bytes += HEADER_LENGTH + frame.payload.len() + if frame.final_length.is_some() { 4 } else { 0 };
                frames += 1;
                describe_frame(frames, bytes, peer, &frame, &mut io::stdout())?;
                io::stdout().flush()?;
                if let Some(dump) = dump.as_mut() {
                    describe_frame(frames, bytes, peer, &frame, dump)?;
                    dump.flush()?;
                }
            }
            Ok(None) => {
                println!("client disconnected after frames={frames} bytes={bytes}");
                log_line(&mut dump, &format!("client disconnected after frames={frames} bytes={bytes}"))?;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted && session_shutdown_requested() => {
                println!("read interrupted by shutdown request; stopping AAP session");
                log_line(&mut dump, "read interrupted by shutdown request; stopping AAP session")?;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.kind() == io::ErrorKind::TimedOut =>
            {
                println!("read timeout after frames={frames} bytes={bytes}");
                log_line(&mut dump, &format!("read timeout after frames={frames} bytes={bytes}"))?;
                break;
            }
            Err(err) => return Err(err),
        }
    }

    let elapsed = started.elapsed();
    println!("summary: frames={frames} bytes={bytes} elapsed_ms={}", elapsed.as_millis());
    log_line(&mut dump, &format!("summary: frames={frames} bytes={bytes} elapsed_ms={}", elapsed.as_millis()))?;
    Ok(())
}

struct FrameReader {
    buf: Vec<u8>,
    max_bytes: usize,
}

impl FrameReader {
    fn new(max_bytes: usize) -> Self {
        Self { buf: Vec::new(), max_bytes }
    }

    fn read_frame<R: Read + ?Sized>(&mut self, stream: &mut R) -> io::Result<Option<AapFrame>> {
        loop {
            if let Some(frame) = self.try_parse_frame()? {
                return Ok(Some(frame));
            }

            let mut tmp = [0u8; 4096];
            let n = stream.read(&mut tmp)?;
            if n == 0 {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!("peer closed with {} buffered bytes", self.buf.len()),
                ));
            }
            if self.buf.len() + n > self.max_bytes {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("frame probe buffer exceeds max_bytes={}", self.max_bytes),
                ));
            }
            self.buf.extend_from_slice(&tmp[..n]);
        }
    }

    fn try_parse_frame(&mut self) -> io::Result<Option<AapFrame>> {
        if self.buf.len() < HEADER_LENGTH {
            return Ok(None);
        }

        let channel = self.buf[0];
        let flags = self.buf[1];
        let payload_len = u16::from_be_bytes([self.buf[2], self.buf[3]]) as usize;
        let mut header_len = HEADER_LENGTH;
        let mut final_length = None;

        if (flags & FRAME_TYPE_MASK) == FRAME_TYPE_FIRST {
            if self.buf.len() < 8 {
                return Ok(None);
            }
            final_length = Some(u32::from_be_bytes([
                self.buf[4], self.buf[5], self.buf[6], self.buf[7],
            ]));
            header_len = 8;
        }

        let frame_len = header_len + payload_len;
        if self.buf.len() < frame_len {
            return Ok(None);
        }

        let payload = self.buf[header_len..frame_len].to_vec();
        self.buf.drain(..frame_len);
        Ok(Some(AapFrame { channel, flags, final_length, payload }))
    }
}

fn log_line(dump: &mut Option<Box<dyn Write>>, line: &str) -> io::Result<()> {
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "{line}")?;
        dump.flush()?;
    }
    Ok(())
}

fn describe_frame(
    index: usize,
    total_bytes: usize,
    peer: impl std::fmt::Display,
    frame: &AapFrame,
    out: &mut dyn Write,
) -> io::Result<()> {
    let msg = control_hint(frame);
    writeln!(
        out,
        "frame #{index} peer={peer} total_bytes={total_bytes} ch=0x{ch:02x} flags=0x{flags:02x} type={frame_type} enc={enc} ctrl={ctrl} payload_len={payload_len} final_len={final_len:?}{msg}",
        ch = frame.channel,
        flags = frame.flags,
        frame_type = frame_type_name(frame.flags),
        enc = (frame.flags & FLAG_ENCRYPTED) != 0,
        ctrl = (frame.flags & FLAG_CONTROL) != 0,
        payload_len = frame.payload.len(),
        final_len = frame.final_length,
        msg = msg.map(|m| format!(" msg={m}")).unwrap_or_default(),
    )?;

    let dump_len = frame.payload.len().min(128);
    if dump_len > 0 {
        print_hex_dump(0, &frame.payload[..dump_len], out)?;
        if frame.payload.len() > dump_len {
            writeln!(out, "... payload truncated for display: {} / {} bytes", dump_len, frame.payload.len())?;
        }
    }
    Ok(())
}

fn frame_type_name(flags: u8) -> &'static str {
    match flags & FRAME_TYPE_MASK {
        0 => "middle",
        FRAME_TYPE_FIRST => "first",
        FRAME_TYPE_LAST => "last",
        FRAME_TYPE_MASK => "single",
        _ => "unknown",
    }
}

fn control_hint(frame: &AapFrame) -> Option<String> {
    if frame.channel != 0 || frame.payload.len() < 2 {
        return None;
    }
    let id = u16::from_be_bytes([frame.payload[0], frame.payload[1]]);
    let body = &frame.payload[2..];
    let name = proto::control_message_name(id);

    // VERSION_REQUEST/RESPONSE and ENCAPSULATED_SSL are fixed-size/control
    // wrappers, not protobuf message bodies.
    if id == 0x0001 && frame.payload.len() >= 6 {
        let major = u16::from_be_bytes([frame.payload[2], frame.payload[3]]);
        let minor = u16::from_be_bytes([frame.payload[4], frame.payload[5]]);
        return Some(format!("{name} {major}.{minor}"));
    }
    if id == 0x0002 && frame.payload.len() >= 8 {
        let major = u16::from_be_bytes([frame.payload[2], frame.payload[3]]);
        let minor = u16::from_be_bytes([frame.payload[4], frame.payload[5]]);
        let status = u16::from_be_bytes([frame.payload[6], frame.payload[7]]);
        return Some(format!("{name} {major}.{minor} status={status}"));
    }
    if id == 0x0003 {
        return Some(format!("{name} tls_bytes={}", body.len()));
    }

    if let Some(decoded) = proto::describe_control_body(id, body) {
        return Some(decoded);
    }
    Some(format!("{name} id=0x{id:04x} proto_body_len={}", body.len()))
}


// ---------------------------------------------------------------------------
// AAP plaintext version session
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct VersionSessionConfig {
    pub addr: String,
    pub dump_path: Option<PathBuf>,
    pub timeout_ms: u64,
    pub max_bytes: usize,
    pub response_major: u16,
    pub response_minor: u16,
    pub response_status: u16,
}

impl Default for VersionSessionConfig {
    fn default() -> Self {
        Self {
            addr: "0.0.0.0:5277".to_string(),
            dump_path: Some(PathBuf::from("/data/local/tmp/arda-aap-version.log")),
            timeout_ms: 60_000,
            max_bytes: 1024 * 1024,
            response_major: 1,
            response_minor: 7,
            response_status: 0,
        }
    }
}

pub fn version_session(config: VersionSessionConfig) -> io::Result<()> {
    println!("arda aap version session");
    println!("addr={}", config.addr);
    println!("timeout_ms={}", config.timeout_ms);
    println!("response={}.{} status=0x{:04x}", config.response_major, config.response_minor, config.response_status);
    if let Some(path) = &config.dump_path {
        println!("dump={}", path.display());
    } else {
        println!("dump=none");
    }
    println!();

    let listener = TcpListener::bind(&config.addr)?;
    println!("listening on {}", listener.local_addr()?);
    println!("waiting for one client...");

    let (mut stream, peer) = listener.accept()?;
    println!("client connected: {peer}");
    stream.set_read_timeout(Some(Duration::from_millis(config.timeout_ms)))?;
    stream.set_write_timeout(Some(Duration::from_millis(config.timeout_ms)))?;

    let mut dump: Option<Box<dyn Write>> = match &config.dump_path {
        Some(path) => Some(Box::new(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)?,
        )),
        None => None,
    };

    log_line(&mut dump, "arda aap version session")?;
    log_line(&mut dump, &format!("local={}", listener.local_addr()?))?;
    log_line(&mut dump, &format!("peer={peer}"))?;
    log_line(&mut dump, &format!("response={}.{} status=0x{:04x}", config.response_major, config.response_minor, config.response_status))?;
    log_line(&mut dump, "")?;

    let started = Instant::now();
    let mut reader = FrameReader::new(config.max_bytes);
    let frame = match reader.read_frame(&mut stream) {
        Ok(Some(frame)) => frame,
        Ok(None) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "peer closed before version request")),
        Err(err) => return Err(err),
    };

    println!("rx version candidate:");
    describe_frame(1, HEADER_LENGTH + frame.payload.len() + if frame.final_length.is_some() { 4 } else { 0 }, peer, &frame, &mut io::stdout())?;
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "rx version candidate:")?;
        describe_frame(1, HEADER_LENGTH + frame.payload.len() + if frame.final_length.is_some() { 4 } else { 0 }, peer, &frame, dump)?;
        dump.flush()?;
    }

    let Some((req_major, req_minor)) = parse_version_request(&frame) else {
        log_line(&mut dump, "not a valid MESSAGE_VERSION_REQUEST; closing")?;
        return Err(io::Error::new(io::ErrorKind::InvalidData, "expected MESSAGE_VERSION_REQUEST on control channel"));
    };

    println!("version request: {}.{}", req_major, req_minor);
    log_line(&mut dump, &format!("version request: {}.{}", req_major, req_minor))?;

    let response = build_version_response(config.response_major, config.response_minor, config.response_status);
    let out_frame = AapFrame {
        channel: 0,
        flags: FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
        final_length: None,
        payload: response,
    };
    write_frame(&mut stream, &out_frame)?;
    stream.flush()?;

    let wire_len = HEADER_LENGTH + out_frame.payload.len();
    println!("tx version response:");
    describe_frame(1, wire_len, peer, &out_frame, &mut io::stdout())?;
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "tx version response:")?;
        describe_frame(1, wire_len, peer, &out_frame, dump)?;
        dump.flush()?;
    }

    let elapsed = started.elapsed();
    println!("summary: version_response_sent elapsed_ms={}", elapsed.as_millis());
    log_line(&mut dump, &format!("summary: version_response_sent elapsed_ms={}", elapsed.as_millis()))?;
    Ok(())
}

fn parse_version_request(frame: &AapFrame) -> Option<(u16, u16)> {
    if frame.channel != 0 || frame.payload.len() != 6 {
        return None;
    }
    let id = u16::from_be_bytes([frame.payload[0], frame.payload[1]]);
    if id != 0x0001 {
        return None;
    }
    let major = u16::from_be_bytes([frame.payload[2], frame.payload[3]]);
    let minor = u16::from_be_bytes([frame.payload[4], frame.payload[5]]);
    Some((major, minor))
}

fn build_version_response(major: u16, minor: u16, status: u16) -> Vec<u8> {
    let mut payload = Vec::with_capacity(8);
    payload.extend_from_slice(&0x0002u16.to_be_bytes());
    payload.extend_from_slice(&major.to_be_bytes());
    payload.extend_from_slice(&minor.to_be_bytes());
    payload.extend_from_slice(&status.to_be_bytes());
    payload
}

fn write_frame<W: Write + ?Sized>(stream: &mut W, frame: &AapFrame) -> io::Result<()> {
    if frame.payload.len() > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("payload too large for single AAP frame: {}", frame.payload.len()),
        ));
    }
    let payload_len = frame.payload.len() as u16;
    let mut header = [0u8; 8];
    header[0] = frame.channel;
    header[1] = frame.flags;
    header[2..4].copy_from_slice(&payload_len.to_be_bytes());
    if let Some(final_len) = frame.final_length {
        header[4..8].copy_from_slice(&final_len.to_be_bytes());
        stream.write_all(&header[..8])?;
    } else {
        stream.write_all(&header[..4])?;
    }
    stream.write_all(&frame.payload)?;
    Ok(())
}


// ---------------------------------------------------------------------------
// AAP plaintext version + SSL frame probe
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SslFrameProbeConfig {
    pub addr: String,
    pub dump_path: Option<PathBuf>,
    pub timeout_ms: u64,
    pub max_bytes: usize,
    pub max_frames: usize,
    pub response_major: u16,
    pub response_minor: u16,
    pub response_status: u16,
}

impl Default for SslFrameProbeConfig {
    fn default() -> Self {
        Self {
            addr: "0.0.0.0:5277".to_string(),
            dump_path: Some(PathBuf::from("/data/local/tmp/arda-aap-ssl-frame.log")),
            timeout_ms: 60_000,
            max_bytes: 4 * 1024 * 1024,
            max_frames: 8,
            response_major: 1,
            response_minor: 7,
            response_status: 0,
        }
    }
}

pub fn ssl_frame_probe(config: SslFrameProbeConfig) -> io::Result<()> {
    println!("arda aap ssl frame probe");
    println!("addr={}", config.addr);
    println!("timeout_ms={}", config.timeout_ms);
    println!("max_bytes={}", config.max_bytes);
    println!("max_frames={}", config.max_frames);
    println!("response={}.{} status=0x{:04x}", config.response_major, config.response_minor, config.response_status);
    if let Some(path) = &config.dump_path {
        println!("dump={}", path.display());
    } else {
        println!("dump=none");
    }
    println!();

    let listener = TcpListener::bind(&config.addr)?;
    println!("listening on {}", listener.local_addr()?);
    println!("waiting for one client...");

    let (mut stream, peer) = listener.accept()?;
    println!("client connected: {peer}");
    stream.set_read_timeout(Some(Duration::from_millis(config.timeout_ms)))?;
    stream.set_write_timeout(Some(Duration::from_millis(config.timeout_ms)))?;

    let mut dump: Option<Box<dyn Write>> = match &config.dump_path {
        Some(path) => Some(Box::new(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)?,
        )),
        None => None,
    };

    log_line(&mut dump, "arda aap ssl frame probe")?;
    log_line(&mut dump, &format!("local={}", listener.local_addr()?))?;
    log_line(&mut dump, &format!("peer={peer}"))?;
    log_line(&mut dump, &format!("response={}.{} status=0x{:04x}", config.response_major, config.response_minor, config.response_status))?;
    log_line(&mut dump, "note=TLS cert verification is intentionally not part of this probe; it only captures MESSAGE_ENCAPSULATED_SSL bytes")?;
    log_line(&mut dump, "")?;

    let started = Instant::now();
    let mut reader = FrameReader::new(config.max_bytes);

    let version = match reader.read_frame(&mut stream) {
        Ok(Some(frame)) => frame,
        Ok(None) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "peer closed before version request")),
        Err(err) => return Err(err),
    };

    println!("rx version candidate:");
    describe_frame(1, HEADER_LENGTH + version.payload.len() + if version.final_length.is_some() { 4 } else { 0 }, peer, &version, &mut io::stdout())?;
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "rx version candidate:")?;
        describe_frame(1, HEADER_LENGTH + version.payload.len() + if version.final_length.is_some() { 4 } else { 0 }, peer, &version, dump)?;
        dump.flush()?;
    }

    let Some((req_major, req_minor)) = parse_version_request(&version) else {
        log_line(&mut dump, "not a valid MESSAGE_VERSION_REQUEST; closing")?;
        return Err(io::Error::new(io::ErrorKind::InvalidData, "expected MESSAGE_VERSION_REQUEST on control channel"));
    };

    println!("version request: {}.{}", req_major, req_minor);
    log_line(&mut dump, &format!("version request: {}.{}", req_major, req_minor))?;

    let response_payload = build_version_response(config.response_major, config.response_minor, config.response_status);
    let response_frame = AapFrame {
        channel: 0,
        flags: FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
        final_length: None,
        payload: response_payload,
    };
    write_frame(&mut stream, &response_frame)?;
    stream.flush()?;

    println!("tx version response:");
    describe_frame(1, HEADER_LENGTH + response_frame.payload.len(), peer, &response_frame, &mut io::stdout())?;
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "tx version response:")?;
        describe_frame(1, HEADER_LENGTH + response_frame.payload.len(), peer, &response_frame, dump)?;
        dump.flush()?;
    }

    println!("waiting for MESSAGE_ENCAPSULATED_SSL frames...");
    log_line(&mut dump, "waiting for MESSAGE_ENCAPSULATED_SSL frames...")?;

    let mut frames = 1usize;
    let mut bytes = HEADER_LENGTH + version.payload.len() + HEADER_LENGTH + response_frame.payload.len();
    let mut ssl_frames = 0usize;
    while frames < config.max_frames {
        if session_shutdown_requested() {
            println!("shutdown requested; stopping AAP session");
            log_line(&mut dump, "shutdown requested; stopping AAP session")?;
            break;
        }

        let frame = match reader.read_frame(&mut stream) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                println!("client disconnected after frames={frames} ssl_frames={ssl_frames} bytes={bytes}");
                log_line(&mut dump, &format!("client disconnected after frames={frames} ssl_frames={ssl_frames} bytes={bytes}"))?;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted && session_shutdown_requested() => {
                println!("read interrupted by shutdown request; stopping AAP session");
                log_line(&mut dump, "read interrupted by shutdown request; stopping AAP session")?;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.kind() == io::ErrorKind::TimedOut =>
            {
                println!("read timeout after frames={frames} ssl_frames={ssl_frames} bytes={bytes}");
                log_line(&mut dump, &format!("read timeout after frames={frames} ssl_frames={ssl_frames} bytes={bytes}"))?;
                break;
            }
            Err(err) => return Err(err),
        };

        frames += 1;
        bytes += HEADER_LENGTH + frame.payload.len() + if frame.final_length.is_some() { 4 } else { 0 };
        println!("rx post-version frame:");
        describe_frame(frames, bytes, peer, &frame, &mut io::stdout())?;
        if let Some(dump) = dump.as_mut() {
            writeln!(dump, "rx post-version frame:")?;
            describe_frame(frames, bytes, peer, &frame, dump)?;
        }

        if let Some(tls_bytes) = ssl_payload_bytes(&frame) {
            ssl_frames += 1;
            let hint = tls_record_hint(tls_bytes);
            println!("ssl frame #{ssl_frames}: tls_bytes={} {hint}", tls_bytes.len());
            if let Some(dump) = dump.as_mut() {
                writeln!(dump, "ssl frame #{ssl_frames}: tls_bytes={} {hint}", tls_bytes.len())?;
                let dump_len = tls_bytes.len().min(256);
                if dump_len > 0 {
                    print_hex_dump(0, &tls_bytes[..dump_len], dump)?;
                    if tls_bytes.len() > dump_len {
                        writeln!(dump, "... tls bytes truncated for display: {} / {} bytes", dump_len, tls_bytes.len())?;
                    }
                }
                dump.flush()?;
            }
            // This is a probe: one SSL frame is enough to prove post-version encapsulation.
            break;
        }
    }

    let elapsed = started.elapsed();
    println!("summary: frames={frames} ssl_frames={ssl_frames} bytes={bytes} elapsed_ms={}", elapsed.as_millis());
    log_line(&mut dump, &format!("summary: frames={frames} ssl_frames={ssl_frames} bytes={bytes} elapsed_ms={}", elapsed.as_millis()))?;
    Ok(())
}

fn ssl_payload_bytes(frame: &AapFrame) -> Option<&[u8]> {
    if frame.channel != 0 || frame.payload.len() < 2 {
        return None;
    }
    let id = u16::from_be_bytes([frame.payload[0], frame.payload[1]]);
    if id != 0x0003 {
        return None;
    }
    Some(&frame.payload[2..])
}

fn tls_record_hint(data: &[u8]) -> String {
    if data.len() < 5 {
        return "tls_hint=too_short".to_string();
    }
    let content_type = data[0];
    let major = data[1];
    let minor = data[2];
    let record_len = u16::from_be_bytes([data[3], data[4]]) as usize;
    let content_name = match content_type {
        0x14 => "change_cipher_spec",
        0x15 => "alert",
        0x16 => "handshake",
        0x17 => "application_data",
        _ => "unknown",
    };
    let mut s = format!(
        "tls_record type=0x{content_type:02x}({content_name}) version={major:02x}.{minor:02x} record_len={record_len}"
    );
    if content_type == 0x16 && data.len() >= 9 {
        let hs_type = data[5];
        let hs_len = ((data[6] as usize) << 16) | ((data[7] as usize) << 8) | (data[8] as usize);
        let hs_name = match hs_type {
            0x01 => "client_hello",
            0x02 => "server_hello",
            0x0b => "certificate",
            0x0c => "server_key_exchange",
            0x0d => "certificate_request",
            0x0e => "server_hello_done",
            0x10 => "client_key_exchange",
            0x14 => "finished",
            _ => "unknown_handshake",
        };
        s.push_str(&format!(" handshake=0x{hs_type:02x}({hs_name}) handshake_len={hs_len}"));
    }
    if data.len() < 5 + record_len {
        s.push_str(&format!(" note=incomplete_record_have={} need={}", data.len(), 5 + record_len));
    }
    s
}


// ---------------------------------------------------------------------------
// AAP plaintext version + TLS handshake probe
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TlsHandshakeProbeConfig {
    pub transport: String,
    pub usb_path: PathBuf,
    pub addr: String,
    pub dump_path: Option<PathBuf>,
    pub timeout_ms: u64,
    pub host_liveness_timeout_ms: u64,
    pub max_bytes: usize,
    pub max_frames: usize,
    pub response_major: u16,
    pub response_minor: u16,
    pub response_status: u16,
    pub keys_path: PathBuf,
    pub video_file_path: Option<PathBuf>,
    pub video_frame_limit: usize,
    pub video_native: bool,
    pub video_auto_config: bool,
    pub video_config_policy: String,
    pub video_apply_wm_config: bool,
    pub video_wm_size_mode: String,
    pub video_wm_density_mode: String,
    pub video_wm_settle_ms: u64,
    pub video_start_timing: bool,
    pub video_first_frame_timeout_ms: u64,
    pub video_first_frame_max_restarts: u32,
    pub video_native_prewarm_ms: u64,
    pub video_first_frame_kick: bool,
    pub video_first_frame_kick_delay_ms: u64,
    pub video_first_frame_kick_mode: String,
    pub video_first_frame_kick_x: i32,
    pub video_first_frame_kick_y: i32,
    pub video_first_frame_kick_keycode: u32,
    pub video_display_auto: bool,
    pub video_wm_rotation: String,
    pub video_wm_force_geometry: bool,
    pub video_wm_size_orientation: String,
    pub video_width: i32,
    pub video_height: i32,
    pub video_bit_rate: i32,
    pub video_fps: i32,
    pub video_density: u32,
    pub input_open: bool,
    pub input_key_binding: bool,
    pub input_inject: bool,
    pub input_inject_mode: String,
    pub input_app_move_min_ms: u64,
    pub input_key_inject: bool,
    pub audio_open: bool,
    pub audio_stream_filter: String,
    pub audio_dump_dir: Option<PathBuf>,
    pub audio_ack: bool,
    pub audio_test_tone: bool,
    pub audio_tone_hz: f32,
    pub audio_tone_chunk_ms: u64,
    pub audio_tone_volume: f32,
    pub audio_capture: bool,
    pub audio_capture_source: String,
    pub audio_capture_chunk_ms: u64,
    pub audio_capture_gain: f32,
    pub audio_capture_as: String,
    pub audio_capture_package: String,
    pub mic_open: bool,
    pub mic_send_request_open: bool,
    pub mic_dump_path: Option<PathBuf>,
    pub mic_ack: bool,
    pub mic_ack_mode: String,
    pub mic_max_unacked: u32,
    pub sensor_open: bool,
    pub sensor_request_all: bool,
    pub sensor_max_type: i32,
    pub sensor_period_ms: i64,
    pub sensor_jsonl_path: Option<PathBuf>,
    pub sensor_forward_os: bool,
    pub sensor_forward_vhal: bool,
    pub sensor_location_provider: String,
    pub sensor_location_min_ms: u64,
    pub sensor_vhal_min_ms: u64,
    pub sensor_wasm_path: String,
    pub sensor_wasm_fuel: u64,
    pub sensor_wasm_max_commands: usize,
    pub quiet_ping_logs: bool,
    pub log_level: String,
}

impl Default for TlsHandshakeProbeConfig {
    fn default() -> Self {
        Self {
            transport: "tcp".to_string(),
            usb_path: PathBuf::from("/dev/usb_accessory"),
            addr: "0.0.0.0:5277".to_string(),
            dump_path: Some(PathBuf::from("/data/local/tmp/arda-aap-tls-handshake.log")),
            timeout_ms: 60_000,
            host_liveness_timeout_ms: 0,
            max_bytes: 4 * 1024 * 1024,
            max_frames: 64,
            response_major: 1,
            response_minor: 7,
            response_status: 0,
            keys_path: PathBuf::from("/data/local/tmp/arda-certs"),
            video_file_path: None,
            video_frame_limit: 30,
            video_native: false,
            video_auto_config: true,
            video_config_policy: "balanced".to_string(),
            video_apply_wm_config: false,
            video_wm_size_mode: "stream".to_string(),
            video_wm_density_mode: "density".to_string(),
            video_wm_settle_ms: 1200,
            video_start_timing: false,
            video_first_frame_timeout_ms: 0,
            video_first_frame_max_restarts: 0,
            video_native_prewarm_ms: 0,
            video_first_frame_kick: false,
            video_first_frame_kick_delay_ms: 120,
            video_first_frame_kick_mode: "tap".to_string(),
            video_first_frame_kick_x: 1,
            video_first_frame_kick_y: 1,
            video_first_frame_kick_keycode: 3,
            video_display_auto: false,
            video_wm_rotation: "none".to_string(),
            video_wm_force_geometry: false,
            video_wm_size_orientation: "logical".to_string(),
            video_width: 800,
            video_height: 480,
            video_bit_rate: 2_500_000,
            video_fps: 30,
            video_density: 160,
            input_open: false,
            input_key_binding: true,
            input_inject: false,
            input_inject_mode: "uinput".to_string(),
            input_app_move_min_ms: 16,
            input_key_inject: false,
            audio_open: false,
            audio_stream_filter: "all".to_string(),
            audio_dump_dir: None,
            audio_ack: true,
            audio_test_tone: false,
            audio_tone_hz: 440.0,
            audio_tone_chunk_ms: 20,
            audio_tone_volume: 0.20,
            audio_capture: false,
            audio_capture_source: "remote_submix".to_string(),
            audio_capture_chunk_ms: 20,
            audio_capture_gain: 1.0,
            audio_capture_as: "shell".to_string(),
            audio_capture_package: "com.android.shell".to_string(),
            mic_open: false,
            mic_send_request_open: false,
            mic_dump_path: None,
            mic_ack: true,
            mic_ack_mode: "delta".to_string(),
            mic_max_unacked: 8,
            sensor_open: false,
            sensor_request_all: true,
            sensor_max_type: 23,
            sensor_period_ms: -2,
            sensor_jsonl_path: None,
            sensor_forward_os: false,
            sensor_forward_vhal: false,
            sensor_location_provider: "arda_gps".to_string(),
            sensor_location_min_ms: 1000,
            sensor_vhal_min_ms: 500,
            sensor_wasm_path: "builtin".to_string(),
            sensor_wasm_fuel: 50_000,
            sensor_wasm_max_commands: 16,
            quiet_ping_logs: true,
            log_level: "normal".to_string(),
        }
    }
}

#[derive(Debug)]
struct AudioOutputChannelState {
    service_id: i32,
    channel: u8,
    stream_type: String,
    config: Option<proto::AudioConfigSelection>,
    opened: bool,
    setup_sent: bool,
    start_sent: bool,
    session_id: i32,
    max_unacked: u32,
    ack_counter: u32,
    data_packets: u64,
    data_bytes: u64,
    tx_unacked: u32,
    tx_packets: u64,
    tx_bytes: u64,
    tx_next_pts_us: u64,
    tx_phase: f64,
    tx_next_due: Option<Instant>,
    tx_first_logged: bool,
    first_data_logged: bool,
    dump_file: Option<File>,
}

#[derive(Debug)]
struct MicSourceChannelState {
    service_id: i32,
    channel: u8,
    config: Option<proto::AudioConfigSelection>,
    opened: bool,
    setup_sent: bool,
    start_sent: bool,
    mic_request_sent: bool,
    mic_response_seen: bool,
    session_id: i32,
    max_unacked: u32,
    ack_counter: u32,
    ack_window_counter: u32,
    data_packets: u64,
    data_bytes: u64,
    first_data_logged: bool,
    first_ack_logged: bool,
    level_last_log: Instant,
    level_first_logged: bool,
    level_window_packets: u64,
    level_window_bytes: u64,
    level_window_samples: u64,
    level_window_sum_squares: u128,
    level_window_peak: i32,
    level_window_nonzero_samples: u64,
    dump_file: Option<File>,
}

#[derive(Debug)]
struct SensorChannelState {
    service_id: i32,
    channel: u8,
    sensors: Vec<proto::SensorTypeInfo>,
    opened: bool,
    requests_sent: bool,
    requests_attempted: u64,
    responses_seen: u64,
    batches_seen: u64,
    errors_seen: u64,
    jsonl_file: Option<File>,
}

#[derive(Debug)]
struct SensorOsForwarder {
    location_enabled: bool,
    gnss_hal_bridge: PlatformGnssHalBridge,
}

impl SensorOsForwarder {
    fn new(config: &TlsHandshakeProbeConfig) -> Self {
        if config.sensor_forward_os && config.sensor_location_provider != "arda_gps" {
            println!(
                "ARDA_LOCATION_PROVIDER_OPTION_IGNORED legacy_provider={} socket={}",
                config.sensor_location_provider,
                GNSS_HAL_SOCKET_NAME,
            );
        }
        Self {
            location_enabled: config.sensor_forward_os,
            gnss_hal_bridge: PlatformGnssHalBridge::new(config.sensor_location_min_ms),
        }
    }

    fn push_location(
        &mut self,
        location: &proto::DecodedLocationFix,
        dump: &mut Option<Box<dyn Write>>,
    ) -> io::Result<()> {
        if !self.location_enabled {
            return Ok(());
        }

        match self.gnss_hal_bridge.publish_fix(location) {
            Ok(GnssHalPublishOutcome::Published { connected }) => {
                if connected {
                    let line = format!(
                        "ARDA_GNSS_HAL_BRIDGE_CONNECT socket={}",
                        GNSS_HAL_SOCKET_NAME,
                    );
                    println!("{line}");
                    log_line(dump, &line)?;
                }

                let line = format!(
                    "ARDA_GNSS_HAL_FIX_PUBLISHED timestamp_raw={:?} lat={:.7} lon={:.7} accuracy_m={:?} altitude_m={:?} speed_mps={:?} bearing_deg={:?}",
                    location.timestamp_raw,
                    location.latitude_deg,
                    location.longitude_deg,
                    location.accuracy_m,
                    location.altitude_m,
                    location.speed_mps,
                    location.bearing_deg,
                );
                session_log(dump, LOG_SUMMARY, &line)?;
            }
            Ok(GnssHalPublishOutcome::Throttled) => {}
            Err(err) => {
                let line = format!(
                    "ARDA_GNSS_HAL_BRIDGE_ERROR socket={} event=FIX error={}",
                    GNSS_HAL_SOCKET_NAME,
                    err,
                );
                println!("{line}");
                log_line(dump, &line)?;
            }
        }

        Ok(())
    }

    fn push_gnss_satellite(
        &mut self,
        event: &ArdaSensorEvent,
        dump: &mut Option<Box<dyn Write>>,
    ) -> io::Result<()> {
        if !self.location_enabled {
            return Ok(());
        }

        let ArdaSensorEvent::GpsSatellite {
            in_use,
            in_view,
            satellites,
        } = event
        else {
            return Ok(());
        };

        let line = format!(
            "ARDA_GNSS_HAL_SV_NOT_FORWARDED reason=aap_lacks_constellation_and_baseband_cn0 in_use={} in_view={:?} listed={}",
            in_use,
            in_view,
            satellites.len(),
        );
        session_log(dump, LOG_SUMMARY, &line)?;

        Ok(())
    }
}

fn run_command_logged(
    program: &str,
    args: &[&str],
    dump: &mut Option<Box<dyn Write>>,
    label: &str,
) -> io::Result<bool> {
    let output = Command::new(program).args(args).output();
    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().replace('\n', " | ");
            let stderr = String::from_utf8_lossy(&output.stderr).trim().replace('\n', " | ");
            let ok = output.status.success();
            let pretty_program = program.strip_prefix("/system/bin/").unwrap_or(program);
            let line = format!(
                "{} cmd='{} {}' status={} stdout='{}' stderr='{}'",
                label,
                pretty_program,
                args.join(" "),
                output.status,
                stdout,
                stderr
            );
            println!("{line}");
            log_line(dump, &line)?;
            Ok(ok)
        }
        Err(err) => {
            let pretty_program = program.strip_prefix("/system/bin/").unwrap_or(program);
            let line = format!("{} cmd='{} {}' failed: {}", label, pretty_program, args.join(" "), err);
            println!("{line}");
            log_line(dump, &line)?;
            Err(err)
        }
    }
}

fn audio_stream_matches(filter: &str, stream_type: &str) -> bool {
    let filter = filter.to_ascii_lowercase();
    let stream = stream_type.to_ascii_lowercase();
    match filter.as_str() {
        "all" | "any" | "*" => true,
        "media" => stream.contains("media"),
        "guidance" | "nav" | "navigation" => stream.contains("guidance"),
        "system" => stream.contains("system"),
        "telephony" | "phone" | "call" => stream.contains("telephony"),
        other => stream.contains(other),
    }
}

fn make_audio_dump_file(
    dump_dir: &Option<PathBuf>,
    service_id: i32,
    stream_type: &str,
    cfg: Option<&proto::AudioConfigSelection>,
) -> io::Result<Option<File>> {
    let Some(dir) = dump_dir.as_ref() else {
        return Ok(None);
    };
    fs::create_dir_all(dir)?;
    let safe_stream = stream_type
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect::<String>();
    let suffix = if let Some(cfg) = cfg {
        format!("{}_{}hz_{}bit_{}ch", safe_stream, cfg.sampling_rate, cfg.bits, cfg.channels)
    } else {
        safe_stream
    };
    let path = dir.join(format!("service{}_{}.pcm", service_id, suffix));
    Ok(Some(OpenOptions::new().create(true).write(true).truncate(true).open(path)?))
}

fn make_mic_dump_file(path: &Option<PathBuf>) -> io::Result<Option<File>> {
    let Some(path) = path.as_ref() else {
        return Ok(None);
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(Some(OpenOptions::new().create(true).write(true).truncate(true).open(path)?))
}

fn make_sensor_jsonl_file(path: &Option<PathBuf>) -> io::Result<Option<File>> {
    let Some(path) = path.as_ref() else {
        return Ok(None);
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(Some(OpenOptions::new().create(true).write(true).truncate(true).open(path)?))
}


fn requested_sensor_types(sensor: &SensorChannelState, config: &TlsHandshakeProbeConfig) -> Vec<i32> {
    if !config.sensor_request_all {
        return Vec::new();
    }
    let mut values = if sensor.sensors.is_empty() {
        (1..=config.sensor_max_type.max(1)).collect::<Vec<_>>()
    } else {
        sensor.sensors.iter().map(|s| s.value).collect::<Vec<_>>()
    };
    values.sort_unstable();
    values.dedup();
    values
        .into_iter()
        .filter(|value| *value >= 1 && *value <= config.sensor_max_type && *value <= 23)
        .collect()
}

fn sensor_request_period_ms(sensor_type: i32, config: &TlsHandshakeProbeConfig) -> i64 {
    if config.sensor_period_ms >= -1 {
        return config.sensor_period_ms;
    }
    match sensor_type {
        1 | 2 | 21 => 1000,
        3 => 500,
        7 | 8 | 14 | 19 | 20 => 500,
        4 | 5 | 6 | 11 | 12 | 18 => 10_000,
        10 | 13 | 15 | 16 | 17 => 0,
        _ => 1000,
    }
}

fn audio_cfg_values(cfg: Option<&proto::AudioConfigSelection>) -> (u32, u32, u32) {
    let rate = cfg.map(|c| c.sampling_rate).filter(|v| *v > 0).unwrap_or(48_000);
    let bits = cfg.map(|c| c.bits).filter(|v| *v > 0).unwrap_or(16);
    let channels = cfg.map(|c| c.channels).filter(|v| *v > 0).unwrap_or(2);
    (rate, bits, channels)
}

fn generate_sine_pcm_s16le(
    cfg: Option<&proto::AudioConfigSelection>,
    tone_hz: f32,
    duration_ms: u64,
    volume: f32,
    phase: &mut f64,
) -> Vec<u8> {
    let (rate, bits, channels) = audio_cfg_values(cfg);
    let rate = rate.max(1);
    let channels = channels.max(1);
    let sample_count = ((rate as u64).saturating_mul(duration_ms).max(1) / 1000).max(1) as usize;
    let bytes_per_sample = if bits <= 8 { 1usize } else { 2usize };
    let mut out = Vec::with_capacity(sample_count * channels as usize * bytes_per_sample);
    let amp = (i16::MAX as f32 * volume.clamp(0.0, 1.0)) as i16;
    let step = (2.0f64 * std::f64::consts::PI * tone_hz.max(1.0) as f64) / rate as f64;
    for _ in 0..sample_count {
        let sample = ((*phase).sin() * amp as f64) as i16;
        *phase += step;
        if *phase > 2.0f64 * std::f64::consts::PI {
            *phase -= 2.0f64 * std::f64::consts::PI;
        }
        for _ in 0..channels {
            if bits <= 8 {
                let unsigned = ((sample as i32 + 32768) >> 8).clamp(0, 255) as u8;
                out.push(unsigned);
            } else {
                out.extend_from_slice(&sample.to_le_bytes());
            }
        }
    }
    out
}


fn audio_chunk_bytes(cfg: Option<&proto::AudioConfigSelection>, chunk_ms: u64) -> usize {
    let (rate, bits, channels) = audio_cfg_values(cfg);
    let bytes_per_sample = if bits <= 8 { 1usize } else { 2usize };
    ((rate.max(1) as u64)
        .saturating_mul(channels.max(1) as u64)
        .saturating_mul(bytes_per_sample as u64)
        .saturating_mul(chunk_ms.max(1))
        / 1000)
        .max(bytes_per_sample as u64 * channels.max(1) as u64) as usize
}

fn apply_pcm_s16le_gain(pcm: &mut [u8], gain: f32) {
    if (gain - 1.0).abs() < 0.01 {
        return;
    }
    let gain = gain.clamp(0.0, 4.0);
    for chunk in pcm.chunks_exact_mut(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as f32;
        let scaled = (sample * gain).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        let b = scaled.to_le_bytes();
        chunk[0] = b[0];
        chunk[1] = b[1];
    }
}

fn pcm_s16le_metrics(pcm: &[u8]) -> (u64, u128, i32, u64) {
    let mut samples = 0u64;
    let mut sum_squares = 0u128;
    let mut peak = 0i32;
    let mut nonzero_samples = 0u64;
    for chunk in pcm.chunks_exact(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as i32;
        let abs = sample.unsigned_abs() as i32;
        if abs > 0 {
            nonzero_samples = nonzero_samples.saturating_add(1);
        }
        peak = peak.max(abs);
        sum_squares = sum_squares.saturating_add((sample as i128 * sample as i128) as u128);
        samples = samples.saturating_add(1);
    }
    (samples, sum_squares, peak, nonzero_samples)
}

fn rms_from_sum_squares(sum_squares: u128, samples: u64) -> f64 {
    if samples == 0 {
        0.0
    } else {
        ((sum_squares as f64) / samples as f64).sqrt()
    }
}

fn pump_audio_capture<W: Write + ?Sized>(
    audio_output_channels: &mut [AudioOutputChannelState],
    capture: &mut Option<AppProcessAudioCapture>,
    capture_buffer: &mut Vec<u8>,
    stream: &mut W,
    tls: &mut tls::AapTlsServer,
    config: &TlsHandshakeProbeConfig,
    dump: &mut Option<Box<dyn Write>>,
) -> io::Result<usize> {
    if !config.audio_capture {
        return Ok(0);
    }
    let Some(capture) = capture.as_mut() else {
        return Ok(0);
    };

    capture.drain_into(capture_buffer, 32);
    let mut tx_ssl_frames = 0usize;

    for audio in audio_output_channels.iter_mut() {
        if !audio.start_sent {
            continue;
        }
        let max_in_flight = audio.max_unacked.max(1).min(8);
        let chunk_bytes = audio_chunk_bytes(audio.config.as_ref(), config.audio_capture_chunk_ms);
        while audio.tx_unacked < max_in_flight && capture_buffer.len() >= chunk_bytes {
            let mut pcm = capture_buffer.drain(..chunk_bytes).collect::<Vec<u8>>();
            apply_pcm_s16le_gain(&mut pcm, config.audio_capture_gain);
            let payload = proto::build_media_data_payload(audio.tx_next_pts_us, &pcm);
            let tx_count = write_encrypted_plaintext_fragmented(
                stream,
                tls,
                audio.channel,
                &payload,
                dump,
                "AUDIO_CAPTURE_MEDIA_MESSAGE_DATA",
            )?;
            tx_ssl_frames += tx_count;
            audio.tx_packets += 1;
            audio.tx_bytes += pcm.len() as u64;
            audio.tx_unacked = audio.tx_unacked.saturating_add(1);
            audio.tx_next_pts_us = audio.tx_next_pts_us.saturating_add(config.audio_capture_chunk_ms.saturating_mul(1000));
            if !audio.tx_first_logged || session_log_enabled(LOG_NORMAL) {
                let (rate, bits, channels) = audio_cfg_values(audio.config.as_ref());
                let line = format!(
                    "AUDIO_CAPTURE sent service_id={} stream={} packet={} pts_us={} bytes={} format={}Hz/{}bit/{}ch source={} gain={} buffered={} unacked={}/{}",
                    audio.service_id,
                    audio.stream_type,
                    audio.tx_packets,
                    audio.tx_next_pts_us.saturating_sub(config.audio_capture_chunk_ms.saturating_mul(1000)),
                    pcm.len(),
                    rate,
                    bits,
                    channels,
                    config.audio_capture_source,
                    config.audio_capture_gain,
                    capture_buffer.len(),
                    audio.tx_unacked,
                    max_in_flight
                );
                println!("{line}");
                log_line(dump, &line)?;
                audio.tx_first_logged = true;
            }
        }
    }
    Ok(tx_ssl_frames)
}

fn pump_audio_test_tone<W: Write + ?Sized>(
    audio_output_channels: &mut [AudioOutputChannelState],
    stream: &mut W,
    tls: &mut tls::AapTlsServer,
    config: &TlsHandshakeProbeConfig,
    dump: &mut Option<Box<dyn Write>>,
) -> io::Result<usize> {
    if !config.audio_test_tone {
        return Ok(0);
    }
    let mut tx_ssl_frames = 0usize;
    let now = Instant::now();
    for audio in audio_output_channels.iter_mut() {
        if !audio.start_sent {
            continue;
        }
        if let Some(due) = audio.tx_next_due {
            if now < due {
                continue;
            }
        }
        // DHU advertises high max_unacked for audio, but use a small local guard
        // so a missing ACK cannot flood the TCP/TLS loop with generated PCM.
        let max_in_flight = audio.max_unacked.max(1).min(8);
        if audio.tx_unacked >= max_in_flight {
            continue;
        }
        let pcm = generate_sine_pcm_s16le(
            audio.config.as_ref(),
            config.audio_tone_hz,
            config.audio_tone_chunk_ms,
            config.audio_tone_volume,
            &mut audio.tx_phase,
        );
        let payload = proto::build_media_data_payload(audio.tx_next_pts_us, &pcm);
        let tx_count = write_encrypted_plaintext_fragmented(
            stream,
            tls,
            audio.channel,
            &payload,
            dump,
            "AUDIO_TEST_TONE_MEDIA_MESSAGE_DATA",
        )?;
        tx_ssl_frames += tx_count;
        audio.tx_packets += 1;
        audio.tx_bytes += pcm.len() as u64;
        audio.tx_unacked = audio.tx_unacked.saturating_add(1);
        audio.tx_next_pts_us = audio.tx_next_pts_us.saturating_add(config.audio_tone_chunk_ms.saturating_mul(1000));
        audio.tx_next_due = Some(now + Duration::from_millis(config.audio_tone_chunk_ms.max(1)));
        if !audio.tx_first_logged || session_log_enabled(LOG_NORMAL) {
            let (rate, bits, channels) = audio_cfg_values(audio.config.as_ref());
            let line = format!(
                "AUDIO_TEST_TONE sent service_id={} stream={} packet={} pts_us={} bytes={} format={}Hz/{}bit/{}ch tone_hz={} volume={} unacked={}/{}",
                audio.service_id,
                audio.stream_type,
                audio.tx_packets,
                audio.tx_next_pts_us.saturating_sub(config.audio_tone_chunk_ms.saturating_mul(1000)),
                pcm.len(),
                rate,
                bits,
                channels,
                config.audio_tone_hz,
                config.audio_tone_volume,
                audio.tx_unacked,
                max_in_flight
            );
            println!("{line}");
            log_line(dump, &line)?;
            audio.tx_first_logged = true;
        }
    }
    Ok(tx_ssl_frames)
}

pub fn tls_handshake_probe(config: TlsHandshakeProbeConfig) -> io::Result<()> {
    println!("arda aap tls handshake probe");
    println!("addr={}", config.addr);
    println!("timeout_ms={}", config.timeout_ms);
    println!("max_bytes={}", config.max_bytes);
    println!("max_frames={}", config.max_frames);
    println!("response={}.{} status=0x{:04x}", config.response_major, config.response_minor, config.response_status);
    println!("keys={}", config.keys_path.display());
    if let Some(path) = &config.video_file_path {
        println!("video_file={}", path.display());
        println!("video_frame_limit={}", config.video_frame_limit);
    } else {
        println!("video_file=none");
    }
    if let Some(path) = &config.dump_path {
        println!("dump={}", path.display());
    } else {
        println!("dump=none");
    }
    println!();

    let listener = TcpListener::bind(&config.addr)?;
    println!("listening on {}", listener.local_addr()?);
    println!("waiting for one client...");

    let (mut stream, peer) = listener.accept()?;
    println!("client connected: {peer}");
    stream.set_read_timeout(Some(Duration::from_millis(config.timeout_ms)))?;
    stream.set_write_timeout(Some(Duration::from_millis(config.timeout_ms)))?;

    let mut dump: Option<Box<dyn Write>> = match &config.dump_path {
        Some(path) => Some(Box::new(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)?,
        )),
        None => None,
    };

    log_line(&mut dump, "arda aap tls handshake probe")?;
    log_line(&mut dump, &format!("local={}", listener.local_addr()?))?;
    log_line(&mut dump, &format!("peer={peer}"))?;
    log_line(&mut dump, &format!("response={}.{} status=0x{:04x}", config.response_major, config.response_minor, config.response_status))?;
    log_line(&mut dump, &format!("keys={}", config.keys_path.display()))?;
    log_line(&mut dump, "role=phone-like TCP server + TLS server using md_cert/md_key; peer cert accepted")?;
    log_line(&mut dump, "")?;

    let started = Instant::now();
    let mut reader = FrameReader::new(config.max_bytes);

    let version = match reader.read_frame(&mut stream) {
        Ok(Some(frame)) => frame,
        Ok(None) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "peer closed before version request")),
        Err(err) => return Err(err),
    };

    println!("rx version candidate:");
    describe_frame(1, HEADER_LENGTH + version.payload.len() + if version.final_length.is_some() { 4 } else { 0 }, peer, &version, &mut io::stdout())?;
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "rx version candidate:")?;
        describe_frame(1, HEADER_LENGTH + version.payload.len() + if version.final_length.is_some() { 4 } else { 0 }, peer, &version, dump)?;
        dump.flush()?;
    }

    let Some((req_major, req_minor)) = parse_version_request(&version) else {
        log_line(&mut dump, "not a valid MESSAGE_VERSION_REQUEST; closing")?;
        return Err(io::Error::new(io::ErrorKind::InvalidData, "expected MESSAGE_VERSION_REQUEST on control channel"));
    };

    println!("version request: {}.{}", req_major, req_minor);
    log_line(&mut dump, &format!("version request: {}.{}", req_major, req_minor))?;

    let response_payload = build_version_response(config.response_major, config.response_minor, config.response_status);
    let response_frame = AapFrame {
        channel: 0,
        flags: FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
        final_length: None,
        payload: response_payload,
    };
    write_frame(&mut stream, &response_frame)?;
    stream.flush()?;

    println!("tx version response:");
    describe_frame(1, HEADER_LENGTH + response_frame.payload.len(), peer, &response_frame, &mut io::stdout())?;
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "tx version response:")?;
        describe_frame(1, HEADER_LENGTH + response_frame.payload.len(), peer, &response_frame, dump)?;
        dump.flush()?;
    }

    let mut tls = tls::AapTlsServer::new_phone_like(&config.keys_path)?;
    println!("tls server ready: {}", tls.state_string());
    log_line(&mut dump, &format!("tls server ready: {}", tls.state_string()))?;

    let mut frames = 1usize;
    let mut bytes = HEADER_LENGTH + version.payload.len() + HEADER_LENGTH + response_frame.payload.len();
    let mut ssl_frames = 0usize;
    let mut tx_ssl_frames = 0usize;
    let mut decrypted_bytes = 0usize;

    while frames < config.max_frames {
        if session_shutdown_requested() {
            println!("shutdown requested; stopping AAP session");
            log_line(&mut dump, "shutdown requested; stopping AAP session")?;
            break;
        }

        let frame = match reader.read_frame(&mut stream) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                println!("client disconnected after frames={frames} ssl_frames={ssl_frames} bytes={bytes}");
                log_line(&mut dump, &format!("client disconnected after frames={frames} ssl_frames={ssl_frames} bytes={bytes}"))?;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted && session_shutdown_requested() => {
                println!("read interrupted by shutdown request; stopping AAP session");
                log_line(&mut dump, "read interrupted by shutdown request; stopping AAP session")?;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.kind() == io::ErrorKind::TimedOut =>
            {
                println!("read timeout after frames={frames} ssl_frames={ssl_frames} bytes={bytes}");
                log_line(&mut dump, &format!("read timeout after frames={frames} ssl_frames={ssl_frames} bytes={bytes}"))?;
                break;
            }
            Err(err) => return Err(err),
        };

        frames += 1;
        bytes += HEADER_LENGTH + frame.payload.len() + if frame.final_length.is_some() { 4 } else { 0 };
        println!("rx tls-stage frame:");
        describe_frame(frames, bytes, peer, &frame, &mut io::stdout())?;
        if let Some(dump) = dump.as_mut() {
            writeln!(dump, "rx tls-stage frame:")?;
            describe_frame(frames, bytes, peer, &frame, dump)?;
        }

        let Some(tls_bytes) = ssl_payload_bytes(&frame) else {
            let line = "non MESSAGE_ENCAPSULATED_SSL frame during TLS probe; ignoring";
            println!("{line}");
            log_line(&mut dump, line)?;
            continue;
        };

        ssl_frames += 1;
        let hint = tls_record_hint(tls_bytes);
        println!("rx ssl frame #{ssl_frames}: tls_bytes={} {hint}", tls_bytes.len());
        if let Some(dump) = dump.as_mut() {
            writeln!(dump, "rx ssl frame #{ssl_frames}: tls_bytes={} {hint}", tls_bytes.len())?;
            let dump_len = tls_bytes.len().min(256);
            if dump_len > 0 {
                print_hex_dump(0, &tls_bytes[..dump_len], dump)?;
                if tls_bytes.len() > dump_len {
                    writeln!(dump, "... tls bytes truncated for display: {} / {} bytes", dump_len, tls_bytes.len())?;
                }
            }
            dump.flush()?;
        }

        let step = match tls.feed_tls(tls_bytes) {
            Ok(step) => step,
            Err(err) => {
                println!("tls process error: {err}");
                log_line(&mut dump, &format!("tls process error: {err}"))?;
                return Err(err);
            }
        };

        println!(
            "tls step: state='{}' handshaking={} cipher={} outgoing_tls={} plaintext={}",
            step.state,
            step.still_handshaking,
            step.cipher_suite,
            step.outgoing_tls.len(),
            step.plaintext.len()
        );
        log_line(
            &mut dump,
            &format!(
                "tls step: state='{}' handshaking={} cipher={} outgoing_tls={} plaintext={}",
                step.state,
                step.still_handshaking,
                step.cipher_suite,
                step.outgoing_tls.len(),
                step.plaintext.len()
            ),
        )?;

        if !step.plaintext.is_empty() {
            decrypted_bytes += step.plaintext.len();
            println!("decrypted plaintext bytes={}", step.plaintext.len());
            if let Some(dump) = dump.as_mut() {
                writeln!(dump, "decrypted plaintext bytes={}", step.plaintext.len())?;
                print_hex_dump(0, &step.plaintext[..step.plaintext.len().min(256)], dump)?;
                dump.flush()?;
            }
        }

        if !step.outgoing_tls.is_empty() {
            tx_ssl_frames += 1;
            let tx_frame = build_ssl_frame(&step.outgoing_tls);
            write_frame(&mut stream, &tx_frame)?;
            stream.flush()?;
            println!("tx ssl frame #{tx_ssl_frames}: tls_bytes={}", step.outgoing_tls.len());
            if let Some(dump) = dump.as_mut() {
                writeln!(dump, "tx ssl frame #{tx_ssl_frames}: tls_bytes={} {}", step.outgoing_tls.len(), tls_record_hint(&step.outgoing_tls))?;
                print_hex_dump(0, &step.outgoing_tls[..step.outgoing_tls.len().min(256)], dump)?;
                dump.flush()?;
            }
        }

        if !tls.is_handshaking() {
            println!("tls handshake complete: cipher={}", tls.cipher_suite());
            log_line(&mut dump, &format!("tls handshake complete: cipher={}", tls.cipher_suite()))?;
            break;
        }
    }

    let elapsed = started.elapsed();
    println!("summary: frames={frames} rx_ssl_frames={ssl_frames} tx_ssl_frames={tx_ssl_frames} decrypted_bytes={decrypted_bytes} bytes={bytes} elapsed_ms={}", elapsed.as_millis());
    log_line(&mut dump, &format!("summary: frames={frames} rx_ssl_frames={ssl_frames} tx_ssl_frames={tx_ssl_frames} decrypted_bytes={decrypted_bytes} bytes={bytes} elapsed_ms={}", elapsed.as_millis()))?;
    Ok(())
}



// ---------------------------------------------------------------------------
// AAP TLS plaintext probe
// ---------------------------------------------------------------------------

/// Runs the same VERSION + TLS handshake as `tls_handshake_probe`, but keeps
/// the connection open afterwards and decrypts post-handshake encrypted AAP
/// frames. This is the first probe after DHU reports SSL success: it should let
/// us see the next plaintext control message, usually SERVICE_DISCOVERY_REQUEST.
pub fn tls_plain_probe(config: TlsHandshakeProbeConfig) -> io::Result<()> {
    println!("arda aap tls plain probe");
    println!("addr={}", config.addr);
    println!("timeout_ms={}", config.timeout_ms);
    println!("max_bytes={}", config.max_bytes);
    println!("max_frames={}", config.max_frames);
    println!("response={}.{} status=0x{:04x}", config.response_major, config.response_minor, config.response_status);
    println!("keys={}", config.keys_path.display());
    println!("log_level={}", normalize_session_log_level(&config.log_level));
    if let Some(path) = &config.dump_path {
        println!("dump={}", path.display());
    } else {
        println!("dump=none");
    }
    println!();

    let listener = TcpListener::bind(&config.addr)?;
    println!("listening on {}", listener.local_addr()?);
    println!("waiting for one client...");

    let (mut stream, peer) = listener.accept()?;
    println!("client connected: {peer}");
    stream.set_read_timeout(Some(Duration::from_millis(config.timeout_ms)))?;
    stream.set_write_timeout(Some(Duration::from_millis(config.timeout_ms)))?;

    let mut dump: Option<Box<dyn Write>> = match &config.dump_path {
        Some(path) => Some(Box::new(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)?,
        )),
        None => None,
    };

    log_line(&mut dump, "arda aap tls plain probe")?;
    log_line(&mut dump, &format!("local={}", listener.local_addr()?))?;
    log_line(&mut dump, &format!("peer={peer}"))?;
    log_line(&mut dump, &format!("response={}.{} status=0x{:04x}", config.response_major, config.response_minor, config.response_status))?;
    log_line(&mut dump, &format!("keys={}", config.keys_path.display()))?;
    log_line(&mut dump, "role=phone-like TCP server + TLS server using md_cert/md_key; peer cert accepted")?;
    log_line(&mut dump, "post_handshake=decrypt encrypted AA frames and log plaintext only")?;
    log_line(&mut dump, "")?;

    let started = Instant::now();
    let mut reader = FrameReader::new(config.max_bytes);

    let version = match reader.read_frame(&mut stream) {
        Ok(Some(frame)) => frame,
        Ok(None) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "peer closed before version request")),
        Err(err) => return Err(err),
    };

    println!("rx version candidate:");
    describe_frame(1, HEADER_LENGTH + version.payload.len() + if version.final_length.is_some() { 4 } else { 0 }, peer, &version, &mut io::stdout())?;
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "rx version candidate:")?;
        describe_frame(1, HEADER_LENGTH + version.payload.len() + if version.final_length.is_some() { 4 } else { 0 }, peer, &version, dump)?;
        dump.flush()?;
    }

    let Some((req_major, req_minor)) = parse_version_request(&version) else {
        log_line(&mut dump, "not a valid MESSAGE_VERSION_REQUEST; closing")?;
        return Err(io::Error::new(io::ErrorKind::InvalidData, "expected MESSAGE_VERSION_REQUEST on control channel"));
    };

    println!("version request: {}.{}", req_major, req_minor);
    log_line(&mut dump, &format!("version request: {}.{}", req_major, req_minor))?;

    let response_payload = build_version_response(config.response_major, config.response_minor, config.response_status);
    let response_frame = AapFrame {
        channel: 0,
        flags: FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
        final_length: None,
        payload: response_payload,
    };
    write_frame(&mut stream, &response_frame)?;
    stream.flush()?;

    println!("tx version response:");
    describe_frame(1, HEADER_LENGTH + response_frame.payload.len(), peer, &response_frame, &mut io::stdout())?;
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "tx version response:")?;
        describe_frame(1, HEADER_LENGTH + response_frame.payload.len(), peer, &response_frame, dump)?;
        dump.flush()?;
    }

    let mut tls = tls::AapTlsServer::new_phone_like(&config.keys_path)?;
    println!("tls server ready: {}", tls.state_string());
    log_line(&mut dump, &format!("tls server ready: {}", tls.state_string()))?;

    let mut frames = 1usize;
    let mut bytes = HEADER_LENGTH + version.payload.len() + HEADER_LENGTH + response_frame.payload.len();
    let mut rx_ssl_frames = 0usize;
    let mut tx_ssl_frames = 0usize;
    let mut encrypted_frames = 0usize;
    let mut decrypted_frames = 0usize;
    let mut decrypted_bytes = 0usize;
    let mut handshake_done_logged = false;

    while frames < config.max_frames {
        if session_shutdown_requested() {
            println!("shutdown requested; stopping AAP session");
            log_line(&mut dump, "shutdown requested; stopping AAP session")?;
            break;
        }

        let frame = match reader.read_frame(&mut stream) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                println!("client disconnected after frames={frames} encrypted_frames={encrypted_frames} decrypted_frames={decrypted_frames} bytes={bytes}");
                log_line(&mut dump, &format!("client disconnected after frames={frames} encrypted_frames={encrypted_frames} decrypted_frames={decrypted_frames} bytes={bytes}"))?;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted && session_shutdown_requested() => {
                println!("read interrupted by shutdown request; stopping AAP session");
                log_line(&mut dump, "read interrupted by shutdown request; stopping AAP session")?;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.kind() == io::ErrorKind::TimedOut =>
            {
                println!("read timeout after frames={frames} encrypted_frames={encrypted_frames} decrypted_frames={decrypted_frames} bytes={bytes}");
                log_line(&mut dump, &format!("read timeout after frames={frames} encrypted_frames={encrypted_frames} decrypted_frames={decrypted_frames} bytes={bytes}"))?;
                break;
            }
            Err(err) => return Err(err),
        };

        frames += 1;
        bytes += HEADER_LENGTH + frame.payload.len() + if frame.final_length.is_some() { 4 } else { 0 };
        let likely_encrypted_ping = config.quiet_ping_logs
            && frame.channel == 0
            && (frame.flags & FLAG_ENCRYPTED) != 0
            && frame.final_length.is_none()
            && frame.payload.len() <= 64;
        if !likely_encrypted_ping && session_log_enabled(LOG_VERBOSE) {
            println!("rx frame:");
            describe_frame(frames, bytes, peer, &frame, &mut io::stdout())?;
            if let Some(dump) = dump.as_mut() {
                writeln!(dump, "rx frame:")?;
                describe_frame(frames, bytes, peer, &frame, dump)?;
                dump.flush()?;
            }
        }

        if tls.is_handshaking() {
            let Some(tls_bytes) = ssl_payload_bytes(&frame) else {
                let line = "non MESSAGE_ENCAPSULATED_SSL frame while TLS is still handshaking; ignoring";
                println!("{line}");
                log_line(&mut dump, line)?;
                continue;
            };

            rx_ssl_frames += 1;
            let hint = tls_record_hint(tls_bytes);
            println!("rx ssl handshake frame #{rx_ssl_frames}: tls_bytes={} {hint}", tls_bytes.len());
            log_line(&mut dump, &format!("rx ssl handshake frame #{rx_ssl_frames}: tls_bytes={} {hint}", tls_bytes.len()))?;

            let step = tls.feed_tls(tls_bytes)?;
            log_tls_step(&mut dump, &step)?;

            if !step.plaintext.is_empty() {
                decrypted_bytes += step.plaintext.len();
                decrypted_frames += 1;
                log_decrypted_payload(&mut dump, peer, frame.channel, frame.flags, decrypted_frames, &step.plaintext)?;
            }

            if !step.outgoing_tls.is_empty() {
                tx_ssl_frames += 1;
                let tx_frame = build_ssl_frame(&step.outgoing_tls);
                write_frame(&mut stream, &tx_frame)?;
                stream.flush()?;
                println!("tx ssl handshake frame #{tx_ssl_frames}: tls_bytes={}", step.outgoing_tls.len());
                log_line(&mut dump, &format!("tx ssl handshake frame #{tx_ssl_frames}: tls_bytes={} {}", step.outgoing_tls.len(), tls_record_hint(&step.outgoing_tls)))?;
            }

            if !tls.is_handshaking() && !handshake_done_logged {
                handshake_done_logged = true;
                println!("tls handshake complete: cipher={}", tls.cipher_suite());
                log_line(&mut dump, &format!("tls handshake complete: cipher={}", tls.cipher_suite()))?;
                println!("waiting for encrypted post-TLS AA frames...");
                log_line(&mut dump, "waiting for encrypted post-TLS AA frames...")?;
            }
            continue;
        }

        if (frame.flags & FLAG_ENCRYPTED) == 0 {
            let line = "post-TLS frame is not marked encrypted; logging plaintext as-is";
            println!("{line}");
            log_line(&mut dump, line)?;
            decrypted_frames += 1;
            decrypted_bytes += frame.payload.len();
            log_decrypted_payload(&mut dump, peer, frame.channel, frame.flags, decrypted_frames, &frame.payload)?;
            continue;
        }

        encrypted_frames += 1;
        println!("rx encrypted AA frame #{encrypted_frames}: tls_bytes={} {}", frame.payload.len(), tls_record_hint(&frame.payload));
        log_line(&mut dump, &format!("rx encrypted AA frame #{encrypted_frames}: tls_bytes={} {}", frame.payload.len(), tls_record_hint(&frame.payload)))?;

        let step = tls.feed_tls(&frame.payload)?;
        log_tls_step(&mut dump, &step)?;

        if !step.plaintext.is_empty() {
            decrypted_frames += 1;
            decrypted_bytes += step.plaintext.len();
            log_decrypted_payload(&mut dump, peer, frame.channel, frame.flags, decrypted_frames, &step.plaintext)?;
        } else {
            log_line(&mut dump, "decrypted plaintext bytes=0")?;
        }

        // Any post-handshake outgoing TLS records here are protocol-level records
        // generated by rustls, not an AA application response yet. Send them back
        // on the same AA channel only if rustls produced something.
        if !step.outgoing_tls.is_empty() {
            tx_ssl_frames += 1;
            let tx_frame = AapFrame {
                channel: frame.channel,
                flags: (frame.flags | FLAG_ENCRYPTED) & !FLAG_CONTROL,
                final_length: None,
                payload: step.outgoing_tls,
            };
            write_frame(&mut stream, &tx_frame)?;
            stream.flush()?;
            println!("tx encrypted TLS maintenance frame #{tx_ssl_frames}: tls_bytes={}", tx_frame.payload.len());
            log_line(&mut dump, &format!("tx encrypted TLS maintenance frame #{tx_ssl_frames}: tls_bytes={} {}", tx_frame.payload.len(), tls_record_hint(&tx_frame.payload)))?;
        }
    }

    let elapsed = started.elapsed();
    println!("summary: frames={frames} rx_ssl_frames={rx_ssl_frames} tx_ssl_frames={tx_ssl_frames} encrypted_frames={encrypted_frames} decrypted_frames={decrypted_frames} decrypted_bytes={decrypted_bytes} bytes={bytes} elapsed_ms={}", elapsed.as_millis());
    log_line(&mut dump, &format!("summary: frames={frames} rx_ssl_frames={rx_ssl_frames} tx_ssl_frames={tx_ssl_frames} encrypted_frames={encrypted_frames} decrypted_frames={decrypted_frames} decrypted_bytes={decrypted_bytes} bytes={bytes} elapsed_ms={}", elapsed.as_millis()))?;
    Ok(())
}

fn log_tls_step(dump: &mut Option<Box<dyn Write>>, step: &tls::TlsServerStep) -> io::Result<()> {
    if !session_log_enabled(LOG_VERBOSE) {
        return Ok(());
    }
    let line = format!(
        "tls step: state='{}' handshaking={} cipher={} outgoing_tls={} plaintext={}",
        step.state,
        step.still_handshaking,
        step.cipher_suite,
        step.outgoing_tls.len(),
        step.plaintext.len()
    );
    println!("{line}");
    log_line(dump, &line)
}

fn is_ping_request_payload(payload: &[u8]) -> bool {
    payload.len() >= 2 && u16::from_be_bytes([payload[0], payload[1]]) == 11
}

fn is_ping_response_payload(payload: &[u8]) -> bool {
    payload.len() >= 2 && u16::from_be_bytes([payload[0], payload[1]]) == 12
}

fn log_decrypted_payload(
    dump: &mut Option<Box<dyn Write>>,
    peer: impl std::fmt::Display,
    channel: u8,
    flags: u8,
    index: usize,
    plaintext: &[u8],
) -> io::Result<()> {
    if !session_log_enabled(LOG_VERBOSE) {
        return Ok(());
    }
    let pseudo = AapFrame {
        channel,
        flags: flags & !FLAG_ENCRYPTED,
        final_length: None,
        payload: plaintext.to_vec(),
    };
    let hint = control_hint(&pseudo)
        .or_else(|| {
            if channel != 0 && (flags & FLAG_CONTROL) == 0 {
                proto::describe_media_payload(plaintext)
            } else {
                None
            }
        })
        .map(|h| format!(" {h}"))
        .unwrap_or_default();
    println!("decrypted AA payload #{index}: channel=0x{channel:02x} bytes={}{}", plaintext.len(), hint);
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "decrypted AA payload #{index}: channel=0x{channel:02x} bytes={}{}", plaintext.len(), hint)?;
        describe_frame(index, 0, peer, &pseudo, dump)?;
        dump.flush()?;
    }
    Ok(())
}




// ---------------------------------------------------------------------------
// AAP TLS + ServiceDiscoveryRequest probe
// ---------------------------------------------------------------------------

/// Runs VERSION + TLS, waits for AUTH_COMPLETE, then sends a phone-like
/// ServiceDiscoveryRequest built through generated protobuf structs. The
/// request is encrypted in TLS and transmitted as an AA frame with ENCRYPTED set.

const ARDA_VIDEO_SDR_REPORT_ACTION: &str =
    "com.github.deadknight.ardaplatform.action.REPORT_VIDEO_SDR";
const ARDA_VIDEO_SDR_REPORT_RECEIVER: &str =
    "com.github.deadknight.ardaplatform.ArdaVideoConfigReceiver";
const ARDA_DEFAULT_PLATFORM_PACKAGE: &str =
    "com.github.deadknight.ardaplatform.debug";

fn json_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 8);
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

fn video_sdr_report_json(
    video: &proto::MediaSinkSelection,
    selection_policy: &str,
) -> String {
    let selected = video.selected_video_config.as_ref();
    let configs = video
        .video_config_selections
        .iter()
        .map(|config| {
            format!(
                concat!(
                    "{{\"config_index\":{},\"width\":{},\"height\":{},",
                    "\"fps\":{},\"density\":{},\"real_density\":{},",
                    "\"width_margin\":{},\"height_margin\":{},",
                    "\"margin_left\":{},\"margin_right\":{},",
                    "\"margin_top\":{},\"margin_bottom\":{},",
                    "\"content_inset_left\":{},\"content_inset_right\":{},",
                    "\"content_inset_top\":{},\"content_inset_bottom\":{},",
                    "\"stable_inset_left\":{},\"stable_inset_right\":{},",
                    "\"stable_inset_top\":{},\"stable_inset_bottom\":{},",
                    "\"codec\":\"{}\",\"resolution\":\"{}\"}}"
                ),
                config.config_index,
                config.width,
                config.height,
                config.fps,
                config.density,
                config.real_density,
                config.width_margin,
                config.height_margin,
                config.margin_left,
                config.margin_right,
                config.margin_top,
                config.margin_bottom,
                config.content_inset_left,
                config.content_inset_right,
                config.content_inset_top,
                config.content_inset_bottom,
                config.stable_inset_left,
                config.stable_inset_right,
                config.stable_inset_top,
                config.stable_inset_bottom,
                json_escape(&config.codec),
                json_escape(&config.resolution),
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    format!(
        concat!(
            "{{\"schema\":1,\"service_id\":{},\"display_id\":{},",
            "\"display_type\":\"{}\",\"selection_policy\":\"{}\",",
            "\"selected_config_index\":{},\"selected_resolution\":\"{}\",",
            "\"selected_fps\":{},\"configs\":[{}]}}"
        ),
        video.service_id,
        video.display_id,
        json_escape(&video.display_type),
        json_escape(&proto::normalize_video_config_policy(selection_policy)),
        selected.map(|value| value.config_index as i32).unwrap_or(-1),
        selected
            .map(|value| format!("{}x{}", value.width, value.height))
            .unwrap_or_default(),
        selected.map(|value| value.fps).unwrap_or(0),
        configs,
    )
}

fn report_video_sdr_to_platform(
    video: &proto::MediaSinkSelection,
    selection_policy: &str,
    dump: &mut Option<Box<dyn Write>>,
) -> io::Result<()> {
    let package = env::var("ARDA_PLATFORM_PACKAGE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ARDA_DEFAULT_PLATFORM_PACKAGE.to_string());
    let component = format!("{package}/{ARDA_VIDEO_SDR_REPORT_RECEIVER}");
    let payload = video_sdr_report_json(video, selection_policy);
    let output = Command::new("/system/bin/am")
        .args([
            "broadcast",
            "--user",
            "0",
            "-a",
            ARDA_VIDEO_SDR_REPORT_ACTION,
            "-n",
            &component,
            "--es",
            "payload",
            &payload,
        ])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout)
        .replace('\n', " ")
        .trim()
        .to_string();
    let stderr = String::from_utf8_lossy(&output.stderr)
        .replace('\n', " ")
        .trim()
        .to_string();
    let line = if output.status.success() {
        format!(
            "VIDEO_PLATFORM_SDR_REPORT=PASS package={} configs={} selected={} output={}",
            package,
            video.video_config_selections.len(),
            video
                .selected_video_config
                .as_ref()
                .map(|value| format!("{}x{}@{}", value.width, value.height, value.fps))
                .unwrap_or_else(|| "none".to_string()),
            stdout,
        )
    } else {
        format!(
            "VIDEO_PLATFORM_SDR_REPORT=FAIL package={} status={} stderr={} stdout={}",
            package,
            output.status,
            stderr,
            stdout,
        )
    };
    println!("{line}");
    log_line(dump, &line)?;
    Ok(())
}

pub fn tls_service_discovery_probe(config: TlsHandshakeProbeConfig) -> io::Result<()> {
    AAP_PEER_SESSION_STARTED.store(false, Ordering::SeqCst);

    install_session_signal_handlers();
    set_session_log_level(&config.log_level);
    println!("arda aap service discovery probe");
    println!("transport={}", config.transport);
    if config.transport != "tcp" {
        println!("usb_path={}", config.usb_path.display());
    } else {
        println!("addr={}", config.addr);
    }
    println!("timeout_ms={}", config.timeout_ms);
    println!(
        "host_liveness_timeout_ms={}",
        config.host_liveness_timeout_ms
    );
    println!("max_bytes={}", config.max_bytes);
    println!("max_frames={}", config.max_frames);
    println!("response={}.{} status=0x{:04x}", config.response_major, config.response_minor, config.response_status);
    println!("keys={}", config.keys_path.display());
    println!("log_level={}", normalize_session_log_level(&config.log_level));
    if let Some(path) = &config.dump_path {
        println!("dump={}", path.display());
    } else {
        println!("dump=none");
    }
    println!();

    let (mut stream, local_label, peer_label) = open_session_stream(&config)?;
    let peer = peer_label.as_str();
    println!("transport ready: local={local_label} peer={peer}");
    stream.set_read_timeout(Some(Duration::from_millis(config.timeout_ms)))?;
    stream.set_write_timeout(Some(Duration::from_millis(config.timeout_ms)))?;

    let mut dump: Option<Box<dyn Write>> = match &config.dump_path {
        Some(path) => Some(Box::new(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)?,
        )),
        None => None,
    };

    log_line(&mut dump, "arda aap service discovery probe")?;
    log_line(&mut dump, &format!("transport={}", config.transport))?;
    log_line(&mut dump, &format!("local={local_label}"))?;
    log_line(&mut dump, &format!("peer={peer}"))?;
    log_line(
        &mut dump,
        &format!(
            "host_liveness_timeout_ms={}",
            config.host_liveness_timeout_ms
        ),
    )?;
    log_line(&mut dump, &format!("response={}.{} status=0x{:04x}", config.response_major, config.response_minor, config.response_status))?;
    log_line(&mut dump, &format!("keys={}", config.keys_path.display()))?;
    if let Some(path) = &config.video_file_path {
        log_line(&mut dump, &format!("video_file={}", path.display()))?;
        log_line(&mut dump, &format!("video_frame_limit={}", config.video_frame_limit))?;
    } else {
        log_line(&mut dump, "video_file=none")?;
    }
    if config.video_native {
        log_line(
            &mut dump,
            &format!(
                "video_native=true size={}x{} bit_rate={} fps={} density={} frame_limit={} auto_config={} policy={} apply_wm={} wm_size_mode={} wm_density_mode={} wm_settle_ms={} video_start_timing={} first_frame_timeout_ms={} first_frame_max_restarts={} native_prewarm_ms={} display_auto={} wm_rotation={} wm_force_geometry={} wm_size_orientation={}",
                config.video_width,
                config.video_height,
                config.video_bit_rate,
                config.video_fps,
                config.video_density,
                config.video_frame_limit,
                config.video_auto_config,
                config.video_config_policy,
                config.video_apply_wm_config,
                config.video_wm_size_mode,
                config.video_wm_density_mode,
                config.video_wm_settle_ms,
                config.video_start_timing,
                config.video_first_frame_timeout_ms,
                config.video_first_frame_max_restarts,
                config.video_native_prewarm_ms,
                config.video_display_auto,
                config.video_wm_rotation,
                config.video_wm_force_geometry,
                config.video_wm_size_orientation
            ),
        )?;
    } else {
        log_line(&mut dump, "video_native=false")?;
    }
    log_line(
        &mut dump,
        &format!(
            "input_open={} input_key_binding={} input_inject={} input_inject_mode={} input_app_move_min_ms={} input_key_inject={}",
            config.input_open,
            config.input_key_binding,
            config.input_inject,
            config.input_inject_mode,
            config.input_app_move_min_ms,
            config.input_key_inject
        ),
    )?;
    log_line(
        &mut dump,
        &format!(
            "audio_open={} audio_stream_filter={} audio_dump_dir={} audio_ack={}",
            config.audio_open,
            config.audio_stream_filter,
            config.audio_dump_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "none".to_string()),
            config.audio_ack
        ),
    )?;
    log_line(&mut dump, &format!("audio_test_tone={} tone_hz={} chunk_ms={} volume={}", config.audio_test_tone, config.audio_tone_hz, config.audio_tone_chunk_ms, config.audio_tone_volume))?;
    log_line(&mut dump, &format!("audio_capture={} source={} chunk_ms={} gain={} as={} package={}", config.audio_capture, config.audio_capture_source, config.audio_capture_chunk_ms, config.audio_capture_gain, config.audio_capture_as, config.audio_capture_package))?;
    log_line(&mut dump, &format!("mic_open={} mic_send_request_open={} mic_dump_path={} mic_ack={} mic_ack_mode={} mic_max_unacked={}", config.mic_open, config.mic_send_request_open, config.mic_dump_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "none".to_string()), config.mic_ack, config.mic_ack_mode, config.mic_max_unacked))?;
    log_line(&mut dump, &format!("quiet_ping_logs={} log_level={}", config.quiet_ping_logs, normalize_session_log_level(&config.log_level)))?;
    log_line(&mut dump, &format!("role=phone-like AAP endpoint over {} + TLS server; sends encrypted ServiceDiscoveryRequest after AUTH_COMPLETE", config.transport))?;
    log_line(&mut dump, "")?;

    let started = Instant::now();
    let mut reader = FrameReader::new(config.max_bytes);

    let version = match reader.read_frame(&mut stream) {
        Ok(Some(frame)) => frame,
        Ok(None) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "peer closed before version request")),
        Err(err) => return Err(err),
    };

    println!("rx version candidate:");
    describe_frame(1, HEADER_LENGTH + version.payload.len() + if version.final_length.is_some() { 4 } else { 0 }, peer, &version, &mut io::stdout())?;
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "rx version candidate:")?;
        describe_frame(1, HEADER_LENGTH + version.payload.len() + if version.final_length.is_some() { 4 } else { 0 }, peer, &version, dump)?;
        dump.flush()?;
    }

    let Some((req_major, req_minor)) = parse_version_request(&version) else {
        log_line(&mut dump, "not a valid MESSAGE_VERSION_REQUEST; closing")?;
        return Err(io::Error::new(io::ErrorKind::InvalidData, "expected MESSAGE_VERSION_REQUEST on control channel"));
    };

    println!("version request: {}.{}", req_major, req_minor);
    log_line(&mut dump, &format!("version request: {}.{}", req_major, req_minor))?;

    crate::aap_key_update::prepare_aap_attempt();
    AAP_PEER_SESSION_STARTED.store(true, Ordering::SeqCst);
    println!("AAP_PEER_SESSION_STARTED=YES version={}.{}", req_major, req_minor);
    log_line(
        &mut dump,
        &format!("AAP_PEER_SESSION_STARTED=YES version={}.{}", req_major, req_minor),
    )?;

    let response_payload = build_version_response(config.response_major, config.response_minor, config.response_status);
    let response_frame = AapFrame {
        channel: 0,
        flags: FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
        final_length: None,
        payload: response_payload,
    };
    write_frame(&mut stream, &response_frame)?;
    stream.flush()?;

    println!("tx version response:");
    describe_frame(1, HEADER_LENGTH + response_frame.payload.len(), peer, &response_frame, &mut io::stdout())?;
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "tx version response:")?;
        describe_frame(1, HEADER_LENGTH + response_frame.payload.len(), peer, &response_frame, dump)?;
        dump.flush()?;
    }

    let mut tls = tls::AapTlsServer::new_phone_like(&config.keys_path)?;
    println!("tls server ready: {}", tls.state_string());
    log_line(&mut dump, &format!("tls server ready: {}", tls.state_string()))?;

    let mut frames = 1usize;
    let mut bytes = HEADER_LENGTH + version.payload.len() + HEADER_LENGTH + response_frame.payload.len();
    let mut rx_ssl_frames = 0usize;
    let mut tx_ssl_frames = 0usize;
    let mut encrypted_frames = 0usize;
    let mut decrypted_frames = 0usize;
    let mut decrypted_bytes = 0usize;
    let mut service_request_sent = false;
    let mut channel_open_sent = false;
    let mut selected_video_service_id: Option<i32> = None;
    let mut selected_input_service_id: Option<i32> = None;
    let mut selected_input_source: Option<proto::InputSourceSelection> = None;
    let mut selected_video_config_for_input: Option<proto::VideoConfigSelection> = None;
    let mut input_touch_map: Option<InputTouchMap> = None;
    let mut input_touch_injector: Option<TouchInjector> = None;
    let mut input_app_injector: Option<AppProcessInputInjector> = None;
    let mut input_bridge_injector: Option<PlatformInputBridgeInjector> = None;
    let mut input_touch_inject_events = 0usize;
    let mut input_app_move_drop_events = 0usize;
    let mut last_app_move_inject_at: Option<std::time::Instant> = None;
    let mut input_key_inject_events = 0usize;
    let mut input_channel_open_sent = false;
    let mut input_channel_opened = false;
    let mut input_key_binding_sent = false;
    let mut video_channel_opened = false;
    let mut video_setup_sent = false;
    let mut video_start_sent = false;
    let mut video_max_unacked: u32 = 1;
    let video_session_id: i32 = 1;
    let mut audio_output_channels: Vec<AudioOutputChannelState> = Vec::new();
    let mut mic_source_channel: Option<MicSourceChannelState> = None;
    let mut sensor_channel: Option<SensorChannelState> = None;
    let mut sensor_os_forwarder = SensorOsForwarder::new(&config);
    let mut sensor_pipeline: Option<SensorPipeline> = None;
    if config.sensor_open {
        match SensorPipeline::new(SensorPipelineConfig {
            wasm_spec: config.sensor_wasm_path.clone(),
            fuel_per_event: config.sensor_wasm_fuel,
            max_commands_per_event: config.sensor_wasm_max_commands,
            vhal_enabled: config.sensor_forward_vhal,
            vhal_min_interval_ms: config.sensor_vhal_min_ms,
        }) {
            Ok((pipeline, ready)) => {
                println!("{ready}");
                log_line(&mut dump, &ready)?;
                sensor_pipeline = Some(pipeline);
            }
            Err(err) => {
                let line = format!(
                    "SENSOR_WASM_ENGINE disabled error={} vhal_fail_closed=true sensor_decode_continues=true",
                    err
                );
                println!("{line}");
                log_line(&mut dump, &line)?;
            }
        }
    }
    let mut audio_capture: Option<AppProcessAudioCapture> = None;
    let mut audio_capture_buffer: Vec<u8> = Vec::new();
    let mut audio_channel_open_requests_sent = 0usize;
    let mut audio_data_packets = 0u64;
    let mut audio_data_bytes = 0u64;
    let mut mic_channel_open_requests_sent = 0usize;
    let mut mic_data_packets = 0u64;
    let mut mic_data_bytes = 0u64;
    let mut sensor_channel_open_requests_sent = 0usize;
    let mut sensor_requests_sent = 0u64;
    let mut sensor_batches_seen = 0u64;
    let mut sensor_errors_seen = 0u64;
    let mut video_sender: Option<VideoSender> = if config.video_native {
        Some(VideoSender::Live(VideoLiveSender::new(VideoLiveConfig {
            width: config.video_width,
            height: config.video_height,
            bit_rate: config.video_bit_rate,
            fps: config.video_fps,
            density: config.video_density,
            frame_limit: config.video_frame_limit,
            apply_wm_config: config.video_apply_wm_config,
            wm_size_mode: config.video_wm_size_mode.clone(),
            wm_density_mode: config.video_wm_density_mode.clone(),
            wm_settle_ms: config.video_wm_settle_ms,
            startup_timing: config.video_start_timing,
            first_frame_timeout_ms: config.video_first_frame_timeout_ms,
            first_frame_max_restarts: config.video_first_frame_max_restarts,
            native_prewarm_ms: config.video_native_prewarm_ms,
            first_frame_kick: config.video_first_frame_kick,
            first_frame_kick_delay_ms: config.video_first_frame_kick_delay_ms,
            first_frame_kick_mode: config.video_first_frame_kick_mode.clone(),
            first_frame_kick_x: config.video_first_frame_kick_x,
            first_frame_kick_y: config.video_first_frame_kick_y,
            first_frame_kick_keycode: config.video_first_frame_kick_keycode,
            display_auto: config.video_display_auto,
            wm_rotation: config.video_wm_rotation.clone(),
            wm_force_geometry: config.video_wm_force_geometry,
            wm_size_orientation: config.video_wm_size_orientation.clone(),
        })))
    } else {
        match &config.video_file_path {
            Some(path) => Some(VideoSender::File(VideoFileSender::load(path.clone(), config.video_frame_limit)?)),
            None => None,
        }
    };
    let mut handshake_done_logged = false;
    let mut decrypted_fragments = fragment::FragmentAssembler::default();
    // Once media START is sent, the video sender must be pumped even when the
    // HU is quiet.  Otherwise live video only advances on incoming ping/focus
    // frames and appears frozen after the first IDR.
    let mut video_pump_timeout_enabled = false;
    let mut last_hu_activity_at: Option<Instant> = None;
    let mut cleanup_reason = "session-end";

    while config.max_frames == 0 || frames < config.max_frames {
        if session_shutdown_requested() {
            cleanup_reason = "signal";
            println!("shutdown requested; stopping AAP session");
            log_line(&mut dump, "shutdown requested; stopping AAP session")?;
            break;
        }

        let frame = match reader.read_frame(&mut stream) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                cleanup_reason = "peer-disconnect";
                println!("client disconnected after frames={frames} service_request_sent={service_request_sent} bytes={bytes}");
                log_line(&mut dump, &format!("client disconnected after frames={frames} service_request_sent={service_request_sent} bytes={bytes}"))?;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted && session_shutdown_requested() => {
                cleanup_reason = "signal";
                println!("read interrupted by shutdown request; stopping AAP session");
                log_line(&mut dump, "read interrupted by shutdown request; stopping AAP session")?;
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.kind() == io::ErrorKind::TimedOut =>
            {
                if video_start_sent && config.host_liveness_timeout_ms > 0 {
                    if let Some(last_activity) = last_hu_activity_at.as_ref() {
                        let elapsed = last_activity.elapsed();
                        if elapsed >= Duration::from_millis(config.host_liveness_timeout_ms) {
                            cleanup_reason = "host-liveness-timeout";
                            let line = format!(
                                "AAP_HOST_LIVENESS_TIMEOUT elapsed_ms={} timeout_ms={} frames={} video_start_sent={} video_channel_opened={}",
                                elapsed.as_millis(),
                                config.host_liveness_timeout_ms,
                                frames,
                                video_start_sent,
                                video_channel_opened
                            );
                            println!("{line}");
                            log_line(&mut dump, &line)?;
                            break;
                        }
                    }
                }

                let audio_pump_active = (config.audio_test_tone || config.audio_capture) && audio_output_channels.iter().any(|audio| audio.start_sent);
                if video_start_sent || audio_pump_active {
                    let mut sent_any = 0usize;
                    if video_start_sent {
                        if let (Some(sender), Some(video_channel)) = (video_sender.as_mut(), selected_video_service_id.map(|id| id as u8)) {
                            let sent = sender.send_available(
                                &mut stream,
                                &mut tls,
                                video_channel,
                                video_session_id,
                                video_max_unacked,
                                &mut dump,
                            )?;
                            tx_ssl_frames += sent;
                            sent_any += sent;
                        }
                    }
                    let sent = pump_audio_test_tone(
                        &mut audio_output_channels,
                        &mut stream,
                        &mut tls,
                        &config,
                        &mut dump,
                    )?;
                    tx_ssl_frames += sent;
                    sent_any += sent;
                    let sent = pump_audio_capture(
                        &mut audio_output_channels,
                        &mut audio_capture,
                        &mut audio_capture_buffer,
                        &mut stream,
                        &mut tls,
                        &config,
                        &mut dump,
                    )?;
                    tx_ssl_frames += sent;
                    sent_any += sent;
                    if sent_any == 0 {
                        // Avoid a busy loop when capture/audio has nothing ready or flow control
                        // is waiting for the next ACK.
                        thread::sleep(Duration::from_millis(5));
                    }
                    continue;
                }
                cleanup_reason = "read-timeout";
                println!("read timeout after frames={frames} service_request_sent={service_request_sent} bytes={bytes}");
                log_line(&mut dump, &format!("read timeout after frames={frames} service_request_sent={service_request_sent} bytes={bytes}"))?;
                break;
            }
            Err(err) => return Err(err),
        };

        frames += 1;
        if video_start_sent {
            last_hu_activity_at = Some(Instant::now());
        }
        bytes += HEADER_LENGTH + frame.payload.len() + if frame.final_length.is_some() { 4 } else { 0 };
        let likely_encrypted_ping = config.quiet_ping_logs
            && frame.channel == 0
            && (frame.flags & FLAG_ENCRYPTED) != 0
            && frame.final_length.is_none()
            && frame.payload.len() <= 64;
        if !likely_encrypted_ping && session_log_enabled(LOG_VERBOSE) {
            println!("rx frame:");
            describe_frame(frames, bytes, peer, &frame, &mut io::stdout())?;
            if let Some(dump) = dump.as_mut() {
                writeln!(dump, "rx frame:")?;
                describe_frame(frames, bytes, peer, &frame, dump)?;
                dump.flush()?;
            }
        }

        let mut plaintext_chunks: Vec<Vec<u8>> = Vec::new();

        if tls.is_handshaking() {
            let Some(tls_bytes) = ssl_payload_bytes(&frame) else {
                let line = "non MESSAGE_ENCAPSULATED_SSL frame while TLS is still handshaking; ignoring";
                println!("{line}");
                log_line(&mut dump, line)?;
                continue;
            };

            rx_ssl_frames += 1;
            let hint = tls_record_hint(tls_bytes);
            println!("rx ssl handshake frame #{rx_ssl_frames}: tls_bytes={} {hint}", tls_bytes.len());
            log_line(&mut dump, &format!("rx ssl handshake frame #{rx_ssl_frames}: tls_bytes={} {hint}", tls_bytes.len()))?;

            let step = tls.feed_tls(tls_bytes)?;
            log_tls_step(&mut dump, &step)?;

            if !step.plaintext.is_empty() {
                plaintext_chunks.push(step.plaintext);
            }

            if !step.outgoing_tls.is_empty() {
                tx_ssl_frames += 1;
                let tx_frame = build_ssl_frame(&step.outgoing_tls);
                write_frame(&mut stream, &tx_frame)?;
                stream.flush()?;
                println!("tx ssl handshake frame #{tx_ssl_frames}: tls_bytes={}", step.outgoing_tls.len());
                log_line(&mut dump, &format!("tx ssl handshake frame #{tx_ssl_frames}: tls_bytes={} {}", step.outgoing_tls.len(), tls_record_hint(&step.outgoing_tls)))?;
            }

            if !tls.is_handshaking() && !handshake_done_logged {
                handshake_done_logged = true;
                println!("tls handshake complete: cipher={}", tls.cipher_suite());
                log_line(&mut dump, &format!("tls handshake complete: cipher={}", tls.cipher_suite()))?;
                println!("waiting for AUTH_COMPLETE then sending ServiceDiscoveryRequest...");
                log_line(&mut dump, "waiting for AUTH_COMPLETE then sending ServiceDiscoveryRequest...")?;
            }
        } else if (frame.flags & FLAG_ENCRYPTED) == 0 {
            let line = "post-TLS frame is not marked encrypted; logging plaintext as-is";
            println!("{line}");
            log_line(&mut dump, line)?;
            plaintext_chunks.push(frame.payload.clone());
        } else {
            encrypted_frames += 1;
            if !likely_encrypted_ping && session_log_enabled(LOG_VERBOSE) {
                println!("rx encrypted AA frame #{encrypted_frames}: tls_bytes={} {}", frame.payload.len(), tls_record_hint(&frame.payload));
                log_line(&mut dump, &format!("rx encrypted AA frame #{encrypted_frames}: tls_bytes={} {}", frame.payload.len(), tls_record_hint(&frame.payload)))?;
            }

            let step = tls.feed_tls(&frame.payload)?;
            if !likely_encrypted_ping && session_log_enabled(LOG_VERBOSE) {
                log_tls_step(&mut dump, &step)?;
            }
            if !step.plaintext.is_empty() {
                plaintext_chunks.push(step.plaintext);
            }
            if !step.outgoing_tls.is_empty() {
                tx_ssl_frames += 1;
                let tx_frame = AapFrame {
                    channel: frame.channel,
                    flags: (frame.flags | FLAG_ENCRYPTED) & !FLAG_CONTROL,
                    final_length: None,
                    payload: step.outgoing_tls,
                };
                write_frame(&mut stream, &tx_frame)?;
                stream.flush()?;
                if session_log_enabled(LOG_VERBOSE) {
                    println!("tx encrypted TLS maintenance frame #{tx_ssl_frames}: tls_bytes={}", tx_frame.payload.len());
                    log_line(&mut dump, &format!("tx encrypted TLS maintenance frame #{tx_ssl_frames}: tls_bytes={} {}", tx_frame.payload.len(), tls_record_hint(&tx_frame.payload)))?;
                }
            }
        }

        for plaintext in plaintext_chunks {
            if plaintext.is_empty() {
                continue;
            }

            let plaintext_frame = AapFrame {
                channel: frame.channel,
                flags: frame.flags & !FLAG_ENCRYPTED,
                final_length: frame.final_length,
                payload: plaintext,
            };
            let reassembled = decrypted_fragments.push(&plaintext_frame);
            log_reassembly_wait_or_drop(&mut dump, frame.channel, &reassembled)?;
            let plaintext = match reassembled {
                fragment::ReassemblyResult::Complete { payload, fragments, expected_len } => {
                    if fragments > 1 && session_log_enabled(LOG_VERBOSE) {
                        let line = format!(
                            "decrypted AA full-frame complete: channel=0x{:02x} fragments={} expected_len={:?} actual_len={}",
                            frame.channel,
                            fragments,
                            expected_len,
                            payload.len()
                        );
                        println!("{line}");
                        log_line(&mut dump, &line)?;
                    }
                    payload
                }
                fragment::ReassemblyResult::Waiting { .. } | fragment::ReassemblyResult::Dropped(_) => {
                    continue;
                }
            };

            decrypted_frames += 1;
            decrypted_bytes += plaintext.len();
            let quiet_ping_payload = config.quiet_ping_logs
                && frame.channel == 0
                && (is_ping_request_payload(&plaintext) || is_ping_response_payload(&plaintext));
            if !quiet_ping_payload {
                log_decrypted_payload(&mut dump, peer, frame.channel, frame.flags, decrypted_frames, &plaintext)?;
            }

            if !service_request_sent && is_control_message(&plaintext, 0x0004) {
                crate::aap_key_update::confirm_aap_authenticated();
                println!("AUTH_COMPLETE observed; sending encrypted ServiceDiscoveryRequest");
                log_line(&mut dump, "AUTH_COMPLETE observed; sending encrypted ServiceDiscoveryRequest")?;
                let control_payload = proto::build_service_discovery_request_control_payload(
                    "Arda",
                    "Arda AAOS Sender",
                )
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                log_line(&mut dump, &format!("tx ServiceDiscoveryRequest plaintext bytes={}", control_payload.len()))?;
                if let Some(dump) = dump.as_mut() {
                    let pseudo = AapFrame {
                        channel: 0,
                        flags: FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
                        final_length: None,
                        payload: control_payload.clone(),
                    };
                    describe_frame(0, 0, peer, &pseudo, dump)?;
                    dump.flush()?;
                }

                let tx_count = write_encrypted_plaintext_fragmented(
                    &mut stream,
                    &mut tls,
                    0,
                    &control_payload,
                    &mut dump,
                    "ServiceDiscoveryRequest",
                )?;
                service_request_sent = true;
                tx_ssl_frames += tx_count;
                println!("tx encrypted ServiceDiscoveryRequest frame(s): count={tx_count}");
                log_line(&mut dump, &format!("tx encrypted ServiceDiscoveryRequest frame(s): count={tx_count}"))?;
            }

            if !channel_open_sent {
                if let Some(summary) = proto::parse_service_discovery_response_control_payload(&plaintext, &config.video_config_policy)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
                {
                    println!("SERVICE_DISCOVERY_RESPONSE observed; parsed {} detail line(s)", summary.details.len());
                    log_line(&mut dump, &format!("SERVICE_DISCOVERY_RESPONSE observed; parsed {} detail line(s)", summary.details.len()))?;
                    for line in &summary.details {
                        println!("{line}");
                        log_line(&mut dump, line)?;
                    }

                    let input_candidate = summary.input_source.clone();
                    let audio_candidates = summary.audio_media_sinks.clone();
                    let mic_candidate = summary.media_source.clone();
                    let sensor_candidate = summary.sensor_source.clone();
                    if config.audio_open || config.mic_open {
                        println!(
                            "selected audio candidates={} filter={} media_source={:?}",
                            audio_candidates.len(),
                            config.audio_stream_filter,
                            summary.media_source
                        );
                        log_line(&mut dump, &format!(
                            "selected audio candidates={} filter={} media_source={:?}",
                            audio_candidates.len(),
                            config.audio_stream_filter,
                            summary.media_source
                        ))?;
                    }
                    if config.sensor_open {
                        println!("selected sensor source={:?}", sensor_candidate);
                        log_line(&mut dump, &format!("selected sensor source={:?}", sensor_candidate))?;
                    }
                    if let Some(input) = input_candidate.as_ref() {
                        let touchscreens = input
                            .touchscreens
                            .iter()
                            .map(|ts| format!("#{}:{}x{}:{}:secondary={}", ts.index, ts.width, ts.height, ts.type_name, ts.is_secondary))
                            .collect::<Vec<_>>()
                            .join(",");
                        let touchpads = input
                            .touchpads
                            .iter()
                            .map(|tp| format!("#{}:{}x{}:phys={}x{}:nav={}:abs={}:tap={}:sens={}", tp.index, tp.width, tp.height, tp.physical_width, tp.physical_height, tp.ui_navigation, tp.ui_absolute, tp.tap_as_select, tp.sensitivity))
                            .collect::<Vec<_>>()
                            .join(",");
                        let keycodes = input
                            .keycodes_supported
                            .iter()
                            .map(|code| format!("{}({})", code, proto::keycode_name(*code as u32)))
                            .collect::<Vec<_>>()
                            .join(",");
                        println!(
                            "selected input InputSourceService id={} display_id={} touchscreens=[{}] touchpads=[{}] keycodes=[{}] feedback=[{}]",
                            input.service_id,
                            input.display_id,
                            touchscreens,
                            touchpads,
                            keycodes,
                            input.feedback_events_supported.join(",")
                        );
                        log_line(
                            &mut dump,
                            &format!(
                                "selected input InputSourceService id={} display_id={} touchscreens=[{}] touchpads=[{}] keycodes=[{}] feedback=[{}]",
                                input.service_id,
                                input.display_id,
                                touchscreens,
                                touchpads,
                                keycodes,
                                input.feedback_events_supported.join(",")
                            ),
                        )?;
                    }

                    if let Some(video) = summary.video_media_sink {
                        selected_video_service_id = Some(video.service_id);
                        selected_video_config_for_input = video.selected_video_config.clone();
                        println!(
                            "selected video MediaSinkService id={} type={} display_id={} display_type={} video_configs={} first_config={:?}",
                            video.service_id,
                            video.available_type,
                            video.display_id,
                            video.display_type,
                            video.video_configs,
                            video.first_video_config
                        );
                        log_line(
                            &mut dump,
                            &format!(
                                "selected video MediaSinkService id={} type={} display_id={} display_type={} video_configs={} first_config={:?}",
                                video.service_id,
                                video.available_type,
                                video.display_id,
                                video.display_type,
                                video.video_configs,
                                video.first_video_config
                            ),
                        )?;

                        if let Err(error) = report_video_sdr_to_platform(
                            &video,
                            &config.video_config_policy,
                            &mut dump,
                        ) {
                            let line = format!(
                                "VIDEO_PLATFORM_SDR_REPORT=FAIL error={error}",
                            );
                            println!("{line}");
                            log_line(&mut dump, &line)?;
                        }

                        if config.video_auto_config {
                            if let Some(vc) = video.selected_video_config.as_ref() {
                                let line = format!(
                                    "VIDEO_AUTO_CONFIG selected from SDR: policy={} service_id={} config_index={} stream={}x{} fps={} density={} real_density={} total_margins={}x{} margins=L{} R{} T{} B{} content={}x{} safe={}x{} content_insets=L{} R{} T{} B{} codec={} resolution={}",
                                    config.video_config_policy,
                                    video.service_id,
                                    vc.config_index,
                                    vc.width,
                                    vc.height,
                                    vc.fps,
                                    vc.density,
                                    vc.real_density,
                                    vc.width_margin,
                                    vc.height_margin,
                                    vc.margin_left,
                                    vc.margin_right,
                                    vc.margin_top,
                                    vc.margin_bottom,
                                    vc.content_width,
                                    vc.content_height,
                                    vc.safe_width,
                                    vc.safe_height,
                                    vc.content_inset_left,
                                    vc.content_inset_right,
                                    vc.content_inset_top,
                                    vc.content_inset_bottom,
                                    vc.codec,
                                    vc.resolution
                                );
                                println!("{line}");
                                log_line(&mut dump, &line)?;
                                if let Some(sender) = video_sender.as_mut() {
                                    sender.configure_from_sdr(vc, &mut dump)?;
                                }
                            } else {
                                let line = "VIDEO_AUTO_CONFIG requested but no concrete video config was parsed from SDR";
                                println!("{line}");
                                log_line(&mut dump, line)?;
                            }
                        } else if let Some(vc) = video.selected_video_config.as_ref() {
                            let line = format!(
                                "VIDEO_AUTO_CONFIG disabled: keeping manual native size={}x{} fps={} density={} even though SDR selected config_index={} stream={}x{} fps={} density={} resolution={}",
                                config.video_width,
                                config.video_height,
                                config.video_fps,
                                config.video_density,
                                vc.config_index,
                                vc.width,
                                vc.height,
                                vc.fps,
                                vc.density,
                                vc.resolution
                            );
                            println!("{line}");
                            log_line(&mut dump, &line)?;
                        }

                        let control_payload = proto::build_channel_open_request_control_payload(video.service_id, 0)
                            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                        log_line(&mut dump, &format!("tx ChannelOpenRequest plaintext bytes={} service_id={} priority=0", control_payload.len(), video.service_id))?;
                        // CHANNEL_OPEN_REQUEST is a service-channel control frame, not a
                        // channel-0 control message. DHU rejects it on channel 0 with
                        // MESSAGE_UNEXPECTED_MESSAGE. Real AA/DHU traces use:
                        //   channel = service_id
                        //   flags   = ENCRYPTED | CONTROL | FIRST | LAST = 0x0f
                        if let Some(dump) = dump.as_mut() {
                            let pseudo = AapFrame {
                                channel: video.service_id as u8,
                                flags: FLAG_CONTROL | FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
                                final_length: None,
                                payload: control_payload.clone(),
                            };
                            describe_frame(0, 0, peer, &pseudo, dump)?;
                            dump.flush()?;
                        }

                        let tx_count = write_encrypted_plaintext_fragmented_with_flags(
                            &mut stream,
                            &mut tls,
                            video.service_id as u8,
                            FLAG_CONTROL,
                            &control_payload,
                            &mut dump,
                            "CHANNEL_OPEN_REQUEST",
                        )?;
                        channel_open_sent = true;
                        tx_ssl_frames += tx_count;
                        println!("tx encrypted CHANNEL_OPEN_REQUEST frame(s): count={tx_count} channel=0x{:02x} service_id={} flags=ENCRYPTED|CONTROL", video.service_id as u8, video.service_id);
                        log_line(&mut dump, &format!("tx encrypted CHANNEL_OPEN_REQUEST frame(s): count={tx_count} channel=0x{:02x} service_id={} flags=ENCRYPTED|CONTROL", video.service_id as u8, video.service_id))?;
                    } else {
                        println!("SERVICE_DISCOVERY_RESPONSE has no video MediaSinkService candidate yet");
                        log_line(&mut dump, "SERVICE_DISCOVERY_RESPONSE has no video MediaSinkService candidate yet")?;
                        channel_open_sent = true;
                    }
                    if config.input_open {
                        if let Some(input) = input_candidate.clone() {
                            selected_input_service_id = Some(input.service_id);
                            selected_input_source = Some(input.clone());
                            let map = InputTouchMap::from_config(&config, Some(&input), selected_video_config_for_input.as_ref());
                            let line = format!(
                                "INPUT_TOUCH_MAP source={}x{} target={}x{} mode={} offset={}x{} stream={}x{}",
                                map.source_width,
                                map.source_height,
                                map.target_width,
                                map.target_height,
                                map.mode.as_str(),
                                map.x_offset,
                                map.y_offset,
                                map.stream_width,
                                map.stream_height
                            );
                            println!("{line}");
                            log_line(&mut dump, &line)?;
                            input_touch_map = Some(map.clone());

                            // Do not start app_process here. AAOS can get unstable if framework
                            // input is armed before video/input focus has settled. App mode now
                            // starts lazily on the first injected event; uinput still prepares
                            // early because it only creates a virtual device.
                            if false
                                && (config.input_inject || config.input_key_inject)
                                && is_app_process_input_inject_mode(&config.input_inject_mode)
                                && input_app_injector.is_none()
                            {
                                match AppProcessInputInjector::create(input.display_id as i32) {
                                    Ok((injector, report)) => {
                                        for line in report.lines() {
                                            let line = format!("INPUT_APP_INJECTOR_PREPARE {line}");
                                            println!("{line}");
                                            log_line(&mut dump, &line)?;
                                        }
                                        input_app_injector = Some(injector);
                                    }
                                    Err(err) => {
                                        let line = format!("INPUT_APP_INJECT prepare failed: {err}");
                                        println!("{line}");
                                        log_line(&mut dump, &line)?;
                                    }
                                }
                            } else if config.input_inject
                                && !is_shell_touch_inject_mode(&config.input_inject_mode)
                                && !is_app_process_input_inject_mode(&config.input_inject_mode)
                                && !is_platform_input_bridge_mode(&config.input_inject_mode)
                                && input_touch_injector.is_none()
                            {
                                match TouchInjector::create(map.target_width, map.target_height) {
                                    Ok((injector, report)) => {
                                        for line in report.lines() {
                                            let line = format!("INPUT_TOUCH_INJECTOR_PREPARE {line}");
                                            println!("{line}");
                                            log_line(&mut dump, &line)?;
                                        }
                                        input_touch_injector = Some(injector);
                                    }
                                    Err(err) => {
                                        let line = format!("INPUT_TOUCH_INJECT prepare failed: {err}");
                                        println!("{line}");
                                        log_line(&mut dump, &line)?;
                                    }
                                }
                            }
                            let control_payload = proto::build_channel_open_request_control_payload(input.service_id, 0)
                                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                            log_line(&mut dump, &format!("tx Input ChannelOpenRequest plaintext bytes={} service_id={} priority=0", control_payload.len(), input.service_id))?;
                            if let Some(dump) = dump.as_mut() {
                                let pseudo = AapFrame {
                                    channel: input.service_id as u8,
                                    flags: FLAG_CONTROL | FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
                                    final_length: None,
                                    payload: control_payload.clone(),
                                };
                                describe_frame(0, 0, peer, &pseudo, dump)?;
                                dump.flush()?;
                            }

                            let tx_count = write_encrypted_plaintext_fragmented_with_flags(
                                &mut stream,
                                &mut tls,
                                input.service_id as u8,
                                FLAG_CONTROL,
                                &control_payload,
                                &mut dump,
                                "INPUT_CHANNEL_OPEN_REQUEST",
                            )?;
                            input_channel_open_sent = true;
                            tx_ssl_frames += tx_count;
                            println!("tx encrypted INPUT_CHANNEL_OPEN_REQUEST frame(s): count={tx_count} channel=0x{:02x} service_id={} flags=ENCRYPTED|CONTROL", input.service_id as u8, input.service_id);
                            log_line(&mut dump, &format!("tx encrypted INPUT_CHANNEL_OPEN_REQUEST frame(s): count={tx_count} channel=0x{:02x} service_id={} flags=ENCRYPTED|CONTROL", input.service_id as u8, input.service_id))?;
                        } else {
                            println!("INPUT_OPEN requested but SDR has no InputSourceService candidate");
                            log_line(&mut dump, "INPUT_OPEN requested but SDR has no InputSourceService candidate")?;
                        }
                    }
                    if config.audio_open {
                        for audio in audio_candidates {
                            if !audio_stream_matches(&config.audio_stream_filter, &audio.audio_stream_type) {
                                let line = format!(
                                    "AUDIO_OPEN skip service_id={} stream={} filter={}",
                                    audio.service_id, audio.audio_stream_type, config.audio_stream_filter
                                );
                                println!("{line}");
                                log_line(&mut dump, &line)?;
                                continue;
                            }
                            let first_cfg = audio.configs.first().cloned();
                            let line = if let Some(cfg) = first_cfg.as_ref() {
                                format!(
                                    "AUDIO_OPEN selected service_id={} stream={} type={} config_index={} rate={} bits={} channels={}",
                                    audio.service_id,
                                    audio.audio_stream_type,
                                    audio.available_type,
                                    cfg.config_index,
                                    cfg.sampling_rate,
                                    cfg.bits,
                                    cfg.channels
                                )
                            } else {
                                format!(
                                    "AUDIO_OPEN selected service_id={} stream={} type={} config=none",
                                    audio.service_id, audio.audio_stream_type, audio.available_type
                                )
                            };
                            println!("{line}");
                            log_line(&mut dump, &line)?;

                            let dump_file = match make_audio_dump_file(&config.audio_dump_dir, audio.service_id, &audio.audio_stream_type, first_cfg.as_ref()) {
                                Ok(file) => file,
                                Err(err) => {
                                    let line = format!("AUDIO_DUMP open failed service_id={}: {err}", audio.service_id);
                                    println!("{line}");
                                    log_line(&mut dump, &line)?;
                                    None
                                }
                            };

                            let control_payload = proto::build_channel_open_request_control_payload(audio.service_id, 0)
                                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                            let tx_count = write_encrypted_plaintext_fragmented_with_flags(
                                &mut stream,
                                &mut tls,
                                audio.service_id as u8,
                                FLAG_CONTROL,
                                &control_payload,
                                &mut dump,
                                "AUDIO_CHANNEL_OPEN_REQUEST",
                            )?;
                            tx_ssl_frames += tx_count;
                            audio_channel_open_requests_sent += 1;
                            println!("tx encrypted AUDIO_CHANNEL_OPEN_REQUEST frame(s): count={tx_count} channel=0x{:02x} service_id={} stream={}", audio.service_id as u8, audio.service_id, audio.audio_stream_type);
                            log_line(&mut dump, &format!("tx encrypted AUDIO_CHANNEL_OPEN_REQUEST frame(s): count={tx_count} channel=0x{:02x} service_id={} stream={}", audio.service_id as u8, audio.service_id, audio.audio_stream_type))?;
                            audio_output_channels.push(AudioOutputChannelState {
                                service_id: audio.service_id,
                                channel: audio.service_id as u8,
                                stream_type: audio.audio_stream_type,
                                config: first_cfg,
                                opened: false,
                                setup_sent: false,
                                start_sent: false,
                                session_id: 100 + audio.service_id,
                                max_unacked: 1,
                                ack_counter: 0,
                                data_packets: 0,
                                data_bytes: 0,
                                tx_unacked: 0,
                                tx_packets: 0,
                                tx_bytes: 0,
                                tx_next_pts_us: 0,
                                tx_phase: 0.0,
                                tx_next_due: None,
                                tx_first_logged: false,
                                first_data_logged: false,
                                dump_file,
                            });
                        }
                    }
                    if config.mic_open {
                        if let Some(source) = mic_candidate.clone() {
                            let cfg_text = source.config.as_ref()
                                .map(|cfg| format!("config_index={} rate={} bits={} channels={}", cfg.config_index, cfg.sampling_rate, cfg.bits, cfg.channels))
                                .unwrap_or_else(|| "config=none".to_string());
                            let line = format!(
                                "MIC_SOURCE_OPEN selected service_id={} type={} {}",
                                source.service_id, source.available_type, cfg_text
                            );
                            println!("{line}");
                            log_line(&mut dump, &line)?;

                            let dump_file = match make_mic_dump_file(&config.mic_dump_path) {
                                Ok(file) => file,
                                Err(err) => {
                                    let line = format!("MIC_DUMP open failed: {err}");
                                    println!("{line}");
                                    log_line(&mut dump, &line)?;
                                    None
                                }
                            };

                            let control_payload = proto::build_channel_open_request_control_payload(source.service_id, 0)
                                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                            let tx_count = write_encrypted_plaintext_fragmented_with_flags(
                                &mut stream,
                                &mut tls,
                                source.service_id as u8,
                                FLAG_CONTROL,
                                &control_payload,
                                &mut dump,
                                "MIC_CHANNEL_OPEN_REQUEST",
                            )?;
                            tx_ssl_frames += tx_count;
                            mic_channel_open_requests_sent += 1;
                            println!("tx encrypted MIC_CHANNEL_OPEN_REQUEST frame(s): count={tx_count} channel=0x{:02x} service_id={}", source.service_id as u8, source.service_id);
                            log_line(&mut dump, &format!("tx encrypted MIC_CHANNEL_OPEN_REQUEST frame(s): count={tx_count} channel=0x{:02x} service_id={}", source.service_id as u8, source.service_id))?;
                            mic_source_channel = Some(MicSourceChannelState {
                                service_id: source.service_id,
                                channel: source.service_id as u8,
                                config: source.config,
                                opened: false,
                                setup_sent: false,
                                start_sent: false,
                                mic_request_sent: false,
                                mic_response_seen: false,
                                // For HU->device media_source the HU owns the media session id.
                                // DHU currently replies with session_id=0; using our synthetic
                                // local id in MEDIA_ACK makes DHU stop after max_unacked packets.
                                session_id: 0,
                                max_unacked: 1,
                                ack_counter: 0,
                                ack_window_counter: 0,
                                data_packets: 0,
                                data_bytes: 0,
                                first_data_logged: false,
                                first_ack_logged: false,
                                level_last_log: Instant::now(),
                                level_first_logged: false,
                                level_window_packets: 0,
                                level_window_bytes: 0,
                                level_window_samples: 0,
                                level_window_sum_squares: 0,
                                level_window_peak: 0,
                                level_window_nonzero_samples: 0,
                                dump_file,
                            });
                        } else {
                            println!("MIC_SOURCE_OPEN requested but SDR has no MediaSourceService candidate");
                            log_line(&mut dump, "MIC_SOURCE_OPEN requested but SDR has no MediaSourceService candidate")?;
                        }
                    }
                    if config.sensor_open {
                        if let Some(source) = sensor_candidate.clone() {
                            let sensor_list = source
                                .sensors
                                .iter()
                                .map(|s| format!("{}({})", s.name, s.value))
                                .collect::<Vec<_>>()
                                .join(",");
                            let line = format!(
                                "SENSOR_SOURCE_OPEN selected service_id={} sensors=[{}]",
                                source.service_id, sensor_list
                            );
                            println!("{line}");
                            log_line(&mut dump, &line)?;

                            let jsonl_file = match make_sensor_jsonl_file(&config.sensor_jsonl_path) {
                                Ok(file) => file,
                                Err(err) => {
                                    let line = format!("SENSOR_JSONL open failed: {err}");
                                    println!("{line}");
                                    log_line(&mut dump, &line)?;
                                    None
                                }
                            };

                            let control_payload = proto::build_channel_open_request_control_payload(source.service_id, 0)
                                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                            let tx_count = write_encrypted_plaintext_fragmented_with_flags(
                                &mut stream,
                                &mut tls,
                                source.service_id as u8,
                                FLAG_CONTROL,
                                &control_payload,
                                &mut dump,
                                "SENSOR_CHANNEL_OPEN_REQUEST",
                            )?;
                            tx_ssl_frames += tx_count;
                            sensor_channel_open_requests_sent += 1;
                            println!("tx encrypted SENSOR_CHANNEL_OPEN_REQUEST frame(s): count={tx_count} channel=0x{:02x} service_id={}", source.service_id as u8, source.service_id);
                            log_line(&mut dump, &format!("tx encrypted SENSOR_CHANNEL_OPEN_REQUEST frame(s): count={tx_count} channel=0x{:02x} service_id={}", source.service_id as u8, source.service_id))?;
                            sensor_channel = Some(SensorChannelState {
                                service_id: source.service_id,
                                channel: source.service_id as u8,
                                sensors: source.sensors,
                                opened: false,
                                requests_sent: false,
                                requests_attempted: 0,
                                responses_seen: 0,
                                batches_seen: 0,
                                errors_seen: 0,
                                jsonl_file,
                            });
                        } else {
                            println!("SENSOR_OPEN requested but SDR has no SensorSourceService candidate");
                            log_line(&mut dump, "SENSOR_OPEN requested but SDR has no SensorSourceService candidate")?;
                        }
                    }
                }
            }

            if is_control_message(&plaintext, 0x0008) {
                if selected_video_service_id.map(|id| id as u8) == Some(frame.channel) {
                    video_channel_opened = true;
                    println!("CHANNEL_OPEN_RESPONSE observed for selected_video_service_id={selected_video_service_id:?} opened={video_channel_opened}");
                    log_line(&mut dump, &format!("CHANNEL_OPEN_RESPONSE observed for selected_video_service_id={selected_video_service_id:?} opened={video_channel_opened}"))?;
                }
                if selected_input_service_id.map(|id| id as u8) == Some(frame.channel) {
                    input_channel_opened = true;
                    println!("INPUT_CHANNEL_OPEN_RESPONSE observed for selected_input_service_id={selected_input_service_id:?} opened={input_channel_opened}");
                    log_line(&mut dump, &format!("INPUT_CHANNEL_OPEN_RESPONSE observed for selected_input_service_id={selected_input_service_id:?} opened={input_channel_opened}"))?;
                }
                if let Some(audio) = audio_output_channels.iter_mut().find(|audio| audio.channel == frame.channel) {
                    audio.opened = true;
                    println!("AUDIO_CHANNEL_OPEN_RESPONSE observed service_id={} stream={} opened=true", audio.service_id, audio.stream_type);
                    log_line(&mut dump, &format!("AUDIO_CHANNEL_OPEN_RESPONSE observed service_id={} stream={} opened=true", audio.service_id, audio.stream_type))?;
                }
                if let Some(mic) = mic_source_channel.as_mut().filter(|mic| mic.channel == frame.channel) {
                    mic.opened = true;
                    println!("MIC_CHANNEL_OPEN_RESPONSE observed service_id={} opened=true", mic.service_id);
                    log_line(&mut dump, &format!("MIC_CHANNEL_OPEN_RESPONSE observed service_id={} opened=true", mic.service_id))?;
                }
                if let Some(sensor) = sensor_channel.as_mut().filter(|sensor| sensor.channel == frame.channel) {
                    sensor.opened = true;
                    println!("SENSOR_CHANNEL_OPEN_RESPONSE observed service_id={} opened=true", sensor.service_id);
                    log_line(&mut dump, &format!("SENSOR_CHANNEL_OPEN_RESPONSE observed service_id={} opened=true", sensor.service_id))?;
                }
            }

            if let Some(sensor) = sensor_channel.as_mut().filter(|sensor| sensor.opened && !sensor.requests_sent) {
                let requested_types = requested_sensor_types(sensor, &config);
                if requested_types.is_empty() {
                    let line = "SENSOR_REQUEST skipped: no advertised/requestable sensor types <= configured max";
                    println!("{line}");
                    log_line(&mut dump, line)?;
                    sensor.requests_sent = true;
                } else {
                    for sensor_type in requested_types {
                        let period = sensor_request_period_ms(sensor_type, &config);
                        let (sensor_payload, summary) = proto::build_sensor_request_payload(sensor_type, period)
                            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                        let tx_count = write_encrypted_plaintext_fragmented(
                            &mut stream,
                            &mut tls,
                            sensor.channel,
                            &sensor_payload,
                            &mut dump,
                            "SENSOR_MESSAGE_REQUEST",
                        )?;
                        tx_ssl_frames += tx_count;
                        sensor.requests_attempted += 1;
                        sensor_requests_sent += 1;
                        println!("tx encrypted {summary} frame(s): count={tx_count} channel=0x{:02x}", sensor.channel);
                        log_line(&mut dump, &format!("tx encrypted {summary} frame(s): count={tx_count} channel=0x{:02x}", sensor.channel))?;
                    }
                    sensor.requests_sent = true;
                }
            }

            if input_channel_opened
                && config.input_key_binding
                && !input_key_binding_sent
                && selected_input_service_id.map(|id| id as u8) == Some(frame.channel)
            {
                if let Some(input) = selected_input_source.as_ref() {
                    if input.keycodes_supported.is_empty() {
                        let line = "INPUT_KEY_BINDING skipped: SDR advertised no supported keycodes";
                        println!("{line}");
                        log_line(&mut dump, line)?;
                        input_key_binding_sent = true;
                    } else {
                        let (input_payload, summary) = proto::build_input_key_binding_request_payload(&input.keycodes_supported)
                            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                        println!("INPUT_CHANNEL_OPEN_RESPONSE accepted; sending encrypted {summary}");
                        log_line(&mut dump, &format!("INPUT_CHANNEL_OPEN_RESPONSE accepted; sending encrypted {summary}"))?;
                        log_line(&mut dump, &format!("tx InputKeyBindingRequest plaintext bytes={} channel=0x{:02x}", input_payload.len(), frame.channel))?;
                        if let Some(dump) = dump.as_mut() {
                            let pseudo = AapFrame {
                                channel: frame.channel,
                                flags: FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
                                final_length: None,
                                payload: input_payload.clone(),
                            };
                            describe_frame(0, 0, peer, &pseudo, dump)?;
                            dump.flush()?;
                        }

                        let tx_count = write_encrypted_plaintext_fragmented(
                            &mut stream,
                            &mut tls,
                            frame.channel,
                            &input_payload,
                            &mut dump,
                            "INPUT_KEY_BINDING_REQUEST",
                        )?;
                        tx_ssl_frames += tx_count;
                        input_key_binding_sent = true;
                        println!("tx encrypted INPUT_KEY_BINDING_REQUEST frame(s): count={tx_count} channel=0x{:02x}", frame.channel);
                        log_line(&mut dump, &format!("tx encrypted INPUT_KEY_BINDING_REQUEST frame(s): count={tx_count} channel=0x{:02x}", frame.channel))?;
                    }
                }
            }

            if input_channel_opened
                && selected_input_service_id.map(|id| id as u8) == Some(frame.channel)
                && (frame.flags & FLAG_CONTROL) == 0
            {
                if session_log_enabled(LOG_VERBOSE) {
                    if let Some(summary) = proto::describe_input_payload(&plaintext) {
                        println!("INPUT_MESSAGE observed; {summary}");
                        log_line(&mut dump, &format!("INPUT_MESSAGE observed; {summary}"))?;
                    } else {
                        println!("INPUT_MESSAGE raw undecoded channel=0x{:02x} bytes={}", frame.channel, plaintext.len());
                        log_line(&mut dump, &format!("INPUT_MESSAGE raw undecoded channel=0x{:02x} bytes={}", frame.channel, plaintext.len()))?;
                    }
                }

                if config.input_inject || config.input_key_inject {
                    if let Some(report) = proto::decode_input_report_payload(&plaintext) {
                        if config.input_inject {
                            if let Some(touch) = report.touch.as_ref() {
                                if let Some(pointer) = select_touch_pointer(touch) {
                                    let map = input_touch_map.get_or_insert_with(|| {
                                        InputTouchMap::from_config(
                                            &config,
                                            selected_input_source.as_ref(),
                                            selected_video_config_for_input.as_ref(),
                                        )
                                    });
                                    let (x, y) = map.map(pointer.x, pointer.y);
                                    let action = match touch.action {
                                        proto::DecodedPointerAction::Down | proto::DecodedPointerAction::PointerDown => Some(TouchInjectAction::Down),
                                        proto::DecodedPointerAction::Moved => Some(TouchInjectAction::Move),
                                        proto::DecodedPointerAction::Up | proto::DecodedPointerAction::PointerUp => Some(TouchInjectAction::Up),
                                        proto::DecodedPointerAction::Unknown => None,
                                    };

                                    if let Some(action) = action {
                                        if is_platform_input_bridge_mode(&config.input_inject_mode) {
                                            if input_bridge_injector.is_none() {
                                                let display_id = selected_input_source.as_ref().map(|input| input.display_id as i32).unwrap_or(0);
                                                match PlatformInputBridgeInjector::create(display_id) {
                                                    Ok((injector, report)) => {
                                                        for line in report.lines() {
                                                            let line = format!("INPUT_PLATFORM_BRIDGE {line}");
                                                            println!("{line}");
                                                            log_line(&mut dump, &line)?;
                                                        }
                                                        input_bridge_injector = Some(injector);
                                                    }
                                                    Err(err) => {
                                                        let line = format!("INPUT_PLATFORM_BRIDGE connect failed: {err}");
                                                        println!("{line}");
                                                        log_line(&mut dump, &line)?;
                                                    }
                                                }
                                            }

                                            let mut drop_bridge_move = false;
                                            if matches!(action, TouchInjectAction::Move) && config.input_app_move_min_ms > 0 {
                                                let now = std::time::Instant::now();
                                                if let Some(last) = last_app_move_inject_at {
                                                    if now.duration_since(last).as_millis() < config.input_app_move_min_ms as u128 {
                                                        drop_bridge_move = true;
                                                    }
                                                }
                                                if !drop_bridge_move {
                                                    last_app_move_inject_at = Some(now);
                                                }
                                            } else if !matches!(action, TouchInjectAction::Move) {
                                                last_app_move_inject_at = None;
                                            }

                                            if drop_bridge_move {
                                                input_app_move_drop_events += 1;
                                                if session_log_enabled(LOG_VERBOSE) && (input_app_move_drop_events <= 8 || input_app_move_drop_events % 25 == 0) {
                                                    let line = format!(
                                                        "INPUT_PLATFORM_TOUCH_DROP action=Move src={},{} mapped={},{} pointer_id={} dropped={} min_ms={}",
                                                        pointer.x,
                                                        pointer.y,
                                                        x,
                                                        y,
                                                        pointer.pointer_id,
                                                        input_app_move_drop_events,
                                                        config.input_app_move_min_ms
                                                    );
                                                    println!("{line}");
                                                    log_line(&mut dump, &line)?;
                                                }
                                            } else {
                                                let inject_result = match input_bridge_injector.as_mut() {
                                                    Some(injector) => injector.inject_touch(action, x, y, pointer.pointer_id),
                                                    None => Err(io::Error::new(io::ErrorKind::NotConnected, "platform input bridge is not available")),
                                                };
                                                match inject_result {
                                                    Ok(()) => {
                                                        input_touch_inject_events += 1;
                                                        if session_log_enabled(LOG_VERBOSE) || (!matches!(action, TouchInjectAction::Move) && session_log_enabled(LOG_NORMAL)) {
                                                            let line = format!(
                                                                "INPUT_PLATFORM_TOUCH_INJECT action={:?} src={},{} mapped={},{} pointer_id={} events={}",
                                                                action,
                                                                pointer.x,
                                                                pointer.y,
                                                                x,
                                                                y,
                                                                pointer.pointer_id,
                                                                input_touch_inject_events
                                                            );
                                                            println!("{line}");
                                                            log_line(&mut dump, &line)?;
                                                        }
                                                    }
                                                    Err(err) => {
                                                        input_bridge_injector = None;
                                                        let line = format!("INPUT_PLATFORM_TOUCH_INJECT error action={:?} mapped={},{}: {err}", action, x, y);
                                                        println!("{line}");
                                                        log_line(&mut dump, &line)?;
                                                    }
                                                }
                                            }
                                        } else if is_app_process_input_inject_mode(&config.input_inject_mode) {
                                            if input_app_injector.is_none() {
                                                let display_id = selected_input_source.as_ref().map(|input| input.display_id as i32).unwrap_or(0);
                                                match AppProcessInputInjector::create(display_id) {
                                                    Ok((injector, report)) => {
                                                        for line in report.lines() {
                                                            let line = format!("INPUT_APP_INJECTOR {line}");
                                                            println!("{line}");
                                                            log_line(&mut dump, &line)?;
                                                        }
                                                        input_app_injector = Some(injector);
                                                    }
                                                    Err(err) => {
                                                        let line = format!("INPUT_APP_INJECT disabled: failed to start app_process inputd: {err}");
                                                        println!("{line}");
                                                        log_line(&mut dump, &line)?;
                                                    }
                                                }
                                            }

                                            if let Some(injector) = input_app_injector.as_mut() {
                                                let mut drop_app_move = false;
                                                if matches!(action, TouchInjectAction::Move) && config.input_app_move_min_ms > 0 {
                                                    let now = std::time::Instant::now();
                                                    if let Some(last) = last_app_move_inject_at {
                                                        if now.duration_since(last).as_millis() < config.input_app_move_min_ms as u128 {
                                                            drop_app_move = true;
                                                        }
                                                    }
                                                    if !drop_app_move {
                                                        last_app_move_inject_at = Some(now);
                                                    }
                                                } else if !matches!(action, TouchInjectAction::Move) {
                                                    last_app_move_inject_at = None;
                                                }

                                                if drop_app_move {
                                                    input_app_move_drop_events += 1;
                                                    if session_log_enabled(LOG_VERBOSE) && (input_app_move_drop_events <= 8 || input_app_move_drop_events % 25 == 0) {
                                                        let line = format!(
                                                            "INPUT_APP_TOUCH_DROP action=Move src={},{} mapped={},{} pointer_id={} dropped={} min_ms={}",
                                                            pointer.x,
                                                            pointer.y,
                                                            x,
                                                            y,
                                                            pointer.pointer_id,
                                                            input_app_move_drop_events,
                                                            config.input_app_move_min_ms
                                                        );
                                                        println!("{line}");
                                                        log_line(&mut dump, &line)?;
                                                    }
                                                } else {
                                                    match injector.inject_touch(action, x, y, pointer.pointer_id) {
                                                        Ok(()) => {
                                                            input_touch_inject_events += 1;
                                                            if session_log_enabled(LOG_VERBOSE) || (!matches!(action, TouchInjectAction::Move) && session_log_enabled(LOG_NORMAL)) {
                                                                let line = format!(
                                                                    "INPUT_APP_TOUCH_INJECT action={:?} src={},{} mapped={},{} pointer_id={} events={}",
                                                                    action,
                                                                    pointer.x,
                                                                    pointer.y,
                                                                    x,
                                                                    y,
                                                                    pointer.pointer_id,
                                                                    input_touch_inject_events
                                                                );
                                                                println!("{line}");
                                                                log_line(&mut dump, &line)?;
                                                            }
                                                        }
                                                        Err(err) => {
                                                            let line = format!("INPUT_APP_TOUCH_INJECT error action={:?} mapped={},{}: {err}", action, x, y);
                                                            println!("{line}");
                                                            log_line(&mut dump, &line)?;
                                                        }
                                                    }
                                                }
                                            }
                                        } else if is_shell_touch_inject_mode(&config.input_inject_mode) {
                                            match input::inject_touch_motionevent_shell(action, x, y) {
                                                Ok(()) => {
                                                    input_touch_inject_events += 1;
                                                    if session_log_enabled(LOG_VERBOSE) || (!matches!(action, TouchInjectAction::Move) && session_log_enabled(LOG_NORMAL)) {
                                                        let line = format!(
                                                            "INPUT_TOUCH_SHELL_INJECT action={:?} src={},{} mapped={},{} pointer_id={} events={}",
                                                            action,
                                                            pointer.x,
                                                            pointer.y,
                                                            x,
                                                            y,
                                                            pointer.pointer_id,
                                                            input_touch_inject_events
                                                        );
                                                        println!("{line}");
                                                        log_line(&mut dump, &line)?;
                                                    }
                                                }
                                                Err(err) => {
                                                    let line = format!("INPUT_TOUCH_SHELL_INJECT error action={:?} mapped={},{}: {err}", action, x, y);
                                                    println!("{line}");
                                                    log_line(&mut dump, &line)?;
                                                }
                                            }
                                        } else {
                                            if input_touch_injector.is_none() {
                                                match TouchInjector::create(map.target_width, map.target_height) {
                                                    Ok((injector, report)) => {
                                                        for line in report.lines() {
                                                            let line = format!("INPUT_TOUCH_INJECTOR {line}");
                                                            println!("{line}");
                                                            log_line(&mut dump, &line)?;
                                                        }
                                                        input_touch_injector = Some(injector);
                                                    }
                                                    Err(err) => {
                                                        let line = format!("INPUT_TOUCH_INJECT disabled: failed to create uinput touchscreen: {err}");
                                                        println!("{line}");
                                                        log_line(&mut dump, &line)?;
                                                    }
                                                }
                                            }

                                            if let Some(injector) = input_touch_injector.as_mut() {
                                                match injector.inject(action, x, y) {
                                                    Ok(()) => {
                                                        input_touch_inject_events += 1;
                                                        if session_log_enabled(LOG_VERBOSE) || (!matches!(action, TouchInjectAction::Move) && session_log_enabled(LOG_NORMAL)) {
                                                            let line = format!(
                                                                "INPUT_TOUCH_INJECT action={:?} src={},{} mapped={},{} pointer_id={} events={}",
                                                                action,
                                                                pointer.x,
                                                                pointer.y,
                                                                x,
                                                                y,
                                                                pointer.pointer_id,
                                                                input_touch_inject_events
                                                            );
                                                            println!("{line}");
                                                            log_line(&mut dump, &line)?;
                                                        }
                                                    }
                                                    Err(err) => {
                                                        let line = format!("INPUT_TOUCH_INJECT error action={:?} mapped={},{}: {err}", action, x, y);
                                                        println!("{line}");
                                                        log_line(&mut dump, &line)?;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    let line = "INPUT_TOUCH_INJECT skipped: touch report has no pointer_data";
                                    println!("{line}");
                                    log_line(&mut dump, line)?;
                                }
                            }
                        }

                        if config.input_key_inject {
                            for key in &report.keys {
                                if !key.down {
                                    continue;
                                }
                                if let Some(android_code) = input::android_passthrough_keycode(key.keycode) {
                                    let key_result = if is_platform_input_bridge_mode(&config.input_inject_mode) {
                                        if input_bridge_injector.is_none() {
                                            let display_id = selected_input_source.as_ref().map(|input| input.display_id as i32).unwrap_or(0);
                                            match PlatformInputBridgeInjector::create(display_id) {
                                                Ok((injector, report)) => {
                                                    for line in report.lines() {
                                                        let line = format!("INPUT_PLATFORM_BRIDGE {line}");
                                                        println!("{line}");
                                                        log_line(&mut dump, &line)?;
                                                    }
                                                    input_bridge_injector = Some(injector);
                                                }
                                                Err(err) => {
                                                    let line = format!("INPUT_PLATFORM_KEY_INJECT connect failed: {err}");
                                                    println!("{line}");
                                                    log_line(&mut dump, &line)?;
                                                }
                                            }
                                        }
                                        let result = match input_bridge_injector.as_mut() {
                                            Some(injector) => injector.inject_key_press(android_code),
                                            None => Err(io::Error::new(io::ErrorKind::NotConnected, "platform input bridge is not available")),
                                        };
                                        if result.is_err() {
                                            input_bridge_injector = None;
                                        }
                                        result
                                    } else if is_app_process_input_inject_mode(&config.input_inject_mode) {
                                        if input_app_injector.is_none() {
                                            let display_id = selected_input_source.as_ref().map(|input| input.display_id as i32).unwrap_or(0);
                                            match AppProcessInputInjector::create(display_id) {
                                                Ok((injector, report)) => {
                                                    for line in report.lines() {
                                                        let line = format!("INPUT_APP_INJECTOR {line}");
                                                        println!("{line}");
                                                        log_line(&mut dump, &line)?;
                                                    }
                                                    input_app_injector = Some(injector);
                                                }
                                                Err(err) => {
                                                    let line = format!("INPUT_APP_KEY_INJECT disabled: failed to start app_process inputd: {err}");
                                                    println!("{line}");
                                                    log_line(&mut dump, &line)?;
                                                }
                                            }
                                        }
                                        match input_app_injector.as_mut() {
                                            Some(injector) => injector.inject_key_press(android_code),
                                            None => Err(io::Error::new(io::ErrorKind::NotConnected, "app_process inputd is not available")),
                                        }
                                    } else {
                                        input::inject_android_keyevent_shell(android_code)
                                    };

                                    match key_result {
                                        Ok(()) => {
                                            input_key_inject_events += 1;
                                            let line = format!(
                                                "INPUT_KEY_INJECT mode={} keycode={}({}) android_keyevent={} events={}",
                                                config.input_inject_mode,
                                                key.keycode,
                                                proto::keycode_name(key.keycode),
                                                android_code,
                                                input_key_inject_events
                                            );
                                            println!("{line}");
                                            log_line(&mut dump, &line)?;
                                        }
                                        Err(err) => {
                                            let line = format!("INPUT_KEY_INJECT error mode={} keycode={}({}): {err}", config.input_inject_mode, key.keycode, proto::keycode_name(key.keycode));
                                            println!("{line}");
                                            log_line(&mut dump, &line)?;
                                        }
                                    }
                                } else {
                                    let line = format!(
                                        "INPUT_KEY_INJECT skipped unmapped keycode={}({})",
                                        key.keycode,
                                        proto::keycode_name(key.keycode)
                                    );
                                    println!("{line}");
                                    log_line(&mut dump, &line)?;
                                }
                            }
                        }
                    }
                }
            }

            if let Some(mic) = mic_source_channel.as_mut().filter(|mic| mic.opened && !mic.setup_sent) {
                // MediaSource / microphone is HU -> device.  Unlike media sinks,
                // the device side must not send AV setup/start on this channel;
                // OpenAuto/DHU treats that as an unexpected message.  After the
                // channel-open response we stay passive and wait for HU-side
                // MicrophoneRequest and/or raw media data.
                mic.setup_sent = true;
                mic.start_sent = true;
                println!("MIC_CHANNEL_OPEN_RESPONSE accepted; media_source passive mode: not sending MEDIA_MESSAGE_SETUP/START; waiting for HU MICROPHONE_REQUEST/DATA on channel=0x{:02x}", mic.channel);
                log_line(&mut dump, &format!("MIC_CHANNEL_OPEN_RESPONSE accepted; media_source passive mode: not sending MEDIA_MESSAGE_SETUP/START; waiting for HU MICROPHONE_REQUEST/DATA on channel=0x{:02x}", mic.channel))?;
                if config.mic_send_request_open && !mic.mic_request_sent {
                    // Experimental: some HUs/DHU do not begin sending microphone PCM
                    // just because the media_source channel is open.  Request the HU
                    // microphone explicitly, but do not send media SETUP/START on this
                    // source channel (OpenAuto/DHU rejected that as an unexpected
                    // message).  If the direction is wrong for a HU, it should be easy
                    // to see from HU logs without disturbing the sink audio path.
                    let (req_payload, req_summary) = proto::build_microphone_request_payload(
                        true,
                        config.mic_max_unacked,
                        false,
                        false,
                    )
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                    let tx_count = write_encrypted_plaintext_fragmented(
                        &mut stream,
                        &mut tls,
                        mic.channel,
                        &req_payload,
                        &mut dump,
                        "MICROPHONE_REQUEST_OPEN",
                    )?;
                    tx_ssl_frames += tx_count;
                    mic.mic_request_sent = true;
                    mic.max_unacked = config.mic_max_unacked;
                    println!("tx encrypted {req_summary} frame(s): count={tx_count} channel=0x{:02x} mode=request-only", mic.channel);
                    log_line(&mut dump, &format!("tx encrypted {req_summary} frame(s): count={tx_count} channel=0x{:02x} mode=request-only", mic.channel))?;
                }
            }

            if let Some(mic) = mic_source_channel.as_mut().filter(|mic| mic.channel == frame.channel && (frame.flags & FLAG_CONTROL) == 0) {
                if let Some((status, session_id, summary)) = proto::parse_microphone_response_payload(&plaintext) {
                    mic.mic_response_seen = true;
                    let previous_session_id = mic.session_id;
                    // Keep the HU-provided session id even when it is 0.  DHU reports
                    // session_id=0 for microphone source, and MEDIA_ACK must use that
                    // same value; otherwise capture stalls exactly at max_unacked.
                    mic.session_id = session_id;
                    println!("MICROPHONE_RESPONSE observed service_id={} {}; active_session_id={} previous_session_id={}", mic.service_id, summary, mic.session_id, previous_session_id);
                    log_line(&mut dump, &format!("MICROPHONE_RESPONSE observed service_id={} {}; active_session_id={} previous_session_id={}", mic.service_id, summary, mic.session_id, previous_session_id))?;
                    if status != 0 {
                        let line = format!("MICROPHONE_RESPONSE non-zero status={} service_id={}", status, mic.service_id);
                        println!("{line}");
                        log_line(&mut dump, &line)?;
                    }
                }
                if let Some((open, max_unacked, summary)) = proto::parse_microphone_request_payload(&plaintext) {
                    println!("MICROPHONE_REQUEST observed from HU service_id={} {}", mic.service_id, summary);
                    log_line(&mut dump, &format!("MICROPHONE_REQUEST observed from HU service_id={} {}", mic.service_id, summary))?;
                    if max_unacked > 0 {
                        mic.max_unacked = max_unacked;
                    }
                    let response_status = 0;
                    let (resp_payload, resp_summary) = proto::build_microphone_response_payload(response_status, mic.session_id)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                    let tx_count = write_encrypted_plaintext_fragmented(
                        &mut stream,
                        &mut tls,
                        frame.channel,
                        &resp_payload,
                        &mut dump,
                        "MICROPHONE_RESPONSE",
                    )?;
                    tx_ssl_frames += tx_count;
                    println!("tx encrypted {resp_summary} frame(s): count={tx_count} channel=0x{:02x} request_open={}", frame.channel, open);
                    log_line(&mut dump, &format!("tx encrypted {resp_summary} frame(s): count={tx_count} channel=0x{:02x} request_open={}", frame.channel, open))?;
                    if !open {
                        let line = format!("MICROPHONE_REQUEST HU requested close service_id={}", mic.service_id);
                        println!("{line}");
                        log_line(&mut dump, &line)?;
                    }
                }
                if let Some((pts_us, pcm)) = proto::parse_media_data_payload(&plaintext) {
                    mic.data_packets += 1;
                    mic.data_bytes += pcm.len() as u64;
                    mic.ack_counter = mic.ack_counter.saturating_add(1);
                    mic_data_packets += 1;
                    mic_data_bytes += pcm.len() as u64;
                    if let Some(file) = mic.dump_file.as_mut() {
                        file.write_all(pcm)?;
                    }
                    let (samples, sum_squares, peak, nonzero_samples) = pcm_s16le_metrics(pcm);
                    mic.level_window_packets = mic.level_window_packets.saturating_add(1);
                    mic.level_window_bytes = mic.level_window_bytes.saturating_add(pcm.len() as u64);
                    mic.level_window_samples = mic.level_window_samples.saturating_add(samples);
                    mic.level_window_sum_squares = mic.level_window_sum_squares.saturating_add(sum_squares);
                    mic.level_window_peak = mic.level_window_peak.max(peak);
                    mic.level_window_nonzero_samples = mic.level_window_nonzero_samples.saturating_add(nonzero_samples);
                    let now_for_level = Instant::now();
                    if !mic.level_first_logged || now_for_level.duration_since(mic.level_last_log) >= Duration::from_secs(1) {
                        let rms = rms_from_sum_squares(mic.level_window_sum_squares, mic.level_window_samples);
                        let silence = mic.level_window_peak <= 4 || mic.level_window_nonzero_samples == 0;
                        let line = format!(
                            "MIC_LEVEL service_id={} packets_window={} total_packets={} bytes_window={} total_bytes={} rms={:.1} peak={} nonzero_samples={} silence={}",
                            mic.service_id,
                            mic.level_window_packets,
                            mic.data_packets,
                            mic.level_window_bytes,
                            mic.data_bytes,
                            rms,
                            mic.level_window_peak,
                            mic.level_window_nonzero_samples,
                            silence
                        );
                        println!("{line}");
                        log_line(&mut dump, &line)?;
                        mic.level_first_logged = true;
                        mic.level_last_log = now_for_level;
                        mic.level_window_packets = 0;
                        mic.level_window_bytes = 0;
                        mic.level_window_samples = 0;
                        mic.level_window_sum_squares = 0;
                        mic.level_window_peak = 0;
                        mic.level_window_nonzero_samples = 0;
                    }
                    if !mic.first_data_logged || session_log_enabled(LOG_NORMAL) {
                        let cfg_text = mic.config.as_ref()
                            .map(|cfg| format!("{}Hz/{}bit/{}ch", cfg.sampling_rate, cfg.bits, cfg.channels))
                            .unwrap_or_else(|| "unknown-format".to_string());
                        let line = format!(
                            "MIC_DATA service_id={} packet={} pts_us={} bytes={} total_bytes={} format={} dumped={}",
                            mic.service_id,
                            mic.data_packets,
                            pts_us,
                            pcm.len(),
                            mic.data_bytes,
                            cfg_text,
                            mic.dump_file.is_some()
                        );
                        println!("{line}");
                        log_line(&mut dump, &line)?;
                        mic.first_data_logged = true;
                    }
                    if config.mic_ack && config.mic_ack_mode != "off" {
                        mic.ack_window_counter = mic.ack_window_counter.saturating_add(1);
                        let window_size = (mic.max_unacked / 2).clamp(1, 32);
                        let (should_ack, ack_value) = match config.mic_ack_mode.as_str() {
                            // OpenAuto/aasdk treats ACK.value as a packet count that releases
                            // flow-control credit, not as a cumulative sequence number.  For
                            // per-packet ACK, always release exactly one packet.
                            "delta" => (true, 1),
                            "window" => {
                                if mic.ack_window_counter >= window_size {
                                    (true, mic.ack_window_counter)
                                } else {
                                    (false, 0)
                                }
                            }
                            // Diagnostic old behavior: cumulative value.  DHU tends to stop/close
                            // early with this mode, but keep it for comparison.
                            "cumulative" => (true, mic.ack_counter),
                            // Diagnostic only: unlikely, but useful if a HU expects byte-count credit.
                            "bytes" => (true, pcm.len().min(u32::MAX as usize) as u32),
                            _ => (false, 0),
                        };

                        if should_ack {
                            let (ack_payload, summary) = proto::build_media_ack_payload(mic.session_id, ack_value)
                                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                            let tx_count = write_encrypted_plaintext_fragmented(
                                &mut stream,
                                &mut tls,
                                frame.channel,
                                &ack_payload,
                                &mut dump,
                                "MIC_MEDIA_MESSAGE_ACK",
                            )?;
                            tx_ssl_frames += tx_count;
                            if config.mic_ack_mode == "window" {
                                mic.ack_window_counter = 0;
                            }
                            if !mic.first_ack_logged || mic.ack_counter % 50 == 0 || session_log_enabled(LOG_VERBOSE) {
                                println!("tx encrypted MIC {summary} mode={} frame(s): count={tx_count} channel=0x{:02x} total_packets={} total_bytes={}", config.mic_ack_mode, frame.channel, mic.data_packets, mic.data_bytes);
                                log_line(&mut dump, &format!("tx encrypted MIC {summary} mode={} frame(s): count={tx_count} channel=0x{:02x} total_packets={} total_bytes={}", config.mic_ack_mode, frame.channel, mic.data_packets, mic.data_bytes))?;
                                mic.first_ack_logged = true;
                            }
                        }
                    }
                }
            }

            if let Some(sensor) = sensor_channel.as_mut().filter(|sensor| sensor.opened && sensor.channel == frame.channel && (frame.flags & FLAG_CONTROL) == 0) {
                if let Some(summary) = proto::parse_sensor_response_payload(&plaintext) {
                    sensor.responses_seen += 1;
                    println!("SENSOR_RESPONSE observed service_id={} {summary}", sensor.service_id);
                    log_line(&mut dump, &format!("SENSOR_RESPONSE observed service_id={} {summary}", sensor.service_id))?;
                }
                if let Some(summary) = proto::parse_sensor_error_payload(&plaintext) {
                    sensor.errors_seen += 1;
                    sensor_errors_seen += 1;
                    println!("SENSOR_ERROR observed service_id={} {summary}", sensor.service_id);
                    log_line(&mut dump, &format!("SENSOR_ERROR observed service_id={} {summary}", sensor.service_id))?;
                }
                if let Some(batch) = proto::parse_sensor_batch_payload(&plaintext) {
                    sensor.batches_seen += 1;
                    sensor_batches_seen += 1;
                    println!("SENSOR_BATCH observed service_id={} batch={} {}", sensor.service_id, sensor.batches_seen, batch.summary);
                    log_line(&mut dump, &format!("SENSOR_BATCH observed service_id={} batch={} {}", sensor.service_id, sensor.batches_seen, batch.summary))?;
                    if let Some(file) = sensor.jsonl_file.as_mut() {
                        writeln!(file, "{}", batch.json_line)?;
                    }
                    for event in &batch.events {
                        let line = format!("SENSOR_NORMALIZED {}", event.summary());
                        println!("{line}");
                        log_line(&mut dump, &line)?;
                    }
                    if let Some(location) = batch.location.as_ref() {
                        sensor_os_forwarder.push_location(location, &mut dump)?;
                    }
                    for event in &batch.events {
                        sensor_os_forwarder.push_gnss_satellite(event, &mut dump)?;
                    }
                    if let Some(pipeline) = sensor_pipeline.as_mut() {
                        for line in pipeline.process_events(&batch.events) {
                            println!("{line}");
                            log_line(&mut dump, &line)?;
                        }
                    }
                }
            }

            if let Some(index) = audio_output_channels.iter().position(|audio| audio.opened && !audio.setup_sent) {
                let channel = audio_output_channels[index].channel;
                let (media_payload, summary) = proto::build_audio_pcm_media_setup_payload()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                println!("AUDIO_CHANNEL_OPEN_RESPONSE accepted; sending encrypted {summary} channel=0x{channel:02x} stream={}", audio_output_channels[index].stream_type);
                log_line(&mut dump, &format!("AUDIO_CHANNEL_OPEN_RESPONSE accepted; sending encrypted {summary} channel=0x{channel:02x} stream={}", audio_output_channels[index].stream_type))?;
                let tx_count = write_encrypted_plaintext_fragmented(
                    &mut stream,
                    &mut tls,
                    channel,
                    &media_payload,
                    &mut dump,
                    "AUDIO_MEDIA_MESSAGE_SETUP",
                )?;
                tx_ssl_frames += tx_count;
                audio_output_channels[index].setup_sent = true;
                println!("tx encrypted AUDIO_MEDIA_MESSAGE_SETUP frame(s): count={tx_count} channel=0x{channel:02x}");
                log_line(&mut dump, &format!("tx encrypted AUDIO_MEDIA_MESSAGE_SETUP frame(s): count={tx_count} channel=0x{channel:02x}"))?;
            }

            if let Some(index) = audio_output_channels.iter().position(|audio| audio.opened && audio.setup_sent && !audio.start_sent && audio.channel == frame.channel && (frame.flags & FLAG_CONTROL) == 0) {
                let session_id = audio_output_channels[index].session_id;
                if let Some((media_payload, configuration_index, max_unacked, summary)) = proto::maybe_build_media_start_from_config_payload(&plaintext, session_id)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
                {
                    audio_output_channels[index].max_unacked = max_unacked.max(1);
                    println!("AUDIO MEDIA_MESSAGE_CONFIG READY observed service_id={} stream={}; sending encrypted {summary}", audio_output_channels[index].service_id, audio_output_channels[index].stream_type);
                    log_line(&mut dump, &format!("AUDIO MEDIA_MESSAGE_CONFIG READY observed service_id={} stream={}; sending encrypted {summary}", audio_output_channels[index].service_id, audio_output_channels[index].stream_type))?;
                    log_line(&mut dump, &format!("tx AudioMediaStart plaintext bytes={} session_id={} configuration_index={}", media_payload.len(), session_id, configuration_index))?;
                    let tx_count = write_encrypted_plaintext_fragmented(
                        &mut stream,
                        &mut tls,
                        frame.channel,
                        &media_payload,
                        &mut dump,
                        "AUDIO_MEDIA_MESSAGE_START",
                    )?;
                    tx_ssl_frames += tx_count;
                    audio_output_channels[index].start_sent = true;
                    println!("tx encrypted AUDIO_MEDIA_MESSAGE_START frame(s): count={tx_count} channel=0x{:02x}", frame.channel);
                    log_line(&mut dump, &format!("tx encrypted AUDIO_MEDIA_MESSAGE_START frame(s): count={tx_count} channel=0x{:02x}", frame.channel))?;
                    if config.audio_test_tone || config.audio_capture {
                        if !video_pump_timeout_enabled {
                            stream.set_read_timeout(Some(Duration::from_millis(20)))?;
                            video_pump_timeout_enabled = true;
                            let line = "AUDIO_PUMP enabled: receive timeout reduced to 20ms so generated/captured PCM can be sent between HU frames";
                            println!("{line}");
                            log_line(&mut dump, line)?;
                        }
                        if config.audio_capture && audio_capture.is_none() {
                            let (rate, _bits, channels) = audio_cfg_values(audio_output_channels[index].config.as_ref());
                            match AppProcessAudioCapture::create(
                                rate,
                                channels,
                                &config.audio_capture_source,
                                config.audio_capture_chunk_ms,
                                &config.audio_capture_as,
                                &config.audio_capture_package,
                            ) {
                                Ok((capture, report)) => {
                                    println!("AUDIO_CAPTURE_APP {}", report.trim().replace('\n', " | "));
                                    log_line(&mut dump, &format!("AUDIO_CAPTURE_APP {}", report.trim().replace('\n', " | ")))?;
                                    audio_capture = Some(capture);
                                }
                                Err(err) => {
                                    let line = format!("AUDIO_CAPTURE disabled: failed to start app_process AudioRecord capture: {err}");
                                    println!("{line}");
                                    log_line(&mut dump, &line)?;
                                }
                            }
                        }
                        let sent = pump_audio_test_tone(
                            &mut audio_output_channels,
                            &mut stream,
                            &mut tls,
                            &config,
                            &mut dump,
                        )?;
                        tx_ssl_frames += sent;
                        let sent = pump_audio_capture(
                            &mut audio_output_channels,
                            &mut audio_capture,
                            &mut audio_capture_buffer,
                            &mut stream,
                            &mut tls,
                            &config,
                            &mut dump,
                        )?;
                        tx_ssl_frames += sent;
                    }
                }
            }

            if let Some(index) = audio_output_channels.iter().position(|audio| audio.start_sent && audio.channel == frame.channel && (frame.flags & FLAG_CONTROL) == 0) {
                if let Some((session_id, ack_value, summary)) = proto::parse_media_ack_payload(&plaintext) {
                    if session_id == audio_output_channels[index].session_id {
                        let released = ack_value.max(1);
                        audio_output_channels[index].tx_unacked = audio_output_channels[index].tx_unacked.saturating_sub(released);
                        if session_log_enabled(LOG_VERBOSE) {
                            println!("AUDIO_MEDIA_MESSAGE_ACK observed service_id={} stream={} {}; tx_unacked={}", audio_output_channels[index].service_id, audio_output_channels[index].stream_type, summary, audio_output_channels[index].tx_unacked);
                            log_line(&mut dump, &format!("AUDIO_MEDIA_MESSAGE_ACK observed service_id={} stream={} {}; tx_unacked={}", audio_output_channels[index].service_id, audio_output_channels[index].stream_type, summary, audio_output_channels[index].tx_unacked))?;
                        }
                    }
                }
                if let Some((pts_us, pcm)) = proto::parse_media_data_payload(&plaintext) {
                    audio_output_channels[index].data_packets += 1;
                    audio_output_channels[index].data_bytes += pcm.len() as u64;
                    audio_output_channels[index].ack_counter = audio_output_channels[index].ack_counter.saturating_add(1);
                    audio_data_packets += 1;
                    audio_data_bytes += pcm.len() as u64;
                    if let Some(file) = audio_output_channels[index].dump_file.as_mut() {
                        file.write_all(pcm)?;
                    }
                    if !audio_output_channels[index].first_data_logged || session_log_enabled(LOG_NORMAL) {
                        let cfg_text = audio_output_channels[index].config.as_ref()
                            .map(|cfg| format!("{}Hz/{}bit/{}ch", cfg.sampling_rate, cfg.bits, cfg.channels))
                            .unwrap_or_else(|| "unknown-format".to_string());
                        let line = format!(
                            "AUDIO_DATA service_id={} stream={} packet={} pts_us={} bytes={} total_bytes={} format={} dumped={}",
                            audio_output_channels[index].service_id,
                            audio_output_channels[index].stream_type,
                            audio_output_channels[index].data_packets,
                            pts_us,
                            pcm.len(),
                            audio_output_channels[index].data_bytes,
                            cfg_text,
                            audio_output_channels[index].dump_file.is_some()
                        );
                        println!("{line}");
                        log_line(&mut dump, &line)?;
                        audio_output_channels[index].first_data_logged = true;
                    }
                    if config.audio_ack {
                        let (ack_payload, summary) = proto::build_media_ack_payload(audio_output_channels[index].session_id, audio_output_channels[index].ack_counter)
                            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                        let tx_count = write_encrypted_plaintext_fragmented(
                            &mut stream,
                            &mut tls,
                            frame.channel,
                            &ack_payload,
                            &mut dump,
                            "AUDIO_MEDIA_MESSAGE_ACK",
                        )?;
                        tx_ssl_frames += tx_count;
                        if session_log_enabled(LOG_VERBOSE) {
                            println!("tx encrypted AUDIO {summary} frame(s): count={tx_count} channel=0x{:02x}", frame.channel);
                            log_line(&mut dump, &format!("tx encrypted AUDIO {summary} frame(s): count={tx_count} channel=0x{:02x}", frame.channel))?;
                        }
                    }
                }
            }

            if video_channel_opened
                && !video_setup_sent
                && selected_video_service_id.map(|id| id as u8) == Some(frame.channel)
            {
                let (media_payload, summary) = proto::build_video_media_setup_payload()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                println!("CHANNEL_OPEN_RESPONSE accepted; sending encrypted {summary}");
                log_line(&mut dump, &format!("CHANNEL_OPEN_RESPONSE accepted; sending encrypted {summary}"))?;
                log_line(&mut dump, &format!("tx MediaSetup plaintext bytes={} channel=0x{:02x}", media_payload.len(), frame.channel))?;

                if let Some(dump) = dump.as_mut() {
                    let pseudo = AapFrame {
                        channel: frame.channel,
                        flags: FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
                        final_length: None,
                        payload: media_payload.clone(),
                    };
                    describe_frame(0, 0, peer, &pseudo, dump)?;
                    dump.flush()?;
                }

                let tx_count = write_encrypted_plaintext_fragmented(
                    &mut stream,
                    &mut tls,
                    frame.channel,
                    &media_payload,
                    &mut dump,
                    "MEDIA_MESSAGE_SETUP",
                )?;
                tx_ssl_frames += tx_count;
                video_setup_sent = true;
                println!("tx encrypted MEDIA_MESSAGE_SETUP frame(s): count={tx_count} channel=0x{:02x}", frame.channel);
                log_line(&mut dump, &format!("tx encrypted MEDIA_MESSAGE_SETUP frame(s): count={tx_count} channel=0x{:02x}", frame.channel))?;
            }

            if video_channel_opened
                && video_setup_sent
                && !video_start_sent
                && selected_video_service_id.map(|id| id as u8) == Some(frame.channel)
                && (frame.flags & FLAG_CONTROL) == 0
            {
                if let Some((media_payload, configuration_index, max_unacked, summary)) = proto::maybe_build_media_start_from_config_payload(&plaintext, video_session_id)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
                {
                    video_max_unacked = max_unacked.max(1);
                    println!("MEDIA_MESSAGE_CONFIG READY observed; sending encrypted {summary}");
                    log_line(&mut dump, &format!("MEDIA_MESSAGE_CONFIG READY observed; sending encrypted {summary}"))?;
                    log_line(&mut dump, &format!("tx MediaStart plaintext bytes={} session_id={} configuration_index={}", media_payload.len(), video_session_id, configuration_index))?;
                    if let Some(dump) = dump.as_mut() {
                        let pseudo = AapFrame {
                            channel: frame.channel,
                            flags: FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
                            final_length: None,
                            payload: media_payload.clone(),
                        };
                        describe_frame(0, 0, peer, &pseudo, dump)?;
                        dump.flush()?;
                    }

                    let tx_count = write_encrypted_plaintext_fragmented(
                        &mut stream,
                        &mut tls,
                        frame.channel,
                        &media_payload,
                        &mut dump,
                        "MEDIA_MESSAGE_START",
                    )?;
                    tx_ssl_frames += tx_count;
                    video_start_sent = true;
                    if config.host_liveness_timeout_ms > 0 {
                        last_hu_activity_at = Some(Instant::now());
                        let line = format!(
                            "AAP_HOST_LIVENESS_ARMED timeout_ms={} source=inbound-frame",
                            config.host_liveness_timeout_ms
                        );
                        println!("{line}");
                        log_line(&mut dump, &line)?;
                    }
                    if let Some(sender) = video_sender.as_mut() {
                        sender.mark_media_start_sent();
                    }
                    println!("tx encrypted MEDIA_MESSAGE_START frame(s): count={tx_count} channel=0x{:02x}", frame.channel);
                    log_line(&mut dump, &format!("tx encrypted MEDIA_MESSAGE_START frame(s): count={tx_count} channel=0x{:02x}", frame.channel))?;
                    if config.video_start_timing {
                        let line = "VIDEO_START_LATENCY media_start_sent_ms=0";
                        println!("{line}");
                        log_line(&mut dump, line)?;
                    }

                    if !video_pump_timeout_enabled {
                        stream.set_read_timeout(Some(Duration::from_millis(20)))?;
                        video_pump_timeout_enabled = true;
                        let line = "VIDEO_PUMP enabled: receive timeout reduced to 20ms so native video is pumped between HU frames";
                        println!("{line}");
                        log_line(&mut dump, line)?;
                    }

                    if let Some(sender) = video_sender.as_mut() {
                        let sent = sender.send_available(
                            &mut stream,
                            &mut tls,
                            frame.channel,
                            video_session_id,
                            video_max_unacked,
                            &mut dump,
                        )?;
                        tx_ssl_frames += sent;
                    }
                }
            }

            if video_start_sent
                && selected_video_service_id.map(|id| id as u8) == Some(frame.channel)
                && (frame.flags & FLAG_CONTROL) == 0
            {
                if let Some((session_id, ack_value, summary)) = proto::parse_media_ack_payload(&plaintext) {
                    if session_log_enabled(LOG_VERBOSE) {
                        println!("MEDIA_MESSAGE_ACK observed; {summary}");
                        log_line(&mut dump, &format!("MEDIA_MESSAGE_ACK observed; {summary}"))?;
                    }
                    if session_id == video_session_id {
                        if let Some(sender) = video_sender.as_mut() {
                            if let Some(line) = sender.on_ack(ack_value) {
                                println!("{line}");
                                log_line(&mut dump, &line)?;
                            }
                            let sent = sender.send_available(
                                &mut stream,
                                &mut tls,
                                frame.channel,
                                video_session_id,
                                video_max_unacked,
                                &mut dump,
                            )?;
                            tx_ssl_frames += sent;
                        }
                    }
                }
            }

            if let Some(control_payload) = proto::maybe_build_ping_response_control_payload(&plaintext)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
            {
                if !config.quiet_ping_logs {
                    println!("PING_REQUEST observed; sending encrypted PING_RESPONSE");
                    log_line(&mut dump, "PING_REQUEST observed; sending encrypted PING_RESPONSE")?;
                    log_line(&mut dump, &format!("tx PingResponse plaintext bytes={}", control_payload.len()))?;
                    if let Some(dump) = dump.as_mut() {
                        let pseudo = AapFrame {
                            channel: 0,
                            flags: FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
                            final_length: None,
                            payload: control_payload.clone(),
                        };
                        describe_frame(0, 0, peer, &pseudo, dump)?;
                        dump.flush()?;
                    }
                }

                let tx_count = write_encrypted_plaintext_fragmented(
                    &mut stream,
                    &mut tls,
                    0,
                    &control_payload,
                    &mut dump,
                    "PING_RESPONSE",
                )?;
                tx_ssl_frames += tx_count;
                if !config.quiet_ping_logs {
                    println!("tx encrypted PING_RESPONSE frame(s): count={tx_count}");
                    log_line(&mut dump, &format!("tx encrypted PING_RESPONSE frame(s): count={tx_count}"))?;
                }
            }

            if video_start_sent {
                if let (Some(sender), Some(video_channel)) = (video_sender.as_mut(), selected_video_service_id.map(|id| id as u8)) {
                    let sent = sender.send_available(
                        &mut stream,
                        &mut tls,
                        video_channel,
                        video_session_id,
                        video_max_unacked,
                        &mut dump,
                    )?;
                    tx_ssl_frames += sent;
                }
            }
        }
    }

    if cleanup_reason == "session-end"
        && config.max_frames > 0
        && frames >= config.max_frames
    {
        cleanup_reason = "max-frames";
        println!("max_frames reached: {frames}");
        let _ = log_line(&mut dump, &format!("max_frames reached: {frames}"));
    }
    if session_shutdown_requested() {
        cleanup_reason = "signal";
    }
    let notify_peer_during_cleanup = !matches!(
        cleanup_reason,
        "host-liveness-timeout" | "peer-disconnect" | "read-timeout"
    );
    println!("SESSION_CLEANUP begin reason={} video_opened={} input_opened={} audio_channels={} mic_opened={} sensor_opened={}",
        cleanup_reason,
        video_channel_opened,
        input_channel_opened,
        audio_output_channels.iter().filter(|audio| audio.opened).count(),
        mic_source_channel.as_ref().map(|mic| mic.opened).unwrap_or(false),
        sensor_channel.as_ref().map(|sensor| sensor.opened).unwrap_or(false)
    );
    let _ = log_line(&mut dump, &format!("SESSION_CLEANUP begin reason={} video_opened={} input_opened={} audio_channels={} mic_opened={} sensor_opened={}",
        cleanup_reason,
        video_channel_opened,
        input_channel_opened,
        audio_output_channels.iter().filter(|audio| audio.opened).count(),
        mic_source_channel.as_ref().map(|mic| mic.opened).unwrap_or(false),
        sensor_channel.as_ref().map(|sensor| sensor.opened).unwrap_or(false)
    ));

    if notify_peer_during_cleanup {
    if let Some(mic) = mic_source_channel.as_mut() {
        if mic.opened && mic.mic_request_sent {
            if let Ok((payload, summary)) = proto::build_microphone_request_payload(false, mic.max_unacked.max(1), false, false) {
                match write_encrypted_plaintext_fragmented(&mut stream, &mut tls, mic.channel, &payload, &mut dump, "MICROPHONE_REQUEST_CLOSE") {
                    Ok(count) => {
                        tx_ssl_frames += count;
                        println!("SESSION_CLEANUP tx encrypted {} frame(s): count={} channel=0x{:02x}", summary, count, mic.channel);
                        let _ = log_line(&mut dump, &format!("SESSION_CLEANUP tx encrypted {} frame(s): count={} channel=0x{:02x}", summary, count, mic.channel));
                    }
                    Err(err) => {
                        println!("SESSION_CLEANUP microphone close request failed channel=0x{:02x}: {}", mic.channel, err);
                        let _ = log_line(&mut dump, &format!("SESSION_CLEANUP microphone close request failed channel=0x{:02x}: {}", mic.channel, err));
                    }
                }
            }
        }
        if mic.opened {
            if let Ok(payload) = proto::build_channel_close_notification_control_payload() {
                match write_encrypted_plaintext_fragmented(&mut stream, &mut tls, mic.channel, &payload, &mut dump, "MIC_CHANNEL_CLOSE_NOTIFICATION") {
                    Ok(count) => {
                        tx_ssl_frames += count;
                        println!("SESSION_CLEANUP tx encrypted MIC_CHANNEL_CLOSE_NOTIFICATION frame(s): count={} channel=0x{:02x}", count, mic.channel);
                        let _ = log_line(&mut dump, &format!("SESSION_CLEANUP tx encrypted MIC_CHANNEL_CLOSE_NOTIFICATION frame(s): count={} channel=0x{:02x}", count, mic.channel));
                    }
                    Err(err) => {
                        println!("SESSION_CLEANUP mic channel close failed channel=0x{:02x}: {}", mic.channel, err);
                        let _ = log_line(&mut dump, &format!("SESSION_CLEANUP mic channel close failed channel=0x{:02x}: {}", mic.channel, err));
                    }
                }
            }
        }
    }

    if let Ok(payload) = proto::build_channel_close_notification_control_payload() {
        if let Some(sensor) = sensor_channel.as_ref().filter(|sensor| sensor.opened) {
            match write_encrypted_plaintext_fragmented(&mut stream, &mut tls, sensor.channel, &payload, &mut dump, "SENSOR_CHANNEL_CLOSE_NOTIFICATION") {
                Ok(count) => {
                    tx_ssl_frames += count;
                    println!("SESSION_CLEANUP tx encrypted SENSOR_CHANNEL_CLOSE_NOTIFICATION frame(s): count={} channel=0x{:02x} service_id={}", count, sensor.channel, sensor.service_id);
                    let _ = log_line(&mut dump, &format!("SESSION_CLEANUP tx encrypted SENSOR_CHANNEL_CLOSE_NOTIFICATION frame(s): count={} channel=0x{:02x} service_id={}", count, sensor.channel, sensor.service_id));
                }
                Err(err) => {
                    println!("SESSION_CLEANUP sensor channel close failed channel=0x{:02x}: {}", sensor.channel, err);
                    let _ = log_line(&mut dump, &format!("SESSION_CLEANUP sensor channel close failed channel=0x{:02x}: {}", sensor.channel, err));
                }
            }
        }
        for audio in audio_output_channels.iter().filter(|audio| audio.opened) {
            match write_encrypted_plaintext_fragmented(&mut stream, &mut tls, audio.channel, &payload, &mut dump, "AUDIO_CHANNEL_CLOSE_NOTIFICATION") {
                Ok(count) => {
                    tx_ssl_frames += count;
                    println!("SESSION_CLEANUP tx encrypted AUDIO_CHANNEL_CLOSE_NOTIFICATION frame(s): count={} channel=0x{:02x} service_id={} stream={}", count, audio.channel, audio.service_id, audio.stream_type);
                    let _ = log_line(&mut dump, &format!("SESSION_CLEANUP tx encrypted AUDIO_CHANNEL_CLOSE_NOTIFICATION frame(s): count={} channel=0x{:02x} service_id={} stream={}", count, audio.channel, audio.service_id, audio.stream_type));
                }
                Err(err) => {
                    println!("SESSION_CLEANUP audio channel close failed channel=0x{:02x}: {}", audio.channel, err);
                    let _ = log_line(&mut dump, &format!("SESSION_CLEANUP audio channel close failed channel=0x{:02x}: {}", audio.channel, err));
                }
            }
        }
        if input_channel_opened {
            if let Some(channel) = selected_input_service_id.map(|id| id as u8) {
                match write_encrypted_plaintext_fragmented(&mut stream, &mut tls, channel, &payload, &mut dump, "INPUT_CHANNEL_CLOSE_NOTIFICATION") {
                    Ok(count) => {
                        tx_ssl_frames += count;
                        println!("SESSION_CLEANUP tx encrypted INPUT_CHANNEL_CLOSE_NOTIFICATION frame(s): count={} channel=0x{:02x}", count, channel);
                        let _ = log_line(&mut dump, &format!("SESSION_CLEANUP tx encrypted INPUT_CHANNEL_CLOSE_NOTIFICATION frame(s): count={} channel=0x{:02x}", count, channel));
                    }
                    Err(err) => {
                        println!("SESSION_CLEANUP input channel close failed channel=0x{:02x}: {}", channel, err);
                        let _ = log_line(&mut dump, &format!("SESSION_CLEANUP input channel close failed channel=0x{:02x}: {}", channel, err));
                    }
                }
            }
        }
        if video_channel_opened {
            if let Some(channel) = selected_video_service_id.map(|id| id as u8) {
                match write_encrypted_plaintext_fragmented(&mut stream, &mut tls, channel, &payload, &mut dump, "VIDEO_CHANNEL_CLOSE_NOTIFICATION") {
                    Ok(count) => {
                        tx_ssl_frames += count;
                        println!("SESSION_CLEANUP tx encrypted VIDEO_CHANNEL_CLOSE_NOTIFICATION frame(s): count={} channel=0x{:02x}", count, channel);
                        let _ = log_line(&mut dump, &format!("SESSION_CLEANUP tx encrypted VIDEO_CHANNEL_CLOSE_NOTIFICATION frame(s): count={} channel=0x{:02x}", count, channel));
                    }
                    Err(err) => {
                        println!("SESSION_CLEANUP video channel close failed channel=0x{:02x}: {}", channel, err);
                        let _ = log_line(&mut dump, &format!("SESSION_CLEANUP video channel close failed channel=0x{:02x}: {}", channel, err));
                    }
                }
            }
        }
    }

    } else {
        let line = format!(
            "SESSION_CLEANUP peer notifications skipped reason={}",
            cleanup_reason
        );
        println!("{line}");
        let _ = log_line(&mut dump, &line);
    }

    if let Some(injector) = input_touch_injector.as_mut() {
        let _ = injector.force_up();
        println!("SESSION_CLEANUP uinput force_up sent");
        let _ = log_line(&mut dump, "SESSION_CLEANUP uinput force_up sent");
    }
    if let Some(injector) = input_bridge_injector.as_mut() {
        injector.stop();
        println!("SESSION_CLEANUP platform input bridge client stopped");
        let _ = log_line(&mut dump, "SESSION_CLEANUP platform input bridge client stopped");
    }
    if let Some(injector) = input_app_injector.as_mut() {
        injector.stop();
        println!("SESSION_CLEANUP app_process input injector stopped");
        let _ = log_line(&mut dump, "SESSION_CLEANUP app_process input injector stopped");
    }
    if let Some(capture) = audio_capture.as_mut() {
        capture.stop();
        println!("SESSION_CLEANUP app_process audio capture stopped");
        let _ = log_line(&mut dump, "SESSION_CLEANUP app_process audio capture stopped");
    }
    if let Some(sender) = video_sender.as_mut() {
        sender.stop();
        println!("SESSION_CLEANUP video sender stopped");
        let _ = log_line(&mut dump, "SESSION_CLEANUP video sender stopped");
    }
    println!("SESSION_CLEANUP end reason={}", cleanup_reason);
    let _ = log_line(&mut dump, &format!("SESSION_CLEANUP end reason={}", cleanup_reason));

    let elapsed = started.elapsed();
    let audio_tx_packets: u64 = audio_output_channels.iter().map(|audio| audio.tx_packets).sum();
    let audio_tx_bytes: u64 = audio_output_channels.iter().map(|audio| audio.tx_bytes).sum();
    println!("summary: frames={frames} rx_ssl_frames={rx_ssl_frames} tx_ssl_frames={tx_ssl_frames} encrypted_frames={encrypted_frames} decrypted_frames={decrypted_frames} service_request_sent={service_request_sent} channel_open_sent={channel_open_sent} input_channel_open_sent={input_channel_open_sent} input_channel_opened={input_channel_opened} input_key_binding_sent={input_key_binding_sent} video_channel_opened={video_channel_opened} video_setup_sent={video_setup_sent} video_start_sent={video_start_sent} selected_video_service_id={selected_video_service_id:?} selected_input_service_id={selected_input_service_id:?} audio_channel_open_requests_sent={audio_channel_open_requests_sent} audio_channels={} audio_data_packets={audio_data_packets} audio_data_bytes={audio_data_bytes} audio_tx_packets={audio_tx_packets} audio_tx_bytes={audio_tx_bytes} mic_channel_open_requests_sent={mic_channel_open_requests_sent} mic_opened={} mic_data_packets={mic_data_packets} mic_data_bytes={mic_data_bytes} sensor_channel_open_requests_sent={sensor_channel_open_requests_sent} sensor_opened={} sensor_requests_sent={sensor_requests_sent} sensor_batches_seen={sensor_batches_seen} sensor_errors_seen={sensor_errors_seen} input_touch_inject_events={input_touch_inject_events} input_key_inject_events={input_key_inject_events} decrypted_bytes={decrypted_bytes} bytes={bytes} elapsed_ms={}", audio_output_channels.len(), mic_source_channel.as_ref().map(|m| m.opened).unwrap_or(false), sensor_channel.as_ref().map(|s| s.opened).unwrap_or(false), elapsed.as_millis());
    log_line(&mut dump, &format!("summary: frames={frames} rx_ssl_frames={rx_ssl_frames} tx_ssl_frames={tx_ssl_frames} encrypted_frames={encrypted_frames} decrypted_frames={decrypted_frames} service_request_sent={service_request_sent} channel_open_sent={channel_open_sent} input_channel_open_sent={input_channel_open_sent} input_channel_opened={input_channel_opened} input_key_binding_sent={input_key_binding_sent} video_channel_opened={video_channel_opened} video_setup_sent={video_setup_sent} video_start_sent={video_start_sent} selected_video_service_id={selected_video_service_id:?} selected_input_service_id={selected_input_service_id:?} audio_channel_open_requests_sent={audio_channel_open_requests_sent} audio_channels={} audio_data_packets={audio_data_packets} audio_data_bytes={audio_data_bytes} audio_tx_packets={audio_tx_packets} audio_tx_bytes={audio_tx_bytes} mic_channel_open_requests_sent={mic_channel_open_requests_sent} mic_opened={} mic_data_packets={mic_data_packets} mic_data_bytes={mic_data_bytes} sensor_channel_open_requests_sent={sensor_channel_open_requests_sent} sensor_opened={} sensor_requests_sent={sensor_requests_sent} sensor_batches_seen={sensor_batches_seen} sensor_errors_seen={sensor_errors_seen} input_touch_inject_events={input_touch_inject_events} input_key_inject_events={input_key_inject_events} decrypted_bytes={decrypted_bytes} bytes={bytes} elapsed_ms={}", audio_output_channels.len(), mic_source_channel.as_ref().map(|m| m.opened).unwrap_or(false), sensor_channel.as_ref().map(|s| s.opened).unwrap_or(false), elapsed.as_millis()))?;
    Ok(())
}



#[derive(Debug)]
enum VideoSender {
    File(VideoFileSender),
    Live(VideoLiveSender),
}

impl VideoSender {
    fn mark_media_start_sent(&mut self) {
        if let Self::Live(sender) = self {
            sender.mark_media_start_sent();
        }
    }

    fn on_ack(&mut self, ack_value: u32) -> Option<String> {
        match self {
            Self::File(sender) => {
                sender.on_ack(ack_value);
                None
            }
            Self::Live(sender) => sender.on_ack(ack_value),
        }
    }

    fn stop(&mut self) {
        match self {
            Self::File(_) => {}
            Self::Live(sender) => sender.stop_child(),
        }
    }

    fn configure_from_sdr(
        &mut self,
        selection: &proto::VideoConfigSelection,
        dump: &mut Option<Box<dyn Write>>,
    ) -> io::Result<()> {
        match self {
            Self::Live(sender) => sender.configure_from_sdr(selection, dump),
            Self::File(_) => {
                let line = format!(
                    "VIDEO_AUTO_CONFIG ignored for file sender: SDR selected {}x{} fps={} density={} margins={}x{}",
                    selection.width,
                    selection.height,
                    selection.fps,
                    selection.density,
                    selection.width_margin,
                    selection.height_margin
                );
                println!("{line}");
                log_line(dump, &line)
            }
        }
    }

    fn send_available<W: Write + ?Sized>(
        &mut self,
        stream: &mut W,
        tls: &mut tls::AapTlsServer,
        channel: u8,
        session_id: i32,
        max_unacked: u32,
        dump: &mut Option<Box<dyn Write>>,
    ) -> io::Result<usize> {
        match self {
            Self::File(sender) => sender.send_available(stream, tls, channel, session_id, max_unacked, dump),
            Self::Live(sender) => sender.send_available(stream, tls, channel, session_id, max_unacked, dump),
        }
    }
}

#[derive(Debug)]
struct VideoFileSender {
    path: PathBuf,
    codec_config: Option<Vec<u8>>,
    frames: Vec<Vec<u8>>,
    next_frame: usize,
    unacked: u32,
    highest_ack: u32,
    codec_config_sent: bool,
}

impl VideoFileSender {
    fn load(path: PathBuf, frame_limit: usize) -> io::Result<Self> {
        let data = std::fs::read(&path).map_err(|err| {
            io::Error::new(
                err.kind(),
                format!("cannot read --video-file {}: {err}", path.display()),
            )
        })?;
        let mut parser = AnnexBStreamParser::new();
        parser.push(&data);

        let mut codec_config = Vec::new();
        let mut frames = Vec::new();
        while let Some(nal) = parser.next_nal().or_else(|| parser.finish()) {
            match nal.nal_type() {
                NalType::Sps | NalType::Pps => codec_config.extend_from_slice(nal.bytes()),
                NalType::SliceIdr | NalType::SliceNonIdr => {
                    frames.push(nal.bytes().to_vec());
                    if frame_limit > 0 && frames.len() >= frame_limit {
                        break;
                    }
                }
                NalType::Sei | NalType::Aud => {
                    // Keep these out of the minimal first sender. Once full access-unit
                    // grouping is needed, prepend them to the next slice frame.
                }
                _ => {}
            }
        }

        println!(
            "video file loaded: path={} codec_config_bytes={} video_frames={} frame_limit={}",
            path.display(),
            codec_config.len(),
            frames.len(),
            frame_limit
        );

        Ok(Self {
            path,
            codec_config: if codec_config.is_empty() { None } else { Some(codec_config) },
            frames,
            next_frame: 0,
            unacked: 0,
            highest_ack: 0,
            codec_config_sent: false,
        })
    }

    fn on_ack(&mut self, ack_value: u32) {
        // In this DHU trace Ack.ack is reported as 1 for each acknowledged
        // media buffer, not as a monotonically increasing cumulative counter.
        // Treat every ACK as releasing at least one in-flight DATA frame.
        let released = ack_value.max(1);
        self.highest_ack = self.highest_ack.max(ack_value);
        self.unacked = self.unacked.saturating_sub(released);
    }

    fn send_available<W: Write + ?Sized>(
        &mut self,
        stream: &mut W,
        tls: &mut tls::AapTlsServer,
        channel: u8,
        session_id: i32,
        max_unacked: u32,
        dump: &mut Option<Box<dyn Write>>,
    ) -> io::Result<usize> {
        let mut tx_frames = 0usize;
        if !self.codec_config_sent {
            if let Some(config) = self.codec_config.as_ref() {
                let payload = proto::build_media_codec_config_payload(config);
                println!(
                    "VIDEO_FILE sending MEDIA_MESSAGE_CODEC_CONFIG bytes={} source={}",
                    config.len(),
                    self.path.display()
                );
                log_line(
                    dump,
                    &format!(
                        "VIDEO_FILE sending MEDIA_MESSAGE_CODEC_CONFIG bytes={} source={}",
                        config.len(),
                        self.path.display()
                    ),
                )?;
                tx_frames += write_encrypted_plaintext_fragmented(
                    stream,
                    tls,
                    channel,
                    &payload,
                    dump,
                    "MEDIA_MESSAGE_CODEC_CONFIG",
                )?;
            } else {
                println!("VIDEO_FILE no SPS/PPS codec config found in {}", self.path.display());
                log_line(dump, &format!("VIDEO_FILE no SPS/PPS codec config found in {}", self.path.display()))?;
            }
            self.codec_config_sent = true;
        }

        let max_unacked = max_unacked.max(1);
        while self.next_frame < self.frames.len() && self.unacked < max_unacked {
            let frame_index = self.next_frame;
            let pts_us = (frame_index as u64) * 33_333;
            let payload = proto::build_media_data_payload(pts_us, &self.frames[frame_index]);
            println!(
                "VIDEO_FILE sending MEDIA_MESSAGE_DATA frame={} pts_us={} bytes={} unacked={}/{} session_id={}",
                frame_index,
                pts_us,
                self.frames[frame_index].len(),
                self.unacked + 1,
                max_unacked,
                session_id
            );
            log_line(
                dump,
                &format!(
                    "VIDEO_FILE sending MEDIA_MESSAGE_DATA frame={} pts_us={} bytes={} unacked={}/{} session_id={}",
                    frame_index,
                    pts_us,
                    self.frames[frame_index].len(),
                    self.unacked + 1,
                    max_unacked,
                    session_id
                ),
            )?;
            tx_frames += write_encrypted_plaintext_fragmented(
                stream,
                tls,
                channel,
                &payload,
                dump,
                "MEDIA_MESSAGE_DATA",
            )?;
            self.next_frame += 1;
            self.unacked += 1;
        }

        if self.next_frame >= self.frames.len() && self.unacked == 0 {
            println!("VIDEO_FILE finished: frames_sent={} source={}", self.frames.len(), self.path.display());
            log_line(dump, &format!("VIDEO_FILE finished: frames_sent={} source={}", self.frames.len(), self.path.display()))?;
        }

        Ok(tx_frames)
    }
}

#[derive(Debug, Clone)]
struct VideoLiveConfig {
    width: i32,
    height: i32,
    bit_rate: i32,
    fps: i32,
    density: u32,
    /// 0 means unlimited; otherwise stop sending after this many video slice NALs.
    frame_limit: usize,
    apply_wm_config: bool,
    wm_size_mode: String,
    wm_density_mode: String,
    wm_settle_ms: u64,
    startup_timing: bool,
    first_frame_timeout_ms: u64,
    first_frame_max_restarts: u32,
    native_prewarm_ms: u64,
    first_frame_kick: bool,
    first_frame_kick_delay_ms: u64,
    first_frame_kick_mode: String,
    first_frame_kick_x: i32,
    first_frame_kick_y: i32,
    first_frame_kick_keycode: u32,
    display_auto: bool,
    wm_rotation: String,
    wm_force_geometry: bool,
    wm_size_orientation: String,
}

#[derive(Debug)]
enum LiveNalEvent {
    Status(String),
    Nal(NalType, Vec<u8>),
    Ended(String),
}

#[derive(Debug)]
struct VideoLiveSender {
    config: VideoLiveConfig,
    child: Option<Child>,
    rx: Option<mpsc::Receiver<LiveNalEvent>>,
    codec_config: Vec<u8>,
    codec_config_sent: bool,
    no_codec_config_warned: bool,
    next_pts_index: usize,
    frames_sent: usize,
    unacked: u32,
    highest_ack: u32,
    ended: bool,
    wm_applied: bool,
    wm_settled: bool,
    created_at: Instant,
    media_start_at: Option<Instant>,
    child_spawned_at: Option<Instant>,
    first_stdout_at: Option<Instant>,
    first_video_data_sent_at: Option<Instant>,
    first_ack_at: Option<Instant>,
    first_frame_restart_count: u32,
    prewarm_done: bool,
}


#[derive(Debug, Clone, Copy)]
struct WmSizeInfo {
    physical_width: i32,
    physical_height: i32,
    override_width: Option<i32>,
    override_height: Option<i32>,
}

fn run_text_command(program: &str, args: &[&str]) -> io::Result<String> {
    let out = Command::new(program).args(args).output()?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !stderr.trim().is_empty() {
            if !text.ends_with('\n') && !text.is_empty() {
                text.push('\n');
            }
            text.push_str(stderr.trim());
        }
    }
    Ok(text)
}

fn parse_wm_size_line(line: &str) -> Option<(i32, i32)> {
    let (_, rhs) = line.split_once(':')?;
    let mut parts = rhs.trim().split('x');
    let w = parts.next()?.trim().parse::<i32>().ok()?;
    let h = parts.next()?.trim().parse::<i32>().ok()?;
    Some((w, h))
}

fn probe_wm_size() -> Option<(WmSizeInfo, String)> {
    let text = run_text_command("wm", &["size"]).ok()?;
    let mut physical = None;
    let mut override_size = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Physical size:") {
            physical = parse_wm_size_line(trimmed);
        } else if trimmed.starts_with("Override size:") {
            override_size = parse_wm_size_line(trimmed);
        }
    }
    let (physical_width, physical_height) = physical?;
    let (override_width, override_height) = match override_size {
        Some((w, h)) => (Some(w), Some(h)),
        None => (None, None),
    };
    Some((
        WmSizeInfo {
            physical_width,
            physical_height,
            override_width,
            override_height,
        },
        text.trim().replace('\n', " | "),
    ))
}

fn rotation_degrees_to_quarter(value: &str, target_width: i32, target_height: i32, wm: Option<WmSizeInfo>) -> Option<&'static str> {
    match value {
        "0" => Some("0"),
        "90" => Some("1"),
        "180" => Some("2"),
        "270" => Some("3"),
        "auto" => {
            let info = wm?;
            let target_landscape = target_width >= target_height;
            let physical_landscape = info.physical_width >= info.physical_height;
            let target_aspect = target_width.max(1) as f32 / target_height.max(1) as f32;
            let physical_aspect = info.physical_width.max(1) as f32 / info.physical_height.max(1) as f32;
            let aspect_mismatch = (target_aspect - physical_aspect).abs() > 0.35;
            if target_landscape && (!physical_landscape || aspect_mismatch) {
                // We cannot know the physical mounting direction from software.
                // 270 is the safer default for the Waveshare 7.9 case observed here
                // where the visible panel image was clockwise-rotated.
                Some("3")
            } else if !target_landscape && physical_landscape {
                Some("1")
            } else {
                None
            }
        }
        _ => None,
    }
}

impl VideoLiveSender {
    fn new(config: VideoLiveConfig) -> Self {
        Self {
            config,
            child: None,
            rx: None,
            codec_config: Vec::new(),
            codec_config_sent: false,
            no_codec_config_warned: false,
            next_pts_index: 0,
            frames_sent: 0,
            unacked: 0,
            highest_ack: 0,
            ended: false,
            wm_applied: false,
            wm_settled: false,
            created_at: Instant::now(),
            media_start_at: None,
            child_spawned_at: None,
            first_stdout_at: None,
            first_video_data_sent_at: None,
            first_ack_at: None,
            first_frame_restart_count: 0,
            prewarm_done: false,
        }
    }

    fn mark_media_start_sent(&mut self) {
        if self.media_start_at.is_none() {
            self.media_start_at = Some(Instant::now());
        }
    }

    fn ms_since_media_start(&self, at: Instant) -> u128 {
        self.media_start_at
            .map(|start| at.saturating_duration_since(start).as_millis())
            .unwrap_or_else(|| at.saturating_duration_since(self.created_at).as_millis())
    }

    fn on_ack(&mut self, ack_value: u32) -> Option<String> {
        // In this DHU trace Ack.ack is reported as 1 for each acknowledged
        // media buffer, not as a monotonically increasing cumulative counter.
        // Treat every ACK as releasing at least one in-flight DATA frame.
        let released = ack_value.max(1);
        self.highest_ack = self.highest_ack.max(ack_value);
        self.unacked = self.unacked.saturating_sub(released);
        if self.config.startup_timing && self.first_video_data_sent_at.is_some() && self.first_ack_at.is_none() {
            let now = Instant::now();
            self.first_ack_at = Some(now);
            return Some(format!(
                "VIDEO_START_LATENCY first_ack_ms={} ack={}",
                self.ms_since_media_start(now),
                ack_value
            ));
        }
        None
    }

    fn log_display_probe(&self, label: &str, dump: &mut Option<Box<dyn Write>>) -> io::Result<Option<WmSizeInfo>> {
        if !self.config.display_auto && self.config.wm_rotation == "none" {
            return Ok(None);
        }
        match probe_wm_size() {
            Some((info, text)) => {
                let line = format!(
                    "DISPLAY_AUTO {} wm_size='{}' physical={}x{} override={}",
                    label,
                    text,
                    info.physical_width,
                    info.physical_height,
                    match (info.override_width, info.override_height) {
                        (Some(w), Some(h)) => format!("{}x{}", w, h),
                        _ => "none".to_string(),
                    }
                );
                println!("{line}");
                log_line(dump, &line)?;
                Ok(Some(info))
            }
            None => {
                let line = format!("DISPLAY_AUTO {} wm_size_probe_failed", label);
                println!("{line}");
                log_line(dump, &line)?;
                Ok(None)
            }
        }
    }

    fn apply_rotation_if_needed(
        &self,
        target_width: i32,
        target_height: i32,
        initial_wm: Option<WmSizeInfo>,
        dump: &mut Option<Box<dyn Write>>,
    ) -> io::Result<()> {
        if self.config.wm_rotation == "none" {
            return Ok(());
        }
        let Some(quarter) = rotation_degrees_to_quarter(
            self.config.wm_rotation.as_str(),
            target_width,
            target_height,
            initial_wm,
        ) else {
            let line = format!(
                "DISPLAY_AUTO rotation skipped rotation={} target={}x{} note=no change needed or wm size unavailable",
                self.config.wm_rotation,
                target_width,
                target_height
            );
            println!("{line}");
            return log_line(dump, &line);
        };

        let status_cmd = Command::new("cmd")
            .arg("window")
            .arg("set-user-rotation")
            .arg("lock")
            .arg(quarter)
            .status();
        let status_accel = Command::new("settings")
            .arg("put")
            .arg("system")
            .arg("accelerometer_rotation")
            .arg("0")
            .status();
        let status_user = Command::new("settings")
            .arg("put")
            .arg("system")
            .arg("user_rotation")
            .arg(quarter)
            .status();
        let line = format!(
            "DISPLAY_AUTO rotation applied requested={} quarter={} target={}x{} cmd_window={:?} settings_accel={:?} settings_user={:?}",
            self.config.wm_rotation,
            quarter,
            target_width,
            target_height,
            status_cmd,
            status_accel,
            status_user
        );
        println!("{line}");
        log_line(dump, &line)?;
        thread::sleep(Duration::from_millis(250));
        Ok(())
    }

    fn log_wm_command(
        &self,
        label: &str,
        program: &str,
        args: &[&str],
        dump: &mut Option<Box<dyn Write>>,
    ) -> io::Result<()> {
        let output = Command::new(program).args(args).output();
        let line = match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).trim().replace('\n', " | ");
                let stderr = String::from_utf8_lossy(&out.stderr).trim().replace('\n', " | ");
                format!(
                    "WM_CONFIG command {}: {} {} -> status={:?} stdout='{}' stderr='{}'",
                    label,
                    program,
                    args.join(" "),
                    out.status,
                    stdout,
                    stderr
                )
            }
            Err(err) => format!(
                "WM_CONFIG command {}: {} {} -> error={}",
                label,
                program,
                args.join(" "),
                err
            ),
        };
        if self.config.display_auto || self.config.wm_force_geometry {
            println!("{line}");
        }
        log_line(dump, &line)
    }

    fn apply_wm_size_and_density(
        &self,
        size_arg: &str,
        density_arg: &str,
        dump: &mut Option<Box<dyn Write>>,
    ) -> io::Result<()> {
        if self.config.wm_force_geometry {
            // In DHU-priority mode we deliberately ignore the physical HDMI panel.
            // Reset first, then write the requested override through both frontends
            // because AAOS builds differ in which path actually updates WindowManager.
            self.log_wm_command("force-reset", "wm", &["size", "reset"], dump)?;
            self.log_wm_command("force-reset", "cmd", &["window", "size", "reset"], dump)?;
            thread::sleep(Duration::from_millis(150));
            for attempt in 1..=3 {
                let attempt_label = format!("force-size-attempt-{}", attempt);
                self.log_wm_command(&attempt_label, "wm", &["size", size_arg], dump)?;
                self.log_wm_command(&attempt_label, "cmd", &["window", "size", size_arg], dump)?;
                thread::sleep(Duration::from_millis(250));
                match probe_wm_size() {
                    Some((info, text)) => {
                        let matched = info.override_width == Some(size_arg.split('x').next().unwrap_or("0").parse::<i32>().unwrap_or(0))
                            && info.override_height == Some(size_arg.split('x').nth(1).unwrap_or("0").parse::<i32>().unwrap_or(0));
                        let line = format!(
                            "WM_CONFIG force-verify attempt={} requested={} wm_size='{}' matched={}",
                            attempt,
                            size_arg,
                            text.trim().replace('\n', " | "),
                            matched
                        );
                        println!("{line}");
                        log_line(dump, &line)?;
                        if matched {
                            break;
                        }
                    }
                    None => {
                        let line = format!("WM_CONFIG force-verify attempt={} requested={} wm_size_probe_failed", attempt, size_arg);
                        println!("{line}");
                        log_line(dump, &line)?;
                    }
                }
            }
            self.log_wm_command("force-density", "wm", &["density", density_arg], dump)?;
            self.log_wm_command("force-density", "cmd", &["window", "density", density_arg], dump)?;
        } else {
            self.log_wm_command("size", "wm", &["size", size_arg], dump)?;
            self.log_wm_command("density", "wm", &["density", density_arg], dump)?;
        }
        Ok(())
    }

    fn wm_density_for_selection(&self, selection: &proto::VideoConfigSelection) -> u32 {
        match self.config.wm_density_mode.as_str() {
            "real" | "real_density" => selection.real_density.max(1),
            _ => selection.density.max(1),
        }
    }

    fn wm_size_for_selection(&self, selection: &proto::VideoConfigSelection) -> (i32, i32, &'static str) {
        match self.config.wm_size_mode.as_str() {
            "content" | "content_area" => (selection.content_width, selection.content_height, "content"),
            "safe" | "safe_area" => (selection.safe_width, selection.safe_height, "safe"),
            _ => (selection.width, selection.height, "stream"),
        }
    }

    fn wm_command_size_for_target(
        &self,
        target_width: i32,
        target_height: i32,
        initial_wm: Option<WmSizeInfo>,
    ) -> (i32, i32, &'static str) {
        let right_angle_rotation = matches!(self.config.wm_rotation.as_str(), "90" | "270")
            || (self.config.wm_rotation == "auto"
                && initial_wm.map(|wm| wm.physical_width < wm.physical_height).unwrap_or(false)
                && target_width >= target_height);
        match self.config.wm_size_orientation.as_str() {
            "natural" if right_angle_rotation => (target_height, target_width, "natural-rotated"),
            "auto" if right_angle_rotation => (target_height, target_width, "auto-natural-rotated"),
            "natural" => (target_width, target_height, "natural-no-rotation"),
            "auto" => (target_width, target_height, "auto-logical"),
            _ => (target_width, target_height, "logical"),
        }
    }

    fn apply_wm_config_from_sdr(
        &mut self,
        selection: &proto::VideoConfigSelection,
        dump: &mut Option<Box<dyn Write>>,
    ) -> io::Result<()> {
        if !self.config.apply_wm_config {
            let line = format!(
                "WM_CONFIG not applied: stream={}x{} density={} real_density={} margins=L{} R{} T{} B{} content={}x{} safe={}x{}",
                selection.width,
                selection.height,
                selection.density,
                selection.real_density,
                selection.margin_left,
                selection.margin_right,
                selection.margin_top,
                selection.margin_bottom,
                selection.content_width,
                selection.content_height,
                selection.safe_width,
                selection.safe_height
            );
            println!("{line}");
            return log_line(dump, &line);
        }

        let (wm_width, wm_height, size_mode) = self.wm_size_for_selection(selection);
        let wm_density = self.wm_density_for_selection(selection);
        let initial_wm = self.log_display_probe("before", dump)?;
        self.apply_rotation_if_needed(wm_width, wm_height, initial_wm, dump)?;
        let (cmd_w, cmd_h, orientation_mode) = self.wm_command_size_for_target(wm_width, wm_height, initial_wm);
        let line = format!(
            "WM_CONFIG applying: logical_size={}x{} command_size={}x{} size_mode={} size_orientation={} density={} density_mode={} force_geometry={} stream={}x{} margins=L{} R{} T{} B{} content={}x{} safe={}x{}",
            wm_width,
            wm_height,
            cmd_w,
            cmd_h,
            size_mode,
            orientation_mode,
            wm_density,
            self.config.wm_density_mode,
            self.config.wm_force_geometry,
            selection.width,
            selection.height,
            selection.margin_left,
            selection.margin_right,
            selection.margin_top,
            selection.margin_bottom,
            selection.content_width,
            selection.content_height,
            selection.safe_width,
            selection.safe_height
        );
        println!("{line}");
        log_line(dump, &line)?;

        let size_arg = format!("{}x{}", cmd_w, cmd_h);
        let density_arg = wm_density.to_string();
        self.apply_wm_size_and_density(&size_arg, &density_arg, dump)?;
        let _ = self.log_display_probe("after", dump)?;
        self.wm_applied = true;
        Ok(())
    }

    fn apply_manual_wm_config_if_needed(
        &mut self,
        dump: &mut Option<Box<dyn Write>>,
    ) -> io::Result<()> {
        if !self.config.apply_wm_config || self.wm_applied {
            return Ok(());
        }

        let density_arg = self.config.density.max(1).to_string();
        let initial_wm = self.log_display_probe("before", dump)?;
        self.apply_rotation_if_needed(self.config.width, self.config.height, initial_wm, dump)?;
        let (cmd_w, cmd_h, orientation_mode) = self.wm_command_size_for_target(self.config.width, self.config.height, initial_wm);
        let size_arg = format!("{}x{}", cmd_w, cmd_h);
        let line = format!(
            "WM_CONFIG applying manual native config: logical_size={}x{} command_size={} size_orientation={} density={} size_mode={} density_mode={} force_geometry={} note=no SDR auto-config applied",
            self.config.width,
            self.config.height,
            size_arg,
            orientation_mode,
            density_arg,
            self.config.wm_size_mode,
            self.config.wm_density_mode,
            self.config.wm_force_geometry
        );
        println!("{line}");
        log_line(dump, &line)?;

        self.apply_wm_size_and_density(&size_arg, &density_arg, dump)?;
        let _ = self.log_display_probe("after", dump)?;
        self.wm_applied = true;
        Ok(())
    }

    fn configure_from_sdr(
        &mut self,
        selection: &proto::VideoConfigSelection,
        dump: &mut Option<Box<dyn Write>>,
    ) -> io::Result<()> {
        if self.child.is_some() {
            let line = "VIDEO_AUTO_CONFIG ignored because native capture child is already running";
            println!("{line}");
            return log_line(dump, line);
        }
        self.config.width = selection.width;
        self.config.height = selection.height;
        self.config.fps = selection.fps;
        self.config.density = self.wm_density_for_selection(selection);
        let line = format!(
            "VIDEO_NATIVE configured from SDR: stream_size={}x{} fps={} bit_rate={} density={} real_density={} total_margins={}x{} margins=L{} R{} T{} B{} content={}x{} safe={}x{} config_index={} codec={} resolution={}",
            self.config.width,
            self.config.height,
            self.config.fps,
            self.config.bit_rate,
            selection.density,
            selection.real_density,
            selection.width_margin,
            selection.height_margin,
            selection.margin_left,
            selection.margin_right,
            selection.margin_top,
            selection.margin_bottom,
            selection.content_width,
            selection.content_height,
            selection.safe_width,
            selection.safe_height,
            selection.config_index,
            selection.codec,
            selection.resolution
        );
        println!("{line}");
        log_line(dump, &line)?;
        self.apply_wm_config_from_sdr(selection, dump)
    }

    fn settle_after_wm_if_needed(&mut self, dump: &mut Option<Box<dyn Write>>) -> io::Result<()> {
        if !self.wm_applied || self.wm_settled || self.config.wm_settle_ms == 0 {
            return Ok(());
        }

        let line = format!(
            "WM_CONFIG settling before native capture: wait_ms={} note=screenrecord may stall if started while display is reconfiguring",
            self.config.wm_settle_ms
        );
        println!("{line}");
        log_line(dump, &line)?;
        let settle_start = Instant::now();
        thread::sleep(Duration::from_millis(self.config.wm_settle_ms));
        self.wm_settled = true;
        if self.config.startup_timing {
            let line = format!(
                "VIDEO_START_LATENCY wm_settle_done_ms={} wait_ms={}",
                self.ms_since_media_start(Instant::now()),
                settle_start.elapsed().as_millis()
            );
            println!("{line}");
            log_line(dump, &line)?;
        }
        Ok(())
    }

    fn spawn_screenrecord_capture_child(&self, stdout: Stdio, seconds: &str) -> io::Result<Child> {
        let size = format!("{}x{}", self.config.width, self.config.height);
        let bit_rate = self.config.bit_rate.to_string();
        let codec_name = preferred_codec_name();
        let stderr = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open("/data/local/tmp/arda-screenrecord-live.stderr.log")
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null());

        // Android 16's platform screenrecord already owns the matching
        // SurfaceFlinger -> virtual display -> MediaCodec surface path.  Keep
        // the Rust/AAP side responsible only for Annex-B parsing and AA media
        // flow control instead of dlsym'ing private libgui C++ ABI symbols.
        Command::new("/system/bin/screenrecord")
            .arg("--output-format=h264")
            .arg("--size")
            .arg(&size)
            .arg("--bit-rate")
            .arg(&bit_rate)
            .arg("--time-limit")
            .arg(seconds)
            .arg("--codec-name")
            .arg(&codec_name)
            .arg("-")
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
    }

    fn run_prewarm_if_needed(&mut self, dump: &mut Option<Box<dyn Write>>) -> io::Result<()> {
        if self.prewarm_done || self.config.native_prewarm_ms == 0 {
            return Ok(());
        }

        let prewarm_ms = self.config.native_prewarm_ms;
        let start = Instant::now();
        let mut child = self
            .spawn_screenrecord_capture_child(Stdio::null(), "0")
            .map_err(|err| io::Error::new(err.kind(), format!("failed to spawn screenrecord prewarm child: {err}")))?;

        let line = format!(
            "VIDEO_PREWARM started ms={} size={}x{} bit_rate={} fps={} codec={} note=warming platform screenrecord/encoder before real AA video stream",
            prewarm_ms,
            self.config.width,
            self.config.height,
            self.config.bit_rate,
            self.config.fps,
            preferred_codec_name()
        );
        println!("{line}");
        log_line(dump, &line)?;

        thread::sleep(Duration::from_millis(prewarm_ms));
        Self::terminate_screenrecord_capture_child(&mut child, Duration::from_millis(1500));
        self.prewarm_done = true;

        let elapsed_ms = start.elapsed().as_millis();
        let line = format!(
            "VIDEO_START_LATENCY prewarm_done_ms={} elapsed_ms={} requested_ms={}",
            self.ms_since_media_start(Instant::now()),
            elapsed_ms,
            prewarm_ms
        );
        println!("{line}");
        log_line(dump, &line)?;
        Ok(())
    }

    fn first_frame_kick_point(&self) -> (i32, i32) {
        let x = self.config.first_frame_kick_x;
        let y = self.config.first_frame_kick_y;
        if matches!(
            self.config.first_frame_kick_mode.as_str(),
            "center" | "center-cancel" | "center-move-cancel"
        ) {
            return ((self.config.width / 2).max(1), (self.config.height / 2).max(1));
        }
        (x.max(0), y.max(0))
    }

    fn first_frame_kick_command(&self) -> (String, Vec<String>) {
        let (x, y) = self.first_frame_kick_point();
        match self.config.first_frame_kick_mode.as_str() {
            "home" => ("/system/bin/input".to_string(), vec!["keyevent".to_string(), "3".to_string()]),
            "wakeup" => ("/system/bin/input".to_string(), vec!["keyevent".to_string(), "224".to_string()]),
            "keyevent" => ("/system/bin/input".to_string(), vec!["keyevent".to_string(), self.config.first_frame_kick_keycode.to_string()]),
            "wm-nudge" => {
                // Re-apply the current WM override size/density. This usually dirties
                // WindowManager/SurfaceFlinger without clicking anything in the UI.
                // It is safer than HOME/center tap, but may be a bit slower than a real touch.
                let script = r#"
size="$(/system/bin/wm size 2>/dev/null | /system/bin/sed -n 's/.*Override size: //p' | /system/bin/head -n 1)"
[ -z "$size" ] && size="$(/system/bin/wm size 2>/dev/null | /system/bin/sed -n 's/.*Physical size: //p' | /system/bin/head -n 1)"
if [ -n "$size" ]; then
  /system/bin/wm size "$size" >/dev/null 2>&1 || true
  /system/bin/cmd window size "$size" >/dev/null 2>&1 || true
fi
density="$(/system/bin/wm density 2>/dev/null | /system/bin/sed -n 's/.*Override density: //p' | /system/bin/head -n 1)"
[ -z "$density" ] && density="$(/system/bin/wm density 2>/dev/null | /system/bin/sed -n 's/.*Physical density: //p' | /system/bin/head -n 1)"
if [ -n "$density" ]; then
  /system/bin/wm density "$density" >/dev/null 2>&1 || true
  /system/bin/cmd window density "$density" >/dev/null 2>&1 || true
fi
"#
                .trim()
                .to_string();
                ("/system/bin/sh".to_string(), vec!["-c".to_string(), script])
            }
            "touch-cancel" | "center-cancel" => {
                // Android 11's `input motionevent` only supports DOWN/UP/MOVE, not CANCEL.
                // The fast path that worked on DHU is effectively a short DOWN-only dirty
                // pulse: it wakes SurfaceFlinger/encoder without delivering ACTION_UP/click.
                // Keep the user-facing name center-cancel because it means "no click".
                let script = format!(
                    "/system/bin/input touchscreen motionevent DOWN {x} {y}; /system/bin/sleep 0.03"
                );
                ("/system/bin/sh".to_string(), vec!["-c".to_string(), script])
            }
            "center-move-cancel" => {
                // MOVE without ACTION_UP avoids click, while still dirtying maps/compositor.
                let x2 = x.saturating_add(1);
                let y2 = y.saturating_add(1);
                let script = format!(
                    "/system/bin/input touchscreen motionevent DOWN {x} {y}; /system/bin/sleep 0.02; /system/bin/input touchscreen motionevent MOVE {x2} {y2}; /system/bin/sleep 0.02"
                );
                ("/system/bin/sh".to_string(), vec!["-c".to_string(), script])
            }
            "center" => (
                "/system/bin/input".to_string(),
                vec!["tap".to_string(), x.to_string(), y.to_string()],
            ),
            _ => (
                "/system/bin/input".to_string(),
                vec!["tap".to_string(), x.to_string(), y.to_string()],
            ),
        }
    }

    fn maybe_kick_first_frame(&mut self, dump: &mut Option<Box<dyn Write>>) -> io::Result<()> {
        if !self.config.first_frame_kick {
            return Ok(());
        }
        let delay_ms = self.config.first_frame_kick_delay_ms;
        if delay_ms > 0 {
            thread::sleep(Duration::from_millis(delay_ms));
        }
        let (program, args) = self.first_frame_kick_command();
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let started = Instant::now();
        let result = run_text_command(&program, &arg_refs);
        let elapsed_ms = started.elapsed().as_millis();
        match result {
            Ok(text) => {
                let line = format!(
                    "VIDEO_FIRST_FRAME_KICK sent mode={} delay_ms={} cmd='{} {}' elapsed_ms={} output='{}'",
                    self.config.first_frame_kick_mode,
                    delay_ms,
                    program,
                    args.join(" "),
                    elapsed_ms,
                    text.trim().replace('\n', " | ")
                );
                println!("{line}");
                log_line(dump, &line)?;
            }
            Err(err) => {
                let line = format!(
                    "VIDEO_FIRST_FRAME_KICK failed mode={} delay_ms={} cmd='{} {}' elapsed_ms={} error={}",
                    self.config.first_frame_kick_mode,
                    delay_ms,
                    program,
                    args.join(" "),
                    elapsed_ms,
                    err
                );
                println!("{line}");
                log_line(dump, &line)?;
            }
        }
        Ok(())
    }

    fn ensure_started(&mut self, dump: &mut Option<Box<dyn Write>>) -> io::Result<()> {
        if self.child.is_some() || self.ended {
            return Ok(());
        }

        self.apply_manual_wm_config_if_needed(dump)?;
        self.settle_after_wm_if_needed(dump)?;
        self.run_prewarm_if_needed(dump)?;

        let size = format!("{}x{}", self.config.width, self.config.height);
        let bit_rate = self.config.bit_rate.to_string();
        let fps = self.config.fps.to_string();
        // Child lifetime is controlled by this session. 0 means capture until killed.
        let seconds = String::from("0");

        let mut child = self
            .spawn_screenrecord_capture_child(Stdio::piped(), &seconds)
            .map_err(|err| io::Error::new(err.kind(), format!("failed to spawn screenrecord capture child: {err}")))?;

        let child_spawned_at = Instant::now();
        self.child_spawned_at = Some(child_spawned_at);
        if self.config.startup_timing {
            let line = format!("VIDEO_START_LATENCY child_spawned_ms={}", self.ms_since_media_start(child_spawned_at));
            println!("{line}");
            log_line(dump, &line)?;
        }

        let Some(mut stdout) = child.stdout.take() else {
            Self::terminate_screenrecord_capture_child(&mut child, Duration::from_millis(1500));
            return Err(io::Error::new(io::ErrorKind::Other, "screenrecord capture child stdout missing"));
        };

        let (tx, rx) = mpsc::channel::<LiveNalEvent>();
        thread::spawn(move || {
            let mut parser = AnnexBStreamParser::new();
            let mut buf = [0u8; 64 * 1024];
            let mut reads = 0u64;
            let mut nals = 0u64;
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        reads += 1;
                        if reads == 1 {
                            let _ = tx.send(LiveNalEvent::Status(format!(
                                "VIDEO_NATIVE stdout first read bytes={}",
                                n
                            )));
                        }
                        parser.push(&buf[..n]);
                        while let Some(nal) = parser.next_nal() {
                            let ty = nal.nal_type();
                            nals += 1;
                            if nals <= 4 {
                                let _ = tx.send(LiveNalEvent::Status(format!(
                                    "VIDEO_NATIVE parser nal#{} type={:?} bytes={}",
                                    nals,
                                    ty,
                                    nal.bytes().len()
                                )));
                            }
                            if tx.send(LiveNalEvent::Nal(ty, nal.bytes().to_vec())).is_err() {
                                return;
                            }
                        }
                    }
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                    Err(err) => {
                        let _ = tx.send(LiveNalEvent::Ended(format!("native-capture stdout read error: {err}")));
                        return;
                    }
                }
            }
            if let Some(nal) = parser.finish() {
                let ty = nal.nal_type();
                let _ = tx.send(LiveNalEvent::Nal(ty, nal.bytes().to_vec()));
            }
            let _ = tx.send(LiveNalEvent::Ended("native-capture stdout EOF".to_string()));
        });

        let line = format!(
            "VIDEO_NATIVE started child: size={} bit_rate={} fps={} codec={} frame_limit={}",
            size,
            bit_rate,
            fps,
            preferred_codec_name(),
            self.config.frame_limit
        );
        println!("{line}");
        log_line(dump, &line)?;
        log_line(dump, &format!("VIDEO_NATIVE backend=platform-screenrecord codec={} stderr_log=/data/local/tmp/arda-screenrecord-live.stderr.log", preferred_codec_name()))?;

        self.child = Some(child);
        self.rx = Some(rx);
        self.maybe_kick_first_frame(dump)?;
        Ok(())
    }


    fn first_frame_timeout_elapsed(&self) -> bool {
        if self.frames_sent > 0 || self.config.first_frame_timeout_ms == 0 {
            return false;
        }
        let Some(spawned_at) = self.child_spawned_at else {
            return false;
        };
        spawned_at.elapsed() >= Duration::from_millis(self.config.first_frame_timeout_ms)
            && self.first_frame_restart_count < self.config.first_frame_max_restarts
    }

    fn reset_child_for_first_frame_retry(&mut self, dump: &mut Option<Box<dyn Write>>) -> io::Result<()> {
        self.first_frame_restart_count += 1;
        let elapsed_ms = self
            .child_spawned_at
            .map(|at| at.elapsed().as_millis())
            .unwrap_or(0);
        let line = format!(
            "VIDEO_START_LATENCY first_frame_timeout_restart attempt={}/{} elapsed_ms={} timeout_ms={} note=no slice NAL received yet; restarting screenrecord capture child",
            self.first_frame_restart_count,
            self.config.first_frame_max_restarts,
            elapsed_ms,
            self.config.first_frame_timeout_ms
        );
        println!("{line}");
        log_line(dump, &line)?;

        if let Some(child) = self.child.as_mut() {
            Self::terminate_screenrecord_capture_child(child, Duration::from_millis(1500));
        }
        self.child = None;
        self.rx = None;
        self.codec_config.clear();
        self.codec_config_sent = false;
        self.no_codec_config_warned = false;
        self.next_pts_index = 0;
        self.unacked = 0;
        self.highest_ack = 0;
        self.ended = false;
        self.child_spawned_at = None;
        self.first_stdout_at = None;
        self.first_video_data_sent_at = None;
        self.first_ack_at = None;
        Ok(())
    }

    fn send_available<W: Write + ?Sized>(
        &mut self,
        stream: &mut W,
        tls: &mut tls::AapTlsServer,
        channel: u8,
        session_id: i32,
        max_unacked: u32,
        dump: &mut Option<Box<dyn Write>>,
    ) -> io::Result<usize> {
        self.ensure_started(dump)?;

        if self.config.frame_limit > 0 && self.frames_sent >= self.config.frame_limit {
            self.stop_child();
            return Ok(0);
        }

        let mut tx_frames = 0usize;
        let max_unacked = max_unacked.max(1);

        loop {
            if self.unacked >= max_unacked {
                break;
            }
            if self.config.frame_limit > 0 && self.frames_sent >= self.config.frame_limit {
                self.stop_child();
                break;
            }

            let event = match self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
                Some(event) => event,
                None => break,
            };

            match event {
                LiveNalEvent::Status(line) => {
                    if self.config.startup_timing && line.starts_with("VIDEO_NATIVE stdout first read") && self.first_stdout_at.is_none() {
                        let now = Instant::now();
                        self.first_stdout_at = Some(now);
                        let timing = format!("VIDEO_START_LATENCY first_stdout_ms={} {}", self.ms_since_media_start(now), line);
                        println!("{timing}");
                        log_line(dump, &timing)?;
                    } else {
                        session_log(dump, LOG_SUMMARY, &line)?;
                    }
                }
                LiveNalEvent::Nal(NalType::Sps, bytes) | LiveNalEvent::Nal(NalType::Pps, bytes) => {
                    self.codec_config.extend_from_slice(&bytes);
                    // If the encoder emits a new SPS/PPS set later, send it before following frames.
                    if self.codec_config_sent {
                        self.codec_config_sent = false;
                    }
                }
                LiveNalEvent::Nal(NalType::SliceIdr, bytes) | LiveNalEvent::Nal(NalType::SliceNonIdr, bytes) => {
                    if !self.codec_config_sent {
                        if self.codec_config.is_empty() {
                            if !self.no_codec_config_warned {
                                let line = "VIDEO_NATIVE warning: sending H264 DATA before SPS/PPS codec config";
                                println!("{line}");
                                log_line(dump, line)?;
                                self.no_codec_config_warned = true;
                            }
                        } else {
                            let payload = proto::build_media_codec_config_payload(&self.codec_config);
                            println!(
                                "VIDEO_NATIVE sending MEDIA_MESSAGE_CODEC_CONFIG bytes={}",
                                self.codec_config.len()
                            );
                            log_line(
                                dump,
                                &format!("VIDEO_NATIVE sending MEDIA_MESSAGE_CODEC_CONFIG bytes={}", self.codec_config.len()),
                            )?;
                            tx_frames += write_encrypted_plaintext_fragmented(
                                stream,
                                tls,
                                channel,
                                &payload,
                                dump,
                                "MEDIA_MESSAGE_CODEC_CONFIG",
                            )?;
                            if self.config.startup_timing {
                                let now = Instant::now();
                                let line = format!("VIDEO_START_LATENCY codec_config_sent_ms={} bytes={}", self.ms_since_media_start(now), self.codec_config.len());
                                println!("{line}");
                                log_line(dump, &line)?;
                            }
                            self.codec_config.clear();
                        }
                        self.codec_config_sent = true;
                    }

                    let pts_us = (self.next_pts_index as u64) * 1_000_000u64 / (self.config.fps as u64).max(1);
                    self.next_pts_index += 1;
                    let payload = proto::build_media_data_payload(pts_us, &bytes);
                    if session_log_enabled(LOG_VERBOSE) {
                        println!(
                            "VIDEO_NATIVE sending MEDIA_MESSAGE_DATA frame={} pts_us={} bytes={} unacked={}/{} session_id={}",
                            self.frames_sent,
                            pts_us,
                            bytes.len(),
                            self.unacked + 1,
                            max_unacked,
                            session_id
                        );
                        log_line(
                            dump,
                            &format!(
                                "VIDEO_NATIVE sending MEDIA_MESSAGE_DATA frame={} pts_us={} bytes={} unacked={}/{} session_id={}",
                                self.frames_sent,
                                pts_us,
                                bytes.len(),
                                self.unacked + 1,
                                max_unacked,
                                session_id
                            ),
                        )?;
                    }
                    tx_frames += write_encrypted_plaintext_fragmented(
                        stream,
                        tls,
                        channel,
                        &payload,
                        dump,
                        "MEDIA_MESSAGE_DATA",
                    )?;
                    if self.config.startup_timing && self.first_video_data_sent_at.is_none() {
                        let now = Instant::now();
                        self.first_video_data_sent_at = Some(now);
                        let line = format!(
                            "VIDEO_START_LATENCY first_video_data_sent_ms={} frame={} bytes={} pts_us={}",
                            self.ms_since_media_start(now),
                            self.frames_sent,
                            bytes.len(),
                            pts_us
                        );
                        println!("{line}");
                        log_line(dump, &line)?;
                    }
                    self.frames_sent += 1;
                    self.unacked += 1;
                }
                LiveNalEvent::Nal(NalType::Sei, _) | LiveNalEvent::Nal(NalType::Aud, _) => {
                    // Keep first live pipeline minimal, same as file sender.
                }
                LiveNalEvent::Nal(_, _) => {}
                LiveNalEvent::Ended(reason) => {
                    let line = format!("VIDEO_NATIVE ended: {reason}");
                    println!("{line}");
                    log_line(dump, &line)?;
                    self.ended = true;
                    self.stop_child();
                    break;
                }
            }
        }

        if self.first_frame_timeout_elapsed() {
            self.reset_child_for_first_frame_retry(dump)?;
        }

        if self.config.frame_limit > 0 && self.frames_sent >= self.config.frame_limit {
            let line = format!("VIDEO_NATIVE frame limit reached: frames_sent={}", self.frames_sent);
            println!("{line}");
            log_line(dump, &line)?;
            self.stop_child();
        }

        Ok(tx_frames)
    }

    fn terminate_screenrecord_capture_child(child: &mut Child, graceful_timeout: Duration) {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }

        // Platform screenrecord installs graceful handlers for SIGINT/SIGHUP.
        // Do not send SIGTERM here: current AOSP screenrecord does not catch it.
        // Let screenrecord destroy its virtual display and stop/release the
        // encoder first, then use SIGKILL only as a bounded fallback.
        unsafe {
            let _ = kill(child.id() as c_int, SIGINT);
        }

        let start = Instant::now();
        while start.elapsed() < graceful_timeout {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => thread::sleep(Duration::from_millis(40)),
                Err(_) => break,
            }
        }

        let _ = child.kill();
        let _ = child.wait();
    }

    fn stop_child(&mut self) {
        if let Some(child) = self.child.as_mut() {
            Self::terminate_screenrecord_capture_child(child, Duration::from_millis(2500));
        }
        self.child = None;
        self.ended = true;
    }
}

impl Drop for VideoLiveSender {
    fn drop(&mut self) {
        self.stop_child();
    }
}



#[derive(Debug, Clone)]
struct InputTouchMap {
    source_width: i32,
    source_height: i32,
    stream_width: i32,
    stream_height: i32,
    target_width: i32,
    target_height: i32,
    x_offset: i32,
    y_offset: i32,
    mode: String,
}

impl InputTouchMap {
    fn from_config(
        config: &TlsHandshakeProbeConfig,
        input: Option<&proto::InputSourceSelection>,
        video: Option<&proto::VideoConfigSelection>,
    ) -> Self {
        let stream_width = video.map(|v| v.width).unwrap_or(config.video_width).max(1);
        let stream_height = video.map(|v| v.height).unwrap_or(config.video_height).max(1);

        let touchscreen = input
            .and_then(|i| i.touchscreens.iter().find(|ts| !ts.is_secondary).or_else(|| i.touchscreens.first()));
        let source_width = touchscreen.map(|ts| ts.width).unwrap_or(stream_width).max(1);
        let source_height = touchscreen.map(|ts| ts.height).unwrap_or(stream_height).max(1);

        let normalized_mode = match config.video_wm_size_mode.as_str() {
            "content" | "content_area" => "content",
            "safe" | "safe_area" => "safe",
            _ => "stream",
        };

        let (target_width, target_height, x_offset, y_offset) = match (normalized_mode, video) {
            ("content", Some(v)) => (
                v.content_width.max(1),
                v.content_height.max(1),
                v.margin_left as i32,
                v.margin_top as i32,
            ),
            ("safe", Some(v)) => (
                v.safe_width.max(1),
                v.safe_height.max(1),
                v.margin_left as i32 + v.content_inset_left as i32,
                v.margin_top as i32 + v.content_inset_top as i32,
            ),
            _ => (stream_width, stream_height, 0, 0),
        };

        Self {
            source_width,
            source_height,
            stream_width,
            stream_height,
            target_width,
            target_height,
            x_offset,
            y_offset,
            mode: normalized_mode.to_string(),
        }
    }

    fn map(&self, x: u32, y: u32) -> (i32, i32) {
        let stream_x = scale_coord(x as i64, self.source_width as i64, self.stream_width as i64);
        let stream_y = scale_coord(y as i64, self.source_height as i64, self.stream_height as i64);
        let mapped_x = clamp_i32(stream_x - self.x_offset, 0, self.target_width.saturating_sub(1));
        let mapped_y = clamp_i32(stream_y - self.y_offset, 0, self.target_height.saturating_sub(1));
        (mapped_x, mapped_y)
    }
}

fn select_touch_pointer(touch: &proto::DecodedTouchEvent) -> Option<&proto::DecodedPointer> {
    if touch.pointers.is_empty() {
        return None;
    }
    touch
        .pointers
        .get(touch.action_index as usize)
        .or_else(|| touch.pointers.iter().find(|p| p.pointer_id == 0))
        .or_else(|| touch.pointers.first())
}

fn scale_coord(value: i64, source_size: i64, target_size: i64) -> i32 {
    if source_size <= 0 || target_size <= 0 {
        return 0;
    }
    ((value * target_size + source_size / 2) / source_size) as i32
}

fn clamp_i32(value: i32, min: i32, max: i32) -> i32 {
    value.max(min).min(max.max(min))
}

fn is_control_message(payload: &[u8], id: u16) -> bool {
    payload.len() >= 2 && u16::from_be_bytes([payload[0], payload[1]]) == id
}


fn write_encrypted_plaintext_fragmented<W: Write + ?Sized>(
    stream: &mut W,
    tls: &mut tls::AapTlsServer,
    channel: u8,
    plaintext: &[u8],
    dump: &mut Option<Box<dyn Write>>,
    label: &str,
) -> io::Result<usize> {
    write_encrypted_plaintext_fragmented_with_flags(
        stream,
        tls,
        channel,
        0,
        plaintext,
        dump,
        label,
    )
}

fn write_encrypted_plaintext_fragmented_with_flags<W: Write + ?Sized>(
    stream: &mut W,
    tls: &mut tls::AapTlsServer,
    channel: u8,
    base_flags: u8,
    plaintext: &[u8],
    dump: &mut Option<Box<dyn Write>>,
    label: &str,
) -> io::Result<usize> {
    let first = fragment::DEFAULT_FIRST_FRAGMENT_PAYLOAD_BYTES;
    let continuation = fragment::openauto_continuation_fragment_payload_bytes(first);
    let plain_frames = fragment::fragment_plain_payload(
        plaintext,
        fragment::PlainPayloadFragmentOptions {
            channel,
            base_flags,
            first_fragment_payload_bytes: first,
            continuation_fragment_payload_bytes: continuation,
            first_final_length: Some(plaintext.len() as u32),
        },
    );

    let multi = plain_frames.len() > 1;
    if multi && session_log_enabled(LOG_VERBOSE) {
        println!(
            "tx {label}: fragmenting plaintext bytes={} into {} AA fragments first={} continuation={}",
            plaintext.len(),
            plain_frames.len(),
            first,
            continuation
        );
        log_line(
            dump,
            &format!(
                "tx {label}: fragmenting plaintext bytes={} into {} AA fragments first={} continuation={}",
                plaintext.len(),
                plain_frames.len(),
                first,
                continuation
            ),
        )?;
    }

    let mut written = 0usize;
    for (idx, plain_frame) in plain_frames.iter().enumerate() {
        let encrypted = tls.encrypt_plaintext(&plain_frame.payload)?;
        let tx_frame = AapFrame {
            channel: plain_frame.channel,
            flags: plain_frame.flags | FLAG_ENCRYPTED,
            final_length: plain_frame.final_length,
            payload: encrypted,
        };
        write_frame(stream, &tx_frame)?;
        written += 1;

        if multi && session_log_enabled(LOG_VERBOSE) {
            println!(
                "tx {label} fragment {}/{}: type={} plain_bytes={} tls_bytes={} final_len={:?}",
                idx + 1,
                plain_frames.len(),
                frame_type_name(tx_frame.flags),
                plain_frame.payload.len(),
                tx_frame.payload.len(),
                tx_frame.final_length
            );
            log_line(
                dump,
                &format!(
                    "tx {label} fragment {}/{}: type={} plain_bytes={} tls_bytes={} final_len={:?} {}",
                    idx + 1,
                    plain_frames.len(),
                    frame_type_name(tx_frame.flags),
                    plain_frame.payload.len(),
                    tx_frame.payload.len(),
                    tx_frame.final_length,
                    tls_record_hint(&tx_frame.payload)
                ),
            )?;
        }
    }

    stream.flush()?;
    Ok(written)
}

fn log_reassembly_wait_or_drop(
    dump: &mut Option<Box<dyn Write>>,
    channel: u8,
    result: &fragment::ReassemblyResult,
) -> io::Result<()> {
    if !session_log_enabled(LOG_VERBOSE) {
        return Ok(());
    }
    match result {
        fragment::ReassemblyResult::Waiting { fragments, expected_len, actual_len } => {
            let line = format!(
                "decrypted AA fragment_wait: channel=0x{channel:02x} fragments={} expected_len={:?} actual_len={}",
                fragments, expected_len, actual_len
            );
            println!("{line}");
            log_line(dump, &line)
        }
        fragment::ReassemblyResult::Dropped(reason) => {
            let line = format!("decrypted AA fragment_drop: {reason}");
            println!("{line}");
            log_line(dump, &line)
        }
        fragment::ReassemblyResult::Complete { .. } => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// AAP proto smoke probe
// ---------------------------------------------------------------------------

pub fn proto_smoke() -> io::Result<()> {
    println!("arda aap proto smoke");
    println!("{}", proto::smoke_summary());
    println!("generated module: OUT_DIR/protos/mod.rs");
    println!("next: build ServiceDiscoveryRequest/Response and ChannelOpen messages via proto structs");
    Ok(())
}

pub fn fragment_smoke() -> io::Result<()> {
    println!("arda aap fragment smoke");
    println!("{}", fragment::smoke_summary());
    println!("policy: split plaintext/application payload before TLS; encrypt each fragment separately");
    println!("first fragment carries final_length = total plaintext payload length");
    Ok(())
}

fn build_ssl_frame(tls_bytes: &[u8]) -> AapFrame {
    let mut payload = Vec::with_capacity(2 + tls_bytes.len());
    payload.extend_from_slice(&0x0003u16.to_be_bytes());
    payload.extend_from_slice(tls_bytes);
    AapFrame {
        channel: 0,
        flags: FRAME_TYPE_FIRST | FRAME_TYPE_LAST,
        final_length: None,
        payload,
    }
}

fn is_platform_input_bridge_mode(mode: &str) -> bool {
    matches!(
        mode.to_ascii_lowercase().as_str(),
        "bridge" | "platform" | "platform-bridge" | "platform_bridge" | "system-app" | "system_app"
    )
}

fn is_app_process_input_inject_mode(mode: &str) -> bool {
    matches!(
        mode.to_ascii_lowercase().as_str(),
        "app" | "framework" | "java" | "app-process" | "app_process" | "inputd"
    )
}

fn is_shell_touch_inject_mode(mode: &str) -> bool {
    matches!(
        mode.to_ascii_lowercase().as_str(),
        "shell" | "motionevent" | "shell-motionevent" | "input"
    )
}
