//! PC oyununda okunacak bölgeyi seçme penceresi: oyunun istemci alanını tam kaplayan, yarı
//! karartılmış, tıklanabilir bir pencere. Kullanıcı fareyle dikdörtgen çizer; sonuç `set_region`
//! komutuyla gelir (`ui/region.html`).
//!
//! Pencere oluşturmak ve kapatmak ana thread'i bekletir: senkron bir komuttan ya da kısayol
//! işleyicisinden doğrudan çağrılırsa Windows'ta kilitlenebilir. Bu yüzden çağıranlar bunları ayrı
//! bir thread'de (async komut, `spawn_blocking`) çalıştırır.

use ceviri_core::Rect;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::controller::ScreenRect;
use crate::overlay;

const LABEL: &str = "region";

/// `area`: oyunun istemci alanının ekrandaki yeri (`Reading.screen` ile aynı birim). Açıksa önce
/// kapatılır, böylece pencere taşındıysa yeni yerinde açılır.
pub fn open(app: &AppHandle, area: Rect) -> Result<(), String> {
    close(app);
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("region.html".into()))
        .title("Anlık Çeviri — bölge seç")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
        .map_err(|e| format!("Bölge seçme penceresi açılamadı: {e}"))?;
    overlay::place(&window, ScreenRect { x: area.x, y: area.y, width: area.width, height: area.height });
    let _ = window.show();
    let _ = window.set_focus();
    Ok(())
}

pub fn close(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.destroy();
    }
}
