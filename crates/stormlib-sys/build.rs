use std::{env, path::PathBuf, process::Command};

fn run(command: &mut Command) {
    let status = command
        .status()
        .expect("CMake is required to build vendored StormLib");
    assert!(status.success(), "StormLib build failed: {command:?}");
}

fn main() {
    assert_eq!(env::var("CARGO_CFG_TARGET_OS").unwrap(), "windows");
    assert_eq!(env::var("CARGO_CFG_TARGET_ARCH").unwrap(), "x86_64");
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("vendor/StormLib");
    let build = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("storm-build");
    let static_crt = env::var("CARGO_CFG_TARGET_FEATURE")
        .unwrap_or_default()
        .contains("crt-static");
    run(Command::new("cmake")
        .arg("-S")
        .arg(&source)
        .arg("-B")
        .arg(&build)
        .args([
            "-A",
            "x64",
            "-DBUILD_SHARED_LIBS=OFF",
            "-DSTORM_UNICODE=ON",
            "-DSTORM_USE_BUNDLED_LIBRARIES=ON",
            "-DSTORM_BUILD_TESTS=OFF",
            "-DSTORM_SKIP_INSTALL=ON",
            "-DCMAKE_POLICY_DEFAULT_CMP0091=NEW",
        ])
        .arg(if static_crt {
            "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded"
        } else {
            "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL"
        }));
    run(Command::new("cmake").arg("--build").arg(&build).args([
        "--config",
        "Release",
        "--parallel",
        "4",
    ]));
    cc::Build::new()
        .cpp(true)
        .file("bridge.cpp")
        .include(source.join("src"))
        .define("UNICODE", None)
        .define("_UNICODE", None)
        .define("__STORMLIB_NO_STATIC_LINK__", None)
        .static_crt(static_crt)
        .debug(false)
        .compile("storm_bridge");
    println!("cargo:rustc-link-search=native={}/Release", build.display());
    println!("cargo:rustc-link-lib=static=StormLib");
    println!("cargo:rustc-link-lib=user32");
    println!("cargo:rustc-link-lib=wininet");
    println!("cargo:rerun-if-changed=vendor/StormLib");
    println!("cargo:rerun-if-changed=bridge.cpp");
}
