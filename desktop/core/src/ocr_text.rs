//! OCR çıktısının hafif temizliği. Birleştirme ve sabitlemeden önce her satıra uygulanır; böylece
//! aynı yazının küçük okuma farkları (fazla boşluk, görünmez karakter) farklı metin sayılmaz,
//! önbellek anahtarı da tutarlı kalır. Anlamı değiştirebilecek düzeltme yapılmaz.

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

    #[test]
    fn harf_olmayan_metin_cevrilmez() {
        assert!(has_letters("Level 3"));
        assert!(has_letters("Çık"));
        assert!(!has_letters("100"));
        assert!(!has_letters("3/5 — 12:40"));
    }
}
