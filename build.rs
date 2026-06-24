use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    generate_protos();
    build_native_cpp();
}

fn generate_protos() {
    println!("cargo:rerun-if-changed=proto/protos.proto");
    println!("cargo:rerun-if-changed=proto/ev.proto");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing")).join("protos");
    std::fs::create_dir_all(&out_dir).expect("create OUT_DIR/protos");

    protobuf_codegen::Codegen::new()
        .pure()
        .out_dir(&out_dir)
        .include("proto")
        .inputs(["proto/protos.proto", "proto/ev.proto"])
        .run_from_script();
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
