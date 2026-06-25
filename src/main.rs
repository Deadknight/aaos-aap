mod aap;
mod capture;
mod h264;
mod input;
mod native;

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

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

  aap-session-probe ... --video-native [--video-config-policy highest|balanced|lowest|index:N] [--video-size WIDTHxHEIGHT --video-fps N --video-density DPI] [--apply-wm-config --wm-size-mode stream|content|safe --wm-density-mode density|real --wm-settle-ms 1200]
      Starts native-capture as an internal child after video START and sends live H264 to the opened video channel.
      Current AAP session: service discovery, video channel open, AV setup/start, ping responses. With --video-file, sends Annex-B H264 after START.

  aap-session-probe ... --input-open [--input-key-binding|--no-input-key-binding] [--input-inject] [--input-inject-mode uinput|shell] [--input-key-inject]
      Opens the HU InputSourceService channel, logs touchscreen/touchpad/key/rotary reports, optionally sends key binding request, and can inject touchscreen/key events into Android.

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
    video_width: i32,
    video_height: i32,
    video_bit_rate: i32,
    video_fps: i32,
    video_density: u32,
    input_open: bool,
    input_key_binding: bool,
    input_inject: bool,
    input_inject_mode: String,
    input_key_inject: bool,
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
        video_width: args.video_width,
        video_height: args.video_height,
        video_bit_rate: args.video_bit_rate,
        video_fps: args.video_fps,
        video_density: args.video_density,
        input_open: args.input_open,
        input_key_binding: args.input_key_binding,
        input_inject: args.input_inject,
        input_inject_mode: args.input_inject_mode,
        input_key_inject: args.input_key_inject,
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
        video_width: args.video_width,
        video_height: args.video_height,
        video_bit_rate: args.video_bit_rate,
        video_fps: args.video_fps,
        video_density: args.video_density,
        input_open: args.input_open,
        input_key_binding: args.input_key_binding,
        input_inject: args.input_inject,
        input_inject_mode: args.input_inject_mode,
        input_key_inject: args.input_key_inject,
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
        video_width: args.video_width,
        video_height: args.video_height,
        video_bit_rate: args.video_bit_rate,
        video_fps: args.video_fps,
        video_density: args.video_density,
        input_open: args.input_open,
        input_key_binding: args.input_key_binding,
        input_inject: args.input_inject,
        input_inject_mode: args.input_inject_mode,
        input_key_inject: args.input_key_inject,
    })?;
    Ok(())
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
    let mut video_width = 800i32;
    let mut video_height = 480i32;
    let mut video_bit_rate = 2_500_000i32;
    let mut video_fps = 30i32;
    let mut video_density = 160u32;
    let mut input_open = false;
    let mut input_key_binding = true;
    let mut input_inject = false;
    let mut input_inject_mode = String::from("uinput");
    let mut input_key_inject = false;

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
                let value = argv.get(i).ok_or("--input-inject-mode requires uinput|shell")?.to_ascii_lowercase();
                input_inject_mode = match value.as_str() {
                    "uinput" | "shell" | "motionevent" | "shell-motionevent" | "input" => value,
                    other => return Err(format!("--input-inject-mode unsupported value: {other}; use uinput|shell").into()),
                };
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
        video_width,
        video_height,
        video_bit_rate,
        video_fps,
        video_density,
        input_open,
        input_key_binding,
        input_inject,
        input_inject_mode,
        input_key_inject,
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
