//! Ekrandaki yazıyı türüne ayırır: altyazı/açıklama (cümle), menü öğesi ya da "diğer" (logo,
//! tabela, dükkan adı gibi oyun dünyasına ait yazı). Odak modunda yalnızca ilk ikisi çevrilir;
//! diğerlerinin üstüne çizilmez, görüntü kapanmaz.
//!
//! Kesin bir tanıma değil, sezgisel kurallardır. Ayırt edici işaret harekettir: arayüz yazısı
//! (menü, düğme ipucu, altyazı) ekranda sabit durur, oyun dünyasındaki yazı kamerayla kayar.
//! - Cümle gibi duran metin (birkaç kelime ya da cümle noktalaması) altyazıdır.
//! - Altyazı bölgesindeki (ekranın alt-orta kısmı) kısa metin de altyazıdır ("Yeah.", "Tommy!").
//! - Diğer kısa metin, bir süredir yerinden oynamıyorsa arayüzdür (menü öğesi, "Resume" ipucu).
//!   Hizalı bir listenin parçasıysa daha kısa beklenir.
//! - Çok büyük yazı (oyun logosu, bölüm başlığı) ve ona bitişik küçük yazı (logonun alt satırı)
//!   atlanır.
//!
//! Oyuncu dururken sabit kalan bir tabela arayüzden ayırt edilemez; bekleme süresi dolunca çevrilir.

use crate::geometry::Rect;
use crate::stabilizer::StableBlock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Altyazı, görev açıklaması, ipucu: cümle.
    Dialogue,
    /// Kısa arayüz yazısı: menü öğesi, düğme ipucu.
    Menu,
    /// Logo, tabela, tek başına etiket: çevrilmez.
    Other,
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Satır yüksekliği (ekran yüksekliğine oran) bundan büyükse logo/başlık sayılır.
    pub max_line_height: f64,
    /// Bu kadar kelime ve üstü cümle sayılır.
    pub sentence_words: usize,
    /// Bu kadar öğe (kendisi dahil) hizalıysa liste sayılır.
    pub min_list_items: usize,
    /// Listedeki kısa metin en az bu kadar süredir yerinde duruyor olmalı.
    pub list_still_seconds: f64,
    /// Tek başına duran kısa metin en az bu kadar süredir yerinde duruyor olmalı (kamerayla kayan
    /// tabela elenir).
    pub lone_still_seconds: f64,
    /// Altyazı bölgesi: kutunun merkezi ekranın bu oranından aşağıda ve yatayda ortaya bu kadar yakın.
    pub subtitle_top: f64,
    pub subtitle_center: f64,
    /// Liste öğelerinin satır yükseklikleri oranı (büyük / küçük) en fazla bu kadar olabilir.
    pub max_height_ratio: f64,
    /// Alt alta iki öğe arasındaki boşluk, satır yüksekliğinin en fazla bu katı olabilir.
    pub max_item_gap: f64,
    /// Sol kenar ya da merkez hizası için tolerans (satır yüksekliği cinsinden).
    pub align_tolerance: f64,
    /// x koordinatlarını y ile aynı birime çevirmek için video en-boy oranı.
    pub aspect: f64,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            max_line_height: 0.07,
            sentence_words: 4,
            min_list_items: 3,
            list_still_seconds: 0.4,
            lone_still_seconds: 1.0,
            subtitle_top: 0.6,
            subtitle_center: 0.12,
            max_height_ratio: 1.3,
            max_item_gap: 3.5,
            align_tolerance: 1.0,
            aspect: crate::geometry::VIDEO_ASPECT,
        }
    }
}

/// `blocks` ile aynı sırada tür listesi.
pub fn classify(blocks: &[StableBlock], options: &Options) -> Vec<Kind> {
    let short: Vec<bool> = blocks.iter().map(|b| !is_sentence(&b.text, options)).collect();
    let huge: Vec<&StableBlock> = blocks.iter().filter(|b| b.line_height > options.max_line_height).collect();
    blocks
        .iter()
        .enumerate()
        .map(|(index, block)| {
            if block.line_height > options.max_line_height || huge.iter().any(|logo| attached(block, logo)) {
                return Kind::Other;
            }
            if !short[index] {
                return Kind::Dialogue;
            }
            if !is_plain_latin(&block.text) {
                return Kind::Other;
            }
            if in_subtitle_zone(block.bbox, options) && block.still_seconds >= options.list_still_seconds * 0.5 {
                return Kind::Dialogue;
            }
            let candidates: Vec<&StableBlock> =
                blocks.iter().enumerate().filter(|(other, b)| short[*other] && similar_height(block, b, options)).map(|(_, b)| b).collect();
            let listed = column_size(block, &candidates, options) >= options.min_list_items || row_size(block, &candidates, options) >= options.min_list_items;
            let needed = if listed { options.list_still_seconds } else { options.lone_still_seconds };
            if block.still_seconds >= needed {
                Kind::Menu
            } else {
                Kind::Other
            }
        })
        .collect()
}

fn in_subtitle_zone(bbox: Rect, options: &Options) -> bool {
    let center_x = bbox.x + bbox.width / 2.0;
    let center_y = bbox.y + bbox.height / 2.0;
    center_y >= options.subtitle_top && (center_x - 0.5).abs() <= options.subtitle_center
}

/// Küçük yazı büyük bir logonun hemen altında/üstünde ve onunla yatayda örtüşüyor mu.
fn attached(block: &StableBlock, logo: &StableBlock) -> bool {
    if block.id == logo.id {
        return false;
    }
    let (a, b) = (block.bbox, logo.bbox);
    let overlaps = a.right().min(b.right()) > a.x.max(b.x);
    let gap = (a.y - b.bottom()).max(b.y - a.bottom());
    overlaps && gap <= block.line_height * 1.5
}

/// Birkaç kelime ya da cümle noktalamasıyla biten en az iki kelime.
fn is_sentence(text: &str, options: &Options) -> bool {
    let words = text.split_whitespace().filter(|w| w.chars().any(char::is_alphabetic)).count();
    if words >= options.sentence_words {
        return true;
    }
    let ends_like_sentence = text.trim_end_matches(['"', '\'', '”', '’', ')']).ends_with(['.', '!', '?', '…']);
    words >= 2 && (ends_like_sentence || text.contains(", "))
}

/// OCR stilize yazıyı bazen Kiril/Yunan harfleriyle okur ("map" → "тар"); bunlar çevrilemez.
fn is_plain_latin(text: &str) -> bool {
    text.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_ascii() || ('\u{00C0}'..='\u{024F}').contains(&c))
}

fn similar_height(a: &StableBlock, b: &StableBlock, options: &Options) -> bool {
    let (small, large) = (a.line_height.min(b.line_height), a.line_height.max(b.line_height));
    small > 0.0 && large / small <= options.max_height_ratio
}

fn aligned(a: Rect, b: Rect, tolerance: f64, aspect: f64) -> bool {
    let left = (a.x - b.x).abs() * aspect <= tolerance;
    let center = ((a.x + a.width / 2.0) - (b.x + b.width / 2.0)).abs() * aspect <= tolerance;
    left || center
}

/// `block`'un içinde olduğu, alt alta hizalı öğe zincirinin uzunluğu.
fn column_size(block: &StableBlock, candidates: &[&StableBlock], options: &Options) -> usize {
    let tolerance = block.line_height * options.align_tolerance;
    let mut column: Vec<&StableBlock> = candidates.iter().copied().filter(|b| aligned(block.bbox, b.bbox, tolerance, options.aspect)).collect();
    column.sort_by(|a, b| a.bbox.y.total_cmp(&b.bbox.y));
    chain_size(block, &column, |upper, lower| {
        let gap = lower.bbox.y - upper.bbox.bottom();
        gap > -block.line_height * 0.3 && gap <= block.line_height * options.max_item_gap
    })
}

/// `block`'un içinde olduğu, aynı satırda yan yana duran öğe zincirinin uzunluğu (sekme çubuğu).
fn row_size(block: &StableBlock, candidates: &[&StableBlock], options: &Options) -> usize {
    let center_y = |b: &StableBlock| b.bbox.y + b.bbox.height / 2.0;
    let mut row: Vec<&StableBlock> = candidates.iter().copied().filter(|b| (center_y(b) - center_y(block)).abs() <= block.line_height * 0.3).collect();
    row.sort_by(|a, b| a.bbox.x.total_cmp(&b.bbox.x));
    chain_size(block, &row, |left, right| {
        let gap = (right.bbox.x - left.bbox.right()) * options.aspect;
        gap > 0.0 && gap <= block.line_height * 8.0
    })
}

/// Sıralı listede, ardışık öğeleri `linked` olan ve `block`'u içeren kesintisiz parçanın uzunluğu.
fn chain_size(block: &StableBlock, sorted: &[&StableBlock], linked: impl Fn(&StableBlock, &StableBlock) -> bool) -> usize {
    let Some(position) = sorted.iter().position(|b| b.id == block.id) else {
        return 0;
    };
    let mut size = 1;
    let mut index = position;
    while index > 0 && linked(sorted[index - 1], sorted[index]) {
        index -= 1;
        size += 1;
    }
    index = position;
    while index + 1 < sorted.len() && linked(sorted[index], sorted[index + 1]) {
        index += 1;
        size += 1;
    }
    size
}

/// Çevirinin, orijinal yazıyı kapatmadan gösterileceği kutu: hemen altı; altta yer yoksa hemen üstü.
pub fn beside(bbox: Rect, line_height: f64) -> Rect {
    let gap = line_height * 0.35;
    let below = bbox.bottom() + gap;
    let y = if below + bbox.height <= 0.985 { below } else { (bbox.y - gap - bbox.height).max(0.0) };
    Rect::new(bbox.x, y, bbox.width, bbox.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str, text: &str, x: f64, y: f64, w: f64, h: f64) -> StableBlock {
        StableBlock { id: id.into(), bbox: Rect::new(x, y, w, h), line_height: h, text: text.into(), still_seconds: 5.0 }
    }

    fn kinds(blocks: &[StableBlock]) -> Vec<Kind> {
        classify(blocks, &Options::default())
    }

    /// Gerçek kare: GTA Vice City duraklatma menüsü (960x540 Remote Play penceresi, Vision OCR).
    fn gta_menu() -> Vec<StableBlock> {
        vec![
            block("b1", "Prand", 0.036, 0.061, 0.169, 0.136),
            block("b2", "thert", 0.071, 0.163, 0.156, 0.086),
            block("b3", "Vies cuy", 0.033, 0.311, 0.181, 0.096),
            block("b4", "THE DEFINITIVE EDITION", 0.044, 0.411, 0.165, 0.022),
            block("b5", "Resume", 0.094, 0.537, 0.067, 0.030),
            block("b6", "Map", 0.108, 0.596, 0.035, 0.030),
            block("b7", "Briefs", 0.100, 0.719, 0.052, 0.030),
            block("b8", "Options", 0.094, 0.837, 0.067, 0.030),
            block("b9", "Social Club", 0.079, 0.892, 0.094, 0.035),
        ]
    }

    #[test]
    fn menude_logo_atlanir_ogeler_cevrilir() {
        use Kind::*;
        assert_eq!(kinds(&gta_menu()), [Other, Other, Other, Other, Menu, Menu, Menu, Menu, Menu]);
    }

    #[test]
    fn altyazi_cumlesi_diyalogdur() {
        let subtitle = block("s", "You shouldn't be here.", 0.3, 0.85, 0.4, 0.035);
        let long = block("l", "I think we should split up", 0.3, 0.80, 0.4, 0.035);
        assert_eq!(kinds(&[subtitle, long]), [Kind::Dialogue, Kind::Dialogue]);
    }

    #[test]
    fn kamerayla_kayan_tabela_ve_dukkan_adi_atlanir() {
        let mut blocks = [
            block("a", "Ocean View Hotel", 0.2, 0.3, 0.2, 0.04),
            block("b", "PIZZA", 0.7, 0.5, 0.08, 0.05),
            block("c", "Is anybody out there?", 0.3, 0.85, 0.4, 0.035),
        ];
        blocks[0].still_seconds = 0.4;
        blocks[1].still_seconds = 0.0;
        assert_eq!(kinds(&blocks), [Kind::Other, Kind::Other, Kind::Dialogue]);
    }

    #[test]
    fn tek_basina_duran_ipucu_bir_saniye_sabit_kalinca_cevrilir() {
        // Gerçek kare: duraklatılmış ara sahnede sağ altta "Resume ⊗".
        let mut prompt = [block("p", "ReSume x", 0.718, 0.848, 0.142, 0.041)];
        assert_eq!(kinds(&prompt), [Kind::Menu]);
        prompt[0].still_seconds = 0.6;
        assert_eq!(kinds(&prompt), [Kind::Other]);
    }

    #[test]
    fn altyazi_bolgesindeki_kisa_replik_diyalogdur() {
        let mut line = [block("s", "Yeah.", 0.46, 0.86, 0.08, 0.035)];
        line[0].still_seconds = 0.4;
        assert_eq!(kinds(&line), [Kind::Dialogue]);
    }

    #[test]
    fn kamerayla_kayan_kisa_yazilar_menu_sayilmaz() {
        let mut menu = gta_menu();
        for item in &mut menu {
            item.still_seconds = 0.2;
        }
        // Liste yarım saniyeye yakın durunca çevrilir; tek başına yazıdan daha çabuk.
        let mut settled = gta_menu();
        for item in &mut settled {
            item.still_seconds = 0.5;
        }
        assert_eq!(kinds(&settled)[4], Kind::Menu);
        assert!(kinds(&menu).iter().all(|k| *k == Kind::Other));
    }

    #[test]
    fn yan_yana_sekmeler_menudur() {
        let tabs = [
            block("a", "Map", 0.10, 0.05, 0.04, 0.03),
            block("b", "Inventory", 0.20, 0.05, 0.10, 0.03),
            block("c", "Skills", 0.36, 0.05, 0.06, 0.03),
        ];
        assert_eq!(kinds(&tabs), [Kind::Menu, Kind::Menu, Kind::Menu]);
    }

    #[test]
    fn iki_ogeli_grup_menudur_kiril_okunan_yazi_degildir() {
        let pair = [block("a", "Yes", 0.4, 0.5, 0.04, 0.03), block("b", "No", 0.4, 0.56, 0.03, 0.03)];
        assert_eq!(kinds(&pair), [Kind::Menu, Kind::Menu]);

        let mut menu = gta_menu();
        menu[5].text = "тар".into();
        assert_eq!(kinds(&menu)[5], Kind::Other);
        assert_eq!(kinds(&menu)[4], Kind::Menu);
    }

    #[test]
    fn ceviri_altyazinin_altina_yer_yoksa_ustune_konur() {
        let middle = beside(Rect::new(0.3, 0.70, 0.4, 0.04), 0.04);
        assert!((middle.y - 0.754).abs() < 1e-9 && middle.x == 0.3 && middle.height == 0.04);

        // İki satırlık altyazı ekranın dibinde: altı taşar, çeviri üstüne çıkar.
        let bottom = beside(Rect::new(0.3, 0.86, 0.4, 0.09), 0.04);
        assert!((bottom.y - (0.86 - 0.014 - 0.09)).abs() < 1e-9);
    }
}
