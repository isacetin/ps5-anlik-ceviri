//! Yerel çeviri modeli (Opus-MT) için metin hazırlığı. İş hattı arka uca `Terms::protect` çıktısını
//! verir (XML kaçışlı, adlar `<x>…</x>` içinde) ve dönen metni `terms::unprotect` ile açar; DeepL bu
//! biçimi kendisi anlar, Opus-MT anlamaz. Burada:
//!
//! - adlar modele `ZXQ0`, `ZXQ1`… yer tutucularıyla gider, çeviriden sonra geri konur ve addan sonra
//!   gelen ek ("ZXQ0'da" → "Store'da") adın son sesine göre yeniden uyumlanır;
//! - baştaki madde/düğme işaretleri (`•`, `›`, `©`…) modelin sözlüğünde olmadığı için ayrılıp
//!   çeviriden sonra geri eklenir.

/// Bu işaretlerle başlayan metinlerde işaret çeviriye gitmez. Tire (altyazı konuşma çizgisi),
/// tırnak ve parantez model tarafından doğru taşındığı için ayrılmaz.
fn is_prefix_mark(c: char) -> bool {
    !c.is_alphanumeric() && !c.is_whitespace() && !"-\"'(“‘".contains(c)
}

const PLACEHOLDER: &str = "ZXQ";

#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    /// Modele gidecek metin; boşsa çevrilecek bir şey yoktur (yalnızca işaret).
    pub source: String,
    prefix: String,
    names: Vec<String>,
}

/// `Terms::protect` çıktısını modele gidecek hale getirir.
pub fn prepare(protected: &str) -> Prepared {
    let mut text = String::with_capacity(protected.len());
    let mut names = Vec::new();
    let mut rest = protected;
    while let Some(start) = rest.find("<x>") {
        let Some(end) = rest[start..].find("</x>").map(|e| start + e) else { break };
        text.push_str(&unescape(&rest[..start]));
        text.push_str(&format!("{PLACEHOLDER}{}", names.len()));
        names.push(unescape(&rest[start + 3..end]));
        rest = &rest[end + 4..];
    }
    text.push_str(&unescape(rest));

    let body_start = text.find(|c: char| !is_prefix_mark(c)).unwrap_or(text.len());
    let body_start = text[body_start..].find(|c: char| !c.is_whitespace()).map_or(text.len(), |i| body_start + i);
    let prefix = text[..body_start].to_string();
    Prepared { source: text[body_start..].to_string(), prefix, names }
}

impl Prepared {
    /// Model çıktısını iş hattının beklediği biçime getirir: adlar yerinde, baştaki işaret geri
    /// eklenmiş, XML kaçışlı (`terms::unprotect` ile açılır). Bir yer tutucu kaybolduysa ya da
    /// çoğaldıysa `None`; o zaman `plain` ile adlar metnin içinde çevrilmelidir.
    pub fn finish(&self, translated: &str) -> Option<String> {
        // Sözlükte olmayan karakterler "⁇" olarak döner.
        let mut text = translated.replace('⁇', " ").split_whitespace().collect::<Vec<_>>().join(" ");
        // ZXQ1, ZXQ10'un başı olduğu için büyükten küçüğe.
        for (index, name) in self.names.iter().enumerate().rev() {
            let placeholder = format!("{PLACEHOLDER}{index}");
            let positions: Vec<usize> = text
                .match_indices(&placeholder)
                .filter(|(at, _)| !text[at + placeholder.len()..].starts_with(|c: char| c.is_ascii_digit()))
                .map(|(at, _)| at)
                .collect();
            let [at] = positions[..] else { return None };
            let after = at + placeholder.len();
            let (suffix_end, suffix) = apostrophe_suffix(&text[after..]);
            let replacement = match suffix {
                Some(suffix) => format!("{name}'{}", harmonize(name, suffix)),
                None => name.clone(),
            };
            text.replace_range(at..after + suffix_end, &replacement);
        }
        if self.source.is_empty() {
            text.clear();
        }
        Some(escape(&format!("{}{}", self.prefix, text)))
    }

    /// Yer tutucusuz hali (yedek): adlar metnin içinde, model onları da çevirebilir.
    pub fn plain(&self) -> Prepared {
        let mut source = self.source.clone();
        for (index, name) in self.names.iter().enumerate().rev() {
            source = source.replace(&format!("{PLACEHOLDER}{index}"), name);
        }
        Prepared { source, prefix: self.prefix.clone(), names: Vec::new() }
    }
}

/// Yer tutucunun hemen ardındaki "'da" gibi ek: (kesme işaretiyle birlikte bayt uzunluğu, ek).
fn apostrophe_suffix(after: &str) -> (usize, Option<&str>) {
    let Some(apostrophe) = after.chars().next().filter(|c| *c == '\'' || *c == '’') else { return (0, None) };
    let start = apostrophe.len_utf8();
    let end = after[start..].find(|c: char| !c.is_alphabetic()).map_or(after.len(), |i| start + i);
    if end == start {
        return (0, None);
    }
    (end, Some(&after[start..end]))
}

/// Adın son sesi: son ünlüsü, ünlüyle mi bittiği, sert ünsüzle mi bittiği.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Ending {
    vowel: char,
    ends_with_vowel: bool,
    voiceless: bool,
}

const VOWELS: &str = "aeıioöuü";

/// Adın Türkçe okunuşuna göre son sesi; İngilizce yazım için kaba kurallar ("Store" → "stor",
/// "Cody" → "kodi", "May" → "mey"), sondaki sayılar okunuşlarıyla ("PS5" → "beş").
fn ending(name: &str) -> Option<Ending> {
    let lower = name.to_lowercase();
    let tail: String = lower.trim_end_matches(|c: char| !c.is_alphanumeric()).to_string();
    let digits: String = tail.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
    let spoken = if digits.is_empty() { english_sound(&tail) } else { number_word(&digits).to_string() };
    let vowel = spoken.chars().rev().find(|c| VOWELS.contains(*c))?;
    let last = spoken.chars().last()?;
    Some(Ending { vowel, ends_with_vowel: VOWELS.contains(last), voiceless: "pçtksşhf".contains(last) })
}

fn english_sound(word: &str) -> String {
    // "Dawn", "Shaw": o diye okunur.
    let mut chars: Vec<char> = word.replace("aw", "o").chars().filter(|c| c.is_alphabetic()).collect();
    let n = chars.len();
    // Sondaki okunmayan e: "Store", "Rose".
    if n > 2 && chars[n - 1] == 'e' && !VOWELS.contains(chars[n - 2]) && chars[n - 2] != 'l' {
        chars.pop();
    }
    let n = chars.len();
    if n >= 2 {
        match (chars[n - 2], chars[n - 1]) {
            // "May", "Grey": ey diye okunur.
            ('a' | 'e', 'y') => chars.splice(n - 2.., ['e', 'y']).for_each(drop),
            // "Cody": i.
            (c, 'y') if !VOWELS.contains(c) => chars[n - 1] = 'i',
            // "Two", "Snow": u/o.
            (_, 'w') => chars[n - 1] = 'u',
            _ => {}
        }
    }
    chars
        .into_iter()
        .map(|c| match c {
            // Sert ünsüz gibi okunanlar: "Doc", "Max", "Iraq".
            'c' | 'x' | 'q' => 'k',
            other => other,
        })
        .collect()
}

/// Sayının son okunan kelimesi: 5 → "beş", 20 → "yirmi", 100 → "yüz".
fn number_word(digits: &str) -> &'static str {
    const ONES: [&str; 10] = ["sıfır", "bir", "iki", "üç", "dört", "beş", "altı", "yedi", "sekiz", "dokuz"];
    const TENS: [&str; 10] = ["", "on", "yirmi", "otuz", "kırk", "elli", "altmış", "yetmiş", "seksen", "doksan"];
    let d: Vec<usize> = digits.bytes().map(|b| (b - b'0') as usize).collect();
    let n = d.len();
    if d[n - 1] != 0 || n == 1 {
        ONES[d[n - 1]]
    } else if d[n - 2] != 0 {
        TENS[d[n - 2]]
    } else if n == 3 || (n > 3 && d[n - 3] != 0) {
        "yüz"
    } else {
        "bin"
    }
}

fn is_back(vowel: char) -> bool {
    "aıou".contains(vowel)
}

fn is_rounded(vowel: char) -> bool {
    "oöuü".contains(vowel)
}

/// Eki adın son sesine göre yeniden yazar: ünlü uyumu, kaynaştırma ünsüzü (y/n) ve d/t, c/ç.
fn harmonize(name: &str, suffix: &str) -> String {
    let Some(end) = ending(name) else { return suffix.to_string() };
    let mut chars: Vec<char> = suffix.chars().collect();
    if chars.iter().any(|c| c.is_uppercase()) {
        return suffix.to_string();
    }
    // Model eki yer tutucunun okunuşuna göre çekti; kaynaştırma ünsüzünü at, sonra adınkine göre ekle.
    if chars.len() > 1 && (chars[0] == 'y' || chars[0] == 'n') && VOWELS.contains(chars[1]) {
        chars.remove(0);
    }
    let starts_with_vowel = chars.first().is_some_and(|c| VOWELS.contains(*c));
    let instrumental = matches!(chars.as_slice(), ['l', 'a' | 'e', ..]) && chars.len() <= 3;
    let genitive = chars.len() >= 2 && "ıiuü".contains(chars[0]) && chars[1] == 'n';

    let mut previous = end.vowel;
    for c in chars.iter_mut() {
        if "ae".contains(*c) {
            *c = if is_back(previous) { 'a' } else { 'e' };
            previous = *c;
        } else if "ıiuü".contains(*c) {
            *c = match (is_back(previous), is_rounded(previous)) {
                (true, false) => 'ı',
                (false, false) => 'i',
                (true, true) => 'u',
                (false, true) => 'ü',
            };
            previous = *c;
        }
    }
    if let Some(first) = chars.first_mut() {
        *first = match *first {
            'd' | 't' => if end.voiceless { 't' } else { 'd' },
            'c' | 'ç' => if end.voiceless { 'ç' } else { 'c' },
            other => other,
        };
    }
    let buffer = match (end.ends_with_vowel, starts_with_vowel || instrumental) {
        (true, true) if genitive => "n",
        (true, true) => "y",
        _ => "",
    };
    format!("{buffer}{}", chars.into_iter().collect::<String>())
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terms::{unprotect, Terms};

    fn terms(user: &[&str]) -> Terms {
        Terms::new(&user.iter().map(|t| t.to_string()).collect::<Vec<_>>())
    }

    /// Hazırla → "çevir" → bitir → aç; `model` yer tutuculu kaynaktan sahte çeviri üretir.
    fn round_trip(t: &Terms, text: &str, model: impl Fn(&str) -> String) -> String {
        let prepared = prepare(&t.protect(text));
        unprotect(&prepared.finish(&model(&prepared.source)).expect("yer tutucu korunmalı"))
    }

    #[test]
    fn adlar_yer_tutucuyla_gider_ve_geri_konur() {
        let t = terms(&["It Takes Two", "May"]);

        let prepared = prepare(&t.protect("Have you played It Takes Two, May?"));
        assert_eq!(prepared.source, "Have you played ZXQ0, ZXQ1?");
        let plain = prepared.plain();
        assert_eq!(plain.source, "Have you played It Takes Two, May?");
        assert_eq!(plain.finish("It Takes Two'yu oynadın mı, May?"), Some("It Takes Two'yu oynadın mı, May?".into()));

        let shown = round_trip(&t, "Have you played It Takes Two, May?", |_| "ZXQ0'u oynadın mı, ZXQ1?".into());
        assert_eq!(shown, "It Takes Two'yu oynadın mı, May?");
    }

    #[test]
    fn kaybolan_ya_da_cogalan_yer_tutucu_reddedilir() {
        let prepared = prepare(&terms(&["Cody"]).protect("Cody wins"));

        assert_eq!(prepared.finish("kazandı"), None);
        assert_eq!(prepared.finish("ZXQ0 ZXQ0 kazandı"), None);
        assert_eq!(prepared.finish("ZXQ0 kazandı"), Some("Cody kazandı".into()));
    }

    #[test]
    fn on_ve_ustu_yer_tutucu_karismaz() {
        let names: Vec<String> = ["Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliet", "Kilo"]
            .map(String::from)
            .to_vec();
        let t = Terms::new(&names);
        let text = names.join(" ");

        let shown = round_trip(&t, &text, |source| source.to_string());

        assert_eq!(shown, text);
    }

    #[test]
    fn bastaki_isaret_ayrilir_ve_geri_eklenir() {
        let t = terms(&[]);

        assert_eq!(prepare(&t.protect("• Jump")).source, "Jump");
        assert_eq!(round_trip(&t, "• Jump", |_| "Zıpla".into()), "• Zıpla");
        assert_eq!(round_trip(&t, "© 2021 Hazelight", |_| "2021 Hazelight".into()), "© 2021 Hazelight");
        assert_eq!(round_trip(&t, "& Proceed", |_| "Devam et".into()), "& Devam et");
        // Altyazı çizgisi ayrılmaz, model kendisi taşır.
        assert_eq!(prepare(&t.protect("- Yeah.")).source, "- Yeah.");
        // Yalnızca işaretten oluşan metin çevrilmez.
        let only_mark = prepare(&t.protect("•"));
        assert_eq!(only_mark.source, "");
        assert_eq!(only_mark.finish(""), Some("•".into()));
    }

    #[test]
    fn bilinmeyen_karakter_isareti_atilir_ve_xml_kacislanir() {
        let t = terms(&[]);

        assert_eq!(round_trip(&t, "Ch›1pter Select", |_| "Ch ⁇ 1pter Seç".into()), "Ch 1pter Seç");
        let prepared = prepare(&t.protect("Q&A <beta>"));
        assert_eq!(prepared.source, "Q&A <beta>");
        assert_eq!(prepared.finish("S&C <beta>"), Some("S&amp;C &lt;beta&gt;".into()));
    }

    #[test]
    fn ek_adin_son_sesine_gore_uyumlanir() {
        // Model eki yer tutucunun okunuşuna göre çeker ("ZXQ0" → sıfır); ek adınkine göre düzelir.
        let cases = [
            ("PlayStation Store", "da", "da"),  // stor → da
            ("PlayStation Store", "den", "dan"), // yanlış uyum düzelir
            ("The Shed", "a", "e"),
            ("It Takes Two", "ı", "yu"),       // tu → kaynaştırma y
            ("Cody", "da", "de"),               // kodi
            ("Cody", "ın", "nin"),              // tamlayan: kaynaştırma n
            ("Whack-a-Cody", "da", "de"),
            ("May", "a", "e"),                  // mey
            ("May", "ın", "in"),
            ("Rose", "a", "a"),                 // ros
            ("Rose", "la", "la"),
            ("Cody", "la", "yle"),              // ile → kodiyle
            ("PS5", "da", "te"),                // beş → sert ünsüz
            ("PS4", "e", "e"),                  // dört
            ("FC 26", "dan", "dan"),            // altı
            ("FC 20", "de", "de"),              // yirmi
            ("Horizon Zero Dawn", "ı", "u"),    // don
            ("Max", "da", "ta"),
        ];
        for (name, suffix, expected) in cases {
            assert_eq!(harmonize(name, suffix), expected, "{name}'{suffix}");
        }
    }

    #[test]
    fn eksiz_ve_buyuk_harfli_devam_degismez() {
        let t = terms(&["Mira"]);

        assert_eq!(round_trip(&t, "Talk to Mira.", |_| "ZXQ0 ile konuş.".into()), "Mira ile konuş.");
        assert_eq!(round_trip(&t, "Mira", |_| "ZXQ0'ABC".into()), "Mira'ABC");
    }
}
