//! Linux /dev/uinput based input injection.
//!
//! This is intentionally small and dependency-free because arda-senderd is
//! cross-compiled with a minimal Android NDK Docker image. The first target is a
//! virtual direct touchscreen that can emit tap and swipe gestures.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::mem;
use std::os::raw::{c_char, c_int, c_long, c_ulong};
use std::os::unix::io::AsRawFd;
use std::thread;
use std::time::Duration;

const UINPUT_PATH: &str = "/dev/uinput";

const EV_SYN: c_int = 0x00;
const EV_KEY: c_int = 0x01;
const EV_ABS: c_int = 0x03;

const SYN_REPORT: u16 = 0;

const BTN_TOUCH: c_int = 0x14a;

const ABS_X: c_int = 0x00;
const ABS_Y: c_int = 0x01;
const ABS_PRESSURE: c_int = 0x18;
const ABS_MT_SLOT: c_int = 0x2f;
const ABS_MT_TOUCH_MAJOR: c_int = 0x30;
const ABS_MT_POSITION_X: c_int = 0x35;
const ABS_MT_POSITION_Y: c_int = 0x36;
const ABS_MT_TRACKING_ID: c_int = 0x39;
const ABS_MT_PRESSURE: c_int = 0x3a;

const INPUT_PROP_DIRECT: c_int = 0x01;

const BUS_USB: u16 = 0x03;
const ABS_CNT: usize = 0x40;
const UINPUT_MAX_NAME_SIZE: usize = 80;

#[repr(C)]
#[derive(Clone, Copy)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[repr(C)]
struct UinputUserDev {
    name: [c_char; UINPUT_MAX_NAME_SIZE],
    id: InputId,
    ff_effects_max: u32,
    absmax: [i32; ABS_CNT],
    absmin: [i32; ABS_CNT],
    absfuzz: [i32; ABS_CNT],
    absflat: [i32; ABS_CNT],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TimeVal {
    tv_sec: c_long,
    tv_usec: c_long,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct InputEvent {
    time: TimeVal,
    type_: u16,
    code: u16,
    value: i32,
}

unsafe extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}

const IOC_NRBITS: c_ulong = 8;
const IOC_TYPEBITS: c_ulong = 8;
const IOC_SIZEBITS: c_ulong = 14;

const IOC_NRSHIFT: c_ulong = 0;
const IOC_TYPESHIFT: c_ulong = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: c_ulong = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: c_ulong = IOC_SIZESHIFT + IOC_SIZEBITS;

const IOC_NONE: c_ulong = 0;
const IOC_WRITE: c_ulong = 1;

const fn ioc(dir: c_ulong, type_: c_ulong, nr: c_ulong, size: c_ulong) -> c_ulong {
    (dir << IOC_DIRSHIFT) | (type_ << IOC_TYPESHIFT) | (nr << IOC_NRSHIFT) | (size << IOC_SIZESHIFT)
}

const fn io(type_: c_ulong, nr: c_ulong) -> c_ulong {
    ioc(IOC_NONE, type_, nr, 0)
}

const fn iow_int(type_: c_ulong, nr: c_ulong) -> c_ulong {
    ioc(IOC_WRITE, type_, nr, mem::size_of::<c_int>() as c_ulong)
}

const UI_DEV_CREATE: c_ulong = io(b'U' as c_ulong, 1);
const UI_DEV_DESTROY: c_ulong = io(b'U' as c_ulong, 2);
const UI_SET_EVBIT: c_ulong = iow_int(b'U' as c_ulong, 100);
const UI_SET_KEYBIT: c_ulong = iow_int(b'U' as c_ulong, 101);
const UI_SET_ABSBIT: c_ulong = iow_int(b'U' as c_ulong, 103);
const UI_SET_PROPBIT: c_ulong = iow_int(b'U' as c_ulong, 110);

#[derive(Debug, Clone, Copy)]
pub enum InputAction {
    Tap { x: i32, y: i32, duration_ms: u64 },
    Swipe {
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        duration_ms: u64,
        steps: u32,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct InputTestConfig {
    pub width: i32,
    pub height: i32,
    pub action: InputAction,
}

pub fn run_input_test(config: InputTestConfig) -> io::Result<String> {
    let mut report = String::new();
    report.push_str("arda input test\n");
    report.push_str(&format!("uinput={}\n", UINPUT_PATH));
    report.push_str(&format!("size={}x{}\n", config.width, config.height));
    report.push_str(&format!("action={:?}\n\n", config.action));

    let mut dev = VirtualTouch::create(config.width, config.height, &mut report)?;

    match config.action {
        InputAction::Tap { x, y, duration_ms } => {
            report.push_str(&format!("tap down x={} y={}\n", x, y));
            dev.touch_down(x, y, 1)?;
            thread::sleep(Duration::from_millis(duration_ms));
            report.push_str("tap up\n");
            dev.touch_up()?;
        }
        InputAction::Swipe {
            x1,
            y1,
            x2,
            y2,
            duration_ms,
            steps,
        } => {
            let steps = steps.max(1);
            report.push_str(&format!(
                "swipe from {},{} to {},{} duration_ms={} steps={}\n",
                x1, y1, x2, y2, duration_ms, steps
            ));
            dev.touch_down(x1, y1, 1)?;
            let per_step_ms = (duration_ms / steps as u64).max(1);
            for step in 1..=steps {
                let x = lerp_i32(x1, x2, step, steps);
                let y = lerp_i32(y1, y2, step, steps);
                thread::sleep(Duration::from_millis(per_step_ms));
                dev.touch_move(x, y)?;
            }
            dev.touch_up()?;
        }
    }

    // Give Android InputReader a moment to consume final SYN_REPORT before the
    // short-lived test device is destroyed.
    thread::sleep(Duration::from_millis(120));
    report.push_str("OK\n");
    Ok(report)
}

fn lerp_i32(a: i32, b: i32, step: u32, steps: u32) -> i32 {
    let a64 = i64::from(a);
    let b64 = i64::from(b);
    let step64 = i64::from(step);
    let steps64 = i64::from(steps.max(1));
    (a64 + ((b64 - a64) * step64) / steps64) as i32
}

struct VirtualTouch {
    file: File,
    tracking_id: i32,
}

impl VirtualTouch {
    fn create(width: i32, height: i32, report: &mut String) -> io::Result<Self> {
        report.push_str("open /dev/uinput\n");
        let mut file = OpenOptions::new().read(true).write(true).open(UINPUT_PATH)?;
        let fd = file.as_raw_fd();

        report.push_str("set capability bits\n");
        ioctl_int(fd, UI_SET_EVBIT, EV_SYN)?;
        ioctl_int(fd, UI_SET_EVBIT, EV_KEY)?;
        ioctl_int(fd, UI_SET_EVBIT, EV_ABS)?;
        ioctl_int(fd, UI_SET_KEYBIT, BTN_TOUCH)?;
        ioctl_int(fd, UI_SET_PROPBIT, INPUT_PROP_DIRECT)?;

        for abs in [
            ABS_X,
            ABS_Y,
            ABS_PRESSURE,
            ABS_MT_SLOT,
            ABS_MT_TOUCH_MAJOR,
            ABS_MT_POSITION_X,
            ABS_MT_POSITION_Y,
            ABS_MT_TRACKING_ID,
            ABS_MT_PRESSURE,
        ] {
            ioctl_int(fd, UI_SET_ABSBIT, abs)?;
        }

        let mut user_dev = zeroed_user_dev();
        set_name(&mut user_dev.name, "arda-virtual-touch");
        user_dev.id = InputId {
            bustype: BUS_USB,
            vendor: 0x1209,
            product: 0xAADA,
            version: 1,
        };

        set_abs_range(&mut user_dev, ABS_X, 0, width.saturating_sub(1));
        set_abs_range(&mut user_dev, ABS_Y, 0, height.saturating_sub(1));
        set_abs_range(&mut user_dev, ABS_PRESSURE, 0, 255);
        set_abs_range(&mut user_dev, ABS_MT_SLOT, 0, 0);
        set_abs_range(&mut user_dev, ABS_MT_TOUCH_MAJOR, 0, 255);
        set_abs_range(&mut user_dev, ABS_MT_POSITION_X, 0, width.saturating_sub(1));
        set_abs_range(&mut user_dev, ABS_MT_POSITION_Y, 0, height.saturating_sub(1));
        set_abs_range(&mut user_dev, ABS_MT_TRACKING_ID, 0, 65535);
        set_abs_range(&mut user_dev, ABS_MT_PRESSURE, 0, 255);

        report.push_str("write uinput_user_dev\n");
        write_struct(&mut file, &user_dev)?;

        report.push_str("UI_DEV_CREATE\n");
        ioctl_noarg(fd, UI_DEV_CREATE)?;
        thread::sleep(Duration::from_millis(350));
        report.push_str("device created\n");

        Ok(Self { file, tracking_id: 1 })
    }

    fn touch_down(&mut self, x: i32, y: i32, tracking_id: i32) -> io::Result<()> {
        self.tracking_id = tracking_id;
        self.emit_abs(ABS_MT_SLOT, 0)?;
        self.emit_abs(ABS_MT_TRACKING_ID, tracking_id)?;
        self.emit_abs(ABS_MT_TOUCH_MAJOR, 8)?;
        self.emit_abs(ABS_MT_PRESSURE, 80)?;
        self.emit_abs(ABS_MT_POSITION_X, x)?;
        self.emit_abs(ABS_MT_POSITION_Y, y)?;
        self.emit_abs(ABS_X, x)?;
        self.emit_abs(ABS_Y, y)?;
        self.emit_abs(ABS_PRESSURE, 80)?;
        self.emit_key(BTN_TOUCH, 1)?;
        self.syn()
    }

    fn touch_move(&mut self, x: i32, y: i32) -> io::Result<()> {
        self.emit_abs(ABS_MT_SLOT, 0)?;
        self.emit_abs(ABS_MT_POSITION_X, x)?;
        self.emit_abs(ABS_MT_POSITION_Y, y)?;
        self.emit_abs(ABS_X, x)?;
        self.emit_abs(ABS_Y, y)?;
        self.syn()
    }

    fn touch_up(&mut self) -> io::Result<()> {
        self.emit_abs(ABS_MT_SLOT, 0)?;
        self.emit_abs(ABS_MT_TRACKING_ID, -1)?;
        self.emit_abs(ABS_MT_PRESSURE, 0)?;
        self.emit_abs(ABS_PRESSURE, 0)?;
        self.emit_key(BTN_TOUCH, 0)?;
        self.syn()
    }

    fn emit_abs(&mut self, code: c_int, value: i32) -> io::Result<()> {
        self.emit(EV_ABS as u16, code as u16, value)
    }

    fn emit_key(&mut self, code: c_int, value: i32) -> io::Result<()> {
        self.emit(EV_KEY as u16, code as u16, value)
    }

    fn syn(&mut self) -> io::Result<()> {
        self.emit(EV_SYN as u16, SYN_REPORT, 0)
    }

    fn emit(&mut self, type_: u16, code: u16, value: i32) -> io::Result<()> {
        let ev = InputEvent {
            time: TimeVal { tv_sec: 0, tv_usec: 0 },
            type_,
            code,
            value,
        };
        write_struct(&mut self.file, &ev)
    }
}

impl Drop for VirtualTouch {
    fn drop(&mut self) {
        let _ = self.touch_up();
        let _ = self.file.flush();
        thread::sleep(Duration::from_millis(50));
        unsafe {
            let _ = ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY);
        }
    }
}

fn zeroed_user_dev() -> UinputUserDev {
    UinputUserDev {
        name: [0; UINPUT_MAX_NAME_SIZE],
        id: InputId { bustype: 0, vendor: 0, product: 0, version: 0 },
        ff_effects_max: 0,
        absmax: [0; ABS_CNT],
        absmin: [0; ABS_CNT],
        absfuzz: [0; ABS_CNT],
        absflat: [0; ABS_CNT],
    }
}

fn set_name(target: &mut [c_char; UINPUT_MAX_NAME_SIZE], name: &str) {
    for (dst, src) in target.iter_mut().zip(name.bytes()) {
        *dst = src as c_char;
    }
}

fn set_abs_range(dev: &mut UinputUserDev, code: c_int, min: i32, max: i32) {
    let idx = code as usize;
    if idx < ABS_CNT {
        dev.absmin[idx] = min;
        dev.absmax[idx] = max;
    }
}

fn ioctl_noarg(fd: c_int, request: c_ulong) -> io::Result<()> {
    let rc = unsafe { ioctl(fd, request) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn ioctl_int(fd: c_int, request: c_ulong, value: c_int) -> io::Result<()> {
    let rc = unsafe { ioctl(fd, request, value) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn write_struct<T>(file: &mut File, value: &T) -> io::Result<()> {
    let ptr = value as *const T as *const u8;
    let len = mem::size_of::<T>();
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    file.write_all(bytes)
}
