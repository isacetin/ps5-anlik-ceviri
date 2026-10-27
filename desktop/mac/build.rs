//! Mac'te `mac/` Swift paketinin `CeviriBridge` statik kütüphanesini derleyip bağlar.
//! Diğer işletim sistemlerinde hiçbir şey yapmaz (crate orada boştur).

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let package = manifest.join("../../mac").canonicalize().expect("mac/ Swift paketi bulunamadı");
    for path in ["Package.swift", "Sources/PS5CeviriCore", "Sources/PS5CeviriMac", "Sources/CeviriBridge"] {
        println!("cargo:rerun-if-changed={}", package.join(path).display());
    }

    // Swift'in kendi .build klasörüne dokunmamak için ayrı bir derleme klasörü.
    let scratch = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("swift");
    let status = Command::new("swift")
        .args(["build", "-c", "release", "--product", "CeviriBridge", "--package-path"])
        .arg(&package)
        .arg("--scratch-path")
        .arg(&scratch)
        .status()
        .expect("swift çalıştırılamadı (Xcode ya da Command Line Tools gerekli)");
    assert!(status.success(), "Swift köprüsü (CeviriBridge) derlenemedi");

    println!("cargo:rustc-link-search=native={}", scratch.join("release").display());
    println!("cargo:rustc-link-lib=static=CeviriBridge");

    // Swift çalışma zamanı. Framework'ler ve swiftCore, Swift nesne dosyalarındaki autolink
    // kayıtlarıyla kendiliğinden bağlanır; bağlayıcının onları bulacağı klasörler gerekir.
    let sdk = xcrun(&["--sdk", "macosx", "--show-sdk-path"]);
    println!("cargo:rustc-link-search=native={sdk}/usr/lib/swift");
    let swiftc = xcrun(&["--find", "swiftc"]);
    let toolchain = Path::new(&swiftc).parent().and_then(Path::parent).expect("swiftc yolu");
    println!("cargo:rustc-link-search=native={}", toolchain.join("lib/swift/macosx").display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

fn xcrun(args: &[&str]) -> String {
    let output = Command::new("xcrun").args(args).output().expect("xcrun çalıştırılamadı");
    assert!(output.status.success(), "xcrun {args:?} başarısız");
    String::from_utf8(output.stdout).expect("xcrun çıktısı").trim().to_string()
}
