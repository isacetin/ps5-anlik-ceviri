//! Yerel (çevrimdışı) çeviri: Opus-MT en→tr modeli CTranslate2 ile CPU'da, int8 çalışır (cümle
//! başına ~40–100 ms). Model ilk kullanımdan önce indirilir (`model`). Arka uç, DeepL istemcisiyle
//! aynı girdiyi alır (`Terms::protect` çıktısı) ve aynı biçimde döner; adların ve baştaki
//! işaretlerin işlenmesi `ceviri_core::opus_text`'tedir.

pub mod model;

use std::path::Path;

use ceviri_core::opus_text::{self, Prepared};
use ceviri_core::TranslationError;
use ct2rs::tokenizers::sentencepiece::Tokenizer;
use ct2rs::{ComputeType, Config, Device, TranslationOptions, Translator};

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

    /// Boş kaynaklar (yalnızca işaret) modele gitmez, boş çeviri alır.
    fn run(&self, prepared: &[Prepared]) -> Result<Vec<String>, TranslationError> {
        let sources: Vec<&str> = prepared.iter().map(|p| p.source.as_str()).filter(|s| !s.is_empty()).collect();
        let translated = if sources.is_empty() {
            Vec::new()
        } else {
            self.translator.translate_batch(&sources, &self.options, None).map_err(|e| TranslationError::Local(e.to_string()))?
        };
        let mut translated = translated.into_iter().map(|(text, _score)| text);
        Ok(prepared.iter().map(|p| if p.source.is_empty() { String::new() } else { translated.next().unwrap_or_default() }).collect())
    }
}

#[cfg(test)]
mod tests {
    use ceviri_core::terms::{unprotect, Terms};
    use ceviri_core::storage;

    use super::*;

    /// Model bu bilgisayarda indirilmişse gerçek çeviriyi dener (CI'da model yok, atlanır).
    #[test]
    fn indirilmis_modelle_gercek_ceviri() {
        let Some(dir) = storage::local_model_dir().filter(|d| model::is_installed(d)) else {
            eprintln!("yerel model yok, atlandı");
            return;
        };
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
}
