//! PS5'in kendi arayüzü Türkçe; Türkçe metni DeepL'e göndermek kota harcar ve Türkçe yazının
//! üstüne yine Türkçe çizer. Türkçe görünen metinler çeviriye gitmez.
//!
//! Mac sürümü `NLLanguageRecognizer` kullanıyordu; Windows'ta karşılığı olmadığı için burada
//! iki platformda aynı çalışan `whatlang` kullanılır.

use whatlang::{Detector, Lang};

const TURKISH_LETTERS: &[char] = &['ç', 'ğ', 'ı', 'ş', 'ö', 'ü', 'Ç', 'Ğ', 'İ', 'Ş', 'Ö', 'Ü'];

/// Yanlışlıkla İngilizceyi Türkçe sanmak çeviriyi kaçırtır; bu yüzden eşik yüksek tutulur.
const MIN_CONFIDENCE: f64 = 0.8;

pub fn looks_turkish(text: &str) -> bool {
    if text.contains(TURKISH_LETTERS) {
        return true;
    }
    let detector = Detector::with_allowlist(vec![Lang::Eng, Lang::Tur]);
    detector.detect(text).is_some_and(|info| info.lang() == Lang::Tur && info.confidence() > MIN_CONFIDENCE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turkce_harf_iceren_metin_turkcedir() {
        assert!(looks_turkish("Konsol Depolama Alanı"));
        assert!(looks_turkish("Seçenekler"));
        assert!(looks_turkish("PS5 sisteminiz kullanışlı hale"));
    }

    #[test]
    fn turkce_harfsiz_uzun_turkce_cumle_taninir() {
        assert!(looks_turkish("Bu oyunu oynamak icin internet gerekli"));
        assert!(looks_turkish("Yeni bildirim var"));
    }

    #[test]
    fn kisa_belirsiz_metin_ceviriye_gider() {
        // whatlang kısa metinlerde kararsız ("Hello" bile düşük güvenle Türkçe çıkıyor); emin
        // olunmayan metin çevrilir. Bedeli: harfsiz kısa Türkçe menü öğeleri biraz kota harcar.
        assert!(!looks_turkish("Kupalar"));
        assert!(!looks_turkish("Son Oynananlar"));
    }

    #[test]
    fn ingilizce_metin_turkce_sayilmaz() {
        for text in [
            "Play Local",
            "Play Online",
            "Options",
            "Accessibility Options",
            "Credits",
            "The gate is locked.",
            "You shouldn't have come here, traveler. The gate is locked.",
            "Press any button to continue",
            "Chapter One: The Shed",
            // whatlang bunları düşük güvenle Türkçe buluyor; eşik bu yüzden yüksek.
            "Hello",
            "Quit Game",
            "Load Game",
            "Settings",
            "Rose",
        ] {
            assert!(!looks_turkish(text), "{text}");
        }
    }
}
