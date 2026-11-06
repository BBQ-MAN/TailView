use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/tailview.rc");
    println!("cargo:rerun-if-changed=assets/tailview.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    assert_eq!(env::var("CARGO_CFG_TARGET_ENV").as_deref(), Ok("msvc"),
        "Windows builds require the MSVC toolchain and Windows SDK");
    let rc = env::var_os("RC").map(PathBuf::from).unwrap_or_else(|| {
        let bin = PathBuf::from(env::var_os("ProgramFiles(x86)")
            .expect("Windows SDK requires ProgramFiles(x86)"))
            .join("Windows Kits/10/bin");
        let mut versions: Vec<_> = std::fs::read_dir(bin).expect("install Windows SDK")
            .filter_map(Result::ok).map(|e| e.path().join("x64/rc.exe"))
            .filter(|p| p.is_file()).collect();
        versions.sort();
        versions.pop().expect("Windows SDK rc.exe not found; set RC to its path")
    });
    let resource = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("tailview.res");
    let status = Command::new(rc).arg("/nologo").arg("/fo").arg(&resource)
        .arg("assets/tailview.rc").status().expect("run Windows resource compiler");
    assert!(status.success(), "failed to compile TailView icon resource");
    println!("cargo:rustc-link-arg-bin=tailview={}", resource.display());
    println!("cargo:rerun-if-env-changed=RC");
}
