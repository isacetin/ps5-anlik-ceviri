//! Ekran overlay'i: oyun penceresinin video alanının tam üstüne oturan şeffaf, her zaman üstte,
//! tıklamayı alttaki pencereye geçiren bir pencere. Çizim `ui/overlay.html`'dedir; buradan her tur
//! blok listesi gönderilir, pencere de video alanı hareket ettikçe yer değiştirir.

use std::sync::Mutex;

use ceviri_core::OverlayBlock;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::controller::ScreenRect;

const LABEL: &str = "overlay";

pub struct Overlay {
    app: AppHandle,
    /// Pencerenin son konumu; değişmediyse yeniden konumlandırılmaz.
    placed: Mutex<Option<ScreenRect>>,
}

impl Overlay {
    pub fn new(app: AppHandle) -> Overlay {
        Overlay { app, placed: Mutex::new(None) }
    }

    /// `screen` yoksa (oyun penceresi bulunamadı ya da durduruldu) overlay gizlenir.
    pub fn show(&self, blocks: &[OverlayBlock], screen: Option<ScreenRect>) {
        let mut placed = self.placed.lock().unwrap();
        let Some(rect) = screen else {
            if let Some(window) = self.app.get_webview_window(LABEL) {
                let _ = window.hide();
            }
            *placed = None;
            return;
        };
        let window = match self.app.get_webview_window(LABEL) {
            Some(window) => window,
            None => match self.create() {
                Ok(window) => window,
                Err(error) => {
                    eprintln!("Overlay penceresi açılamadı: {error}");
                    return;
                }
            },
        };
        if *placed != Some(rect) {
            let _ = window.set_position(LogicalPosition::new(rect.x, rect.y));
            let _ = window.set_size(LogicalSize::new(rect.width, rect.height));
            let _ = window.show();
            *placed = Some(rect);
        }
        let _ = self.app.emit_to(LABEL, "overlay", blocks);
    }

    fn create(&self) -> tauri::Result<WebviewWindow> {
        let window = WebviewWindowBuilder::new(&self.app, LABEL, WebviewUrl::App("overlay.html".into()))
            .title("Anlık Çeviri — overlay")
            .transparent(true)
            .decorations(false)
            .shadow(false)
            .resizable(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .focused(false)
            .visible(false)
            .build()?;
        // Fare ve klavye oyuna gitsin; overlay yalnızca görüntüdür.
        window.set_ignore_cursor_events(true)?;
        Ok(window)
    }
}
