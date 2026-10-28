//! Windows ekran okuyucu: Mac'teki Swift `ScreenReader`'ın karşılığı.
//! Pencereyi bul → yakalama oturumunu açık tut → en son karenin okunacak alanını kes → OCR.
//! Okunacak alan PS Remote Play'de 16:9 video alanı, PC oyununda pencerenin istemci alanıdır
//! (isteğe bağlı olarak kullanıcının seçtiği bölge).
//!
//! Bazı oyunlarda (ör. Call of Duty'nin "Fullscreen Extended Window" modu) pencere yakalaması
//! siyah kare verir. Oyun modunda bu olunca oyunun bulunduğu monitör yakalanır ve oyunun
//! istemci alanı oradan kesilir; uygulamanın kendi pencereleri o yakalamaya girmez.

use std::path::Path;
use std::time::{Duration, Instant};

use ceviri_core::geometry::{client_rect, sub_rect, video_rect};
use ceviri_core::{ReadError, Reading, Rect, ScreenReader, SharedRegion};
use image::{imageops, RgbaImage};
use windows::Media::Ocr::OcrEngine;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::HMONITOR;

use crate::capture::WindowCapture;
use crate::imaging::{crop, draw_boxes, is_mostly_black, prepare_for_ocr};
use crate::{ensure_mta, ocr, win};

/// Bu kadar tur üst üste siyah kare gelirse (2 fps'te ~10 sn) kullanıcı uyarılır; daha kısası
/// yükleme ekranıdır.
const BLACK_ROUNDS: u32 = 20;
/// Pencere yakalaması bu kadar tur (~2 sn) siyah gelirse monitör yakalamaya geçilir.
const WINDOW_BLACK_ROUNDS: u32 = 4;
/// `CEVIRI_YAKALAMA=ekran`: oyun modunda baştan monitör yakalanır (test ve sorun giderme için).
const CAPTURE_ENV: &str = "CEVIRI_YAKALAMA";

/// Yakalanan şey: pencere ya da monitör (tutamaç, sayı olarak).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Window(isize),
    Monitor(isize),
}

/// Neyin okunduğu.
enum Source {
    /// PS Remote Play (ya da `filter`'a uyan pencere): 16:9 video alanı.
    RemotePlay { filter: Option<String>, title_bar: Option<f64> },
    /// Bu bilgisayarda oynanan oyun: istemci alanı, varsa içindeki bölge.
    Game { filter: String, region: SharedRegion },
}

pub struct WindowsScreenReader {
    source: Source,
    /// OCR'dan önce okunan alanın büyütme oranı (küçük yazılar için).
    ocr_scale: f64,
    engine: OcrEngine,
    max_dimension: u32,
    /// Açık yakalama oturumu ve neyi yakaladığı.
    capture: Option<(Target, WindowCapture)>,
    /// Oyun modunda pencere yerine monitör yakalanıyor.
    monitor: bool,
    last_video: Option<RgbaImage>,
    /// Üst üste gelen siyah kare sayısı (yalnızca oyun modu).
    black_rounds: u32,
}

// Tüm nesneler çok iş parçacıklı daireye (MTA) aittir: frame pool serbest iş parçacıklı,
// D3D11 cihazı ve OcrEngine iş parçacığından bağımsızdır. Okuyucu bir kez worker thread'e
// taşınır ve orada kullanılır.
unsafe impl Send for WindowsScreenReader {}

impl WindowsScreenReader {
    /// `window_filter`: küçük harfle başlıkta ya da exe adında aranır; `None` ise PS Remote Play.
    /// `title_bar`: başlık çubuğu yüksekliği (fiziksel piksel); `None` ise pencereden ölçülür, tam ekranda 0.
    pub fn new(window_filter: Option<&str>, title_bar: Option<f64>, ocr_scale: f64) -> Result<WindowsScreenReader, String> {
        Self::with_source(Source::RemotePlay { filter: window_filter.map(str::to_lowercase), title_bar }, ocr_scale)
    }

    /// Bu bilgisayarda oynanan oyun: `window` başlıkta ya da exe adında aranır.
    pub fn game(window: &str, region: SharedRegion) -> Result<WindowsScreenReader, String> {
        let mut reader = Self::with_source(Source::Game { filter: window.to_lowercase(), region }, 1.0)?;
        reader.monitor = std::env::var(CAPTURE_ENV).is_ok_and(|v| v.eq_ignore_ascii_case("ekran"));
        Ok(reader)
    }

    fn with_source(source: Source, ocr_scale: f64) -> Result<WindowsScreenReader, String> {
        ensure_mta();
        if !crate::capture::is_supported() {
            return Err("Bu Windows sürümü pencere yakalamayı desteklemiyor (Windows 10 1903+ gerekir).".into());
        }
        let setup = ocr::setup().map_err(|e| format!("OCR başlatılamadı: {e}"))?;
        let engine = setup.engine.ok_or(
            "İngilizce OCR dili kurulu değil. Ayarlar > Zaman ve dil > Dil ve bölge > Dil ekle > English (United States).",
        )?;
        Ok(WindowsScreenReader {
            source,
            ocr_scale: ocr_scale.clamp(0.25, 4.0),
            engine,
            max_dimension: setup.max_dimension,
            capture: None,
            monitor: false,
            last_video: None,
            black_rounds: 0,
        })
    }

    fn find_window(&self) -> Result<win::WindowInfo, ReadError> {
        let windows = win::visible_windows();
        let (found, name, hint) = match &self.source {
            Source::RemotePlay { filter: Some(filter), .. } => {
                (windows.into_iter().find(|w| w.matches(filter)), filter.as_str(), "PS Remote Play açık ve PS5'e bağlı mı?")
            }
            Source::RemotePlay { filter: None, .. } => {
                (windows.into_iter().find(|w| w.looks_like_remote_play()), "PS Remote Play", "PS Remote Play açık ve PS5'e bağlı mı?")
            }
            // Oyunlar başlatıcı, açılış ekranı gibi küçük pencereler de açar: en büyüğü oyundur.
            Source::Game { filter, .. } => (
                windows.into_iter().filter(|w| w.matches(filter)).max_by_key(|w| (!w.minimized, w.width() as i64 * w.height() as i64)),
                filter.as_str(),
                "Oyun açık mı?",
            ),
        };
        match found {
            None => Err(ReadError::WindowNotFound(format!("Ekranda {name} penceresi bulunamadı. {hint}"))),
            Some(window) if window.minimized => {
                Err(ReadError::WindowNotFound(format!("{name} penceresi simge durumunda; simge durumundaki pencereler yakalanamaz.")))
            }
            Some(window) => Ok(window),
        }
    }

    /// Yakalanan karede okunacak alan (piksel). `base`: karenin ekrandaki yeri (pencere ya da monitör).
    fn area(&self, window: &win::WindowInfo, base: RECT, image: &RgbaImage, fullscreen: bool) -> Rect {
        let (width, height) = (image.width() as f64, image.height() as f64);
        match &self.source {
            Source::RemotePlay { title_bar, .. } => {
                let scale_y = height / window.height().max(1) as f64;
                let top_inset = title_bar.unwrap_or(if fullscreen { 0.0 } else { window.top_inset as f64 * scale_y });
                video_rect(width, height, top_inset)
            }
            Source::Game { region, .. } => {
                let client = client_rect(rect(base), rect(window.client), width, height);
                match *region.lock().unwrap() {
                    Some(region) => sub_rect(client, region),
                    None => client,
                }
            }
        }
    }
}

fn rect(r: RECT) -> Rect {
    Rect::new(r.left as f64, r.top as f64, (r.right - r.left) as f64, (r.bottom - r.top) as f64)
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
        let (target, base) = match self.monitor.then(|| win::monitor_of(&window)).flatten() {
            Some((monitor, bounds)) => {
                // Overlay sonradan açılabilir: her turda bakılır (zaten ayarlı pencereye dokunulmaz).
                win::exclude_own_windows(true);
                (Target::Monitor(monitor.0 as isize), bounds)
            }
            None => (Target::Window(window.hwnd.0 as isize), window.frame),
        };
        if self.capture.as_ref().map(|(current, _)| *current) != Some(target) {
            let capture = match target {
                Target::Window(_) => WindowCapture::start(window.hwnd),
                Target::Monitor(monitor) => WindowCapture::start_monitor(HMONITOR(monitor as *mut _)),
            };
            let capture = capture.map_err(|e| ReadError::Other(format!("Yakalama başlatılamadı: {e}")))?;
            self.capture = Some((target, capture));
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
        let video_px = self.area(&window, base, &image, fullscreen);
        let video = crop(&image, video_px);
        let captured = Instant::now();

        // Oyunun tüm istemci alanına bakılır: seçili bölge (altyazı bandı) karanlık sahnede
        // yazısızken de siyahtır.
        if matches!(self.source, Source::Game { .. }) {
            let client = client_rect(rect(base), rect(window.client), image.width() as f64, image.height() as f64);
            let view = imageops::crop_imm(
                &image,
                client.x as u32,
                client.y as u32,
                (client.width as u32).max(1),
                (client.height as u32).max(1),
            );
            self.black_rounds = if is_mostly_black(&*view) { self.black_rounds + 1 } else { 0 };
            if !self.monitor && self.black_rounds == WINDOW_BLACK_ROUNDS {
                // Pencere yakalaması bu oyunda çalışmıyor: monitöre geçilir, sonraki tur oradan okur.
                self.monitor = true;
                self.black_rounds = 0;
                return Err(ReadError::NoFrameYet);
            }
            if self.black_rounds == BLACK_ROUNDS {
                return Err(ReadError::Other(format!(
                    "Oyun görüntüsü alınamıyor (kare siyah; pencere: \"{}\", {}, {}x{}). Oyunun grafik ayarlarında ekran modunu \"Kenarlıksız pencere\" (Fullscreen Borderless) ya da \"Pencere\" (Windowed) yapın; özel tam ekran yakalanamaz.",
                    window.title,
                    window.exe,
                    window.width(),
                    window.height(),
                )));
            }
        }

        // Kare, yakalanan pencerenin ya da monitörün fiziksel piksel karşılığıdır; ekran konumu da
        // fiziksel piksel (overlay öyle yerleşir; farklı ölçekli monitörlerde mantıksal birim kayardı).
        let scale_x = image.width() as f64 / (base.right - base.left).max(1) as f64;
        let scale_y = image.height() as f64 / (base.bottom - base.top).max(1) as f64;
        let screen = Rect::new(
            base.left as f64 + video_px.x / scale_x,
            base.top as f64 + video_px.y / scale_y,
            video_px.width / scale_x,
            video_px.height / scale_y,
        );

        let prepared = prepare_for_ocr(&video, self.ocr_scale, self.max_dimension);
        let lines = ocr::recognize(&self.engine, &prepared).map_err(|e| ReadError::Other(format!("OCR hatası: {e}")))?;
        let reading = Reading {
            lines,
            video_width: video.width(),
            video_height: video.height(),
            fullscreen,
            capture_ms: (captured - start).as_millis() as u64,
            ocr_ms: captured.elapsed().as_millis() as u64,
            screen: Some(screen),
        };
        self.last_video = Some(video);
        Ok(reading)
    }

    fn dump(&self, path: &Path, boxes: &[Rect]) -> Result<(), String> {
        let video = self.last_video.as_ref().ok_or("henüz okunmuş kare yok")?;
        draw_boxes(video, boxes).save(path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

impl Drop for WindowsScreenReader {
    fn drop(&mut self) {
        // Çeviri durunca uygulamanın pencereleri yeniden ekran görüntüsüne/kayda girsin.
        if self.monitor {
            win::exclude_own_windows(false);
        }
    }
}
