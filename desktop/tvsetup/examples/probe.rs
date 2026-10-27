//! Fizibilite: Rust adb istemcisiyle TV'ye bağlanıp salt okunur komutlar çalıştırır.
//! cargo run -p ceviri-tvsetup --example probe -- 192.168.1.5 ~/.android/adbkey
use adb_client::tcp::ADBTcpDevice;
use adb_client::ADBDeviceExt;
use std::net::SocketAddr;

fn run(device: &mut ADBTcpDevice, command: &str) {
    let mut out = Vec::new();
    match device.shell_command(&command, Some(&mut out), None) {
        Ok(status) => println!("$ {command}  [{status:?}]\n{}", String::from_utf8_lossy(&out).trim()),
        Err(error) => println!("$ {command}  HATA: {error}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let address: SocketAddr = format!("{}:5555", args[1]).parse().unwrap();
    let connect = || ADBTcpDevice::new_with_custom_private_key(address, &args[2]).expect("bağlanılamadı");

    println!("--- tek çağrıda birleşik komut");
    let mut device = connect();
    run(&mut device, "getprop ro.build.version.release; pm list packages com.isacetin; appops get com.isacetin.ps5ceviri SYSTEM_ALERT_WINDOW");
    println!("--- aynı bağlantıda ikinci komut");
    run(&mut device, "echo ikinci");
    println!("--- yeni bağlantıda komut");
    let mut device = connect();
    run(&mut device, "echo yeni-baglanti");
}
