//! Linux /dev/uinput based input injection.
//!
//! This is intentionally small and dependency-free because arda-senderd is
//! cross-compiled with a minimal Android NDK Docker image. The first target is a
//! virtual direct touchscreen that can emit tap and swipe gestures.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::mem;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::os::raw::{c_char, c_int, c_long, c_ulong};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
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

/// Full Android MotionEvent action for the framework/platform bridge.
///
/// This is deliberately separate from `TouchInjectAction`, which remains the
/// compatibility action set for shell/uinput/single-pointer fallbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiTouchInjectAction {
    Down,
    Move,
    Up,
    PointerDown,
    PointerUp,
    Cancel,
}

impl MultiTouchInjectAction {
    fn bridge_word(self) -> &'static str {
        match self {
            MultiTouchInjectAction::Down => "DOWN",
            MultiTouchInjectAction::Move => "MOVE",
            MultiTouchInjectAction::Up => "UP",
            MultiTouchInjectAction::PointerDown => "POINTER_DOWN",
            MultiTouchInjectAction::PointerUp => "POINTER_UP",
            MultiTouchInjectAction::Cancel => "CANCEL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MultiTouchPointer {
    pub pointer_id: u32,
    pub x: i32,
    pub y: i32,
}

const MAX_FRAMEWORK_TOUCH_POINTERS: usize = 16;

fn validate_multi_touch_event(
    action: MultiTouchInjectAction,
    action_index: u32,
    pointers: &[MultiTouchPointer],
) -> io::Result<()> {
    if pointers.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "multi-touch event has no pointers",
        ));
    }
    if pointers.len() > MAX_FRAMEWORK_TOUCH_POINTERS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "multi-touch pointer count {} exceeds limit {}",
                pointers.len(),
                MAX_FRAMEWORK_TOUCH_POINTERS
            ),
        ));
    }
    if action_index as usize >= pointers.len()
        && !matches!(action, MultiTouchInjectAction::Move | MultiTouchInjectAction::Cancel)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "multi-touch action index {} is outside pointer count {}",
                action_index,
                pointers.len()
            ),
        ));
    }

    let mut ids = std::collections::BTreeSet::new();
    for pointer in pointers {
        if pointer.pointer_id > 31 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Android MotionEvent pointer id {} exceeds 31",
                    pointer.pointer_id
                ),
            ));
        }
        if !ids.insert(pointer.pointer_id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate multi-touch pointer id {}", pointer.pointer_id),
            ));
        }
    }
    Ok(())
}

fn build_multi_touch_line(
    action: MultiTouchInjectAction,
    action_index: u32,
    pointers: &[MultiTouchPointer],
) -> io::Result<String> {
    validate_multi_touch_event(action, action_index, pointers)?;
    let mut line = format!(
        "TM {} {} {}",
        action.bridge_word(),
        action_index,
        pointers.len()
    );
    for pointer in pointers {
        use std::fmt::Write as _;
        write!(
            &mut line,
            " {} {} {}",
            pointer.pointer_id,
            pointer.x,
            pointer.y
        )
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "failed to format multi-touch line"))?;
    }
    Ok(line)
}

const INPUTD_JAR_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/arda_inputd.jar"));
const INPUTD_JAR_PATH: &str = "/data/local/tmp/arda-inputd.jar";
const PLATFORM_INPUT_BRIDGE_SOCKET_NAME: &str = "arda_input_bridge";

/// Client for the platform-integrated privileged ARDA input bridge.
///
/// The bridge is a persistent system_ext app signed with the platform key. It
/// owns INJECT_EVENTS and performs InputManagerGlobal.injectInputEvent() inside
/// Android's framework process model. arda-senderd only forwards the already
/// mapped AAP input reports over a root-authenticated abstract UNIX-domain socket line protocol:
///
///   DISPLAY <id>
///   TM <action> <action_index> <count> <pointer_id> <x> <y> ...
///   T DOWN|MOVE|UP <x> <y> <pointer_id>   (legacy compatibility)
///   K <android_keycode>
///   STOP
pub struct PlatformInputBridgeInjector {
    stream: Option<UnixStream>,
    display_id: i32,
}

fn connect_abstract_unix_stream(name: &str) -> io::Result<UnixStream> {
    let name_bytes = name.as_bytes();
    if name_bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "abstract UNIX socket name is empty",
        ));
    }
    if name_bytes.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "abstract UNIX socket name contains NUL",
        ));
    }

    let mut address: libc::sockaddr_un = unsafe { mem::zeroed() };
    let max_name_len = address.sun_path.len().saturating_sub(1);
    if name_bytes.len() > max_name_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "abstract UNIX socket name is too long: {} > {}",
                name_bytes.len(),
                max_name_len
            ),
        ));
    }

    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    address.sun_path[0] = 0;
    for (dst, src) in address.sun_path[1..].iter_mut().zip(name_bytes.iter()) {
        *dst = *src as c_char;
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

    let address_base = (&address as *const libc::sockaddr_un).cast::<u8>() as usize;
    let path_base = address.sun_path.as_ptr().cast::<u8>() as usize;
    let address_len = path_base
        .saturating_sub(address_base)
        .saturating_add(1)
        .saturating_add(name_bytes.len());

    let connect_result = unsafe {
        libc::connect(
            fd,
            (&address as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
            address_len as libc::socklen_t,
        )
    };

    if connect_result != 0 {
        let err = io::Error::last_os_error();
        unsafe {
            libc::close(fd);
        }
        return Err(err);
    }

    Ok(unsafe { UnixStream::from_raw_fd(fd) })
}

impl PlatformInputBridgeInjector {
    pub fn create(display_id: i32) -> io::Result<(Self, String)> {
        let mut injector = Self {
            stream: None,
            display_id,
        };
        injector.connect()?;

        let mut report = String::new();
        report.push_str("arda platform input bridge client\n");
        report.push_str(&format!(
            "socket_name={}\n",
            PLATFORM_INPUT_BRIDGE_SOCKET_NAME
        ));
        report.push_str("transport=unix-abstract\n");
        report.push_str("peer_auth=SO_PEERCRED/root-uid\n");
        report.push_str(&format!("display_id={}\n", display_id));
        report.push_str("protocol=line-v2-multipointer\n");

        Ok((injector, report))
    }

    fn connect(&mut self) -> io::Result<()> {
        let mut stream = connect_abstract_unix_stream(PLATFORM_INPUT_BRIDGE_SOCKET_NAME)?;
        writeln!(stream, "DISPLAY {}", self.display_id)?;
        stream.flush()?;
        self.stream = Some(stream);
        Ok(())
    }

    fn send_line(&mut self, line: &str) -> io::Result<()> {
        let result = {
            let stream = self.stream.as_mut().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotConnected,
                    "platform input bridge socket is not connected",
                )
            })?;
            writeln!(stream, "{line}").and_then(|_| stream.flush())
        };

        if let Err(err) = result {
            if let Some(stream) = self.stream.take() {
                let _ = stream.shutdown(Shutdown::Both);
            }
            return Err(err);
        }

        Ok(())
    }

    pub fn inject_touch(
        &mut self,
        action: TouchInjectAction,
        x: i32,
        y: i32,
        pointer_id: u32,
    ) -> io::Result<()> {
        self.send_line(&format!(
            "T {} {} {} {}",
            action.shell_word(),
            x,
            y,
            pointer_id
        ))
    }

    pub fn inject_multi_touch(
        &mut self,
        action: MultiTouchInjectAction,
        action_index: u32,
        pointers: &[MultiTouchPointer],
    ) -> io::Result<()> {
        let line = build_multi_touch_line(action, action_index, pointers)?;
        self.send_line(&line)
    }

    pub fn inject_key_press(&mut self, keycode: u32) -> io::Result<()> {
        self.send_line(&format!("K {}", keycode))
    }

    pub fn stop(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            let _ = writeln!(stream, "STOP");
            let _ = stream.flush();
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for PlatformInputBridgeInjector {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Long-lived Android framework injector launched through app_process.
///
/// This follows the scrcpy-style path: arda-senderd remains the single user
/// entry point, but it starts a tiny Java child process under the same uid. The
/// child owns MotionEvent/KeyEvent creation and calls
/// InputManager.injectInputEvent(), so Android receives framework input events
/// directly instead of trying to route a /dev/uinput device to a display.
pub struct AppProcessInputInjector {
    child: Child,
    stdin: Option<ChildStdin>,
    jar_path: PathBuf,
}

impl AppProcessInputInjector {
    pub fn create(display_id: i32) -> io::Result<(Self, String)> {
        if INPUTD_JAR_BYTES.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "embedded arda_inputd.jar is empty; rebuild Android target with d8/android.jar available",
            ));
        }

        let jar_path = PathBuf::from(INPUTD_JAR_PATH);
        fs::write(&jar_path, INPUTD_JAR_BYTES)?;

        let mut report = String::new();
        report.push_str("arda app_process input injector\n");
        report.push_str(&format!("jar={} bytes={}\n", jar_path.display(), INPUTD_JAR_BYTES.len()));
        report.push_str(&format!("display_id={}\n", display_id));

        // The live USB AAP session currently runs arda-senderd as root so it can
        // open /dev/usb_accessory. Keep the transport parent privileged for now,
        // but do not inherit uid 0 into the framework input injector. Android's
        // shell uid owns the established INJECT_EVENTS path used by adb shell
        // input. Drop uid/gid in the child immediately before exec instead of
        // depending on /system/bin/su, which is not present on all userdebug
        // images.
        let parent_euid = effective_uid().unwrap_or(u32::MAX);
        let mut child_cmd = Command::new("/system/bin/app_process");
        child_cmd
            .env("CLASSPATH", &jar_path)
            .arg("/")
            .arg("com.arda.input.InputServer")
            .arg("--display")
            .arg(display_id.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        if parent_euid == 0 {
            report.push_str("launch_as=shell-direct parent_euid=0 uid=2000 gid=2000\n");
            child_cmd.uid(2000).gid(2000);
        } else {
            report.push_str(&format!("launch_as=inherited parent_euid={}\n", parent_euid));
        }

        let mut child = child_cmd.spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "app_process child stdin missing")
        })?;

        report.push_str(&format!("pid={}\n", child.id()));
        thread::sleep(Duration::from_millis(250));

        if let Some(status) = child.try_wait()? {
            let _ = fs::remove_file(&jar_path);
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                format!("app_process inputd exited during startup: {status}"),
            ));
        }

        Ok((Self { child, stdin: Some(stdin), jar_path }, report))
    }

    pub fn inject_touch(
        &mut self,
        action: TouchInjectAction,
        x: i32,
        y: i32,
        pointer_id: u32,
    ) -> io::Result<()> {
        let stdin = self.stdin.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "app_process inputd stdin is closed")
        })?;
        writeln!(stdin, "T {} {} {} {}", action.shell_word(), x, y, pointer_id)?;
        stdin.flush()
    }

    pub fn inject_multi_touch(
        &mut self,
        action: MultiTouchInjectAction,
        action_index: u32,
        pointers: &[MultiTouchPointer],
    ) -> io::Result<()> {
        let line = build_multi_touch_line(action, action_index, pointers)?;
        let stdin = self.stdin.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "app_process inputd stdin is closed")
        })?;
        writeln!(stdin, "{line}")?;
        stdin.flush()
    }

    pub fn inject_key_press(&mut self, keycode: u32) -> io::Result<()> {
        let stdin = self.stdin.as_mut().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "app_process inputd stdin is closed")
        })?;
        writeln!(stdin, "K {}", keycode)?;
        stdin.flush()
    }

    pub fn stop(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = writeln!(stdin, "STOP");
            let _ = stdin.flush();
        }

        for _ in 0..10 {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    let _ = fs::remove_file(&self.jar_path);
                    return;
                }
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(_) => break,
            }
        }

        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.jar_path);
    }
}

impl Drop for AppProcessInputInjector {
    fn drop(&mut self) {
        self.stop();
    }
}


fn append_audio_capture_permission_prep(report: &mut String, package_name: &str) {
    // app_process runs under the adb shell uid. AudioRecord policy checks often
    // use uid+package attribution; make the shell package explicit and best-effort
    // open the relevant app-ops before starting the Java capture process. These
    // commands are intentionally non-fatal because some AAOS builds do not expose
    // every permission/op name.
    let commands: Vec<(&str, Vec<&str>)> = vec![
        ("/system/bin/pm", vec!["grant", package_name, "android.permission.RECORD_AUDIO"]),
        ("/system/bin/pm", vec!["grant", package_name, "android.permission.CAPTURE_AUDIO_OUTPUT"]),
        ("/system/bin/cmd", vec!["appops", "set", package_name, "RECORD_AUDIO", "allow"]),
        ("/system/bin/cmd", vec!["appops", "set", package_name, "CAPTURE_AUDIO_OUTPUT", "allow"]),
    ];

    for (bin, args) in commands {
        match Command::new(bin).args(&args).output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().replace('\n', " ");
                let stderr = String::from_utf8_lossy(&output.stderr).trim().replace('\n', " ");
                report.push_str(&format!(
                    "prep {} {} status={:?} stdout='{}' stderr='{}'\n",
                    bin,
                    args.join(" "),
                    output.status.code(),
                    stdout,
                    stderr,
                ));
            }
            Err(err) => {
                report.push_str(&format!("prep {} {} failed={}\n", bin, args.join(" "), err));
            }
        }
    }
}

fn normalize_audio_capture_launch_as(value: &str) -> &str {
    match value.trim().to_ascii_lowercase().as_str() {
        "root" | "0" | "su" => "root",
        "system" | "1000" => "system",
        "media" | "1013" => "media",
        "audioserver" | "audio-server" | "1041" => "audioserver",
        _ => "shell",
    }
}

fn effective_uid() -> Option<u32> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            let mut parts = rest.split_whitespace();
            let _real = parts.next()?;
            return parts.next()?.parse().ok();
        }
    }
    None
}

fn shell_quote(value: &str) -> String {
    let mut out = String::from("'");
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Long-lived Android audio capture launched through app_process.
///
/// The child process uses AudioRecord and writes raw PCM16LE to stdout. Stderr
/// is inherited for lifecycle/error logs. The Rust side drains stdout on a
/// background thread so the main AAP loop can package PCM into MEDIA_MESSAGE_DATA.
pub struct AppProcessAudioCapture {
    child: Child,
    rx: mpsc::Receiver<Vec<u8>>,
    jar_path: PathBuf,
}

impl AppProcessAudioCapture {
    pub fn create(
        sample_rate: u32,
        channels: u32,
        source: &str,
        chunk_ms: u64,
        launch_as: &str,
        package_name: &str,
    ) -> io::Result<(Self, String)> {
        if INPUTD_JAR_BYTES.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "embedded arda_inputd.jar is empty; rebuild Android target with d8/android.jar available",
            ));
        }

        let jar_path = PathBuf::from(INPUTD_JAR_PATH);
        fs::write(&jar_path, INPUTD_JAR_BYTES)?;

        let mut report = String::new();
        report.push_str("arda app_process audio capture\n");
        report.push_str(&format!("jar={} bytes={}\n", jar_path.display(), INPUTD_JAR_BYTES.len()));
        let launch_as = normalize_audio_capture_launch_as(launch_as);
        let package_name = if package_name.trim().is_empty() { "com.android.shell" } else { package_name.trim() };
        report.push_str(&format!("rate={} channels={} source={} chunk_ms={} launch_as={} package={}\n", sample_rate, channels, source, chunk_ms, launch_as, package_name));
        append_audio_capture_permission_prep(&mut report, package_name);

        let mut base_args = vec![
            "/".to_string(),
            "com.arda.input.AudioCaptureServer".to_string(),
            "--rate".to_string(),
            sample_rate.to_string(),
            "--channels".to_string(),
            channels.to_string(),
            "--source".to_string(),
            source.to_string(),
            "--chunk-ms".to_string(),
            chunk_ms.to_string(),
            "--package".to_string(),
            package_name.to_string(),
        ];

        let mut child_cmd;
        let mut child = if launch_as == "shell" {
            child_cmd = Command::new("/system/bin/app_process");
            child_cmd.env("CLASSPATH", &jar_path);
            for arg in &base_args {
                child_cmd.arg(arg);
            }
            child_cmd
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()?
        } else {
            let uid = match launch_as {
                "root" => "0",
                "system" => "1000",
                "media" => "1013",
                "audioserver" => "1041",
                _ => "0",
            };
            let mut script = String::new();
            script.push_str("CLASSPATH=");
            script.push_str(&shell_quote(&jar_path.to_string_lossy()));
            script.push_str(" exec /system/bin/app_process");
            for arg in &base_args {
                script.push(' ');
                script.push_str(&shell_quote(arg));
            }
            report.push_str(&format!("spawn=/system/bin/su {} sh -c {}\n", uid, script));
            child_cmd = Command::new("/system/bin/su");
            child_cmd
                .arg(uid)
                .arg("sh")
                .arg("-c")
                .arg(script)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()?
        };

        let mut stdout = child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "app_process audiocap stdout missing")
        })?;
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || {
            let mut buf = vec![0u8; 8192];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        report.push_str(&format!("pid={}\n", child.id()));
        thread::sleep(Duration::from_millis(250));

        Ok((Self { child, rx, jar_path }, report))
    }

    pub fn drain_into(&mut self, dst: &mut Vec<u8>, max_chunks: usize) -> usize {
        let mut chunks = 0usize;
        while chunks < max_chunks {
            match self.rx.try_recv() {
                Ok(data) => {
                    dst.extend_from_slice(&data);
                    chunks += 1;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        chunks
    }

    pub fn stop(&mut self) {
        for _ in 0..10 {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    let _ = fs::remove_file(&self.jar_path);
                    return;
                }
                Ok(None) => thread::sleep(Duration::from_millis(50)),
                Err(_) => break,
            }
        }

        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.jar_path);
    }
}

impl Drop for AppProcessAudioCapture {
    fn drop(&mut self) {
        self.stop();
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

#[cfg(test)]
mod multi_touch_protocol_tests {
    use super::{
        build_multi_touch_line, MultiTouchInjectAction, MultiTouchPointer,
        MAX_FRAMEWORK_TOUCH_POINTERS,
    };

    #[test]
    fn encodes_three_pointer_move_without_collapsing_ids() {
        let pointers = [
            MultiTouchPointer {
                pointer_id: 2,
                x: 100,
                y: 200,
            },
            MultiTouchPointer {
                pointer_id: 7,
                x: 300,
                y: 400,
            },
            MultiTouchPointer {
                pointer_id: 11,
                x: 500,
                y: 600,
            },
        ];
        let line = build_multi_touch_line(
            MultiTouchInjectAction::Move,
            0,
            &pointers,
        )
        .expect("three-pointer MOVE should encode");
        assert_eq!(
            line,
            "TM MOVE 0 3 2 100 200 7 300 400 11 500 600"
        );
    }

    #[test]
    fn preserves_pointer_down_action_index_for_fourth_pointer() {
        let pointers = [
            MultiTouchPointer {
                pointer_id: 0,
                x: 10,
                y: 20,
            },
            MultiTouchPointer {
                pointer_id: 3,
                x: 30,
                y: 40,
            },
            MultiTouchPointer {
                pointer_id: 5,
                x: 50,
                y: 60,
            },
            MultiTouchPointer {
                pointer_id: 9,
                x: 70,
                y: 80,
            },
        ];
        let line = build_multi_touch_line(
            MultiTouchInjectAction::PointerDown,
            3,
            &pointers,
        )
        .expect("four-pointer POINTER_DOWN should encode");
        assert!(line.starts_with("TM POINTER_DOWN 3 4 "));
    }

    #[test]
    fn rejects_duplicate_pointer_ids() {
        let pointers = [
            MultiTouchPointer {
                pointer_id: 1,
                x: 10,
                y: 20,
            },
            MultiTouchPointer {
                pointer_id: 1,
                x: 30,
                y: 40,
            },
        ];
        assert!(
            build_multi_touch_line(
                MultiTouchInjectAction::PointerDown,
                1,
                &pointers,
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_more_than_framework_pointer_limit() {
        let pointers = (0..=MAX_FRAMEWORK_TOUCH_POINTERS)
            .map(|index| MultiTouchPointer {
                pointer_id: index as u32,
                x: index as i32,
                y: index as i32,
            })
            .collect::<Vec<_>>();
        assert!(
            build_multi_touch_line(
                MultiTouchInjectAction::Move,
                0,
                &pointers,
            )
            .is_err()
        );
    }
}

