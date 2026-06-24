//! Minimal AAP/TCP transport probes.
//!
//! This is not Android Auto protocol yet. It is the first network transport
//! scaffold: bind a TCP port on AAOS, accept a client, and capture whatever
//! bytes arrive so the next step can implement the actual message framing and
//! service negotiation.

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub mod proto;
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
    let hint = control_hint(&pseudo).map(|h| format!(" {h}")).unwrap_or_default();
    println!("decrypted AA payload #{index}: channel=0x{channel:02x} bytes={}{}", plaintext.len(), hint);
    if let Some(dump) = dump.as_mut() {
        writeln!(dump, "decrypted AA payload #{index}: channel=0x{channel:02x} bytes={}{}", plaintext.len(), hint)?;
        describe_frame(index, 0, peer, &pseudo, dump)?;
        dump.flush()?;
    }
    Ok(())
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
