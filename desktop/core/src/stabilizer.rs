use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

use crate::block_merger::MergedBlock;
use crate::geometry::Rect;

/// Ekranda gösterilmeye hazır, kimliği kareler boyunca sabit kalan blok.
#[derive(Clone, Debug, PartialEq)]
pub struct StableBlock {
    pub id: String,
    pub bbox: Rect,
    pub line_height: f64,
    /// Kabul edilmiş (çeviriye gidecek) kaynak metin.
    pub text: String,
}

/// Karelerden gelen blokları zaman içinde takip eder:
/// - Metin en az `min_stable_frames` kare ve `min_stable_seconds` saniye değişmeden kalınca
///   "hazır" olur (daktilo efektiyle harf harf beliren metin, yazımı bitmeden çevrilmez). Süre
///   şartı sayesinde tur sıklığı artınca bekleme kısalır ama yarım metin kabul edilmez.
/// - OCR'ın tek karelik harf oynamaları ("Kupalar" / "Kupalar.") metni değiştirmiş sayılmaz.
/// - Ekrandaki bir bloğun metni değişirse, yeni metin oturana kadar eski metin gösterilmeye
///   devam eder (diyalog geçişlerinde ve OCR oynamalarında boşluk/titreme olmaz).
/// - Bir kare görünmeyen blok `hold_seconds` boyunca tutulur; titreme olmaz.
/// - OCR kutusunun kareden kareye birkaç piksellik oynaması gösterilen kutuyu kıpırdatmaz;
///   kutu ancak gerçekten yer değiştirince güncellenir.
#[derive(Clone, Debug)]
pub struct Stabilizer {
    pub min_stable_frames: u32,
    pub min_stable_seconds: f64,
    pub hold_seconds: f64,
    /// Kutunun bir kenarı satır yüksekliğinin bu katından az oynarsa gösterilen kutu değişmez.
    pub box_tolerance: f64,
    /// Yeni blok, takip edilen blokla en az bu kadar örtüşürse (IoU) aynı blok sayılır.
    pub min_overlap: f64,
    tracked: Vec<Tracked>,
    next_id: u64,
}

#[derive(Clone, Debug)]
struct Tracked {
    id: String,
    block: MergedBlock,
    /// Gösterilen kutu ve satır yüksekliği: OCR oynamalarından arındırılmış.
    shown_bbox: Rect,
    shown_line_height: f64,
    /// Gösterilen metin; henüz hiçbir metin oturmadıysa `None`.
    accepted_text: Option<String>,
    /// Oturması beklenen metin ve kaç karedir aynı kaldığı.
    candidate_text: String,
    candidate_frames: u32,
    candidate_since: f64,
    last_seen: f64,
}

impl Tracked {
    fn observe(&mut self, text: &str, now: f64, min_stable_frames: u32, min_stable_seconds: f64) {
        if let Some(accepted) = &self.accepted_text {
            if same_text(accepted, text) {
                self.candidate_text = accepted.clone();
                self.candidate_frames = min_stable_frames;
                return;
            }
        }
        if self.candidate_frames > 0 && same_text(&self.candidate_text, text) {
            self.candidate_frames += 1;
        } else {
            self.candidate_text = text.to_string();
            self.candidate_frames = 1;
            self.candidate_since = now;
        }
        // Küçük pay: tur aralığı tam süreye denk geldiğinde kayan nokta hatası bir tur kaybettirmesin.
        if self.candidate_frames >= min_stable_frames && now - self.candidate_since >= min_stable_seconds - 1e-6 {
            self.accepted_text = Some(self.candidate_text.clone());
        }
    }

    /// Yeni OCR kutusunu alır; gösterilen kutu yalnızca belirgin bir kayma varsa güncellenir.
    fn place(&mut self, block: &MergedBlock, tolerance: f64) {
        let (old, new) = (self.shown_bbox, block.bbox);
        let limit = block.line_height.max(self.shown_line_height) * tolerance;
        // x ekseni normalize birimde y'den ~16/9 kat "kısa"; aynı piksel toleransı için daraltılır.
        let limit_x = limit / crate::geometry::VIDEO_ASPECT;
        let moved = (old.x - new.x).abs() > limit_x
            || (old.right() - new.right()).abs() > limit_x
            || (old.y - new.y).abs() > limit
            || (old.bottom() - new.bottom()).abs() > limit;
        if moved {
            self.shown_bbox = new;
            self.shown_line_height = block.line_height;
        }
        self.block = block.clone();
    }
}

impl Default for Stabilizer {
    fn default() -> Stabilizer {
        Stabilizer { min_stable_frames: 2, min_stable_seconds: 0.3, hold_seconds: 0.5, box_tolerance: 0.25, min_overlap: 0.3, tracked: Vec::new(), next_id: 1 }
    }
}

impl Stabilizer {
    pub fn new() -> Stabilizer {
        Stabilizer::default()
    }

    pub fn update(&mut self, blocks: &[MergedBlock], now: f64) -> Vec<StableBlock> {
        let mut matched = vec![false; self.tracked.len()];
        for block in blocks {
            // En çok örtüşen eşleşmemiş blok; eşitlikte ilk bulunan (Swift `max(by:)` gibi).
            let mut best: Option<(usize, f64)> = None;
            for (index, item) in self.tracked.iter().enumerate() {
                if matched[index] {
                    continue;
                }
                let overlap = iou(item.block.bbox, block.bbox);
                if overlap >= self.min_overlap && best.is_none_or(|(_, b)| overlap > b) {
                    best = Some((index, overlap));
                }
            }

            if let Some((index, _)) = best {
                matched[index] = true;
                let item = &mut self.tracked[index];
                item.place(block, self.box_tolerance);
                item.last_seen = now;
                item.observe(&block.text, now, self.min_stable_frames, self.min_stable_seconds);
            } else {
                let mut item = Tracked {
                    id: format!("b{}", self.next_id),
                    block: block.clone(),
                    shown_bbox: block.bbox,
                    shown_line_height: block.line_height,
                    accepted_text: None,
                    candidate_text: block.text.clone(),
                    candidate_frames: 0,
                    candidate_since: now,
                    last_seen: now,
                };
                item.observe(&block.text, now, self.min_stable_frames, self.min_stable_seconds);
                self.tracked.push(item);
                matched.push(true);
                self.next_id += 1;
            }
        }

        let hold = self.hold_seconds;
        self.tracked.retain(|item| now - item.last_seen <= hold);
        self.tracked
            .iter()
            .filter_map(|item| {
                item.accepted_text.as_ref().map(|text| StableBlock {
                    id: item.id.clone(),
                    bbox: item.shown_bbox,
                    line_height: item.shown_line_height,
                    text: text.clone(),
                })
            })
            .collect()
    }
}

/// Noktalama/boşluk/büyük-küçük harf farkı ve aynı uzunlukta tek tük harf oynaması aynı
/// metin sayılır. Uzunluk değişirse (daktilo efekti) farklı metindir.
pub fn same_text(a: &str, b: &str) -> bool {
    let left: Vec<char> = normalize(a).chars().collect();
    let right: Vec<char> = normalize(b).chars().collect();
    if left == right {
        return true;
    }
    if left.len() != right.len() || left.is_empty() {
        return false;
    }
    let differences = left.iter().zip(&right).filter(|(x, y)| x != y).count();
    differences <= (left.len() / 20).max(1)
}

/// Türkçe kurallarla: "KAPI KİLİTLİ" ile "kapı kilitli" aynı sayılır (I→ı, İ→i).
pub fn normalize(text: &str) -> String {
    // Önce küçük harf: aksan katlaması önce yapılırsa İ noktasını kaybedip ı olur.
    let lowered: String = text
        .chars()
        .flat_map(|c| match c {
            'I' => vec!['ı'],
            'İ' => vec!['i'],
            other => other.to_lowercase().collect(),
        })
        .collect();
    let folded: String = lowered.nfd().filter(|c| !is_combining_mark(*c)).filter(|c| c.is_alphanumeric() || *c == ' ').collect();
    folded.split(' ').filter(|word| !word.is_empty()).collect::<Vec<_>>().join(" ")
}

fn iou(a: Rect, b: Rect) -> f64 {
    let width = a.right().min(b.right()) - a.x.max(b.x);
    let height = a.bottom().min(b.bottom()) - a.y.max(b.y);
    if width <= 0.0 || height <= 0.0 {
        return 0.0;
    }
    let intersection = width * height;
    intersection / (a.width * a.height + b.width * b.height - intersection)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn block(text: &str) -> MergedBlock {
        block_at(text, 0.8)
    }

    fn block_at(text: &str, y: f64) -> MergedBlock {
        MergedBlock { bbox: Rect::new(0.2, y, 0.5, 0.06), line_height: 0.03, text: text.into() }
    }

    fn texts(blocks: &[StableBlock]) -> Vec<&str> {
        blocks.iter().map(|b| b.text.as_str()).collect()
    }

    #[test]
    fn tek_karede_gorulen_metin_henuz_gosterilmez() {
        let mut stabilizer = Stabilizer::new();
        assert!(stabilizer.update(&[block("Door is locked")], 0.0).is_empty());
    }

    #[test]
    fn iki_kare_ayni_kalan_metin_gosterilir() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block("Door is locked")], 0.0);

        assert_eq!(texts(&stabilizer.update(&[block("Door is locked")], 0.5)), ["Door is locked"]);
    }

    #[test]
    fn daktilo_efektinde_metin_bitmeden_gosterilmez() {
        let mut stabilizer = Stabilizer::new();
        assert!(stabilizer.update(&[block("You sho")], 0.0).is_empty());
        assert!(stabilizer.update(&[block("You shouldn't")], 0.5).is_empty());
        assert!(stabilizer.update(&[block("You shouldn't be here.")], 1.0).is_empty());

        assert_eq!(texts(&stabilizer.update(&[block("You shouldn't be here.")], 1.5)), ["You shouldn't be here."]);
    }

    #[test]
    fn ocr_harf_oynamasi_metni_degistirmez() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block("The gate is locked.")], 0.0);
        stabilizer.update(&[block("The gate is locked")], 0.5);

        // Tek harf oynaması: "1" / "l". Kabul edilen metin ilk okunduğu gibi kalır.
        assert_eq!(texts(&stabilizer.update(&[block("The gate is 1ocked.")], 1.0)), ["The gate is locked."]);
    }

    #[test]
    fn yeni_metin_oturana_kadar_eski_metin_gosterilir() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block("Hello, traveler.")], 0.0);
        stabilizer.update(&[block("Hello, traveler.")], 0.5);

        // Diyalog değişiyor: yeni satır bir kare görüldü, henüz oturmadı.
        assert_eq!(texts(&stabilizer.update(&[block("The gate is locked.")], 1.0)), ["Hello, traveler."]);
        assert_eq!(texts(&stabilizer.update(&[block("The gate is locked.")], 1.5)), ["The gate is locked."]);
    }

    #[test]
    fn gosterilen_metin_tek_karelik_ocr_hatasinda_kaybolmaz() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block("Konsol Depolama Alanı")], 0.0);
        stabilizer.update(&[block("Konsol Depolama Alanı")], 0.5);

        assert_eq!(texts(&stabilizer.update(&[block("Konsol Depolama")], 1.0)), ["Konsol Depolama Alanı"]);
        assert_eq!(texts(&stabilizer.update(&[block("Konsol Depolama Alanı")], 1.5)), ["Konsol Depolama Alanı"]);
    }

    #[test]
    fn kimlik_kareler_boyunca_sabit_kalir() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block_at("Hello", 0.80)], 0.0);
        let first = stabilizer.update(&[block_at("Hello", 0.801)], 0.5);
        let second = stabilizer.update(&[block_at("Hello", 0.799)], 1.0);

        assert_eq!(first.iter().map(|b| &b.id).collect::<Vec<_>>(), second.iter().map(|b| &b.id).collect::<Vec<_>>());
    }

    #[test]
    fn bir_kare_kaybolan_blok_tutulur() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block("Hello")], 0.0);
        stabilizer.update(&[block("Hello")], 0.5);

        assert_eq!(texts(&stabilizer.update(&[], 0.9)), ["Hello"]);
        assert!(stabilizer.update(&[], 1.1).is_empty());
    }

    #[test]
    fn hizli_turlarda_sure_dolmadan_metin_kabul_edilmez() {
        let mut stabilizer = Stabilizer::new();
        // Saniyede 5 tur: iki kare yetmez, metin 0.3 sn aynı kalmalı.
        assert!(stabilizer.update(&[block("Door is locked")], 0.0).is_empty());
        assert!(stabilizer.update(&[block("Door is locked")], 0.2).is_empty());

        assert_eq!(texts(&stabilizer.update(&[block("Door is locked")], 0.4)), ["Door is locked"]);
    }

    #[test]
    fn hizli_turlarda_daktilo_metni_bitince_kabul_edilir() {
        let mut stabilizer = Stabilizer::new();
        assert!(stabilizer.update(&[block("You sho")], 0.0).is_empty());
        assert!(stabilizer.update(&[block("You shouldn't")], 0.2).is_empty());
        assert!(stabilizer.update(&[block("You shouldn't be here.")], 0.4).is_empty());
        assert!(stabilizer.update(&[block("You shouldn't be here.")], 0.6).is_empty());

        assert_eq!(texts(&stabilizer.update(&[block("You shouldn't be here.")], 0.8)), ["You shouldn't be here."]);
    }

    #[test]
    fn kutunun_kucuk_oynamasi_gosterilen_kutuyu_degistirmez() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block_at("Hello", 0.800)], 0.0);
        stabilizer.update(&[block_at("Hello", 0.802)], 0.5);

        // Satır yüksekliği 0.03: 0.002'lik oynama toleransın (0.0075) altında.
        let shown = stabilizer.update(&[block_at("Hello", 0.798)], 1.0);
        assert_eq!(shown[0].bbox.y, 0.800);

        // Gerçek kayma (altyazı bir satır yukarı çıktı) hemen izlenir.
        let moved = stabilizer.update(&[block_at("Hello", 0.770)], 1.5);
        assert_eq!(moved[0].bbox.y, 0.770);
    }

    #[test]
    fn farkli_konumdaki_bloklar_ayri_takip_edilir() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block_at("Top", 0.1), block_at("Bottom", 0.8)], 0.0);

        let shown = stabilizer.update(&[block_at("Top", 0.1), block_at("Bottom", 0.8)], 0.5);

        assert_eq!(shown.iter().map(|b| b.text.as_str()).collect::<HashSet<_>>(), HashSet::from(["Top", "Bottom"]));
        assert_eq!(shown.iter().map(|b| &b.id).collect::<HashSet<_>>().len(), 2);
    }

    #[test]
    fn normalizasyon_noktalama_ve_buyuk_harfi_yok_sayar() {
        assert!(same_text("KAPI KİLİTLİ!", "kapı  kilitli"));
        assert!(!same_text("Hello", "Hello there"));
    }

    #[test]
    fn normalizasyon_turkce_harfleri_katlar() {
        assert_eq!(normalize("KAPI KİLİTLİ!"), "kapı kilitli");
        assert_eq!(normalize("Çığ  düştü, Şükrü."), "cıg dustu sukru");
    }
}
