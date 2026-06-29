mod aap;
mod capture;
mod h264;
mod input;
mod native;

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs as unix_fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use capture::screenrecord::{ScreenrecordConfig, ScreenrecordSource};
use h264::annexb::{AnnexBStreamParser, NalStats};

fn main() {
    if let Err(err) = real_main() {
        eprintln!("arda-senderd: error: {err}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let Some(cmd) = args.next() else {
        print_help();
        return Ok(());
    };

    auto_ensure_aaos_aoa_gadget_for_command(&cmd);

    match cmd.as_str() {
        "capture-test" => capture_test(args.collect()),
        "nal-info" => nal_info(args.collect()),
        "native-probe" => native_probe(),
        "native-encoder-test" => native_encoder_test(args.collect()),
        "native-surface-encoder-test" => native_surface_encoder_test(args.collect()),
        "native-display-probe" => native_display_probe(),
        "native-display-active-probe" => native_display_active_probe(),
        "native-display-capture-test" => native_display_capture_test(args.collect()),
        "native-capture" => native_capture(args.collect()),
        "input-test" => input_test(args.collect()),
        "aaos-aoa-gadget" | "usb-gadget-aoa" => aaos_aoa_gadget(args.collect()),
        "aap-tcp-probe" => aap_tcp_probe(args.collect()),
        "aap-frame-probe" => aap_frame_probe(args.collect()),
        "aap-version-session" => aap_version_session(args.collect()),
        "aap-ssl-frame-probe" => aap_ssl_frame_probe(args.collect()),
        "aap-tls-handshake-probe" => aap_tls_handshake_probe(args.collect()),
        "aap-tls-plain-probe" => aap_tls_plain_probe(args.collect()),
        "aap-service-discovery-probe" => aap_service_discovery_probe(args.collect()),
        "aap-session-probe" => aap_service_discovery_probe(args.collect()),
        "aap-proto-smoke" => aap_proto_smoke(),
        "aap-fragment-smoke" => aap_fragment_smoke(),
        "version" | "--version" => {
            println!("arda-senderd {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command: {other}").into()),
    }
}

fn print_help() {
    eprintln!(
        r#"arda-senderd

Commands:
  capture-test [--size 1280x720] [--bit-rate 6000000] [--seconds 10] [--out PATH|-] [--loop]
      Starts Android screenrecord in raw H264 mode and writes Annex-B H264 to PATH or stdout.

  nal-info FILE [--limit 80]
      Prints NAL types and summary for a raw Annex-B H264 stream.

  native-probe
      Runs the embedded C++/NDK probe for future native capture support.

  native-encoder-test [--size 320x240] [--bit-rate 1000000] [--fps 15] [--seconds 3] [--out PATH]
      Runs embedded C++ AMediaCodec H264 encoder with dummy black YUV frames and writes encoded H264 to PATH.

  native-surface-encoder-test [--size 320x240] [--bit-rate 1000000] [--fps 15] [--seconds 3] [--out PATH]
      Runs embedded C++ AMediaCodec input-surface encoder and draws EGL/GLES frames into the encoder surface.

  native-display-probe
      Probes Android private libgui/SurfaceComposerClient symbols needed for mirroring the AAOS display into the encoder surface.

  native-display-active-probe
      Actively calls a minimal subset of private SurfaceComposerClient APIs: getInternalDisplayToken, createDisplay, destroyDisplay.

  native-display-capture-test [--size 1280x720] [--bit-rate 6000000] [--fps 30] [--seconds 10] [--out PATH|-]
      Routes a private SurfaceComposerClient virtual display into the MediaCodec input surface and writes encoded H264 to PATH or stdout.

  native-capture [--size 1280x720] [--bit-rate 6000000] [--fps 30] [--seconds 0] [--out -|PATH]
      Production-style native display capture. Use --seconds 0 to run until the process is killed. Stdout mode is binary-clean raw H264.

  input-test [--size 1280x720] tap X Y [--duration-ms 80]
  input-test [--size 1280x720] swipe X1 Y1 X2 Y2 [--duration-ms 300] [--steps 20]
      Creates a temporary /dev/uinput virtual touchscreen and injects a tap or swipe. Run with root on AAOS.

  aaos-aoa-gadget [--udc fe980000.usb] [--wait-ms 10000] [--name arda] [--no-bind] [--check] [--unbind]
      Creates/binds the AAOS USB-C Android Open Accessory configfs gadget using accessory.usb0 and /dev/usb_accessory. Run as root on AAOS.

  aap-tcp-probe [--addr 0.0.0.0:5277] [--dump PATH] [--timeout-ms 60000]
      Raw TCP probe: accepts one client and hex-dumps incoming bytes.

  aap-frame-probe [--addr 0.0.0.0:5277] [--dump PATH] [--timeout-ms 60000] [--max-frames 256]
      AAP frame probe: parses AA channel/flags/length/final_length framing and names plaintext control messages.

  aap-version-session [--addr 0.0.0.0:5277] [--dump PATH] [--response 1.7] [--status 0]
      Accepts one AAP TCP client, reads plaintext VERSION_REQUEST, replies with VERSION_RESPONSE, then closes.

  aap-ssl-frame-probe [--addr 0.0.0.0:5277] [--dump PATH] [--response 1.7] [--max-frames 8]
      Accepts one AAP TCP client, replies to VERSION_REQUEST, then reads MESSAGE_ENCAPSULATED_SSL frames and prints TLS record hints.

  aap-tls-handshake-probe [--addr 0.0.0.0:5277] [--dump PATH] [--keys /data/local/tmp/arda-certs] [--response 1.7] [--max-frames 64]
      Runs phone-like AAP VERSION_RESPONSE, then TLS server handshake over MESSAGE_ENCAPSULATED_SSL using md_cert/md_key. Peer cert is accepted.

  aap-tls-plain-probe [--addr 0.0.0.0:5277] [--dump PATH] [--keys /data/local/tmp/arda-certs] [--response 1.7] [--max-frames 128]
      Runs VERSION + TLS, then keeps reading encrypted post-TLS AA frames and logs decrypted plaintext messages.

  aap-service-discovery-probe [--addr 0.0.0.0:5277] [--dump PATH] [--keys /data/local/tmp/arda-certs] [--response 1.7] [--max-frames 256]
      Runs VERSION + TLS, waits for AUTH_COMPLETE, sends encrypted protobuf ServiceDiscoveryRequest, and replies to PING_REQUEST.

  aap-session-probe [--addr 0.0.0.0:5277] [--dump PATH] [--keys /data/local/tmp/arda-certs] [--response 1.7] [--max-frames 256] [--video-file PATH] [--video-frames 30]
      Full AAP session probe: service discovery, video channel open/setup/start, ping responses, optional H264 file send.

  aap-session-probe ... --video-native [--video-config-policy highest|balanced|lowest|index:N] [--video-size WIDTHxHEIGHT --video-fps N --video-density DPI] [--apply-wm-config --wm-size-mode stream|content|safe --wm-density-mode density|real --wm-settle-ms 1200|--fast-start] [--display-auto|--display-profile waveshare-79] [--wm-rotation none|auto|0|90|180|270] [--dhu-priority|--wm-size-orientation logical|natural]
      Starts native-capture as an internal child after video START and sends live H264 to the opened video channel.
      Current AAP session: service discovery, video channel open, AV setup/start, ping responses. With --video-file, sends Annex-B H264 after START.

  aap-session-probe ... --input-open [--input-key-binding|--no-input-key-binding] [--input-inject] [--input-inject-mode uinput|app|shell] [--input-app-move-min-ms N] [--input-key-inject] [--quiet-ping|--log-ping] [--log-level quiet|summary|normal|verbose]
      Opens the HU InputSourceService channel, logs touchscreen/touchpad/key/rotary reports, optionally sends key binding request, and can inject touchscreen/key events into Android. The default input injection mode is uinput; app_process/framework remains available via --input-inject-mode app, with shell fallback also available. Ping request/response logs are quiet by default in session mode. Use --log-level quiet/summary for low-latency testing; --log-level verbose restores wire/per-frame logs. Use --fast-start to reduce WM settle to 300ms, enable startup timing logs, and use a conservative first-frame restart guard. Use --video-first-frame-kick to synthesize a tiny AAOS input event after native capture starts if the first frame waits for a screen change; modes include edge tap, center tap, center-cancel, center-move-cancel, wm-nudge, touch-cancel, wakeup key, and explicit keyevent. Use --display-profile waveshare-79 to apply the known Waveshare 7.9 rotation/DHU-priority profile, or --wm-rotation 90/270 for manual correction.

  aap-session-probe ... --audio-open [--audio-stream all|media|guidance|system|telephony] [--audio-test-tone|--audio-capture] [--audio-capture-source remote_submix|mic|default] [--audio-dump|--audio-dump-dir PATH] [--mic-open] [--mic-dump PATH] [--no-audio-ack]
      Opens HU audio MediaSinkService channels, sends PCM setup/start, dumps inbound phone PCM data when requested, and ACKs received audio packets.

  aap-session-probe ... --sensor-open [--sensor-max-type 21] [--sensor-jsonl PATH] [--sensor-forward-os] [--sensor-forward-vhal] [--sensor-vhal-min-ms 500]
      Opens the HU SensorSourceService channel, subscribes to advertised sensor types up to 21, decodes SensorBatch messages, optionally writes JSONL, and best-effort forwards LOCATION through cmd location plus supported vehicle values through cmd car_service inject-vhal-event.

  aap-fragment-smoke
      Verifies AAP FIRST/MIDDLE/LAST plaintext fragmentation sizing used before TLS encryption.

Examples on AAOS:
  /data/local/tmp/arda-senderd capture-test --size 1280x720 --seconds 10 --out /sdcard/arda.h264

Live native capture from Mac:
  adb exec-out /data/local/tmp/arda-senderd native-capture --size 1280x720 --bit-rate 6000000 --fps 30 --seconds 0 --out - \
    | mpv --demuxer=lavf --demuxer-lavf-format=h264 --no-correct-pts --container-fps-override=30 --profile=low-latency -
"#
    );
}


#[derive(Debug, Clone)]
struct AaosAoaGadgetArgs {
    name: String,
    udc: Option<String>,
    wait_ms: u64,
    vendor_id: String,
    product_id: String,
    serial: String,
    manufacturer: String,
    product: String,
    max_power: u32,
    no_bind: bool,
    check: bool,
    unbind: bool,
    quiet: bool,
}

fn aaos_aoa_gadget(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_aaos_aoa_gadget_args(argv.clone())?;
    let gadget_path = format!("/config/usb_gadget/{}", args.name);

    if args.check {
        return print_aoa_gadget_status(&gadget_path);
    }

    if !is_effective_root() {
        let mut root_argv = argv;
        if !args.quiet && !root_argv.iter().any(|arg| arg == "--quiet") {
            root_argv.push("--quiet".to_string());
        }
        run_aaos_aoa_gadget_as_root(&root_argv)?;
        if args.quiet {
            return Ok(());
        }
        return print_aoa_gadget_status(&gadget_path);
    }

    if args.unbind {
        write_configfs_text_ignore(format!("{gadget_path}/UDC"), "\n");
        if !args.quiet {
            println!("aaos-aoa-gadget: unbound {gadget_path}");
            return print_aoa_gadget_status(&gadget_path);
        }
        return Ok(());
    }

    let status = ensure_aaos_aoa_gadget(&args)?;
    if !args.quiet {
        if status.changed {
            eprintln!("aaos-aoa-gadget: configured and bound to {}", status.bound_udc.as_deref().unwrap_or("<none>"));
        } else {
            eprintln!("aaos-aoa-gadget: already configured; no changes needed");
        }
        print_aoa_gadget_status(&gadget_path)?;
    }
    Ok(())
}

fn default_aaos_aoa_gadget_args() -> AaosAoaGadgetArgs {
    AaosAoaGadgetArgs {
        name: "arda".to_string(),
        udc: None,
        wait_ms: 10_000,
        vendor_id: "0x18D1".to_string(),
        product_id: "0x2D00".to_string(),
        serial: "arda-aaos".to_string(),
        manufacturer: "aa-proxy".to_string(),
        product: "AAOS AOA".to_string(),
        max_power: 500,
        no_bind: false,
        check: false,
        unbind: false,
        quiet: false,
    }
}

fn parse_aaos_aoa_gadget_args(argv: Vec<String>) -> Result<AaosAoaGadgetArgs, Box<dyn std::error::Error>> {
    let mut args = default_aaos_aoa_gadget_args();

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--name" => {
                i += 1;
                args.name = argv.get(i).ok_or("--name requires value")?.clone();
            }
            "--udc" => {
                i += 1;
                args.udc = Some(argv.get(i).ok_or("--udc requires value")?.clone());
            }
            "--wait-ms" => {
                i += 1;
                args.wait_ms = argv.get(i).ok_or("--wait-ms requires value")?.parse()?;
            }
            "--vendor-id" => {
                i += 1;
                args.vendor_id = argv.get(i).ok_or("--vendor-id requires value")?.clone();
            }
            "--product-id" => {
                i += 1;
                args.product_id = argv.get(i).ok_or("--product-id requires value")?.clone();
            }
            "--serial" => {
                i += 1;
                args.serial = argv.get(i).ok_or("--serial requires value")?.clone();
            }
            "--manufacturer" => {
                i += 1;
                args.manufacturer = argv.get(i).ok_or("--manufacturer requires value")?.clone();
            }
            "--product" => {
                i += 1;
                args.product = argv.get(i).ok_or("--product requires value")?.clone();
            }
            "--max-power" => {
                i += 1;
                args.max_power = argv.get(i).ok_or("--max-power requires value")?.parse()?;
            }
            "--no-bind" => args.no_bind = true,
            "--check" => args.check = true,
            "--unbind" => args.unbind = true,
            "--quiet" => args.quiet = true,
            "--help" | "-h" => {
                println!(
                    "aaos-aoa-gadget [--udc fe980000.usb] [--wait-ms 10000] [--name arda] [--vendor-id 0x18D1] [--product-id 0x2D00] [--serial arda-aaos] [--manufacturer aa-proxy] [--product 'AAOS AOA'] [--max-power 500] [--no-bind] [--check] [--unbind] [--quiet]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown aaos-aoa-gadget arg: {other}").into()),
        }
        i += 1;
    }

    Ok(args)
}

#[derive(Debug, Clone)]
struct AoaGadgetStatus {
    changed: bool,
    bound_udc: Option<String>,
}

fn auto_ensure_aaos_aoa_gadget_for_command(cmd: &str) {
    if !should_auto_ensure_aaos_aoa_gadget(cmd) || env_flag_disabled("ARDA_AAOS_AOA_AUTO") {
        return;
    }

    // Fast no-op on non-AAOS hosts or kernels without a USB device controller.
    if !Path::new("/sys/class/udc").exists() {
        return;
    }

    let mut args = default_aaos_aoa_gadget_args();
    args.wait_ms = 2_000;

    match ensure_aaos_aoa_gadget_maybe_root(&args) {
        Ok(status) if status.changed => {
            eprintln!(
                "arda aaos-aoa: auto-configured USB accessory gadget on {}",
                status.bound_udc.as_deref().unwrap_or("<none>")
            );
        }
        Ok(_) => {}
        Err(err) => {
            // Do not fail the main daemon/probe because of the preflight. Some dev paths
            // intentionally run over TCP or on images without gadget support.
            eprintln!("arda aaos-aoa: auto-setup skipped: {err}");
        }
    }
}

fn should_auto_ensure_aaos_aoa_gadget(cmd: &str) -> bool {
    matches!(
        cmd,
        "aap-tcp-probe"
            | "aap-frame-probe"
            | "aap-version-session"
            | "aap-ssl-frame-probe"
            | "aap-tls-handshake-probe"
            | "aap-tls-plain-probe"
            | "aap-service-discovery-probe"
            | "aap-session-probe"
    )
}

fn env_flag_disabled(name: &str) -> bool {
    match env::var(name) {
        Ok(value) => {
            let value = value.trim().to_ascii_lowercase();
            matches!(value.as_str(), "0" | "false" | "off" | "no" | "disabled")
        }
        Err(_) => false,
    }
}

fn ensure_aaos_aoa_gadget_maybe_root(args: &AaosAoaGadgetArgs) -> Result<AoaGadgetStatus, Box<dyn std::error::Error>> {
    let mut effective_args = args.clone();
    if effective_args.udc.is_none() {
        effective_args.udc = Some(wait_for_udc(Duration::from_millis(effective_args.wait_ms))?);
    }

    let udc = effective_args.udc.as_deref().unwrap_or_default();
    let gadget_path = format!("/config/usb_gadget/{}", effective_args.name);
    let needs_root_fix = !aaos_aoa_gadget_ready(&gadget_path, udc) || !usb_accessory_node_permissions_ok();

    if needs_root_fix && !is_effective_root() {
        let root_argv = aoa_gadget_args_to_argv(&effective_args, true);
        run_aaos_aoa_gadget_as_root(&root_argv)?;
        return Ok(AoaGadgetStatus {
            changed: true,
            bound_udc: read_trim(format!("{gadget_path}/UDC")),
        });
    }

    ensure_aaos_aoa_gadget(&effective_args)
}

fn aoa_gadget_args_to_argv(args: &AaosAoaGadgetArgs, quiet: bool) -> Vec<String> {
    let mut argv = vec![
        "--name".to_string(),
        args.name.clone(),
        "--wait-ms".to_string(),
        args.wait_ms.to_string(),
        "--vendor-id".to_string(),
        args.vendor_id.clone(),
        "--product-id".to_string(),
        args.product_id.clone(),
        "--serial".to_string(),
        args.serial.clone(),
        "--manufacturer".to_string(),
        args.manufacturer.clone(),
        "--product".to_string(),
        args.product.clone(),
        "--max-power".to_string(),
        args.max_power.to_string(),
    ];
    if let Some(udc) = &args.udc {
        argv.push("--udc".to_string());
        argv.push(udc.clone());
    }
    if args.no_bind {
        argv.push("--no-bind".to_string());
    }
    if args.unbind {
        argv.push("--unbind".to_string());
    }
    if quiet || args.quiet {
        argv.push("--quiet".to_string());
    }
    argv
}

fn run_aaos_aoa_gadget_as_root(argv: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let exe = env::current_exe()
        .ok()
        .or_else(|| env::args().next().map(PathBuf::from))
        .ok_or("cannot resolve arda-senderd path for root re-exec")?;
    let exe_s = exe.to_string_lossy();
    let mut parts = vec![shell_quote(exe_s.as_ref()), "aaos-aoa-gadget".to_string()];
    parts.extend(argv.iter().map(|arg| shell_quote(arg)));
    let command_line = parts.join(" ");

    let output = Command::new("su")
        .args(["-c", command_line.as_str()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "root aaos-aoa-gadget failed: status={} stdout='{}' stderr='{}'",
            output.status,
            stdout.trim(),
            stderr.trim()
        )
        .into());
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
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

fn is_effective_root() -> bool {
    effective_uid().map(|uid| uid == 0).unwrap_or(false)
}

fn effective_uid() -> Option<u32> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            let mut parts = rest.split_whitespace();
            let _real = parts.next()?;
            let effective = parts.next()?;
            return effective.parse().ok();
        }
    }
    None
}

fn ensure_configfs_usb_gadget_mounted() -> Result<(), Box<dyn std::error::Error>> {
    if Path::new("/config/usb_gadget").exists() {
        return Ok(());
    }

    fs::create_dir_all("/config")?;
    run_command_ignore("mount", &["-t", "configfs", "configfs", "/config"]);

    if Path::new("/config/usb_gadget").exists() {
        Ok(())
    } else {
        Err("configfs is not mounted at /config or /config/usb_gadget is unavailable".into())
    }
}

fn ensure_aaos_aoa_gadget(args: &AaosAoaGadgetArgs) -> Result<AoaGadgetStatus, Box<dyn std::error::Error>> {
    ensure_configfs_usb_gadget_mounted()?;

    let gadget_path = format!("/config/usb_gadget/{}", args.name);
    let udc = match args.udc.clone() {
        Some(udc) => udc,
        None => wait_for_udc(Duration::from_millis(args.wait_ms))?,
    };

    if aaos_aoa_gadget_ready(&gadget_path, &udc) {
        ensure_usb_accessory_node()?;
        return Ok(AoaGadgetStatus {
            changed: false,
            bound_udc: read_trim(format!("{gadget_path}/UDC")),
        });
    }

    if Path::new(&gadget_path).exists() {
        write_configfs_text_ignore(format!("{gadget_path}/UDC"), "\n");
        remove_file_ignore(format!("{gadget_path}/configs/c.1/accessory.usb0"));
    }

    fs::create_dir_all(&gadget_path)?;

    write_configfs_text(format!("{gadget_path}/idVendor"), &format!("{}\n", args.vendor_id))?;
    write_configfs_text(format!("{gadget_path}/idProduct"), &format!("{}\n", args.product_id))?;
    write_configfs_text(format!("{gadget_path}/bcdDevice"), "0x0100\n")?;
    write_configfs_text(format!("{gadget_path}/bcdUSB"), "0x0200\n")?;

    fs::create_dir_all(format!("{gadget_path}/strings/0x409"))?;
    write_configfs_text(format!("{gadget_path}/strings/0x409/serialnumber"), &format!("{}\n", args.serial))?;
    write_configfs_text(format!("{gadget_path}/strings/0x409/manufacturer"), &format!("{}\n", args.manufacturer))?;
    write_configfs_text(format!("{gadget_path}/strings/0x409/product"), &format!("{}\n", args.product))?;

    fs::create_dir_all(format!("{gadget_path}/configs/c.1/strings/0x409"))?;
    write_configfs_text(format!("{gadget_path}/configs/c.1/strings/0x409/configuration"), "Android Auto Accessory\n")?;
    write_configfs_text(format!("{gadget_path}/configs/c.1/MaxPower"), &format!("{}\n", args.max_power))?;

    let function_path = format!("{gadget_path}/functions/accessory.usb0");
    let config_link = format!("{gadget_path}/configs/c.1/accessory.usb0");
    fs::create_dir_all(&function_path)?;
    remove_file_ignore(&config_link);

    // This AAOS/configfs combination rejects a relative symlink target with EPERM.
    // Use the absolute configfs function path, matching the verified manual setup.
    unix_fs::symlink(&function_path, &config_link)?;

    if !args.no_bind {
        write_configfs_text(format!("{gadget_path}/UDC"), &format!("{udc}\n"))?;
    }

    ensure_usb_accessory_node()?;
    Ok(AoaGadgetStatus {
        changed: true,
        bound_udc: read_trim(format!("{gadget_path}/UDC")),
    })
}

fn aaos_aoa_gadget_ready(gadget_path: &str, expected_udc: &str) -> bool {
    let Some(bound_udc) = read_trim(format!("{gadget_path}/UDC")) else {
        return false;
    };
    if bound_udc != expected_udc {
        return false;
    }

    let function_path = Path::new(gadget_path).join("functions/accessory.usb0");
    let config_link = Path::new(gadget_path).join("configs/c.1/accessory.usb0");
    if !function_path.exists() || !config_link.exists() {
        return false;
    }

    Path::new("/dev/usb_accessory").exists() || usb_accessory_minor().ok().flatten().is_some()
}

fn read_trim<P: AsRef<Path>>(path: P) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn wait_for_udc(timeout: Duration) -> Result<String, Box<dyn std::error::Error>> {
    let start = Instant::now();
    while start.elapsed() <= timeout {
        if let Some(udc) = first_udc()? {
            return Ok(udc);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err("no UDC found under /sys/class/udc".into())
}

fn first_udc() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let Ok(entries) = fs::read_dir("/sys/class/udc") else {
        return Ok(None);
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

fn write_configfs_text<P: AsRef<Path>>(path: P, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(path, value)?;
    Ok(())
}

fn write_configfs_text_ignore<P: AsRef<Path>>(path: P, value: &str) {
    let _ = fs::write(path, value);
}

fn remove_file_ignore<P: AsRef<Path>>(path: P) {
    let _ = fs::remove_file(path);
}

fn run_command_ignore(program: &str, args: &[&str]) {
    let _ = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn usb_accessory_node_permissions_ok() -> bool {
    let Ok(meta) = fs::metadata("/dev/usb_accessory") else {
        return false;
    };
    let mode = meta.permissions().mode() & 0o777;
    meta.uid() == 0 && mode == 0o660
}

fn ensure_usb_accessory_node() -> Result<(), Box<dyn std::error::Error>> {
    let minor = usb_accessory_minor()?;
    if let Some(minor) = minor {
        if !Path::new("/dev/usb_accessory").exists() {
            let minor_s = minor.to_string();
            run_command_ignore("mknod", &["/dev/usb_accessory", "c", "10", minor_s.as_str()]);
        }
        run_command_ignore("chown", &["root:usb", "/dev/usb_accessory"]);
        run_command_ignore("chmod", &["660", "/dev/usb_accessory"]);
    }
    Ok(())
}

fn usb_accessory_minor() -> Result<Option<u32>, Box<dyn std::error::Error>> {
    let Ok(text) = fs::read_to_string("/proc/misc") else {
        return Ok(None);
    };
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(minor) = parts.next() else { continue; };
        let Some(name) = parts.next() else { continue; };
        let lowered = name.to_ascii_lowercase();
        if lowered.contains("usb_accessory") || lowered.contains("accessory") || lowered.contains("aoa") {
            return Ok(Some(minor.parse()?));
        }
    }
    Ok(None)
}

fn print_aoa_gadget_status(gadget_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("===== aaos-aoa-gadget status =====");
    println!("gadget={gadget_path}");

    println!("\n===== /sys/class/udc =====");
    print_dir_names("/sys/class/udc")?;

    println!("\n===== gadget UDC =====");
    match fs::read_to_string(format!("{gadget_path}/UDC")) {
        Ok(text) => print!("{}", text),
        Err(err) => println!("NO_GADGET_UDC ({err})"),
    }

    println!("\n===== functions =====");
    print_dir_names(format!("{gadget_path}/functions"))?;

    println!("\n===== config c.1 =====");
    print_dir_names(format!("{gadget_path}/configs/c.1"))?;

    println!("\n===== /proc/misc accessory =====");
    let misc = fs::read_to_string("/proc/misc").unwrap_or_default();
    let mut any = false;
    for line in misc.lines() {
        let lowered = line.to_ascii_lowercase();
        if lowered.contains("usb") || lowered.contains("access") || lowered.contains("aoa") {
            println!("{line}");
            any = true;
        }
    }
    if !any {
        println!("NO_MISC_USB_ACCESSORY");
    }

    println!("\n===== /dev/usb_accessory =====");
    match fs::metadata("/dev/usb_accessory") {
        Ok(meta) => println!("/dev/usb_accessory exists: mode={:o}", meta.permissions().mode() & 0o7777),
        Err(_) => println!("NO_DEV_USB_ACCESSORY"),
    }

    Ok(())
}

fn print_dir_names<P: AsRef<Path>>(path: P) -> Result<(), Box<dyn std::error::Error>> {
    let path_ref = path.as_ref();
    let Ok(entries) = fs::read_dir(path_ref) else {
        println!("{}: not found or unreadable", path_ref.display());
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            let target = fs::read_link(entry.path())?;
            println!("{name} -> {}", target.display());
        } else if ty.is_dir() {
            println!("{name}/");
        } else {
            println!("{name}");
        }
    }
    Ok(())
}

#[derive(Debug)]
struct CaptureArgs {
    size: String,
    bit_rate: u32,
    seconds: u32,
    out: OutputTarget,
    loop_forever: bool,
    verbose: bool,
}

#[derive(Debug)]
enum OutputTarget {
    Stdout,
    File(PathBuf),
}

fn capture_test(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_capture_args(argv)?;

    let output_is_stdout = matches!(args.out, OutputTarget::Stdout);
    let mut writer: Option<Box<dyn Write>> = match &args.out {
        OutputTarget::Stdout => None,
        OutputTarget::File(path) => Some(Box::new(File::create(path)?)),
    };

    let mut total_stats = NalStats::default();
    let mut run_index = 0u64;

    loop {
        run_index += 1;
        if !output_is_stdout {
            eprintln!(
                "arda capture: starting screenrecord run #{run_index}: size={} bit_rate={} seconds={}",
                args.size, args.bit_rate, args.seconds
            );
        }

        let config = ScreenrecordConfig {
            size: args.size.clone(),
            bit_rate: args.bit_rate,
            seconds: args.seconds,
            display_id: None,
        };

        if output_is_stdout {
            // Binary-clean live mode. Do not print any diagnostics to stdout/stderr.
            // Let screenrecord write raw H264 directly to this process stdout.
            let _status = ScreenrecordSource::run_direct_to_stdout(config)?;
        } else {
            let mut source = ScreenrecordSource::spawn(config)?;
            let mut buf = [0u8; 64 * 1024];
            let mut parser = AnnexBStreamParser::new();
            let mut run_stats = NalStats::default();

            loop {
                let read = match source.stdout_mut().read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                    Err(err) => return Err(err.into()),
                };

                parser.push(&buf[..read]);
                while let Some(nal) = parser.next_nal() {
                    let nal_type = nal.nal_type();
                    run_stats.add(nal_type);
                    total_stats.add(nal_type);
                    if args.verbose {
                        eprintln!(
                            "nal type={:?} len={} key={}",
                            nal_type,
                            nal.bytes().len(),
                            nal_type.is_key_video_unit()
                        );
                    }
                    writer.as_mut().expect("file writer").write_all(nal.bytes())?;
                }
            }

            // Flush final partially buffered NAL at process end.
            if let Some(nal) = parser.finish() {
                let nal_type = nal.nal_type();
                run_stats.add(nal_type);
                total_stats.add(nal_type);
                writer.as_mut().expect("file writer").write_all(nal.bytes())?;
            }
            writer.as_mut().expect("file writer").flush()?;

            let status = source.wait_timeout(Duration::from_secs(2))?;
            eprintln!("arda capture: run #{run_index} ended: status={status:?} stats={run_stats}");
        }

        if !args.loop_forever {
            if !output_is_stdout {
                eprintln!("arda capture: total stats={total_stats}");
            }
            break;
        }
    }

    Ok(())
}

fn parse_capture_args(argv: Vec<String>) -> Result<CaptureArgs, Box<dyn std::error::Error>> {
    let mut size = String::from("1280x720");
    let mut bit_rate = 6_000_000u32;
    let mut seconds = 10u32;
    let mut out = OutputTarget::Stdout;
    let mut loop_forever = false;
    let mut verbose = false;

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--size" => {
                i += 1;
                size = argv.get(i).ok_or("--size requires value")?.clone();
            }
            "--bit-rate" | "--bitrate" => {
                i += 1;
                bit_rate = parse_u32(argv.get(i).ok_or("--bit-rate requires value")?)?;
            }
            "--seconds" | "--time-limit" => {
                i += 1;
                seconds = parse_u32(argv.get(i).ok_or("--seconds requires value")?)?;
            }
            "--out" | "-o" => {
                i += 1;
                let value = argv.get(i).ok_or("--out requires value")?;
                out = if value == "-" {
                    OutputTarget::Stdout
                } else {
                    OutputTarget::File(PathBuf::from(value))
                };
            }
            "--loop" => loop_forever = true,
            "--verbose" | "-v" => verbose = true,
            other => return Err(format!("unknown capture-test arg: {other}").into()),
        }
        i += 1;
    }

    if !size.contains('x') {
        return Err("--size must look like WIDTHxHEIGHT".into());
    }
    if seconds == 0 || seconds > 180 {
        return Err("screenrecord seconds must be 1..180".into());
    }

    Ok(CaptureArgs {
        size,
        bit_rate,
        seconds,
        out,
        loop_forever,
        verbose,
    })
}

fn parse_u32(s: &str) -> Result<u32, Box<dyn std::error::Error>> {
    Ok(s.parse::<u32>()?)
}

fn native_probe() -> Result<(), Box<dyn std::error::Error>> {
    let report = native::probe().map_err(|err| format!("native probe failed: {err}"))?;
    print!("{report}");
    Ok(())
}


fn native_display_probe() -> Result<(), Box<dyn std::error::Error>> {
    let report = native::display_probe().map_err(|err| format!("native display probe failed: {err}"))?;
    print!("{report}");
    Ok(())
}

fn native_display_active_probe() -> Result<(), Box<dyn std::error::Error>> {
    let report = native::display_active_probe()
        .map_err(|err| format!("native display active probe failed: {err}"))?;
    print!("{report}");
    Ok(())
}

#[derive(Debug)]
struct NativeEncoderArgs {
    width: i32,
    height: i32,
    bit_rate: i32,
    fps: i32,
    seconds: i32,
    out: String,
}

fn native_encoder_test(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_native_encoder_args(argv)?;
    let report = native::encoder_test(
        &args.out,
        args.width,
        args.height,
        args.bit_rate,
        args.fps,
        args.seconds,
    )
    .map_err(|err| format!("native encoder test failed: {err}"))?;
    print!("{report}");
    Ok(())
}

fn native_surface_encoder_test(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut args = parse_native_encoder_args(argv)?;
    if args.out == "/sdcard/arda-native-encoder-test.h264" {
        args.out = String::from("/sdcard/arda-native-surface-encoder-test.h264");
    }
    let report = native::surface_encoder_test(
        &args.out,
        args.width,
        args.height,
        args.bit_rate,
        args.fps,
        args.seconds,
    )
    .map_err(|err| format!("native surface encoder test failed: {err}"))?;
    print!("{report}");
    Ok(())
}

fn native_display_capture_test(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut args = parse_native_encoder_args(argv)?;
    if args.out == "/sdcard/arda-native-encoder-test.h264" {
        args.out = String::from("/sdcard/arda-native-display-capture-test.h264");
    }
    let output_is_stdout = args.out == "-";
    let report = native::display_capture_test(
        &args.out,
        args.width,
        args.height,
        args.bit_rate,
        args.fps,
        args.seconds,
    )
    .map_err(|err| format!("native display capture test failed: {err}"))?;
    if !output_is_stdout {
        print!("{report}");
    }
    Ok(())
}

fn native_capture(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_native_capture_args(argv)?;
    let output_is_stdout = args.out == "-";
    let report = native::display_capture_test(
        &args.out,
        args.width,
        args.height,
        args.bit_rate,
        args.fps,
        args.seconds,
    )
    .map_err(|err| format!("native capture failed: {err}"))?;
    if !output_is_stdout {
        print!("{report}");
    }
    Ok(())
}

fn parse_native_encoder_args(argv: Vec<String>) -> Result<NativeEncoderArgs, Box<dyn std::error::Error>> {
    let mut width = 320i32;
    let mut height = 240i32;
    let mut bit_rate = 1_000_000i32;
    let mut fps = 15i32;
    let mut seconds = 3i32;
    let mut out = String::from("/sdcard/arda-native-encoder-test.h264");

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--size" => {
                i += 1;
                let value = argv.get(i).ok_or("--size requires value")?;
                let (w, h) = parse_size_i32(value)?;
                width = w;
                height = h;
            }
            "--bit-rate" | "--bitrate" => {
                i += 1;
                bit_rate = argv.get(i).ok_or("--bit-rate requires value")?.parse::<i32>()?;
            }
            "--fps" => {
                i += 1;
                fps = argv.get(i).ok_or("--fps requires value")?.parse::<i32>()?;
            }
            "--seconds" | "--time-limit" => {
                i += 1;
                seconds = argv.get(i).ok_or("--seconds requires value")?.parse::<i32>()?;
            }
            "--out" | "-o" => {
                i += 1;
                out = argv.get(i).ok_or("--out requires value")?.clone();
            }
            other => return Err(format!("unknown native-encoder-test arg: {other}").into()),
        }
        i += 1;
    }

    if width <= 0 || height <= 0 {
        return Err("--size must be positive WIDTHxHEIGHT".into());
    }
    if bit_rate <= 0 {
        return Err("--bit-rate must be positive".into());
    }
    if fps <= 0 || fps > 60 {
        return Err("--fps must be 1..60".into());
    }
    if seconds <= 0 || seconds > 1800 {
        return Err("--seconds must be 1..1800".into());
    }

    Ok(NativeEncoderArgs {
        width,
        height,
        bit_rate,
        fps,
        seconds,
        out,
    })
}

fn parse_native_capture_args(argv: Vec<String>) -> Result<NativeEncoderArgs, Box<dyn std::error::Error>> {
    let mut width = 1280i32;
    let mut height = 720i32;
    let mut bit_rate = 6_000_000i32;
    let mut fps = 30i32;
    let mut seconds = 0i32;
    let mut out = String::from("-");

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--size" => {
                i += 1;
                let value = argv.get(i).ok_or("--size requires value")?;
                let (w, h) = parse_size_i32(value)?;
                width = w;
                height = h;
            }
            "--bit-rate" | "--bitrate" => {
                i += 1;
                bit_rate = argv.get(i).ok_or("--bit-rate requires value")?.parse::<i32>()?;
            }
            "--fps" => {
                i += 1;
                fps = argv.get(i).ok_or("--fps requires value")?.parse::<i32>()?;
            }
            "--seconds" | "--time-limit" => {
                i += 1;
                seconds = argv.get(i).ok_or("--seconds requires value")?.parse::<i32>()?;
            }
            "--out" | "-o" => {
                i += 1;
                out = argv.get(i).ok_or("--out requires value")?.clone();
            }
            other => return Err(format!("unknown native-capture arg: {other}").into()),
        }
        i += 1;
    }

    if width <= 0 || height <= 0 {
        return Err("--size must be positive WIDTHxHEIGHT".into());
    }
    if bit_rate <= 0 {
        return Err("--bit-rate must be positive".into());
    }
    if fps <= 0 || fps > 60 {
        return Err("--fps must be 1..60".into());
    }
    if seconds < 0 || seconds > 24 * 60 * 60 {
        return Err("--seconds must be 0..86400; 0 means run until killed".into());
    }

    Ok(NativeEncoderArgs {
        width,
        height,
        bit_rate,
        fps,
        seconds,
        out,
    })
}

fn parse_size_i32(s: &str) -> Result<(i32, i32), Box<dyn std::error::Error>> {
    let Some((w, h)) = s.split_once('x') else {
        return Err("size must look like WIDTHxHEIGHT".into());
    };
    Ok((w.parse::<i32>()?, h.parse::<i32>()?))
}

fn input_test(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let config = parse_input_test_args(argv)?;
    let report = input::run_input_test(config)?;
    print!("{report}");
    Ok(())
}

fn parse_input_test_args(argv: Vec<String>) -> Result<input::InputTestConfig, Box<dyn std::error::Error>> {
    let mut width = 1280i32;
    let mut height = 720i32;
    let mut duration_override: Option<u64> = None;
    let mut steps = 20u32;

    let mut i = 0usize;
    while i < argv.len() {
        match argv[i].as_str() {
            "--size" => {
                i += 1;
                let value = argv.get(i).ok_or("--size requires value")?;
                let parsed = parse_size_i32(value)?;
                width = parsed.0;
                height = parsed.1;
                i += 1;
            }
            "--duration-ms" => {
                i += 1;
                duration_override = Some(argv.get(i).ok_or("--duration-ms requires value")?.parse::<u64>()?);
                i += 1;
            }
            "--steps" => {
                i += 1;
                steps = argv.get(i).ok_or("--steps requires value")?.parse::<u32>()?;
                i += 1;
            }
            "tap" | "swipe" => break,
            other => return Err(format!("unknown input-test arg before action: {other}").into()),
        }
    }

    let action_name = argv.get(i).map(String::as_str).ok_or("input-test requires action: tap or swipe")?;

    let action = match action_name {
        "tap" => {
            let x = argv.get(i + 1).ok_or("tap requires X")?.parse::<i32>()?;
            let y = argv.get(i + 2).ok_or("tap requires Y")?.parse::<i32>()?;
            let mut j = i + 3;
            while j < argv.len() {
                match argv[j].as_str() {
                    "--duration-ms" => {
                        j += 1;
                        duration_override = Some(argv.get(j).ok_or("--duration-ms requires value")?.parse::<u64>()?);
                    }
                    other => return Err(format!("unknown tap arg after coordinates: {other}").into()),
                }
                j += 1;
            }
            input::InputAction::Tap { x, y, duration_ms: duration_override.unwrap_or(80) }
        }
        "swipe" => {
            let x1 = argv.get(i + 1).ok_or("swipe requires X1")?.parse::<i32>()?;
            let y1 = argv.get(i + 2).ok_or("swipe requires Y1")?.parse::<i32>()?;
            let x2 = argv.get(i + 3).ok_or("swipe requires X2")?.parse::<i32>()?;
            let y2 = argv.get(i + 4).ok_or("swipe requires Y2")?.parse::<i32>()?;
            let mut j = i + 5;
            while j < argv.len() {
                match argv[j].as_str() {
                    "--duration-ms" => {
                        j += 1;
                        duration_override = Some(argv.get(j).ok_or("--duration-ms requires value")?.parse::<u64>()?);
                    }
                    "--steps" => {
                        j += 1;
                        steps = argv.get(j).ok_or("--steps requires value")?.parse::<u32>()?;
                    }
                    other => return Err(format!("unknown swipe arg after coordinates: {other}").into()),
                }
                j += 1;
            }
            input::InputAction::Swipe {
                x1,
                y1,
                x2,
                y2,
                duration_ms: duration_override.unwrap_or(300),
                steps,
            }
        }
        other => return Err(format!("unknown input-test action: {other}").into()),
    };

    if width <= 0 || height <= 0 {
        return Err("--size must be positive WIDTHxHEIGHT".into());
    }
    let duration_ms = match action {
        input::InputAction::Tap { duration_ms, .. } => duration_ms,
        input::InputAction::Swipe { duration_ms, .. } => duration_ms,
    };
    if duration_ms > 30_000 {
        return Err("--duration-ms must be <= 30000".into());
    }
    if steps == 0 || steps > 240 {
        return Err("--steps must be 1..240".into());
    }

    Ok(input::InputTestConfig { width, height, action })
}


#[derive(Debug)]
struct AapTcpProbeArgs {
    addr: String,
    dump: Option<PathBuf>,
    timeout_ms: u64,
    max_bytes: usize,
}

fn aap_tcp_probe(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_aap_tcp_probe_args(argv)?;
    aap::tcp_probe(aap::TcpProbeConfig {
        addr: args.addr,
        dump_path: args.dump,
        timeout_ms: args.timeout_ms,
        max_bytes: args.max_bytes,
    })?;
    Ok(())
}

fn parse_aap_tcp_probe_args(argv: Vec<String>) -> Result<AapTcpProbeArgs, Box<dyn std::error::Error>> {
    let mut addr = String::from("0.0.0.0:5277");
    let mut dump = Some(PathBuf::from("/data/local/tmp/arda-aap-tcp.log"));
    let mut timeout_ms = 60_000u64;
    let mut max_bytes = 1024usize * 1024usize;

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--addr" | "--listen" => {
                i += 1;
                addr = argv.get(i).ok_or("--addr requires value")?.clone();
            }
            "--dump" => {
                i += 1;
                let value = argv.get(i).ok_or("--dump requires value")?;
                dump = if value == "-" || value == "none" {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "--timeout-ms" => {
                i += 1;
                timeout_ms = argv.get(i).ok_or("--timeout-ms requires value")?.parse::<u64>()?;
            }
            "--max-bytes" => {
                i += 1;
                max_bytes = argv.get(i).ok_or("--max-bytes requires value")?.parse::<usize>()?;
            }
            other => return Err(format!("unknown aap-tcp-probe arg: {other}").into()),
        }
        i += 1;
    }

    if !addr.contains(':') {
        return Err("--addr must look like HOST:PORT".into());
    }
    if timeout_ms == 0 {
        return Err("--timeout-ms must be positive".into());
    }
    if max_bytes == 0 {
        return Err("--max-bytes must be positive".into());
    }

    Ok(AapTcpProbeArgs {
        addr,
        dump,
        timeout_ms,
        max_bytes,
    })
}


#[derive(Debug)]
struct AapFrameProbeArgs {
    addr: String,
    dump: Option<PathBuf>,
    timeout_ms: u64,
    max_bytes: usize,
    max_frames: usize,
}

fn aap_frame_probe(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_aap_frame_probe_args(argv)?;
    aap::frame_probe(aap::FrameProbeConfig {
        addr: args.addr,
        dump_path: args.dump,
        timeout_ms: args.timeout_ms,
        max_bytes: args.max_bytes,
        max_frames: args.max_frames,
    })?;
    Ok(())
}

fn parse_aap_frame_probe_args(argv: Vec<String>) -> Result<AapFrameProbeArgs, Box<dyn std::error::Error>> {
    let mut addr = String::from("0.0.0.0:5277");
    let mut dump = Some(PathBuf::from("/data/local/tmp/arda-aap-frames.log"));
    let mut timeout_ms = 60_000u64;
    let mut max_bytes = 4usize * 1024usize * 1024usize;
    let mut max_frames = 256usize;

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--addr" | "--listen" => {
                i += 1;
                addr = argv.get(i).ok_or("--addr requires value")?.clone();
            }
            "--dump" => {
                i += 1;
                let value = argv.get(i).ok_or("--dump requires value")?;
                dump = if value == "-" || value == "none" {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "--timeout-ms" => {
                i += 1;
                timeout_ms = argv.get(i).ok_or("--timeout-ms requires value")?.parse::<u64>()?;
            }
            "--max-bytes" => {
                i += 1;
                max_bytes = argv.get(i).ok_or("--max-bytes requires value")?.parse::<usize>()?;
            }
            "--max-frames" => {
                i += 1;
                max_frames = argv.get(i).ok_or("--max-frames requires value")?.parse::<usize>()?;
            }
            other => return Err(format!("unknown aap-frame-probe arg: {other}").into()),
        }
        i += 1;
    }

    if !addr.contains(':') {
        return Err("--addr must look like HOST:PORT".into());
    }
    if timeout_ms == 0 {
        return Err("--timeout-ms must be positive".into());
    }
    if max_bytes == 0 {
        return Err("--max-bytes must be positive".into());
    }
    if max_frames == 0 {
        return Err("--max-frames must be positive".into());
    }

    Ok(AapFrameProbeArgs {
        addr,
        dump,
        timeout_ms,
        max_bytes,
        max_frames,
    })
}


#[derive(Debug)]
struct AapVersionSessionArgs {
    addr: String,
    dump: Option<PathBuf>,
    timeout_ms: u64,
    max_bytes: usize,
    response_major: u16,
    response_minor: u16,
    response_status: u16,
}

fn aap_version_session(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_aap_version_session_args(argv)?;
    aap::version_session(aap::VersionSessionConfig {
        addr: args.addr,
        dump_path: args.dump,
        timeout_ms: args.timeout_ms,
        max_bytes: args.max_bytes,
        response_major: args.response_major,
        response_minor: args.response_minor,
        response_status: args.response_status,
    })?;
    Ok(())
}

fn parse_aap_version_session_args(argv: Vec<String>) -> Result<AapVersionSessionArgs, Box<dyn std::error::Error>> {
    let mut addr = String::from("0.0.0.0:5277");
    let mut dump = Some(PathBuf::from("/data/local/tmp/arda-aap-version.log"));
    let mut timeout_ms = 60_000u64;
    let mut max_bytes = 1024usize * 1024usize;
    let mut response_major = 1u16;
    let mut response_minor = 7u16;
    let mut response_status = 0u16;

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--addr" | "--listen" => {
                i += 1;
                addr = argv.get(i).ok_or("--addr requires value")?.clone();
            }
            "--dump" => {
                i += 1;
                let value = argv.get(i).ok_or("--dump requires value")?;
                dump = if value == "-" || value == "none" {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "--timeout-ms" => {
                i += 1;
                timeout_ms = argv.get(i).ok_or("--timeout-ms requires value")?.parse::<u64>()?;
            }
            "--max-bytes" => {
                i += 1;
                max_bytes = argv.get(i).ok_or("--max-bytes requires value")?.parse::<usize>()?;
            }
            "--response" | "--version" => {
                i += 1;
                let value = argv.get(i).ok_or("--response requires value like 1.7")?;
                let Some((major, minor)) = value.split_once('.') else {
                    return Err("--response must look like MAJOR.MINOR".into());
                };
                response_major = major.parse::<u16>()?;
                response_minor = minor.parse::<u16>()?;
            }
            "--status" => {
                i += 1;
                let value = argv.get(i).ok_or("--status requires value")?;
                response_status = if let Some(hex) = value.strip_prefix("0x") {
                    u16::from_str_radix(hex, 16)?
                } else {
                    value.parse::<u16>()?
                };
            }
            other => return Err(format!("unknown aap-version-session arg: {other}").into()),
        }
        i += 1;
    }

    if !addr.contains(':') {
        return Err("--addr must look like HOST:PORT".into());
    }
    if timeout_ms == 0 {
        return Err("--timeout-ms must be positive".into());
    }
    if max_bytes == 0 {
        return Err("--max-bytes must be positive".into());
    }

    Ok(AapVersionSessionArgs {
        addr,
        dump,
        timeout_ms,
        max_bytes,
        response_major,
        response_minor,
        response_status,
    })
}


#[derive(Debug)]
struct AapSslFrameProbeArgs {
    addr: String,
    dump: Option<PathBuf>,
    timeout_ms: u64,
    max_bytes: usize,
    max_frames: usize,
    response_major: u16,
    response_minor: u16,
    response_status: u16,
}

fn aap_ssl_frame_probe(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_aap_ssl_frame_probe_args(argv)?;
    aap::ssl_frame_probe(aap::SslFrameProbeConfig {
        addr: args.addr,
        dump_path: args.dump,
        timeout_ms: args.timeout_ms,
        max_bytes: args.max_bytes,
        max_frames: args.max_frames,
        response_major: args.response_major,
        response_minor: args.response_minor,
        response_status: args.response_status,
    })?;
    Ok(())
}

fn parse_aap_ssl_frame_probe_args(argv: Vec<String>) -> Result<AapSslFrameProbeArgs, Box<dyn std::error::Error>> {
    let mut addr = String::from("0.0.0.0:5277");
    let mut dump = Some(PathBuf::from("/data/local/tmp/arda-aap-ssl-frame.log"));
    let mut timeout_ms = 60_000u64;
    let mut max_bytes = 4usize * 1024usize * 1024usize;
    let mut max_frames = 8usize;
    let mut response_major = 1u16;
    let mut response_minor = 7u16;
    let mut response_status = 0u16;

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--addr" | "--listen" => {
                i += 1;
                addr = argv.get(i).ok_or("--addr requires value")?.clone();
            }
            "--dump" => {
                i += 1;
                let value = argv.get(i).ok_or("--dump requires value")?;
                dump = if value == "-" || value == "none" {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "--timeout-ms" => {
                i += 1;
                timeout_ms = argv.get(i).ok_or("--timeout-ms requires value")?.parse::<u64>()?;
            }
            "--max-bytes" => {
                i += 1;
                max_bytes = argv.get(i).ok_or("--max-bytes requires value")?.parse::<usize>()?;
            }
            "--max-frames" => {
                i += 1;
                max_frames = argv.get(i).ok_or("--max-frames requires value")?.parse::<usize>()?;
            }
            "--response" | "--version" => {
                i += 1;
                let value = argv.get(i).ok_or("--response requires value like 1.7")?;
                let Some((major, minor)) = value.split_once('.') else {
                    return Err("--response must look like MAJOR.MINOR".into());
                };
                response_major = major.parse::<u16>()?;
                response_minor = minor.parse::<u16>()?;
            }
            "--status" => {
                i += 1;
                let value = argv.get(i).ok_or("--status requires value")?;
                response_status = if let Some(hex) = value.strip_prefix("0x") {
                    u16::from_str_radix(hex, 16)?
                } else {
                    value.parse::<u16>()?
                };
            }
            other => return Err(format!("unknown aap-ssl-frame-probe arg: {other}").into()),
        }
        i += 1;
    }

    if !addr.contains(':') {
        return Err("--addr must look like HOST:PORT".into());
    }
    if timeout_ms == 0 {
        return Err("--timeout-ms must be positive".into());
    }
    if max_bytes == 0 {
        return Err("--max-bytes must be positive".into());
    }
    if max_frames == 0 {
        return Err("--max-frames must be positive".into());
    }

    Ok(AapSslFrameProbeArgs {
        addr,
        dump,
        timeout_ms,
        max_bytes,
        max_frames,
        response_major,
        response_minor,
        response_status,
    })
}


#[derive(Debug)]
struct AapTlsHandshakeProbeArgs {
    addr: String,
    dump: Option<PathBuf>,
    timeout_ms: u64,
    max_bytes: usize,
    max_frames: usize,
    response_major: u16,
    response_minor: u16,
    response_status: u16,
    keys: PathBuf,
    video_file: Option<PathBuf>,
    video_frame_limit: usize,
    video_native: bool,
    video_auto_config: bool,
    video_config_policy: String,
    video_apply_wm_config: bool,
    video_wm_size_mode: String,
    video_wm_density_mode: String,
    video_wm_settle_ms: u64,
    video_start_timing: bool,
    video_first_frame_timeout_ms: u64,
    video_first_frame_max_restarts: u32,
    video_native_prewarm_ms: u64,
    video_first_frame_kick: bool,
    video_first_frame_kick_delay_ms: u64,
    video_first_frame_kick_mode: String,
    video_first_frame_kick_x: i32,
    video_first_frame_kick_y: i32,
    video_first_frame_kick_keycode: u32,
    video_display_auto: bool,
    video_wm_rotation: String,
    video_wm_force_geometry: bool,
    video_wm_size_orientation: String,
    video_width: i32,
    video_height: i32,
    video_bit_rate: i32,
    video_fps: i32,
    video_density: u32,
    input_open: bool,
    input_key_binding: bool,
    input_inject: bool,
    input_inject_mode: String,
    input_app_move_min_ms: u64,
    input_key_inject: bool,
    audio_open: bool,
    audio_stream_filter: String,
    audio_dump_dir: Option<PathBuf>,
    audio_ack: bool,
    audio_test_tone: bool,
    audio_tone_hz: f32,
    audio_tone_chunk_ms: u64,
    audio_tone_volume: f32,
    audio_capture: bool,
    audio_capture_source: String,
    audio_capture_chunk_ms: u64,
    audio_capture_gain: f32,
    audio_capture_as: String,
    audio_capture_package: String,
    mic_open: bool,
    mic_send_request_open: bool,
    mic_dump_path: Option<PathBuf>,
    mic_ack: bool,
    mic_ack_mode: String,
    mic_max_unacked: u32,
    sensor_open: bool,
    sensor_request_all: bool,
    sensor_max_type: i32,
    sensor_period_ms: i64,
    sensor_jsonl_path: Option<PathBuf>,
    sensor_forward_os: bool,
    sensor_forward_vhal: bool,
    sensor_location_provider: String,
    sensor_location_min_ms: u64,
    sensor_vhal_min_ms: u64,
    quiet_ping_logs: bool,
    log_level: String,
}

fn aap_tls_handshake_probe(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_aap_tls_handshake_probe_args(argv)?;
    aap::tls_handshake_probe(aap::TlsHandshakeProbeConfig {
        addr: args.addr,
        dump_path: args.dump,
        timeout_ms: args.timeout_ms,
        max_bytes: args.max_bytes,
        max_frames: args.max_frames,
        response_major: args.response_major,
        response_minor: args.response_minor,
        response_status: args.response_status,
        keys_path: args.keys,
        video_file_path: args.video_file,
        video_frame_limit: args.video_frame_limit,
        video_native: args.video_native,
        video_auto_config: args.video_auto_config,
        video_config_policy: args.video_config_policy,
        video_apply_wm_config: args.video_apply_wm_config,
        video_wm_size_mode: args.video_wm_size_mode,
        video_wm_density_mode: args.video_wm_density_mode,
        video_wm_settle_ms: args.video_wm_settle_ms,
        video_start_timing: args.video_start_timing,
        video_first_frame_timeout_ms: args.video_first_frame_timeout_ms,
        video_first_frame_max_restarts: args.video_first_frame_max_restarts,
        video_native_prewarm_ms: args.video_native_prewarm_ms,
        video_first_frame_kick: args.video_first_frame_kick,
        video_first_frame_kick_delay_ms: args.video_first_frame_kick_delay_ms,
        video_first_frame_kick_mode: args.video_first_frame_kick_mode.clone(),
        video_first_frame_kick_x: args.video_first_frame_kick_x,
        video_first_frame_kick_y: args.video_first_frame_kick_y,
        video_first_frame_kick_keycode: args.video_first_frame_kick_keycode,
        video_display_auto: args.video_display_auto,
        video_wm_rotation: args.video_wm_rotation,
        video_wm_force_geometry: args.video_wm_force_geometry,
        video_wm_size_orientation: args.video_wm_size_orientation,
        video_width: args.video_width,
        video_height: args.video_height,
        video_bit_rate: args.video_bit_rate,
        video_fps: args.video_fps,
        video_density: args.video_density,
        input_open: args.input_open,
        input_key_binding: args.input_key_binding,
        input_inject: args.input_inject,
        input_inject_mode: args.input_inject_mode,
        input_app_move_min_ms: args.input_app_move_min_ms,
        input_key_inject: args.input_key_inject,
        audio_open: args.audio_open,
        audio_stream_filter: args.audio_stream_filter.clone(),
        audio_dump_dir: args.audio_dump_dir.clone(),
        audio_ack: args.audio_ack,
        audio_test_tone: args.audio_test_tone,
        audio_tone_hz: args.audio_tone_hz,
        audio_tone_chunk_ms: args.audio_tone_chunk_ms,
        audio_tone_volume: args.audio_tone_volume,
        audio_capture: args.audio_capture,
        audio_capture_source: args.audio_capture_source.clone(),
        audio_capture_chunk_ms: args.audio_capture_chunk_ms,
        audio_capture_gain: args.audio_capture_gain,
        audio_capture_as: args.audio_capture_as.clone(),
        audio_capture_package: args.audio_capture_package.clone(),
        mic_open: args.mic_open,
        mic_send_request_open: args.mic_send_request_open,
        mic_dump_path: args.mic_dump_path.clone(),
        mic_ack: args.mic_ack,
        mic_ack_mode: args.mic_ack_mode.clone(),
        mic_max_unacked: args.mic_max_unacked,
        sensor_open: args.sensor_open,
        sensor_request_all: args.sensor_request_all,
        sensor_max_type: args.sensor_max_type,
        sensor_period_ms: args.sensor_period_ms,
        sensor_jsonl_path: args.sensor_jsonl_path.clone(),
        sensor_forward_os: args.sensor_forward_os,
        sensor_forward_vhal: args.sensor_forward_vhal,
        sensor_location_provider: args.sensor_location_provider.clone(),
        sensor_location_min_ms: args.sensor_location_min_ms,
        sensor_vhal_min_ms: args.sensor_vhal_min_ms,
        quiet_ping_logs: args.quiet_ping_logs,
        log_level: args.log_level.clone(),
    })?;
    Ok(())
}

fn aap_proto_smoke() -> Result<(), Box<dyn std::error::Error>> {
    aap::proto_smoke()?;
    Ok(())
}

fn aap_fragment_smoke() -> Result<(), Box<dyn std::error::Error>> {
    aap::fragment_smoke()?;
    Ok(())
}

fn aap_tls_plain_probe(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut args = parse_aap_tls_handshake_probe_args(argv)?;
    if args.dump == Some(PathBuf::from("/data/local/tmp/arda-aap-tls-handshake.log")) {
        args.dump = Some(PathBuf::from("/data/local/tmp/arda-aap-tls-plain.log"));
    }
    if args.max_frames == 64 {
        args.max_frames = 128;
    }
    aap::tls_plain_probe(aap::TlsHandshakeProbeConfig {
        addr: args.addr,
        dump_path: args.dump,
        timeout_ms: args.timeout_ms,
        max_bytes: args.max_bytes,
        max_frames: args.max_frames,
        response_major: args.response_major,
        response_minor: args.response_minor,
        response_status: args.response_status,
        keys_path: args.keys,
        video_file_path: args.video_file,
        video_frame_limit: args.video_frame_limit,
        video_native: args.video_native,
        video_auto_config: args.video_auto_config,
        video_config_policy: args.video_config_policy,
        video_apply_wm_config: args.video_apply_wm_config,
        video_wm_size_mode: args.video_wm_size_mode,
        video_wm_density_mode: args.video_wm_density_mode,
        video_wm_settle_ms: args.video_wm_settle_ms,
        video_start_timing: args.video_start_timing,
        video_first_frame_timeout_ms: args.video_first_frame_timeout_ms,
        video_first_frame_max_restarts: args.video_first_frame_max_restarts,
        video_native_prewarm_ms: args.video_native_prewarm_ms,
        video_first_frame_kick: args.video_first_frame_kick,
        video_first_frame_kick_delay_ms: args.video_first_frame_kick_delay_ms,
        video_first_frame_kick_mode: args.video_first_frame_kick_mode.clone(),
        video_first_frame_kick_x: args.video_first_frame_kick_x,
        video_first_frame_kick_y: args.video_first_frame_kick_y,
        video_first_frame_kick_keycode: args.video_first_frame_kick_keycode,
        video_display_auto: args.video_display_auto,
        video_wm_rotation: args.video_wm_rotation,
        video_wm_force_geometry: args.video_wm_force_geometry,
        video_wm_size_orientation: args.video_wm_size_orientation,
        video_width: args.video_width,
        video_height: args.video_height,
        video_bit_rate: args.video_bit_rate,
        video_fps: args.video_fps,
        video_density: args.video_density,
        input_open: args.input_open,
        input_key_binding: args.input_key_binding,
        input_inject: args.input_inject,
        input_inject_mode: args.input_inject_mode,
        input_app_move_min_ms: args.input_app_move_min_ms,
        input_key_inject: args.input_key_inject,
        audio_open: args.audio_open,
        audio_stream_filter: args.audio_stream_filter.clone(),
        audio_dump_dir: args.audio_dump_dir.clone(),
        audio_ack: args.audio_ack,
        audio_test_tone: args.audio_test_tone,
        audio_tone_hz: args.audio_tone_hz,
        audio_tone_chunk_ms: args.audio_tone_chunk_ms,
        audio_tone_volume: args.audio_tone_volume,
        audio_capture: args.audio_capture,
        audio_capture_source: args.audio_capture_source.clone(),
        audio_capture_chunk_ms: args.audio_capture_chunk_ms,
        audio_capture_gain: args.audio_capture_gain,
        audio_capture_as: args.audio_capture_as.clone(),
        audio_capture_package: args.audio_capture_package.clone(),
        mic_open: args.mic_open,
        mic_send_request_open: args.mic_send_request_open,
        mic_dump_path: args.mic_dump_path.clone(),
        mic_ack: args.mic_ack,
        mic_ack_mode: args.mic_ack_mode.clone(),
        mic_max_unacked: args.mic_max_unacked,
        sensor_open: args.sensor_open,
        sensor_request_all: args.sensor_request_all,
        sensor_max_type: args.sensor_max_type,
        sensor_period_ms: args.sensor_period_ms,
        sensor_jsonl_path: args.sensor_jsonl_path.clone(),
        sensor_forward_os: args.sensor_forward_os,
        sensor_forward_vhal: args.sensor_forward_vhal,
        sensor_location_provider: args.sensor_location_provider.clone(),
        sensor_location_min_ms: args.sensor_location_min_ms,
        sensor_vhal_min_ms: args.sensor_vhal_min_ms,
        quiet_ping_logs: args.quiet_ping_logs,
        log_level: args.log_level.clone(),
    })?;
    Ok(())
}


fn aap_service_discovery_probe(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let mut args = parse_aap_tls_handshake_probe_args(argv)?;
    if args.dump == Some(PathBuf::from("/data/local/tmp/arda-aap-tls-handshake.log")) {
        args.dump = Some(PathBuf::from("/data/local/tmp/arda-aap-service-discovery.log"));
    }
    if args.max_frames == 64 {
        args.max_frames = 256;
    }
    aap::tls_service_discovery_probe(aap::TlsHandshakeProbeConfig {
        addr: args.addr,
        dump_path: args.dump,
        timeout_ms: args.timeout_ms,
        max_bytes: args.max_bytes,
        max_frames: args.max_frames,
        response_major: args.response_major,
        response_minor: args.response_minor,
        response_status: args.response_status,
        keys_path: args.keys,
        video_file_path: args.video_file,
        video_frame_limit: args.video_frame_limit,
        video_native: args.video_native,
        video_auto_config: args.video_auto_config,
        video_config_policy: args.video_config_policy,
        video_apply_wm_config: args.video_apply_wm_config,
        video_wm_size_mode: args.video_wm_size_mode,
        video_wm_density_mode: args.video_wm_density_mode,
        video_wm_settle_ms: args.video_wm_settle_ms,
        video_start_timing: args.video_start_timing,
        video_first_frame_timeout_ms: args.video_first_frame_timeout_ms,
        video_first_frame_max_restarts: args.video_first_frame_max_restarts,
        video_native_prewarm_ms: args.video_native_prewarm_ms,
        video_first_frame_kick: args.video_first_frame_kick,
        video_first_frame_kick_delay_ms: args.video_first_frame_kick_delay_ms,
        video_first_frame_kick_mode: args.video_first_frame_kick_mode.clone(),
        video_first_frame_kick_x: args.video_first_frame_kick_x,
        video_first_frame_kick_y: args.video_first_frame_kick_y,
        video_first_frame_kick_keycode: args.video_first_frame_kick_keycode,
        video_display_auto: args.video_display_auto,
        video_wm_rotation: args.video_wm_rotation,
        video_wm_force_geometry: args.video_wm_force_geometry,
        video_wm_size_orientation: args.video_wm_size_orientation,
        video_width: args.video_width,
        video_height: args.video_height,
        video_bit_rate: args.video_bit_rate,
        video_fps: args.video_fps,
        video_density: args.video_density,
        input_open: args.input_open,
        input_key_binding: args.input_key_binding,
        input_inject: args.input_inject,
        input_inject_mode: args.input_inject_mode,
        input_app_move_min_ms: args.input_app_move_min_ms,
        input_key_inject: args.input_key_inject,
        audio_open: args.audio_open,
        audio_stream_filter: args.audio_stream_filter.clone(),
        audio_dump_dir: args.audio_dump_dir.clone(),
        audio_ack: args.audio_ack,
        audio_test_tone: args.audio_test_tone,
        audio_tone_hz: args.audio_tone_hz,
        audio_tone_chunk_ms: args.audio_tone_chunk_ms,
        audio_tone_volume: args.audio_tone_volume,
        audio_capture: args.audio_capture,
        audio_capture_source: args.audio_capture_source.clone(),
        audio_capture_chunk_ms: args.audio_capture_chunk_ms,
        audio_capture_gain: args.audio_capture_gain,
        audio_capture_as: args.audio_capture_as.clone(),
        audio_capture_package: args.audio_capture_package.clone(),
        mic_open: args.mic_open,
        mic_send_request_open: args.mic_send_request_open,
        mic_dump_path: args.mic_dump_path.clone(),
        mic_ack: args.mic_ack,
        mic_ack_mode: args.mic_ack_mode.clone(),
        mic_max_unacked: args.mic_max_unacked,
        sensor_open: args.sensor_open,
        sensor_request_all: args.sensor_request_all,
        sensor_max_type: args.sensor_max_type,
        sensor_period_ms: args.sensor_period_ms,
        sensor_jsonl_path: args.sensor_jsonl_path.clone(),
        sensor_forward_os: args.sensor_forward_os,
        sensor_forward_vhal: args.sensor_forward_vhal,
        sensor_location_provider: args.sensor_location_provider.clone(),
        sensor_location_min_ms: args.sensor_location_min_ms,
        sensor_vhal_min_ms: args.sensor_vhal_min_ms,
        quiet_ping_logs: args.quiet_ping_logs,
        log_level: args.log_level.clone(),
    })?;
    Ok(())
}

fn normalize_wm_rotation_arg(value: &str) -> Result<String, Box<dyn std::error::Error>> {
    let lower = value.to_ascii_lowercase();
    let normalized = match lower.as_str() {
        "none" | "off" | "disable" | "disabled" => "none",
        "auto" | "detect" => "auto",
        "0" | "natural" => "0",
        "1" | "90" | "cw" | "clockwise" => "90",
        "2" | "180" | "flip" => "180",
        "3" | "270" | "ccw" | "counterclockwise" | "counter-clockwise" => "270",
        other => return Err(format!("unsupported rotation value {other}; use none|auto|0|90|180|270").into()),
    };
    Ok(normalized.to_string())
}


fn parse_android_keycode(value: &str) -> Result<u32, Box<dyn std::error::Error>> {
    let v = value.trim();
    if let Ok(n) = v.parse::<u32>() {
        return Ok(n);
    }
    let name = v.trim_start_matches("KEYCODE_").to_ascii_uppercase();
    let code = match name.as_str() {
        "HOME" => 3,
        "BACK" => 4,
        "CALL" => 5,
        "ENDCALL" => 6,
        "SEARCH" => 84,
        "WAKEUP" | "WAKE_UP" => 224,
        "SOFT_SLEEP" => 276,
        "MEDIA_PLAY_PAUSE" => 85,
        "MEDIA_NEXT" => 87,
        "MEDIA_PREVIOUS" => 88,
        "MEDIA_PLAY" => 126,
        "MEDIA_PAUSE" => 127,
        other => return Err(format!("unsupported keycode name: {other}; use a number or HOME/BACK/SEARCH/WAKEUP/MEDIA_PLAY_PAUSE").into()),
    };
    Ok(code)
}

fn parse_aap_tls_handshake_probe_args(argv: Vec<String>) -> Result<AapTlsHandshakeProbeArgs, Box<dyn std::error::Error>> {
    let mut addr = String::from("0.0.0.0:5277");
    let mut dump = Some(PathBuf::from("/data/local/tmp/arda-aap-tls-handshake.log"));
    let mut timeout_ms = 60_000u64;
    let mut max_bytes = 4usize * 1024usize * 1024usize;
    let mut max_frames = 64usize;
    let mut response_major = 1u16;
    let mut response_minor = 7u16;
    let mut response_status = 0u16;
    let mut keys = PathBuf::from("/data/local/tmp/arda-certs");
    let mut video_file: Option<PathBuf> = None;
    let mut video_frame_limit = 30usize;
    let mut video_native = false;
    let mut video_auto_config = true;
    let mut video_config_policy = String::from("highest");
    let mut video_apply_wm_config = false;
    let mut video_wm_size_mode = String::from("stream");
    let mut video_wm_density_mode = String::from("density");
    let mut video_wm_settle_ms = 1200u64;
    let mut video_start_timing = false;
    let mut video_first_frame_timeout_ms = 0u64;
    let mut video_first_frame_max_restarts = 0u32;
    let mut video_native_prewarm_ms = 0u64;
    let mut video_first_frame_kick = false;
    let mut video_first_frame_kick_delay_ms = 120u64;
    let mut video_first_frame_kick_mode = String::from("tap");
    let mut video_first_frame_kick_x = 1i32;
    let mut video_first_frame_kick_y = 1i32;
    let mut video_first_frame_kick_keycode = 3u32;
    let mut video_display_auto = false;
    let mut video_wm_rotation = String::from("none");
    let mut video_wm_force_geometry = false;
    let mut video_wm_size_orientation = String::from("logical");
    let mut video_width = 800i32;
    let mut video_height = 480i32;
    let mut video_bit_rate = 2_500_000i32;
    let mut video_fps = 30i32;
    let mut video_density = 160u32;
    let mut input_open = false;
    let mut input_key_binding = true;
    let mut input_inject = false;
    let mut input_inject_mode = String::from("uinput");
    let mut input_app_move_min_ms = 16u64;
    let mut input_key_inject = false;
    let mut audio_open = false;
    let mut audio_stream_filter = String::from("all");
    let mut audio_dump_dir: Option<PathBuf> = None;
    let mut audio_ack = true;
    let mut audio_test_tone = false;
    let mut audio_tone_hz: f32 = 440.0;
    let mut audio_tone_chunk_ms: u64 = 20;
    let mut audio_tone_volume: f32 = 0.20;
    let mut audio_capture = false;
    let mut audio_capture_source = String::from("remote_submix");
    let mut audio_capture_chunk_ms: u64 = 20;
    let mut audio_capture_gain: f32 = 1.0;
    let mut audio_capture_as = String::from("shell");
    let mut audio_capture_package = String::from("com.android.shell");
    let mut mic_open = false;
    let mut mic_send_request_open = false;
    let mut mic_dump_path: Option<PathBuf> = None;
    let mut mic_ack = true;
    let mut mic_ack_mode = String::from("delta");
    let mut mic_max_unacked: u32 = 8;
    let mut sensor_open = false;
    let mut sensor_request_all = true;
    let mut sensor_max_type: i32 = 21;
    let mut sensor_period_ms: i64 = -2;
    let mut sensor_jsonl_path: Option<PathBuf> = None;
    let mut sensor_forward_os = false;
    let mut sensor_forward_vhal = false;
    let mut sensor_location_provider = String::from("arda_gps");
    let mut sensor_location_min_ms: u64 = 1000;
    let mut sensor_vhal_min_ms: u64 = 500;
    let mut quiet_ping_logs = true;
    let mut log_level = String::from("normal");

    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--addr" | "--listen" => {
                i += 1;
                addr = argv.get(i).ok_or("--addr requires value")?.clone();
            }
            "--dump" => {
                i += 1;
                let value = argv.get(i).ok_or("--dump requires value")?;
                dump = if value == "-" || value == "none" {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "--keys" | "--keys-path" => {
                i += 1;
                keys = PathBuf::from(argv.get(i).ok_or("--keys requires directory path")?);
            }
            "--video-file" | "--h264-file" => {
                i += 1;
                video_file = Some(PathBuf::from(argv.get(i).ok_or("--video-file requires path")?));
            }
            "--video-frames" | "--video-frame-limit" => {
                i += 1;
                video_frame_limit = argv.get(i).ok_or("--video-frames requires value")?.parse::<usize>()?;
            }
            "--video-native" | "--native-video" | "--live-video" => {
                video_native = true;
            }
            "--video-auto-config" => {
                video_auto_config = true;
            }
            "--no-video-auto-config" | "--video-manual-config" => {
                video_auto_config = false;
            }
            "--video-config-policy" | "--video-policy" => {
                i += 1;
                let value = argv.get(i).ok_or("--video-config-policy requires highest|balanced|lowest|index:N")?;
                video_config_policy = aap::proto::normalize_video_config_policy(value);
            }
            "--video-config-index" | "--video-index" => {
                i += 1;
                let value = argv.get(i).ok_or("--video-config-index requires number")?.parse::<usize>()?;
                video_config_policy = format!("index:{value}");
            }
            "--apply-wm-config" => {
                video_apply_wm_config = true;
            }
            "--no-apply-wm-config" => {
                video_apply_wm_config = false;
            }
            "--wm-size-mode" => {
                i += 1;
                video_wm_size_mode = argv.get(i).ok_or("--wm-size-mode requires stream|content|safe")?.to_ascii_lowercase();
            }
            "--wm-density-mode" => {
                i += 1;
                video_wm_density_mode = argv.get(i).ok_or("--wm-density-mode requires density|real")?.to_ascii_lowercase();
            }
            "--wm-settle-ms" | "--video-wm-settle-ms" => {
                i += 1;
                video_wm_settle_ms = argv.get(i).ok_or("--wm-settle-ms requires milliseconds")?.parse::<u64>()?;
            }
            "--fast-start" | "--video-fast-start" => {
                video_wm_settle_ms = 300;
                video_start_timing = true;
                if video_first_frame_timeout_ms == 0 {
                    video_first_frame_timeout_ms = 8000;
                }
                if video_first_frame_max_restarts == 0 {
                    video_first_frame_max_restarts = 1;
                }
            }
            "--no-fast-start" | "--safe-start" => {
                video_wm_settle_ms = 1200;
                video_first_frame_timeout_ms = 0;
                video_first_frame_max_restarts = 0;
                video_native_prewarm_ms = 0;
            }
            "--video-start-timing" | "--startup-timing" | "--latency-log" | "--latency-logs" => {
                video_start_timing = true;
            }
            "--no-video-start-timing" | "--no-startup-timing" | "--no-latency-log" => {
                video_start_timing = false;
            }
            "--video-first-frame-timeout-ms" | "--first-frame-timeout-ms" => {
                i += 1;
                video_first_frame_timeout_ms = argv.get(i).ok_or("--video-first-frame-timeout-ms requires milliseconds")?.parse::<u64>()?;
            }
            "--video-first-frame-restarts" | "--first-frame-restarts" | "--video-capture-restarts" => {
                i += 1;
                video_first_frame_max_restarts = argv.get(i).ok_or("--video-first-frame-restarts requires count")?.parse::<u32>()?;
            }
            "--no-video-first-frame-restart" | "--no-first-frame-restart" => {
                video_first_frame_timeout_ms = 0;
                video_first_frame_max_restarts = 0;
            }
            "--video-native-prewarm-ms" | "--native-prewarm-ms" | "--video-prewarm-ms" => {
                i += 1;
                video_native_prewarm_ms = argv.get(i).ok_or("--video-native-prewarm-ms requires milliseconds")?.parse::<u64>()?;
            }
            "--video-native-prewarm" | "--native-prewarm" | "--prewarm-video" => {
                if video_native_prewarm_ms == 0 {
                    video_native_prewarm_ms = 700;
                }
            }
            "--no-video-native-prewarm" | "--no-native-prewarm" | "--no-prewarm-video" => {
                video_native_prewarm_ms = 0;
            }
            "--video-first-frame-kick" | "--first-frame-kick" | "--video-kick-input" | "--kick-video-input" | "--video-kick-tap" => {
                video_first_frame_kick = true;
                video_first_frame_kick_mode = String::from("tap");
            }
            "--video-first-frame-kick-center" | "--first-frame-kick-center" | "--video-kick-center" | "--video-kick-tap-center" => {
                video_first_frame_kick = true;
                video_first_frame_kick_mode = String::from("center");
            }
            "--video-first-frame-kick-center-cancel" | "--first-frame-kick-center-cancel" | "--video-kick-center-cancel" | "--video-kick-no-click-center" => {
                video_first_frame_kick = true;
                video_first_frame_kick_mode = String::from("center-cancel");
            }
            "--video-first-frame-kick-center-move-cancel" | "--first-frame-kick-center-move-cancel" | "--video-kick-center-move-cancel" => {
                video_first_frame_kick = true;
                video_first_frame_kick_mode = String::from("center-move-cancel");
            }
            "--video-first-frame-kick-wm-nudge" | "--first-frame-kick-wm-nudge" | "--video-kick-wm-nudge" | "--video-dirty-wm" | "--video-first-frame-dirty" => {
                video_first_frame_kick = true;
                video_first_frame_kick_mode = String::from("wm-nudge");
            }
            "--video-first-frame-kick-cancel" | "--first-frame-kick-cancel" | "--video-kick-cancel" | "--video-kick-no-click" => {
                video_first_frame_kick = true;
                video_first_frame_kick_mode = String::from("touch-cancel");
            }
            "--video-first-frame-kick-wakeup" | "--first-frame-kick-wakeup" | "--video-kick-wakeup" => {
                video_first_frame_kick = true;
                video_first_frame_kick_mode = String::from("wakeup");
            }
            "--no-video-first-frame-kick" | "--no-first-frame-kick" | "--no-video-kick-input" | "--no-kick-video-input" => {
                video_first_frame_kick = false;
            }
            "--video-first-frame-kick-mode" | "--first-frame-kick-mode" | "--video-kick-mode" => {
                i += 1;
                video_first_frame_kick = true;
                let value = argv.get(i).ok_or("--video-first-frame-kick-mode requires tap|center|center-cancel|center-move-cancel|wm-nudge|touch-cancel|wakeup|home|keyevent")?.to_ascii_lowercase();
                video_first_frame_kick_mode = match value.as_str() {
                    "tap" | "touch" | "edge" | "corner" => String::from("tap"),
                    "center" | "centre" | "tap-center" | "center-tap" | "middle" => String::from("center"),
                    "center-cancel" | "centre-cancel" | "cancel-center" | "middle-cancel" | "center-no-click" | "no-click-center" => String::from("center-cancel"),
                    "center-move-cancel" | "centre-move-cancel" | "move-cancel-center" | "center-drag-cancel" => String::from("center-move-cancel"),
                    "wm-nudge" | "dirty" | "wm-dirty" | "window-nudge" | "relayout" | "wm" => String::from("wm-nudge"),
                    "cancel" | "touch-cancel" | "down-cancel" | "motionevent-cancel" | "no-click" => String::from("touch-cancel"),
                    "wakeup" | "wake" | "key-wakeup" => String::from("wakeup"),
                    "home" => String::from("home"),
                    "key" | "keyevent" => String::from("keyevent"),
                    other => return Err(format!("--video-first-frame-kick-mode unsupported value: {other}; use tap, center, center-cancel, center-move-cancel, wm-nudge, touch-cancel, wakeup, home, or keyevent").into()),
                };
            }
            "--video-first-frame-kick-delay-ms" | "--first-frame-kick-delay-ms" | "--video-kick-delay-ms" => {
                i += 1;
                video_first_frame_kick = true;
                video_first_frame_kick_delay_ms = argv.get(i).ok_or("--video-first-frame-kick-delay-ms requires milliseconds")?.parse::<u64>()?;
            }
            "--video-first-frame-kick-tap" | "--first-frame-kick-tap" | "--video-kick-tap-at" => {
                i += 1;
                video_first_frame_kick = true;
                video_first_frame_kick_mode = String::from("tap");
                let value = argv.get(i).ok_or("--video-first-frame-kick-tap requires X,Y or XxY")?;
                let normalized = value.replace('x', ",").replace('X', ",").replace(':', ",");
                let mut parts = normalized.split(',');
                let x = parts.next().ok_or("--video-first-frame-kick-tap requires X,Y")?.trim().parse::<i32>()?;
                let y = parts.next().ok_or("--video-first-frame-kick-tap requires X,Y")?.trim().parse::<i32>()?;
                video_first_frame_kick_x = x;
                video_first_frame_kick_y = y;
            }
            "--video-first-frame-kick-keycode" | "--first-frame-kick-keycode" | "--video-kick-keycode" => {
                i += 1;
                video_first_frame_kick = true;
                video_first_frame_kick_mode = String::from("keyevent");
                video_first_frame_kick_keycode = parse_android_keycode(argv.get(i).ok_or("--video-first-frame-kick-keycode requires Android keycode/name")?)?;
            }
            "--display-auto" | "--auto-display" | "--auto-geometry" | "--auto-wm" => {
                video_display_auto = true;
                video_apply_wm_config = true;
            }
            "--no-display-auto" | "--no-auto-display" | "--no-auto-geometry" | "--no-auto-wm" => {
                video_display_auto = false;
            }
            "--dhu-priority" | "--force-dhu-geometry" | "--force-wm-stream" | "--wm-force-geometry" => {
                video_apply_wm_config = true;
                video_wm_force_geometry = true;
            }
            "--no-dhu-priority" | "--no-force-dhu-geometry" | "--no-wm-force-geometry" => {
                video_wm_force_geometry = false;
            }
            "--wm-size-orientation" | "--display-size-orientation" => {
                i += 1;
                let value = argv.get(i).ok_or("--wm-size-orientation requires logical|natural|auto")?.to_ascii_lowercase();
                match value.as_str() {
                    "logical" | "stream" | "display" => video_wm_size_orientation = String::from("logical"),
                    "natural" | "pre-rotation" | "native" => video_wm_size_orientation = String::from("natural"),
                    "auto" => video_wm_size_orientation = String::from("auto"),
                    other => return Err(format!("--wm-size-orientation unsupported value: {other}; use logical, natural, or auto").into()),
                }
            }
            "--wm-natural-size" | "--natural-wm-size" => {
                video_wm_size_orientation = String::from("natural");
                video_apply_wm_config = true;
            }
            "--wm-logical-size" | "--logical-wm-size" => {
                video_wm_size_orientation = String::from("logical");
                video_apply_wm_config = true;
            }
            "--wm-rotation" | "--display-rotation" | "--video-wm-rotation" => {
                i += 1;
                video_wm_rotation = normalize_wm_rotation_arg(argv.get(i).ok_or("--wm-rotation requires none|auto|0|90|180|270")?)?;
                if video_wm_rotation != "none" {
                    video_apply_wm_config = true;
                    video_display_auto = true;
                }
            }
            "--auto-rotation" | "--wm-auto-rotation" => {
                video_wm_rotation = String::from("auto");
                video_apply_wm_config = true;
                video_display_auto = true;
            }
            "--no-wm-rotation" | "--no-display-rotation" => {
                video_wm_rotation = String::from("none");
            }
            "--display-profile" | "--wm-profile" => {
                i += 1;
                let profile = argv.get(i).ok_or("--display-profile requires waveshare-79")?.to_ascii_lowercase();
                match profile.as_str() {
                    "waveshare-79" | "waveshare_79" | "waveshare7.9" | "waveshare-7.9" | "7.9" => {
                        video_apply_wm_config = true;
                        video_display_auto = true;
                        video_wm_force_geometry = true;
                        video_wm_size_orientation = String::from("natural");
                        // The 7.9 HDMI bar panel commonly reports a portrait-oriented scanout;
                        // in our AAOS/DHU setup the visible content was 90° clockwise, so lock
                        // Android's logical display 270° to compensate. Override with
                        // --wm-rotation 90 if your unit is mounted the other way.
                        video_wm_rotation = String::from("270");
                    }
                    other => return Err(format!("--display-profile unsupported value: {other}; use waveshare-79").into()),
                }
            }
            "--waveshare-79" | "--waveshare79" | "--waveshare-7.9" => {
                video_apply_wm_config = true;
                video_display_auto = true;
                video_wm_force_geometry = true;
                video_wm_size_orientation = String::from("natural");
                video_wm_rotation = String::from("270");
            }
            "--video-size" | "--live-size" => {
                i += 1;
                let value = argv.get(i).ok_or("--video-size requires WIDTHxHEIGHT")?;
                let (w, h) = parse_size_i32(value)?;
                video_width = w;
                video_height = h;
            }
            "--video-bit-rate" | "--video-bitrate" | "--live-bit-rate" => {
                i += 1;
                video_bit_rate = argv.get(i).ok_or("--video-bit-rate requires value")?.parse::<i32>()?;
            }
            "--video-fps" | "--live-fps" => {
                i += 1;
                video_fps = argv.get(i).ok_or("--video-fps requires value")?.parse::<i32>()?;
            }
            "--video-density" | "--wm-density" | "--live-density" => {
                i += 1;
                video_density = argv.get(i).ok_or("--video-density requires DPI value")?.parse::<u32>()?;
            }
            "--input-open" | "--open-input" | "--input" => {
                input_open = true;
            }
            "--no-input-open" | "--no-open-input" | "--no-input" => {
                input_open = false;
            }
            "--input-key-binding" | "--input-bind-keys" => {
                input_key_binding = true;
            }
            "--no-input-key-binding" | "--no-input-bind-keys" => {
                input_key_binding = false;
            }
            "--input-inject" | "--input-touch-inject" | "--inject-input" | "--inject-touch" => {
                input_open = true;
                input_inject = true;
            }
            "--input-inject-mode" | "--input-touch-mode" | "--touch-inject-mode" => {
                i += 1;
                let value = argv.get(i).ok_or("--input-inject-mode requires uinput|app|shell")?.to_ascii_lowercase();
                input_inject_mode = match value.as_str() {
                    "app" | "framework" | "java" | "app-process" | "app_process" | "inputd" | "uinput" | "shell" | "motionevent" | "shell-motionevent" | "input" => value,
                    other => return Err(format!("--input-inject-mode unsupported value: {other}; use uinput|app|shell").into()),
                };
            }
            "--input-app-move-min-ms" | "--app-input-move-min-ms" | "--input-move-min-ms" => {
                i += 1;
                input_app_move_min_ms = argv.get(i).ok_or("--input-app-move-min-ms requires milliseconds")?.parse::<u64>()?;
            }
            "--no-input-inject" | "--no-input-touch-inject" | "--no-inject-input" | "--no-inject-touch" => {
                input_inject = false;
            }
            "--input-key-inject" | "--inject-input-keys" | "--inject-keys" => {
                input_open = true;
                input_key_binding = true;
                input_key_inject = true;
            }
            "--no-input-key-inject" | "--no-inject-input-keys" | "--no-inject-keys" => {
                input_key_inject = false;
            }
            "--audio-open" | "--open-audio" | "--media-audio" | "--audio-output" => {
                audio_open = true;
            }
            "--no-audio-open" | "--no-open-audio" | "--no-media-audio" | "--no-audio-output" => {
                audio_open = false;
            }
            "--audio-stream" | "--audio-stream-filter" => {
                i += 1;
                audio_stream_filter = argv.get(i).ok_or("--audio-stream requires all|media|guidance|system|telephony")?.to_ascii_lowercase();
            }
            "--audio-dump" | "--dump-audio" => {
                audio_open = true;
                audio_dump_dir = Some(PathBuf::from("/data/local/tmp/arda-audio"));
            }
            "--audio-dump-dir" | "--dump-audio-dir" => {
                i += 1;
                audio_open = true;
                audio_dump_dir = Some(PathBuf::from(argv.get(i).ok_or("--audio-dump-dir requires path")?));
            }
            "--no-audio-dump" | "--no-dump-audio" => {
                audio_dump_dir = None;
            }
            "--audio-ack" | "--ack-audio" => {
                audio_ack = true;
            }
            "--no-audio-ack" | "--no-ack-audio" => {
                audio_ack = false;
            }
            "--audio-test-tone" | "--audio-tone" | "--test-audio" | "--send-audio-tone" => {
                audio_open = true;
                audio_test_tone = true;
            }
            "--no-audio-test-tone" | "--no-audio-tone" | "--no-test-audio" => {
                audio_test_tone = false;
            }
            "--audio-tone-hz" | "--audio-test-tone-hz" | "--tone-hz" => {
                i += 1;
                audio_open = true;
                audio_test_tone = true;
                audio_tone_hz = argv.get(i).ok_or("--audio-tone-hz requires frequency")?.parse::<f32>()?;
            }
            "--audio-tone-chunk-ms" | "--audio-chunk-ms" => {
                i += 1;
                audio_tone_chunk_ms = argv.get(i).ok_or("--audio-tone-chunk-ms requires milliseconds")?.parse::<u64>()?;
            }
            "--audio-tone-volume" | "--audio-volume" => {
                i += 1;
                audio_tone_volume = argv.get(i).ok_or("--audio-tone-volume requires 0.0..1.0")?.parse::<f32>()?;
            }
            "--audio-capture" | "--capture-audio" | "--audio-remote-submix" | "--capture-aaos-audio" => {
                audio_open = true;
                audio_capture = true;
            }
            "--no-audio-capture" | "--no-capture-audio" => {
                audio_capture = false;
            }
            "--audio-capture-source" | "--audio-source" | "--capture-audio-source" => {
                i += 1;
                audio_open = true;
                audio_capture = true;
                audio_capture_source = argv.get(i).ok_or("--audio-capture-source requires remote_submix|mic|default|voice_communication|unprocessed")?.to_ascii_lowercase();
            }
            "--audio-capture-chunk-ms" | "--capture-audio-chunk-ms" => {
                i += 1;
                audio_capture_chunk_ms = argv.get(i).ok_or("--audio-capture-chunk-ms requires milliseconds")?.parse::<u64>()?;
            }
            "--audio-capture-gain" | "--capture-audio-gain" => {
                i += 1;
                audio_capture_gain = argv.get(i).ok_or("--audio-capture-gain requires 0.0..4.0")?.parse::<f32>()?;
            }
            "--audio-capture-as" | "--capture-audio-as" | "--audio-capture-user" | "--audio-capture-uid" => {
                i += 1;
                audio_open = true;
                audio_capture = true;
                let value = argv.get(i).ok_or("--audio-capture-as requires shell|root|system|media|audioserver")?.to_ascii_lowercase();
                audio_capture_as = match value.as_str() {
                    "shell" | "root" | "system" | "media" | "audioserver" | "audio-server" | "0" | "1000" | "1013" | "1041" => value,
                    other => return Err(format!("--audio-capture-as unsupported value: {other}; use shell|root|system|media|audioserver").into()),
                };
            }
            "--audio-capture-package" | "--capture-audio-package" | "--audio-package" => {
                i += 1;
                audio_open = true;
                audio_capture = true;
                audio_capture_package = argv.get(i).ok_or("--audio-capture-package requires package name")?.to_string();
            }
            "--mic-open" | "--microphone-open" | "--audio-source-open" | "--hu-mic-open" => {
                mic_open = true;
            }
            "--no-mic-open" | "--no-microphone-open" | "--no-audio-source-open" => {
                mic_open = false;
            }
            "--mic-send-request" | "--mic-send-request-open" | "--mic-request-open" | "--microphone-send-request" => {
                mic_open = true;
                mic_send_request_open = true;
            }
            "--no-mic-send-request" | "--no-mic-send-request-open" | "--no-mic-request-open" => {
                mic_send_request_open = false;
            }
            "--mic-dump" | "--mic-dump-path" | "--mic-dump-file" | "--microphone-dump" => {
                i += 1;
                mic_open = true;
                mic_dump_path = Some(PathBuf::from(argv.get(i).ok_or("--mic-dump requires PATH")?));
            }
            "--no-mic-dump" | "--no-microphone-dump" => {
                mic_dump_path = None;
            }
            "--no-mic-ack" | "--no-microphone-ack" => {
                mic_ack = false;
                mic_ack_mode = String::from("off");
            }
            "--mic-ack" | "--microphone-ack" => {
                mic_ack = true;
                if mic_ack_mode == "off" {
                    mic_ack_mode = String::from("delta");
                }
            }
            "--mic-ack-mode" | "--microphone-ack-mode" => {
                i += 1;
                mic_open = true;
                let value = argv.get(i).ok_or("--mic-ack-mode requires delta|one|window|cumulative|bytes|off")?.to_ascii_lowercase();
                mic_ack_mode = match value.as_str() {
                    "delta" | "one" | "per-packet" | "each" => String::from("delta"),
                    "window" | "chunk" | "count" => String::from("window"),
                    "cumulative" | "total" | "counter" => String::from("cumulative"),
                    "bytes" | "byte" => String::from("bytes"),
                    "off" | "none" | "no" => String::from("off"),
                    other => return Err(format!("--mic-ack-mode unsupported value: {other}; use delta|window|cumulative|bytes|off").into()),
                };
                mic_ack = mic_ack_mode != "off";
            }
            "--mic-ack-delta" | "--mic-ack-one" | "--mic-ack-each" => {
                mic_ack = true;
                mic_ack_mode = String::from("delta");
            }
            "--mic-ack-window" => {
                mic_ack = true;
                mic_ack_mode = String::from("window");
            }
            "--mic-ack-cumulative" | "--mic-ack-counter" => {
                mic_ack = true;
                mic_ack_mode = String::from("cumulative");
            }
            "--mic-ack-bytes" => {
                mic_ack = true;
                mic_ack_mode = String::from("bytes");
            }
            "--mic-max-unacked" | "--microphone-max-unacked" => {
                i += 1;
                mic_open = true;
                mic_max_unacked = argv.get(i).ok_or("--mic-max-unacked requires 1..4096")?.parse::<u32>()?;
            }
            "--sensor-open" | "--sensors-open" | "--open-sensors" => {
                sensor_open = true;
            }
            "--no-sensor-open" | "--no-sensors-open" => {
                sensor_open = false;
            }
            "--sensor-request-all" | "--sensors-request-all" | "--sensor-all" => {
                sensor_open = true;
                sensor_request_all = true;
            }
            "--no-sensor-request-all" | "--no-sensor-all" => {
                sensor_request_all = false;
            }
            "--sensor-max-type" | "--sensors-max-type" => {
                i += 1;
                sensor_open = true;
                sensor_max_type = argv.get(i).ok_or("--sensor-max-type requires 1..21")?.parse::<i32>()?;
            }
            "--sensor-period-ms" | "--sensors-period-ms" => {
                i += 1;
                sensor_open = true;
                sensor_period_ms = argv.get(i).ok_or("--sensor-period-ms requires milliseconds, 0 event-driven, -1 stop, -2 auto")?.parse::<i64>()?;
            }
            "--sensor-jsonl" | "--sensor-jsonl-path" | "--sensors-jsonl" => {
                i += 1;
                sensor_open = true;
                sensor_jsonl_path = Some(PathBuf::from(argv.get(i).ok_or("--sensor-jsonl requires PATH")?));
            }
            "--no-sensor-jsonl" => {
                sensor_jsonl_path = None;
            }
            "--sensor-forward-os" | "--sensors-forward-os" | "--sensor-os" => {
                sensor_open = true;
                sensor_forward_os = true;
                sensor_forward_vhal = true;
            }
            "--sensor-forward-location" | "--sensor-location-os" => {
                sensor_open = true;
                sensor_forward_os = true;
            }
            "--no-sensor-forward-os" | "--no-sensor-os" => {
                sensor_forward_os = false;
                sensor_forward_vhal = false;
            }
            "--no-sensor-location-os" | "--no-sensor-forward-location" => {
                sensor_forward_os = false;
            }
            "--sensor-forward-vhal" | "--sensor-vhal-os" | "--sensor-car-service" => {
                sensor_open = true;
                sensor_forward_vhal = true;
            }
            "--no-sensor-forward-vhal" | "--no-sensor-vhal-os" | "--no-sensor-car-service" => {
                sensor_forward_vhal = false;
            }
            "--sensor-location-provider" | "--sensor-provider" => {
                i += 1;
                sensor_open = true;
                sensor_location_provider = argv.get(i).ok_or("--sensor-location-provider requires provider name")?.to_string();
            }
            "--sensor-location-min-ms" | "--sensor-os-min-ms" => {
                i += 1;
                sensor_open = true;
                sensor_location_min_ms = argv.get(i).ok_or("--sensor-location-min-ms requires milliseconds")?.parse::<u64>()?;
            }
            "--sensor-vhal-min-ms" | "--sensor-car-service-min-ms" => {
                i += 1;
                sensor_open = true;
                sensor_forward_vhal = true;
                sensor_vhal_min_ms = argv.get(i).ok_or("--sensor-vhal-min-ms requires milliseconds")?.parse::<u64>()?;
            }
            "--quiet-ping" | "--quiet-ping-logs" | "--no-ping-log" | "--no-ping-logs" => {
                quiet_ping_logs = true;
            }
            "--log-ping" | "--log-ping-logs" | "--no-quiet-ping" => {
                quiet_ping_logs = false;
            }
            "--log-level" | "--session-log-level" => {
                i += 1;
                let value = argv.get(i).ok_or("--log-level requires quiet|summary|normal|verbose")?.to_ascii_lowercase();
                log_level = match value.as_str() {
                    "quiet" | "off" | "none" | "fast" => String::from("quiet"),
                    "summary" | "minimal" | "min" => String::from("summary"),
                    "normal" => String::from("normal"),
                    "verbose" | "trace" | "debug" | "wire" => String::from("verbose"),
                    other => return Err(format!("--log-level unsupported value: {other}; use quiet|summary|normal|verbose").into()),
                };
            }
            "--quiet-logs" | "--fast-logs" | "--no-wire-logs" => {
                log_level = String::from("quiet");
            }
            "--summary-logs" | "--minimal-logs" => {
                log_level = String::from("summary");
            }
            "--verbose-logs" | "--wire-logs" | "--trace-logs" => {
                log_level = String::from("verbose");
            }
            "--no-dump" | "--dump-none" => {
                dump = None;
            }
            "--timeout-ms" => {
                i += 1;
                timeout_ms = argv.get(i).ok_or("--timeout-ms requires value")?.parse::<u64>()?;
            }
            "--max-bytes" => {
                i += 1;
                max_bytes = argv.get(i).ok_or("--max-bytes requires value")?.parse::<usize>()?;
            }
            "--max-frames" => {
                i += 1;
                max_frames = argv.get(i).ok_or("--max-frames requires value")?.parse::<usize>()?;
            }
            "--response" | "--version" => {
                i += 1;
                let value = argv.get(i).ok_or("--response requires value like 1.7")?;
                let Some((major, minor)) = value.split_once('.') else {
                    return Err("--response must look like MAJOR.MINOR".into());
                };
                response_major = major.parse::<u16>()?;
                response_minor = minor.parse::<u16>()?;
            }
            "--status" => {
                i += 1;
                let value = argv.get(i).ok_or("--status requires value")?;
                response_status = if let Some(hex) = value.strip_prefix("0x") {
                    u16::from_str_radix(hex, 16)?
                } else {
                    value.parse::<u16>()?
                };
            }
            other => return Err(format!("unknown aap-tls-handshake-probe arg: {other}").into()),
        }
        i += 1;
    }

    if !addr.contains(':') {
        return Err("--addr must look like HOST:PORT".into());
    }
    if timeout_ms == 0 {
        return Err("--timeout-ms must be positive".into());
    }
    if max_bytes == 0 {
        return Err("--max-bytes must be positive".into());
    }
    if max_frames == 0 {
        return Err("--max-frames must be positive".into());
    }
    if video_native && video_file.is_some() {
        return Err("use either --video-native or --video-file, not both".into());
    }
    if video_width <= 0 || video_height <= 0 {
        return Err("--video-size must be positive WIDTHxHEIGHT".into());
    }
    if video_bit_rate <= 0 {
        return Err("--video-bit-rate must be positive".into());
    }
    if video_fps <= 0 || video_fps > 60 {
        return Err("--video-fps must be 1..60".into());
    }
    if video_density == 0 {
        return Err("--video-density must be positive".into());
    }
    if !matches!(video_wm_size_mode.as_str(), "stream" | "content" | "content_area" | "safe" | "safe_area") {
        return Err("--wm-size-mode must be stream, content, or safe".into());
    }
    if !matches!(video_wm_density_mode.as_str(), "density" | "real" | "real_density") {
        return Err("--wm-density-mode must be density or real".into());
    }
    if video_wm_settle_ms > 10_000 {
        return Err("--wm-settle-ms must be 0..10000".into());
    }
    if video_first_frame_timeout_ms > 60_000 {
        return Err("--video-first-frame-timeout-ms must be 0..60000".into());
    }
    if video_first_frame_max_restarts > 20 {
        return Err("--video-first-frame-restarts must be 0..20".into());
    }
    if video_native_prewarm_ms > 10_000 {
        return Err("--video-native-prewarm-ms must be 0..10000".into());
    }
    if video_first_frame_kick_delay_ms > 10_000 {
        return Err("--video-first-frame-kick-delay-ms must be 0..10000".into());
    }
    if video_first_frame_kick_keycode > 1000 {
        return Err("--video-first-frame-kick-keycode must be 0..1000".into());
    }
    if !matches!(video_wm_rotation.as_str(), "none" | "auto" | "0" | "90" | "180" | "270") {
        return Err("--wm-rotation must be none, auto, 0, 90, 180, or 270".into());
    }
    if !matches!(video_wm_size_orientation.as_str(), "logical" | "natural" | "auto") {
        return Err("--wm-size-orientation must be logical, natural, or auto".into());
    }
    if audio_stream_filter.trim().is_empty() {
        return Err("--audio-stream cannot be empty".into());
    }
    if !(1.0..=24_000.0).contains(&audio_tone_hz) {
        return Err("--audio-tone-hz must be 1..24000".into());
    }
    if audio_tone_chunk_ms == 0 || audio_tone_chunk_ms > 200 {
        return Err("--audio-tone-chunk-ms must be 1..200".into());
    }
    if !(0.0..=1.0).contains(&audio_tone_volume) {
        return Err("--audio-tone-volume must be 0.0..1.0".into());
    }
    if audio_capture_chunk_ms == 0 || audio_capture_chunk_ms > 200 {
        return Err("--audio-capture-chunk-ms must be 1..200".into());
    }
    if !(0.0..=4.0).contains(&audio_capture_gain) {
        return Err("--audio-capture-gain must be 0.0..4.0".into());
    }
    if mic_max_unacked == 0 || mic_max_unacked > 4096 {
        return Err("--mic-max-unacked must be 1..4096".into());
    }
    match mic_ack_mode.as_str() {
        "delta" | "window" | "cumulative" | "bytes" | "off" => {}
        other => return Err(format!("--mic-ack-mode unsupported value: {other}; use delta|window|cumulative|bytes|off").into()),
    }
    if mic_ack_mode == "off" {
        mic_ack = false;
    }

    Ok(AapTlsHandshakeProbeArgs {
        addr,
        dump,
        timeout_ms,
        max_bytes,
        max_frames,
        response_major,
        response_minor,
        response_status,
        keys,
        video_file,
        video_frame_limit,
        video_native,
        video_auto_config,
        video_config_policy,
        video_apply_wm_config,
        video_wm_size_mode,
        video_wm_density_mode,
        video_wm_settle_ms,
        video_start_timing,
        video_first_frame_timeout_ms,
        video_first_frame_max_restarts,
        video_native_prewarm_ms,
        video_first_frame_kick,
        video_first_frame_kick_delay_ms,
        video_first_frame_kick_mode,
        video_first_frame_kick_x,
        video_first_frame_kick_y,
        video_first_frame_kick_keycode,
        video_display_auto,
        video_wm_rotation,
        video_wm_force_geometry,
        video_wm_size_orientation,
        video_width,
        video_height,
        video_bit_rate,
        video_fps,
        video_density,
        input_open,
        input_key_binding,
        input_inject,
        input_inject_mode,
        input_app_move_min_ms,
        input_key_inject,
        audio_open,
        audio_stream_filter,
        audio_dump_dir,
        audio_ack,
        audio_test_tone,
        audio_tone_hz,
        audio_tone_chunk_ms,
        audio_tone_volume,
        audio_capture,
        audio_capture_source,
        audio_capture_chunk_ms,
        audio_capture_gain,
        audio_capture_as,
        audio_capture_package,
        mic_open,
        mic_send_request_open,
        mic_dump_path,
        mic_ack,
        mic_ack_mode,
        mic_max_unacked,
        sensor_open,
        sensor_request_all,
        sensor_max_type,
        sensor_period_ms,
        sensor_jsonl_path,
        sensor_forward_os,
        sensor_forward_vhal,
        sensor_location_provider,
        sensor_location_min_ms,
        sensor_vhal_min_ms,
        quiet_ping_logs,
        log_level,
    })
}

fn nal_info(argv: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    if argv.is_empty() {
        return Err("nal-info requires FILE".into());
    }

    let file = PathBuf::from(&argv[0]);
    let mut limit = 80usize;
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--limit" => {
                i += 1;
                limit = argv.get(i).ok_or("--limit requires value")?.parse::<usize>()?;
            }
            other => return Err(format!("unknown nal-info arg: {other}").into()),
        }
        i += 1;
    }

    let mut data = Vec::new();
    File::open(&file)?.read_to_end(&mut data)?;

    let mut parser = AnnexBStreamParser::new();
    parser.push(&data);

    let mut stats = NalStats::default();
    let mut index = 0usize;
    while let Some(nal) = parser.next_nal() {
        let ty = nal.nal_type();
        stats.add(ty);
        if index < limit {
            println!(
                "#{:<4} offset={:<9} len={:<8} type={:<20} key={}",
                index,
                nal.offset(),
                nal.bytes().len(),
                format!("{:?}", ty),
                ty.is_key_video_unit()
            );
        }
        index += 1;
    }
    if let Some(nal) = parser.finish() {
        let ty = nal.nal_type();
        stats.add(ty);
        if index < limit {
            println!(
                "#{:<4} offset={:<9} len={:<8} type={:<20} key={}",
                index,
                nal.offset(),
                nal.bytes().len(),
                format!("{:?}", ty),
                ty.is_key_video_unit()
            );
        }
        index += 1;
    }

    println!("total_nals={index} stats={stats}");
    Ok(())
}
