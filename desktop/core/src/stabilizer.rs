use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

use serde::Serialize;

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
    /// Kutunun kaç saniyedir yerinden oynamadığı (menü yazısı durur, tabela kamerayla kayar).
    pub still_seconds: f64,
    /// Kabul edilen metnin OCR güveni (0...1).
    pub confidence: f64,
}

/// Bir altyazı adayının yaşam döngüsündeki yeri (stabilizasyon kısmı). Çeviri ve gösterim
/// aşamaları (`Translating`, `Displayed`…) iş hattında eklenir; burada yalnızca okuma tarafı vardır.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CandidateState {
    /// İlk kez görüldü (tek kare).
    Tracking,
    /// Metin değişti ya da henüz oturmadı; bekleniyor.
    Stabilizing,
    /// Metin oturdu, çeviriye hazır.
    Ready,
}

/// Teşhis için tek adayın durumu (oyun metni içermez).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateInfo {
    pub id: String,
    pub state: CandidateState,
    /// Takibin ilk görüldüğü andan beri geçen süre (sn).
    pub age_s: f64,
    /// Bekleyen adayın kaç karedir aynı kaldığı.
    pub frames: u32,
    /// Oturmaya ne kadar yakın: geçen süre / gereken süre (0...1).
    pub stability: f64,
    /// Bu aday için gereken bekleme (sn): uyarlamalı.
    pub required_s: f64,
    pub confidence: f64,
    /// Son gözlem öncekinin devamı mı (daktilo efekti, yarım altyazı).
    pub growing: bool,
    /// Son değişimde eski ve yeni metnin benzerliği (0...1).
    pub similarity: f64,
    /// Kutunun ekrandaki yeri (normalize).
    pub region: Rect,
}

/// Oturum boyunca biriken sayaçlar (teşhis).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleStats {
    /// Açılan takip (aday) sayısı.
    pub candidates: u64,
    /// Oturup kabul edilen metin sayısı.
    pub accepted: u64,
    /// Hiç oturmadan ekrandan kalkan aday sayısı (yarım/geçici metin, hızlı diyalog).
    pub dropped: u64,
    /// OCR oynaması yüzünden farklı okunan ama aynı sayılan gözlem sayısı.
    pub duplicates: u64,
    /// Önceki metnin devamı olarak gelen (büyüyen) gözlem sayısı.
    pub growths: u64,
    /// Kutusu büyüdüğü için IoU'su düşük olsa da metin devamı sayesinde aynı takipte kalan blok.
    pub continued: u64,
    /// Hızlı yoldan (cümle sonu + yüksek güven) kabul edilen metin sayısı.
    pub fast_accepts: u64,
    /// Tek karede (tamamlanmış ve çok net) kabul edilen metin sayısı.
    pub instant_accepts: u64,
}

/// Karelerden gelen blokları zaman içinde takip eder:
/// - Metin en az `min_stable_frames` kare ve gereken süre boyunca değişmeden kalınca "hazır" olur
///   (daktilo efektiyle harf harf beliren metin, yazımı bitmeden çevrilmez). Süre şartı sayesinde
///   tur sıklığı artınca bekleme kısalır ama yarım metin kabul edilmez.
/// - Gereken süre uyarlamalıdır (`required`): önceki metnin devamı olarak büyüyen ve cümle sonu
///   olmayan metin daha uzun, cümle sonu olan ve güveni yüksek metin daha kısa bekler; düşük güven
///   bir kare daha ister. Üst sınır `max_stabilize_seconds`: hiçbir metin sonsuza dek beklemez.
/// - OCR'ın tek karelik harf oynamaları ("Kupalar" / "Kupalar.") metni değiştirmiş sayılmaz.
/// - Ekrandaki bir bloğun metni değişirse, yeni metin oturana kadar eski metin gösterilmeye
///   devam eder (diyalog geçişlerinde ve OCR oynamalarında boşluk/titreme olmaz).
/// - Bir kare görünmeyen blok `hold_seconds` boyunca tutulur; titreme olmaz.
/// - OCR kutusunun kareden kareye birkaç piksellik oynaması gösterilen kutuyu kıpırdatmaz;
///   kutu ancak gerçekten yer değiştirince güncellenir.
/// - Metin büyüdükçe kutusu genişler (IoU düşer); aynı satır bandında ve metin öncekinin devamıysa
///   aynı takipte kalır.
#[derive(Clone, Debug)]
pub struct Stabilizer {
    pub min_stable_frames: u32,
    pub min_stable_seconds: f64,
    pub hold_seconds: f64,
    /// Kutunun bir kenarı satır yüksekliğinin bu katından az oynarsa gösterilen kutu değişmez.
    pub box_tolerance: f64,
    /// Yeni blok, takip edilen blokla en az bu kadar örtüşürse (IoU) aynı blok sayılır.
    pub min_overlap: f64,
    /// Hızlı yol: cümle sonu olan, güveni yüksek, büyümeyen metin için gereken süre.
    pub fast_seconds: f64,
    /// Önceki metnin devamı olarak büyüyen, cümle sonu olmayan metne eklenen süre.
    pub growth_extra_seconds: f64,
    /// Bu oyunda daktilo efekti görüldüyse noktalamasız çok kelimeli metne eklenen süre.
    pub typewriter_extra_seconds: f64,
    /// Bu güvenin altındaki metin bir kare ve `low_confidence_extra_seconds` daha bekler.
    pub low_confidence: f64,
    pub low_confidence_extra_seconds: f64,
    /// Hızlı yol için gereken güven.
    pub high_confidence: f64,
    /// Tek karede kabul için gereken güven: cümle sonu olan, büyümeyen, yeni metin; bu oyunda
    /// daktilo efekti görülmediyse ikinci kare beklenmez (çok hızlı altyazılar kaybolmasın).
    pub instant_confidence: f64,
    /// Gereken sürenin üst sınırı.
    pub max_stabilize_seconds: f64,
    tracked: Vec<Tracked>,
    next_id: u64,
    /// Son metin değişimlerinin ne kadarı büyüme (0...1, üstel ortalama): oyunun altyazı davranışı.
    growth_score: f64,
    stats: SubtitleStats,
}

#[derive(Clone, Debug)]
struct Tracked {
    id: String,
    block: MergedBlock,
    /// Gösterilen kutu ve satır yüksekliği: OCR oynamalarından arındırılmış.
    shown_bbox: Rect,
    shown_line_height: f64,
    /// Kutunun en son yer değiştirdiği (ya da ilk görüldüğü) an.
    still_since: f64,
    first_seen: f64,
    /// Gösterilen metin; henüz hiçbir metin oturmadıysa `None`.
    accepted_text: Option<String>,
    accepted_confidence: f64,
    /// Oturması beklenen metin ve kaç karedir aynı kaldığı.
    candidate_text: String,
    candidate_frames: u32,
    candidate_since: f64,
    candidate_confidence: f64,
    /// Aday, bir önceki adayın (ya da gösterilen metnin) devamı mı.
    candidate_growth: bool,
    /// Son değişimdeki benzerlik.
    similarity: f64,
    last_seen: f64,
}

/// Uyarlamalı beklemenin girdileri.
struct Params {
    min_frames: u32,
    min_seconds: f64,
    fast_seconds: f64,
    growth_extra: f64,
    typewriter_extra: f64,
    typewriter_seen: bool,
    low_confidence: f64,
    low_extra: f64,
    high_confidence: f64,
    instant_confidence: f64,
    max_seconds: f64,
}

/// Bir gözlemin sonucu (sayaçlar için).
#[derive(PartialEq)]
enum Observed {
    Same,
    /// Farklı okundu ama OCR oynaması sayıldı.
    Duplicate,
    Changed { growth: bool },
}

impl Tracked {
    /// Gereken kare sayısı ve süre (sn).
    fn required(&self, p: &Params) -> (u32, f64) {
        let text = &self.candidate_text;
        let terminated = ends_sentence(text);
        let mut frames = p.min_frames;
        let mut seconds = p.min_seconds;
        if self.candidate_growth && !terminated {
            seconds += p.growth_extra;
        } else if !terminated && p.typewriter_seen && word_count(text) >= 4 {
            seconds += p.typewriter_extra;
        }
        if self.candidate_confidence < p.low_confidence {
            frames += 1;
            seconds += p.low_extra;
        } else if terminated && self.candidate_confidence >= p.high_confidence && !self.candidate_growth {
            seconds = seconds.min(p.fast_seconds);
        }
        (frames, seconds.min(p.max_seconds))
    }

    /// Tek karede kabul: tamamlanmış (cümle sonu), çok net, önceki metnin devamı olmayan aday ve
    /// oyunda daktilo efekti görülmemiş.
    fn instant(&self, p: &Params) -> bool {
        self.candidate_frames == 1
            && !p.typewriter_seen
            && !self.candidate_growth
            && self.candidate_confidence >= p.instant_confidence
            && ends_sentence(&self.candidate_text)
    }

    /// Çevirisi önceden başlatılabilir mi: tamamlanmış görünen, güveni yüksek, büyümeyen aday.
    fn prefetchable(&self, p: &Params) -> bool {
        self.accepted_text.as_deref() != Some(self.candidate_text.as_str())
            && self.candidate_frames >= 1
            && !self.candidate_growth
            && self.candidate_confidence >= p.high_confidence
            && ends_sentence(&self.candidate_text)
    }

    fn observe(&mut self, text: &str, confidence: f64, now: f64, p: &Params) -> (Observed, bool) {
        if let Some(accepted) = &self.accepted_text {
            if same_text(accepted, text) {
                let duplicate = accepted != text;
                self.candidate_text = accepted.clone();
                self.candidate_frames = p.min_frames.max(self.candidate_frames);
                self.candidate_growth = false;
                return (if duplicate { Observed::Duplicate } else { Observed::Same }, false);
            }
        }
        let outcome = if self.candidate_frames > 0 && same_text(&self.candidate_text, text) {
            self.candidate_frames += 1;
            // Güven kare kare değişir; ortalaması tutulur.
            self.candidate_confidence += (confidence - self.candidate_confidence) / self.candidate_frames as f64;
            if self.candidate_text != text {
                Observed::Duplicate
            } else {
                Observed::Same
            }
        } else {
            let previous = if self.candidate_frames > 0 { Some(self.candidate_text.as_str()) } else { self.accepted_text.as_deref() };
            let growth = previous.is_some_and(|old| is_growth(old, text));
            self.similarity = previous.map_or(0.0, |old| similarity(old, text));
            let first = self.candidate_frames == 0 && self.accepted_text.is_none();
            self.candidate_text = text.to_string();
            self.candidate_frames = 1;
            self.candidate_since = now;
            self.candidate_confidence = confidence;
            self.candidate_growth = growth;
            if first {
                Observed::Same
            } else {
                Observed::Changed { growth }
            }
        };
        let (frames, seconds) = self.required(p);
        let mut fast = false;
        let instant = self.instant(p);
        // Küçük pay: tur aralığı tam süreye denk geldiğinde kayan nokta hatası bir tur kaybettirmesin.
        if instant || (self.candidate_frames >= frames && now - self.candidate_since >= seconds - 1e-6) {
            fast = instant || seconds < p.min_seconds - 1e-9;
            self.accepted_text = Some(self.candidate_text.clone());
            self.accepted_confidence = self.candidate_confidence;
            self.candidate_growth = false;
        }
        (outcome, fast)
    }

    /// Yeni OCR kutusunu alır; gösterilen kutu yalnızca belirgin bir kayma varsa güncellenir.
    fn place(&mut self, block: &MergedBlock, tolerance: f64, now: f64) {
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
            self.still_since = now;
        }
        self.block = block.clone();
    }

    /// Bu blok, kutusu büyüyüp IoU'su düştüğü halde bu takibin devamı mı: aynı satır bandında,
    /// yatayda örtüşüyor ve metni takip edilen metnin devamı (ya da tersi).
    fn continues(&self, block: &MergedBlock) -> bool {
        let (a, b) = (self.block.bbox, block.bbox);
        let vertical = a.bottom().min(b.bottom()) - a.y.max(b.y);
        let horizontal = a.right().min(b.right()) - a.x.max(b.x);
        if vertical < 0.5 * a.height.min(b.height) || horizontal <= 0.0 {
            return false;
        }
        let known = [Some(self.candidate_text.as_str()), self.accepted_text.as_deref()];
        known.into_iter().flatten().any(|old| is_growth(old, &block.text) || is_growth(&block.text, old))
    }
}

impl Default for Stabilizer {
    fn default() -> Stabilizer {
        Stabilizer {
            min_stable_frames: 2,
            min_stable_seconds: 0.3,
            hold_seconds: 0.5,
            box_tolerance: 0.25,
            min_overlap: 0.3,
            fast_seconds: 0.15,
            growth_extra_seconds: 0.5,
            typewriter_extra_seconds: 0.3,
            low_confidence: 0.6,
            low_confidence_extra_seconds: 0.2,
            high_confidence: 0.9,
            instant_confidence: 0.97,
            max_stabilize_seconds: 1.5,
            tracked: Vec::new(),
            next_id: 1,
            growth_score: 0.0,
            stats: SubtitleStats::default(),
        }
    }
}

impl Stabilizer {
    /// Büyüme ortalaması bunun üstündeyse oyunda daktilo efekti var sayılır.
    const TYPEWRITER_SCORE: f64 = 0.25;

    pub fn new() -> Stabilizer {
        Stabilizer::default()
    }

    fn params(&self) -> Params {
        Params {
            min_frames: self.min_stable_frames,
            min_seconds: self.min_stable_seconds,
            fast_seconds: self.fast_seconds,
            growth_extra: self.growth_extra_seconds,
            typewriter_extra: self.typewriter_extra_seconds,
            typewriter_seen: self.growth_score > Stabilizer::TYPEWRITER_SCORE,
            low_confidence: self.low_confidence,
            low_extra: self.low_confidence_extra_seconds,
            high_confidence: self.high_confidence,
            instant_confidence: self.instant_confidence,
            max_seconds: self.max_stabilize_seconds,
        }
    }

    pub fn update(&mut self, blocks: &[MergedBlock], now: f64) -> Vec<StableBlock> {
        let params = self.params();
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
            if best.is_none() {
                // Kutusu büyüyen (ya da kısalan) metin: aynı satırda ve metni devamıysa aynı takip.
                best = self.tracked.iter().enumerate().find(|(index, item)| !matched[*index] && item.continues(block)).map(|(index, _)| (index, 0.0));
                if best.is_some() {
                    self.stats.continued += 1;
                }
            }

            let observed = if let Some((index, _)) = best {
                matched[index] = true;
                let item = &mut self.tracked[index];
                let was_accepted = item.accepted_text.clone();
                item.place(block, self.box_tolerance, now);
                item.last_seen = now;
                let (observed, fast) = item.observe(&block.text, block.confidence, now, &params);
                if item.accepted_text != was_accepted {
                    self.stats.accepted += 1;
                    self.stats.fast_accepts += u64::from(fast);
                    self.stats.instant_accepts += u64::from(item.candidate_frames == 1);
                }
                observed
            } else {
                let mut item = Tracked {
                    id: format!("b{}", self.next_id),
                    block: block.clone(),
                    shown_bbox: block.bbox,
                    shown_line_height: block.line_height,
                    still_since: now,
                    first_seen: now,
                    accepted_text: None,
                    accepted_confidence: block.confidence,
                    candidate_text: block.text.clone(),
                    candidate_frames: 0,
                    candidate_since: now,
                    candidate_confidence: block.confidence,
                    candidate_growth: false,
                    similarity: 0.0,
                    last_seen: now,
                };
                let (observed, _) = item.observe(&block.text, block.confidence, now, &params);
                if item.accepted_text.is_some() {
                    self.stats.accepted += 1;
                    self.stats.fast_accepts += 1;
                    self.stats.instant_accepts += 1;
                }
                self.tracked.push(item);
                matched.push(true);
                self.next_id += 1;
                self.stats.candidates += 1;
                observed
            };
            match observed {
                Observed::Duplicate => self.stats.duplicates += 1,
                Observed::Changed { growth } => {
                    self.stats.growths += u64::from(growth);
                    self.growth_score = self.growth_score * 0.8 + if growth { 0.2 } else { 0.0 };
                }
                Observed::Same => {}
            }
        }

        let hold = self.hold_seconds;
        let mut dropped = 0;
        self.tracked.retain(|item| {
            let keep = now - item.last_seen <= hold;
            if !keep && item.accepted_text.is_none() {
                dropped += 1;
            }
            keep
        });
        self.stats.dropped += dropped;
        self.tracked
            .iter()
            .filter_map(|item| {
                item.accepted_text.as_ref().map(|text| StableBlock {
                    id: item.id.clone(),
                    bbox: item.shown_bbox,
                    line_height: item.shown_line_height,
                    text: text.clone(),
                    still_seconds: now - item.still_since,
                    confidence: item.accepted_confidence,
                })
            })
            .collect()
    }

    /// Henüz oturmamış ama tamamlanmış görünen adayların metinleri: çevirileri önceden başlatılır,
    /// oturduklarında çeviri hazır olur (gösterilmezler).
    pub fn prefetch(&self) -> Vec<String> {
        let params = self.params();
        self.tracked.iter().filter(|t| t.prefetchable(&params)).map(|t| t.candidate_text.clone()).collect()
    }

    /// Son `max_age` saniyede değişmiş ve henüz oturmamış aday var mı (bir sonraki kare hemen okunur).
    pub fn has_fresh_pending(&self, now: f64, max_age: f64) -> bool {
        self.tracked.iter().any(|t| t.accepted_text.as_deref() != Some(t.candidate_text.as_str()) && now - t.candidate_since < max_age)
    }

    /// Takip edilen adaylar (teşhis); metin içermez.
    pub fn candidates(&self, now: f64) -> Vec<CandidateInfo> {
        let params = self.params();
        self.tracked
            .iter()
            .map(|item| {
                let pending = item.accepted_text.as_deref() != Some(item.candidate_text.as_str());
                let (frames, required) = item.required(&params);
                let state = if !pending {
                    CandidateState::Ready
                } else if item.candidate_frames <= 1 && item.accepted_text.is_none() {
                    CandidateState::Tracking
                } else {
                    CandidateState::Stabilizing
                };
                let elapsed = now - item.candidate_since;
                let stability = if pending { (elapsed / required.max(1e-6)).min(1.0) * (item.candidate_frames.min(frames) as f64 / frames as f64) } else { 1.0 };
                CandidateInfo {
                    id: item.id.clone(),
                    state,
                    age_s: now - item.first_seen,
                    frames: item.candidate_frames,
                    stability,
                    required_s: required,
                    confidence: item.candidate_confidence,
                    growing: item.candidate_growth,
                    similarity: item.similarity,
                    region: item.shown_bbox,
                }
            })
            .collect()
    }

    pub fn stats(&self) -> &SubtitleStats {
        &self.stats
    }

    /// Takipleri bırakır (pencere kayboldu, uyku, okuyucu yenilendi); sayaçlar korunur.
    pub fn reset(&mut self) {
        self.stats.dropped += self.tracked.iter().filter(|t| t.accepted_text.is_none()).count() as u64;
        self.tracked.clear();
    }
}

/// Cümle sonu noktalaması (kapanış tırnağı/parantezi yok sayılır).
pub fn ends_sentence(text: &str) -> bool {
    let trimmed = text.trim_end_matches(|c: char| c.is_whitespace() || "\"'”’)]»".contains(c));
    trimmed.ends_with(['.', '!', '?', '…', '。', '！', '？'])
}

fn word_count(text: &str) -> usize {
    text.split_whitespace().filter(|w| w.chars().any(char::is_alphabetic)).count()
}

/// `new`, `old`'un devamı mı: normalize halde `old` ile başlıyor ve daha uzun (daktilo efekti,
/// parça parça gelen altyazı). Tek-iki harflik başlangıçlar sayılmaz.
pub fn is_growth(old: &str, new: &str) -> bool {
    let (old, new) = (normalize(old), normalize(new));
    old.chars().count() >= 3 && new.len() > old.len() && new.starts_with(&old)
}

/// Normalize iki metnin benzerliği: 1 − düzenleme uzaklığı / uzun olanın uzunluğu (0...1).
/// Çok uzun metinlerde hesaplanmaz (0).
pub fn similarity(a: &str, b: &str) -> f64 {
    let a: Vec<char> = normalize(a).chars().collect();
    let b: Vec<char> = normalize(b).chars().collect();
    let longest = a.len().max(b.len());
    if longest == 0 {
        return 1.0;
    }
    if longest > 400 {
        return 0.0;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            current[j + 1] = (previous[j] + usize::from(ca != cb)).min(previous[j + 1] + 1).min(current[j] + 1);
        }
        previous = current;
    }
    1.0 - previous[b.len()] as f64 / longest as f64
}

/// Noktalama/boşluk/büyük-küçük harf farkı ve aynı uzunlukta tek tük harf oynaması aynı
/// metin sayılır. Uzunluk değişirse (daktilo efekti) farklı metindir. Bir rakamın başka bir
/// rakama dönmesi OCR oynaması değil gerçek değişikliktir ("2/5" → "3/5", "Bölüm 3" → "Bölüm 4");
/// OCR'ın karıştırdığı çiftler harf↔rakam ya da harf↔harftir ("l"/"1", "O"/"0").
pub fn same_text(a: &str, b: &str) -> bool {
    let left: Vec<char> = normalize(a).chars().collect();
    let right: Vec<char> = normalize(b).chars().collect();
    if left == right {
        return true;
    }
    if left.len() != right.len() || left.is_empty() {
        return false;
    }
    let differing: Vec<(&char, &char)> = left.iter().zip(&right).filter(|(x, y)| x != y).collect();
    if differing.iter().any(|(x, y)| x.is_ascii_digit() && y.is_ascii_digit()) {
        return false;
    }
    differing.len() <= (left.len() / 20).max(1)
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
        MergedBlock { bbox: Rect::new(0.2, y, 0.5, 0.06), line_height: 0.03, text: text.into(), confidence: 0.85, line_count: 1 }
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
        assert_eq!(shown[0].still_seconds, 1.0);
        let moved = stabilizer.update(&[block_at("Hello", 0.770)], 1.5);
        assert_eq!(moved[0].bbox.y, 0.770);
        assert_eq!(moved[0].still_seconds, 0.0);
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
    fn yalnizca_bir_rakami_degisen_metin_yeni_metindir() {
        assert!(!same_text("Collect the herbs 2/5", "Collect the herbs 3/5"));
        assert!(!same_text("Chapter 3", "Chapter 4"));
        // OCR'ın harf/rakam karıştırması aynı metin sayılır.
        assert!(same_text("The gate is locked.", "The gate is 1ocked."));
        assert!(same_text("Room 101 is open", "Room 1O1 is open"));

        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block("Collect the herbs 2/5")], 0.0);
        stabilizer.update(&[block("Collect the herbs 2/5")], 0.5);
        stabilizer.update(&[block("Collect the herbs 3/5")], 1.0);
        assert_eq!(texts(&stabilizer.update(&[block("Collect the herbs 3/5")], 1.5)), ["Collect the herbs 3/5"]);
    }

    #[test]
    fn ayni_uzunlukta_ardisik_replikler_takilmaz() {
        // Aynı yerde, aynı uzunlukta, yalnızca bir rakamı farklı replikler art arda.
        let mut stabilizer = Stabilizer::new();
        let mut now = 0.0;
        for n in 0..5 {
            let text = format!("Objective {n}: reach the tower");
            stabilizer.update(&[block(&text)], now);
            let shown = stabilizer.update(&[block(&text)], now + 0.4);
            assert_eq!(texts(&shown), [text.as_str()]);
            now += 1.0;
        }
    }

    fn conf(text: &str, confidence: f64) -> MergedBlock {
        MergedBlock { confidence, ..block(text) }
    }

    /// Senaryo: aynı metin 10 karede; bir kez kabul edilir, kopya sayılmaz, kimlik sabit.
    #[test]
    fn ayni_metin_on_karede_bir_kez_kabul_edilir() {
        let mut stabilizer = Stabilizer::new();
        let mut ids = HashSet::new();
        for frame in 0..10 {
            for b in stabilizer.update(&[block("The door is locked.")], frame as f64 * 0.2) {
                ids.insert(b.id);
            }
        }
        assert_eq!(ids.len(), 1);
        assert_eq!(stabilizer.stats().accepted, 1);
        assert_eq!(stabilizer.stats().candidates, 1);
    }

    #[test]
    fn noktalama_ve_bosluk_farki_kopya_sayilir() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block("The door is locked.")], 0.0);
        stabilizer.update(&[block("The door is locked.")], 0.4);

        let shown = stabilizer.update(&[block("THE DOOR IS  LOCKED")], 0.6);

        assert_eq!(texts(&shown), ["The door is locked."]);
        assert_eq!(stabilizer.stats().duplicates, 1);
        assert_eq!(stabilizer.stats().accepted, 1);
    }

    /// Senaryo A: metin tamamlanıyor. Yarım hali (noktalamasız, öncekinin devamı) taban sürede
    /// kabul edilmez; tamamı gelince kabul edilir.
    #[test]
    fn buyuyen_yarim_altyazi_cevrilmeden_tamamlanmasi_beklenir() {
        let mut stabilizer = Stabilizer::new();
        assert!(stabilizer.update(&[block("The ancient kingdom")], 0.0).is_empty());
        // "was" eklendi: büyüme. 0,4 sn durması (eski kuralda kabul edilirdi) yetmez.
        assert!(stabilizer.update(&[block("The ancient kingdom was")], 0.2).is_empty());
        assert!(stabilizer.update(&[block("The ancient kingdom was")], 0.4).is_empty());
        assert!(stabilizer.update(&[block("The ancient kingdom was")], 0.6).is_empty());
        assert!(stabilizer.update(&[block("The ancient kingdom was destroyed by war.")], 0.8).is_empty());

        let shown = stabilizer.update(&[block("The ancient kingdom was destroyed by war.")], 1.1);

        assert_eq!(texts(&shown), ["The ancient kingdom was destroyed by war."]);
        assert_eq!(stabilizer.stats().accepted, 1, "yarım hal hiç kabul edilmemeli");
        assert!(stabilizer.stats().growths >= 2);
    }

    /// Senaryo A'nın sınırı: büyüyen metin uzun süre durursa (oyuncu tuşa basmadı) yine gösterilir.
    #[test]
    fn duran_yarim_metin_ust_sinirda_kabul_edilir() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block("You need to find")], 0.0);
        stabilizer.update(&[block("You need to find the key")], 0.2);
        assert!(stabilizer.update(&[block("You need to find the key")], 0.8).is_empty());

        assert_eq!(texts(&stabilizer.update(&[block("You need to find the key")], 1.0)), ["You need to find the key"]);
    }

    /// Senaryo B: tamamlanmış, cümle sonu olan, güveni yüksek metin hızlı yoldan kabul edilir.
    #[test]
    fn tamamlanmis_net_metin_hizli_kabul_edilir() {
        let mut stabilizer = Stabilizer::new();
        assert!(stabilizer.update(&[conf("Run away!", 0.95)], 0.0).is_empty());

        assert_eq!(texts(&stabilizer.update(&[conf("Run away!", 0.95)], 0.2)), ["Run away!"]);
        assert_eq!(stabilizer.stats().fast_accepts, 1);
        // Noktalamasız ya da güveni orta metin taban süreyi bekler.
        let mut other = Stabilizer::new();
        other.update(&[conf("Run away", 0.95)], 0.0);
        assert!(other.update(&[conf("Run away", 0.95)], 0.2).is_empty());
    }

    /// Çok hızlı altyazı: tamamlanmış ve çok net metin tek karede kabul edilir.
    #[test]
    fn tamamlanmis_cok_net_metin_tek_karede_kabul_edilir() {
        let mut stabilizer = Stabilizer::new();

        assert_eq!(texts(&stabilizer.update(&[conf("Run away!", 1.0)], 0.0)), ["Run away!"]);
        // Yeni ilgisiz replik de tek karede.
        assert_eq!(texts(&stabilizer.update(&[conf("Get down, now!", 1.0)], 0.1)), ["Get down, now!"]);
        assert_eq!(stabilizer.stats().instant_accepts, 2);
        // Noktalamasız, büyüyen ya da güveni 0,97'nin altında olan metin tek karede kabul edilmez.
        let mut other = Stabilizer::new();
        assert!(other.update(&[conf("Run away", 1.0)], 0.0).is_empty());
        assert!(Stabilizer::new().update(&[conf("Run away!", 0.95)], 0.0).is_empty());
        let mut growing = Stabilizer::new();
        growing.update(&[conf("Wait for", 1.0)], 0.0);
        assert!(growing.update(&[conf("Wait for me.", 1.0)], 0.1).is_empty(), "devamı olan metin beklenmeli");
    }

    #[test]
    fn daktilo_gorulen_oyunda_tek_kare_kabul_yok() {
        let mut stabilizer = Stabilizer::new();
        for (i, text) in ["Th", "The ki", "The king is", "The king is dead."].iter().enumerate() {
            stabilizer.update(&[conf(text, 1.0)], i as f64 * 0.1);
        }
        let below = |text: &str| MergedBlock { bbox: Rect::new(0.2, 0.5, 0.5, 0.06), ..conf(text, 1.0) };
        assert!(stabilizer.update(&[below("Long live the king.")], 2.0).is_empty());
    }

    #[test]
    fn tamamlanmis_gorunen_aday_onceden_cevrilebilir() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[conf("Hold the line.", 0.93)], 0.0);
        assert_eq!(stabilizer.prefetch(), ["Hold the line."]);
        assert!(stabilizer.has_fresh_pending(0.05, 2.0));
        stabilizer.update(&[conf("Hold the line.", 0.93)], 0.2);
        assert!(stabilizer.prefetch().is_empty(), "oturunca önceden çeviri listesinden çıkar");
        assert!(!stabilizer.has_fresh_pending(0.25, 2.0));
        // Noktalamasız ya da güvensiz aday önceden çevrilmez.
        let mut other = Stabilizer::new();
        other.update(&[conf("Hold the", 0.93), MergedBlock { bbox: Rect::new(0.2, 0.2, 0.5, 0.06), ..conf("Who is there?", 0.5) }], 0.0);
        assert!(other.prefetch().is_empty());
    }

    #[test]
    fn dusuk_guvenli_metin_bir_kare_daha_bekler() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[conf("Door is locked", 0.5)], 0.0);
        assert!(stabilizer.update(&[conf("Door is locked", 0.5)], 0.5).is_empty(), "iki kare yetmez");

        assert_eq!(texts(&stabilizer.update(&[conf("Door is locked", 0.5)], 0.6)), ["Door is locked"]);
    }

    /// Senaryo C: ilgisiz yeni metin önceki adayı kapatır; büyüme beklemesi uygulanmaz.
    #[test]
    fn ilgisiz_yeni_metin_yeni_aday_baslatir() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block("Open the door")], 0.0);
        stabilizer.update(&[block("Open the door")], 0.4);

        stabilizer.update(&[block("Run away from here")], 0.6);
        let shown = stabilizer.update(&[block("Run away from here")], 0.9);

        assert_eq!(texts(&shown), ["Run away from here"]);
        assert_eq!(stabilizer.stats().growths, 0);
    }

    /// Kutusu büyüyen metin aynı takipte kalır (eski kısa metin ayrı blok olarak asılı kalmaz).
    #[test]
    fn genisleyen_kutu_ayni_takipte_kalir() {
        let narrow = |text: &str, w: f64| MergedBlock { bbox: Rect::new(0.3, 0.8, w, 0.04), ..block(text) };
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[narrow("Wait for me", 0.08)], 0.0);
        stabilizer.update(&[narrow("Wait for me", 0.08)], 0.4);

        let shown = stabilizer.update(&[narrow("Wait for me, I am coming with you.", 0.4)], 0.6);
        assert_eq!(shown.len(), 1, "eski kutu ayrı takip olmamalı");
        let shown = stabilizer.update(&[narrow("Wait for me, I am coming with you.", 0.4)], 0.9);

        assert_eq!(texts(&shown), ["Wait for me, I am coming with you."]);
        assert_eq!(stabilizer.stats().continued, 1);
        assert_eq!(stabilizer.stats().candidates, 1);
    }

    /// Hızlı diyalog: oturmadan kaybolan adaylar sayılır, sonsuza dek beklenmez.
    #[test]
    fn oturmadan_kaybolan_aday_dusurulur() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block("Quick line one")], 0.0);
        stabilizer.update(&[], 0.2);

        assert!(stabilizer.update(&[], 0.6).is_empty());
        assert_eq!(stabilizer.stats().dropped, 1);
        assert!(stabilizer.candidates(0.6).is_empty());
    }

    #[test]
    fn daktilo_goruldukten_sonra_noktalamasiz_uzun_metin_biraz_daha_bekler() {
        let mut stabilizer = Stabilizer::new();
        for (i, text) in ["Th", "The ki", "The king is", "The king is dead."].iter().enumerate() {
            stabilizer.update(&[block(text)], i as f64 * 0.1);
        }
        assert!(stabilizer.params().typewriter_seen);
        let below = |text: &str| MergedBlock { bbox: Rect::new(0.2, 0.5, 0.5, 0.06), ..block(text) };
        stabilizer.update(&[below("Long live the new king")], 2.0);
        assert!(stabilizer.update(&[below("Long live the new king")], 2.4).is_empty());

        assert_eq!(texts(&stabilizer.update(&[below("Long live the new king")], 2.6)), ["Long live the new king"]);
    }

    #[test]
    fn aday_yasam_dongusu_teshiste_gorunur() {
        let mut stabilizer = Stabilizer::new();
        stabilizer.update(&[block("Find the key")], 0.0);
        assert_eq!(stabilizer.candidates(0.0)[0].state, CandidateState::Tracking);
        stabilizer.update(&[block("Find the key")], 0.2);
        let info = &stabilizer.candidates(0.2)[0];
        assert_eq!(info.state, CandidateState::Stabilizing);
        assert!(info.stability > 0.5 && info.stability < 1.0, "{info:?}");
        stabilizer.update(&[block("Find the key")], 0.4);
        assert_eq!(stabilizer.candidates(0.4)[0].state, CandidateState::Ready);
        stabilizer.update(&[block("Find the key now")], 0.6);
        let info = &stabilizer.candidates(0.6)[0];
        assert_eq!(info.state, CandidateState::Stabilizing);
        assert!(info.growing);
        assert!(info.similarity > 0.7);
    }

    #[test]
    fn buyume_ve_cumle_sonu_kurallari() {
        assert!(is_growth("The ancient", "The ancient kingdom"));
        assert!(is_growth("You sho", "You shouldn't"));
        assert!(!is_growth("Th", "The king"), "çok kısa başlangıç sayılmaz");
        assert!(!is_growth("Open the door", "Open the door"));
        assert!(!is_growth("Open the door", "Run away"));
        assert!(ends_sentence("Run away!"));
        assert!(ends_sentence("\"Is it you?\""));
        assert!(ends_sentence("Wait…"));
        assert!(!ends_sentence("The ancient kingdom was"));
        assert!((similarity("abc", "abc") - 1.0).abs() < 1e-9);
        assert!(similarity("open the door", "run away") < 0.5);
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
