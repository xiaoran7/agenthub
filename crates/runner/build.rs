use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let out_dir = env::var("OUT_DIR").unwrap();
    let ico_path = Path::new(&manifest_dir).join("resources").join("icon.ico");

    let manifest_path = Path::new(&manifest_dir).join("resources").join("app.manifest");

    println!("cargo:rerun-if-changed=resources/icon.ico");
    println!("cargo:rerun-if-changed=resources/icon.svg");
    println!("cargo:rerun-if-changed=resources/app.manifest");
    println!("cargo:rerun-if-changed=build.rs");

    let mut rc_content = String::new();
    if ico_path.exists() {
        rc_content.push_str(&format!(
            "1 ICON \"{}\"\n",
            ico_path.to_str().unwrap().replace('\\', "/")
        ));
    }
    if manifest_path.exists() {
        rc_content.push_str(&format!(
            "1 24 \"{}\"\n",
            manifest_path.to_str().unwrap().replace('\\', "/")
        ));
    }

    if rc_content.is_empty() {
        return;
    }

    let rc_path = Path::new(&out_dir).join("app_icon.rc");
    let res_path = Path::new(&out_dir).join("app_icon.res");

    if fs::write(&rc_path, rc_content).is_err() {
        return;
    }

    let compiled = try_compile_with_windres(&rc_path, &res_path)
        || try_compile_with_rc(&rc_path, &res_path);

    if compiled && res_path.exists() {
        println!("cargo:rustc-link-arg={}", res_path.display());
    }
}

fn try_compile_with_windres(rc_path: &Path, res_path: &Path) -> bool {
    let candidates = [
        PathBuf::from(r"C:\Users\tanner\AppData\Local\Programs\msys64\ucrt64\bin\windres.exe"),
        PathBuf::from("windres.exe"),
        PathBuf::from("windres"),
    ];

    for candidate in &candidates {
        let status = Command::new(candidate)
            .arg("-i")
            .arg(rc_path)
            .arg("-O")
            .arg("coff")
            .arg("-o")
            .arg(res_path)
            .status();

        if let Ok(exit) = status {
            if exit.success() {
                return true;
            }
        }
    }
    false
}

fn try_compile_with_rc(rc_path: &Path, res_path: &Path) -> bool {
    let candidates = [
        PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\rc.exe"),
        PathBuf::from("rc.exe"),
    ];

    for candidate in &candidates {
        let status = Command::new(candidate)
            .arg("/r")
            .arg(format!("/fo{}", res_path.display()))
            .arg(rc_path)
            .status();

        if let Ok(exit) = status {
            if exit.success() {
                return true;
            }
        }
    }
    false
}
