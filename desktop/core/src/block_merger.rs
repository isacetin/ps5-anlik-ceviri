//! OCR satırlarını filtreler ve aynı paragrafa ait olanları tek bloğa birleştirir, böylece
//! çok satırlı bir diyalog DeepL'e tek cümle olarak gider.

use std::collections::BTreeMap;

use crate::geometry::{Rect, VIDEO_ASPECT};

/// OCR'ın bulduğu tek satır; `bbox` video alanına göre normalize, origin sol üst.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    pub text: String,
    pub bbox: Rect,
    /// 0...1. Güven değeri vermeyen OCR motorları (Windows) 1 verir.
    pub confidence: f64,
}

/// Birleştirilmiş metin bloğu: bir veya birden çok OCR satırından oluşan paragraf.
#[derive(Clone, Debug, PartialEq)]
pub struct MergedBlock {
    /// Satır kutularının birleşimi; video alanına göre normalize, origin sol üst.
    pub bbox: Rect,
    /// Medyan satır yüksekliği (normalize); TV'de font boyutu için başlangıç değeri.
    pub line_height: f64,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub min_confidence: f64,
    /// Alt alta iki satır arasındaki boşluk, satır yüksekliğinin en fazla bu katı olabilir.
    pub max_line_gap: f64,
    /// Satır yükseklikleri oranı (büyük / küçük) en fazla bu kadar olabilir.
    pub max_height_ratio: f64,
    /// Sol kenar ya da merkez hizası için tolerans (satır yüksekliği cinsinden).
    pub align_tolerance: f64,
    /// Aynı satırda bölünmüş iki parça arasındaki yatay boşluk (satır yüksekliği cinsinden).
    pub max_word_gap: f64,
    /// Üst satır en az bu genişlikteyse (satır yüksekliği cinsinden) kaydırılmış paragraf satırı
    /// sayılır. Daha kısa satırlar (menü öğeleri) yalnızca alt satır küçük harfle devam ediyorsa birleşir.
    pub min_wrap_width: f64,
    /// x koordinatlarını y ile aynı birime çevirmek için video en-boy oranı.
    pub aspect: f64,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            min_confidence: 0.4,
            max_line_gap: 0.7,
            max_height_ratio: 1.4,
            align_tolerance: 1.0,
            max_word_gap: 1.2,
            min_wrap_width: 12.0,
            aspect: VIDEO_ASPECT,
        }
    }
}

pub fn merge(lines: &[TextLine]) -> Vec<MergedBlock> {
    merge_with(lines, &Options::default())
}

pub fn merge_with(lines: &[TextLine], options: &Options) -> Vec<MergedBlock> {
    let valid: Vec<TextLine> = lines
        .iter()
        .filter_map(|line| {
            let text = line.text.trim();
            let usable = line.confidence >= options.min_confidence && text.chars().count() >= 2 && text.chars().any(char::is_alphabetic);
            usable.then(|| TextLine { text: text.to_string(), ..line.clone() })
        })
        .collect();

    // Union-find: birleştirilebilir her satır çifti aynı gruba düşer.
    let mut parent: Vec<usize> = (0..valid.len()).collect();
    fn root(parent: &mut [usize], mut index: usize) -> usize {
        while parent[index] != index {
            parent[index] = parent[parent[index]];
            index = parent[index];
        }
        index
    }
    for i in 0..valid.len() {
        for j in i + 1..valid.len() {
            if belong_together(&valid[i], &valid[j], options) {
                let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                parent[rj] = ri;
            }
        }
    }

    let mut groups: BTreeMap<usize, Vec<&TextLine>> = BTreeMap::new();
    for (index, line) in valid.iter().enumerate() {
        groups.entry(root(&mut parent, index)).or_default().push(line);
    }
    let mut blocks: Vec<MergedBlock> = groups.into_values().map(|group| make_block(&group)).collect();
    blocks.sort_by(|a, b| (a.bbox.y, a.bbox.x).partial_cmp(&(b.bbox.y, b.bbox.x)).unwrap_or(std::cmp::Ordering::Equal));
    blocks
}

/// Satır kutusu, x ekseni en-boy oranıyla y birimine çevrilmiş haliyle.
struct Box {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

impl Box {
    fn new(rect: Rect, aspect: f64) -> Box {
        Box { left: rect.x * aspect, right: rect.right() * aspect, top: rect.y, bottom: rect.bottom() }
    }

    fn height(&self) -> f64 {
        self.bottom - self.top
    }

    fn center_x(&self) -> f64 {
        (self.left + self.right) / 2.0
    }

    fn center_y(&self) -> f64 {
        (self.top + self.bottom) / 2.0
    }
}

fn belong_together(a: &TextLine, b: &TextLine, options: &Options) -> bool {
    let box_a = Box::new(a.bbox, options.aspect);
    let box_b = Box::new(b.bbox, options.aspect);
    let small = box_a.height().min(box_b.height());
    let large = box_a.height().max(box_b.height());
    if small <= 0.0 || large / small > options.max_height_ratio {
        return false;
    }
    let line_height = (box_a.height() + box_b.height()) / 2.0;

    // Aynı satırın OCR tarafından bölünmüş parçaları.
    if (box_a.center_y() - box_b.center_y()).abs() <= line_height * 0.3 {
        let gap = box_a.left.max(box_b.left) - box_a.right.min(box_b.right);
        return gap <= line_height * options.max_word_gap;
    }

    // Alt alta satırlar: dikey boşluk küçük, yatayda örtüşüyor ve solda ya da ortada hizalı.
    let a_is_upper = box_a.top <= box_b.top;
    let (upper, lower) = if a_is_upper { (&box_a, &box_b) } else { (&box_b, &box_a) };
    let lower_text = if a_is_upper { &b.text } else { &a.text };
    let vertical_gap = lower.top - upper.bottom;
    if vertical_gap > line_height * options.max_line_gap || vertical_gap <= -line_height * 0.5 {
        return false;
    }
    let overlaps = upper.right.min(lower.right) > upper.left.max(lower.left);
    let tolerance = line_height * options.align_tolerance;
    let left_aligned = (upper.left - lower.left).abs() <= tolerance;
    let centered = (upper.center_x() - lower.center_x()).abs() <= tolerance;
    if !overlaps || !(left_aligned || centered) {
        return false;
    }

    // Menü listesi değil paragraf mı: üst satır kaydırılacak kadar uzun ya da alt satır cümleye
    // küçük harfle devam ediyor.
    let upper_wrapped = upper.right - upper.left >= line_height * options.min_wrap_width;
    let continues_sentence = lower_text.chars().next().is_some_and(char::is_lowercase);
    upper_wrapped || continues_sentence
}

fn make_block(lines: &[&TextLine]) -> MergedBlock {
    let mut heights: Vec<f64> = lines.iter().map(|l| l.bbox.height).collect();
    heights.sort_by(f64::total_cmp);
    let median_height = heights[heights.len() / 2];
    let center_y = |line: &TextLine| line.bbox.y + line.bbox.height / 2.0;

    // Satırları yukarıdan aşağı satır gruplarına, her grubu soldan sağa diz.
    let mut sorted: Vec<&TextLine> = lines.to_vec();
    sorted.sort_by(|a, b| center_y(a).total_cmp(&center_y(b)));
    let mut rows: Vec<Vec<&TextLine>> = Vec::new();
    for line in sorted {
        match rows.last_mut() {
            Some(row) if (center_y(line) - center_y(row.last().unwrap())).abs() <= median_height * 0.5 => row.push(line),
            _ => rows.push(vec![line]),
        }
    }
    let row_texts: Vec<String> = rows
        .into_iter()
        .map(|mut row| {
            row.sort_by(|a, b| a.bbox.x.total_cmp(&b.bbox.x));
            row.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ")
        })
        .collect();

    let left = lines.iter().map(|l| l.bbox.x).fold(f64::INFINITY, f64::min);
    let top = lines.iter().map(|l| l.bbox.y).fold(f64::INFINITY, f64::min);
    let right = lines.iter().map(|l| l.bbox.right()).fold(f64::NEG_INFINITY, f64::max);
    let bottom = lines.iter().map(|l| l.bbox.bottom()).fold(f64::NEG_INFINITY, f64::max);
    MergedBlock { bbox: Rect::new(left, top, right - left, bottom - top), line_height: median_height, text: join_rows(&row_texts) }
}

/// Satırları boşlukla birleştirir; satır sonu tirelemesini ("trav-" + "eler") geri alır.
pub fn join_rows(rows: &[String]) -> String {
    let mut result = String::new();
    for row in rows {
        if result.is_empty() {
            result = row.clone();
        } else if result.ends_with('-') && row.chars().next().is_some_and(char::is_lowercase) {
            result.pop();
            result.push_str(row);
        } else {
            result.push(' ');
            result.push_str(row);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, x: f64, y: f64, w: f64, h: f64) -> TextLine {
        TextLine { text: text.into(), bbox: Rect::new(x, y, w, h), confidence: 0.9 }
    }

    fn texts(blocks: &[MergedBlock]) -> Vec<&str> {
        blocks.iter().map(|b| b.text.as_str()).collect()
    }

    #[test]
    fn soldan_hizali_cok_satirli_diyalog_tek_blok_olur() {
        let blocks = merge(&[
            line("You shouldn't have come here,", 0.20, 0.780, 0.50, 0.035),
            line("traveler. The gate is locked.", 0.20, 0.822, 0.46, 0.035),
        ]);

        assert_eq!(texts(&blocks), ["You shouldn't have come here, traveler. The gate is locked."]);
        let b = blocks[0].bbox;
        assert!((b.x - 0.20).abs() < 1e-9);
        assert!((b.y - 0.780).abs() < 1e-9);
        assert!((b.width - 0.50).abs() < 1e-9);
        assert!((b.height - 0.077).abs() < 1e-9);
        assert!((blocks[0].line_height - 0.035).abs() < 1e-9);
    }

    #[test]
    fn ortalanmis_uzun_altyazi_satirlari_birlesir() {
        let blocks = merge(&[
            line("I left the key by the old well near the", 0.27, 0.85, 0.46, 0.04),
            line("Village gate.", 0.43, 0.895, 0.14, 0.04),
        ]);

        assert_eq!(texts(&blocks), ["I left the key by the old well near the Village gate."]);
    }

    #[test]
    fn kucuk_harfle_devam_eden_kisa_satir_birlesir() {
        // Gerçek PS5 ana ekranı: "PS5 sisteminiz" / "kullanışlı hale".
        let blocks = merge(&[
            line("PS5 sisteminiz", 0.925, 0.563, 0.073, 0.022),
            line("kullanışlı hale", 0.925, 0.593, 0.073, 0.022),
        ]);

        assert_eq!(texts(&blocks), ["PS5 sisteminiz kullanışlı hale"]);
    }

    #[test]
    fn menu_ogeleri_ayri_kalir() {
        // Gerçek oyun menüsü (It Takes Two): alt alta, soldan hizalı kısa öğeler.
        let blocks = merge(&[
            line("Play Local", 0.115, 0.591, 0.087, 0.030),
            line("Play Online", 0.099, 0.655, 0.085, 0.028),
            line("Options", 0.099, 0.717, 0.061, 0.028),
            line("Accessibility Options", 0.099, 0.779, 0.165, 0.028),
            line("Credits", 0.099, 0.841, 0.057, 0.028),
        ]);

        assert_eq!(texts(&blocks), ["Play Local", "Play Online", "Options", "Accessibility Options", "Credits"]);
    }

    #[test]
    fn farkli_cumleyle_baslayan_kisa_altyazilar_ayri_kalir() {
        let blocks = merge(&[
            line("Where is the key?", 0.40, 0.85, 0.20, 0.04),
            line("I left it by the well.", 0.38, 0.895, 0.24, 0.04),
        ]);

        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn ayni_satirda_bolunmus_parcalar_soldan_saga_birlesir() {
        // Gerçek PS5 OCR çıktısından: "EA SPORTS FC™" + "27 Standart Sürüm PS4 ve PS5".
        let blocks = merge(&[
            line("27 Standart Sürüm PS4 ve PS5", 0.733, 0.715, 0.150, 0.019),
            line("EA SPORTS FC™", 0.654, 0.715, 0.074, 0.019),
        ]);

        assert_eq!(texts(&blocks), ["EA SPORTS FC™ 27 Standart Sürüm PS4 ve PS5"]);
    }

    #[test]
    fn uzaktaki_farkli_kartlar_ayri_kalir() {
        // PS5 ana ekranı: yan yana iki kart başlığı ve altlarındaki uzak satırlar.
        let blocks = merge(&[
            line("Kupalar", 0.106, 0.511, 0.060, 0.022),
            line("Konsol Depolama Alanı", 0.381, 0.510, 0.138, 0.028),
            line("Son Oynananlar", 0.104, 0.674, 0.104, 0.026),
        ]);

        assert_eq!(texts(&blocks), ["Konsol Depolama Alanı", "Kupalar", "Son Oynananlar"]);
    }

    #[test]
    fn boyutu_cok_farkli_satirlar_birlesmez() {
        let blocks = merge(&[line("CHAPTER ONE", 0.30, 0.40, 0.40, 0.08), line("The beginning", 0.30, 0.49, 0.20, 0.03)]);

        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn gecersiz_satirlar_elenir() {
        let blocks = merge(&[
            line("Hello there", 0.1, 0.1, 0.2, 0.03),
            TextLine { confidence: 0.1, ..line("Bulanık", 0.1, 0.5, 0.2, 0.03) },
            line("A", 0.5, 0.5, 0.01, 0.03),
            line("2,799.30", 0.7, 0.7, 0.1, 0.03),
        ]);

        assert_eq!(texts(&blocks), ["Hello there"]);
    }

    #[test]
    fn satir_sonu_tirelemesi_geri_alinir() {
        assert_eq!(join_rows(&["The trav-".into(), "eler waits.".into()]), "The traveler waits.");
        assert_eq!(join_rows(&["Mid-".into(), "Autumn".into()]), "Mid- Autumn");
    }
}
