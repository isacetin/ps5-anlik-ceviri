//! Çeviri işçileri: ana döngüden bağımsız çalışır, ana döngüyü hiç bekletmez.
//!
//! - Sabit sayıda işçi thread'i (yerel modelde 1, DeepL'de 2) ortak kuyruktan parti alır. Kuyruk
//!   yalnızca o an ekranda olan metinlerdir (`Translator::request`); eski metin birikmez.
//! - Arka uç panik yaparsa işçi ölmez: hata sayılır, metinler bekletilip yeniden denenir.
//! - Bir parti `stuck_after` saniyeden uzun sürerse işçi "takıldı" sayılır; metinleri serbest
//!   bırakılır ve yerine yeni işçi açılır (en fazla `max_recoveries` kez / `recovery_window`).
//! - Önbellek diske ayrı bir thread'de, en sık `save_every` saniyede bir yazılır; kapanışta son
//!   hali yazılır. (Eskiden her çeviriden sonra tüm önbellek yazılıyordu.)

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use ceviri_core::context::ContextWindow;
use ceviri_core::health::{WORKER_BUSY, WORKER_IDLE, WORKER_STUCK};
use ceviri_core::{storage, Breaker, TranslationError, Translator, TranslatorStats, Wanted};

/// Çeviri arka ucu (gerçekte DeepL ya da yerel model, testlerde sahte). İkinci argüman bağlamdır
/// (az önce geçen konuşma ve oyun ipucu, düz metin; boş olabilir). Yerel model yok sayar.
pub type Backend = Arc<dyn Fn(&[String], &str) -> Result<Vec<String>, TranslationError> + Send + Sync>;

#[derive(Clone, Debug)]
pub struct Options {
    pub workers: usize,
    /// Bir parti bundan uzun sürerse işçi takılmış sayılır (sn).
    pub stuck_after: f64,
    /// Önbellek en sık bu aralıkla diske yazılır (sn).
    pub save_every: f64,
    /// `recovery_window` saniye içinde en fazla bu kadar işçi yenilenir (sonsuz döngü olmasın).
    pub max_recoveries: usize,
    pub recovery_window: f64,
    /// Kısa metinlerin bellek kapsamı (oyun profili kimliği; boşsa genel).
    pub scope: String,
    /// İsteklere eklenecek bağlam (DeepL); `None`: bağlam gönderilmez (yerel model).
    pub context: Option<ContextWindow>,
}

impl Default for Options {
    fn default() -> Options {
        Options { workers: 1, stuck_after: 20.0, save_every: 5.0, max_recoveries: 3, recovery_window: 600.0, scope: String::new(), context: None }
    }
}

/// Teşhis için anlık durum.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServiceStats {
    pub queue: usize,
    pub in_flight: usize,
    /// `health::WORKER_*`.
    pub worker: &'static str,
    pub backoff: f64,
    pub recoveries: u64,
    pub breaker: Breaker,
    /// Kalıcı ve oturum belleğindeki girdi sayısı.
    pub memory: (usize, usize),
    pub context_lines: usize,
    pub translator: TranslatorStats,
}

pub struct TranslationService {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    work: Condvar,
    backend: Backend,
    ready: Sender<()>,
    cache_path: Option<PathBuf>,
    started: Instant,
    options: Options,
    /// Aynı anda tek kayıt (aynı geçici dosyaya iki yazım olmasın).
    saving: Mutex<()>,
}

struct State {
    translator: Translator,
    stop: bool,
    slots: Vec<Slot>,
    recovery_times: Vec<f64>,
    recoveries: u64,
}

#[derive(Clone, Copy, Default)]
struct Slot {
    generation: u64,
    busy_since: Option<f64>,
}

impl Shared {
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Kilit içinde panik olmaz (arka uç kilitsiz çağrılır); yine de zehirli kilit döngüyü
        // durdurmasın.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(&self, cache: &HashMap<String, String>) {
        let Some(path) = &self.cache_path else { return };
        let _guard = self.saving.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = storage::save_cache(path, cache);
    }
}

impl TranslationService {
    /// İşçileri başlatır. `ready`, önbelleğe yeni çeviri yazılınca haber verir.
    pub fn start(backend: Backend, cache: HashMap<String, String>, cache_path: Option<PathBuf>, options: Options, ready: Sender<()>) -> TranslationService {
        let workers = options.workers.max(1);
        let translator = Translator::new(cache).with_scope(&options.scope).with_context(options.context.clone().unwrap_or_else(ContextWindow::disabled));
        let shared = Arc::new(Shared {
            state: Mutex::new(State { translator, stop: false, slots: vec![Slot::default(); workers], recovery_times: Vec::new(), recoveries: 0 }),
            work: Condvar::new(),
            backend,
            ready,
            cache_path,
            started: Instant::now(),
            options,
            saving: Mutex::new(()),
        });
        for slot in 0..workers {
            spawn_worker(&shared, slot, 0);
        }
        let saver = shared.clone();
        std::thread::Builder::new().name("ceviri-onbellek".into()).spawn(move || save_loop(&saver)).expect("thread açılamadı");
        TranslationService { shared }
    }

    /// Ekrandaki metinleri bildirir; önbellekteki çevirileri döner, eksikleri işçilere verir.
    pub fn request(&self, texts: &[String]) -> HashMap<String, String> {
        let items: Vec<Wanted> = texts.iter().map(|t| Wanted::new(t.clone())).collect();
        self.request_items(&items)
    }

    /// `request` gibi; öncelik ve konuşma bilgisiyle.
    pub fn request_items(&self, items: &[Wanted]) -> HashMap<String, String> {
        let now = self.shared.now();
        let mut state = self.shared.lock();
        let found = state.translator.request_items(items, now);
        let pending = state.translator.queued(now) > 0;
        drop(state);
        if pending {
            self.shared.work.notify_all();
        }
        found
    }

    /// Takılan işçileri bulur ve (sınır dolmadıysa) yeniler. Yapılan toparlanmaları anlatan
    /// mesajlar döner; ana döngü her turda çağırır.
    pub fn maintain(&self) -> Vec<String> {
        let now = self.shared.now();
        let options = &self.shared.options;
        let mut messages = Vec::new();
        let mut state = self.shared.lock();
        if state.stop {
            return messages;
        }
        for slot in 0..state.slots.len() {
            let Some(since) = state.slots[slot].busy_since else { continue };
            if now - since <= options.stuck_after {
                continue;
            }
            state.recovery_times.retain(|t| now - t < options.recovery_window);
            if state.recovery_times.len() >= options.max_recoveries {
                continue;
            }
            // Takılan işin metinleri serbest; geç dönerse sonucu yine önbelleğe yazılır.
            state.translator.expire(now, options.stuck_after);
            let generation = state.slots[slot].generation + 1;
            state.slots[slot] = Slot { generation, busy_since: None };
            state.recovery_times.push(now);
            state.recoveries += 1;
            spawn_worker(&self.shared, slot, generation);
            messages.push(format!("Çeviri {:.0} sn'dir yanıt vermiyor; yeni çeviri işçisi açıldı.", now - since));
        }
        messages
    }

    /// Sahne değişti: konuşma bağlamı ve ona bağlı oturum belleği silinir.
    pub fn reset_context(&self) {
        self.shared.lock().translator.reset_context();
    }

    /// Uykudan uyanıldı: beklemeler sıfırlanır, servis tek metinle denenir, bağlam silinir.
    pub fn on_wake(&self) {
        let now = self.shared.now();
        self.shared.lock().translator.on_wake(now);
        self.shared.work.notify_all();
    }

    pub fn stats(&self) -> ServiceStats {
        let now = self.shared.now();
        let state = self.shared.lock();
        let stuck = state.slots.iter().any(|s| s.busy_since.is_some_and(|since| now - since > self.shared.options.stuck_after));
        let busy = state.slots.iter().any(|s| s.busy_since.is_some());
        ServiceStats {
            queue: state.translator.queued(now),
            in_flight: state.translator.in_flight(),
            worker: if stuck {
                WORKER_STUCK
            } else if busy {
                WORKER_BUSY
            } else {
                WORKER_IDLE
            },
            backoff: state.translator.backoff_remaining(now),
            recoveries: state.recoveries,
            breaker: state.translator.breaker(),
            memory: state.translator.memory_len(),
            context_lines: state.translator.context_lines(),
            translator: state.translator.stats().clone(),
        }
    }

    /// Hizmetin saatine göre şimdiki an (sn); `TranslatorStats` içindeki anlar bu saattedir.
    pub fn now(&self) -> f64 {
        self.shared.now()
    }

    /// İşçileri durdurur ve önbelleği son haliyle yazar. Takılı bir işçi beklenmez.
    pub fn shutdown(&self) {
        let snapshot = {
            let mut state = self.shared.lock();
            if state.stop {
                return;
            }
            state.stop = true;
            state.translator.take_dirty_snapshot()
        };
        self.shared.work.notify_all();
        if let Some(cache) = snapshot {
            self.shared.save(&cache);
        }
    }
}

impl Drop for TranslationService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn spawn_worker(shared: &Arc<Shared>, slot: usize, generation: u64) {
    let shared = shared.clone();
    std::thread::Builder::new()
        .name(format!("ceviri-isci-{slot}"))
        .spawn(move || worker_loop(&shared, slot, generation))
        .expect("thread açılamadı");
}

fn worker_loop(shared: &Shared, slot: usize, generation: u64) {
    loop {
        let batch = {
            let mut state = shared.lock();
            loop {
                if state.stop || state.slots[slot].generation != generation {
                    return;
                }
                let now = shared.now();
                if let Some(batch) = state.translator.take_batch(now, Translator::MAX_BATCH) {
                    state.slots[slot].busy_since = Some(now);
                    break batch;
                }
                // Bekleme süresi dolunca da uyansın diye kısa aralıklarla bakılır.
                state = shared.work.wait_timeout(state, Duration::from_millis(250)).unwrap_or_else(PoisonError::into_inner).0;
            }
        };
        let result = catch_unwind(AssertUnwindSafe(|| (shared.backend)(&batch.texts, &batch.context))).unwrap_or_else(|panic| Err(TranslationError::Local(panic_message(&panic))));
        let stored = {
            let mut state = shared.lock();
            let stored = state.translator.complete(&batch, result, shared.now());
            if state.slots[slot].generation == generation {
                state.slots[slot].busy_since = None;
            }
            stored
        };
        if stored {
            let _ = shared.ready.send(());
        }
        // Başka metin kuyrukta bekliyorsa diğer işçiler de uyansın.
        shared.work.notify_all();
    }
}

fn save_loop(shared: &Shared) {
    let every = Duration::from_secs_f64(shared.options.save_every.max(0.1));
    loop {
        let snapshot = {
            // İşçiler koşul değişkenini sık uyandırır; kayıt yine de en sık `every` aralıkla olur.
            let due = Instant::now() + every;
            let mut state = shared.lock();
            loop {
                if state.stop {
                    return;
                }
                let left = due.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                state = shared.work.wait_timeout(state, left).unwrap_or_else(PoisonError::into_inner).0;
            }
            state.translator.take_dirty_snapshot()
        };
        if let Some(cache) = snapshot {
            shared.save(&cache);
        }
    }
}

pub fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    let detail = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned()).unwrap_or_else(|| "bilinmeyen hata".into());
    format!("beklenmeyen hata: {detail}")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn echo() -> Backend {
        Arc::new(|texts: &[String], _: &str| Ok(texts.iter().map(|t| format!("TR({t})")).collect()))
    }

    /// `texts` önbelleğe girene kadar her 20 ms'de bir ister (ana döngü gibi).
    fn wait_translated(service: &TranslationService, texts: &[String], limit: Duration) -> HashMap<String, String> {
        let deadline = Instant::now() + limit;
        loop {
            let found = service.request(texts);
            if found.len() == texts.len() || Instant::now() > deadline {
                return found;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn cevirir_ve_haber_verir() {
        let (ready, notified) = mpsc::channel();
        let service = TranslationService::start(echo(), HashMap::new(), None, Options::default(), ready);
        let texts = strings(&["Hello there", "Jump"]);

        let found = wait_translated(&service, &texts, Duration::from_secs(2));

        assert_eq!(found["Jump"], "TR(Jump)");
        assert!(notified.recv_timeout(Duration::from_secs(1)).is_ok());
        assert_eq!(service.stats().worker, WORKER_IDLE);
    }

    #[test]
    fn arka_uc_panigi_iscilyi_oldurmez() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let backend: Backend = Arc::new(move |texts: &[String], _: &str| {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("model çöktü");
            }
            Ok(texts.iter().map(|t| format!("TR({t})")).collect())
        });
        let (ready, _notified) = mpsc::channel();
        let service = TranslationService::start(backend, HashMap::new(), None, Options::default(), ready);
        let texts = strings(&["Hello there"]);

        // İlk deneme panikler; metin birkaç sn bekletilip yeniden denenir ve çevrilir.
        let found = wait_translated(&service, &texts, Duration::from_secs(6));

        assert_eq!(found["Hello there"], "TR(Hello there)");
        let stats = service.stats();
        assert_eq!(stats.translator.errors, 1);
        assert!(matches!(stats.translator.last_error, Some(TranslationError::Local(ref m)) if m.contains("model çöktü")));
    }

    #[test]
    fn takilan_isci_yenilenir_ve_metin_cevrilir() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        // İlk çağrı "sonsuza kadar" takılır (testte 3 sn), sonrakiler hemen döner.
        let backend: Backend = Arc::new(move |texts: &[String], _: &str| {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                std::thread::sleep(Duration::from_secs(3));
            }
            Ok(texts.iter().map(|t| format!("TR({t})")).collect())
        });
        let (ready, _notified) = mpsc::channel();
        let options = Options { stuck_after: 0.3, ..Options::default() };
        let service = TranslationService::start(backend, HashMap::new(), None, options, ready);
        let texts = strings(&["Are you still there?"]);
        service.request(&texts);
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(service.stats().worker, WORKER_STUCK);

        let messages = service.maintain();

        assert_eq!(messages.len(), 1, "{messages:?}");
        let found = wait_translated(&service, &texts, Duration::from_secs(2));
        assert_eq!(found["Are you still there?"], "TR(Are you still there?)");
        assert_eq!(service.stats().recoveries, 1);
        assert_ne!(service.stats().worker, WORKER_STUCK);
    }

    #[test]
    fn yenileme_sayisi_sinirlidir() {
        let backend: Backend = Arc::new(|_: &[String], _: &str| {
            std::thread::sleep(Duration::from_secs(2));
            Ok(vec![])
        });
        let (ready, _notified) = mpsc::channel();
        let options = Options { stuck_after: 0.05, max_recoveries: 2, ..Options::default() };
        let service = TranslationService::start(backend, HashMap::new(), None, options, ready);
        let mut recovered = 0;
        for i in 0..8 {
            service.request(&[format!("text number {i}")]);
            std::thread::sleep(Duration::from_millis(120));
            recovered += service.maintain().len();
        }
        assert_eq!(recovered, 2);
    }

    #[test]
    fn kapanista_onbellek_yazilir() {
        let dir = std::env::temp_dir().join(format!("ceviri-engine-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("translations.json");
        let (ready, _notified) = mpsc::channel();
        let options = Options { save_every: 3600.0, ..Options::default() };
        let service = TranslationService::start(echo(), HashMap::new(), Some(path.clone()), options, ready);
        wait_translated(&service, &strings(&["Jump"]), Duration::from_secs(2));
        assert!(!path.exists(), "kayıt aralığı dolmadan yazılmamalı");

        service.shutdown();

        assert_eq!(storage::load_cache(&path)["Jump"], "TR(Jump)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn onbellek_belli_araliklarla_yazilir() {
        let dir = std::env::temp_dir().join(format!("ceviri-engine-periodic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("translations.json");
        let (ready, _notified) = mpsc::channel();
        let options = Options { save_every: 0.2, ..Options::default() };
        let service = TranslationService::start(echo(), HashMap::new(), Some(path.clone()), options, ready);
        wait_translated(&service, &strings(&["Jump"]), Duration::from_secs(2));

        let deadline = Instant::now() + Duration::from_secs(3);
        while !path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(storage::load_cache(&path)["Jump"], "TR(Jump)");
        drop(service);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
