//! Yerel (çevrimdışı) çeviri: Opus-MT en→tr modeli CTranslate2 ile CPU'da, int8 çalışır (cümle
//! başına ~40–100 ms). Model ilk kullanımdan önce indirilir (`model`). Arka uç, DeepL istemcisiyle
//! aynı girdiyi alır (`Terms::protect` çıktısı) ve aynı biçimde döner; adların ve baştaki
//! işaretlerin işlenmesi `ceviri_core::opus_text`'tedir.

pub mod model;
pub mod reload;

use std::path::Path;

use ceviri_core::opus_text::{self, Prepared};
use ceviri_core::TranslationError;
use ct2rs::tokenizers::sentencepiece::Tokenizer;
use ct2rs::{ComputeType, Config, Device, TranslationOptions, Translator};

/// Takılmaya karşı korunan yerel model (`reload`): her çağrı süre bütçesiyle yapılır, model
/// içinde takılırsa model bir kez taze yüklenip aynı metinler hemen yeniden çevrilir. Yüklenen her
/// kopya kullanılmadan önce ısıtılır.
pub fn reloading(dir: &Path) -> Result<reload::Reloading<OpusTranslator>, String> {
    let dir = dir.to_path_buf();
    let model = reload::Reloading::new(
        Box::new(move || {
            let model = OpusTranslator::load(&dir)?;
            model.warm_up();
            Ok(model)
        }),
        reload::Options::default(),
    )?;
    // Bu bilgisayardaki çeviri hızı ölçülür; takılma bütçesi ilk gerçek altyazıdan önce hazır olur.
    let _ = translate_guarded(&model, &["Calibrating the translation model now.".to_string()]);
    Ok(model)
}

/// Takılmaya karşı korunan çeviri: `reloading` kopyasıyla, metinlerin uzunluğuna göre bütçeyle.
pub fn translate_guarded(model: &reload::Reloading<OpusTranslator>, texts: &[String]) -> Result<Vec<String>, TranslationError> {
    let cost = texts.iter().map(|t| t.chars().count()).sum::<usize>() as f64;
    let owned = texts.to_vec();
    model.call(cost, move |opus: &OpusTranslator| opus.translate(&owned)).map_err(TranslationError::Local)?
}

pub struct OpusTranslator {
    translator: Translator<Tokenizer>,
    options: TranslationOptions<String, String>,
}

impl OpusTranslator {
    /// Modeli belleğe yükler (~0,7 sn, ~250 MB).
    pub fn load(dir: &Path) -> Result<OpusTranslator, String> {
        if !model::is_installed(dir) {
            return Err("Yerel çeviri modeli indirilmemiş.".into());
        }
        let tokenizer = Tokenizer::new(dir).map_err(|e| format!("Yerel model okunamadı: {e}"))?;
        // Oyun ve Remote Play de CPU kullanıyor; çekirdeklerin yarısı (en fazla 4) yeter.
        let threads = std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(1, 4));
        let config = Config { device: Device::CPU, compute_type: ComputeType::INT8, num_threads_per_replica: threads, ..Config::default() };
        let translator = Translator::with_tokenizer(dir, tokenizer, &config).map_err(|e| format!("Yerel model yüklenemedi: {e}"))?;
        // Beam 4: greedy belirgin şekilde hızlı değil, çeviriyi daha çok değiştiriyor (ölçüldü).
        let options = TranslationOptions { beam_size: 4, max_decoding_length: 256, ..TranslationOptions::default() };
        Ok(OpusTranslator { translator, options })
    }

    /// İlk çağrının ısınma maliyetini (bellek ayırma, önbellek) iş hattı başlamadan öder.
    pub fn warm_up(&self) {
        let _ = self.translate(&["The door is locked.".to_string()]);
    }

    /// `texts`: `Terms::protect` çıktısı. Dönen metinler `terms::unprotect` ile açılır.
    pub fn translate(&self, texts: &[String]) -> Result<Vec<String>, TranslationError> {
        let prepared: Vec<Prepared> = texts.iter().map(|t| opus_text::prepare(t)).collect();
        let first = self.run(&prepared)?;
        let mut out: Vec<Option<String>> = prepared.iter().zip(&first).map(|(p, t)| p.finish(t)).collect();
        // Yer tutucusu kaybolanlar, adlar metnin içinde bir kez daha çevrilir.
        let retry: Vec<usize> = (0..out.len()).filter(|&i| out[i].is_none()).collect();
        if !retry.is_empty() {
            let plain: Vec<Prepared> = retry.iter().map(|&i| prepared[i].plain()).collect();
            for ((&i, p), t) in retry.iter().zip(&plain).zip(self.run(&plain)?) {
                out[i] = p.finish(&t);
            }
        }
        Ok(out.into_iter().map(Option::unwrap_or_default).collect())
    }

    /// Boş kaynaklar (yalnızca işaret) modele gitmez, boş çeviri alır. Çok cümleli metin cümle
    /// cümle çevrilir (model birden çok cümlede bazen birini atlıyor); hepsi tek partide gider.
    fn run(&self, prepared: &[Prepared]) -> Result<Vec<String>, TranslationError> {
        let pieces: Vec<Vec<String>> = prepared.iter().map(|p| if p.source.is_empty() { Vec::new() } else { opus_text::split_sentences(&p.source) }).collect();
        let sources: Vec<&str> = pieces.iter().flatten().map(String::as_str).collect();
        let translated = if sources.is_empty() {
            Vec::new()
        } else {
            self.translator.translate_batch(&sources, &self.options, None).map_err(|e| TranslationError::Local(e.to_string()))?
        };
        let mut translated = translated.into_iter().map(|(text, _score)| text);
        Ok(pieces.iter().map(|parts| parts.iter().map(|_| translated.next().unwrap_or_default()).collect::<Vec<_>>().join(" ")).collect())
    }
}

#[cfg(test)]
mod tests {
    use ceviri_core::terms::{unprotect, Terms};
    use ceviri_core::storage;

    use super::*;

    /// İndirilmiş model (yoksa test atlanır ve bunu açıkça yazar). `CEVIRI_REQUIRE_MODEL=1` ile
    /// model yoksa test başarısız olur: gerçek modelle koşulduğu iddia edilen ortamda sessizce
    /// "geçmesin".
    fn installed_model() -> Option<std::path::PathBuf> {
        let dir = storage::local_model_dir().filter(|d| model::is_installed(d));
        if dir.is_none() {
            assert!(std::env::var_os("CEVIRI_REQUIRE_MODEL").is_none(), "CEVIRI_REQUIRE_MODEL verildi ama yerel model kurulu değil");
            eprintln!("UYARI: yerel model kurulu değil; gerçek model testi ATLANDI (çalıştırılmadı)");
        }
        dir
    }

    /// Model bu bilgisayarda indirilmişse gerçek çeviriyi dener (CI'da model yok, atlanır).
    #[test]
    fn indirilmis_modelle_gercek_ceviri() {
        let Some(dir) = installed_model() else { return };
        let opus = OpusTranslator::load(&dir).unwrap();
        let terms = Terms::new(&["It Takes Two".into(), "May".into()]);
        let texts: Vec<String> = ["Press any button to continue", "It Takes Two", "May 1 Wins", "• Jump", "•", "Return to the PlayStation Store."]
            .iter()
            .map(|t| terms.protect(t))
            .collect();

        let out: Vec<String> = opus.translate(&texts).unwrap().iter().map(|t| unprotect(t)).collect();

        assert_eq!(out[0], "Devam etmek için herhangi bir düğmeye basın");
        assert_eq!(out[1], "It Takes Two");
        assert!(out[2].starts_with("May 1"), "{}", out[2]);
        assert!(out[3].starts_with("• "), "{}", out[3]);
        assert_eq!(out[4], "•");
        assert!(out[5].contains("PlayStation Store'a"), "{}", out[5]);
    }

    /// GERÇEK model: çağrı model içinde takılırsa (benzetim: ilk kopyanın ilk çağrısı 6 sn bekler)
    /// aynı istek taze kopyayla hemen çevrilir; çağıran takılmayı beklemez, en çok bir taze kopya.
    #[test]
    fn gercek_modelde_takilan_cagri_taze_kopyayla_hemen_cevrilir() {
        let Some(dir) = installed_model() else { return };
        let model = reloading(&dir).unwrap();
        let texts = vec!["The door is locked.".to_string()];
        let hung = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let started = std::time::Instant::now();
        let flag = hung.clone();
        let owned = texts.clone();
        let out = model
            .call(20.0, move |opus: &OpusTranslator| {
                if !flag.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    std::thread::sleep(std::time::Duration::from_secs(6));
                }
                opus.translate(&owned)
            })
            .expect("taze kopyayla çevrilmeli")
            .unwrap();

        assert_eq!(unprotect(&out[0]), "Kapı kilitli.");
        assert!(started.elapsed() < std::time::Duration::from_secs(3), "takılma beklendi: {:?}", started.elapsed());
        let stats = model.stats();
        assert_eq!((stats.reloads, stats.stuck_alive), (1, 1));
        // Sonraki çağrı taze kopyayla, yeniden yükleme olmadan ve hızlı.
        let started = std::time::Instant::now();
        assert_eq!(unprotect(&translate_guarded(&model, &texts).unwrap()[0]), "Kapı kilitli.");
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        assert_eq!(model.stats().reloads, 1);
    }

    /// Gerçek modelde görülen hatalar: çok cümlede cümle düşürme, tamlamalı sözlük teriminde ek,
    /// fiil olarak geçen terimin isimle kilitlenmesi.
    #[test]
    fn gercek_modelde_cumle_dusmez_ve_sozluk_dogru_eklenir() {
        use ceviri_core::profile::GameProfile;
        use ceviri_core::resolve::{Resolution, Resolver};
        use ceviri_core::term_glossary::GlossaryEntry;
        let Some(dir) = installed_model() else { return };
        let opus = OpusTranslator::load(&dir).unwrap();
        let translate = |key: &str| unprotect(&opus.translate(&[key.to_string()]).unwrap()[0]);

        let out = translate("Listen to me. Whatever happens tonight, you have to promise me you won't go back for him.");
        assert!(out.to_lowercase().contains("dinle"), "ilk cümle düşmemeli: {out}");

        let mut profile = GameProfile::new("Test RPG");
        profile.glossary = vec![GlossaryEntry::new("Charge", "Hücum"), GlossaryEntry::new("Site of Grace", "Lütuf Noktası")];
        let resolver = Resolver::new(Terms::new(&[]), Some(&profile), &[]);
        let key = |text: &str| match resolver.resolve(text) {
            Resolution::Model { key, .. } => key,
            other => panic!("{other:?}"),
        };
        let out = translate(&key("Rest at the Site of Grace."));
        assert!(out.contains("Lütuf Noktası") && !out.contains("Noktasıda"), "{out}");
        let verb = key("Charge the gate!");
        assert!(!verb.contains("<g>"), "fiil olarak kilitlenmemeli: {verb}");
        assert!(!translate(&verb).contains("Hücum"), "fiil isim karşılığıyla çevrilmemeli");
    }
}
