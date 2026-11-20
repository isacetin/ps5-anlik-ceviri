//! Oyun terim sözlüğü: "Quest" → "Görev", "Critical Hit" → "Kritik Vuruş". Çeviri modeli oyun
//! terimini bağlamdan bilemez ("Charge" → şarj mı hücum mu?); sözlükteki terim modele **Türkçesiyle**
//! ve dokunulmaz işaretle gider: `<g>Görev</g>` (DeepL `ignore_tags`, yerel modelde yer tutucu).
//!
//! Kurallar:
//! - Öncelik: kullanıcının oyuna özel düzeltmesi > oyun profili > kullanıcının genel sözlüğü >
//!   yerleşik genel sözlük. Aynı terim üst katmanda varsa alttaki yok sayılır.
//! - Uzun ifade önce: "critical hit" ile "hit" ikisi de varsa "Critical Hit" tek eşleşmedir.
//! - Kelime sınırı: "Questions" "Quest" ile eşleşmez; yalnızca İngilizce çoğul (-s, -es, -y→-ies)
//!   tanınır ve Türkçe çoğul eki (-lar/-ler) ünlü uyumuyla eklenir (yalnızca tek kelimelik hedefte).
//! - Büyük/küçük harf: kaynağın yazımı hedefe taşınır (cümle başı büyük, tümü büyük → tümü büyük,
//!   Türkçe I/İ kurallarıyla). `case_sensitive` terim yalnızca birebir yazımla eşleşir.
//! - `preserve`: terim çevrilmez, ad gibi korunur (`<x>`).
//! - Türkçe çıktıda alt dize değiştirme yapılmaz: ekler bozulmaz. Kesmeyle gelen ek ("Görev'i")
//!   cins isimde kesmesiz yazılır ("Görevi"), `terms::unprotect` yapar.

use serde::{Deserialize, Serialize};

use crate::terms::{escape, word_ranges, Terms};

/// Sözlük girdisi (profil dosyasında ve kullanıcı sözlüğünde bu biçimde saklanır).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GlossaryEntry {
    /// İngilizce terim ya da ifade.
    pub source: String,
    /// Tercih edilen Türkçe karşılık (`preserve` ise yok sayılır).
    pub target: String,
    /// Kabul edilebilir diğer karşılıklar (bilgi; çeviride kullanılmaz).
    pub alternatives: Vec<String>,
    /// "item", "skill", "ui"… serbest etiket.
    pub category: String,
    pub case_sensitive: bool,
    /// Terim çevrilmez, olduğu gibi kalır (oyun/karakter adı gibi).
    pub preserve: bool,
    pub notes: String,
    /// Girdinin kaynağı: "builtin", "profile", "user".
    pub origin: String,
    /// 0...1: kaynağın güvenilirliği (kullanıcı düzeltmesi 1).
    pub confidence: f64,
}

impl Default for GlossaryEntry {
    fn default() -> GlossaryEntry {
        GlossaryEntry {
            source: String::new(),
            target: String::new(),
            alternatives: Vec::new(),
            category: String::new(),
            case_sensitive: false,
            preserve: false,
            notes: String::new(),
            origin: "profile".into(),
            confidence: 1.0,
        }
    }
}

impl GlossaryEntry {
    pub fn new(source: &str, target: &str) -> GlossaryEntry {
        GlossaryEntry { source: source.into(), target: target.into(), ..GlossaryEntry::default() }
    }

    /// Doğrulama: boş/aşırı uzun/işaret içeren girdi reddedilir.
    pub fn validate(&self) -> Result<(), String> {
        let source = self.source.trim();
        if source.is_empty() || !source.chars().any(char::is_alphanumeric) {
            return Err("sözlük terimi boş".into());
        }
        if source.chars().count() > 100 || self.target.chars().count() > 100 {
            return Err(format!("sözlük terimi çok uzun: {source}"));
        }
        if !self.preserve && self.target.trim().is_empty() {
            return Err(format!("\"{source}\" için Türkçe karşılık boş"));
        }
        if [source, self.target.as_str()].iter().any(|t| t.contains(['<', '>'])) {
            return Err(format!("sözlükte < ve > kullanılamaz: {source}"));
        }
        if !(0.0..=1.0).contains(&self.confidence) {
            return Err(format!("güven 0 ile 1 arasında olmalı: {source}"));
        }
        Ok(())
    }
}

/// Sözlük katmanı; küçük olan önce gelir (önceliklidir).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Layer {
    /// Kullanıcının bu oyun için düzeltmesi.
    UserGame,
    /// Oyun profilinin sözlüğü.
    Game,
    /// Kullanıcının tüm oyunlar için sözlüğü.
    UserGlobal,
    /// Uygulamayla gelen genel oyun terimleri.
    BuiltIn,
}

impl Layer {
    pub fn is_game_scope(self) -> bool {
        matches!(self, Layer::UserGame | Layer::Game)
    }
}

/// Uygulamayla gelen, anlamı oyunlarda tek olan terimler. Bilerek kısa tutulur: genel kelimeler
/// ("Charge", "Light") burada yoktur, onlar oyun profiline göre değişir.
pub const BUILT_IN: &[(&str, &str)] = &[
    ("Side Quest", "Yan Görev"),
    ("Main Quest", "Ana Görev"),
    ("Quest", "Görev"),
    ("Fast Travel", "Hızlı Seyahat"),
    ("Skill Tree", "Yetenek Ağacı"),
    ("Skill", "Yetenek"),
    ("Critical Hit", "Kritik Vuruş"),
    ("Save Point", "Kayıt Noktası"),
    ("Checkpoint", "Kontrol Noktası"),
    ("Inventory", "Envanter"),
    ("Health Potion", "Can İksiri"),
];

#[derive(Clone, Debug, PartialEq)]
struct Compiled {
    /// Karşılaştırma için kelimeler (`case_sensitive` değilse küçük harf).
    words: Vec<String>,
    /// Kelimeler arasındaki boşluk dışı işaretler.
    separators: Vec<String>,
    source: String,
    target: String,
    preserve: bool,
    case_sensitive: bool,
    layer: Layer,
}

/// Derlenmiş, katmanlı sözlük.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glossary {
    entries: Vec<Compiled>,
    /// İlk kelimenin küçük hali (tek kelimelik terimde çoğul biçimleri de) → girdiler, eşleşme
    /// sırasıyla. Binlerce girdide de her kelimede yalnızca birkaç girdi denenir.
    by_first: std::collections::HashMap<String, Vec<usize>>,
}

/// Metinde bulunan terim.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub start: usize,
    pub end: usize,
    /// Modele gidecek işaretli hali (`<g>…</g>` ya da `<x>…</x>`).
    pub marked: String,
    /// Gösterilecek düz hali.
    pub plain: String,
    pub layer: Layer,
}

impl Glossary {
    /// Katmanlardan derler (verilen sırayla değil, katman önceliğiyle). Geçersiz girdiler atlanır.
    pub fn new(layers: &[(Layer, &[GlossaryEntry])]) -> Glossary {
        let mut ordered: Vec<(Layer, &GlossaryEntry)> = layers.iter().flat_map(|(layer, entries)| entries.iter().map(move |e| (*layer, e))).collect();
        ordered.sort_by_key(|(layer, _)| *layer);
        let mut entries: Vec<Compiled> = Vec::new();
        for (layer, entry) in ordered {
            if entry.validate().is_err() {
                continue;
            }
            let source = entry.source.trim();
            let ranges = word_ranges(source);
            let words: Vec<String> = ranges.iter().map(|&(s, e)| if entry.case_sensitive { source[s..e].to_string() } else { source[s..e].to_lowercase() }).collect();
            let separators = ranges.windows(2).map(|w| separator(&source[w[0].1..w[1].0])).collect();
            // Üst katmanda aynı terim varsa alt katmanınki yok sayılır.
            let duplicate = entries.iter().any(|c| c.words.iter().map(|w| w.to_lowercase()).eq(words.iter().map(|w| w.to_lowercase())));
            if duplicate {
                continue;
            }
            entries.push(Compiled {
                words,
                separators,
                source: source.to_string(),
                target: entry.target.trim().to_string(),
                preserve: entry.preserve,
                case_sensitive: entry.case_sensitive,
                layer,
            });
        }
        // Uzun ifade önce; eşit uzunlukta üst katman önce.
        entries.sort_by(|a, b| b.words.len().cmp(&a.words.len()).then(a.layer.cmp(&b.layer)));
        let mut by_first: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
        for (index, entry) in entries.iter().enumerate() {
            let first = entry.words[0].to_lowercase();
            let mut keys = vec![first.clone()];
            if entry.words.len() == 1 {
                keys.extend([format!("{first}s"), format!("{first}es")]);
                if let Some(stem) = first.strip_suffix('y') {
                    keys.push(format!("{stem}ies"));
                }
            }
            for key in keys {
                by_first.entry(key).or_default().push(index);
            }
        }
        Glossary { entries, by_first }
    }

    /// Yalnızca yerleşik terimler.
    pub fn built_in() -> Glossary {
        let entries: Vec<GlossaryEntry> = BUILT_IN.iter().map(|(s, t)| GlossaryEntry { origin: "builtin".into(), ..GlossaryEntry::new(s, t) }).collect();
        Glossary::new(&[(Layer::BuiltIn, &entries)])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Metindeki terimler (çakışmasız, soldan sağa). `skip` aralıkları (adlar) atlanır.
    pub fn find(&self, text: &str, skip: &[(usize, usize)]) -> Vec<Hit> {
        let words = word_ranges(text);
        let lower: Vec<String> = words.iter().map(|&(s, e)| text[s..e].to_lowercase()).collect();
        let mut hits = Vec::new();
        let mut i = 0;
        'words: while i < words.len() {
            if skip.iter().any(|&(s, e)| words[i].0 < e && words[i].1 > s) {
                i += 1;
                continue;
            }
            let candidates = self.by_first.get(&lower[i]).map(Vec::as_slice).unwrap_or(&[]);
            for entry in candidates.iter().map(|&index| &self.entries[index]) {
                let k = entry.words.len();
                if i + k > words.len() {
                    continue;
                }
                let span = &words[i..i + k];
                if skip.iter().any(|&(s, e)| span[0].0 < e && span[k - 1].1 > s) {
                    continue;
                }
                let separators_match = (0..k - 1).all(|j| separator(&text[span[j].1..span[j + 1].0]) == entry.separators[j]);
                if !separators_match {
                    continue;
                }
                let candidate: Vec<&str> = if entry.case_sensitive { span.iter().map(|&(s, e)| &text[s..e]).collect() } else { lower[i..i + k].iter().map(String::as_str).collect() };
                let head_matches = candidate[..k - 1].iter().zip(&entry.words).all(|(a, b)| a == b);
                if !head_matches {
                    continue;
                }
                let last = candidate[k - 1];
                let wanted = &entry.words[k - 1];
                let plural = if last == wanted {
                    false
                } else if !entry.case_sensitive && !entry.preserve && is_english_plural(wanted, last) && !entry.target.contains(' ') {
                    true
                } else {
                    continue;
                };
                // Terim fiil olarak geçiyorsa ("Charge the gate", "we must charge") kilitlenmez:
                // Türkçe isim karşılığı cümleye fiil olarak oturmaz, model kendisi çevirir.
                if !entry.preserve && used_as_verb(text, span[0].0, span[k - 1].1) {
                    continue;
                }
                let original = &text[span[0].0..span[k - 1].1];
                let (marked, plain) = if entry.preserve {
                    (format!("<x>{}</x>", escape(&entry.source)), entry.source.clone())
                } else {
                    let mut target = apply_case(original, &entry.target, entry.case_sensitive);
                    if plural {
                        target = turkish_plural(&target);
                    }
                    (format!("<g>{}</g>", escape(&target)), target)
                };
                hits.push(Hit { start: span[0].0, end: span[k - 1].1, marked, plain, layer: entry.layer });
                i += k;
                continue 'words;
            }
            i += 1;
        }
        hits
    }

    /// Metnin tamamı tek bir terimse (işaretler ve boşluk dışında) karşılığı, çevresindeki
    /// işaretlerle ("Charge!" → "Hücum!"): modele gitmez.
    pub fn whole(&self, text: &str) -> Option<Hit> {
        let hits = self.find(text, &[]);
        let [hit] = hits.as_slice() else { return None };
        let (before, after) = (&text[..hit.start], &text[hit.end..]);
        if before.chars().chain(after.chars()).any(char::is_alphanumeric) {
            return None;
        }
        Some(Hit { plain: format!("{before}{}{after}", hit.plain), ..hit.clone() })
    }
}

/// Adları (`<x>`) ve sözlük terimlerini (`<g>`) işaretler; geri kalanı XML'e göre kaçışlar.
/// İkinci değer bulunan sözlük terimi sayısıdır.
pub fn protect(terms: &Terms, glossary: &Glossary, text: &str) -> (String, usize) {
    let names = terms.spans(text);
    let name_ranges: Vec<(usize, usize)> = names.iter().map(|(s, e, _)| (*s, *e)).collect();
    let hits = glossary.find(text, &name_ranges);
    let mut marks: Vec<(usize, usize, String)> = names.into_iter().map(|(s, e, canonical)| (s, e, format!("<x>{}</x>", escape(&canonical)))).collect();
    marks.extend(hits.iter().map(|h| (h.start, h.end, h.marked.clone())));
    marks.sort_by_key(|(s, _, _)| *s);
    let mut out = String::with_capacity(text.len() + 16);
    let mut cursor = 0;
    for (start, end, marked) in marks {
        if start < cursor {
            continue;
        }
        out.push_str(&escape(&text[cursor..start]));
        out.push_str(&marked);
        cursor = end;
    }
    out.push_str(&escape(&text[cursor..]));
    (out, hits.len())
}

/// Önünde fiil getiren kelimeler: kip ve özne zamirleri ("to charge", "we charge", "don't skill").
const VERB_BEFORE: &[&str] = &["to", "will", "would", "can", "could", "must", "should", "shall", "might", "let's", "lets", "don't", "dont", "please", "we", "i", "they", "you", "must've", "gonna"];
/// Cümle başındaki terimden sonra gelince emir kipi gösterenler ("Charge the gate", "Light it up").
const OBJECT_AFTER: &[&str] = &[
    "the", "a", "an", "your", "my", "his", "her", "their", "our", "its", "this", "that", "these", "those", "him", "them", "it", "us", "me", "all", "every", "forward", "ahead", "up", "down", "into", "at", "toward", "towards",
];

/// Metindeki terim (bayt aralığı) fiil gibi mi kullanılmış. Sezgisel ve temkinlidir: emin değilse
/// `false` (terim isim sayılır). Yanlış "fiil" kararı yalnızca sözlüğün o yerde uygulanmamasıdır.
fn used_as_verb(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].trim_end();
    let previous = before.rsplit(|c: char| c.is_whitespace()).next().unwrap_or("").trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase();
    if !previous.is_empty() && VERB_BEFORE.contains(&previous.as_str()) && !before.ends_with([',', '.', '!', '?', ':']) {
        return true;
    }
    let sentence_start = before.is_empty() || before.ends_with(['.', '!', '?', ':', '"', '“', '…', '-']);
    let next = text[end..].split_whitespace().next().unwrap_or("").trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    sentence_start && OBJECT_AFTER.contains(&next.as_str())
}

fn separator(between: &str) -> String {
    between.chars().filter(|c| !c.is_whitespace()).collect()
}

/// `found`, `word`ün İngilizce çoğulu mu: -s, -es, ünsüz+y → -ies.
fn is_english_plural(word: &str, found: &str) -> bool {
    if word.ends_with('s') {
        return false;
    }
    if found == format!("{word}s") || found == format!("{word}es") {
        return true;
    }
    let mut chars = word.chars();
    let last = chars.next_back();
    let before = chars.clone().next_back();
    last == Some('y') && before.is_some_and(|c| !"aeiou".contains(c)) && found == format!("{}ies", chars.as_str())
}

/// Kaynağın yazımını hedefe taşır: tümü büyük → tümü büyük; küçük harfle başlıyor → küçük;
/// büyük harfle başlıyor → hedef olduğu gibi (sözlükteki yazım).
fn apply_case(original: &str, target: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        return target.to_string();
    }
    let letters: Vec<char> = original.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return turkish_upper(target);
    }
    if letters.first().is_some_and(|c| c.is_lowercase()) {
        // Hedef bir kısaltmayla başlıyorsa ("XP Puanı") dokunulmaz.
        let first_word = target.split_whitespace().next().unwrap_or("");
        if first_word.chars().filter(|c| c.is_alphabetic()).count() > 1 && first_word.chars().filter(|c| c.is_alphabetic()).all(char::is_uppercase) {
            return target.to_string();
        }
        return turkish_lower(target);
    }
    target.to_string()
}

pub fn turkish_upper(text: &str) -> String {
    text.chars()
        .flat_map(|c| match c {
            'i' => vec!['İ'],
            'ı' => vec!['I'],
            other => other.to_uppercase().collect(),
        })
        .collect()
}

pub fn turkish_lower(text: &str) -> String {
    text.chars()
        .flat_map(|c| match c {
            'I' => vec!['ı'],
            'İ' => vec!['i'],
            other => other.to_lowercase().collect(),
        })
        .collect()
}

/// Türkçe çoğul eki, son ünlüye göre (kalın → -lar, ince → -ler); tümü büyük yazımda büyük.
pub fn turkish_plural(word: &str) -> String {
    let last = word.chars().rev().find(|c| "aeıioöuüAEIİOÖUÜ".contains(*c));
    let back = last.is_some_and(|c| "aıouAIOU".contains(c));
    let suffix = if back { "lar" } else { "ler" };
    let upper = word.chars().filter(|c| c.is_alphabetic()).all(char::is_uppercase) && word.chars().filter(|c| c.is_alphabetic()).count() > 1;
    if upper {
        format!("{word}{}", turkish_upper(suffix))
    } else {
        format!("{word}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terms::unprotect;

    fn entries(pairs: &[(&str, &str)]) -> Vec<GlossaryEntry> {
        pairs.iter().map(|(s, t)| GlossaryEntry::new(s, t)).collect()
    }

    fn marked(glossary: &Glossary, text: &str) -> String {
        protect(&Terms::new(&[]), glossary, text).0
    }

    #[test]
    fn genel_sozluk_terimi_isaretlenir() {
        let g = Glossary::built_in();

        assert_eq!(marked(&g, "Open your Inventory."), "Open your <g>Envanter</g>.");
        assert_eq!(marked(&g, "The door is locked."), "The door is locked.");
    }

    #[test]
    fn oyuna_ozel_terim_genelden_once_gelir() {
        let game = entries(&[("Quest", "Macera")]);
        let user = entries(&[("Quest", "Görevcik")]);
        let builtin: Vec<GlossaryEntry> = entries(&[("Quest", "Görev")]);

        let g = Glossary::new(&[(Layer::BuiltIn, &builtin), (Layer::Game, &game)]);
        assert_eq!(marked(&g, "New Quest"), "New <g>Macera</g>");
        // Kullanıcının oyuna özel düzeltmesi profilin de üstündedir.
        let g = Glossary::new(&[(Layer::Game, &game), (Layer::UserGame, &user), (Layer::BuiltIn, &builtin)]);
        assert_eq!(marked(&g, "New Quest"), "New <g>Görevcik</g>");
        assert_eq!(g.find("New Quest", &[])[0].layer, Layer::UserGame);
    }

    #[test]
    fn uzun_ifade_kisadan_once_eslesir() {
        let g = Glossary::new(&[(Layer::Game, &entries(&[("hit", "Vuruş"), ("Critical Hit", "Kritik Vuruş"), ("critical", "Kritik")]))]);

        assert_eq!(marked(&g, "That was a critical hit!"), "That was a <g>kritik vuruş</g>!");
        assert_eq!(marked(&g, "A hit and a critical miss"), "A <g>vuruş</g> and a <g>kritik</g> miss");
    }

    #[test]
    fn kelime_siniri_korunur() {
        let g = Glossary::new(&[(Layer::Game, &entries(&[("Quest", "Görev"), ("Art", "Sanat")]))]);

        assert_eq!(marked(&g, "Questions remain."), "Questions remain.");
        assert_eq!(marked(&g, "Start the party"), "Start the party");
        assert_eq!(marked(&g, "Quest-giver"), "<g>Görev</g>-giver");
    }

    #[test]
    fn buyuk_kucuk_harf_tasinir() {
        let g = Glossary::new(&[(Layer::Game, &entries(&[("Inventory", "Envanter"), ("Item", "Eşya")]))]);

        assert_eq!(marked(&g, "INVENTORY FULL"), "<g>ENVANTER</g> FULL");
        assert_eq!(marked(&g, "your inventory"), "your <g>envanter</g>");
        assert_eq!(marked(&g, "ITEM"), "<g>EŞYA</g>");
        assert_eq!(turkish_upper("istif ıslak"), "İSTİF ISLAK");
        assert_eq!(turkish_lower("İSTİF ISLAK"), "istif ıslak");
    }

    #[test]
    fn buyuk_kucuk_harfe_duyarli_terim_yalnizca_birebir_eslesir() {
        let entry = GlossaryEntry { case_sensitive: true, ..GlossaryEntry::new("HP", "Can") };
        let g = Glossary::new(&[(Layer::Game, &[entry])]);

        assert_eq!(marked(&g, "Restore HP"), "Restore <g>Can</g>");
        assert_eq!(marked(&g, "Restore hp"), "Restore hp");
    }

    #[test]
    fn ingilizce_cogul_turkce_eke_doner() {
        let g = Glossary::new(&[(Layer::Game, &entries(&[("Quest", "Görev"), ("Ability", "Yetenek"), ("Box", "Kutu"), ("Save Point", "Kayıt Noktası")]))]);

        assert_eq!(marked(&g, "Your Quests"), "Your <g>Görevler</g>");
        assert_eq!(marked(&g, "Abilities unlocked"), "<g>Yetenekler</g> unlocked");
        assert_eq!(marked(&g, "Boxes"), "<g>Kutular</g>");
        // Çok kelimeli hedefte çoğul eki güvenle eklenemez ("Kayıt Noktaları"): eşleşmez, model çevirir.
        assert_eq!(marked(&g, "Save Points"), "Save Points");
        assert_eq!(turkish_plural("Görev"), "Görevler");
        assert_eq!(turkish_plural("Kutu"), "Kutular");
        assert_eq!(turkish_plural("GÖREV"), "GÖREVLER");
    }

    #[test]
    fn turkce_ek_kesmesi_cins_isimde_kalkar() {
        // DeepL dokunulmaz terimden sonra eki kesmeyle yazar; cins isimde Türkçe yazım kesmesizdir.
        assert_eq!(unprotect("<g>Görev</g>'i tamamla"), "Görevi tamamla");
        assert_eq!(unprotect("<g>Envanter</g>’de yer yok"), "Envanterde yer yok");
        assert_eq!(unprotect("<g>Lütuf Noktası</g>'da dinlen"), "Lütuf Noktasında dinlen");
        assert_eq!(unprotect("<g>Lütuf Noktası</g>'nda dinlen"), "Lütuf Noktasında dinlen");
        // Ad (x) kesmeyle kalır.
        assert_eq!(unprotect("<x>Kratos</x>'un baltası"), "Kratos'un baltası");
        // Kesmeden sonra harf yoksa (tırnak) dokunulmaz.
        assert_eq!(unprotect("'<g>Görev</g>' yazıyor"), "'Görev' yazıyor");
    }

    #[test]
    fn karakter_adi_korunur_ve_sozluk_adin_icine_girmez() {
        let terms = Terms::new(&["Geralt of Rivia".to_string()]);
        let g = Glossary::new(&[(Layer::Game, &entries(&[("Rivia", "Rivya"), ("Witcher", "Witcher")]))]);

        let (key, hits) = protect(&terms, &g, "Geralt of Rivia is a Witcher");

        assert_eq!(key, "<x>Geralt of Rivia</x> is a <g>Witcher</g>");
        assert_eq!(hits, 1);
        let preserve = GlossaryEntry { preserve: true, ..GlossaryEntry::new("Elden Ring", "") };
        let g = Glossary::new(&[(Layer::Game, &[preserve])]);
        assert_eq!(marked(&g, "Welcome to Elden Ring"), "Welcome to <x>Elden Ring</x>");
    }

    #[test]
    fn metnin_tamami_terimse_modele_gitmez() {
        let g = Glossary::built_in();

        assert_eq!(g.whole("Inventory").map(|h| h.plain), Some("Envanter".to_string()));
        assert_eq!(g.whole("» Fast Travel").map(|h| h.plain), Some("» Hızlı Seyahat".to_string()));
        assert_eq!(g.whole("Open Inventory"), None);
    }

    #[test]
    fn ayni_terim_farkli_oyunda_farkli_cevrilir() {
        let rpg = Glossary::new(&[(Layer::Game, &entries(&[("Charge", "Hücum")]))]);
        let racing = Glossary::new(&[(Layer::Game, &entries(&[("Charge", "Şarj")]))]);

        assert_eq!(marked(&rpg, "Charge!"), "<g>Hücum</g>!");
        assert_eq!(marked(&racing, "Charge!"), "<g>Şarj</g>!");
        assert_eq!(marked(&Glossary::built_in(), "Charge!"), "Charge!", "genel sözlükte belirsiz kelime yok");
    }

    #[test]
    fn binlerce_girdide_de_hizli() {
        let many: Vec<GlossaryEntry> = (0..5000).map(|i| GlossaryEntry::new(&format!("Term{i} Phrase"), &format!("Terim{i}"))).collect();
        let g = Glossary::new(&[(Layer::Game, &many)]);
        let text = "You need to find the key before sunset, then talk to the Term4999 Phrase keeper.";
        let started = std::time::Instant::now();
        for _ in 0..1000 {
            assert_eq!(g.find(text, &[]).len(), 1);
        }
        // 1000 arama; doğrusal taramada saniyeler sürerdi.
        assert!(started.elapsed() < std::time::Duration::from_millis(500), "{:?}", started.elapsed());
    }

    #[test]
    fn fiil_olarak_gecen_terim_kilitlenmez() {
        let g = Glossary::new(&[(Layer::Game, &entries(&[("Charge", "Hücum"), ("Light", "Işık")]))]);

        assert_eq!(marked(&g, "Charge the gate!"), "Charge the gate!");
        assert_eq!(marked(&g, "We must charge now."), "We must charge now.");
        assert_eq!(marked(&g, "Light it up."), "Light it up.");
        // İsim olarak kullanımda uygulanır.
        assert_eq!(marked(&g, "Lead the Charge!"), "Lead the <g>Hücum</g>!");
        assert_eq!(marked(&g, "The Light fades."), "The <g>Işık</g> fades.");
        assert_eq!(marked(&g, "Charge attack ready."), "<g>Hücum</g> attack ready.");
        assert_eq!(g.whole("Charge!").map(|h| h.plain), Some("Hücum!".to_string()));
    }

    #[test]
    fn gecersiz_girdi_atlanir() {
        let bad = vec![GlossaryEntry::new("", "x"), GlossaryEntry::new("Door", ""), GlossaryEntry::new("<b>", "y"), GlossaryEntry::new("Key", "Anahtar")];
        let g = Glossary::new(&[(Layer::Game, &bad)]);
        assert_eq!(g.len(), 1);
        assert!(GlossaryEntry::new("Door", "").validate().is_err());
        assert!(GlossaryEntry { confidence: 2.0, ..GlossaryEntry::new("a", "b") }.validate().is_err());
    }
}
