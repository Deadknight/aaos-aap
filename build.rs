use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    generate_protos();
    build_sensor_wasm();
    build_inputd_java();
    build_native_cpp();
}


fn build_sensor_wasm() {
    println!("cargo:rerun-if-changed=wasm/arda_vhal_default.wat");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    let wasm_out = out_dir.join("arda_vhal_default.wasm");
    let bytes = wat::parse_file("wasm/arda_vhal_default.wat")
        .expect("compile wasm/arda_vhal_default.wat");
    fs::write(&wasm_out, bytes).expect("write embedded arda_vhal_default.wasm");
}

fn generate_protos() {
    println!("cargo:rerun-if-changed=proto/protos.proto");
    println!("cargo:rerun-if-changed=proto/ev.proto");
    println!("cargo:rerun-if-changed=proto/arda_vehicle_router.proto");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing")).join("protos");
    std::fs::create_dir_all(&out_dir).expect("create OUT_DIR/protos");

    protobuf_codegen::Codegen::new()
        .pure()
        // rust-protobuf v3 intentionally does not generate field accessors by
        // default. The router client uses the accessor API consistently, so
        // request it explicitly for all generated messages.
        .customize(protobuf_codegen::Customize::default().generate_accessors(true))
        .out_dir(&out_dir)
        .include("proto")
        .inputs([
            "proto/protos.proto",
            "proto/ev.proto",
            "proto/arda_vehicle_router.proto",
        ])
        .run()
        .expect("generate protobuf Rust sources");

    verify_vehicle_router_accessors(&out_dir);
}

fn verify_vehicle_router_accessors(out_dir: &Path) {
    let generated = out_dir.join("arda_vehicle_router.rs");
    let source = fs::read_to_string(&generated)
        .unwrap_or_else(|err| panic!("cannot read generated router protobuf {}: {err}", generated.display()));

    // The router source deliberately uses rust-protobuf's generated accessor API.
    // Fail in build.rs with one actionable error instead of allowing a stale
    // no-accessor OUT_DIR snapshot to expand into hundreds of E0599 errors.
    let required = [
        "pub fn set_protocol_version",
        "pub fn has_hello",
        "pub fn set_hello",
        "pub fn source_id(&self)",
        "pub fn set_source_id",
        "pub fn property_id(&self)",
        "pub fn set_property_id",
        "pub fn has_vhal_events",
        "pub fn set_vhal_events",
    ];
    let missing = required
        .iter()
        .copied()
        .filter(|needle| !source.contains(needle))
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        panic!(
            "generated arda_vehicle_router.rs is missing required rust-protobuf accessors {:?}; remove stale arda-senderd build artifacts and rerun codegen",
            missing
        );
    }

    println!("cargo:warning=ARDA_PROTO_ACCESSORS_OK={}", generated.display());
}

fn build_inputd_java() {
    println!("cargo:rerun-if-changed=src/inputd/java/com/arda/input/InputServer.java");
    println!("cargo:rerun-if-changed=src/inputd/java/com/arda/input/AudioCaptureServer.java");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    let jar_out = out_dir.join("arda_inputd.jar");
    let target = env::var("TARGET").unwrap_or_default();

    if !target.contains("android") {
        // Host builds are not used for the app_process injector. Keep them possible.
        fs::write(&jar_out, []).expect("write host placeholder arda_inputd.jar");
        return;
    }

    let javac = env::var("JAVAC").unwrap_or_else(|_| "javac".to_string());
    let d8 = find_d8().unwrap_or_else(|| {
        panic!("Android d8 not found. Install Android build-tools or set D8=/path/to/d8")
    });
    let jar = env::var("JAR").unwrap_or_else(|_| "jar".to_string());
    let android_jar = find_android_jar().unwrap_or_else(|| {
        panic!("android.jar not found. Install an Android SDK platform or set ANDROID_JAR=/path/to/android.jar")
    });

    let java_src_dir = PathBuf::from("src/inputd/java/com/arda/input");
    let java_sources = fs::read_dir(&java_src_dir)
        .expect("read inputd java source dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|v| v.to_str()) == Some("java"))
        .collect::<Vec<_>>();
    if java_sources.is_empty() {
        panic!("no Java sources found in {}", java_src_dir.display());
    }
    let classes_dir = out_dir.join("arda_inputd_classes");
    let dex_dir = out_dir.join("arda_inputd_dex");
    let _ = fs::remove_dir_all(&classes_dir);
    let _ = fs::remove_dir_all(&dex_dir);
    fs::create_dir_all(&classes_dir).expect("create inputd classes dir");
    fs::create_dir_all(&dex_dir).expect("create inputd dex dir");

    let javac_status = Command::new(&javac)
        .args(["-source", "8", "-target", "8", "-encoding", "UTF-8"])
        .arg("-classpath")
        .arg(&android_jar)
        .arg("-d")
        .arg(&classes_dir)
        .args(&java_sources)
        .status()
        .unwrap_or_else(|err| panic!("failed to run javac {javac}: {err}"));
    if !javac_status.success() {
        panic!("arda inputd javac failed with {javac_status}");
    }

    // D8 does not accept a raw classes directory on all build-tools versions
    // (some versions fail with "Unsupported source file type"). Package the
    // compiled .class tree into a temporary jar first, then dex that jar.
    let classes_jar = out_dir.join("arda_inputd_classes.jar");
    let _ = fs::remove_file(&classes_jar);
    let classes_jar_status = Command::new(&jar)
        .arg("cf")
        .arg(&classes_jar)
        .arg("-C")
        .arg(&classes_dir)
        .arg(".")
        .status()
        .unwrap_or_else(|err| panic!("failed to run jar {jar}: {err}"));
    if !classes_jar_status.success() {
        panic!("arda inputd class jar packaging failed with {classes_jar_status}");
    }

    let d8_status = Command::new(&d8)
        .arg("--min-api")
        .arg(env::var("ANDROID_API").unwrap_or_else(|_| "23".to_string()))
        .arg("--classpath")
        .arg(&android_jar)
        .arg("--output")
        .arg(&dex_dir)
        .arg(&classes_jar)
        .status()
        .unwrap_or_else(|err| panic!("failed to run d8 {}: {err}", d8.display()));
    if !d8_status.success() {
        panic!("arda inputd d8 failed with {d8_status}");
    }

    let classes_dex = dex_dir.join("classes.dex");
    if !classes_dex.exists() {
        panic!("d8 did not produce {}", classes_dex.display());
    }
    let _ = fs::remove_file(&jar_out);
    let jar_status = Command::new(&jar)
        .arg("cf")
        .arg(&jar_out)
        .arg("-C")
        .arg(&dex_dir)
        .arg("classes.dex")
        .status()
        .unwrap_or_else(|err| panic!("failed to run jar {jar}: {err}"));
    if !jar_status.success() {
        panic!("arda inputd jar packaging failed with {jar_status}");
    }
}

fn find_android_jar() -> Option<PathBuf> {
    if let Ok(value) = env::var("ANDROID_JAR") {
        let path = PathBuf::from(value);
        if path.exists() {
            return Some(path);
        }
    }

    for env_name in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(root) = env::var(env_name) {
            let platforms = PathBuf::from(root).join("platforms");
            if let Some(path) = newest_android_jar(&platforms) {
                return Some(path);
            }
        }
    }
    None
}

fn newest_android_jar(platforms: &Path) -> Option<PathBuf> {
    let mut candidates = fs::read_dir(platforms)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path().join("android.jar");
            if !path.exists() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let api = name.strip_prefix("android-").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
            Some((api, path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(api, _)| *api);
    candidates.pop().map(|(_, path)| path)
}

fn find_d8() -> Option<PathBuf> {
    if let Ok(value) = env::var("D8") {
        let path = PathBuf::from(value);
        if path.exists() {
            return Some(path);
        }
    }

    for env_name in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(root) = env::var(env_name) {
            let build_tools = PathBuf::from(root).join("build-tools");
            if let Some(path) = newest_build_tool(&build_tools, "d8") {
                return Some(path);
            }
        }
    }

    find_on_path("d8")
}

fn newest_build_tool(build_tools: &Path, tool: &str) -> Option<PathBuf> {
    let mut candidates = fs::read_dir(build_tools)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(tool))
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop()
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path_env = env::var_os("PATH")?;
    env::split_paths(&path_env)
        .map(|dir| dir.join(name))
        .find(|path| path.exists())
}

fn build_native_cpp() {
    println!("cargo:rerun-if-changed=native/arda_native_probe.cpp");

    let target = env::var("TARGET").unwrap_or_default();
    if !target.contains("android") {
        // Host builds are not used for the AAOS target. Keep them possible.
        return;
    }

    let android_api = env::var("ANDROID_API").unwrap_or_else(|_| "23".to_string());
    let compiler = env::var(target_env_name("CXX", &target)).unwrap_or_else(|_| match target.as_str() {
        "aarch64-linux-android" => format!("aarch64-linux-android{android_api}-clang++"),
        "armv7-linux-androideabi" => format!("armv7a-linux-androideabi{android_api}-clang++"),
        other => panic!("unsupported Android target for native C++ probe: {other}"),
    });

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    let obj = out_dir.join("arda_native_probe.o");

    let status = Command::new(&compiler)
        .args([
            "-std=c++17",
            "-fPIC",
            "-fno-exceptions",
            "-fno-rtti",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror=return-type",
            "-c",
            "native/arda_native_probe.cpp",
            "-o",
        ])
        .arg(&obj)
        .status()
        .unwrap_or_else(|err| panic!("failed to run C++ compiler {compiler}: {err}"));

    if !status.success() {
        panic!("C++ native probe compile failed with {status}");
    }

    println!("cargo:rustc-link-arg={}", obj.display());
    println!("cargo:rustc-link-lib=mediandk");
    println!("cargo:rustc-link-lib=EGL");
    println!("cargo:rustc-link-lib=GLESv2");
    println!("cargo:rustc-link-lib=android");
    println!("cargo:rustc-link-lib=dl");
}

fn target_env_name(prefix: &str, target: &str) -> String {
    format!("{}_{}", prefix, target.replace('-', "_"))
}
