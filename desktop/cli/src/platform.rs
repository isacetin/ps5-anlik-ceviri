//! İşletim sistemine göre platform katmanı. Ana döngü yalnızca `ScreenReader`'ı görür.

use ceviri_core::ScreenReader;

pub struct Window {
    pub app: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

/// `job`'ı worker thread'de çalıştırır, bitince süreç onun sonucuyla çıkar. Mac'te ana thread
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
        // Koordinatlar ölçeklenmeden fiziksel piksel olarak gelsin (yakalanan kareyle aynı birim).
        #[cfg(windows)]
        ceviri_windows::win::enable_dpi_awareness();
        finish(job())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use ceviri_mac::{MacScreenReader, REMOTE_PLAY_BUNDLE_ID};

    pub fn screen_reader(app: Option<&str>, fps: f64, title_bar: Option<f64>) -> Result<Box<dyn ScreenReader>, String> {
        Ok(Box::new(MacScreenReader::new(app.unwrap_or(REMOTE_PLAY_BUNDLE_ID), fps, title_bar)?))
    }

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

    pub fn list_windows() -> Result<Vec<Window>, String> {
        Err(UNSUPPORTED.into())
    }
}

pub use imp::{list_windows, screen_reader};
