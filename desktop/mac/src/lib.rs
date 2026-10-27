//! Mac platform katmanı: `mac/` Swift paketindeki ScreenCaptureKit + Vision kodunu (CeviriBridge)
//! C ABI üzerinden Rust'a açar. Swift tarafı gerçek oyunda doğrulanmış koddur; burada yeniden
//! yazılmaz.
//!
//! Thread kuralı: `prepare` ve `run_main_loop` ANA thread'den çağrılır ve ana thread olay
//! döngüsüne bırakılır. Okuma işleri (`MacScreenReader`, `list_windows`) başka bir thread'den
//! yapılır; Swift tarafı işi kendi içinde ana aktöre taşır. Tauri gibi GUI çatıları ana thread'de
//! olay döngüsünü zaten çalıştırır.

#![cfg(target_os = "macos")]

use std::ffi::{c_char, c_void, CStr, CString};
use std::path::Path;
use std::ptr::NonNull;

use ceviri_core::{ReadError, Reading, Rect, ScreenReader, TextLine};
use serde::{Deserialize, Serialize};

pub const REMOTE_PLAY_BUNDLE_ID: &str = "com.playstation.RemotePlay";

extern "C" {
    fn ceviri_prepare();
    fn ceviri_run_main_loop();
    fn ceviri_string_free(pointer: *mut c_char);
    fn ceviri_list_windows() -> *mut c_char;
    fn ceviri_reader_new(bundle_id: *const c_char, fps: f64, title_bar: f64) -> *mut c_void;
    fn ceviri_reader_free(handle: *mut c_void);
    fn ceviri_reader_read(handle: *mut c_void) -> *mut c_char;
    fn ceviri_reader_dump(handle: *mut c_void, path: *const c_char, boxes_json: *const c_char) -> *mut c_char;
}

/// Ana thread'den, başka her şeyden önce çağrılır (pencere sunucusu bağlantısını açar).
pub fn prepare() {
    unsafe { ceviri_prepare() }
}

/// Ana thread'i olay döngüsüne bırakır; dönmez. Program worker thread'den `process::exit` ile biter.
pub fn run_main_loop() -> ! {
    unsafe { ceviri_run_main_loop() };
    unreachable!("ceviri_run_main_loop döndü")
}

#[derive(Clone, Debug, Deserialize)]
pub struct WindowInfo {
    #[serde(rename = "bundleID")]
    pub bundle_id: String,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    #[derive(Deserialize)]
    struct Response {
        ok: Option<Vec<WindowInfo>>,
        error: Option<ErrorJson>,
    }
    let json = take(unsafe { ceviri_list_windows() }).ok_or("köprü yanıt vermedi")?;
    let response: Response = serde_json::from_str(&json).map_err(|e| format!("köprü yanıtı okunamadı: {e}"))?;
    match (response.ok, response.error) {
        (Some(windows), _) => Ok(windows),
        (_, Some(error)) => Err(error.message),
        _ => Err("köprü boş yanıt verdi".into()),
    }
}

/// ScreenCaptureKit akışı + Vision OCR. Bir pencereye (varsayılan PS Remote Play) bağlıdır.
pub struct MacScreenReader {
    handle: NonNull<c_void>,
}

// Swift tarafı tüm durumu ana aktörde tutar; tutamaç başka thread'e taşınabilir.
unsafe impl Send for MacScreenReader {}

impl MacScreenReader {
    /// `title_bar`: başlık çubuğu yüksekliği (pt); `None` ise tam ekranda 0, pencerede 28.
    pub fn new(bundle_id: &str, fps: f64, title_bar: Option<f64>) -> Result<MacScreenReader, String> {
        let bundle = CString::new(bundle_id).map_err(|_| "geçersiz bundle ID")?;
        let handle = unsafe { ceviri_reader_new(bundle.as_ptr(), fps, title_bar.unwrap_or(-1.0)) };
        NonNull::new(handle)
            .map(|handle| MacScreenReader { handle })
            .ok_or_else(|| "okuyucu oluşturulamadı (ana thread'den mi çağrıldı?)".into())
    }
}

impl ScreenReader for MacScreenReader {
    fn read(&mut self) -> Result<Reading, ReadError> {
        let json = take(unsafe { ceviri_reader_read(self.handle.as_ptr()) }).ok_or(ReadError::Other("köprü yanıt vermedi".into()))?;
        parse_reading(&json)
    }

    fn dump(&self, path: &Path, boxes: &[Rect]) -> Result<(), String> {
        #[derive(Serialize)]
        struct BoxJson {
            x: f64,
            y: f64,
            w: f64,
            h: f64,
        }
        let boxes: Vec<BoxJson> = boxes.iter().map(|b| BoxJson { x: b.x, y: b.y, w: b.width, h: b.height }).collect();
        let path = CString::new(path.to_string_lossy().as_bytes()).map_err(|_| "geçersiz yol")?;
        let boxes = CString::new(serde_json::to_string(&boxes).unwrap()).unwrap();
        match take(unsafe { ceviri_reader_dump(self.handle.as_ptr(), path.as_ptr(), boxes.as_ptr()) }) {
            None => Ok(()),
            Some(error) => Err(error),
        }
    }
}

impl Drop for MacScreenReader {
    fn drop(&mut self) {
        unsafe { ceviri_reader_free(self.handle.as_ptr()) }
    }
}

#[derive(Deserialize)]
struct ErrorJson {
    kind: String,
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadingJson {
    lines: Vec<LineJson>,
    video_width: u32,
    video_height: u32,
    fullscreen: bool,
    capture_ms: u64,
    ocr_ms: u64,
}

#[derive(Deserialize)]
struct LineJson {
    text: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    confidence: f64,
}

fn parse_reading(json: &str) -> Result<Reading, ReadError> {
    #[derive(Deserialize)]
    struct Response {
        reading: Option<ReadingJson>,
        error: Option<ErrorJson>,
    }
    let response: Response = serde_json::from_str(json).map_err(|e| ReadError::Other(format!("köprü yanıtı okunamadı: {e}")))?;
    if let Some(error) = response.error {
        return Err(match error.kind.as_str() {
            "windowNotFound" => ReadError::WindowNotFound(error.message),
            "noFrameYet" => ReadError::NoFrameYet,
            _ => ReadError::Other(error.message),
        });
    }
    let reading = response.reading.ok_or(ReadError::Other("köprü boş yanıt verdi".into()))?;
    Ok(Reading {
        lines: reading
            .lines
            .into_iter()
            .map(|l| TextLine { text: l.text, bbox: Rect::new(l.x, l.y, l.w, l.h), confidence: l.confidence })
            .collect(),
        video_width: reading.video_width,
        video_height: reading.video_height,
        fullscreen: reading.fullscreen,
        capture_ms: reading.capture_ms,
        ocr_ms: reading.ocr_ms,
    })
}

/// Swift'in `strdup` ile verdiği string'i kopyalayıp bırakır; NULL → `None`.
fn take(pointer: *mut c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let text = unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned();
    unsafe { ceviri_string_free(pointer) };
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kopru_okuma_yaniti_cozulur() {
        let json = r#"{"reading":{"lines":[{"text":"Play Local","x":0.1,"y":0.5,"w":0.2,"h":0.03,"confidence":1}],
            "videoWidth":1920,"videoHeight":1080,"fullscreen":true,"captureMs":3,"ocrMs":95}}"#;

        let reading = parse_reading(json).unwrap();

        assert_eq!(reading.lines.len(), 1);
        assert_eq!(reading.lines[0].text, "Play Local");
        assert_eq!(reading.lines[0].bbox, Rect::new(0.1, 0.5, 0.2, 0.03));
        assert_eq!((reading.video_width, reading.video_height, reading.fullscreen), (1920, 1080, true));
        assert_eq!((reading.capture_ms, reading.ocr_ms), (3, 95));
    }

    #[test]
    fn kopru_hata_turleri_eslenir() {
        let error = |kind: &str| parse_reading(&format!(r#"{{"error":{{"kind":"{kind}","message":"m"}}}}"#)).unwrap_err();

        assert_eq!(error("windowNotFound"), ReadError::WindowNotFound("m".into()));
        assert_eq!(error("noFrameYet"), ReadError::NoFrameYet);
        assert_eq!(error("other"), ReadError::Other("m".into()));
    }
}
