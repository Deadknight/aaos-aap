//! Minimal AAP/TCP transport probes.
//!
//! This is not Android Auto protocol yet. It is the first network transport
//! scaffold: bind a TCP port on AAOS, accept a client, and capture whatever
//! bytes arrive so the next step can implement the actual message framing and
//! service negotiation.

use std::env;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::h264::annexb::{AnnexBStreamParser, NalType};

pub mod proto;
mod fragment;
mod tls;

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

    fn read_frame(&mut self, stream: &mut TcpStream) -> io::Result<Option<AapFrame>> {
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
    peer: SocketAddr,
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

fn write_frame(stream: &mut TcpStream, frame: &AapFrame) -> io::Result<()> {
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
        let frame = match reader.read_frame(&mut stream) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                println!("client disconnected after frames={frames} ssl_frames={ssl_frames} bytes={bytes}");
                log_line(&mut dump, &format!("client disconnected after frames={frames} ssl_frames={ssl_frames} bytes={bytes}"))?;
                break;
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
    pub addr: String,
    pub dump_path: Option<PathBuf>,
    pub timeout_ms: u64,
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
    pub video_width: i32,
    pub video_height: i32,
    pub video_bit_rate: i32,
    pub video_fps: i32,
}

impl Default for TlsHandshakeProbeConfig {
    fn default() -> Self {
        Self {
            addr: "0.0.0.0:5277".to_string(),
            dump_path: Some(PathBuf::from("/data/local/tmp/arda-aap-tls-handshake.log")),
            timeout_ms: 60_000,
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
            video_width: 800,
            video_height: 480,
            video_bit_rate: 2_500_000,
            video_fps: 30,
        }
    }
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
        let frame = match reader.read_frame(&mut stream) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                println!("client disconnected after frames={frames} ssl_frames={ssl_frames} bytes={bytes}");
                log_line(&mut dump, &format!("client disconnected after frames={frames} ssl_frames={ssl_frames} bytes={bytes}"))?;
                break;
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
        let frame = match reader.read_frame(&mut stream) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                println!("client disconnected after frames={frames} encrypted_frames={encrypted_frames} decrypted_frames={decrypted_frames} bytes={bytes}");
                log_line(&mut dump, &format!("client disconnected after frames={frames} encrypted_frames={encrypted_frames} decrypted_frames={decrypted_frames} bytes={bytes}"))?;
                break;
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
        println!("rx frame:");
        describe_frame(frames, bytes, peer, &frame, &mut io::stdout())?;
        if let Some(dump) = dump.as_mut() {
            writeln!(dump, "rx frame:")?;
            describe_frame(frames, bytes, peer, &frame, dump)?;
            dump.flush()?;
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

fn log_decrypted_payload(
    dump: &mut Option<Box<dyn Write>>,
    peer: SocketAddr,
    channel: u8,
    flags: u8,
    index: usize,
    plaintext: &[u8],
) -> io::Result<()> {
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
pub fn tls_service_discovery_probe(config: TlsHandshakeProbeConfig) -> io::Result<()> {
    println!("arda aap service discovery probe");
    println!("addr={}", config.addr);
    println!("timeout_ms={}", config.timeout_ms);
    println!("max_bytes={}", config.max_bytes);
    println!("max_frames={}", config.max_frames);
    println!("response={}.{} status=0x{:04x}", config.response_major, config.response_minor, config.response_status);
    println!("keys={}", config.keys_path.display());
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

    log_line(&mut dump, "arda aap service discovery probe")?;
    log_line(&mut dump, &format!("local={}", listener.local_addr()?))?;
    log_line(&mut dump, &format!("peer={peer}"))?;
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
                "video_native=true size={}x{} bit_rate={} fps={} frame_limit={} auto_config={}",
                config.video_width,
                config.video_height,
                config.video_bit_rate,
                config.video_fps,
                config.video_frame_limit,
                config.video_auto_config
            ),
        )?;
    } else {
        log_line(&mut dump, "video_native=false")?;
    }
    log_line(&mut dump, "role=phone-like TCP server + TLS server; sends encrypted ServiceDiscoveryRequest after AUTH_COMPLETE")?;
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
    let mut service_request_sent = false;
    let mut channel_open_sent = false;
    let mut selected_video_service_id: Option<i32> = None;
    let mut video_channel_opened = false;
    let mut video_setup_sent = false;
    let mut video_start_sent = false;
    let mut video_max_unacked: u32 = 1;
    let video_session_id: i32 = 1;
    let mut video_sender: Option<VideoSender> = if config.video_native {
        Some(VideoSender::Live(VideoLiveSender::new(VideoLiveConfig {
            width: config.video_width,
            height: config.video_height,
            bit_rate: config.video_bit_rate,
            fps: config.video_fps,
            frame_limit: config.video_frame_limit,
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

    while frames < config.max_frames {
        let frame = match reader.read_frame(&mut stream) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                println!("client disconnected after frames={frames} service_request_sent={service_request_sent} bytes={bytes}");
                log_line(&mut dump, &format!("client disconnected after frames={frames} service_request_sent={service_request_sent} bytes={bytes}"))?;
                break;
            }
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.kind() == io::ErrorKind::TimedOut =>
            {
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
                        if sent == 0 {
                            // Avoid a busy loop when the capture reader has no complete NAL yet
                            // or flow control is waiting for the next ACK.
                            thread::sleep(Duration::from_millis(5));
                        }
                    } else {
                        thread::sleep(Duration::from_millis(5));
                    }
                    continue;
                }
                println!("read timeout after frames={frames} service_request_sent={service_request_sent} bytes={bytes}");
                log_line(&mut dump, &format!("read timeout after frames={frames} service_request_sent={service_request_sent} bytes={bytes}"))?;
                break;
            }
            Err(err) => return Err(err),
        };

        frames += 1;
        bytes += HEADER_LENGTH + frame.payload.len() + if frame.final_length.is_some() { 4 } else { 0 };
        println!("rx frame:");
        describe_frame(frames, bytes, peer, &frame, &mut io::stdout())?;
        if let Some(dump) = dump.as_mut() {
            writeln!(dump, "rx frame:")?;
            describe_frame(frames, bytes, peer, &frame, dump)?;
            dump.flush()?;
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
            println!("rx encrypted AA frame #{encrypted_frames}: tls_bytes={} {}", frame.payload.len(), tls_record_hint(&frame.payload));
            log_line(&mut dump, &format!("rx encrypted AA frame #{encrypted_frames}: tls_bytes={} {}", frame.payload.len(), tls_record_hint(&frame.payload)))?;

            let step = tls.feed_tls(&frame.payload)?;
            log_tls_step(&mut dump, &step)?;
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
                println!("tx encrypted TLS maintenance frame #{tx_ssl_frames}: tls_bytes={}", tx_frame.payload.len());
                log_line(&mut dump, &format!("tx encrypted TLS maintenance frame #{tx_ssl_frames}: tls_bytes={} {}", tx_frame.payload.len(), tls_record_hint(&tx_frame.payload)))?;
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
                    if fragments > 1 {
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
            log_decrypted_payload(&mut dump, peer, frame.channel, frame.flags, decrypted_frames, &plaintext)?;

            if !service_request_sent && is_control_message(&plaintext, 0x0004) {
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
                if let Some(summary) = proto::parse_service_discovery_response_control_payload(&plaintext)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
                {
                    println!("SERVICE_DISCOVERY_RESPONSE observed; parsed {} detail line(s)", summary.details.len());
                    log_line(&mut dump, &format!("SERVICE_DISCOVERY_RESPONSE observed; parsed {} detail line(s)", summary.details.len()))?;
                    for line in &summary.details {
                        println!("{line}");
                        log_line(&mut dump, line)?;
                    }

                    if let Some(video) = summary.video_media_sink {
                        selected_video_service_id = Some(video.service_id);
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

                        if config.video_auto_config {
                            if let Some(vc) = video.selected_video_config.as_ref() {
                                let line = format!(
                                    "VIDEO_AUTO_CONFIG selected from SDR: service_id={} config_index={} {}x{} fps={} density={} real_density={} margins={}x{} codec={} resolution={}",
                                    video.service_id,
                                    vc.config_index,
                                    vc.width,
                                    vc.height,
                                    vc.fps,
                                    vc.density,
                                    vc.real_density,
                                    vc.width_margin,
                                    vc.height_margin,
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
                }
            }

            if is_control_message(&plaintext, 0x0008) {
                if selected_video_service_id.map(|id| id as u8) == Some(frame.channel) {
                    video_channel_opened = true;
                }
                println!("CHANNEL_OPEN_RESPONSE observed for selected_video_service_id={selected_video_service_id:?} opened={video_channel_opened}");
                log_line(&mut dump, &format!("CHANNEL_OPEN_RESPONSE observed for selected_video_service_id={selected_video_service_id:?} opened={video_channel_opened}"))?;
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
                    println!("tx encrypted MEDIA_MESSAGE_START frame(s): count={tx_count} channel=0x{:02x}", frame.channel);
                    log_line(&mut dump, &format!("tx encrypted MEDIA_MESSAGE_START frame(s): count={tx_count} channel=0x{:02x}", frame.channel))?;

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
                    println!("MEDIA_MESSAGE_ACK observed; {summary}");
                    log_line(&mut dump, &format!("MEDIA_MESSAGE_ACK observed; {summary}"))?;
                    if session_id == video_session_id {
                        if let Some(sender) = video_sender.as_mut() {
                            sender.on_ack(ack_value);
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

                let tx_count = write_encrypted_plaintext_fragmented(
                    &mut stream,
                    &mut tls,
                    0,
                    &control_payload,
                    &mut dump,
                    "PING_RESPONSE",
                )?;
                tx_ssl_frames += tx_count;
                println!("tx encrypted PING_RESPONSE frame(s): count={tx_count}");
                log_line(&mut dump, &format!("tx encrypted PING_RESPONSE frame(s): count={tx_count}"))?;
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

    let elapsed = started.elapsed();
    println!("summary: frames={frames} rx_ssl_frames={rx_ssl_frames} tx_ssl_frames={tx_ssl_frames} encrypted_frames={encrypted_frames} decrypted_frames={decrypted_frames} service_request_sent={service_request_sent} channel_open_sent={channel_open_sent} video_channel_opened={video_channel_opened} video_setup_sent={video_setup_sent} video_start_sent={video_start_sent} selected_video_service_id={selected_video_service_id:?} decrypted_bytes={decrypted_bytes} bytes={bytes} elapsed_ms={}", elapsed.as_millis());
    log_line(&mut dump, &format!("summary: frames={frames} rx_ssl_frames={rx_ssl_frames} tx_ssl_frames={tx_ssl_frames} encrypted_frames={encrypted_frames} decrypted_frames={decrypted_frames} service_request_sent={service_request_sent} channel_open_sent={channel_open_sent} video_channel_opened={video_channel_opened} video_setup_sent={video_setup_sent} video_start_sent={video_start_sent} selected_video_service_id={selected_video_service_id:?} decrypted_bytes={decrypted_bytes} bytes={bytes} elapsed_ms={}", elapsed.as_millis()))?;
    Ok(())
}



#[derive(Debug)]
enum VideoSender {
    File(VideoFileSender),
    Live(VideoLiveSender),
}

impl VideoSender {
    fn on_ack(&mut self, ack_value: u32) {
        match self {
            Self::File(sender) => sender.on_ack(ack_value),
            Self::Live(sender) => sender.on_ack(ack_value),
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

    fn send_available(
        &mut self,
        stream: &mut TcpStream,
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

    fn send_available(
        &mut self,
        stream: &mut TcpStream,
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
    /// 0 means unlimited; otherwise stop sending after this many video slice NALs.
    frame_limit: usize,
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
        }
    }

    fn on_ack(&mut self, ack_value: u32) {
        // In this DHU trace Ack.ack is reported as 1 for each acknowledged
        // media buffer, not as a monotonically increasing cumulative counter.
        // Treat every ACK as releasing at least one in-flight DATA frame.
        let released = ack_value.max(1);
        self.highest_ack = self.highest_ack.max(ack_value);
        self.unacked = self.unacked.saturating_sub(released);
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
        let line = format!(
            "VIDEO_NATIVE configured from SDR: size={}x{} fps={} bit_rate={} density={} real_density={} margins={}x{} config_index={} codec={} resolution={}",
            self.config.width,
            self.config.height,
            self.config.fps,
            self.config.bit_rate,
            selection.density,
            selection.real_density,
            selection.width_margin,
            selection.height_margin,
            selection.config_index,
            selection.codec,
            selection.resolution
        );
        println!("{line}");
        log_line(dump, &line)
    }

    fn ensure_started(&mut self, dump: &mut Option<Box<dyn Write>>) -> io::Result<()> {
        if self.child.is_some() || self.ended {
            return Ok(());
        }

        let exe = env::current_exe()?;
        let size = format!("{}x{}", self.config.width, self.config.height);
        let bit_rate = self.config.bit_rate.to_string();
        let fps = self.config.fps.to_string();
        // Child lifetime is controlled by this session. 0 means capture until killed.
        let seconds = String::from("0");

        let mut child = Command::new(exe)
            .arg("native-capture")
            .arg("--size")
            .arg(&size)
            .arg("--bit-rate")
            .arg(&bit_rate)
            .arg("--fps")
            .arg(&fps)
            .arg("--seconds")
            .arg(&seconds)
            .arg("--out")
            .arg("-")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| io::Error::new(err.kind(), format!("failed to spawn native-capture child: {err}")))?;

        let Some(mut stdout) = child.stdout.take() else {
            let _ = child.kill();
            return Err(io::Error::new(io::ErrorKind::Other, "native-capture child stdout missing"));
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
            "VIDEO_NATIVE started child: size={} bit_rate={} fps={} frame_limit={}",
            size,
            bit_rate,
            fps,
            self.config.frame_limit
        );
        println!("{line}");
        log_line(dump, &line)?;
        log_line(dump, "VIDEO_NATIVE child_log=/data/local/tmp/arda-display-capture-test.log")?;

        self.child = Some(child);
        self.rx = Some(rx);
        Ok(())
    }

    fn send_available(
        &mut self,
        stream: &mut TcpStream,
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
                    println!("{line}");
                    log_line(dump, &line)?;
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
                            self.codec_config.clear();
                        }
                        self.codec_config_sent = true;
                    }

                    let pts_us = (self.next_pts_index as u64) * 1_000_000u64 / (self.config.fps as u64).max(1);
                    self.next_pts_index += 1;
                    let payload = proto::build_media_data_payload(pts_us, &bytes);
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
                    tx_frames += write_encrypted_plaintext_fragmented(
                        stream,
                        tls,
                        channel,
                        &payload,
                        dump,
                        "MEDIA_MESSAGE_DATA",
                    )?;
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

        if self.config.frame_limit > 0 && self.frames_sent >= self.config.frame_limit {
            let line = format!("VIDEO_NATIVE frame limit reached: frames_sent={}", self.frames_sent);
            println!("{line}");
            log_line(dump, &line)?;
            self.stop_child();
        }

        Ok(tx_frames)
    }

    fn stop_child(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
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


fn is_control_message(payload: &[u8], id: u16) -> bool {
    payload.len() >= 2 && u16::from_be_bytes([payload[0], payload[1]]) == id
}


fn write_encrypted_plaintext_fragmented(
    stream: &mut TcpStream,
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

fn write_encrypted_plaintext_fragmented_with_flags(
    stream: &mut TcpStream,
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
    if multi {
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

        if multi {
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
