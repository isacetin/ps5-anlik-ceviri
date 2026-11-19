//! Çeviri önbelleği ve istek kuyruğu. Ana döngü her turda `request` ile ekrandaki metinleri
//! bildirir ve hazır çevirileri alır; çeviri işçileri (`ceviri_engine::translation`) `take_batch`
//! ile sıradaki partiyi alıp sonucu `complete` ile geri yazar.
//!
//! Kurallar:
//! - Kuyruk yalnızca **o an ekranda olan** çevrilmemiş metinlerdir; her tur baştan kurulur. Ekrandan
//!   kalkmış bir metin için istek atılmaz, böylece eski metinler yeni metni bekletmez.
//! - Aynı metin aynı anda iki kez istenmez. İstek kaydı iş kimliğiyle tutulur; takılan iş
//!   `expire` ile bırakılabilir, geç gelen sonucu yine önbelleğe yazılır.
//! - Hata sonrasında tüm istekler üstel artan bir süre bekletilir (1, 2, 4… sn; 429'da 5, 10…;
//!   kota/anahtar hatasında 5 dk). İlk başarıda sayaç sıfırlanır.
//! - Boş ya da kaynağın aynısı olan çeviri önbelleğe yazılmaz (yoksa o metin kalıcı olarak boş/
//!   çevrilmemiş görünürdü); o metin bir süre bekletilip yeniden denenir.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::stabilizer::normalize;
use crate::terms;

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
    /// İstek süre sınırını aştı (bağlantı ya da yanıt).
    Timeout,
    /// Bağlantı gibi HTTP'ye ulaşamayan hatalar.
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
            TranslationError::Timeout => write!(f, "zaman aşımı"),
            TranslationError::Network(message) => write!(f, "ağ hatası: {message}"),
            TranslationError::Local(message) => write!(f, "yerel çeviri hatası: {message}"),
        }
    }
}

impl std::error::Error for TranslationError {}

/// İşçiye verilen parti. `id`, istek kaydını bu işe bağlar.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub id: u64,
    pub texts: Vec<String>,
    /// Partinin alındığı an (saniye).
    pub started: f64,
}

/// Teşhis için sayaçlar; hiçbiri sıfırlanmaz (oturum boyunca birikir).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TranslatorStats {
    /// Arka uca giden istek (parti) sayısı.
    pub requests: u64,
    /// Önbellekten karşılanan metin sayısı (aynı metnin yeniden çevrilmesi önlendi).
    pub cache_hits: u64,
    pub translated: u64,
    pub errors: u64,
    pub timeouts: u64,
    pub rate_limited: u64,
    /// Daha önce başarısız olmuş bir metnin yeniden istenmesi.
    pub retries: u64,
    /// Boş ya da çevrilmemiş dönen ve önbelleğe yazılmayan çeviriler.
    pub rejected: u64,
    /// Takıldığı için bırakılan işler.
    pub expired: u64,
    pub last_error: Option<TranslationError>,
    /// Son başarılı çevirinin anı ve süresi (sn).
    pub last_success: Option<f64>,
    pub last_latency: Option<f64>,
}

#[derive(Clone, Debug)]
struct Cooldown {
    attempts: u32,
    until: f64,
}

#[derive(Debug)]
pub struct Translator {
    cache: HashMap<String, String>,
    /// Ekranda olup çevirisi henüz olmayan metinler, ekrandaki sırayla (son turun listesi).
    wanted: Vec<String>,
    /// İstek sürüyor: metin → iş kimliği ve başlangıç anı.
    in_flight: HashMap<String, (u64, f64)>,
    /// Başarısız olmuş metinler: kaç kez denendi ve ne zamana kadar beklenecek.
    cooldown: HashMap<String, Cooldown>,
    retry_after: f64,
    /// Art arda başarısız parti sayısı (üstel bekleme için).
    consecutive_failures: u32,
    next_batch: u64,
    dirty: bool,
    stats: TranslatorStats,
}

impl Translator {
    /// DeepL tek istekte en fazla 50 metin kabul eder (yerel model için de makul bir parti boyu).
    pub const MAX_BATCH: usize = 50;
    /// Aynı metin bu kadar kez başarısız olursa uzun süre (`LONG_COOLDOWN`) denenmez.
    pub const MAX_ATTEMPTS: u32 = 3;
    const LONG_COOLDOWN: f64 = 120.0;
    /// Bekleyen/başarısız kayıtların üst sınırı (uzun oturumda büyümesin).
    const MAX_COOLDOWNS: usize = 1000;

    pub fn new(cache: HashMap<String, String>) -> Translator {
        Translator {
            cache,
            wanted: Vec::new(),
            in_flight: HashMap::new(),
            cooldown: HashMap::new(),
            retry_after: f64::NEG_INFINITY,
            consecutive_failures: 0,
            next_batch: 1,
            dirty: false,
            stats: TranslatorStats::default(),
        }
    }

    /// Ekrandaki metinleri bildirir: önbellekte olanların çevirisini döner, olmayanları kuyruğa
    /// koyar. Kuyruk her çağrıda bu listeyle değiştirilir (ekrandan kalkan metin istenmez).
    pub fn request(&mut self, texts: &[String], _now: f64) -> HashMap<String, String> {
        let mut found = HashMap::new();
        let mut wanted = Vec::new();
        let mut seen = HashSet::new();
        for text in texts {
            if !seen.insert(text) {
                continue;
            }
            if let Some(translation) = self.cache.get(text) {
                found.insert(text.clone(), translation.clone());
            } else {
                wanted.push(text.clone());
            }
        }
        self.stats.cache_hits += found.len() as u64;
        self.wanted = wanted;
        found
    }

    /// Önbellekteki çeviri (kuyruğa dokunmaz).
    pub fn cached(&self, text: &str) -> Option<&String> {
        self.cache.get(text)
    }

    /// Şu an istenebilecek metinler: kuyrukta, istek sürmeyen, bekleme süresi dolmuş olanlar.
    fn ready_texts(&self, now: f64) -> impl Iterator<Item = &String> {
        self.wanted
            .iter()
            .filter(move |text| !self.cache.contains_key(*text) && !self.in_flight.contains_key(*text) && self.cooldown.get(*text).is_none_or(|c| now >= c.until))
    }

    /// Kuyrukta bekleyen (henüz istenmemiş) metin sayısı.
    pub fn queued(&self, now: f64) -> usize {
        self.ready_texts(now).count()
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.len()
    }

    /// Genel bekleme sürüyorsa kalan süre (sn).
    pub fn backoff_remaining(&self, now: f64) -> f64 {
        (self.retry_after - now).max(0.0)
    }

    /// İşçi için sıradaki parti (en fazla `max` metin); bekleme sürüyorsa ya da iş yoksa `None`.
    /// Metinler istek sürüyor olarak işaretlenir; sonuç `complete` ile bildirilmelidir.
    pub fn take_batch(&mut self, now: f64, max: usize) -> Option<Batch> {
        if now < self.retry_after {
            return None;
        }
        let texts: Vec<String> = self.ready_texts(now).take(max.max(1)).cloned().collect();
        if texts.is_empty() {
            return None;
        }
        let id = self.next_batch;
        self.next_batch += 1;
        for text in &texts {
            if self.cooldown.contains_key(text) {
                self.stats.retries += 1;
            }
            self.in_flight.insert(text.clone(), (id, now));
        }
        self.stats.requests += 1;
        Some(Batch { id, texts, started: now })
    }

    /// Partinin sonucunu işler. Geçerli çeviriler önbelleğe yazılır; geçersizler (boş, kaynağın
    /// aynısı) bekletilip yeniden denenir. Hata olursa genel bekleme başlar. Bu partiye ait istek
    /// kayıtları kalkar (takıldığı için bırakılmış bir iş geç döndüyse sonuç yine kullanılır).
    /// Önbelleğe en az bir çeviri yazıldıysa `true`.
    pub fn complete(&mut self, batch: &Batch, result: Result<Vec<String>, TranslationError>, now: f64) -> bool {
        for text in &batch.texts {
            if self.in_flight.get(text).is_some_and(|(id, _)| *id == batch.id) {
                self.in_flight.remove(text);
            }
        }
        let result = match result {
            Ok(translations) if translations.len() != batch.texts.len() => Err(TranslationError::InvalidResponse),
            other => other,
        };
        match result {
            Ok(translations) => {
                self.consecutive_failures = 0;
                self.retry_after = f64::NEG_INFINITY;
                let mut stored = false;
                for (text, translation) in batch.texts.iter().zip(translations) {
                    if is_valid_translation(text, &translation) {
                        self.cooldown.remove(text);
                        self.cache.insert(text.clone(), translation);
                        self.stats.translated += 1;
                        stored = true;
                    } else {
                        self.stats.rejected += 1;
                        self.cool_down(text, now);
                    }
                }
                if stored {
                    self.dirty = true;
                    self.stats.last_success = Some(now);
                    self.stats.last_latency = Some((now - batch.started).max(0.0));
                }
                stored
            }
            Err(error) => {
                self.stats.errors += 1;
                match error {
                    TranslationError::Timeout => self.stats.timeouts += 1,
                    TranslationError::RateLimited => self.stats.rate_limited += 1,
                    _ => {}
                }
                // Biçim hatası partideki bir metinden kaynaklanabilir: metinler ayrıca bekletilir
                // ki aynı parti sonsuza kadar tekrarlanmasın.
                if matches!(error, TranslationError::InvalidResponse | TranslationError::Local(_)) {
                    for text in &batch.texts {
                        self.cool_down(text, now);
                    }
                }
                self.consecutive_failures += 1;
                self.retry_after = now + Translator::backoff(&error, self.consecutive_failures);
                self.stats.last_error = Some(error);
                false
            }
        }
    }

    fn cool_down(&mut self, text: &str, now: f64) {
        if self.cooldown.len() >= Translator::MAX_COOLDOWNS && !self.cooldown.contains_key(text) {
            // Süresi dolmuşlar atılır; yine doluysa en eski bekleme kaydı gider.
            self.cooldown.retain(|_, c| c.until > now);
            if self.cooldown.len() >= Translator::MAX_COOLDOWNS {
                if let Some(oldest) = self.cooldown.iter().min_by(|a, b| a.1.until.total_cmp(&b.1.until)).map(|(k, _)| k.clone()) {
                    self.cooldown.remove(&oldest);
                }
            }
        }
        let entry = self.cooldown.entry(text.to_string()).or_insert(Cooldown { attempts: 0, until: now });
        entry.attempts += 1;
        let wait = if entry.attempts >= Translator::MAX_ATTEMPTS { Translator::LONG_COOLDOWN } else { 2f64.powi(entry.attempts as i32) };
        entry.until = now + wait;
    }

    /// `max_age` saniyeden uzun süren işlerin istek kayıtlarını bırakır (metinler yeniden
    /// istenebilir). Bırakılan işlerin kimlikleri döner.
    pub fn expire(&mut self, now: f64, max_age: f64) -> Vec<u64> {
        let expired: BTreeSet<u64> = self.in_flight.values().filter(|(_, started)| now - started > max_age).map(|(id, _)| *id).collect();
        if !expired.is_empty() {
            self.in_flight.retain(|_, (id, _)| !expired.contains(id));
            self.stats.expired += expired.len() as u64;
        }
        expired.into_iter().collect()
    }

    /// Son kaydedilişten beri yeni çeviri geldiyse önbelleğin kopyası (kaydedilmek üzere).
    pub fn take_dirty_snapshot(&mut self) -> Option<HashMap<String, String>> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        Some(self.cache.clone())
    }

    pub fn snapshot(&self) -> &HashMap<String, String> {
        &self.cache
    }

    pub fn stats(&self) -> &TranslatorStats {
        &self.stats
    }

    /// Hata sonrası genel bekleme (sn); `consecutive` art arda kaçıncı hata olduğu (1'den başlar).
    pub fn backoff(error: &TranslationError, consecutive: u32) -> f64 {
        let doubling = 2f64.powi(consecutive.saturating_sub(1).min(10) as i32);
        match error {
            TranslationError::QuotaExceeded | TranslationError::Unauthorized => 300.0,
            TranslationError::RateLimited => (5.0 * doubling).min(60.0),
            _ => doubling.min(30.0),
        }
    }
}

/// Çeviri gösterilmeye değer mi? Boş çeviri ve birkaç kelimelik metnin aynen geri dönmesi
/// (çevrilmemiş) geçersizdir. Metin `Terms::protect` biçimindedir.
pub fn is_valid_translation(source: &str, translation: &str) -> bool {
    let source = terms::unprotect(source);
    let translated = terms::unprotect(translation);
    if !source.chars().any(char::is_alphabetic) {
        return true;
    }
    if !translated.chars().any(char::is_alphanumeric) {
        return false;
    }
    let words = source.split_whitespace().filter(|w| w.chars().any(char::is_alphabetic)).count();
    !(words >= 3 && normalize(&source) == normalize(&translated))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn echo(texts: &[String]) -> Result<Vec<String>, TranslationError> {
        Ok(texts.iter().map(|t| format!("TR({t})")).collect())
    }

    #[test]
    fn onbellekteki_ceviri_hemen_doner() {
        let mut translator = Translator::new(HashMap::from([("Door is locked".into(), "Kapı kilitli".into())]));

        let found = translator.request(&strings(&["Door is locked"]), 0.0);

        assert_eq!(found, HashMap::from([("Door is locked".into(), "Kapı kilitli".into())]));
        assert_eq!(translator.queued(0.0), 0);
        assert_eq!(translator.stats().cache_hits, 1);
    }

    #[test]
    fn eksik_ceviri_istenir_ve_onbellege_yazilir() {
        let mut translator = Translator::new(HashMap::new());

        assert!(translator.request(&strings(&["Hello", "Goodbye"]), 0.0).is_empty());
        let batch = translator.take_batch(0.0, 50).unwrap();
        assert_eq!(batch.texts, strings(&["Hello", "Goodbye"]));
        assert!(translator.complete(&batch, echo(&batch.texts), 0.4));
        // Sonraki tur gelmeden işçi aynı metni yeniden almamalı.
        assert!(translator.take_batch(0.5, 50).is_none());

        let found = translator.request(&strings(&["Hello", "Goodbye"]), 1.0);
        assert_eq!(found, HashMap::from([("Hello".into(), "TR(Hello)".into()), ("Goodbye".into(), "TR(Goodbye)".into())]));
        assert!(translator.take_batch(1.0, 50).is_none());
        assert_eq!(translator.stats().last_latency, Some(0.4));
        assert!(translator.take_dirty_snapshot().is_some());
        assert!(translator.take_dirty_snapshot().is_none(), "kaydedildikten sonra kirli değil");
    }

    #[test]
    fn istek_surerken_ayni_metin_tekrar_istenmez() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Hello"]), 0.0);
        translator.take_batch(0.0, 50).unwrap();

        translator.request(&strings(&["Hello"]), 0.5);

        assert!(translator.take_batch(0.5, 50).is_none());
        assert_eq!(translator.in_flight(), 1);
    }

    #[test]
    fn ekrandan_kalkan_metin_kuyrukta_beklemez() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Old line one.", "Old line two."]), 0.0);

        // İşçi meşgulken diyalog değişti: eski satırlar artık istenmemeli.
        translator.request(&strings(&["Brand new line."]), 0.2);

        assert_eq!(translator.take_batch(0.2, 50).unwrap().texts, strings(&["Brand new line."]));
        assert!(translator.take_batch(0.2, 50).is_none());
    }

    #[test]
    fn hatadan_sonra_ustel_bekleyip_tekrar_dener() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Hello"]), 0.0);

        let batch = translator.take_batch(0.0, 50).unwrap();
        assert!(!translator.complete(&batch, Err(TranslationError::Http(500)), 0.0));
        assert!(translator.take_batch(0.5, 50).is_none(), "1 sn beklenmeli");
        let batch = translator.take_batch(1.0, 50).unwrap();
        translator.complete(&batch, Err(TranslationError::Timeout), 1.0);
        assert!(translator.take_batch(2.5, 50).is_none(), "ikinci hatada 2 sn");
        let batch = translator.take_batch(3.0, 50).unwrap();
        translator.complete(&batch, echo(&batch.texts), 3.1);

        assert_eq!(translator.cached("Hello").map(String::as_str), Some("TR(Hello)"));
        assert_eq!((translator.stats().errors, translator.stats().timeouts), (2, 1));
        assert_eq!(translator.backoff_remaining(3.1), 0.0, "başarıda bekleme sıfırlanır");
    }

    #[test]
    fn bekleme_sureleri() {
        assert_eq!(Translator::backoff(&TranslationError::Http(500), 1), 1.0);
        assert_eq!(Translator::backoff(&TranslationError::Network("x".into()), 3), 4.0);
        assert_eq!(Translator::backoff(&TranslationError::Timeout, 20), 30.0);
        assert_eq!(Translator::backoff(&TranslationError::RateLimited, 1), 5.0);
        assert_eq!(Translator::backoff(&TranslationError::RateLimited, 2), 10.0);
        assert_eq!(Translator::backoff(&TranslationError::RateLimited, 9), 60.0);
        assert_eq!(Translator::backoff(&TranslationError::QuotaExceeded, 1), 300.0);
        assert_eq!(Translator::backoff(&TranslationError::Unauthorized, 1), 300.0);
    }

    #[test]
    fn kota_dolunca_5_dakika_istek_atilmaz() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Hello"]), 0.0);
        let batch = translator.take_batch(0.0, 50).unwrap();
        translator.complete(&batch, Err(TranslationError::QuotaExceeded), 0.0);

        assert!(translator.take_batch(299.0, 50).is_none());
        assert_eq!(translator.take_batch(301.0, 50).unwrap().texts, strings(&["Hello"]));
    }

    #[test]
    fn partiler_en_fazla_elli_metin() {
        let mut translator = Translator::new(HashMap::new());
        let texts: Vec<String> = (1..=120).map(|i| format!("line {i}")).collect();
        translator.request(&texts, 0.0);

        let sizes: Vec<usize> = std::iter::from_fn(|| translator.take_batch(0.0, Translator::MAX_BATCH).map(|b| b.texts.len())).collect();

        assert_eq!(sizes, [50, 50, 20]);
    }

    #[test]
    fn eksik_yanit_gecersiz_sayilir_ve_metin_bekletilir() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["a b", "c d"]), 0.0);
        let batch = translator.take_batch(0.0, 50).unwrap();

        assert!(!translator.complete(&batch, Ok(vec!["tek".into()]), 0.0));

        assert!(translator.snapshot().is_empty());
        assert_eq!(translator.stats().last_error, Some(TranslationError::InvalidResponse));
        // Genel bekleme (1 sn) bitse de metin kendi beklemesindedir (2 sn).
        assert!(translator.take_batch(1.5, 50).is_none());
        assert!(translator.take_batch(2.0, 50).is_some());
    }

    #[test]
    fn bos_ve_cevrilmemis_ceviri_onbellege_yazilmaz() {
        let mut translator = Translator::new(HashMap::new());
        let texts = strings(&["Press any button to continue", "Where are you going?", "Jump"]);
        translator.request(&texts, 0.0);
        let batch = translator.take_batch(0.0, 50).unwrap();

        let stored = translator.complete(&batch, Ok(strings(&["", "Where are you going?", "Zıpla"])), 0.1);

        assert!(stored);
        assert_eq!(translator.cached("Jump").map(String::as_str), Some("Zıpla"));
        assert_eq!(translator.cached("Press any button to continue"), None);
        assert_eq!(translator.cached("Where are you going?"), None);
        assert_eq!(translator.stats().rejected, 2);
        // Reddedilenler hemen tekrar istenmez (ekran her turda aynı metni bildirse de).
        translator.request(&texts, 0.2);
        assert!(translator.take_batch(0.2, 50).is_none());
        // Bekleme bitince yeniden denenir; sayaçta "yeniden deneme" görünür.
        assert_eq!(translator.take_batch(2.2, 50).unwrap().texts.len(), 2);
        assert_eq!(translator.stats().retries, 2);
    }

    #[test]
    fn hep_basarisiz_metin_uzun_sure_bekletilir() {
        let mut translator = Translator::new(HashMap::new());
        let mut now = 0.0;
        for _ in 0..Translator::MAX_ATTEMPTS {
            translator.request(&strings(&["Broken text here"]), now);
            let batch = translator.take_batch(now, 50).expect("deneme yapılmalı");
            translator.complete(&batch, Ok(strings(&[""])), now);
            now += 10.0;
        }

        translator.request(&strings(&["Broken text here", "Fine"]), now);

        // Bozuk metin uzun beklemede; diğer metinler etkilenmez.
        assert_eq!(translator.take_batch(now, 50).unwrap().texts, strings(&["Fine"]));
    }

    #[test]
    fn takilan_is_birakilir_gec_sonuc_yine_kullanilir() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Hello"]), 0.0);
        let stuck = translator.take_batch(0.0, 50).unwrap();

        assert!(translator.expire(10.0, 30.0).is_empty());
        assert_eq!(translator.expire(31.0, 30.0), vec![stuck.id]);
        assert_eq!(translator.in_flight(), 0);

        // Metin yeniden istenir; yeni iş sürerken eski iş döner ve yeni işin kaydını silmez.
        translator.request(&strings(&["Hello"]), 31.0);
        let retry = translator.take_batch(31.0, 50).unwrap();
        translator.complete(&stuck, echo(&stuck.texts), 32.0);
        assert_eq!(translator.in_flight(), 1, "yeni işin kaydı kalmalı");
        assert_eq!(translator.cached("Hello").map(String::as_str), Some("TR(Hello)"));
        translator.complete(&retry, echo(&retry.texts), 32.5);
        assert_eq!(translator.in_flight(), 0);
        assert_eq!(translator.stats().expired, 1);
    }

    #[test]
    fn bekleme_kayitlari_sinirsiz_buyumez() {
        let mut translator = Translator::new(HashMap::new());
        for i in 0..(Translator::MAX_COOLDOWNS + 500) {
            let text = format!("bad line number {i}");
            translator.request(std::slice::from_ref(&text), i as f64);
            let batch = translator.take_batch(i as f64, 50).unwrap();
            translator.complete(&batch, Ok(vec![String::new()]), i as f64);
        }
        assert!(translator.cooldown.len() <= Translator::MAX_COOLDOWNS);
    }

    #[test]
    fn gecerli_ceviri_kurali() {
        assert!(is_valid_translation("Jump", "Zıpla"));
        assert!(!is_valid_translation("Jump", "  "));
        // Kısa metnin aynen dönmesi olağandır ("OK", "Online" gibi).
        assert!(is_valid_translation("OK", "OK"));
        assert!(!is_valid_translation("Where are you going?", "where are you going"));
        // Harfsiz metin (sayı) için boş olmayan her şey geçerli.
        assert!(is_valid_translation("100", "100"));
        // Adlar korunurken XML kaçışlı karşılaştırılmaz.
        assert!(is_valid_translation("Go to <x>Los Santos</x> now please", "Şimdi <x>Los Santos</x>'a git"));
    }
}
