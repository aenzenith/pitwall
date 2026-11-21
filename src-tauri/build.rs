use std::process::Command;

fn main() {
    link_clang_runtime();
    tauri_build::build()
}

/// macOS: whisper.cpp's Metal code asks for the system's version as it runs (`@available`), and
/// clang makes that a call to `__isPlatformVersionAtLeast` wherever the deployment target is
/// below the version asked for: in a release, whose target `tauri build` sets from
/// `bundle.macOS.minimumSystemVersion`. The function is in clang's runtime library, which
/// rustc's link (`-nodefaultlibs`) leaves out, so the release's link failed; it is linked here,
/// from the toolchain that compiled whisper.cpp. A build without a target (CI, `tauri dev`) has
/// no such call and takes nothing from it.
fn link_clang_runtime() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let dir = Command::new("xcrun")
        .args(["clang", "--print-runtime-dir"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|dir| dir.trim().to_string())
        .filter(|dir| {
            std::path::Path::new(dir)
                .join("libclang_rt.osx.a")
                .is_file()
        });
    match dir {
        Some(dir) => {
            println!("cargo:rustc-link-search=native={dir}");
            println!("cargo:rustc-link-lib=static=clang_rt.osx");
        }
        None => println!("cargo:warning=clang's runtime library (libclang_rt.osx.a) wasn't found: a release build may not link"),
    }
}
