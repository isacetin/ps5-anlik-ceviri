//! Platform katmanının sözleşmesi: ekrandan bir tur okuma. Mac (ScreenCaptureKit + Vision) ve
//! Windows (Windows.Graphics.Capture + Windows.Media.Ocr) bunu uygular; ana döngü yalnızca bunu görür.

use std::fmt;

use crate::block_merger::TextLine;

/// Bir tur ekran okumanın sonucu.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// 16:9 video alanına göre normalize satırlar, origin sol üst.
    pub lines: Vec<TextLine>,
    /// OCR'a verilen video alanının piksel boyutu.
    pub video_width: u32,
    pub video_height: u32,
    pub fullscreen: bool,
    pub capture_ms: u64,
    pub ocr_ms: u64,
}

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
}
