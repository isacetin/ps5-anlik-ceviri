//! OCR her şeyi okuyabilir: Kiril/Yunan harfi, birleşik aksan, emoji, kontrol karakteri, yarım
//! XML etiketi, yer tutucuya benzeyen metin. Metin işleme zincirinin hiçbir adımı bunlarla panik
//! yapmamalı (iş hattı turu atlayıp devam eder ama o metin hiç çevrilmez). Rastgele ama
//! tekrarlanabilir girdilerle denenir.

use ceviri_core::block_merger::{self, TextLine};
use ceviri_core::{focus, glossary, language, ocr_text, opus_text, stabilizer, terms, translator, Rect, Stabilizer, Terms};

/// Bağımlılıksız, tekrarlanabilir sözde rastgele sayı üreteci (xorshift).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn unit(&mut self) -> f64 {
        (self.next() % 10_000) as f64 / 10_000.0
    }
}

const PIECES: &[&str] = &[
    "The", "door", "is", "locked", ".", "!", "?", "…", "'", "’", "\"", "|", "|'m", "I", "l", "1", "0", "O", " ", "  ", "\t", "\n", "<x>", "</x>", "<", ">",
    "&", "&amp;", "ZXQ0", "ZXQ1", "ZXQ10", "'da", "’te", "It Takes Two", "PlayStation", "PS5", "Los Santos", "ı", "İ", "ş", "ğ", "Ç", "é", "e\u{301}",
    "тар", "Ω", "😀", "\u{200B}", "\u{FEFF}", "\u{0}", "•", "-", "—", "2/5", "100%", "Resume", "x", "⊗", "...", "Avatar: Frontiers of Pand...",
];

fn random_text(rng: &mut Rng) -> String {
    let count = rng.below(12);
    (0..count).map(|_| PIECES[rng.below(PIECES.len())]).collect::<Vec<_>>().join(if rng.below(2) == 0 { " " } else { "" })
}

#[test]
fn rastgele_ocr_metni_hicbir_adimda_panik_yapmaz() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let terms = Terms::new(&["It Takes Two".into(), "Los Santos".into(), "ZXQ0".into(), "<x>".into(), "e\u{301}".into()]);
    let mut stabilizer = Stabilizer::new();
    for round in 0..20_000 {
        let lines: Vec<TextLine> = (0..rng.below(6))
            .map(|_| {
                let text = ocr_text::clean(&random_text(&mut rng));
                let bbox = Rect::new(rng.unit(), rng.unit(), rng.unit() * 0.6, rng.unit() * 0.1);
                TextLine { text, bbox, confidence: rng.unit() }
            })
            .collect();
        let merged = block_merger::merge(&lines);
        let stable = stabilizer.update(&merged, round as f64 * 0.2);
        let _ = focus::classify(&stable, &focus::Options::default());
        for block in &stable {
            let text = &block.text;
            let _ = language::looks_turkish(text);
            let _ = glossary::lookup(text);
            let _ = terms.is_only_terms(text);
            let protected = terms.protect(text);
            let prepared = opus_text::prepare(&protected);
            // Model çıktısı da rastgele olabilir (yer tutucu kaybolur, çoğalır, ek alır).
            let model = format!("{} {}", prepared.source, random_text(&mut rng));
            let finished = prepared.finish(&model).or_else(|| prepared.plain().finish(&model)).unwrap_or_default();
            let _ = terms::unprotect(&finished);
            let _ = translator::is_valid_translation(&protected, &finished);
            let _ = focus::beside(block.bbox, block.line_height);
        }
        let a = random_text(&mut rng);
        let b = random_text(&mut rng);
        let _ = stabilizer::same_text(&a, &b);
    }
}
