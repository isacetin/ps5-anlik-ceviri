//! Ekran overlay'i: oyun penceresinin video alanının tam üstüne oturan şeffaf, her zaman üstte,
//! tıklamayı alttaki pencereye geçiren bir pencere. Çizim `ui/overlay.html`'dedir; buradan her tur
//! blok listesi gönderilir, pencere de video alanı hareket ettikçe yer değiştirir.
//!
//! Konum birimi `Reading.screen` ile aynıdır: Mac'te nokta (mantıksal), Windows'ta fiziksel piksel
//! (monitörler farklı ölçekteyse mantıksal birim, overlay'in durduğu monitöre göre yanlış çevrilirdi).
//!
//! Windows'ta pencere bir kez görünür açılır ve bir daha gizlenmez: gizlenip yeniden gösterilen
//! pencere (ShowWindow SW_SHOW) odağı oyundan çalardı. "Gizle" orada boş liste çizmek demektir.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use ceviri_core::OverlayBlock;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::controller::ScreenRect;

const LABEL: &str = "overlay";

pub struct Overlay {
    app: AppHandle,
    /// Pencerenin son konumu; değişmediyse yeniden konumlandırılmaz.
    placed: Mutex<Option<ScreenRect>>,
    /// Kullanıcı kısayolla gizledi; iş hattı çalışmaya devam eder, yalnızca çizilmez.
    hidden: AtomicBool,
}

impl Overlay {
    pub fn new(app: AppHandle) -> Overlay {
        Overlay { app, placed: Mutex::new(None), hidden: AtomicBool::new(false) }
    }

    /// Gizliyse gösterir, görünüyorsa gizler; yeni durumu (gizli mi) döner.
    pub fn toggle(&self) -> bool {
        let hidden = !self.hidden.fetch_xor(true, Ordering::SeqCst);
        if hidden {
            self.hide();
        }
        hidden
    }

    pub fn set_hidden(&self, hidden: bool) {
        self.hidden.store(hidden, Ordering::SeqCst);
        if hidden {
            self.hide();
        }
    }

    fn hide(&self) {
        let Some(window) = self.app.get_webview_window(LABEL) else { return };
        if cfg!(windows) {
            let _ = self.app.emit_to(LABEL, "overlay", Vec::<OverlayBlock>::new());
        } else {
            let mut placed = self.placed.lock().unwrap();
            let _ = window.hide();
            *placed = None;
        }
    }

    /// `screen` yoksa (oyun penceresi bulunamadı ya da durduruldu) overlay gizlenir.
    pub fn show(&self, blocks: &[OverlayBlock], screen: Option<ScreenRect>) {
        let Some(rect) = screen.filter(|_| !self.hidden.load(Ordering::SeqCst)) else {
            self.hide();
            return;
        };
        let mut placed = self.placed.lock().unwrap();
        let window = match self.app.get_webview_window(LABEL) {
            Some(window) => window,
            None => match self.create(rect) {
                Ok(window) => window,
                Err(error) => {
                    eprintln!("Overlay penceresi açılamadı: {error}");
                    return;
                }
            },
        };
        if *placed != Some(rect) {
            place(&window, rect);
            if !cfg!(windows) {
                let _ = window.show();
            }
            *placed = Some(rect);
        }
        let _ = self.app.emit_to(LABEL, "overlay", blocks);
    }

    fn create(&self, rect: ScreenRect) -> tauri::Result<WebviewWindow> {
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
            // Windows: WS_EX_NOACTIVATE, overlay hiçbir zaman öne gelen pencere olmaz.
            .focusable(!cfg!(windows))
            // Windows'ta oluşturulurken görünür: o anda odak almadan (SW_SHOWNOACTIVATE) gösterilir.
            .visible(cfg!(windows))
            .build()?;
        place(&window, rect);
        // Fare ve klavye oyuna gitsin; overlay yalnızca görüntüdür.
        window.set_ignore_cursor_events(true)?;
        Ok(window)
    }
}

/// Pencereyi `rect`'e taşır (Mac'te mantıksal, Windows'ta fiziksel birim). Önce konum: başka
/// ölçekli monitöre geçerken Windows boyutu değiştirir, ardından boyut düzeltilir.
pub fn place(window: &WebviewWindow, rect: ScreenRect) {
    #[cfg(windows)]
    {
        use tauri::{PhysicalPosition, PhysicalSize};
        let _ = window.set_position(PhysicalPosition::new(rect.x.round() as i32, rect.y.round() as i32));
        let _ = window.set_size(PhysicalSize::new(rect.width.round().max(1.0) as u32, rect.height.round().max(1.0) as u32));
    }
    #[cfg(not(windows))]
    {
        use tauri::{LogicalPosition, LogicalSize};
        let _ = window.set_position(LogicalPosition::new(rect.x, rect.y));
        let _ = window.set_size(LogicalSize::new(rect.width, rect.height));
    }
}
