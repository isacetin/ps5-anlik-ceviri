//! Çeviri belleği ve istek kuyruğu. Ana döngü her turda `request` ile ekrandaki metinleri
//! bildirir ve hazır çevirileri alır; çeviri işçileri (`ceviri_engine::translation`) `take_batch`
//! ile sıradaki partiyi alıp sonucu `complete` ile geri yazar.
//!
//! Kurallar:
//! - Kuyruk yalnızca **o an ekranda olan** çevrilmemiş metinlerdir; her tur baştan kurulur. Ekrandan
//!   kalkmış bir metin için istek atılmaz, böylece eski metinler yeni metni bekletmez. Kuyruk
//!   önceliğe göre dizilir (altyazı önce, menü sonra).
//! - Aynı metin aynı anda iki kez istenmez. İstek kaydı iş kimliğiyle tutulur; takılan iş
//!   `expire` ile bırakılabilir, geç gelen sonucu yine belleğe yazılır (bellek ile ekran ayrıdır:
//!   ekran yalnızca o an ekrandaki metnin çevirisini gösterir, bayat sonuç ekrana çıkmaz).
//! - Bellek araması: birebir → normalize (büyük/küçük harf, boşluk, sondaki tek nokta; `?` ve `!`
//!   korunur) → rakam kardeş. Bulanık eşleştirme bilerek yapılmaz (yanlış metne yanlış çeviri).
//! - Kısa metinler (≤ 3 kelime) oyun profili kapsamında saklanır ("Charge" bir oyunda hücum, birinde
//!   şarj). Konuşma bağlamıyla çevrilen kısa metin yalnızca oturum belleğine yazılır, diske yazılmaz
//!   ve sahne değişince silinir: bağlama bağlı çeviri başka bağlamda kullanılmaz.
//! - Hata sonrasında istekler üstel artan bir süre bekletilir (1, 2, 4… sn; 429'da `Retry-After`
//!   ya da 5, 10…; anahtar/kota hatasında 5 dk'dan 1 sa'e). Art arda 3 hatada (ya da anahtar/kota
//!   hatasında) devre açılır: süre dolunca yalnızca **tek metinlik** bir deneme (prob) gider, başarılı
//!   olursa devre kapanır. Servis düzelince eski kuyruk birden gönderilmez.
//! - İstek hatası (4xx) yalnızca o metinleri bekletir, servisi değil.
//! - Boş ya da kaynağın aynısı olan çeviri belleğe yazılmaz (yoksa o metin kalıcı olarak boş/
//!   çevrilmemiş görünürdü); o metin bir süre bekletilip yeniden denenir.
//! - Yalnızca rakamları farklı metin ("Respawn in 5" → "Respawn in 4", "2/5" → "3/5") bellekteki
//!   kardeşinin çevirisinden, rakamlar yerine konarak hemen üretilir; geri sayım her saniye
//!   çeviri isteği atmaz. Çeviride rakamlar kaynaktakiyle aynı sırada değilse bu yapılmaz.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;

use serde::Serialize;

use crate::context::ContextWindow;
use crate::stabilizer::normalize;
use crate::terms;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranslationError {
    /// 429: çok sık istek; sunucu `Retry-After` verdiyse saniye.
    RateLimited(Option<u32>),
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
            TranslationError::RateLimited(_) => write!(f, "çok sık istek (429)"),
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

/// Hatanın türü: her tür farklı yeniden deneme politikası alır.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorClass {
    /// Bağlantı kurulamadı: bekleyip yeniden denenir.
    Network,
    Timeout,
    /// 429: `Retry-After` ya da artan bekleme.
    RateLimit,
    /// 403: anahtar geçersiz; kullanıcı düzeltmeden düzelmez, çok seyrek denenir.
    Auth,
    /// 456: kota; çok seyrek denenir.
    Quota,
    /// 4xx: istek (metin) hatalı; yalnızca o metinler bekletilir.
    InvalidRequest,
    /// 5xx.
    Server,
    /// Yanıt çözülemedi ya da eksik.
    InvalidResponse,
    /// Yerel model hatası.
    Local,
}

impl ErrorClass {
    pub fn label(self) -> &'static str {
        match self {
            ErrorClass::Network => "ağ",
            ErrorClass::Timeout => "zaman aşımı",
            ErrorClass::RateLimit => "istek sınırı",
            ErrorClass::Auth => "anahtar",
            ErrorClass::Quota => "kota",
            ErrorClass::InvalidRequest => "geçersiz istek",
            ErrorClass::Server => "sunucu",
            ErrorClass::InvalidResponse => "geçersiz yanıt",
            ErrorClass::Local => "yerel model",
        }
    }
}

impl TranslationError {
    pub fn class(&self) -> ErrorClass {
        match self {
            TranslationError::RateLimited(_) => ErrorClass::RateLimit,
            TranslationError::QuotaExceeded => ErrorClass::Quota,
            TranslationError::Unauthorized => ErrorClass::Auth,
            TranslationError::Http(status) if (400..500).contains(status) => ErrorClass::InvalidRequest,
            TranslationError::Http(_) => ErrorClass::Server,
            TranslationError::InvalidResponse => ErrorClass::InvalidResponse,
            TranslationError::Timeout => ErrorClass::Timeout,
            TranslationError::Network(_) => ErrorClass::Network,
            TranslationError::Local(_) => ErrorClass::Local,
        }
    }

    /// Kullanıcı bir şey yapmadan düzelmez (anahtar, kota).
    pub fn needs_user(&self) -> bool {
        matches!(self.class(), ErrorClass::Auth | ErrorClass::Quota)
    }
}

/// Devre kesici durumu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Breaker {
    /// Normal: istekler gider.
    #[default]
    Closed,
    /// Servis üst üste başarısız: bekleme bitene kadar istek yok.
    Open,
    /// Bekleme bitti: tek metinlik deneme gidiyor/gidecek.
    HalfOpen,
}

impl Breaker {
    pub fn label(self) -> &'static str {
        match self {
            Breaker::Closed => "kapalı",
            Breaker::Open => "açık",
            Breaker::HalfOpen => "deneniyor",
        }
    }
}

/// İşçiye verilen parti. `id`, istek kaydını bu işe bağlar.
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub id: u64,
    pub texts: Vec<String>,
    /// Partinin alındığı an (saniye).
    pub started: f64,
    /// İsteğe eklenecek bağlam (düz metin; boş olabilir).
    pub context: String,
    /// Bağlamda konuşma geçmişi vardı: kısa metinlerin çevirisi ona bağlı olabilir.
    pub contextual: bool,
    /// Devre yarı açıkken giden deneme partisi.
    pub probe: bool,
}

/// Ekrandaki bir metnin isteği.
#[derive(Clone, Debug, PartialEq)]
pub struct Wanted {
    /// İşaretli (`<x>`, `<g>`) kaynak metin.
    pub text: String,
    /// Küçük önce (`focus::Category::priority`).
    pub priority: u8,
    /// Konuşma mı (çevrilince sonraki isteklere bağlam olur).
    pub conversation: bool,
}

impl Wanted {
    pub fn new(text: impl Into<String>) -> Wanted {
        Wanted { text: text.into(), priority: 3, conversation: false }
    }
}

/// Teşhis için sayaçlar; hiçbiri sıfırlanmaz (oturum boyunca birikir).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TranslatorStats {
    /// Arka uca giden istek (parti) sayısı.
    pub requests: u64,
    /// Bellekten karşılanan metin sayısı (aynı metnin yeniden çevrilmesi önlendi).
    pub cache_hits: u64,
    /// Bunlardan normalize eşleşmeyle bulunanlar (büyük harf, boşluk, nokta farkı).
    pub normalized_hits: u64,
    /// Rakam kardeşinden türetilenler.
    pub derived_hits: u64,
    /// Oturum belleğinden (bağlama bağlı kısa metin) karşılananlar.
    pub session_hits: u64,
    pub translated: u64,
    pub errors: u64,
    pub timeouts: u64,
    pub rate_limited: u64,
    /// Daha önce başarısız olmuş bir metnin yeniden istenmesi.
    pub retries: u64,
    /// Boş ya da çevrilmemiş dönen ve belleğe yazılmayan çeviriler.
    pub rejected: u64,
    /// Takıldığı için bırakılan işler.
    pub expired: u64,
    /// Devrenin kaç kez açıldığı ve tek metinlik deneme sayısı.
    pub breaker_opens: u64,
    pub probes: u64,
    /// Bağlamın (sahne değişimi) kaç kez sıfırlandığı.
    pub context_resets: u64,
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
    /// Kalıcı bellek (diske yazılır): saklama anahtarı → çeviri.
    cache: HashMap<String, String>,
    /// Oturum belleği: bağlama bağlı kısa metinler (diske yazılmaz, sahne değişince silinir).
    session: HashMap<String, String>,
    /// `memory_key(anahtar)` → anahtar (normalize arama).
    index: HashMap<String, String>,
    session_index: HashMap<String, String>,
    /// Kısa metinlerin kapsamı (oyun profili kimliği; boşsa genel).
    scope: String,
    context: ContextWindow,
    /// Ekranda olup çevirisi henüz olmayan metinler, öncelik sırasıyla (son turun listesi).
    wanted: Vec<String>,
    /// Son turda konuşma olarak bildirilen metinler.
    conversation: HashSet<String>,
    /// İstek sürüyor: metin → iş kimliği ve başlangıç anı.
    in_flight: HashMap<String, (u64, f64)>,
    /// Başarısız olmuş metinler: kaç kez denendi ve ne zamana kadar beklenecek.
    cooldown: HashMap<String, Cooldown>,
    retry_after: f64,
    /// Art arda başarısız parti sayısı (üstel bekleme için).
    consecutive_failures: u32,
    breaker: Breaker,
    /// Yarı açık devrede giden deneme partisinin kimliği.
    probe: Option<u64>,
    next_batch: u64,
    dirty: bool,
    stats: TranslatorStats,
    /// Rakamlı metinlerin kalıbı ("Respawn in #") → bellekteki bir kaynak anahtar.
    templates: HashMap<String, String>,
}

/// Bellekteki bir çevirinin nereden bulunduğu.
enum Found {
    Exact,
    Normalized,
    Session,
    Derived,
}

impl Translator {
    /// DeepL tek istekte en fazla 50 metin kabul eder (yerel model için de makul bir parti boyu).
    pub const MAX_BATCH: usize = 50;
    /// Aynı metin bu kadar kez başarısız olursa uzun süre (`LONG_COOLDOWN`) denenmez.
    pub const MAX_ATTEMPTS: u32 = 3;
    const LONG_COOLDOWN: f64 = 120.0;
    /// Bekleyen/başarısız kayıtların üst sınırı (uzun oturumda büyümesin).
    const MAX_COOLDOWNS: usize = 1000;
    /// Oturum belleğinin üst sınırı.
    const MAX_SESSION: usize = 2000;
    /// Art arda bu kadar hatada devre açılır.
    pub const BREAKER_THRESHOLD: u32 = 3;
    /// Bu kadar kelimeye kadar metin "kısa" sayılır (kapsam ve bağlam kuralı).
    pub const SHORT_WORDS: usize = 3;

    pub fn new(cache: HashMap<String, String>) -> Translator {
        let templates = cache.keys().filter_map(|key| Some((digit_template(key)?, key.clone()))).collect();
        let index = cache.keys().map(|key| (memory_key(key), key.clone())).collect();
        Translator {
            cache,
            session: HashMap::new(),
            index,
            session_index: HashMap::new(),
            scope: String::new(),
            context: ContextWindow::disabled(),
            templates,
            wanted: Vec::new(),
            conversation: HashSet::new(),
            in_flight: HashMap::new(),
            cooldown: HashMap::new(),
            retry_after: f64::NEG_INFINITY,
            consecutive_failures: 0,
            breaker: Breaker::Closed,
            probe: None,
            next_batch: 1,
            dirty: false,
            stats: TranslatorStats::default(),
        }
    }

    /// Kısa metinlerin kapsamı (oyun profili). Boş: genel.
    pub fn with_scope(mut self, scope: &str) -> Translator {
        self.scope = scope.to_string();
        self
    }

    /// İsteklere eklenecek bağlam (DeepL). Varsayılan: kapalı (yerel model bağlam kullanmaz).
    pub fn with_context(mut self, context: ContextWindow) -> Translator {
        self.context = context;
        self
    }

    /// Ekrandaki metinleri bildirir: bellekte olanların çevirisini döner, olmayanları kuyruğa
    /// koyar. Kuyruk her çağrıda bu listeyle değiştirilir (ekrandan kalkan metin istenmez).
    pub fn request(&mut self, texts: &[String], now: f64) -> HashMap<String, String> {
        let items: Vec<Wanted> = texts.iter().map(|t| Wanted::new(t.clone())).collect();
        self.request_items(&items, now)
    }

    /// `request` gibi; metinler öncelik ve tür bilgisiyle.
    pub fn request_items(&mut self, items: &[Wanted], _now: f64) -> HashMap<String, String> {
        let mut found = HashMap::new();
        let mut wanted: Vec<(u8, usize, String)> = Vec::new();
        let mut seen = HashSet::new();
        self.conversation.clear();
        for (order, item) in items.iter().enumerate() {
            if !seen.insert(&item.text) {
                continue;
            }
            if item.conversation {
                self.conversation.insert(item.text.clone());
            }
            match self.lookup(&item.text) {
                Some((translation, how)) => {
                    // Ekranda gösterilen konuşma bağlama girer (önceden çevrilmiş ya da önbellekten
                    // gelmiş olsa da); tekrarlar bağlama bir kez girer.
                    if item.conversation {
                        self.context.remember(&terms::unprotect(&item.text));
                    }
                    match how {
                        Found::Exact => {}
                        Found::Normalized => self.stats.normalized_hits += 1,
                        Found::Session => self.stats.session_hits += 1,
                        Found::Derived => self.stats.derived_hits += 1,
                    }
                    found.insert(item.text.clone(), translation);
                }
                None => wanted.push((item.priority, order, item.text.clone())),
            }
        }
        wanted.sort_by_key(|(priority, order, _)| (*priority, *order));
        self.stats.cache_hits += found.len() as u64;
        self.wanted = wanted.into_iter().map(|(_, _, text)| text).collect();
        found
    }

    /// Saklama anahtarı: kısa metin profil kapsamında.
    fn storage_key(&self, text: &str) -> String {
        if !self.scope.is_empty() && is_short(text) {
            format!("{}\u{241F}{text}", self.scope)
        } else {
            text.to_string()
        }
    }

    fn lookup(&self, text: &str) -> Option<(String, Found)> {
        let key = self.storage_key(text);
        if let Some(translation) = self.cache.get(&key) {
            return Some((translation.clone(), Found::Exact));
        }
        if let Some(translation) = self.session.get(&key) {
            return Some((translation.clone(), Found::Session));
        }
        let normalized = memory_key(&key);
        if let Some(translation) = self.index.get(&normalized).and_then(|k| self.cache.get(k)) {
            return Some((translation.clone(), Found::Normalized));
        }
        if let Some(translation) = self.session_index.get(&normalized).and_then(|k| self.session.get(k)) {
            return Some((translation.clone(), Found::Session));
        }
        self.derived_translation(&key).map(|t| (t, Found::Derived))
    }

    /// Yalnızca rakamları farklı, bellekteki bir metnin çevirisinden bu metnin çevirisi.
    fn derived_translation(&self, key: &str) -> Option<String> {
        let source = self.templates.get(&digit_template(key)?)?;
        let translation = self.cache.get(source)?;
        let numbers = |s: &str| digit_runs(s).into_iter().map(|(a, b)| s[a..b].to_string()).collect::<Vec<_>>();
        let (old, new) = (numbers(source), numbers(key));
        if numbers(translation) != old || old.len() != new.len() {
            return None;
        }
        let mut out = String::with_capacity(translation.len());
        let mut last = 0;
        for ((start, end), number) in digit_runs(translation).into_iter().zip(&new) {
            out.push_str(&translation[last..start]);
            out.push_str(number);
            last = end;
        }
        out.push_str(&translation[last..]);
        Some(out)
    }

    /// Bellekteki çeviri (birebir; kuyruğa dokunmaz).
    pub fn cached(&self, text: &str) -> Option<&String> {
        let key = self.storage_key(text);
        self.cache.get(&key).or_else(|| self.session.get(&key))
    }

    /// Şu an istenebilecek metinler: kuyrukta, istek sürmeyen, bekleme süresi dolmuş olanlar.
    fn ready_texts(&self, now: f64) -> impl Iterator<Item = &String> {
        self.wanted
            .iter()
            .filter(move |text| self.lookup(text).is_none() && !self.in_flight.contains_key(*text) && self.cooldown.get(*text).is_none_or(|c| now >= c.until))
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

    pub fn breaker(&self) -> Breaker {
        self.breaker
    }

    /// Bağlamdaki replik sayısı.
    pub fn context_lines(&self) -> usize {
        self.context.len()
    }

    /// İşçi için sıradaki parti (en fazla `max` metin); bekleme sürüyorsa ya da iş yoksa `None`.
    /// Metinler istek sürüyor olarak işaretlenir; sonuç `complete` ile bildirilmelidir. Devre yarı
    /// açıkken yalnızca tek metinlik bir deneme gider (öncelikli olan: ekrandaki konuşma).
    pub fn take_batch(&mut self, now: f64, max: usize) -> Option<Batch> {
        if now < self.retry_after {
            return None;
        }
        if self.breaker == Breaker::Open {
            self.breaker = Breaker::HalfOpen;
        }
        let probe = self.breaker == Breaker::HalfOpen;
        if probe && self.probe.is_some() {
            return None;
        }
        let limit = if probe { 1 } else { max.max(1) };
        let texts: Vec<String> = self.ready_texts(now).take(limit).cloned().collect();
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
        if probe {
            self.probe = Some(id);
            self.stats.probes += 1;
        }
        Some(Batch { id, texts, started: now, context: self.context.text(), contextual: self.context.has_history(), probe })
    }

    /// Partinin sonucunu işler. Geçerli çeviriler belleğe yazılır; geçersizler (boş, kaynağın
    /// aynısı) bekletilip yeniden denenir. Hata olursa genel bekleme başlar. Bu partiye ait istek
    /// kayıtları kalkar (takıldığı için bırakılmış bir iş geç döndüyse sonuç yine kullanılır).
    /// Belleğe en az bir çeviri yazıldıysa `true`.
    pub fn complete(&mut self, batch: &Batch, result: Result<Vec<String>, TranslationError>, now: f64) -> bool {
        // Takıldığı için bırakılmış (`expire`) işin geç gelen hatası, o arada çalışan yeni işleri
        // genel beklemeye sokmamalı.
        let current = batch.texts.iter().any(|text| self.in_flight.get(text).is_some_and(|(id, _)| *id == batch.id));
        for text in &batch.texts {
            if self.in_flight.get(text).is_some_and(|(id, _)| *id == batch.id) {
                self.in_flight.remove(text);
            }
        }
        if self.probe == Some(batch.id) {
            self.probe = None;
        }
        let result = match result {
            Ok(translations) if translations.len() != batch.texts.len() => Err(TranslationError::InvalidResponse),
            other => other,
        };
        match result {
            Ok(translations) => {
                self.consecutive_failures = 0;
                self.retry_after = f64::NEG_INFINITY;
                self.breaker = Breaker::Closed;
                let mut stored = false;
                for (text, translation) in batch.texts.iter().zip(translations) {
                    if is_valid_translation(text, &translation) {
                        self.cooldown.remove(text);
                        self.store(text, translation, batch.contextual);
                        if self.conversation.contains(text) {
                            self.context.remember(&terms::unprotect(text));
                        }
                        self.stats.translated += 1;
                        stored = true;
                    } else {
                        self.stats.rejected += 1;
                        self.cool_down(text, now);
                    }
                }
                if stored {
                    self.stats.last_success = Some(now);
                    self.stats.last_latency = Some((now - batch.started).max(0.0));
                }
                stored
            }
            Err(error) => {
                self.stats.errors += 1;
                match error {
                    TranslationError::Timeout => self.stats.timeouts += 1,
                    TranslationError::RateLimited(_) => self.stats.rate_limited += 1,
                    _ => {}
                }
                let class = error.class();
                // Biçim/istek hatası partideki bir metinden kaynaklanabilir: metinler ayrıca
                // bekletilir ki aynı parti sonsuza kadar tekrarlanmasın.
                if matches!(class, ErrorClass::InvalidResponse | ErrorClass::InvalidRequest | ErrorClass::Local) {
                    for text in &batch.texts {
                        self.cool_down(text, now);
                    }
                }
                // İstek hatası servisin değil metnin sorunudur: devre ve genel bekleme değişmez.
                if current && class != ErrorClass::InvalidRequest {
                    self.consecutive_failures += 1;
                    self.retry_after = now + Translator::backoff(&error, self.consecutive_failures);
                    if error.needs_user() || self.consecutive_failures >= Translator::BREAKER_THRESHOLD || batch.probe {
                        if self.breaker != Breaker::Open {
                            self.stats.breaker_opens += 1;
                        }
                        self.breaker = Breaker::Open;
                    }
                }
                self.stats.last_error = Some(error);
                false
            }
        }
    }

    fn store(&mut self, text: &str, translation: String, contextual: bool) {
        let key = self.storage_key(text);
        if contextual && is_short(text) {
            if self.session.len() >= Translator::MAX_SESSION {
                self.session.clear();
                self.session_index.clear();
            }
            self.session_index.insert(memory_key(&key), key.clone());
            self.session.insert(key, translation);
            return;
        }
        if let Some(template) = digit_template(&key) {
            self.templates.insert(template, key.clone());
        }
        self.index.insert(memory_key(&key), key.clone());
        self.cache.insert(key, translation);
        self.dirty = true;
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
            // Takılan deneme yeni bir denemeyi engellemesin.
            if self.probe.is_some_and(|id| expired.contains(&id)) {
                self.probe = None;
            }
        }
        expired.into_iter().collect()
    }

    /// Sahne değişti (pencere yeniden açıldı, uyku, okuyucu yenilendi): konuşma bağlamı ve ona
    /// bağlı oturum belleği silinir.
    pub fn reset_context(&mut self) {
        self.context.reset();
        if !self.session.is_empty() || !self.context.is_empty() {
            self.session.clear();
            self.session_index.clear();
        }
        self.stats.context_resets += 1;
    }

    /// Uykudan uyanıldı: ağ değişmiş olabilir. Beklemeler sıfırlanır; servis daha önce hata
    /// veriyorsa tek metinlik denemeyle başlanır. Bağlam sıfırlanır.
    pub fn on_wake(&mut self, now: f64) {
        let _ = now;
        self.retry_after = f64::NEG_INFINITY;
        self.cooldown.clear();
        if self.breaker != Breaker::Closed || self.consecutive_failures > 0 {
            self.breaker = Breaker::HalfOpen;
        }
        self.reset_context();
    }

    /// Son kaydedilişten beri yeni çeviri geldiyse belleğin kopyası (kaydedilmek üzere).
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

    /// Kalıcı ve oturum belleğindeki girdi sayısı.
    pub fn memory_len(&self) -> (usize, usize) {
        (self.cache.len(), self.session.len())
    }

    pub fn stats(&self) -> &TranslatorStats {
        &self.stats
    }

    /// Hata sonrası genel bekleme (sn); `consecutive` art arda kaçıncı hata olduğu (1'den başlar).
    pub fn backoff(error: &TranslationError, consecutive: u32) -> f64 {
        let doubling = 2f64.powi(consecutive.saturating_sub(1).min(10) as i32);
        match error {
            // Kullanıcı düzeltmeden düzelmez: 5 dk, 10 dk… en çok 1 sa.
            TranslationError::QuotaExceeded | TranslationError::Unauthorized => (300.0 * doubling).min(3600.0),
            TranslationError::RateLimited(Some(seconds)) => (*seconds as f64).clamp(1.0, 600.0),
            TranslationError::RateLimited(None) => (5.0 * doubling).min(60.0),
            _ => doubling.min(30.0),
        }
    }
}

/// Normalize bellek anahtarı: büyük/küçük harf, boşluk, kıvrık tırnak ve sondaki tek nokta farkı
/// yok sayılır; `?`, `!`, `…` ve rakamlar korunur (anlamı değiştirir).
pub fn memory_key(text: &str) -> String {
    let mut key: String = text
        .chars()
        .map(|c| match c {
            '’' | '‘' => '\'',
            '“' | '”' => '"',
            other => other,
        })
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if key.ends_with('.') && !key.ends_with("..") {
        key.pop();
        key.truncate(key.trim_end().len());
    }
    key
}

/// Kısa metin mi (en çok `SHORT_WORDS` harfli kelime).
fn is_short(text: &str) -> bool {
    terms::unprotect(text).split_whitespace().filter(|w| w.chars().any(char::is_alphabetic)).count() <= Translator::SHORT_WORDS
}

/// Metindeki ASCII rakam dizilerinin bayt aralıkları.
fn digit_runs(text: &str) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    for (index, c) in text.char_indices() {
        match (c.is_ascii_digit(), start) {
            (true, None) => start = Some(index),
            (false, Some(s)) => {
                runs.push((s, index));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        runs.push((s, text.len()));
    }
    runs
}

/// Rakam dizileri `#` ile değiştirilmiş kalıp; rakam ya da harf yoksa `None`.
fn digit_template(text: &str) -> Option<String> {
    let runs = digit_runs(text);
    if runs.is_empty() || !text.chars().any(char::is_alphabetic) {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, end) in runs {
        out.push_str(&text[last..start]);
        out.push('#');
        last = end;
    }
    out.push_str(&text[last..]);
    Some(out)
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
        assert_eq!(Translator::backoff(&TranslationError::RateLimited(None), 1), 5.0);
        assert_eq!(Translator::backoff(&TranslationError::RateLimited(None), 2), 10.0);
        assert_eq!(Translator::backoff(&TranslationError::RateLimited(None), 9), 60.0);
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
    fn yalnizca_rakami_degisen_metin_istek_atmadan_cevrilir() {
        let mut translator = Translator::new(HashMap::from([("Respawn in 5 seconds".into(), "5 saniye içinde yeniden doğacaksın".into())]));

        let found = translator.request(&strings(&["Respawn in 4 seconds", "Respawn in 10 seconds"]), 0.0);

        assert_eq!(found["Respawn in 4 seconds"], "4 saniye içinde yeniden doğacaksın");
        assert_eq!(found["Respawn in 10 seconds"], "10 saniye içinde yeniden doğacaksın");
        assert!(translator.take_batch(0.0, 50).is_none(), "geri sayım her saniye istek atmamalı");
        assert!(translator.snapshot().len() == 1, "türetilen çeviri kalıcı önbelleğe yazılmaz");
    }

    #[test]
    fn rakam_sirasi_degisen_ceviriden_turetilmez() {
        // "2 of 5" → "5'te 2": sıra değiştiği için rakamlar güvenle yerine konamaz.
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Herb 2 of 5"]), 0.0);
        let batch = translator.take_batch(0.0, 50).unwrap();
        translator.complete(&batch, Ok(strings(&["Ot 5'te 2"])), 0.1);

        assert!(translator.request(&strings(&["Herb 3 of 5"]), 0.2).is_empty());
        assert_eq!(translator.take_batch(0.2, 50).unwrap().texts, strings(&["Herb 3 of 5"]));
        // Rakamsız metin kalıp sayılmaz.
        assert_eq!(digit_template("No digits"), None);
        assert_eq!(digit_template("123"), None);
    }

    #[test]
    fn birakilan_isin_gec_hatasi_yeni_isleri_bekletmez() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Hello"]), 0.0);
        let stuck = translator.take_batch(0.0, 50).unwrap();
        translator.expire(31.0, 30.0);
        translator.request(&strings(&["Hello", "World"]), 31.0);
        let retry = translator.take_batch(31.0, 50).unwrap();

        translator.complete(&stuck, Err(TranslationError::Timeout), 32.0);

        assert_eq!(translator.backoff_remaining(32.0), 0.0);
        assert_eq!(translator.stats().timeouts, 1, "hata yine sayılır");
        assert!(translator.complete(&retry, echo(&retry.texts), 32.5));
    }

    fn fail(translator: &mut Translator, now: f64, error: TranslationError) -> Batch {
        let batch = translator.take_batch(now, 50).expect("parti alınmalı");
        translator.complete(&batch, Err(error), now);
        batch
    }

    #[test]
    fn normalize_eslesme_ayni_metni_yeniden_istemez() {
        let mut translator = Translator::new(HashMap::from([("The door is locked.".into(), "Kapı kilitli.".into())]));

        let found = translator.request(&strings(&["THE DOOR IS LOCKED", "The  door is locked", "the door is locked."]), 0.0);

        assert_eq!(found.len(), 3);
        assert!(found.values().all(|t| t == "Kapı kilitli."));
        assert_eq!(translator.stats().normalized_hits, 3);
        assert!(translator.take_batch(0.0, 50).is_none());
    }

    #[test]
    fn soru_ve_unlem_isareti_anlami_degistirir() {
        let mut translator = Translator::new(HashMap::from([("Wait.".into(), "Bekle.".into()), ("Open the door".into(), "Kapıyı aç".into())]));

        let found = translator.request(&strings(&["Wait?", "Wait!", "Wait...", "Open the doors", "Open the door"]), 0.0);

        // Bulanık eşleştirme yok: yalnızca birebir aynı metin bulunur.
        assert_eq!(found.keys().collect::<Vec<_>>(), vec!["Open the door"]);
        assert_eq!(translator.take_batch(0.0, 50).unwrap().texts, strings(&["Wait?", "Wait!", "Wait...", "Open the doors"]));
        assert_eq!(memory_key("  Wait .  "), "wait");
        assert_eq!(memory_key("It’s “fine”."), "it's \"fine\"");
    }

    #[test]
    fn kisa_metin_oyun_profiline_ozel_saklanir() {
        let mut rpg = Translator::new(HashMap::new()).with_scope("rpg");
        rpg.request(&strings(&["Charge!", "The gate is closed for the night."]), 0.0);
        let batch = rpg.take_batch(0.0, 50).unwrap();
        rpg.complete(&batch, Ok(strings(&["Hücum!", "Kapı gece kapalı."])), 0.1);
        let saved = rpg.take_dirty_snapshot().unwrap();

        // Aynı dosyayı başka oyun profili okur: kısa metin oraya taşınmaz, uzun cümle ortaktır.
        let mut racing = Translator::new(saved.clone()).with_scope("racing");
        let found = racing.request(&strings(&["Charge!", "The gate is closed for the night."]), 1.0);
        assert_eq!(found.keys().collect::<Vec<_>>(), vec!["The gate is closed for the night."]);
        let mut same_game = Translator::new(saved).with_scope("rpg");
        assert_eq!(same_game.request(&strings(&["Charge!"]), 1.0)["Charge!"], "Hücum!");
    }

    #[test]
    fn baglamla_cevrilen_kisa_metin_diske_yazilmaz_ve_sahne_degisince_silinir() {
        let mut translator = Translator::new(HashMap::new()).with_context(ContextWindow::default());
        let talk = |text: &str| Wanted { text: text.into(), priority: 0, conversation: true };
        translator.request_items(&[talk("Are you ready to leave the village?")], 0.0);
        let first = translator.take_batch(0.0, 50).unwrap();
        assert_eq!(first.context, "", "ilk istekte bağlam yok");
        translator.complete(&first, Ok(strings(&["Köyden ayrılmaya hazır mısın?"])), 0.1);

        translator.request_items(&[talk("Not yet.")], 0.2);
        let second = translator.take_batch(0.2, 50).unwrap();
        assert_eq!(second.context, "Are you ready to leave the village?");
        assert!(second.contextual);
        translator.complete(&second, Ok(strings(&["Henüz değil."])), 0.3);

        assert_eq!(translator.request_items(&[talk("Not yet.")], 0.4)["Not yet."], "Henüz değil.");
        let saved = translator.take_dirty_snapshot().unwrap();
        assert!(saved.contains_key("Are you ready to leave the village?"));
        assert!(!saved.contains_key("Not yet."), "bağlama bağlı kısa çeviri diske yazılmamalı");

        translator.reset_context();

        assert!(translator.request_items(&[talk("Not yet.")], 0.5).is_empty(), "sahne değişince yeniden çevrilmeli");
        assert_eq!(translator.take_batch(0.5, 50).unwrap().context, "");
    }

    /// Önceden (konuşma olarak işaretlenmeden) çevrilmiş replik, ekranda konuşma olarak
    /// gösterilince bağlama girer.
    #[test]
    fn onceden_cevrilip_gosterilen_konusma_baglama_girer() {
        let mut translator = Translator::new(HashMap::new()).with_context(ContextWindow::default());
        let line = "Are you ready to leave the village?";
        translator.request_items(&[Wanted::new(line)], 0.0);
        let prefetched = translator.take_batch(0.0, 50).unwrap();
        translator.complete(&prefetched, echo(&prefetched.texts), 0.1);

        translator.request_items(&[Wanted { text: line.into(), priority: 0, conversation: true }], 0.2);
        translator.request(&strings(&["Not yet."]), 0.3);

        assert_eq!(translator.take_batch(0.3, 50).unwrap().context, line);
    }

    #[test]
    fn menu_ogesi_baglama_girmez() {
        let mut translator = Translator::new(HashMap::new()).with_context(ContextWindow::default());
        translator.request_items(&[Wanted::new("Open the map and choose a destination")], 0.0);
        let batch = translator.take_batch(0.0, 50).unwrap();
        translator.complete(&batch, echo(&batch.texts), 0.1);

        translator.request(&strings(&["Next"]), 0.2);
        assert_eq!(translator.take_batch(0.2, 50).unwrap().context, "");
    }

    #[test]
    fn oncelikli_metin_once_istenir() {
        let mut translator = Translator::new(HashMap::new());
        translator.request_items(&[Wanted { priority: 4, ..Wanted::new("Options") }, Wanted { priority: 0, ..Wanted::new("Run, they are coming!") }, Wanted { priority: 2, ..Wanted::new("Low health") }], 0.0);

        assert_eq!(translator.take_batch(0.0, 2).unwrap().texts, strings(&["Run, they are coming!", "Low health"]));
    }

    #[test]
    fn devre_uc_hatada_acilir_ve_tek_metinle_denenir() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Line one here", "Line two here", "Line three here"]), 0.0);
        fail(&mut translator, 0.0, TranslationError::Http(500));
        fail(&mut translator, 1.0, TranslationError::Network("bağlantı yok".into()));
        assert_eq!(translator.breaker(), Breaker::Closed);
        fail(&mut translator, 3.0, TranslationError::Timeout);
        assert_eq!(translator.breaker(), Breaker::Open);
        assert_eq!(translator.stats().breaker_opens, 1);
        assert!(translator.take_batch(6.9, 50).is_none(), "4 sn bekleme");

        // Servis düzeldi: önce yalnızca ekrandaki ilk metin denenir, eski kuyruk birden gitmez.
        let probe = translator.take_batch(7.0, 50).unwrap();
        assert!(probe.probe);
        assert_eq!(probe.texts, strings(&["Line one here"]));
        assert_eq!(translator.breaker(), Breaker::HalfOpen);
        assert!(translator.take_batch(7.0, 50).is_none(), "deneme sürerken ikinci parti yok");
        translator.complete(&probe, echo(&probe.texts), 7.2);

        assert_eq!(translator.breaker(), Breaker::Closed);
        assert_eq!(translator.take_batch(7.2, 50).unwrap().texts.len(), 2);
    }

    #[test]
    fn basarisiz_deneme_devreyi_yeniden_acar() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Hello there", "General Kenobi"]), 0.0);
        for now in [0.0, 1.0, 3.0] {
            fail(&mut translator, now, TranslationError::Http(503));
        }
        let probe = fail(&mut translator, 7.0, TranslationError::Http(503));

        assert!(probe.probe && probe.texts.len() == 1);
        assert_eq!(translator.breaker(), Breaker::Open);
        assert_eq!(translator.backoff_remaining(7.0), 8.0, "bekleme ikiye katlanır");
    }

    #[test]
    fn anahtar_hatasi_surekli_denenmez_ve_bekleme_artar() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Hello there"]), 0.0);
        fail(&mut translator, 0.0, TranslationError::Unauthorized);

        assert_eq!(translator.breaker(), Breaker::Open, "tek hatada devre açılır");
        assert!(translator.stats().last_error.as_ref().unwrap().needs_user());
        assert!(translator.take_batch(299.0, 50).is_none());
        fail(&mut translator, 300.0, TranslationError::Unauthorized);
        assert_eq!(translator.backoff_remaining(300.0), 600.0);
        assert_eq!(Translator::backoff(&TranslationError::QuotaExceeded, 10), 3600.0);
    }

    #[test]
    fn retry_after_suresine_uyulur() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Hello there"]), 0.0);
        fail(&mut translator, 0.0, TranslationError::RateLimited(Some(12)));

        assert!(translator.take_batch(11.9, 50).is_none());
        assert!(translator.take_batch(12.0, 50).is_some());
        assert_eq!(translator.stats().rate_limited, 1);
    }

    #[test]
    fn istek_hatasi_yalnizca_o_metni_bekletir() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Broken request text"]), 0.0);
        fail(&mut translator, 0.0, TranslationError::Http(400));

        assert_eq!(translator.backoff_remaining(0.0), 0.0, "servis beklemeye girmez");
        assert_eq!(translator.breaker(), Breaker::Closed);
        translator.request(&strings(&["Broken request text", "Healthy text here"]), 0.1);
        assert_eq!(translator.take_batch(0.1, 50).unwrap().texts, strings(&["Healthy text here"]));
        assert_eq!(TranslationError::Http(400).class(), ErrorClass::InvalidRequest);
        assert_eq!(TranslationError::Http(502).class(), ErrorClass::Server);
    }

    #[test]
    fn bos_ve_bozuk_yanit_bellege_yazilmaz() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Hello there"]), 0.0);
        let batch = translator.take_batch(0.0, 50).unwrap();
        assert!(!translator.complete(&batch, Ok(vec![]), 0.1));
        assert_eq!(translator.stats().last_error, Some(TranslationError::InvalidResponse));
        let batch = translator.take_batch(5.0, 50).unwrap();
        assert!(!translator.complete(&batch, Err(TranslationError::InvalidResponse), 5.1));

        assert!(translator.snapshot().is_empty());
        assert!(translator.take_dirty_snapshot().is_none());
    }

    #[test]
    fn uyanmada_beklemeler_sifirlanir_ve_tek_metinle_baslanir() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Hello there", "Second line"]), 0.0);
        fail(&mut translator, 0.0, TranslationError::Network("uyku".into()));
        assert!(translator.take_batch(0.5, 50).is_none());

        translator.on_wake(0.5);

        let probe = translator.take_batch(0.5, 50).expect("uyanınca hemen denenmeli");
        assert!(probe.probe && probe.texts.len() == 1);
        assert_eq!(translator.stats().context_resets, 1);
    }

    #[test]
    fn takilan_deneme_yeni_denemeyi_engellemez() {
        let mut translator = Translator::new(HashMap::new());
        translator.request(&strings(&["Hello there"]), 0.0);
        for now in [0.0, 1.0, 3.0] {
            fail(&mut translator, now, TranslationError::Timeout);
        }
        let stuck = translator.take_batch(7.0, 50).unwrap();
        assert!(translator.take_batch(8.0, 50).is_none());

        translator.expire(40.0, 30.0);

        assert!(translator.take_batch(40.0, 50).is_some(), "yeni deneme gitmeli");
        assert!(stuck.probe);
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
