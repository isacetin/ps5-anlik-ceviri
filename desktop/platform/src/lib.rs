//! İşletim sistemine göre platform katmanı seçimi: Mac'te `ceviri-mac` (Swift köprüsü), Windows'ta
//! `ceviri-windows`. Komut satırı aracı ve masaüstü uygulaması bunu kullanır; iş hattı yalnızca
//! `ScreenReader`'ı görür.

use ceviri_core::{Rect, ScreenReader, SharedRegion};

pub struct Window {
    pub app: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

/// Ekran Kaydı izni: Mac'te `Some(verildi mi)`, Windows'ta böyle bir izin yok (`None`).
pub fn screen_recording_granted() -> Option<bool> {
    #[cfg(target_os = "macos")]
    return Some(ceviri_mac::screen_recording_granted());
    #[cfg(not(target_os = "macos"))]
    None
}

/// Mac'te izin sorusunu gösterir (yalnızca ilk seferde); sonuç `screen_recording_granted` gibi.
pub fn request_screen_recording() -> Option<bool> {
    #[cfg(target_os = "macos")]
    return Some(ceviri_mac::request_screen_recording());
    #[cfg(not(target_os = "macos"))]
    None
}

/// Gizlilik ayarları sayfasını açar: "screen" (Ekran Kaydı) ya da "network" (Yerel Ağ). Yalnızca Mac.
pub fn open_privacy_settings(page: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let anchor = match page {
            "screen" => "Privacy_ScreenCapture",
            "network" => "Privacy_LocalNetwork",
            other => return Err(format!("bilinmeyen ayar sayfası: {other}")),
        };
        std::process::Command::new("open")
            .arg(format!("x-apple.systempreferences:com.apple.preference.security?{anchor}"))
            .status()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = page;
        Err("Bu işletim sisteminde gizlilik izni gerekmiyor".into())
    }
}

/// Bu bilgisayarda oynanan oyunun okunması (PS5'ten bağımsız) yalnızca Windows'ta var.
pub const GAME_SOURCE_SUPPORTED: bool = cfg!(windows);

/// PS Remote Play penceresi ekranda mı?
pub fn remote_play_open() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    return Ok(ceviri_mac::list_windows()?.iter().any(|w| w.bundle_id == ceviri_mac::REMOTE_PLAY_BUNDLE_ID));
    #[cfg(windows)]
    return Ok(ceviri_windows::win::visible_windows().iter().any(|w| w.looks_like_remote_play()));
    #[cfg(not(any(target_os = "macos", windows)))]
    Err("desteklenmeyen işletim sistemi".into())
}

/// Varsayılan tarayıcıda açar.
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("open").arg(url).status();
    #[cfg(windows)]
    let status = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).status();
    #[cfg(not(any(target_os = "macos", windows)))]
    let status = std::process::Command::new("xdg-open").arg(url).status();
    status.map(|_| ()).map_err(|e| e.to_string())
}

/// Süreç başında, ana thread'den çağrılır. Windows'ta koordinatların ölçeklenmeden fiziksel
/// piksel gelmesini sağlar (yakalanan kareyle aynı birim). GUI çatısı bunu zaten yaptıysa zararsız.
pub fn prepare_process() {
    #[cfg(windows)]
    ceviri_windows::win::enable_dpi_awareness();
}

/// Komut satırı için: `job`'ı worker thread'de çalıştırır, bitince süreç onun sonucuyla çıkar. Mac'te ana thread
/// bu sırada olay döngüsünü çalıştırır (ScreenCaptureKit ve Swift ana aktörü buna ihtiyaç duyar).
pub fn run(job: impl FnOnce() -> Result<(), String> + Send + 'static) -> ! {
    let finish = |result: Result<(), String>| -> ! {
        match result {
            Ok(()) => std::process::exit(0),
            Err(message) => {
                if !message.is_empty() {
                    eprintln!("Hata: {message}");
                }
                std::process::exit(1)
            }
        }
    };
    #[cfg(target_os = "macos")]
    {
        ceviri_mac::prepare();
        std::thread::spawn(move || finish(job()));
        ceviri_mac::run_main_loop()
    }
    #[cfg(not(target_os = "macos"))]
    {
        prepare_process();
        finish(job())
    }
}

#[cfg(not(windows))]
const GAME_UNSUPPORTED: &str = "Bilgisayarda oynanan oyunların çevirisi şimdilik yalnızca Windows'ta var.";

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use ceviri_mac::{MacScreenReader, REMOTE_PLAY_BUNDLE_ID};

    pub fn screen_reader(app: Option<&str>, fps: f64, title_bar: Option<f64>) -> Result<Box<dyn ScreenReader>, String> {
        Ok(Box::new(MacScreenReader::new(app.unwrap_or(REMOTE_PLAY_BUNDLE_ID), fps, title_bar)?))
    }

    pub fn game_reader(_window: &str, _region: SharedRegion) -> Result<Box<dyn ScreenReader>, String> {
        Err(GAME_UNSUPPORTED.into())
    }

    pub fn game_client_area(_window: &str) -> Option<Rect> {
        None
    }

    pub fn focus_game(_window: &str) {}

    pub fn list_windows() -> Result<Vec<Window>, String> {
        Ok(ceviri_mac::list_windows()?
            .into_iter()
            .map(|w| Window { app: w.bundle_id, title: w.title, width: w.width, height: w.height })
            .collect())
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use ceviri_windows::{win, WindowsScreenReader};

    /// Windows'ta `app`, pencere başlığında ya da exe adında aranan metindir; `title_bar` piksel.
    pub fn screen_reader(app: Option<&str>, _fps: f64, title_bar: Option<f64>) -> Result<Box<dyn ScreenReader>, String> {
        Ok(Box::new(WindowsScreenReader::new(app, title_bar, 1.0)?))
    }

    /// Bu bilgisayarda oynanan oyun: `window` başlıkta ya da exe adında aranır, istemci alanı okunur.
    pub fn game_reader(window: &str, region: SharedRegion) -> Result<Box<dyn ScreenReader>, String> {
        Ok(Box::new(WindowsScreenReader::game(window, region)?))
    }

    /// Oyun penceresinin istemci alanının ekrandaki yeri (fiziksel piksel, `Reading.screen` gibi);
    /// pencere yoksa `None`.
    pub fn game_client_area(window: &str) -> Option<Rect> {
        win::find_window(window).filter(|w| !w.minimized).map(|w| win::client_screen(&w))
    }

    /// Oyun penceresini öne getirir (bölge seçildikten sonra klavye oyuna dönsün).
    pub fn focus_game(window: &str) {
        // Boş metin her pencereye uyar.
        if let Some(window) = win::find_window(window).filter(|_| !window.is_empty()) {
            win::focus(&window);
        }
    }

    pub fn list_windows() -> Result<Vec<Window>, String> {
        Ok(win::visible_windows()
            .into_iter()
            .map(|w| Window { app: w.exe.clone(), title: w.title.clone(), width: w.width().max(0) as u32, height: w.height().max(0) as u32 })
            .collect())
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    use super::*;

    const UNSUPPORTED: &str = "Ekran okuma yalnızca macOS ve Windows'ta destekleniyor";

    pub fn screen_reader(_app: Option<&str>, _fps: f64, _title_bar: Option<f64>) -> Result<Box<dyn ScreenReader>, String> {
        Err(UNSUPPORTED.into())
    }

    pub fn game_reader(_window: &str, _region: SharedRegion) -> Result<Box<dyn ScreenReader>, String> {
        Err(GAME_UNSUPPORTED.into())
    }

    pub fn game_client_area(_window: &str) -> Option<Rect> {
        None
    }

    pub fn focus_game(_window: &str) {}

    pub fn list_windows() -> Result<Vec<Window>, String> {
        Err(UNSUPPORTED.into())
    }
}

pub use imp::{focus_game, game_client_area, game_reader, list_windows, screen_reader};
