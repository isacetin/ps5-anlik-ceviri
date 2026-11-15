//! Fizibilite: Rust adb istemcisiyle TV'de salt okunur kabuk komutları çalıştırır.
//! cargo run -p ceviri-tvsetup --example probe -- 192.168.1.5 ~/.android/adbkey "komut" ["komut" ...]
use adb_client::tcp::ADBTcpDevice;
use adb_client::ADBDeviceExt;
use std::net::SocketAddr;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let address: SocketAddr = format!("{}:5555", args[1]).parse().unwrap();
    for command in &args[3..] {
        // Her komut kendi bağlantısıyla: bazı TV'ler aynı bağlantıda ikinci oturumu ara sıra reddediyor.
        let mut device = ADBTcpDevice::new_with_custom_private_key(address, &args[2]).expect("bağlanılamadı");
        let mut out = Vec::new();
        match device.shell_command(command, Some(&mut out), None) {
            Ok(status) => println!("$ {command}  [{status:?}]\n{}", String::from_utf8_lossy(&out).trim()),
            Err(error) => println!("$ {command}  HATA: {error}"),
        }
    }
}
