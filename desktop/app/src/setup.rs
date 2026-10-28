//! İlk kurulum sihirbazının komutları: izinler, Remote Play, TV'yi bulma/kurma/deneme/kaldırma.
//! Ağ ve TV işleri bloklayıcıdır; arayüz donmasın diye ayrı thread'de çalışır.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;

use ceviri_core::{storage, OverlayBlock};
use ceviri_engine::{tv_client, TvClient};
use ceviri_tvsetup::{Installer, Step};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::controller::{Controller, Settings};

/// Kurulum sihirbazı TV'ye bunu yükler (build.rs gömer; APK derlenmemişse boştur).
static TV_APK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/tv.apk"));

fn tv_apk_version() -> u32 {
    env!("TV_APK_VERSION").parse().unwrap_or(0)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    /// "macos" | "windows"
    platform: &'static str,
    /// Mac'te Ekran Kaydı izni; Windows'ta böyle bir izin yok (`null`).
    screen_recording: Option<bool>,
    remote_play: bool,
    tv_apk_available: bool,
    has_deepl_key: bool,
    has_local_model: bool,
    /// İndirilecek modelin boyutu (MB).
    local_model_mb: u64,
    setup_done: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TvCandidate {
    ip: String,
    /// TV uygulaması zaten kurulu ve çalışıyor.
    app_running: bool,
}

fn key_path() -> Result<PathBuf, String> {
    storage::app_dir().map(|dir| dir.join("adbkey")).ok_or_else(|| "uygulama klasörü bulunamadı".into())
}

fn parse_ip(ip: &str) -> Result<Ipv4Addr, String> {
    ip.trim().parse().map_err(|_| format!("Geçersiz TV adresi: {ip} (ör. 192.168.1.5)"))
}

async fn blocking<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(job).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn setup_status(controller: State<'_, Controller>) -> Result<SetupStatus, String> {
    let setup_done = controller.status().settings.setup_done;
    blocking(move || {
        let screen_recording = ceviri_platform::screen_recording_granted();
        SetupStatus {
            platform: if cfg!(target_os = "macos") { "macos" } else { "windows" },
            screen_recording,
            // Pencere listesi ekran içeriğine erişir: izin yokken bakmak macOS'un izin penceresini,
            // kullanıcı izin adımına gelmeden açardı.
            remote_play: screen_recording != Some(false) && ceviri_platform::remote_play_open().unwrap_or(false),
            tv_apk_available: !TV_APK.is_empty(),
            has_deepl_key: storage::deepl_key().is_some(),
            has_local_model: storage::local_model_dir().is_some_and(|dir| ceviri_opus::model::is_installed(&dir)),
            local_model_mb: ceviri_opus::model::total_size().div_ceil(1_000_000),
            setup_done,
        }
    })
    .await
}

#[tauri::command]
pub fn request_screen_recording() -> Option<bool> {
    ceviri_platform::request_screen_recording()
}

#[tauri::command]
pub fn open_privacy_settings(page: String) -> Result<(), String> {
    ceviri_platform::open_privacy_settings(&page)
}

/// Yalnızca bilinen adresler açılır (arayüzden gelen rastgele bir adres değil).
#[tauri::command]
pub fn open_link(which: String) -> Result<(), String> {
    let url = match which.as_str() {
        "remote-play" => "https://www.playstation.com/tr-tr/remote-play/",
        "deepl" => "https://www.deepl.com/pro-api",
        other => return Err(format!("bilinmeyen bağlantı: {other}")),
    };
    ceviri_platform::open_url(url)
}

/// Yerel ağda "Ağ üzerinden hata ayıklama"sı açık cihazlar.
#[tauri::command]
pub async fn tv_scan() -> Result<Vec<TvCandidate>, String> {
    blocking(|| {
        ceviri_tvsetup::scan(Duration::from_millis(400))
            .into_iter()
            .map(|ip| TvCandidate {
                ip: ip.to_string(),
                app_running: ceviri_tvsetup::port_open(ip, ceviri_tvsetup::APP_PORT, Duration::from_millis(400)),
            })
            .collect()
    })
    .await
}

#[derive(Clone, Serialize)]
struct SetupProgress {
    label: &'static str,
}

fn step_label(step: &Step) -> &'static str {
    match step {
        Step::Connecting => "TV'ye bağlanılıyor… TV'de izin sorusu çıkarsa \"Her zaman izin ver\"i işaretleyip Tamam'a basın.",
        Step::CheckingVersion => "TV'deki sürüm kontrol ediliyor…",
        Step::Installing => "TV uygulaması yükleniyor (yaklaşık 20 saniye)…",
        Step::GrantingPermission => "Görüntünün üstünde gösterme izni veriliyor…",
        Step::Starting => "TV uygulaması açılıyor…",
        Step::WaitingForApp => "TV uygulamasının hazır olması bekleniyor…",
    }
}

/// Kur (gerekirse), izni ver, başlat. İlerleme "kurulum" olayıyla gelir.
#[tauri::command]
pub async fn tv_install(app: AppHandle, ip: String) -> Result<(), String> {
    if TV_APK.is_empty() {
        return Err("Bu derlemede TV uygulaması yok (APK gömülmemiş).".into());
    }
    let ip = parse_ip(&ip)?;
    let key = key_path()?;
    blocking(move || {
        Installer::new(ip, key)
            .setup(TV_APK, tv_apk_version(), |step| {
                let _ = app.emit_to("main", "kurulum", SetupProgress { label: step_label(&step) });
            })
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await?
}

/// TV'de birkaç saniye "hazır" yazısı gösterir, sonra temizler.
#[tauri::command]
pub async fn tv_test(ip: String) -> Result<(), String> {
    let address = tv_client::with_default_port(&parse_ip(&ip)?.to_string());
    blocking(move || {
        let mut tv = TvClient::new(address);
        let block = OverlayBlock {
            id: "kurulum".into(),
            x: 0.25,
            y: 0.42,
            w: 0.5,
            h: 0.12,
            line_h: 0.06,
            text: "Anlık Çeviri hazır ✓".into(),
        };
        for _ in 0..6 {
            tv.send(std::slice::from_ref(&block))?;
            std::thread::sleep(Duration::from_secs(1));
        }
        tv.close();
        Ok(())
    })
    .await?
}

/// TV uygulamasını kaldırır. TV'nin Ayarlar → Uygulamalar ekranından da kaldırılabilir.
#[tauri::command]
pub async fn tv_uninstall(ip: String) -> Result<(), String> {
    let ip = parse_ip(&ip)?;
    let key = key_path()?;
    blocking(move || Installer::new(ip, key).uninstall().map_err(|e| e.to_string())).await?
}

/// Sihirbazın sonunda: ayarlar kaydedilir ve sihirbaz bir daha kendiliğinden açılmaz.
#[tauri::command]
pub fn finish_setup(controller: State<'_, Controller>, mut settings: Settings) -> Result<(), String> {
    settings.setup_done = true;
    controller.update_settings(settings.clone())?;
    settings.save()
}
