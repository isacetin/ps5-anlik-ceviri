//! Çevrilmeyecek adlar (oyun, karakter, marka). DeepL'e gitmeden önce `<x>…</x>` ile işaretlenir;
//! istek `ignore_tags=x` ile gider, DeepL etiket içini olduğu gibi bırakır. OCR'ın tek harflik
//! hataları ("PayStation") da yakalanır ve adın doğru yazımı kullanılır.

/// Her zaman korunan adlar.
pub const BUILT_IN: &[&str] = &["PlayStation Store", "PlayStation Plus", "PlayStation", "DualSense", "PS5", "PS4"];

/// Bu uzunluktan (harf) kısa adlarda yazım hatası toleransı yoktur; "PS5" ile "PS4" karışmasın.
const FUZZY_MIN_CHARS: usize = 6;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Terms {
    /// Çok kelimeli ve uzun adlar önce denenir.
    entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    canonical: String,
    /// Küçük harfli kelimeler.
    words: Vec<String>,
    /// Kelimeler arasındaki boşluk dışı işaretler ("Avatar: Frontiers" → [":"]).
    separators: Vec<String>,
}

/// Metinde bulunan bir ad: bayt aralığı ve doğru yazımı.
struct Found<'a> {
    start: usize,
    end: usize,
    canonical: &'a str,
}

impl Terms {
    /// Yerleşik adlar + kullanıcının adları (boşlar ve tekrarlar atılır).
    pub fn new(user: &[String]) -> Terms {
        let mut entries: Vec<Entry> = Vec::new();
        for term in BUILT_IN.iter().map(|t| t.to_string()).chain(user.iter().map(|t| t.trim().to_string())) {
            let ranges = word_ranges(&term);
            let words: Vec<String> = ranges.iter().map(|&(s, e)| term[s..e].to_lowercase()).collect();
            if words.is_empty() || entries.iter().any(|e| e.words == words) {
                continue;
            }
            let separators = ranges.windows(2).map(|w| separator(&term[w[0].1..w[1].0])).collect();
            entries.push(Entry { canonical: term, words, separators });
        }
        entries.sort_by(|a, b| b.words.len().cmp(&a.words.len()).then(b.canonical.chars().count().cmp(&a.canonical.chars().count())));
        Terms { entries }
    }

    /// DeepL'e gidecek hali: adlar `<x>doğru yazım</x>`, geri kalan XML'e göre kaçışlı.
    pub fn protect(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 16);
        let mut cursor = 0;
        for found in self.find(text) {
            out.push_str(&escape(&text[cursor..found.start]));
            out.push_str("<x>");
            out.push_str(&escape(found.canonical));
            out.push_str("</x>");
            cursor = found.end;
        }
        out.push_str(&escape(&text[cursor..]));
        out
    }

    /// Metin yalnızca adlardan (ve rakam/işaretlerden) mı oluşuyor? Öyleyse çevrilecek bir şey yok.
    pub fn is_only_terms(&self, text: &str) -> bool {
        let found = self.find(text);
        if found.is_empty() {
            return false;
        }
        let mut cursor = 0;
        let mut rest = String::new();
        for f in &found {
            rest.push_str(&text[cursor..f.start]);
            cursor = f.end;
        }
        rest.push_str(&text[cursor..]);
        !rest.chars().any(char::is_alphabetic)
    }

    fn find(&self, text: &str) -> Vec<Found<'_>> {
        let words = word_ranges(text);
        let lower: Vec<String> = words.iter().map(|&(s, e)| text[s..e].to_lowercase()).collect();
        let mut found = Vec::new();
        let mut i = 0;
        'words: while i < words.len() {
            for entry in &self.entries {
                let k = entry.words.len();
                if i + k > words.len() {
                    continue;
                }
                // Kelimeler arasındaki işaretler adınkilerle aynı olmalı ("It. Takes Two" eşleşmez).
                let same_separators = (0..k - 1).all(|j| separator(&text[words[i + j].1..words[i + j + 1].0]) == entry.separators[j]);
                if !same_separators {
                    continue;
                }
                let candidate = lower[i..i + k].join(" ");
                let wanted = entry.words.join(" ");
                let canonical = entry.canonical.as_str();
                let matches = candidate == wanted
                    || (wanted.chars().count() >= FUZZY_MIN_CHARS && levenshtein(&candidate, &wanted) <= 1)
                    || truncated_match(text, &words[i..i + k], &lower[i..i + k], &entry.words);
                if matches {
                    found.push(Found { start: words[i].0, end: words[i + k - 1].1, canonical });
                    i += k;
                    continue 'words;
                }
            }
            i += 1;
        }
        found
    }
}

/// DeepL yanıtından işaretleri kaldırır ve kaçışları geri çevirir.
pub fn unprotect(translated: &str) -> String {
    translated
        .replace("<x>", "")
        .replace("</x>", "")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Arayüzde sığmayıp kesilmiş ad: "Avatar: Frontiers of Pand..." metnin sonundaysa ve son kelime
/// adın ilgili kelimesinin başıysa (en az 3 harf) eşleşir.
fn truncated_match(text: &str, ranges: &[(usize, usize)], candidate: &[String], wanted: &[String]) -> bool {
    let after = text[ranges[ranges.len() - 1].1..].trim();
    if !(after == "..." || after == "…") {
        return false;
    }
    let last = candidate.len() - 1;
    candidate[..last] == wanted[..last] && candidate[last].chars().count() >= 3 && wanted[last].starts_with(&candidate[last])
}

/// Ayırıcının boşluk dışı kısmı.
fn separator(between: &str) -> String {
    between.chars().filter(|c| !c.is_whitespace()).collect()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Harf/rakam dizilerinin bayt aralıkları ("It's" → "It", "s").
fn word_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, c) in text.char_indices() {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(index),
            (false, Some(s)) => {
                ranges.push((s, index));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        ranges.push((s, text.len()));
    }
    ranges
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            current[j + 1] = (previous[j] + usize::from(ca != cb)).min(previous[j + 1] + 1).min(current[j] + 1);
        }
        previous = current;
    }
    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(user: &[&str]) -> Terms {
        Terms::new(&user.iter().map(|t| t.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn ad_isaretlenir_geri_kalan_cevrilir() {
        let t = terms(&["It Takes Two"]);

        assert_eq!(t.protect("Continue It Takes Two"), "Continue <x>It Takes Two</x>");
        assert_eq!(t.protect("Open PlayStation Store"), "Open <x>PlayStation Store</x>");
        assert_eq!(t.protect("The gate is locked."), "The gate is locked.");
    }

    #[test]
    fn buyuk_kucuk_harf_ve_ocr_hatasi_dogru_yazima_cevrilir() {
        let t = terms(&[]);

        assert_eq!(t.protect("PayStation Store"), "<x>PlayStation Store</x>");
        assert_eq!(t.protect("Connect your DUALSENSE"), "Connect your <x>DualSense</x>");
        // Kısa adlarda tolerans yok: PS4 PS5 sanılmaz.
        assert_eq!(t.protect("PS4 and PS5"), "<x>PS4</x> and <x>PS5</x>");
        assert_eq!(t.protect("PS6"), "PS6");
    }

    #[test]
    fn kelime_icinde_ve_noktalama_ile_bolunmus_ad_eslesmez() {
        let t = terms(&["Rose"]);

        assert_eq!(t.protect("Roses are red"), "Roses are red");
        assert_eq!(t.protect("Ask Rose."), "Ask <x>Rose</x>.");
        assert_eq!(terms(&["It Takes Two"]).protect("It. Takes Two"), "It. Takes Two");
    }

    #[test]
    fn yalnizca_addan_olusan_metin_cevrilmez() {
        let t = terms(&["It Takes Two", "Avatar: Frontiers of Pandora"]);

        assert!(t.is_only_terms("It Takes Two"));
        assert!(t.is_only_terms("PlayStation Store"));
        assert!(t.is_only_terms("Avatar: Frontiers of Pandora"));
        assert!(t.is_only_terms("PS5 ™"));
        assert!(!t.is_only_terms("Play It Takes Two"));
        assert!(!t.is_only_terms("Options"));
    }

    #[test]
    fn sonu_kesilmis_ad_eslesir() {
        let t = terms(&["Avatar: Frontiers of Pandora"]);

        assert!(t.is_only_terms("Avatar: Frontiers of Pand..."));
        assert!(t.is_only_terms("Avatar: Frontiers of Pando…"));
        // Kesilme işareti yoksa ya da kesik kelime çok kısaysa eşleşmez.
        assert!(!t.is_only_terms("Avatar: Frontiers of Pand"));
        assert!(!t.is_only_terms("Avatar: Frontiers of Pa..."));
    }

    #[test]
    fn xml_karakterleri_kacislanir_ve_geri_cevrilir() {
        let t = terms(&[]);

        let protected = t.protect("Q&A <beta> on PS5");
        assert_eq!(protected, "Q&amp;A &lt;beta&gt; on <x>PS5</x>");
        assert_eq!(unprotect("S&amp;C &lt;beta&gt; <x>PS5</x> üzerinde"), "S&C <beta> PS5 üzerinde");
    }

    #[test]
    fn uzun_ad_kisadan_once_eslesir_ve_tekrarlar_atilir() {
        let t = terms(&["playstation", "  ", "Kratos"]);

        assert_eq!(t.protect("PlayStation Plus"), "<x>PlayStation Plus</x>");
        assert_eq!(t.entries.len(), BUILT_IN.len() + 1);
    }
}
