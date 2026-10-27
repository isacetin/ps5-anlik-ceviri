//! TV uygulamasını masaüstünden ağ üzerinden kurar (kullanıcı terminale dokunmaz):
//! ağda TV'yi bul → bağlan (TV bir kez izin sorar) → APK'yı yükle → overlay iznini ver → başlat.
//!
//! Google'ın `adb` aracı yerine adb protokolünü uygulayan `adb_client` kullanılır: ek kurulum
//! gerekmez, Mac ve Windows'ta aynı çalışır. Bağlantı uygulamanın kendi sürecinden açıldığı için
//! macOS Yerel Ağ izni de uygulamaya aittir.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use adb_client::tcp::ADBTcpDevice;
use adb_client::ADBDeviceExt;

/// TV uygulamasının paket adı.
pub const PACKAGE: &str = "com.isacetin.ps5ceviri";
/// Android TV'de "Ağ üzerinden hata ayıklama" açıkken adb'nin dinlediği port.
pub const ADB_PORT: u16 = 5555;
/// TV uygulamasının WebSocket portu (çalıştığının işareti).
pub const APP_PORT: u16 = 8765;

/// TV'nin izin sorusunu kullanıcının onaylaması için verilen süre.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// TV'ye bağlanılıyor; TV ilk seferde "hata ayıklamaya izin verilsin mi?" diye sorar.
    Connecting,
    CheckingVersion,
    Installing,
    GrantingPermission,
    Starting,
    WaitingForApp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupError {
    /// 5555 kapalı: "Ağ üzerinden hata ayıklama" kapalı ya da adres yanlış.
    DebuggingOff,
    /// Bağlantı kuruldu ama TV'deki izin sorusu onaylanmadı (ya da reddedildi).
    NotApproved,
    InstallFailed(String),
    /// Uygulama kuruldu ama çalışmaya başlamadı.
    AppNotRunning,
    Other(String),
}

impl std::fmt::Display for SetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SetupError::DebuggingOff => f.write_str(
                "TV'ye bağlanılamadı. TV'de Ayarlar → Cihaz Tercihleri → Geliştirici seçenekleri altında \"Ağ üzerinden hata ayıklama\" açık mı? TV adresi doğru mu?",
            ),
            SetupError::NotApproved => f.write_str(
                "TV izin vermedi. TV ekranındaki \"Hata ayıklamaya izin verilsin mi?\" sorusunda \"Her zaman izin ver\"i işaretleyip Tamam'a basın ve tekrar deneyin.",
            ),
            SetupError::InstallFailed(message) => write!(f, "TV uygulaması yüklenemedi: {message}"),
            SetupError::AppNotRunning => f.write_str("TV uygulaması yüklendi ama başlamadı. TV'de \"PS5 Çeviri\" uygulamasını açıp \"Overlay'i başlat\"a basın."),
            SetupError::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SetupError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupReport {
    /// TV'de önceden kurulu sürüm (yoksa `None`).
    pub previous_version: Option<u32>,
    pub installed: bool,
}

/// Bu bilgisayarın yerel IPv4 adresi. Hiç paket gönderilmez: UDP soketi yalnızca yönlendirme
/// tablosuna göre bir kaynak adresi seçer.
pub fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// Yerel ağdaki (/24) cihazlardan adb portu açık olanlar: büyük ihtimalle hata ayıklaması açık TV'ler.
pub fn scan(timeout_per_host: Duration) -> Vec<Ipv4Addr> {
    let Some(own) = local_ipv4() else { return Vec::new() };
    let [a, b, c, _] = own.octets();
    let hosts: Vec<Ipv4Addr> = (1..=254u8).map(|d| Ipv4Addr::new(a, b, c, d)).filter(|ip| *ip != own).collect();
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        for chunk in hosts.chunks(16) {
            let sender = sender.clone();
            scope.spawn(move || {
                for ip in chunk {
                    if port_open(*ip, ADB_PORT, timeout_per_host) {
                        let _ = sender.send(*ip);
                    }
                }
            });
        }
    });
    drop(sender);
    let mut found: Vec<Ipv4Addr> = receiver.into_iter().collect();
    found.sort();
    found
}

pub fn port_open(ip: Ipv4Addr, port: u16, timeout: Duration) -> bool {
    TcpStream::connect_timeout(&SocketAddr::V4(SocketAddrV4::new(ip, port)), timeout).is_ok()
}

/// Uygulamanın adb anahtarı yoksa üretir (RSA 2048, PKCS#8 PEM). TV bu anahtarı "her zaman izin
/// ver" ile hatırlar; anahtar her seferinde değişseydi TV her bağlantıda yeniden sorardı.
pub fn ensure_key(path: &Path) -> io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    use rsa::pkcs8::{EncodePrivateKey, LineEnding};
    let key = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).map_err(io::Error::other)?;
    let pem = key.to_pkcs8_pem(LineEnding::LF).map_err(io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, pem.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// `dumpsys package` çıktısından kurulu sürüm kodu ("versionCode=2 minSdk=26 …").
pub fn parse_version_code(dumpsys: &str) -> Option<u32> {
    let start = dumpsys.find("versionCode=")? + "versionCode=".len();
    dumpsys[start..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

pub struct Installer {
    address: SocketAddrV4,
    key_path: PathBuf,
}

impl Installer {
    pub fn new(ip: Ipv4Addr, key_path: impl Into<PathBuf>) -> Installer {
        Installer { address: SocketAddrV4::new(ip, ADB_PORT), key_path: key_path.into() }
    }

    /// Kurulumun tamamı: `prepare` + `start`.
    pub fn setup(&self, apk: &[u8], apk_version: u32, mut progress: impl FnMut(Step)) -> Result<SetupReport, SetupError> {
        let report = self.prepare(apk, apk_version, &mut progress)?;
        self.start(&mut progress)?;
        Ok(report)
    }

    /// Bağlanır, gerekirse yükler ve overlay iznini verir; TV ekranında hiçbir şey açmaz.
    /// `apk` gömülü TV uygulaması, `apk_version` onun sürüm kodu: TV'de aynı ya da daha yeni sürüm
    /// varsa yeniden yüklenmez.
    pub fn prepare(&self, apk: &[u8], apk_version: u32, mut progress: impl FnMut(Step)) -> Result<SetupReport, SetupError> {
        if !port_open(*self.address.ip(), ADB_PORT, Duration::from_secs(2)) {
            return Err(SetupError::DebuggingOff);
        }
        ensure_key(&self.key_path).map_err(|e| SetupError::Other(format!("Anahtar oluşturulamadı: {e}")))?;

        progress(Step::Connecting);
        let mut device = self.connect()?;

        progress(Step::CheckingVersion);
        let previous_version = parse_version_code(&self.shell(&mut device, &format!("dumpsys package {PACKAGE} | grep versionCode"))?);
        let needs_install = previous_version.is_none_or(|v| v < apk_version);
        if needs_install {
            progress(Step::Installing);
            self.install(&mut device, apk)?;
            // Yükleme bağlantıyı yorabiliyor; sonraki adımlar temiz bağlantıyla devam eder.
            device = self.connect()?;
        }

        progress(Step::GrantingPermission);
        self.shell(&mut device, &format!("appops set {PACKAGE} SYSTEM_ALERT_WINDOW allow"))?;
        Ok(SetupReport { previous_version, installed: needs_install })
    }

    /// TV uygulamasını açar ve overlay'i başlatır (TV ekranında uygulama açılır); uygulama
    /// dinlemeye başlayana kadar bekler.
    pub fn start(&self, mut progress: impl FnMut(Step)) -> Result<(), SetupError> {
        progress(Step::Connecting);
        let mut device = self.connect()?;
        progress(Step::Starting);
        self.shell(&mut device, &format!("am start -n {PACKAGE}/.ui.MainActivity --ez start_overlay true"))?;

        progress(Step::WaitingForApp);
        let deadline = Instant::now() + Duration::from_secs(20);
        while !port_open(*self.address.ip(), APP_PORT, Duration::from_millis(500)) {
            if Instant::now() > deadline {
                return Err(SetupError::AppNotRunning);
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Ok(())
    }

    /// TV uygulamasını tamamen kaldırır (ayarları ve izniyle birlikte). Kullanıcı TV'nin kendi
    /// Ayarlar → Uygulamalar ekranından da kaldırabilir; bu yalnızca kolaylıktır. TV'de başka hiçbir
    /// şeye dokunulmaz.
    pub fn uninstall(&self) -> Result<(), SetupError> {
        if !port_open(*self.address.ip(), ADB_PORT, Duration::from_secs(2)) {
            return Err(SetupError::DebuggingOff);
        }
        let mut device = self.connect()?;
        let output = self.shell(&mut device, &format!("pm uninstall {PACKAGE}"))?;
        if output.contains("Success") || output.contains("not installed") || output.contains("Unknown package") {
            Ok(())
        } else {
            Err(SetupError::Other(format!("TV uygulaması kaldırılamadı: {}", output.trim())))
        }
    }

    /// TV ilk bağlantıda kullanıcıya izin sorar; onaylanana kadar bağlantı bekler. Süre dolarsa
    /// ya da reddedilirse `NotApproved`.
    fn connect(&self) -> Result<ADBTcpDevice, SetupError> {
        let (sender, receiver) = mpsc::channel();
        let address = self.address;
        let key_path = self.key_path.clone();
        std::thread::spawn(move || {
            let _ = sender.send(ADBTcpDevice::new_with_custom_private_key(address, key_path));
        });
        match receiver.recv_timeout(APPROVAL_TIMEOUT) {
            Ok(Ok(device)) => Ok(device),
            Ok(Err(error)) => {
                let message = error.to_string();
                if message.contains("AUTH") || message.to_lowercase().contains("auth") {
                    Err(SetupError::NotApproved)
                } else if !port_open(*address.ip(), ADB_PORT, Duration::from_secs(2)) {
                    Err(SetupError::DebuggingOff)
                } else {
                    Err(SetupError::Other(format!("TV'ye bağlanılamadı: {message}")))
                }
            }
            Err(_) => Err(SetupError::NotApproved),
        }
    }

    /// Klasik `adb install` gibi: APK'yı TV'nin geçici klasörüne kopyalar, `pm install -r` ile kurar.
    /// (Akışlı `cmd package install` bazı TV'lerde son durumu düzgün döndürmüyor.)
    fn install(&self, device: &mut ADBTcpDevice, apk: &[u8]) -> Result<(), SetupError> {
        let remote = "/data/local/tmp/anlik-ceviri-tv.apk";
        device.push(&mut io::Cursor::new(apk), &remote).map_err(|e| SetupError::InstallFailed(format!("APK TV'ye kopyalanamadı: {e}")))?;
        let output = self.shell(device, &format!("pm install -r {remote}; rm -f {remote}"))?;
        if output.contains("Success") {
            Ok(())
        } else {
            Err(SetupError::InstallFailed(output.trim().to_string()))
        }
    }

    /// Kabuk komutu; oturum açılamazsa (bazı TV'lerde ara sıra olur) yeniden bağlanıp bir kez dener.
    fn shell(&self, device: &mut ADBTcpDevice, command: &str) -> Result<String, SetupError> {
        let mut output = Vec::new();
        if device.shell_command(&command, Some(&mut output), None).is_err() {
            *device = self.connect()?;
            output.clear();
            device.shell_command(&command, Some(&mut output), None).map_err(|e| SetupError::Other(format!("TV komutu çalışmadı ({command}): {e}")))?;
        }
        Ok(String::from_utf8_lossy(&output).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn surum_kodu_dumpsys_ciktisindan_okunur() {
        assert_eq!(parse_version_code("    versionCode=2 minSdk=26 targetSdk=36\n"), Some(2));
        assert_eq!(parse_version_code("versionCode=140 minSdk=26"), Some(140));
        assert_eq!(parse_version_code(""), None);
    }

    #[test]
    fn anahtar_bir_kez_uretilir_ve_korunur() {
        let dir = std::env::temp_dir().join(format!("ceviri-tvsetup-key-{}", std::process::id()));
        let path = dir.join("adbkey");
        let _ = std::fs::remove_dir_all(&dir);

        ensure_key(&path).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        ensure_key(&path).unwrap();

        assert!(first.starts_with("-----BEGIN PRIVATE KEY-----"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first, "var olan anahtar değişmemeli");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn port_kontrolu() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        assert!(port_open(Ipv4Addr::LOCALHOST, port, Duration::from_millis(300)));
        drop(listener);
        assert!(!port_open(Ipv4Addr::LOCALHOST, port, Duration::from_millis(300)));
    }

    #[test]
    fn hata_ayiklama_kapaliysa_anlasilir_hata() {
        let port_closed_ip = Ipv4Addr::LOCALHOST; // 5555 yerelde kapalı varsayılır
        if port_open(port_closed_ip, ADB_PORT, Duration::from_millis(200)) {
            return; // bu makinede bir adb sunucusu dinliyor; test anlamsız
        }
        let installer = Installer::new(port_closed_ip, std::env::temp_dir().join("yok-adbkey"));

        assert_eq!(installer.setup(&[], 1, |_| {}), Err(SetupError::DebuggingOff));
    }
}
