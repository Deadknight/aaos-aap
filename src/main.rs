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

Examples on AAOS:
  /data/local/tmp/arda-senderd capture-test --size 1280x720 --seconds 10 --out /sdcard/arda.h264

Live from Mac:
  adb exec-out /data/local/tmp/arda-senderd capture-test --size 1280x720 --seconds 180 --out - \
    | mpv --demuxer=lavf --demuxer-lavf-format=h264 --profile=low-latency --untimed -
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
    if seconds <= 0 || seconds > 30 {
        return Err("--seconds must be 1..30 for native encoder smoke test".into());
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
