//! Kurulumu komut satırından dener: TV'yi tarar, bağlanır, gerekirse yükler, izni verir.
//! `--start` verilirse TV'de uygulamayı da açar (TV ekranı HDMI'dan uygulamaya geçer).
//! cargo run -p ceviri-tvsetup --example setup -- ANAHTAR APK SÜRÜM [IP] [--start]
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use ceviri_tvsetup::{scan, Installer};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (key, apk, version) = (&args[0], std::fs::read(&args[1]).expect("APK okunamadı"), args[2].parse().expect("sürüm"));
    let ip: Ipv4Addr = match args.get(3).filter(|a| !a.starts_with("--")) {
        Some(ip) => ip.parse().expect("IP"),
        None => {
            let started = Instant::now();
            let found = scan(Duration::from_millis(400));
            println!("tarama {} ms: {found:?}", started.elapsed().as_millis());
            *found.first().expect("ağda hata ayıklaması açık cihaz yok")
        }
    };
    let installer = Installer::new(ip, key);
    let started = Instant::now();
    let report = installer.prepare(&apk, version, |step| println!("[{:>5} ms] {step:?}", started.elapsed().as_millis()));
    println!("sonuç: {report:?}");
    if args.iter().any(|a| a == "--start") {
        println!("başlat: {:?}", installer.start(|step| println!("[{:>5} ms] {step:?}", started.elapsed().as_millis())));
    }
}
