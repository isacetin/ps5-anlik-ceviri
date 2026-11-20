//! Oyun profili: bir oyunun çeviri ayarları, karakter adları, terim sözlüğü ve kullanıcı
//! düzeltmeleri. `app_dir/profiles/<id>.json` dosyasında saklanır; kullanıcı arayüzden seçer
//! (otomatik oyun tanıma yok). Yeni oyun eklemek yeni bir dosyadır; kod değişmez.
//!
//! Kullanıcı düzeltmesi yalnızca kendi profiline uygulanır (tüm oyunlara değil):
//! - terim düzeltmesi ("Charge" → "Hücum") profilin en öncelikli sözlük katmanına girer;
//! - tam metin düzeltmesi ("The door is locked." → "Kapı kilitli.") o metin bir daha görüldüğünde
//!   modele gitmeden gösterilir.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::context;
use crate::term_glossary::GlossaryEntry;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Style {
    /// Doğal konuşma dili; menüler kısa.
    #[default]
    Natural,
    /// Resmi ("siz") ton.
    Formal,
    /// Samimi, gündelik ton.
    Casual,
}

impl Style {
    fn hint(self) -> &'static str {
        match self {
            Style::Natural => "Natural, conversational dialogue; short UI labels.",
            Style::Formal => "Formal tone.",
            Style::Casual => "Casual, informal tone.",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SubtitleSettings {
    /// Altyazı çevirisi yazının altında (yoksa genel ayar).
    pub below: Option<bool>,
    /// Yalnızca altyazı ve menü (yoksa genel ayar).
    pub focus: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OcrSettings {
    /// Saniyedeki okuma (yoksa genel ayar).
    pub fps: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ContextSettings {
    pub enabled: bool,
    pub max_lines: usize,
    pub max_chars: usize,
}

impl Default for ContextSettings {
    fn default() -> ContextSettings {
        ContextSettings { enabled: true, max_lines: context::DEFAULT_LINES, max_chars: context::DEFAULT_CHARS }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CorrectionKind {
    /// Tek terim: sözlüğe girer, o terimin geçtiği her metinde kullanılır.
    #[default]
    Term,
    /// Tam metin: yalnızca bu metin görüldüğünde.
    Text,
}

/// Kullanıcının bir çeviri için verdiği düzeltme.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Correction {
    pub kind: CorrectionKind,
    /// İngilizce kaynak (terim ya da metin).
    pub source: String,
    pub translation: String,
    /// Unix saniye.
    pub created: u64,
}

impl Correction {
    pub fn validate(&self) -> Result<(), String> {
        let limit = match self.kind {
            CorrectionKind::Term => 100,
            CorrectionKind::Text => 1000,
        };
        if self.source.trim().is_empty() || self.translation.trim().is_empty() {
            return Err("düzeltmede kaynak ve çeviri dolu olmalı".into());
        }
        if self.source.chars().count() > limit || self.translation.chars().count() > limit {
            return Err("düzeltme çok uzun".into());
        }
        if self.source.contains(['<', '>']) || self.translation.contains(['<', '>']) {
            return Err("düzeltmede < ve > kullanılamaz".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GameProfile {
    /// Dosya adı: küçük harf, rakam ve tire (`slug`).
    pub id: String,
    pub name: String,
    /// "action RPG", "racing"… bağlam ipucu olarak kullanılır.
    pub genre: String,
    /// Şimdilik yalnızca EN → TR.
    pub source_language: String,
    pub target_language: String,
    pub style: Style,
    /// Çevrilmeyecek karakter/yer adları (genel "çevrilmeyecek adlar"a eklenir).
    pub characters: Vec<String>,
    pub glossary: Vec<GlossaryEntry>,
    /// "local" ya da "deepl"; boşsa genel ayar.
    pub provider: String,
    pub subtitle: SubtitleSettings,
    pub ocr: OcrSettings,
    pub context: ContextSettings,
    pub corrections: Vec<Correction>,
    pub version: u32,
}

impl Default for GameProfile {
    fn default() -> GameProfile {
        GameProfile {
            id: String::new(),
            name: String::new(),
            genre: String::new(),
            source_language: "EN".into(),
            target_language: "TR".into(),
            style: Style::Natural,
            characters: Vec::new(),
            glossary: Vec::new(),
            provider: String::new(),
            subtitle: SubtitleSettings::default(),
            ocr: OcrSettings::default(),
            context: ContextSettings::default(),
            corrections: Vec::new(),
            version: GameProfile::VERSION,
        }
    }
}

/// Düzeltme sayısı sınırı (dosya ve bellek sınırsız büyümesin).
pub const MAX_CORRECTIONS: usize = 5000;
pub const MAX_GLOSSARY: usize = 5000;

impl GameProfile {
    pub const VERSION: u32 = 1;

    pub fn new(name: &str) -> GameProfile {
        GameProfile { id: slug(name), name: name.trim().to_string(), ..GameProfile::default() }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("profil adı boş".into());
        }
        if self.name.chars().count() > 80 {
            return Err("profil adı çok uzun".into());
        }
        if self.id.is_empty() || self.id != slug(&self.id) {
            return Err(format!("geçersiz profil kimliği: {}", self.id));
        }
        if !self.source_language.eq_ignore_ascii_case("EN") || !self.target_language.eq_ignore_ascii_case("TR") {
            return Err("şimdilik yalnızca İngilizce → Türkçe desteklenir".into());
        }
        if !matches!(self.provider.as_str(), "" | "local" | "deepl") {
            return Err(format!("bilinmeyen çeviri motoru: {}", self.provider));
        }
        if self.ocr.fps.is_some_and(|fps| !(1.0..=10.0).contains(&fps)) {
            return Err("profilde okuma hızı 1–10 arasında olmalı".into());
        }
        if self.glossary.len() > MAX_GLOSSARY || self.corrections.len() > MAX_CORRECTIONS {
            return Err("profilde çok fazla sözlük girdisi ya da düzeltme var".into());
        }
        for entry in &self.glossary {
            entry.validate()?;
        }
        for correction in &self.corrections {
            correction.validate()?;
        }
        Ok(())
    }

    /// Bağlam ipucu (DeepL `context` başına): oyun adı, tür, üslup.
    pub fn hint(&self) -> String {
        let mut hint = format!("Video game: {}", self.name.trim());
        if !self.genre.trim().is_empty() {
            hint.push_str(&format!(" ({})", self.genre.trim()));
        }
        hint.push_str(". ");
        hint.push_str(self.style.hint());
        hint
    }

    /// Terim düzeltmeleri sözlük girdisi olarak (en öncelikli katman).
    pub fn term_corrections(&self) -> Vec<GlossaryEntry> {
        self.corrections
            .iter()
            .filter(|c| c.kind == CorrectionKind::Term && c.validate().is_ok())
            .map(|c| GlossaryEntry { origin: "user".into(), ..GlossaryEntry::new(&c.source, &c.translation) })
            .collect()
    }

    /// Düzeltme ekler; aynı kaynak (büyük/küçük harf farkı yok sayılarak) varsa yenisiyle değiştirir.
    pub fn add_correction(&mut self, correction: Correction) -> Result<(), String> {
        correction.validate()?;
        let key = correction.source.trim().to_lowercase();
        self.corrections.retain(|c| !(c.kind == correction.kind && c.source.trim().to_lowercase() == key));
        if self.corrections.len() >= MAX_CORRECTIONS {
            self.corrections.remove(0);
        }
        self.corrections.push(correction);
        Ok(())
    }
}

/// Profil kimliği: küçük harf ASCII, rakam ve tire ("Elden Ring" → "elden-ring", "Ölüm" → "olum").
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        let c = match c {
            'ç' | 'Ç' => 'c',
            'ğ' | 'Ğ' => 'g',
            'ı' | 'I' | 'İ' | 'i' => 'i',
            'ö' | 'Ö' => 'o',
            'ş' | 'Ş' => 's',
            'ü' | 'Ü' => 'u',
            other => other.to_ascii_lowercase(),
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').chars().take(60).collect()
}

/// Profillerin klasörü.
pub fn profiles_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("profiles")
}

pub fn path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{}.json", slug(id)))
}

/// Profili okur ve doğrular.
pub fn load(dir: &Path, id: &str) -> Result<GameProfile, String> {
    let file = path(dir, id);
    let json = std::fs::read_to_string(&file).map_err(|e| format!("profil okunamadı ({}): {e}", file.display()))?;
    let profile: GameProfile = serde_json::from_str(&json).map_err(|e| format!("profil bozuk ({}): {e}", file.display()))?;
    profile.validate()?;
    Ok(profile)
}

/// Doğrulayıp yazar; yarım yazım dosyayı bozmasın diye önce geçici dosyaya.
pub fn save(dir: &Path, profile: &GameProfile) -> Result<(), String> {
    profile.validate()?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file = path(dir, &profile.id);
    let temporary = file.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(profile).map_err(|e| e.to_string())?;
    std::fs::write(&temporary, json).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, &file).map_err(|e| e.to_string())
}

/// Kayıtlı profiller (kimlik, ad), ada göre sıralı; bozuk dosyalar atlanır.
pub fn list(dir: &Path) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut profiles: Vec<(String, String)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let id = e.path().file_stem()?.to_string_lossy().into_owned();
            load(dir, &id).ok().map(|p| (p.id, p.name))
        })
        .collect();
    profiles.sort_by_key(|p| p.1.to_lowercase());
    profiles
}

/// Kullanıcının genel sözlüğü (`app_dir/glossary.json`); yoksa ya da bozuksa boş.
pub fn load_user_glossary(file: &Path) -> Vec<GlossaryEntry> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|json| serde_json::from_str::<Vec<GlossaryEntry>>(&json).ok())
        .map(|entries| entries.into_iter().filter(|e| e.validate().is_ok()).take(MAX_GLOSSARY).collect())
        .unwrap_or_default()
}

/// "İngilizce = Türkçe" satırlarından sözlük (arayüzdeki metin kutusu). "=" yoksa terim korunur
/// (çevrilmez). Geçersiz satırlar hata mesajıyla döner.
pub fn parse_glossary_lines(text: &str) -> Result<Vec<GlossaryEntry>, String> {
    let mut entries = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = match line.split_once('=') {
            Some((source, target)) => GlossaryEntry::new(source.trim(), target.trim()),
            None => GlossaryEntry { preserve: true, ..GlossaryEntry::new(line, "") },
        };
        entry.validate().map_err(|e| format!("{}. satır: {e}", number + 1))?;
        entries.push(entry);
    }
    Ok(entries)
}

/// Sözlüğü arayüz metnine çevirir (`parse_glossary_lines`'ın tersi).
pub fn glossary_lines(entries: &[GlossaryEntry]) -> String {
    entries.iter().map(|e| if e.preserve { e.source.clone() } else { format!("{} = {}", e.source, e.target) }).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ceviri-profile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn profil_kaydedilip_okunur() {
        let dir = temp_dir("save");
        let mut profile = GameProfile::new("Elden Ring");
        profile.genre = "action RPG".into();
        profile.characters = vec!["Melina".into()];
        profile.glossary = vec![GlossaryEntry::new("Site of Grace", "Lütuf Noktası")];

        save(&dir, &profile).unwrap();

        assert_eq!(load(&dir, "elden-ring").unwrap(), profile);
        assert_eq!(list(&dir), vec![("elden-ring".to_string(), "Elden Ring".to_string())]);
        assert!(!path(&dir, "elden-ring").with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn gecersiz_profil_reddedilir() {
        assert!(GameProfile { name: " ".into(), ..GameProfile::new("x") }.validate().is_err());
        assert!(GameProfile { target_language: "DE".into(), ..GameProfile::new("Game") }.validate().is_err());
        assert!(GameProfile { provider: "google".into(), ..GameProfile::new("Game") }.validate().is_err());
        assert!(GameProfile { id: "../etc".into(), ..GameProfile::new("Game") }.validate().is_err());
        let mut bad = GameProfile::new("Game");
        bad.glossary.push(GlossaryEntry::new("Door", ""));
        assert!(bad.validate().is_err());
        assert!(GameProfile::new("Game").validate().is_ok());
    }

    #[test]
    fn bozuk_dosya_listede_atlanir() {
        let dir = temp_dir("broken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bozuk.json"), "{bozuk").unwrap();
        save(&dir, &GameProfile::new("Hades")).unwrap();

        assert_eq!(list(&dir).len(), 1);
        assert!(load(&dir, "bozuk").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn kimlik_uretimi() {
        assert_eq!(slug("Elden Ring"), "elden-ring");
        assert_eq!(slug("  Ölüm Şövalyesi: İkinci Bölüm! "), "olum-sovalyesi-ikinci-bolum");
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
    }

    #[test]
    fn duzeltme_ayni_kaynagi_degistirir_ve_dogrulanir() {
        let mut profile = GameProfile::new("Game");
        profile.add_correction(Correction { source: "Charge".into(), translation: "Şarj".into(), ..Correction::default() }).unwrap();
        profile.add_correction(Correction { source: "charge".into(), translation: "Hücum".into(), ..Correction::default() }).unwrap();

        assert_eq!(profile.corrections.len(), 1);
        assert_eq!(profile.term_corrections()[0].target, "Hücum");
        assert!(profile.add_correction(Correction { source: "x".into(), translation: "".into(), ..Correction::default() }).is_err());
        assert!(profile.add_correction(Correction { source: "<b>".into(), translation: "y".into(), ..Correction::default() }).is_err());
    }

    #[test]
    fn ipucu_ve_sozluk_metni() {
        let profile = GameProfile { genre: "racing".into(), ..GameProfile::new("Gran Turismo") };
        assert_eq!(profile.hint(), "Video game: Gran Turismo (racing). Natural, conversational dialogue; short UI labels.");

        let entries = parse_glossary_lines("Quest = Görev\n# yorum\n\nGeralt\n").unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[1].preserve);
        assert_eq!(glossary_lines(&entries), "Quest = Görev\nGeralt");
        assert!(parse_glossary_lines("Door =").unwrap_err().starts_with("1. satır"));
    }

    #[test]
    fn kullanici_genel_sozlugu_okunur() {
        let dir = temp_dir("user");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("glossary.json");
        std::fs::write(&file, r#"[{"source":"Quest","target":"Görev"},{"source":"","target":"x"}]"#).unwrap();

        let entries = load_user_glossary(&file);

        assert_eq!(entries.len(), 1);
        assert!(load_user_glossary(&dir.join("yok.json")).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
