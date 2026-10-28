//! Çeviri önbelleği ve istek yönetimi. Ana döngü her turda `lookup` ile hazır çevirileri alır,
//! eksikleri `fetch` ile arka planda (başka bir thread'de) ister; aynı metin aynı anda iki kez
//! istenmez ve hata sonrasında bir süre yeni istek atılmaz.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranslationError {
    /// 429: çok sık istek.
    RateLimited,
    /// 456: aylık karakter kotası doldu.
    QuotaExceeded,
    /// 403: API anahtarı geçersiz.
    Unauthorized,
    Http(u16),
    InvalidResponse,
    /// Bağlantı/zaman aşımı gibi HTTP'ye ulaşamayan hatalar.
    Network(String),
    /// Yerel çeviri modeli çalışamadı.
    Local(String),
}

impl fmt::Display for TranslationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TranslationError::RateLimited => write!(f, "çok sık istek (429)"),
            TranslationError::QuotaExceeded => write!(f, "aylık karakter kotası doldu (456)"),
            TranslationError::Unauthorized => write!(f, "API anahtarı geçersiz (403)"),
            TranslationError::Http(status) => write!(f, "HTTP {status}"),
            TranslationError::InvalidResponse => write!(f, "geçersiz yanıt"),
            TranslationError::Network(message) => write!(f, "ağ hatası: {message}"),
            TranslationError::Local(message) => write!(f, "yerel çeviri hatası: {message}"),
        }
    }
}

impl std::error::Error for TranslationError {}

/// Önbellekteki çeviriler ve çevrilmesi gereken (henüz istenmemiş) metinler.
#[derive(Debug, Default, PartialEq)]
pub struct Lookup {
    pub found: HashMap<String, String>,
    pub to_fetch: Vec<String>,
}

#[derive(Debug)]
pub struct Translator {
    cache: HashMap<String, String>,
    in_flight: HashSet<String>,
    retry_after: f64,
}

impl Translator {
    /// DeepL tek istekte en fazla 50 metin kabul eder (yerel model için de makul bir parti boyu).
    pub const MAX_BATCH: usize = 50;

    pub fn new(cache: HashMap<String, String>) -> Translator {
        Translator { cache, in_flight: HashSet::new(), retry_after: f64::NEG_INFINITY }
    }

    /// `to_fetch` sıralıdır ve istek sürüyor olarak işaretlenir; sonucu `fetch` ile istenmelidir.
    pub fn lookup(&mut self, texts: &[String], now: f64) -> Lookup {
        let mut lookup = Lookup::default();
        for text in texts.iter().collect::<BTreeSet<_>>() {
            if let Some(translation) = self.cache.get(text) {
                lookup.found.insert(text.clone(), translation.clone());
            } else if !self.in_flight.contains(text) {
                lookup.to_fetch.push(text.clone());
            }
        }
        if now < self.retry_after {
            lookup.to_fetch.clear();
        }
        self.in_flight.extend(lookup.to_fetch.iter().cloned());
        lookup
    }

    pub fn snapshot(&self) -> &HashMap<String, String> {
        &self.cache
    }

    pub fn backoff(error: &TranslationError) -> f64 {
        match error {
            TranslationError::QuotaExceeded | TranslationError::Unauthorized => 300.0,
            TranslationError::RateLimited => 10.0,
            _ => 5.0,
        }
    }
}

/// `lookup`'ın döndürdüğü metinleri `backend` ile partiler halinde çevirip önbelleğe yazar.
/// Ağ isteği sırasında kilit tutulmaz; ana döngü bu arada `lookup` yapmaya devam eder.
/// Hata olursa metinler bekleme süresinden sonra tekrar istenebilir.
pub fn fetch(
    translator: &Mutex<Translator>,
    texts: &[String],
    now: f64,
    mut backend: impl FnMut(&[String]) -> Result<Vec<String>, TranslationError>,
) -> Result<(), TranslationError> {
    let mut result = Ok(());
    for batch in texts.chunks(Translator::MAX_BATCH) {
        match backend(batch) {
            Ok(translations) if translations.len() == batch.len() => {
                translator.lock().unwrap().cache.extend(batch.iter().cloned().zip(translations));
            }
            Ok(_) => {
                result = Err(TranslationError::InvalidResponse);
                break;
            }
            Err(error) => {
                result = Err(error);
                break;
            }
        }
    }
    let mut translator = translator.lock().unwrap();
    for text in texts {
        translator.in_flight.remove(text);
    }
    if let Err(error) = &result {
        translator.retry_after = now + Translator::backoff(error);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    /// Çağrıları kaydeden sahte DeepL.
    fn fake(calls: &mut Vec<Vec<String>>, fail: Option<TranslationError>) -> impl FnMut(&[String]) -> Result<Vec<String>, TranslationError> + '_ {
        move |texts| {
            calls.push(texts.to_vec());
            match &fail {
                Some(error) => Err(error.clone()),
                None => Ok(texts.iter().map(|t| format!("TR({t})")).collect()),
            }
        }
    }

    #[test]
    fn onbellekteki_ceviri_hemen_doner() {
        let mut translator = Translator::new(HashMap::from([("Door is locked".into(), "Kapı kilitli".into())]));

        let result = translator.lookup(&strings(&["Door is locked"]), 0.0);

        assert_eq!(result.found, HashMap::from([("Door is locked".into(), "Kapı kilitli".into())]));
        assert!(result.to_fetch.is_empty());
    }

    #[test]
    fn eksik_ceviri_istenir_ve_onbellege_yazilir() {
        let translator = Mutex::new(Translator::new(HashMap::new()));
        let mut calls = Vec::new();

        let first = translator.lock().unwrap().lookup(&strings(&["Hello", "Goodbye"]), 0.0);
        assert!(first.found.is_empty());
        assert_eq!(first.to_fetch, strings(&["Goodbye", "Hello"]));

        assert!(fetch(&translator, &first.to_fetch, 0.0, fake(&mut calls, None)).is_ok());
        let second = translator.lock().unwrap().lookup(&strings(&["Hello", "Goodbye"]), 1.0);

        assert_eq!(second.found, HashMap::from([("Hello".into(), "TR(Hello)".into()), ("Goodbye".into(), "TR(Goodbye)".into())]));
        assert!(second.to_fetch.is_empty());
        assert_eq!(calls, vec![strings(&["Goodbye", "Hello"])]);
    }

    #[test]
    fn istek_surerken_ayni_metin_tekrar_istenmez() {
        let mut translator = Translator::new(HashMap::new());

        translator.lookup(&strings(&["Hello"]), 0.0);

        assert!(translator.lookup(&strings(&["Hello"]), 0.5).to_fetch.is_empty());
    }

    #[test]
    fn hatadan_sonra_bekleyip_tekrar_dener() {
        let translator = Mutex::new(Translator::new(HashMap::new()));
        let mut calls = Vec::new();

        let first = translator.lock().unwrap().lookup(&strings(&["Hello"]), 0.0);
        assert_eq!(fetch(&translator, &first.to_fetch, 0.0, fake(&mut calls, Some(TranslationError::Http(500)))), Err(TranslationError::Http(500)));

        assert!(translator.lock().unwrap().lookup(&strings(&["Hello"]), 1.0).to_fetch.is_empty());
        assert_eq!(translator.lock().unwrap().lookup(&strings(&["Hello"]), 6.0).to_fetch, strings(&["Hello"]));
    }

    #[test]
    fn kota_dolunca_5_dakika_istek_atilmaz() {
        let translator = Mutex::new(Translator::new(HashMap::new()));
        let mut calls = Vec::new();

        let first = translator.lock().unwrap().lookup(&strings(&["Hello"]), 0.0);
        let _ = fetch(&translator, &first.to_fetch, 0.0, fake(&mut calls, Some(TranslationError::QuotaExceeded)));

        assert!(translator.lock().unwrap().lookup(&strings(&["Hello"]), 299.0).to_fetch.is_empty());
        assert_eq!(translator.lock().unwrap().lookup(&strings(&["Hello"]), 301.0).to_fetch, strings(&["Hello"]));
    }

    #[test]
    fn elli_metinden_fazlasi_partilere_bolunur() {
        let translator = Mutex::new(Translator::new(HashMap::new()));
        let mut calls = Vec::new();
        let texts: Vec<String> = (1..=120).map(|i| format!("line {i}")).collect();

        let lookup = translator.lock().unwrap().lookup(&texts, 0.0);
        fetch(&translator, &lookup.to_fetch, 0.0, fake(&mut calls, None)).unwrap();

        assert_eq!(calls.iter().map(Vec::len).collect::<Vec<_>>(), [50, 50, 20]);
        assert_eq!(translator.lock().unwrap().snapshot().len(), 120);
    }

    #[test]
    fn eksik_yanit_gecersiz_sayilir() {
        let translator = Mutex::new(Translator::new(HashMap::new()));
        let lookup = translator.lock().unwrap().lookup(&strings(&["a", "b"]), 0.0);

        let result = fetch(&translator, &lookup.to_fetch, 0.0, |_| Ok(vec!["tek".into()]));

        assert_eq!(result, Err(TranslationError::InvalidResponse));
        assert!(translator.lock().unwrap().snapshot().is_empty());
    }
}
