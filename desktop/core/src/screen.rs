//! Platform katmanının sözleşmesi: ekrandan bir tur okuma. Mac (ScreenCaptureKit + Vision) ve
//! Windows (Windows.Graphics.Capture + Windows.Media.Ocr) bunu uygular; ana döngü yalnızca bunu görür.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::block_merger::TextLine;
use crate::geometry::Rect;

/// Bir tur ekran okumanın sonucu.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// Okunan alana (16:9 video alanı ya da oyunun istemci alanı/bölgesi) göre normalize satırlar,
    /// origin sol üst.
    pub lines: Vec<TextLine>,
    /// OCR'a verilen video alanının piksel boyutu.
    pub video_width: u32,
    pub video_height: u32,
    pub fullscreen: bool,
    pub capture_ms: u64,
    pub ocr_ms: u64,
    /// Video alanının ekrandaki yeri: Mac'te nokta (mantıksal birim), Windows'ta fiziksel piksel;
    /// origin ana ekranın sol üstü. Ekran overlay'i buraya oturur.
    pub screen: Option<Rect>,
}

/// Okunacak bölge (okunan alana göre 0...1); `None` ise tüm alan. Okuyucu her turda bakar, böylece
/// bölge iş hattı çalışırken değiştirilebilir.
pub type SharedRegion = Arc<Mutex<Option<Rect>>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadError {
    /// Hedef pencere ekranda yok; çağıran bekleyip tekrar dener.
    WindowNotFound(String),
    /// Akış yeni açıldı, henüz kare gelmedi; bir sonraki turda geçer.
    NoFrameYet,
    Other(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::WindowNotFound(message) | ReadError::Other(message) => f.write_str(message),
            ReadError::NoFrameYet => f.write_str("akıştan henüz kare gelmedi"),
        }
    }
}

impl std::error::Error for ReadError {}

/// Uygulamalar `Send` olmalı: okuma ana thread dışında, ayrı bir worker'da yapılır.
pub trait ScreenReader: Send {
    /// Pencereyi bulur, en son karenin video alanını okur. Sonuç gelene kadar bloklar.
    fn read(&mut self) -> Result<Reading, ReadError>;

    /// Debug: son okunan video karesini kutularla (normalize, origin sol üst) PNG olarak yazar.
    fn dump(&self, _path: &Path, _boxes: &[Rect]) -> Result<(), String> {
        Err("bu platformda kare kaydı desteklenmiyor".into())
    }
}
