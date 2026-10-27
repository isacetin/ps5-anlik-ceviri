//! İşletim sistemine göre platform katmanı. Ana döngü yalnızca `PlatformReader`'ı görür.

use std::path::Path;

use ceviri_core::{Rect, ScreenReader};

pub struct Window {
    pub app: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

/// Okuyucu + debug çıktısı (kutular çizilmiş kare).
pub trait PlatformReader: ScreenReader {
    fn dump(&self, path: &Path, boxes: &[Rect]) -> Result<(), String>;
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
    finish(job())
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use ceviri_mac::{MacScreenReader, REMOTE_PLAY_BUNDLE_ID};

    impl PlatformReader for MacScreenReader {
        fn dump(&self, path: &Path, boxes: &[Rect]) -> Result<(), String> {
            MacScreenReader::dump(self, path, boxes)
        }
    }

    pub fn screen_reader(app: Option<&str>, fps: f64, title_bar: Option<f64>) -> Result<Box<dyn PlatformReader>, String> {
        Ok(Box::new(MacScreenReader::new(app.unwrap_or(REMOTE_PLAY_BUNDLE_ID), fps, title_bar)?))
    }

    pub fn list_windows() -> Result<Vec<Window>, String> {
        Ok(ceviri_mac::list_windows()?
            .into_iter()
            .map(|w| Window { app: w.bundle_id, title: w.title, width: w.width, height: w.height })
            .collect())
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;

    // Windows katmanı (desktop/winprobe'daki Windows.Graphics.Capture + Windows.Media.Ocr kodu)
    // ayrı bir crate'e taşınınca buraya bağlanacak.
    const NOT_YET: &str = "Bu işletim sistemi için ekran okuma henüz bağlanmadı";

    pub fn screen_reader(_app: Option<&str>, _fps: f64, _title_bar: Option<f64>) -> Result<Box<dyn PlatformReader>, String> {
        Err(NOT_YET.into())
    }

    pub fn list_windows() -> Result<Vec<Window>, String> {
        Err(NOT_YET.into())
    }
}

pub use imp::{list_windows, screen_reader};
