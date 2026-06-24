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
    let name = match id {
        0x0001 => "MESSAGE_VERSION_REQUEST",
        0x0002 => "MESSAGE_VERSION_RESPONSE",
        0x0003 => "MESSAGE_ENCAPSULATED_SSL",
        0x0005 => "MESSAGE_SERVICE_DISCOVERY_REQUEST",
        0x0006 => "MESSAGE_SERVICE_DISCOVERY_RESPONSE",
        0x0007 => "MESSAGE_CHANNEL_OPEN_REQUEST",
        0x0008 => "MESSAGE_CHANNEL_OPEN_RESPONSE",
        0x001a => "MESSAGE_SERVICE_DISCOVERY_UPDATE",
        _ => "UNKNOWN_CONTROL",
    };

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
        return Some(format!("{name} tls_bytes={}", frame.payload.len().saturating_sub(2)));
    }
    Some(format!("{name} id=0x{id:04x}"))
}
