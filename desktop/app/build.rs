//! Tauri'nin derleme adımı + TV uygulamasının (Android APK) gömülmesi. Kurulum sihirbazı APK'yı
//! TV'ye buradan yükler. APK önceden derlenmiş olmalı (`./gradlew assembleRelease`); yoksa boş
//! gömülür ve sihirbaz TV kurulumunun bu derlemede olmadığını söyler (ör. CI).

use std::path::PathBuf;

fn main() {
    embed_tv_apk();
    tauri_build::build()
}

fn embed_tv_apk() {
    let repo = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let apk = std::env::var_os("CEVIRI_TV_APK")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("app/build/outputs/apk/release/app-release.apk"));
    let gradle = repo.join("app/build.gradle.kts");
    println!("cargo:rerun-if-env-changed=CEVIRI_TV_APK");
    println!("cargo:rerun-if-changed={}", apk.display());
    println!("cargo:rerun-if-changed={}", gradle.display());

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("tv.apk");
    match std::fs::read(&apk) {
        Ok(bytes) => std::fs::write(&out, bytes).unwrap(),
        Err(_) => {
            println!("cargo:warning=TV APK bulunamadı ({}); TV kurulumu bu derlemede olmayacak", apk.display());
            std::fs::write(&out, []).unwrap();
        }
    }
    // Sürüm kodu Gradle dosyasından: TV'deki sürüm bundan eskiyse sihirbaz günceller.
    let version = std::fs::read_to_string(&gradle)
        .ok()
        .and_then(|text| {
            let line = text.lines().find(|l| l.trim_start().starts_with("versionCode"))?;
            line.split('=').nth(1)?.trim().parse::<u32>().ok()
        })
        .unwrap_or(0);
    println!("cargo:rustc-env=TV_APK_VERSION={version}");
}
