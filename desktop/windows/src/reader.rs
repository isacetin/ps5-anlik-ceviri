//! Windows ekran okuyucu: Mac'teki Swift `ScreenReader`'ın karşılığı.
//! Pencereyi bul → yakalama oturumunu açık tut → en son karenin 16:9 video alanını kes → OCR.

use std::path::Path;
use std::time::{Duration, Instant};

use ceviri_core::geometry::video_rect;
use ceviri_core::{ReadError, Reading, Rect, ScreenReader};
use image::RgbaImage;
use windows::Media::Ocr::OcrEngine;

use crate::capture::WindowCapture;
use crate::imaging::{crop, draw_boxes, prepare_for_ocr};
use crate::{ensure_mta, ocr, win};

pub struct WindowsScreenReader {
    /// Küçük harfle başlıkta ya da exe adında aranır; `None` ise PS Remote Play.
    window_filter: Option<String>,
    /// Başlık çubuğu yüksekliği (fiziksel piksel); `None` ise pencereden ölçülür, tam ekranda 0.
    title_bar: Option<f64>,
    /// OCR'dan önce video alanının büyütme oranı (küçük yazılar için).
    ocr_scale: f64,
    engine: OcrEngine,
    max_dimension: u32,
    /// Açık yakalama oturumu ve ait olduğu pencere (HWND, sayı olarak).
    capture: Option<(isize, WindowCapture)>,
    last_video: Option<RgbaImage>,
}

// Tüm nesneler çok iş parçacıklı daireye (MTA) aittir: frame pool serbest iş parçacıklı,
// D3D11 cihazı ve OcrEngine iş parçacığından bağımsızdır. Okuyucu bir kez worker thread'e
// taşınır ve orada kullanılır.
unsafe impl Send for WindowsScreenReader {}

impl WindowsScreenReader {
    pub fn new(window_filter: Option<&str>, title_bar: Option<f64>, ocr_scale: f64) -> Result<WindowsScreenReader, String> {
        ensure_mta();
        if !crate::capture::is_supported() {
            return Err("Bu Windows sürümü pencere yakalamayı desteklemiyor (Windows 10 1903+ gerekir).".into());
        }
        let setup = ocr::setup().map_err(|e| format!("OCR başlatılamadı: {e}"))?;
        let engine = setup.engine.ok_or(
            "İngilizce OCR dili kurulu değil. Ayarlar > Zaman ve dil > Dil ve bölge > Dil ekle > English (United States).",
        )?;
        Ok(WindowsScreenReader {
            window_filter: window_filter.map(str::to_lowercase),
            title_bar,
            ocr_scale: ocr_scale.clamp(0.25, 4.0),
            engine,
            max_dimension: setup.max_dimension,
            capture: None,
            last_video: None,
        })
    }

    fn find_window(&self) -> Result<win::WindowInfo, ReadError> {
        let windows = win::visible_windows();
        let found = windows.into_iter().find(|w| match &self.window_filter {
            Some(filter) => w.title.to_lowercase().contains(filter) || w.exe.to_lowercase().contains(filter),
            None => w.looks_like_remote_play(),
        });
        let name = self.window_filter.as_deref().unwrap_or("PS Remote Play");
        match found {
            None => Err(ReadError::WindowNotFound(format!("Ekranda {name} penceresi bulunamadı. PS Remote Play açık ve PS5'e bağlı mı?"))),
            Some(window) if window.minimized => {
                Err(ReadError::WindowNotFound(format!("{name} penceresi simge durumunda; simge durumundaki pencereler yakalanamaz.")))
            }
            Some(window) => Ok(window),
        }
    }
}

impl ScreenReader for WindowsScreenReader {
    fn read(&mut self) -> Result<Reading, ReadError> {
        ensure_mta();
        let window = match self.find_window() {
            Ok(window) => window,
            Err(error) => {
                self.capture = None;
                return Err(error);
            }
        };
        let start = Instant::now();
        let key = window.hwnd.0 as isize;
        if self.capture.as_ref().map(|(hwnd, _)| *hwnd) != Some(key) {
            let capture = WindowCapture::start(window.hwnd).map_err(|e| ReadError::Other(format!("Yakalama başlatılamadı: {e}")))?;
            self.capture = Some((key, capture));
        }
        let (_, capture) = self.capture.as_mut().expect("oturum az önce açıldı");
        let image = match capture.latest_frame(Duration::from_millis(500)) {
            Ok(Some((frame, _fresh))) => {
                RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone()).ok_or(ReadError::Other("kare tampon boyutu hatalı".into()))?
            }
            Ok(None) => return Err(ReadError::NoFrameYet),
            Err(error) => {
                // Pencere kapandı ya da cihaz kayboldu: bir sonraki turda oturum yeniden kurulur.
                self.capture = None;
                return Err(ReadError::Other(format!("Kare okunamadı: {error}")));
            }
        };

        let fullscreen = win::is_fullscreen(&window);
        let scale_y = image.height() as f64 / window.height().max(1) as f64;
        let top_inset = self.title_bar.unwrap_or(if fullscreen { 0.0 } else { window.top_inset as f64 * scale_y });
        let video = crop(&image, video_rect(image.width() as f64, image.height() as f64, top_inset));
        let captured = Instant::now();

        let prepared = prepare_for_ocr(&video, self.ocr_scale, self.max_dimension);
        let lines = ocr::recognize(&self.engine, &prepared).map_err(|e| ReadError::Other(format!("OCR hatası: {e}")))?;
        let reading = Reading {
            lines,
            video_width: video.width(),
            video_height: video.height(),
            fullscreen,
            capture_ms: (captured - start).as_millis() as u64,
            ocr_ms: captured.elapsed().as_millis() as u64,
        };
        self.last_video = Some(video);
        Ok(reading)
    }

    fn dump(&self, path: &Path, boxes: &[Rect]) -> Result<(), String> {
        let video = self.last_video.as_ref().ok_or("henüz okunmuş kare yok")?;
        draw_boxes(video, boxes).save(path).map_err(|e| format!("{}: {e}", path.display()))
    }
}
