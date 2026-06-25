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
use std::process::Command;
use std::thread;
use std::time::Duration;

const UINPUT_PATH: &str = "/dev/uinput";

const EV_SYN: c_int = 0x00;
const EV_KEY: c_int = 0x01;
const EV_ABS: c_int = 0x03;

const SYN_REPORT: u16 = 0;

const BTN_TOOL_FINGER: c_int = 0x145;
const BTN_TOUCH: c_int = 0x14a;

const ABS_X: c_int = 0x00;
const ABS_Y: c_int = 0x01;
const ABS_PRESSURE: c_int = 0x18;
const ABS_MT_SLOT: c_int = 0x2f;
const ABS_MT_TOUCH_MAJOR: c_int = 0x30;
const ABS_MT_TOUCH_MINOR: c_int = 0x31;
const ABS_MT_WIDTH_MAJOR: c_int = 0x32;
const ABS_MT_WIDTH_MINOR: c_int = 0x33;
const ABS_MT_ORIENTATION: c_int = 0x34;
const ABS_MT_POSITION_X: c_int = 0x35;
const ABS_MT_TOOL_TYPE: c_int = 0x37;
const ABS_MT_POSITION_Y: c_int = 0x36;
const ABS_MT_TRACKING_ID: c_int = 0x39;
const ABS_MT_PRESSURE: c_int = 0x3a;

const MT_TOOL_FINGER: i32 = 0;

const INPUT_PROP_DIRECT: c_int = 0x01;

const BUS_VIRTUAL: u16 = 0x06;
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
pub enum TouchInjectAction {
    Down,
    Move,
    Up,
}

impl TouchInjectAction {
    fn shell_word(self) -> &'static str {
        match self {
            TouchInjectAction::Down => "DOWN",
            TouchInjectAction::Move => "MOVE",
            TouchInjectAction::Up => "UP",
        }
    }
}

/// Shell fallback for devices where Android InputReader sees the uinput device
/// but does not route it to the active display. This is slower than /dev/uinput
/// but useful for validating the coordinate path.
pub fn inject_touch_motionevent_shell(action: TouchInjectAction, x: i32, y: i32) -> io::Result<()> {
    let status = Command::new("/system/bin/input")
        .arg("touchscreen")
        .arg("motionevent")
        .arg(action.shell_word())
        .arg(x.to_string())
        .arg(y.to_string())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "/system/bin/input touchscreen motionevent {} {} {} exited with {:?}",
                action.shell_word(),
                x,
                y,
                status.code()
            ),
        ))
    }
}

/// Long-lived uinput touch injector used by the live AAP session.
///
/// This intentionally supports one active pointer first. Multi-touch AA reports
/// are logged by the caller but only pointer 0/action_index is injected for now.
pub struct TouchInjector {
    dev: VirtualTouch,
    width: i32,
    height: i32,
    active: bool,
    next_tracking_id: i32,
}

impl TouchInjector {
    pub fn create(width: i32, height: i32) -> io::Result<(Self, String)> {
        let width = width.max(1);
        let height = height.max(1);
        let mut report = String::new();
        report.push_str("arda touch injector\n");
        report.push_str(&format!("uinput={}\n", UINPUT_PATH));
        report.push_str(&format!("size={}x{}\n", width, height));
        let dev = VirtualTouch::create(width, height, &mut report)?;
        Ok((
            Self {
                dev,
                width,
                height,
                active: false,
                next_tracking_id: 1,
            },
            report,
        ))
    }

    pub fn width(&self) -> i32 { self.width }
    pub fn height(&self) -> i32 { self.height }
    pub fn is_active(&self) -> bool { self.active }

    pub fn inject(&mut self, action: TouchInjectAction, x: i32, y: i32) -> io::Result<()> {
        let x = clamp_i32(x, 0, self.width.saturating_sub(1));
        let y = clamp_i32(y, 0, self.height.saturating_sub(1));
        match action {
            TouchInjectAction::Down => {
                if self.active {
                    let _ = self.dev.touch_up();
                    self.active = false;
                }
                let tracking_id = self.next_tracking_id;
                self.next_tracking_id = self.next_tracking_id.wrapping_add(1).max(1);
                self.dev.touch_down(x, y, tracking_id)?;
                self.active = true;
            }
            TouchInjectAction::Move => {
                if self.active {
                    self.dev.touch_move(x, y)?;
                } else {
                    let tracking_id = self.next_tracking_id;
                    self.next_tracking_id = self.next_tracking_id.wrapping_add(1).max(1);
                    self.dev.touch_down(x, y, tracking_id)?;
                    self.active = true;
                }
            }
            TouchInjectAction::Up => {
                if self.active {
                    self.dev.touch_move(x, y)?;
                    self.dev.touch_up()?;
                    self.active = false;
                }
            }
        }
        Ok(())
    }

    pub fn force_up(&mut self) -> io::Result<()> {
        if self.active {
            self.dev.touch_up()?;
            self.active = false;
        }
        Ok(())
    }
}

/// Shell-based fallback for physical-key injection. This sends a complete
/// Android key press on key-down; key-up reports are ignored by the caller.
pub fn inject_android_keyevent_shell(keycode: u32) -> io::Result<()> {
    let status = Command::new("/system/bin/input")
        .arg("keyevent")
        .arg(keycode.to_string())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("/system/bin/input keyevent {} exited with {:?}", keycode, status.code()),
        ))
    }
}

pub fn android_passthrough_keycode(aa_keycode: u32) -> Option<u32> {
    match aa_keycode {
        // Standard Android keycodes that are safe to forward as-is.
        3 | 4 | 5 | 6 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 79 | 82 | 84 | 85 | 86 | 87 | 88
        | 89 | 90 | 126 | 127 | 164 | 219 | 231 | 260 | 261 | 262 | 263 => Some(aa_keycode),
        // AA_* / OEM pseudo-keycodes need real-HU logs before mapping.
        65536..=65544 => None,
        _ => None,
    }
}

fn clamp_i32(value: i32, min: i32, max: i32) -> i32 {
    value.max(min).min(max.max(min))
}

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
        ioctl_int(fd, UI_SET_KEYBIT, BTN_TOOL_FINGER)?;
        ioctl_int(fd, UI_SET_KEYBIT, BTN_TOUCH)?;
        ioctl_int(fd, UI_SET_PROPBIT, INPUT_PROP_DIRECT)?;

        for abs in [
            ABS_X,
            ABS_Y,
            ABS_PRESSURE,
            ABS_MT_SLOT,
            ABS_MT_TOUCH_MAJOR,
            ABS_MT_TOUCH_MINOR,
            ABS_MT_WIDTH_MAJOR,
            ABS_MT_WIDTH_MINOR,
            ABS_MT_ORIENTATION,
            ABS_MT_POSITION_X,
            ABS_MT_POSITION_Y,
            ABS_MT_TOOL_TYPE,
            ABS_MT_TRACKING_ID,
            ABS_MT_PRESSURE,
        ] {
            ioctl_int(fd, UI_SET_ABSBIT, abs)?;
        }

        let mut user_dev = zeroed_user_dev();
        set_name(&mut user_dev.name, "arda-virtual-touch");
        // Do not expose this as BUS_USB. Android often treats USB direct-touch
        // devices as external/unassociated unless they have a display viewport
        // mapping. BUS_VIRTUAL keeps the device in the built-in/default-display
        // path, which is what we need while arda is driving the captured wm
        // display.
        user_dev.id = InputId {
            bustype: BUS_VIRTUAL,
            vendor: 0x1209,
            product: 0xAADA,
            version: 1,
        };
        report.push_str("bus=BUS_VIRTUAL note=avoid Android external touchscreen viewport routing\n");

        set_abs_range(&mut user_dev, ABS_X, 0, width.saturating_sub(1));
        set_abs_range(&mut user_dev, ABS_Y, 0, height.saturating_sub(1));
        set_abs_range(&mut user_dev, ABS_PRESSURE, 0, 255);
        set_abs_range(&mut user_dev, ABS_MT_SLOT, 0, 0);
        set_abs_range(&mut user_dev, ABS_MT_TOUCH_MAJOR, 0, 255);
        set_abs_range(&mut user_dev, ABS_MT_TOUCH_MINOR, 0, 255);
        set_abs_range(&mut user_dev, ABS_MT_WIDTH_MAJOR, 0, 255);
        set_abs_range(&mut user_dev, ABS_MT_WIDTH_MINOR, 0, 255);
        set_abs_range(&mut user_dev, ABS_MT_ORIENTATION, -90, 90);
        set_abs_range(&mut user_dev, ABS_MT_POSITION_X, 0, width.saturating_sub(1));
        set_abs_range(&mut user_dev, ABS_MT_POSITION_Y, 0, height.saturating_sub(1));
        set_abs_range(&mut user_dev, ABS_MT_TOOL_TYPE, 0, 1);
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
        self.emit_abs(ABS_MT_TOOL_TYPE, MT_TOOL_FINGER)?;
        self.emit_abs(ABS_MT_TOUCH_MAJOR, 8)?;
        self.emit_abs(ABS_MT_TOUCH_MINOR, 8)?;
        self.emit_abs(ABS_MT_WIDTH_MAJOR, 8)?;
        self.emit_abs(ABS_MT_WIDTH_MINOR, 8)?;
        self.emit_abs(ABS_MT_ORIENTATION, 0)?;
        self.emit_abs(ABS_MT_PRESSURE, 80)?;
        self.emit_abs(ABS_MT_POSITION_X, x)?;
        self.emit_abs(ABS_MT_POSITION_Y, y)?;
        self.emit_abs(ABS_X, x)?;
        self.emit_abs(ABS_Y, y)?;
        self.emit_abs(ABS_PRESSURE, 80)?;
        self.emit_key(BTN_TOOL_FINGER, 1)?;
        self.emit_key(BTN_TOUCH, 1)?;
        self.syn()
    }

    fn touch_move(&mut self, x: i32, y: i32) -> io::Result<()> {
        self.emit_abs(ABS_MT_SLOT, 0)?;
        self.emit_abs(ABS_MT_TOUCH_MAJOR, 8)?;
        self.emit_abs(ABS_MT_PRESSURE, 80)?;
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
        self.emit_key(BTN_TOOL_FINGER, 0)?;
        self.syn()
    }

    fn emit_abs(&mut self, code: c_int, value: i32) -> io::Result<()> {
        self.emit(EV_ABS as u16, code as u16, value)
    }

    fn emit_key(&mut self, code: c_int, value: i32) -> io::Result<()> {
        self.emit(EV_KEY as u16, code as u16, value)
    }

    fn syn(&mut self) -> io::Result<()> {
        self.emit(EV_SYN as u16, SYN_REPORT, 0)?;
        self.file.flush()
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
