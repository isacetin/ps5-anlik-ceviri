//! OCR çıktısının hafif temizliği. Birleştirme ve sabitlemeden önce her satıra uygulanır; böylece
//! aynı yazının küçük okuma farkları (fazla boşluk, görünmez karakter) farklı metin sayılmaz,
//! önbellek anahtarı da tutarlı kalır. Anlamı değiştirebilecek düzeltme yapılmaz.
//!
//! `assess` ayrıca okumanın kalitesini ölçer (bkz. `Quality`): bozuk okumalar silinmez, güveni
//! düşürülür ve mevcut sabitleyici onları bir kare daha bekletir. Yalnızca neredeyse tamamı çöp olan
//! okuma atılır.

use crate::speaker;

/// - Görünmez karakterler (sıfır genişlikli boşluk, BOM) atılır; sekme/satır sonu boşluk olur.
/// - Art arda boşluklar teke iner, baş ve son boşluk atılır.
/// - Kelime başındaki `|`, ardından kesme işareti ya da küçük harf geliyorsa `I` olur; tek başına
///   `|` de sonraki kelime küçük harfle başlıyorsa `I` olur (Vision ve Windows OCR büyük I'yı dikey
///   çizgi okuyabiliyor: "|'m here", "| know"). "Save | Load" gibi ayırıcıya dokunulmaz.
pub fn clean(text: &str) -> String {
    let words: Vec<String> = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .map(|word| word.chars().filter(|c| !is_invisible(*c)).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    for (index, word) in words.iter().enumerate() {
        if word == "|" {
            let next_lower = words.get(index + 1).and_then(|w| w.chars().next()).is_some_and(char::is_lowercase);
            out.push(if next_lower { "I".to_string() } else { word.clone() });
        } else {
            out.push(fix_pipe(word));
        }
    }
    out.join(" ")
}

/// Çevrilecek bir şey var mı: en az bir harf. Yalnızca sayı ya da işaretten oluşan yazı
/// ("100", "3/5", "---") çevrilmez, üstüne çizilmez.
pub fn has_letters(text: &str) -> bool {
    text.chars().any(char::is_alphabetic)
}

fn is_invisible(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}' | '\u{00AD}')
}

fn fix_pipe(word: &str) -> String {
    let mut chars = word.chars();
    if chars.next() != Some('|') {
        return word.to_string();
    }
    match chars.next() {
        None => word.to_string(),
        Some(next) if next == '\'' || next == '’' || next.is_lowercase() => format!("I{}", &word[1..]),
        _ => word.to_string(),
    }
}

/// Bir OCR satırının kalite değerlendirmesi.
#[derive(Clone, Debug, PartialEq)]
pub struct Quality {
    /// Güvenli düzeltmeleri yapılmış metin (bkz. `assess`).
    pub text: String,
    /// 0...1: harflerin ne kadarı düzgün sözcüklerde. Çöp sözcük yoksa 1.
    pub score: f64,
}

/// Bu puanın altındaki okuma atılır (harflerinin üçte ikisinden çoğu çöp).
pub const REJECT_SCORE: f64 = 0.35;
/// OCR güveni de düşükse bu puanın altındaki okuma atılır.
pub const DOUBTFUL_SCORE: f64 = 0.6;
const DOUBTFUL_CONFIDENCE: f64 = 0.5;

/// `clean` çıktısını değerlendirir ve yalnızca anlamı değiştirmeyen düzeltmeler yapar:
///
/// - Tek başına küçük `l` ardından küçük harfli sözcük geliyorsa `I` olur ("l know" → "I know");
///   `l'm`, `l'll`, `l've`, `l'd` → `I'm`… (Vision büyük I'yı küçük l okuyabiliyor).
/// - Satır başındaki yalnızca işaretten oluşan parça atılır ("?! Where" → "Where"); satır
///   ortasında tek başına kalan `?`/`!` önceki sözcüğe yapışır ("going ?!" → "going?!"). Üç nokta
///   ve tireye dokunulmaz.
/// - Konuşan etiketinin önüne yapışmış gösterge sayısı atılır ("23m CESARE: Here!" →
///   "CESARE: Here!"). Sayı başka cümlenin başındaysa ("3 wolves left") dokunulmaz.
///
/// Çöp sözcükler (harf-rakam-simge karışımı "ou$3e", dalgalı büyük-küçük harf "CESAREiVJütt",
/// ünlüsüz "Gtt") silinmez, yalnızca puanı düşürür: gerçek bir sözcüğün bozuk okuması olabilirler.
pub fn assess(text: &str) -> Quality {
    let mut tokens: Vec<String> = text.split(' ').filter(|t| !t.is_empty()).map(str::to_string).collect();
    fix_lowercase_l(&mut tokens);
    fix_loose_punctuation(&mut tokens);
    strip_hud_counter(&mut tokens);
    let text = tokens.join(" ");
    let score = score(&tokens);
    Quality { text, score }
}

/// Okuma tamamen atılmalı mı: neredeyse tamamı çöp ya da hem şüpheli hem OCR güveni düşük.
/// Tek harfli anlamsız okuma ("l.", "j") da atılır; "I", "A" kalır.
pub fn reject(quality: &Quality, confidence: f64) -> bool {
    if quality.score < REJECT_SCORE || (quality.score < DOUBTFUL_SCORE && confidence < DOUBTFUL_CONFIDENCE) {
        return true;
    }
    let letters: Vec<char> = quality.text.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() == 1 && !quality.text.contains(char::is_numeric) && !matches!(letters[0], 'I' | 'A' | 'a' | 'O')
}

/// Kaliteye göre düzeltilmiş OCR güveni. Puan 1 iken değişmez; düştükçe güven de düşer ve
/// sabitleyici (`Stabilizer`, düşük güvende bir kare daha bekler, yüksek güvende hızlanmaz)
/// tutarsız okumanın oturmasını geciktirir.
///
/// `floor`: birleştiricinin satırı attığı güven (`block_merger::Options::min_confidence`). Bu eşiği
/// geçen bir satır kalite cezasıyla eşiğin altına düşürülmez: satırı atmak `reject`'in kararıdır,
/// güven yalnızca bekleme süresini etkiler (yoksa satır sayaçlara yansımadan kaybolurdu).
pub fn adjusted_confidence(confidence: f64, score: f64, floor: f64) -> f64 {
    let adjusted = confidence * (0.4 + 0.6 * score.clamp(0.0, 1.0));
    if confidence >= floor {
        adjusted.max(floor)
    } else {
        adjusted
    }
}

fn fix_lowercase_l(tokens: &mut [String]) {
    for i in 0..tokens.len() {
        let replacement = match tokens[i].as_str() {
            "l" => {
                let next_lower = tokens.get(i + 1).and_then(|t| t.chars().next()).is_some_and(char::is_lowercase);
                next_lower.then(|| "I".to_string())
            }
            word => ["l'm", "l'll", "l've", "l'd", "l’m", "l’ll", "l’ve", "l’d"].iter().find(|form| word.starts_with(**form) && !word[form.len()..].chars().any(char::is_alphanumeric)).map(|_| format!("I{}", &word[1..])),
        };
        if let Some(fixed) = replacement {
            tokens[i] = fixed;
        }
    }
}

fn is_loose_punctuation(token: &str) -> bool {
    !token.is_empty() && token.chars().all(|c| matches!(c, '?' | '!' | '.' | ',' | ';' | ':')) && !token.contains("..")
}

fn fix_loose_punctuation(tokens: &mut Vec<String>) {
    while tokens.len() > 1 && is_loose_punctuation(&tokens[0]) {
        tokens.remove(0);
    }
    let mut i = 1;
    while i < tokens.len() {
        let token = &tokens[i];
        if is_loose_punctuation(token) && token.chars().all(|c| matches!(c, '?' | '!')) {
            let previous = &tokens[i - 1];
            if previous.ends_with(['.', '!', '?', '…']) {
                tokens.remove(i);
            } else {
                let joined = format!("{previous}{token}");
                tokens[i - 1] = joined;
                tokens.remove(i);
            }
            continue;
        }
        i += 1;
    }
}

fn strip_hud_counter(tokens: &mut Vec<String>) {
    if tokens.len() < 3 || !is_counter(&tokens[0]) {
        return;
    }
    let rest = tokens[1..].join(" ");
    let caps_label = speaker::split(&rest, |_| false).is_some_and(|line| !line.label.chars().any(char::is_lowercase));
    if caps_label {
        tokens.remove(0);
    }
}

/// "23", "23m", "150 m"nin sayısı, "9?", "100.", "45%".
fn is_counter(token: &str) -> bool {
    let body = token.trim_end_matches(['.', '?', '!', ':']);
    let digits = body.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%');
    let unit = &body[digits.len()..];
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') && matches!(unit, "" | "m" | "km" | "ft" | "yd" | "%")
}

/// Harflerin düzgün sözcüklerdeki oranı. Yalnızca ünlüsüz olan sözcük ("Ngh", "Krrk": ünlem ya
/// da ad olabilir) yarım çöp sayılır; tek başına satırı atmaya yetmez.
fn score(tokens: &[String]) -> f64 {
    let (mut good, mut total) = (0.0f64, 0usize);
    for token in tokens {
        let letters = token.chars().filter(|c| c.is_alphabetic()).count();
        if letters == 0 {
            continue;
        }
        total += letters;
        good += match garbage_kind(token) {
            None => letters as f64,
            Some(Garbage::Vowelless) => letters as f64 / 2.0,
            Some(Garbage::Broken) => 0.0,
        };
    }
    if total == 0 {
        1.0
    } else {
        good / total as f64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Garbage {
    /// Harf-rakam-simge karışımı ya da dalgalı büyük-küçük harf: neredeyse kesin okuma hatası.
    Broken,
    /// Yalnızca ünlüsüz: okuma hatası olabilir, bilinmeyen bir ünlem ya da ad da olabilir.
    Vowelless,
}

const OPENING: &[char] = &['"', '\'', '“', '‘', '(', '[', '¿', '¡', '«', '-', '–', '—', '*'];
const CLOSING: &[char] = &['"', '\'', '”', '’', ')', ']', '.', ',', '!', '?', ';', ':', '…', '»', '-', '–', '—', '*'];

/// Sözcük OCR çöpü gibi mi. Temkinlidir: kısaltmalar (NPC, PS5, R2, 2nd, 100m, $500, 4K, 5v5),
/// ünlemler (Hmm, Shh, Psst) ve tek iç büyük harfli adlar (McCree, iPhone) çöp değildir.
pub fn is_garbage(token: &str) -> bool {
    garbage_kind(token).is_some()
}

fn garbage_kind(token: &str) -> Option<Garbage> {
    let word = token.trim_start_matches(OPENING).trim_end_matches(CLOSING);
    if word.is_empty() {
        return None;
    }
    let word = word.trim_start_matches(['$', '€', '£', '¥', '#', '+']).trim_end_matches(['%', '°', '+']);
    if word.chars().any(|c| !c.is_alphanumeric() && !matches!(c, '\'' | '’' | '-' | '–' | '/' | '.' | ',' | ':' | '&' | '+')) {
        return Some(Garbage::Broken);
    }
    let mut vowelless = false;
    for part in word.split(['\'', '’', '-', '–', '/', '.', ',', ':', '&', '+']) {
        let letters = part.chars().filter(|c| c.is_alphabetic()).count();
        let digits = part.chars().filter(|c| c.is_numeric()).count();
        if letters > 0 && digits > 0 && !alphanumeric_code(part) {
            return Some(Garbage::Broken);
        }
        if digits == 0 && letters >= 2 && wavy_case(part) {
            return Some(Garbage::Broken);
        }
        vowelless |= digits == 0 && letters >= 2 && no_vowels(part);
    }
    vowelless.then_some(Garbage::Vowelless)
}

/// Harf ve rakam birlikte olan olağan biçimler: PS5, R2, MP5, CO2, B2B; 2nd, 100m, 4K, 1080p,
/// 60fps; 5v5, 4x4.
fn alphanumeric_code(part: &str) -> bool {
    let chars: Vec<char> = part.chars().collect();
    let run = |from: usize, digit: bool| chars[from..].iter().take_while(|c| c.is_ascii_digit() == digit && c.is_ascii_alphanumeric()).count();
    let lead_letters = run(0, false);
    if lead_letters > 0 {
        // Harfler + rakamlar (+ en çok bir harf, + rakam): PS5, R2, MP5, B2B.
        let digits = run(lead_letters, true);
        let tail = chars.len() - lead_letters - digits;
        // Sondaki harf yalnızca büyük harfli kodlarda ("B2B"); "Wh4t" bozuk sözcüktür.
        let caps = chars[..lead_letters].iter().all(char::is_ascii_uppercase);
        return lead_letters <= 4 && (1..=4).contains(&digits) && tail <= 1 && (tail == 0 || (caps && chars[chars.len() - 1].is_ascii_alphabetic()));
    }
    let lead_digits = run(0, true);
    let letters = run(lead_digits, false);
    let trailing_digits = if lead_digits + letters < chars.len() { run(lead_digits + letters, true) } else { 0 };
    lead_digits + letters + trailing_digits == chars.len() && (1..=3).contains(&letters) && (trailing_digits == 0 || letters == 1)
}

/// Dalgalı büyük-küçük harf: küçükten büyüğe iki ya da daha çok geçiş ("tHeRe") ya da en az iki
/// büyük harften sonra küçük, sonra yine büyük harf ("CESAREiVJütt", "TREADBounD"). "McCree",
/// "iPhone", "PlayStation", "NPCs" olağandır.
fn wavy_case(part: &str) -> bool {
    let letters: Vec<char> = part.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.windows(2).filter(|w| w[0].is_lowercase() && w[1].is_uppercase()).count() >= 2 {
        return true;
    }
    // Büyük/küçük harf dizileri: (büyük mü, uzunluk).
    let mut runs: Vec<(bool, usize)> = Vec::new();
    for c in &letters {
        let upper = c.is_uppercase();
        match runs.last_mut() {
            Some((u, n)) if *u == upper => *n += 1,
            _ => runs.push((upper, 1)),
        }
    }
    runs.windows(3).any(|w| w[0].0 && w[0].1 >= 2 && !w[1].0 && w[2].0)
}

/// Üç ya da daha çok harfli, ünlüsüz, küçük harf içeren sözcük ("Gtt", "rnnk"). Tümü büyük harfli
/// kısaltmalar (NPC, DLC) ve bilinen ünlemler (Hmm, Shh, Psst, Brr, Tsk) hariç.
fn no_vowels(part: &str) -> bool {
    const EXCEPTIONS: &[&str] = &["hm", "h", "m", "z", "r", "br", "gr", "pft", "ngh", "hngh", "hng", "ng", "nh", "hn", "psh", "pss", "hrm", "hmf", "mph", "grh", "rgh", "tsch", "sh", "shh", "psst", "pst", "pfft", "brr", "grr", "tsk", "mm", "zz", "hmph", "mhm", "nth", "mrs", "mr", "dr", "st", "jr", "sr", "pts", "hrs", "mph", "kph", "lbs", "ltd", "sgt", "cpl", "pvt", "lt", "cmdr", "vs", "tv", "pc", "npc", "dlc", "ch", "pp", "tch", "ssh"];
    let letters: Vec<char> = part.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() < 3 || letters.iter().all(|c| c.is_uppercase()) || !letters.iter().any(|c| c.is_lowercase()) {
        return false;
    }
    if letters.iter().any(|c| "aeiouyAEIOUYıİöÖüÜâêîôûàèìòùáéíóúäëïÀÈÌÒÙÁÉÍÓÚÄËÏ".contains(*c)) {
        return false;
    }
    // Uzatılmış ünlem ("Hmmmm", "Shhhh", "Grrrr"): yinelenen harfler tek sayılır.
    let mut collapsed: Vec<char> = letters.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
    collapsed.dedup();
    let collapsed: String = collapsed.into_iter().collect();
    let lower: String = letters.iter().flat_map(|c| c.to_lowercase()).collect();
    !EXCEPTIONS.contains(&collapsed.as_str()) && !EXCEPTIONS.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bosluklar_ve_gorunmez_karakterler_temizlenir() {
        assert_eq!(clean("  The  door\tis\u{200B} locked. "), "The door is locked.");
        assert_eq!(clean("Press\nany button"), "Press any button");
        assert_eq!(clean("   "), "");
    }

    #[test]
    fn dikey_cizgi_i_harfine_doner() {
        assert_eq!(clean("| know what |'m doing."), "I know what I'm doing.");
        assert_eq!(clean("|t is late."), "It is late.");
        // Gerçek ayırıcı olarak kullanılan çizgiye dokunulmaz.
        assert_eq!(clean("Save | Load"), "Save | Load");
        assert_eq!(clean("L1 |"), "L1 |");
        assert_eq!(clean("HP |100"), "HP |100");
        assert_eq!(clean("A||B"), "A||B");
    }

    fn quality(text: &str) -> Quality {
        assess(&clean(text))
    }

    #[test]
    fn normal_diyalog_tam_puan_alir_ve_degismez() {
        for text in [
            "You shouldn't have come here, traveler.",
            "I don't know what you're talking about!",
            "Wait... what?",
            "Hmm.",
            "Shhhh! They'll hear us.",
            "Brrr, it's cold.",
            "Mr. McCree bought an iPhone at the PlayStation Store.",
            "Mid-Autumn rock'n'roll co-op, AT&T.",
        ] {
            let q = quality(text);
            assert_eq!(q.text, text, "{text}");
            assert_eq!(q.score, 1.0, "{text}");
            assert!(!reject(&q, 0.9));
        }
    }

    #[test]
    fn kisa_anlamli_diyalog_atilmaz() {
        for text in ["No.", "Go!", "I.", "A", "Ok", "Hm?", "Yes!", "Me?"] {
            let q = quality(text);
            assert!(!reject(&q, 0.9), "{text} atılmamalı");
            assert_eq!(q.score, 1.0, "{text}");
        }
    }

    #[test]
    fn sayi_iceren_metin_korunur() {
        for text in ["Collected 3/5 feathers", "Press R2 to sprint", "Reach 100m in 2nd gear", "It costs $500, 45% off", "PS5 runs at 60fps in 4K", "5v5 match at 12:40", "MP5 and CO2 grenade", "Distance 1,250.5 km"] {
            let q = quality(text);
            assert_eq!(q.text, text, "{text}");
            assert_eq!(q.score, 1.0, "{text}");
        }
    }

    #[test]
    fn ozel_isimler_ve_kisaltmalar_cop_sayilmaz() {
        for word in ["Kratos", "Atreus", "Ellie", "NPC", "DLC", "XP", "HP", "Mrs.", "Dr.", "Sgt.", "DeLuca", "O'Driscoll", "Jean-Luc", "Lynx", "Gwynbleidd", "Brynn"] {
            assert!(!is_garbage(word), "{word}");
        }
    }

    #[test]
    fn ocr_copu_puani_dusurur_ama_silinmez() {
        let q = quality("We need the ou$3e key now");
        assert_eq!(q.text, "We need the ou$3e key now");
        assert!(q.score < 1.0 && q.score > DOUBTFUL_SCORE, "{}", q.score);
        assert!(!reject(&q, 0.9));
        for word in ["ou$3e", "waters2aljd", "the[lil", "Wh4t", "CESAREiVJütt'h", "tHeRe", "Gtt", "rnnk"] {
            assert!(is_garbage(word), "{word}");
        }
    }

    #[test]
    fn neredeyse_tamami_cop_okuma_atilir() {
        // Geçişte iki altyazının karışması: atılmaz ama güveni düşer, sabitleyici bekletir.
        let blend = quality("Gtt solkolfege.");
        assert!(blend.score < 1.0 && !reject(&blend, 0.9));
        // Sabitleyicinin "anında kabul" eşiğinin (0.97) altına iner.
        assert!(adjusted_confidence(0.97, blend.score, 0.4) < 0.95);
        let q = quality("CESAREiVJütt'h rnnk Gtt");
        assert!(q.score < REJECT_SCORE, "{}", q.score);
        assert!(reject(&q, 0.9));
        // Şüpheli ama bütünüyle çöp değil: yalnızca OCR güveni de düşükse atılır.
        let half = quality("Gtt here wh4t");
        assert!(half.score >= REJECT_SCORE && half.score < DOUBTFUL_SCORE, "{}", half.score);
        assert!(!reject(&half, 0.9));
        assert!(reject(&half, 0.45));
    }

    #[test]
    fn tek_harfli_anlamsiz_okuma_atilir() {
        assert!(reject(&quality("l."), 0.9));
        assert!(reject(&quality("j"), 0.9));
        assert!(!reject(&quality("I."), 0.9));
        assert!(!reject(&quality("A"), 0.9));
        // Tek harf + sayı bir göstergedir, okuma hatası değil.
        assert!(!reject(&quality("x2"), 0.9));
    }

    #[test]
    fn kucuk_l_buyuk_i_olur() {
        assert_eq!(quality("l don't know.").text, "I don't know.");
        assert_eq!(quality("Then l'm going, l'll be back.").text, "Then I'm going, I'll be back.");
        // Gerçek "l" (ölçü birimi, liste harfi) olduğu gibi kalır.
        assert_eq!(quality("Size: 2 l").text, "Size: 2 l");
        assert_eq!(quality("Option l B").text, "Option l B");
    }

    #[test]
    fn basibos_noktalama_duzeltilir() {
        assert_eq!(quality("?! Where are you").text, "Where are you");
        assert_eq!(quality("Where are you going ?!").text, "Where are you going?!");
        assert_eq!(quality("Get in the car. ?!").text, "Get in the car.");
        // Üç nokta ve diyalog tiresi korunur.
        assert_eq!(quality("... Where?").text, "... Where?");
        assert_eq!(quality("- Run! - Where?").text, "- Run! - Where?");
        assert_eq!(quality("?!").text, "?!");
    }

    #[test]
    fn hud_ve_diyalog_birlesmesi_ayrilir() {
        assert_eq!(quality("23m CESARE: Here!").text, "CESARE: Here!");
        assert_eq!(quality("100. ARTHUR: Let's go.").text, "ARTHUR: Let's go.");
        // Sayı cümlenin parçasıysa dokunulmaz.
        assert_eq!(quality("3 Wolves remaining").text, "3 Wolves remaining");
        assert_eq!(quality("9? We'll lose them.").text, "9? We'll lose them.");
        assert_eq!(quality("2 Objective: Find the key.").text, "2 Objective: Find the key.");
    }

    #[test]
    fn guven_puana_gore_duser_tam_puanda_degismez() {
        assert_eq!(adjusted_confidence(0.97, 1.0, 0.4), 0.97);
        assert!(adjusted_confidence(0.97, 0.5, 0.4) < 0.9);
        assert!(adjusted_confidence(0.9, 0.35, 0.4) < 0.6);
    }

    #[test]
    fn kalite_cezasi_birlestirici_esiginin_altina_dusurmez() {
        // Sınır değerler: eşiği geçen satır eşikte kalır (sessizce kaybolmaz), geçemeyen değişmez.
        assert_eq!(adjusted_confidence(0.55, 0.5, 0.4), 0.4);
        assert_eq!(adjusted_confidence(0.4, REJECT_SCORE, 0.4), 0.4);
        assert!((adjusted_confidence(0.3, 1.0, 0.4) - 0.3).abs() < 1e-12);
        assert!(adjusted_confidence(0.3, 0.5, 0.4) < 0.3);
        // Şüpheli puan ve düşük güvenin sınırı `reject`'tedir.
        let doubtful = Quality { text: "Gtt here wh4t".into(), score: DOUBTFUL_SCORE - 0.01 };
        assert!(reject(&doubtful, DOUBTFUL_CONFIDENCE - 0.01));
        assert!(!reject(&doubtful, DOUBTFUL_CONFIDENCE));
        let fine = Quality { score: DOUBTFUL_SCORE, ..doubtful.clone() };
        assert!(!reject(&fine, 0.1));
        let garbage = Quality { score: REJECT_SCORE - 0.01, ..doubtful };
        assert!(reject(&garbage, 1.0));
    }

    #[test]
    fn kisa_unlemler_atilmaz() {
        for text in ["Ngh!", "Hngh.", "Nnngh!", "Pssh.", "Hrrm.", "Mmph!", "Tch.", "Grrr!", "Zzz...", "Oh!", "Ugh.", "Ow!", "Hmm?", "Shh!"] {
            let q = quality(text);
            assert!(!reject(&q, 0.9), "{text} atılmamalı ({})", q.score);
        }
        // Bilinmeyen ünsüz dizisi yarım çöptür: tek başına atılmaz, güveni düşer.
        let q = quality("Krrk!");
        assert_eq!(q.score, 0.5);
        assert!(!reject(&q, 0.9));
    }

    #[test]
    fn benzer_harfler_karistirilmaz() {
        // I / l / | / 1 / O / 0: yalnızca kesin durumlar düzeltilir, gerisi olduğu gibi kalır.
        assert_eq!(quality("| know").text, "I know");
        assert_eq!(quality("l know").text, "I know");
        assert_eq!(quality("1 know").text, "1 know");
        assert_eq!(quality("Room 101 on floor 0").text, "Room 101 on floor 0");
        assert_eq!(quality("O Captain").text, "O Captain");
        assert!(!reject(&quality("I"), 0.9));
        assert!(!reject(&quality("O"), 0.9));
        assert!(reject(&quality("l"), 0.9));
        // Rakamla bozulmuş sözcük puanı düşürür ama satır atılmaz.
        let q = quality("He11o there");
        assert!(q.score < 1.0 && !reject(&q, 0.9));
    }

    #[test]
    fn oyun_metinleri_korunur() {
        for text in ["R2-D2 is here", "T-800 model", "LVL 99", "DLC", "Hmm.", "Shh!", "0K", "4x4 truck", "MK2 rifle", "B2B deal", "Lv5"] {
            let q = quality(text);
            assert_eq!(q.score, 1.0, "{text}");
            assert_eq!(q.text, text);
        }
    }

    #[test]
    fn harf_olmayan_metin_cevrilmez() {
        assert!(has_letters("Level 3"));
        assert!(has_letters("Çık"));
        assert!(!has_letters("100"));
        assert!(!has_letters("3/5 — 12:40"));
    }
}
