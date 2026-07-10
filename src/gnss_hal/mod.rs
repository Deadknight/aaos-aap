use std::io::{self, Write};
use std::mem;
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use crate::aap::proto::DecodedLocationFix;

pub const GNSS_HAL_SOCKET_NAME: &str = "arda_gnss_hal";
const GNSS_HAL_WIRE_VERSION: &str = "ARDA_GNSS_HAL_V1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GnssHalPublishOutcome {
    Published { connected: bool },
    Throttled,
}

#[derive(Debug)]
pub struct PlatformGnssHalBridge {
    min_interval: Duration,
    last_publish: Option<Instant>,
    stream: Option<UnixStream>,
}

impl PlatformGnssHalBridge {
    pub fn new(min_interval_ms: u64) -> Self {
        Self {
            min_interval: Duration::from_millis(min_interval_ms),
            last_publish: None,
            stream: None,
        }
    }

    pub fn publish_fix(
        &mut self,
        fix: &DecodedLocationFix,
    ) -> io::Result<GnssHalPublishOutcome> {
        validate_fix(fix)?;

        let now = Instant::now();
        if self
            .last_publish
            .is_some_and(|last| now.duration_since(last) < self.min_interval)
        {
            return Ok(GnssHalPublishOutcome::Throttled);
        }

        let line = encode_fix(fix);
        let mut connected = false;

        for attempt in 0..2 {
            if self.stream.is_none() {
                self.stream = Some(connect_abstract(GNSS_HAL_SOCKET_NAME)?);
                connected = true;
            }

            let result = self
                .stream
                .as_mut()
                .expect("GNSS HAL stream must exist")
                .write_all(line.as_bytes());

            match result {
                Ok(()) => {
                    self.last_publish = Some(now);
                    return Ok(GnssHalPublishOutcome::Published { connected });
                }
                Err(_) if attempt == 0 => {
                    self.stream = None;
                    connected = false;
                }
                Err(err) => {
                    self.stream = None;
                    return Err(err);
                }
            }
        }

        unreachable!("GNSS HAL bridge retry loop must return")
    }
}

fn validate_fix(fix: &DecodedLocationFix) -> io::Result<()> {
    if !fix.latitude_deg.is_finite() || !(-90.0..=90.0).contains(&fix.latitude_deg) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid latitude",
        ));
    }

    if !fix.longitude_deg.is_finite() || !(-180.0..=180.0).contains(&fix.longitude_deg) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid longitude",
        ));
    }

    validate_optional_non_negative("accuracy", fix.accuracy_m)?;
    validate_optional_finite("altitude", fix.altitude_m)?;
    validate_optional_non_negative("speed", fix.speed_mps)?;

    if let Some(bearing) = fix.bearing_deg {
        if !bearing.is_finite() || !(0.0..360.0).contains(&bearing) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid bearing",
            ));
        }
    }

    Ok(())
}

fn validate_optional_finite(name: &str, value: Option<f64>) -> io::Result<()> {
    if value.is_some_and(|value| !value.is_finite()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid {name}"),
        ));
    }

    Ok(())
}

fn validate_optional_non_negative(name: &str, value: Option<f64>) -> io::Result<()> {
    validate_optional_finite(name, value)?;

    if value.is_some_and(|value| value < 0.0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid {name}"),
        ));
    }

    Ok(())
}

fn encode_fix(fix: &DecodedLocationFix) -> String {
    format!(
        "{}\tFIX\t{}\t{:.10}\t{:.10}\t{}\t{}\t{}\t{}\n",
        GNSS_HAL_WIRE_VERSION,
        option_u64(fix.timestamp_raw),
        fix.latitude_deg,
        fix.longitude_deg,
        option_f64(fix.accuracy_m),
        option_f64(fix.altitude_m),
        option_f64(fix.speed_mps),
        option_f64(fix.bearing_deg),
    )
}

fn option_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn option_f64(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.10}"))
        .unwrap_or_else(|| "-".to_string())
}

fn connect_abstract(name: &str) -> io::Result<UnixStream> {
    let name_bytes = name.as_bytes();
    let max_name_len = unsafe { mem::zeroed::<libc::sockaddr_un>().sun_path.len() - 1 };

    if name_bytes.is_empty() || name_bytes.len() > max_name_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid abstract socket name length",
        ));
    }

    let fd = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
            0,
        )
    };

    if fd < 0 {
        return Err(io::Error::last_os_error());
    }

    let mut address: libc::sockaddr_un = unsafe { mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;

    for (index, byte) in name_bytes.iter().copied().enumerate() {
        address.sun_path[index + 1] = byte as libc::c_char;
    }

    let base = &address as *const libc::sockaddr_un as usize;
    let path = &address.sun_path as *const _ as usize;
    let address_len = (path - base) + 1 + name_bytes.len();

    let result = unsafe {
        libc::connect(
            fd,
            &address as *const libc::sockaddr_un as *const libc::sockaddr,
            address_len as libc::socklen_t,
        )
    };

    if result != 0 {
        let err = io::Error::last_os_error();
        close_fd(fd);
        return Err(err);
    }

    Ok(unsafe { UnixStream::from_raw_fd(fd) })
}

fn close_fd(fd: RawFd) {
    unsafe {
        libc::close(fd);
    }
}

