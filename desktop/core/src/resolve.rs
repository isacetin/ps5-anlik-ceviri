//! Çeviri öncesi karar (bağlam çözücü): ekrandaki bir metin modele mi gidecek, yoksa hazır bir
//! karşılığı mı var? Sıra:
//!
//! 1. Kullanıcının bu oyun için tam metin düzeltmesi → modele gitmez.
//! 2. Metnin tamamı sözlük terimi ("Inventory") → modele gitmez.
//! 3. Yaygın menü kelimesi (`glossary::lookup`, "Resume") → modele gitmez.
//! 4. Model: adlar `<x>`, sözlük terimleri `<g>` ile işaretli anahtar. Anahtar, adlar ve sözlük
//!    değişince kendiliğinden değişir; eski çeviri yanlış sözlükle yeniden kullanılmaz.

use std::collections::HashMap;

use crate::glossary;
use crate::profile::{CorrectionKind, GameProfile};
use crate::term_glossary::{self, Glossary, GlossaryEntry, Layer};
use crate::terms::Terms;
use crate::translator::memory_key;

/// Hazır karşılığın kaynağı (teşhis).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fixed {
    Correction,
    Glossary,
    Menu,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Resolution {
    Ready { text: String, source: Fixed },
    /// Modele gidecek işaretli metin ve içindeki sözlük terimi sayısı.
    Model { key: String, glossary_hits: usize },
}

#[derive(Clone, Debug)]
pub struct Resolver {
    terms: Terms,
    glossary: Glossary,
    /// `memory_key(kaynak)` → kullanıcının çevirisi.
    overrides: HashMap<String, String>,
    profile_id: String,
    profile_name: String,
    hint: String,
}

impl Default for Resolver {
    fn default() -> Resolver {
        Resolver::new(Terms::new(&[]), None, &[])
    }
}

impl Resolver {
    /// `terms`: genel "çevrilmeyecek adlar" (profilin karakterleri eklenir). `user_glossary`:
    /// kullanıcının tüm oyunlar için sözlüğü.
    pub fn new(terms: Terms, profile: Option<&GameProfile>, user_glossary: &[GlossaryEntry]) -> Resolver {
        let built_in: Vec<GlossaryEntry> = term_glossary::BUILT_IN.iter().map(|(s, t)| GlossaryEntry { origin: "builtin".into(), ..GlossaryEntry::new(s, t) }).collect();
        let (corrections, game) = profile.map(|p| (p.term_corrections(), p.glossary.clone())).unwrap_or_default();
        let glossary = Glossary::new(&[(Layer::UserGame, &corrections), (Layer::Game, &game), (Layer::UserGlobal, user_glossary), (Layer::BuiltIn, &built_in)]);
        let terms = match profile {
            Some(p) if !p.characters.is_empty() => terms.with(&p.characters),
            _ => terms,
        };
        let overrides = profile
            .map(|p| {
                p.corrections
                    .iter()
                    .filter(|c| c.kind == CorrectionKind::Text && c.validate().is_ok())
                    .map(|c| (memory_key(c.source.trim()), c.translation.trim().to_string()))
                    .collect()
            })
            .unwrap_or_default();
        Resolver {
            terms,
            glossary,
            overrides,
            profile_id: profile.map(|p| p.id.clone()).unwrap_or_default(),
            profile_name: profile.map(|p| p.name.clone()).unwrap_or_default(),
            hint: profile.map(GameProfile::hint).unwrap_or_default(),
        }
    }

    pub fn resolve(&self, text: &str) -> Resolution {
        if let Some(fixed) = self.overrides.get(&memory_key(text)) {
            return Resolution::Ready { text: fixed.clone(), source: Fixed::Correction };
        }
        if let Some(hit) = self.glossary.whole(text) {
            return Resolution::Ready { text: hit.plain, source: Fixed::Glossary };
        }
        if let Some(menu) = glossary::lookup(text) {
            return Resolution::Ready { text: menu.to_string(), source: Fixed::Menu };
        }
        let (key, glossary_hits) = term_glossary::protect(&self.terms, &self.glossary, text);
        Resolution::Model { key, glossary_hits }
    }

    pub fn terms(&self) -> &Terms {
        &self.terms
    }

    /// Önbellekte kısa metinlerin kapsamı (profil kimliği; profil yoksa boş).
    pub fn scope(&self) -> &str {
        &self.profile_id
    }

    pub fn profile_name(&self) -> &str {
        &self.profile_name
    }

    /// Bağlam ipucu (profil yoksa boş).
    pub fn hint(&self) -> &str {
        &self.hint
    }

    pub fn glossary_len(&self) -> usize {
        self.glossary.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Correction;

    fn profile() -> GameProfile {
        let mut profile = GameProfile::new("Witcher Test");
        profile.characters = vec!["Geralt".into()];
        profile.glossary = vec![GlossaryEntry::new("Charge", "Hücum")];
        profile.add_correction(Correction { kind: CorrectionKind::Text, source: "The door is locked.".into(), translation: "Kapı sürgülü.".into(), created: 0 }).unwrap();
        profile.add_correction(Correction { kind: CorrectionKind::Term, source: "Quest".into(), translation: "Sefer".into(), created: 0 }).unwrap();
        profile
    }

    #[test]
    fn kullanici_tam_metin_duzeltmesi_modele_gitmez() {
        let resolver = Resolver::new(Terms::new(&[]), Some(&profile()), &[]);

        // Büyük/küçük harf ve sondaki nokta farkı aynı metin.
        assert_eq!(resolver.resolve("THE DOOR IS LOCKED"), Resolution::Ready { text: "Kapı sürgülü.".into(), source: Fixed::Correction });
        // Düzeltme başka oyuna uygulanmaz.
        assert!(matches!(Resolver::default().resolve("The door is locked."), Resolution::Model { .. }));
    }

    #[test]
    fn terim_duzeltmesi_sonraki_cevirilerde_kullanilir() {
        let resolver = Resolver::new(Terms::new(&[]), Some(&profile()), &[]);

        assert_eq!(resolver.resolve("Start the Quest now"), Resolution::Model { key: "Start the <g>Sefer</g> now".into(), glossary_hits: 1 });
        assert_eq!(resolver.resolve("Charge!"), Resolution::Ready { text: "Hücum!".into(), source: Fixed::Glossary });
        assert_eq!(resolver.resolve("Lead the Charge!"), Resolution::Model { key: "Lead the <g>Hücum</g>!".into(), glossary_hits: 1 });
        // Profil yokken yerleşik karşılık.
        assert_eq!(Resolver::default().resolve("Start the Quest now"), Resolution::Model { key: "Start the <g>Görev</g> now".into(), glossary_hits: 1 });
    }

    #[test]
    fn karakter_adi_korunur_ve_kapsam_profildir() {
        let resolver = Resolver::new(Terms::new(&[]), Some(&profile()), &[]);

        assert_eq!(resolver.resolve("Geralt, wait!"), Resolution::Model { key: "<x>Geralt</x>, wait!".into(), glossary_hits: 0 });
        assert_eq!(resolver.scope(), "witcher-test");
        assert!(resolver.hint().starts_with("Video game: Witcher Test"));
        assert_eq!(Resolver::default().scope(), "");
    }

    #[test]
    fn tam_terim_ve_menu_kelimesi_hazir_gelir() {
        let resolver = Resolver::default();

        assert_eq!(resolver.resolve("Inventory"), Resolution::Ready { text: "Envanter".into(), source: Fixed::Glossary });
        assert_eq!(resolver.resolve("Resume"), Resolution::Ready { text: "Devam Et".into(), source: Fixed::Menu });
    }

    #[test]
    fn kullanici_genel_sozlugu_yerlesikten_once_profilden_sonra_gelir() {
        let user = vec![GlossaryEntry::new("Charge", "Şarj"), GlossaryEntry::new("Quest", "Macera")];

        let without_profile = Resolver::new(Terms::new(&[]), None, &user);
        assert_eq!(without_profile.resolve("Quest log"), Resolution::Model { key: "<g>Macera</g> log".into(), glossary_hits: 1 });
        let with_profile = Resolver::new(Terms::new(&[]), Some(&profile()), &user);
        assert_eq!(with_profile.resolve("Charge!"), Resolution::Ready { text: "Hücum!".into(), source: Fixed::Glossary });
        assert_eq!(without_profile.resolve("Charge!"), Resolution::Ready { text: "Şarj!".into(), source: Fixed::Glossary });
    }
}
