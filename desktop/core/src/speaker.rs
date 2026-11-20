//! Altyazının başındaki konuşan etiketi ("ARTHUR: We need to go."). Etiket çeviri motoruna
//! gönderilmez: motorlar adı çeviriyor, cümleye katıyor ya da siliyordu; aynı replik etiketli ve
//! etiketsiz geldiğinde de ayrı önbellek kaydı açılıyordu. Motora yalnızca cümle gider, gösterimde
//! etiket olduğu gibi başa konur ("ARTHUR: Gitmemiz gerekiyor.").
//!
//! Kurallar temkinlidir; emin olunmayan satır olduğu gibi (tek parça) çevrilir:
//!
//! - Etiket satırın başındadır, iki nokta (`:` ya da tam genişlikli `：`) ve boşlukla biter.
//! - 1-3 sözcük, en çok 24 karakter; rakam ve harf dışı işaret yok (yalnızca `'` `’` `.` `-`).
//! - Ya tümü büyük harf ("ARTHUR", "OLD MAN") ya da her sözcük büyük harfle başlar ("Arthur",
//!   "Old Man"). Karışık yazım ("ArTHUR") OCR bozulması sayılır, etiket kabul edilmez.
//! - "Note:", "Objective:", "Warning:" gibi oyun başlıkları etiket değildir.
//! - Baş harfi büyük tek sözcüklü etikette ("Remember: …") cümle de büyük harfle (ya da tırnak,
//!   tire, üç nokta ile) başlamalıdır.
//! - Bilinen ad (oyun profilinin karakterleri, korunan adlar) başlık listesine ve son kurala
//!   takılmaz; biçim kuralları ona da uygulanır.
//! - Etiketten sonra harfli bir cümle olmalı: "Time: 10:30", "https://…", "16:9" etiket değildir.

/// Etiketi ayrılmış satır.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeakerLine<'a> {
    /// Konuşan adı, ekrandaki yazımıyla ("ARTHUR", "Old Man").
    pub label: &'a str,
    /// Çevrilecek cümle.
    pub body: &'a str,
}

const MAX_WORDS: usize = 3;
const MAX_LABEL_CHARS: usize = 24;

/// Etiket sanılmaması gereken oyun başlıkları ve kısaltmalar (küçük harf).
const HEADINGS: &[&str] = &[
    "note", "notes", "objective", "objectives", "new objective", "optional", "optional objective", "main objective", "tip", "tips",
    "pro tip", "hint", "hints", "warning", "caution", "alert", "attention", "important", "notice", "quest", "new quest", "side quest",
    "mission", "new mission", "goal", "goals", "reward", "rewards", "location", "region", "area", "zone", "chapter", "part", "act",
    "episode", "level", "lvl", "rank", "time", "date", "day", "night", "status", "press", "hold", "tutorial", "info", "information",
    "error", "example", "question", "answer", "rule", "rules", "step", "difficulty", "mode", "score", "total", "bonus", "cost",
    "price", "requires", "required", "requirement", "requirements", "effect", "effects", "duration", "damage", "health", "stamina",
    "ammo", "weight", "value", "type", "category", "target", "current", "next", "previous", "controls", "control", "action",
    "update", "news", "title", "subject", "from", "to", "re", "ps", "fyi", "summary", "description", "result", "results",
    "unlocked", "new", "completed", "complete", "failed", "item", "items", "skill", "ability", "trophy", "achievement",
    "trophy unlocked", "achievement unlocked", "saving", "loading", "did you know", "remember", "also", "hp", "mp", "xp", "sp",
    "ap", "ok", "yes", "no", "on", "off", "and", "or", "but", "so", "then", "now", "here", "there", "the", "a", "an", "this",
    "that", "it", "i", "we", "you", "he", "she", "they", "speed", "distance", "money", "cash", "wanted", "bounty", "honor",
    "honour", "version", "build", "platform", "language", "subtitles", "audio", "video", "volume", "brightness", "location found",
];

/// Satırın başında güvenilir bir konuşan etiketi varsa etiketi ve cümleyi döner; emin değilse `None`.
/// `is_known`: ad oyunun bilinen bir karakteri ya da korunan adı mı (büyük/küçük harf duyarsız).
pub fn split<'a>(text: &'a str, is_known: impl Fn(&str) -> bool) -> Option<SpeakerLine<'a>> {
    let text = text.trim_start();
    let (colon, colon_len) = text.char_indices().take(MAX_LABEL_CHARS + 2).find(|(_, c)| matches!(c, ':' | '：')).map(|(i, c)| (i, c.len_utf8()))?;
    let label = text[..colon].trim_end();
    let rest = &text[colon + colon_len..];
    // "ARTHUR :" (OCR boşluğu) kabul; ad ile iki nokta arasında birden çok boşluk değil.
    if label.is_empty() || colon - label.len() > 1 || !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let body = rest.trim();
    if !body.chars().any(char::is_alphabetic) || body.starts_with([':', '：', '/']) {
        return None;
    }
    let words: Vec<&str> = label.split_whitespace().collect();
    if words.is_empty() || words.len() > MAX_WORDS || label.chars().count() > MAX_LABEL_CHARS {
        return None;
    }
    let styles: Option<Vec<Style>> = words.iter().map(|w| word_style(w)).collect();
    let styles = styles?;
    let all_caps = styles.iter().all(|s| *s == Style::Caps);
    let all_title = styles.iter().all(|s| matches!(s, Style::Title | Style::Initial));
    if !(all_caps || all_title) || label.chars().filter(|c| c.is_alphabetic()).count() < 2 {
        return None;
    }
    if is_known(label) {
        return Some(SpeakerLine { label, body });
    }
    let lower = words.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join(" ");
    if HEADINGS.contains(&lower.trim_end_matches('.')) {
        return None;
    }
    // "Remember: always…" gibi tek sözcüklü başlıklar: cümle büyük harfle başlamıyorsa etiket değil.
    if words.len() == 1 && !all_caps && !starts_sentence(body) {
        return None;
    }
    Some(SpeakerLine { label, body })
}

/// Gösterilecek metin: etiket olduğu gibi, ardından çeviri.
pub fn join(label: &str, translated: &str) -> String {
    format!("{label}: {}", translated.trim())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    /// Tümü büyük harf ("ARTHUR").
    Caps,
    /// Baş harf büyük, devamında küçük harf var ("Arthur", "McCree").
    Title,
    /// Tek büyük harf ya da baş harf kısaltması ("J.", "J") — iki biçime de uyar.
    Initial,
}

fn word_style(word: &str) -> Option<Style> {
    let mut chars = word.chars();
    let first = chars.next()?;
    if !first.is_alphabetic() || !first.is_uppercase() {
        return None;
    }
    if !word.chars().all(|c| c.is_alphabetic() || matches!(c, '\'' | '’' | '.' | '-')) {
        return None;
    }
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() == 1 {
        return Some(Style::Initial);
    }
    if letters.iter().all(|c| c.is_uppercase()) {
        return Some(Style::Caps);
    }
    // Baş harften sonra küçük harfle başlayıp yeniden büyüğe geçme en çok bir kez ("McCree",
    // "DeLuca"); "ArTHuR" gibi dalgalı yazım OCR karışıklığıdır.
    let rises = letters.windows(2).filter(|w| w[0].is_lowercase() && w[1].is_uppercase()).count();
    let second_lower = letters[1].is_lowercase();
    (second_lower && rises <= 1).then_some(Style::Title)
}

fn starts_sentence(body: &str) -> bool {
    body.chars().next().is_some_and(|c| c.is_uppercase() || matches!(c, '"' | '“' | '\'' | '‘' | '-' | '–' | '—' | '…' | '.' | '(' | '¿' | '¡'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unknown(_: &str) -> bool {
        false
    }

    fn parts(text: &str) -> Option<(&str, &str)> {
        split(text, unknown).map(|s| (s.label, s.body))
    }

    #[test]
    fn buyuk_harfli_etiket_ayrilir() {
        assert_eq!(parts("ARTHUR: We need to go."), Some(("ARTHUR", "We need to go.")));
        assert_eq!(parts("GERALT: Hmm."), Some(("GERALT", "Hmm.")));
        assert_eq!(parts("OLD MAN: The gate will not hold."), Some(("OLD MAN", "The gate will not hold.")));
        assert_eq!(parts("DUTCH： Easy, son."), Some(("DUTCH", "Easy, son.")));
        assert_eq!(parts("ARTHUR : Come on."), Some(("ARTHUR", "Come on.")));
        // Büyük harfli etiketten sonra küçük harfle devam eden cümle de olur.
        assert_eq!(parts("ARTHUR: well, maybe."), Some(("ARTHUR", "well, maybe.")));
    }

    #[test]
    fn bas_harfi_buyuk_etiket_ayrilir() {
        assert_eq!(parts("Arthur: We need to go."), Some(("Arthur", "We need to go.")));
        assert_eq!(parts("Mimir Head: Brother, look!"), Some(("Mimir Head", "Brother, look!")));
        assert_eq!(parts("Mr. Smith: Hello there."), Some(("Mr. Smith", "Hello there.")));
        assert_eq!(parts("Sadie: \"Move!\""), Some(("Sadie", "\"Move!\"")));
        assert_eq!(parts("McCree: It's high noon."), Some(("McCree", "It's high noon.")));
    }

    #[test]
    fn normal_cumledeki_iki_nokta_etiket_degildir() {
        assert_eq!(parts("There is one thing left: the key."), None);
        assert_eq!(parts("Remember: always check the map."), None);
        assert_eq!(parts("Time: 10:30"), None);
        assert_eq!(parts("Visit https://example.com today"), None);
        assert_eq!(parts("Aspect ratio 16:9"), None);
        assert_eq!(parts("We have three options: run, hide or fight."), None);
        assert_eq!(parts("Score: 1200"), None);
    }

    #[test]
    fn oyun_basliklari_etiket_degildir() {
        assert_eq!(parts("Objective: Find the key."), None);
        assert_eq!(parts("OBJECTIVE: Find the key."), None);
        assert_eq!(parts("New Objective: Escape the mine."), None);
        assert_eq!(parts("Tip: Hold R2 to sprint."), None);
        assert_eq!(parts("WARNING: Low health."), None);
        assert_eq!(parts("Note: The door is locked."), None);
    }

    #[test]
    fn ocr_bozuk_etiket_kabul_edilmez() {
        assert_eq!(parts("ART4UR: We need to go."), None);
        assert_eq!(parts("ArTHuR: We need to go."), None);
        assert_eq!(parts("ARTHUR; We need to go."), None);
        assert_eq!(parts("ARTHUR:We need to go."), None);
        assert_eq!(parts("AR#THUR: We need to go."), None);
        assert_eq!(parts("arthur: we need to go."), None);
        assert_eq!(parts("ARTHUR:"), None);
        assert_eq!(parts("ARTHUR: ..."), None);
        assert_eq!(parts("A: Yes."), None);
    }

    #[test]
    fn etiketsiz_metin_oldugu_gibi_kalir() {
        assert_eq!(parts("We need to go."), None);
        assert_eq!(parts("Hmm."), None);
        assert_eq!(parts(""), None);
        assert_eq!(parts("- Run! - Where?"), None);
    }

    #[test]
    fn uzun_ya_da_cok_sozcuklu_etiket_kabul_edilmez() {
        assert_eq!(parts("The Old Man Of The Mountain: Welcome."), None);
        assert_eq!(parts("This Is A Very Long Label Name: Hi."), None);
    }

    #[test]
    fn bilinen_ad_baslik_kurallarina_takilmaz() {
        let known = |label: &str| label.eq_ignore_ascii_case("Hope") || label.eq_ignore_ascii_case("Note");
        // "Hope" bilinmeseydi tek sözcüklü başlık gibi küçük harfle devam eden cümlede reddedilirdi.
        assert_eq!(split("Hope: we made it.", known).map(|s| s.label), Some("Hope"));
        assert_eq!(split("Hope: we made it.", unknown), None);
        assert_eq!(split("Note: Over here!", known).map(|s| s.label), Some("Note"));
        // Biçim kuralları bilinen ada da uygulanır.
        assert_eq!(split("H0pe: We made it.", known), None);
    }

    #[test]
    fn gosterimde_etiket_basa_konur() {
        assert_eq!(join("ARTHUR", " Gitmemiz gerekiyor. "), "ARTHUR: Gitmemiz gerekiyor.");
    }
}
