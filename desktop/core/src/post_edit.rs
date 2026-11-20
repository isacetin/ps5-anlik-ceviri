//! Çevirinin Türkçesine, TV'ye gitmeden önce yapılan güvenli düzeltmeler. Çeviri motorundan
//! bağımsızdır (yerel Opus-MT de DeepL de aynı katmandan geçer) ve yalnızca gösterimde uygulanır:
//! çeviri belleğine ham çeviri yazılır, kural değişirse eski kayıtlar bozulmaz.
//!
//! Yalnızca anlamı değiştirmeyen biçim kuralları vardır; sözcüklere ve Türkçe harflere dokunulmaz:
//!
//! - Art arda boşluklar teke iner; noktalamadan önceki boşluk silinir ("Dur !" → "Dur!",
//!   "Ne ?" → "Ne?"). Saat gibi iki yanı rakam olan iki noktaya dokunulmaz ("12 : 40").
//! - Model kaynaklı yinelenen işaretler: ",," → ",", "!." → "!", "?." → "?", ".," → ",";
//!   kaynakta olmayan ".." → "." (kaynakta üç nokta varsa "...").
//! - Kaynağın anlamlı sonu korunur: kaynak `?`, `!`, `?!`, `…` ya da `...` ile bitiyorsa çeviri de
//!   öyle biter (model "Koş!"u "Koş." yapabiliyor, soru işaretini düşürebiliyor). Kaynak nokta ile
//!   bitiyor ya da işaretsizse çevirinin sonuna dokunulmaz.
//! - Dar bir hitap kuralı (bkz. `informal_imperatives`): aynı satırda "sen" ile konuşulurken model
//!   emri "siz"li verdiyse ("Sana söyledim. Bekleyin!") yalnızca listedeki emirler "sen"li olur.

/// Düzeltilmiş çeviri. `source`: çevrilen İngilizce metin (adlar ve terimler açılmış hâliyle).
/// Çeviri boşsa olduğu gibi döner.
pub fn apply(source: &str, translated: &str) -> String {
    if translated.trim().is_empty() {
        return translated.trim().to_string();
    }
    let text = collapse_spaces(translated);
    let text = remove_space_before_punctuation(&text);
    let text = fix_repeated_punctuation(source, &text);
    let text = restore_ending(source, &text);
    informal_imperatives(source, &text)
}

fn collapse_spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn remove_space_before_punctuation(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' && i > 0 {
            let next = chars.get(i + 1).copied();
            let after = chars.get(i + 2).copied();
            let previous = chars[i - 1];
            let attach = match next {
                Some(',' | '!' | '?' | ';' | ')' | ']' | '»') => true,
                // Nokta ve üç nokta: ardından boşluk/son/kapanış gelmeli (" .com" gibi değil).
                Some('.' | '…') => after.is_none_or(|a| a == ' ' || a == '.' || a == '!' || a == '?' || a == '"' || a == '”' || a == '\''),
                // "12 : 40" saati korunur.
                Some(':') => !(previous.is_ascii_digit() && after.is_some_and(|a| a == ' ' || a.is_ascii_digit()) && chars.get(i + 3).is_some_and(char::is_ascii_digit)),
                _ => false,
            };
            // Satır başındaki "... Neden?" ya da tire/tırnaktan sonraki boşluk olduğu gibi kalır.
            if attach && !matches!(previous, '-' | '–' | '—' | '"' | '“' | '(' | '[') {
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn fix_repeated_punctuation(source: &str, text: &str) -> String {
    let source_ellipsis = source.contains('…') || source.contains("...");
    let source_double_comma = source.contains(",,");
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match (c, next) {
            (',', Some(',')) if !source_double_comma => {
                i += 1;
                continue;
            }
            // "Gel!." → "Gel!", "Ne?.." → "Ne?"; "Ne?..." (üç nokta) kalır.
            ('.', _) if out.last().is_some_and(|p| matches!(p, '!' | '?')) => {
                let run = chars[i..].iter().take_while(|&&d| d == '.').count();
                if run == 3 {
                    out.extend("...".chars());
                }
                i += run;
                continue;
            }
            // ".," ve ",." → ","
            ('.', Some(',')) if out.last() != Some(&'.') => {
                i += 1;
                continue;
            }
            (',', Some('.')) if chars.get(i + 2) != Some(&'.') => {
                out.push(',');
                i += 2;
                continue;
            }
            ('.', Some('.')) => {
                // Nokta dizisi: tam olarak iki nokta modelin hatasıdır.
                let run = chars[i..].iter().take_while(|&&d| d == '.').count();
                if run == 2 {
                    out.extend(if source_ellipsis { "..." } else { "." }.chars());
                } else {
                    out.extend(std::iter::repeat_n('.', run));
                }
                i += run;
                continue;
            }
            _ => {}
        }
        out.push(c);
        i += 1;
    }
    out.into_iter().collect()
}

/// Metnin sonu: gövde, son noktalama (. ! ? … dizisi) ve kapanış tırnak/parantezleri.
fn split_ending(text: &str) -> (&str, &str, &str) {
    let closing_start = text.trim_end_matches(['"', '”', '\'', '’', ')', ']', '»', ' ']).len();
    let body_end = text[..closing_start].trim_end_matches(['.', '!', '?', '…']).len();
    (&text[..body_end], &text[body_end..closing_start], &text[closing_start..])
}

/// Kaynağın soru/ünlem/üç nokta sonu çeviride kaybolduysa geri konur.
fn restore_ending(source: &str, text: &str) -> String {
    let (_, source_end, _) = split_ending(source.trim());
    let meaningful = source_end.contains(['?', '!', '…']) || source_end.contains("...");
    if !meaningful {
        return text.to_string();
    }
    let (body, end, closing) = split_ending(text);
    if body.trim().is_empty() || same_ending(source_end, end) {
        return text.to_string();
    }
    // Kaynakta son işaret tırnağın içindeyse (He said "Run!") çeviride tırnak sonda olmayabilir;
    // kaynakla çevirinin tırnak düzeni farklıysa dokunulmaz.
    let (_, _, source_closing) = split_ending(source.trim());
    if source_closing.trim().is_empty() != closing.trim().is_empty() {
        return text.to_string();
    }
    format!("{body}{}{closing}", normalize_ending(source_end))
}

/// "…" ile "...", "?!" ile "!?" ve "!!!" ile "!" aynı anlamdadır.
fn same_ending(a: &str, b: &str) -> bool {
    let key = |s: &str| {
        let s = s.replace("...", "…");
        let mut marks: Vec<char> = s.chars().filter(|c| matches!(c, '?' | '!' | '…')).collect();
        marks.sort_unstable();
        marks.dedup();
        marks
    };
    key(a) == key(b)
}

/// Kaynağın sonu, çevirinin sonu olarak: noktalı karışım ("?.") sadeleşir.
fn normalize_ending(end: &str) -> String {
    let has = |c: char| end.contains(c);
    if has('…') || end.contains("...") {
        let tail: String = end.chars().filter(|c| matches!(c, '?' | '!')).collect();
        return if end.contains("...") { format!("...{tail}") } else { format!("…{tail}") };
    }
    end.chars().filter(|c| matches!(c, '?' | '!')).collect()
}

/// "Sen"li konuşmada model emri "siz"li verdiyse yaygın emirler "sen"li olur. Çok dar tutulur:
///
/// - Çeviride "sen" ile hitap eden bir sözcük (sen, seni, sana, senin, sende, senden, seninle)
///   olmalı, "siz" ile hitap eden hiçbir sözcük olmamalı.
/// - Kaynakta çoğul hitap ("you guys", "all of you", "everyone", "you two", "y'all") olmamalı.
/// - Yalnızca listedeki emirler, yalnızca emir yerindeyse (ardından noktalama ya da son gelir).
///   Çoğul ya da kibar anlamı da olabilen ya da başka anlamı olan sözcükler ("Gelin", "Alın",
///   "Kaçın", "Olun") bilerek listede yok.
fn informal_imperatives(source: &str, text: &str) -> String {
    const INFORMAL: &[&str] = &["sen", "seni", "sana", "senin", "sende", "senden", "seninle", "sensin", "senle"];
    const FORMAL: &[&str] = &["siz", "sizi", "size", "sizin", "sizde", "sizden", "sizinle", "sizsiniz", "sizler", "efendim"];
    const PLURAL: &[&str] = &["you guys", "all of you", "everyone", "everybody", "you two", "you three", "y'all", "you all", "both of you", "you people", "men", "boys", "girls", "team", "squad"];
    const IMPERATIVES: &[(&str, &str)] = &[
        ("bekleyin", "bekle"),
        ("durun", "dur"),
        ("koşun", "koş"),
        ("bakın", "bak"),
        ("dinleyin", "dinle"),
        ("gidin", "git"),
        ("saklanın", "saklan"),
        ("susun", "sus"),
        ("yapın", "yap"),
        ("atlayın", "atla"),
        ("bırakın", "bırak"),
        ("çekilin", "çekil"),
        ("kalkın", "kalk"),
        ("eğilin", "eğil"),
        ("uzaklaşın", "uzaklaş"),
        ("izleyin", "izle"),
        ("unutmayın", "unutma"),
        ("korkmayın", "korkma"),
        ("ateş edin", "ateş et"),
        ("yardım edin", "yardım et"),
        ("acele edin", "acele et"),
        ("dikkat edin", "dikkat et"),
        ("takip edin", "takip et"),
        ("sakin olun", "sakin ol"),
    ];
    let words: Vec<String> = text.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()).map(turkish_lower).collect();
    let informal = words.iter().any(|w| INFORMAL.contains(&w.as_str()));
    let formal = words.iter().any(|w| FORMAL.contains(&w.as_str()));
    let source_lower = format!(" {} ", source.to_lowercase().replace(|c: char| !c.is_alphanumeric() && c != '\'', " "));
    let plural = PLURAL.iter().any(|p| source_lower.contains(&format!(" {p} ")));
    if !informal || formal || plural {
        return text.to_string();
    }
    let mut out = text.to_string();
    for (polite, plain) in IMPERATIVES {
        out = replace_imperative(&out, polite, plain);
    }
    out
}

/// `polite` emrini (ardından noktalama ya da metin sonu, önünde sözcük sınırı) `plain` yapar; baş
/// harfin büyüklüğü korunur. Karşılaştırma karakter karakter, Türkçe küçük harfle yapılır.
fn replace_imperative(text: &str, polite: &str, plain: &str) -> String {
    let original: Vec<char> = text.chars().collect();
    let lower: Vec<char> = original.iter().map(|c| turkish_lower_char(*c)).collect();
    let wanted: Vec<char> = polite.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < original.len() {
        let matches = lower[i..].starts_with(&wanted)
            && (i == 0 || !lower[i - 1].is_alphabetic())
            && lower.get(i + wanted.len()).is_none_or(|c| matches!(c, '!' | '.' | ',' | '?' | '…' | '"' | '”' | ')'));
        if matches {
            let segment = &original[i..i + wanted.len()];
            if segment.iter().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()) && segment.len() > 1 {
                // Tümü büyük harf ("BEKLEYİN!") → "BEKLE!"
                out.extend(plain.chars().flat_map(|c| turkish_upper(c).chars().collect::<Vec<_>>()));
                i += wanted.len();
                continue;
            }
            let mut plain_chars = plain.chars();
            if original[i].is_uppercase() {
                if let Some(first) = plain_chars.next() {
                    out.push_str(&turkish_upper(first));
                }
            }
            out.extend(plain_chars);
            i += wanted.len();
        } else {
            out.push(original[i]);
            i += 1;
        }
    }
    out
}

fn turkish_lower(text: &str) -> String {
    text.chars().map(turkish_lower_char).collect()
}

fn turkish_lower_char(c: char) -> char {
    match c {
        'I' => 'ı',
        'İ' => 'i',
        c => c.to_lowercase().next().unwrap_or(c),
    }
}

fn turkish_upper(c: char) -> String {
    match c {
        'i' => "İ".to_string(),
        'ı' => "I".to_string(),
        c => c.to_uppercase().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noktalamadan_once_bosluk_silinir() {
        assert_eq!(apply("Stop!", "Dur !"), "Dur!");
        assert_eq!(apply("What?", "Ne ?"), "Ne?");
        assert_eq!(apply("Well, okay.", "Peki , tamam ."), "Peki, tamam.");
        assert_eq!(apply("Go.", "  Git   hemen  . "), "Git hemen.");
        // Saat ve satır başındaki üç nokta korunur.
        assert_eq!(apply("At 12:40.", "Saat 12 : 40'ta."), "Saat 12 : 40'ta.");
        assert_eq!(apply("... Why?", "... Neden?"), "... Neden?");
        assert_eq!(apply("Visit site.com", "site .com adresine git"), "site .com adresine git");
    }

    #[test]
    fn kaynagin_soru_unlem_ve_uc_nokta_sonu_korunur() {
        assert_eq!(apply("Run!", "Koş."), "Koş!");
        assert_eq!(apply("Where is he?", "O nerede."), "O nerede?");
        assert_eq!(apply("Where is he?", "O nerede"), "O nerede?");
        assert_eq!(apply("What?!", "Ne?"), "Ne?!");
        assert_eq!(apply("I don't know…", "Bilmiyorum."), "Bilmiyorum…");
        assert_eq!(apply("I don't know...", "Bilmiyorum."), "Bilmiyorum...");
        // Eşdeğer sonlara dokunulmaz.
        assert_eq!(apply("I don't know...", "Bilmiyorum…"), "Bilmiyorum…");
        assert_eq!(apply("What!?", "Ne?!"), "Ne?!");
        assert_eq!(apply("No!!!", "Hayır!"), "Hayır!");
        // Kaynak nokta ile bitiyor ya da işaretsizse model sonu olduğu gibi kalır.
        assert_eq!(apply("Wait", "Bekle."), "Bekle.");
        assert_eq!(apply("Fine.", "Peki!"), "Peki!");
    }

    #[test]
    fn tirnak_ve_parantez_icindeki_son_korunur() {
        assert_eq!(apply("\"Run!\"", "\"Koş.\""), "\"Koş!\"");
        assert_eq!(apply("(Really?)", "(Gerçekten.)"), "(Gerçekten?)");
        // Tırnak düzeni farklıysa dokunulmaz.
        assert_eq!(apply("He said \"Run!\"", "\"Koş\" dedi."), "\"Koş\" dedi.");
    }

    #[test]
    fn yinelenen_noktalama_duzelir() {
        assert_eq!(apply("Wait, what?", "Bekle,, ne?"), "Bekle, ne?");
        assert_eq!(apply("Come here!", "Buraya gel!."), "Buraya gel!");
        assert_eq!(apply("Who?", "Kim?."), "Kim?");
        assert_eq!(apply("Go.", "Git.."), "Git.");
        assert_eq!(apply("Well...", "Şey.."), "Şey...");
        assert_eq!(apply("Yes, sir.", "Evet., efendim."), "Evet, efendim.");
        // Gerçek üç nokta ve "?!" korunur.
        assert_eq!(apply("Wait... what?!", "Bekle... ne?!"), "Bekle... ne?!");
    }

    #[test]
    fn turkce_karakterler_bozulmaz() {
        let text = "Çığlık atan şoför, İzmir'e gitmiş ğüöşıç İĞÜŞÖÇ.";
        assert_eq!(apply("The screaming driver went to Izmir.", text), text);
        assert_eq!(apply("Hurry!", "Acele et, Işıl ."), "Acele et, Işıl!");
    }

    #[test]
    fn bos_ya_da_bozuk_ceviri_guvenle_doner() {
        assert_eq!(apply("Run!", ""), "");
        assert_eq!(apply("Run!", "   "), "");
        assert_eq!(apply("Run!", "!!!"), "!!!");
        assert_eq!(apply("Run!", "?"), "?");
        assert_eq!(apply("", "Koş"), "Koş");
        assert_eq!(apply("Run!", "…"), "…");
    }

    #[test]
    fn kisa_emir_cumleleri() {
        assert_eq!(apply("Stop!", "Dur ."), "Dur!");
        assert_eq!(apply("Wait!", "Bekleyin!"), "Bekleyin!");
        assert_eq!(apply("Go.", "Git."), "Git.");
        assert_eq!(apply("Hide!", "Saklanın"), "Saklanın!");
    }

    #[test]
    fn sen_siz_yalnizca_guvenli_durumda_degisir() {
        // Aynı satırda "sen" var, "siz" yok: listedeki emir "sen"li olur.
        assert_eq!(apply("I told you. Wait!", "Sana söyledim. Bekleyin!"), "Sana söyledim. Bekle!");
        assert_eq!(apply("Your fault. Stop, hide!", "Senin hatan. Durun, saklanın!"), "Senin hatan. Dur, saklan!");
        assert_eq!(apply("Help me, I need you!", "Sana ihtiyacım var, yardım edin!"), "Sana ihtiyacım var, yardım et!");
        // Büyük I/İ içeren satırda da çalışır, Türkçe harfler bozulmaz.
        assert_eq!(apply("Isil, I told you. Watch!", "Işıl, İzmir'de sana söyledim. İzleyin!"), "Işıl, İzmir'de sana söyledim. İzle!");
        // "Siz" de geçiyorsa (kibar ya da çoğul hitap) dokunulmaz.
        assert_eq!(apply("I told you. Wait!", "Size söyledim. Bekleyin!"), "Size söyledim. Bekleyin!");
        // "Sen" hiç geçmiyorsa dokunulmaz.
        assert_eq!(apply("Wait!", "Bekleyin!"), "Bekleyin!");
        // Kaynakta çoğul hitap varsa dokunulmaz.
        assert_eq!(apply("You guys wait, I'll show you!", "Siz bekleyin, sana göstereceğim!"), "Siz bekleyin, sana göstereceğim!");
        assert_eq!(apply("Everyone wait, I'll show you!", "Bekleyin, sana göstereceğim!"), "Bekleyin, sana göstereceğim!");
        // Başka anlamı olabilen sözcükler ("Gelin" = gelin; "Alın" = alın) listede yok.
        assert_eq!(apply("Come, I'll show you.", "Gelin, sana göstereceğim."), "Gelin, sana göstereceğim.");
        // Emir yerinde olmayan biçim ("bekleyince") ve sözcük içi eşleşme değişmez.
        assert_eq!(apply("When you wait, you see.", "Sen bekleyince görürsün."), "Sen bekleyince görürsün.");
        assert_eq!(apply("Watch them, I trust you.", "Onları gözetleyin, sana güveniyorum."), "Onları gözetleyin, sana güveniyorum.");
    }

    #[test]
    fn noktalama_kombinasyonlari() {
        assert_eq!(apply("Come!", "Gel!.."), "Gel!");
        assert_eq!(apply("Really?...", "Gerçekten?..."), "Gerçekten?...");
        assert_eq!(apply("What?!", "Ne!?"), "Ne!?");
        assert_eq!(apply("What!?", "Ne."), "Ne!?");
        assert_eq!(apply("Why?", "Neden ?!"), "Neden?");
        assert_eq!(apply("No…?", "Hayır."), "Hayır…?");
        assert_eq!(apply("Hi...", "Selam . . ."), "Selam...");
        assert_eq!(apply("Okay!", "Tamam ,, peki"), "Tamam, peki!");
        // Kaynak ünlemle bitmiyor ama çeviri bitiyor: dokunulmaz.
        assert_eq!(apply("Okay.", "Tamam!"), "Tamam!");
    }

    #[test]
    fn turkce_buyuk_kucuk_harf_korunur() {
        assert_eq!(apply("I told you. Wait!", "SANA SÖYLEDİM. BEKLEYİN!"), "SANA SÖYLEDİM. BEKLE!");
        assert_eq!(apply("I told you. Watch!", "SANA SÖYLEDİM. İZLEYİN!"), "SANA SÖYLEDİM. İZLE!");
        assert_eq!(apply("I told you. Watch!", "Sana söyledim. İzleyin!"), "Sana söyledim. İzle!");
        assert_eq!(apply("I told you. Stop!", "sana söyledim. durun!"), "sana söyledim. dur!");
        assert_eq!(apply("ISTANBUL?", "İSTANBUL IŞIKLARI."), "İSTANBUL IŞIKLARI?");
    }

    #[test]
    fn iki_kez_uygulamak_metni_degistirmez() {
        let sources = ["Come!", "What?!", "What!?", "Why?", "Well...", "Wait…", "No…?", "Okay.", "Okay!", "I told you. Wait!", "Run!", "\"Run!\"", "At 12:40.", ""];
        let translations = [
            "Gel!..", "Gel!.", "Ne?.", "Ne !?", "Şey..", "Bekle ...", "Selam . . .", "Tamam ,, peki", "Sana dedim. BEKLEYİN!", "Sana dedim. Bekleyin!..",
            "\"Koş\".", "\"Koş.\"", "Saat 12 : 40'ta.", "Evet., efendim", "Kim?.", "", "   ", "!!!", "…", "Çığlık ,İzmir ?!", "Yapamam.. Sadece yapamam.",
        ];
        for source in sources {
            for translation in translations {
                let once = apply(source, translation);
                assert_eq!(apply(source, &once), once, "kaynak {source:?}, çeviri {translation:?}");
            }
        }
    }

    #[test]
    fn opus_mt_ciktilari() {
        // Opus-MT'nin sık yaptığı biçim hataları.
        assert_eq!(apply("Are you okay?", "İyi misin ?"), "İyi misin?");
        assert_eq!(apply("Get down!", "Yere yat."), "Yere yat!");
        assert_eq!(apply("I can't... I just can't.", "Yapamam.. Sadece yapamam."), "Yapamam... Sadece yapamam.");
        assert_eq!(apply("Who's there?", "Kim var orada"), "Kim var orada?");
        assert_eq!(apply("Kratos, wait!", "Kratos , bekle !"), "Kratos, bekle!");
        assert_eq!(apply("The door is locked.", "Kapı kilitli."), "Kapı kilitli.");
    }
}
