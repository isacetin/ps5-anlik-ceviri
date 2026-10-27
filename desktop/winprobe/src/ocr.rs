//! Windows.Media.Ocr (Windows'un yerleşik OCR'ı; Mac'teki Vision karşılığı).

use image::RgbaImage;
use windows::core::{Result, HSTRING};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

use ceviri_core::{Rect, TextLine};

pub struct OcrSetup {
    /// Sistemde kurulu OCR dilleri (ör. "tr-TR", "en-US").
    pub installed: Vec<String>,
    pub engine: Option<OcrEngine>,
    pub engine_language: Option<String>,
    /// Bu boyuttan büyük görüntüler küçültülmeden tanınmaz.
    pub max_dimension: u32,
}

/// İngilizce OCR motorunu kurar. Önce en-US, yoksa kurulu herhangi bir İngilizce dil denenir.
pub fn setup() -> Result<OcrSetup> {
    let mut installed = Vec::new();
    for language in OcrEngine::AvailableRecognizerLanguages()? {
        installed.push(language.LanguageTag()?.to_string());
    }
    let max_dimension = OcrEngine::MaxImageDimension()?;

    let mut candidates = vec!["en-US".to_string()];
    candidates.extend(installed.iter().filter(|tag| tag.to_lowercase().starts_with("en")).cloned());
    for tag in candidates {
        let language = Language::CreateLanguage(&HSTRING::from(tag.as_str()))?;
        if OcrEngine::IsLanguageSupported(&language)? {
            if let Ok(engine) = OcrEngine::TryCreateFromLanguage(&language) {
                return Ok(OcrSetup { installed, engine: Some(engine), engine_language: Some(tag), max_dimension });
            }
        }
    }
    Ok(OcrSetup { installed, engine: None, engine_language: None, max_dimension })
}

/// Kutular görüntüye göre normalize (0...1), origin sol üst. Windows OCR güven değeri vermez; 1 kullanılır.
pub fn recognize(engine: &OcrEngine, image: &RgbaImage) -> Result<Vec<TextLine>> {
    let (width, height) = image.dimensions();
    let mut bgra = image.as_raw().clone();
    for pixel in bgra.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let writer = DataWriter::new()?;
    writer.WriteBytes(&bgra)?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        width as i32,
        height as i32,
        BitmapAlphaMode::Premultiplied,
    )?;

    let result = engine.RecognizeAsync(&bitmap)?.get()?;
    let mut lines = Vec::new();
    for line in result.Lines()? {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for word in line.Words()? {
            let r = word.BoundingRect()?;
            x0 = x0.min(r.X as f64);
            y0 = y0.min(r.Y as f64);
            x1 = x1.max((r.X + r.Width) as f64);
            y1 = y1.max((r.Y + r.Height) as f64);
        }
        if x1 <= x0 {
            continue;
        }
        let (w, h) = (width as f64, height as f64);
        lines.push(TextLine {
            text: line.Text()?.to_string(),
            bbox: Rect::new(x0 / w, y0 / h, (x1 - x0) / w, (y1 - y0) / h),
            confidence: 1.0,
        });
    }
    Ok(lines)
}
