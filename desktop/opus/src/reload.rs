//! Takılan yerel modelden kurtulma. CTranslate2 çağrısı iptal edilemez; model içinde takılırsa
//! çağıran thread ve modelin tek kopyası da takılır, sonraki çağrılar onun arkasında bekler.
//!
//! Burada her çağrı süre bütçesiyle yapılır: model çağrısı yardımcı bir thread'de koşar, çağıran
//! en çok `budget` kadar bekler. Bütçe aşılırsa o kopya "takılı" sayılır, model **bir kez** taze
//! yüklenir ve aynı metinler taze kopyayla hemen yeniden çevrilir. Çağıran (çeviri işçisi) böylece
//! takılmaz; o anda ekrandaki altyazı kaybolmadan çevrilir. Takılı çağrı eski kopyayla sürer; bitince
//! eski kopya bırakılır (sonucu atılır: o metin zaten yeniden çevrildi).
//!
//! Sınırlar (kontrolsüz kopya yok):
//! - Aynı anda en çok `max_stuck` takılı kopya yaşar (her kopya ~250 MB). Sınır doluysa yeniden
//!   yükleme yapılmaz; asıl çağrı `hard_limit`'e kadar beklenir, yine bitmezse hata döner (çeviri
//!   servisi bekleyip yeniden dener, devre kesici devreye girer).
//! - `window` içinde en çok `max_reloads` yeniden yükleme.
//! - Yükleme tek seferde bir tane (kilit).
//!
//! Bütçe uyarlamalıdır: başarılı çağrıların karakter başına süresinin ortalaması alınır, bütçe bunun
//! ~8 katıdır (en az `min_budget`). Yavaş bilgisayarda olağan çağrılar takılma sayılmaz. Henüz ölçüm
//! yokken (ilk çağrı) bütçe `max_budget`'tır; `reloading` modeli yükler yüklemez bir ısınma çağrısıyla
//! ölçer.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

pub type Factory<T> = Box<dyn Fn() -> Result<T, String> + Send + Sync>;

#[derive(Clone, Debug)]
pub struct Options {
    pub min_budget: Duration,
    pub max_budget: Duration,
    /// Bütçe = beklenen süre × bu kat (en az `min_budget`).
    pub budget_factor: f64,
    /// Takılma sürerken (yeniden yükleme yapılamıyorsa) en uzun bekleme.
    pub hard_limit: Duration,
    pub max_stuck: usize,
    pub max_reloads: usize,
    pub window: Duration,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            min_budget: Duration::from_millis(700),
            max_budget: Duration::from_secs(4),
            budget_factor: 8.0,
            hard_limit: Duration::from_secs(3),
            max_stuck: 1,
            max_reloads: 3,
            window: Duration::from_secs(600),
        }
    }
}

/// Teşhis için sayaçlar.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub calls: u64,
    /// Bütçeyi aşan (takılan) çağrı sayısı.
    pub stuck: u64,
    pub reloads: u64,
    /// Takılma görüldü ama sınır yüzünden yeniden yüklenemedi.
    pub refused_reloads: u64,
    pub errors: u64,
    /// Şu an yaşayan takılı kopya sayısı.
    pub stuck_alive: usize,
    pub last_error: Option<String>,
}

pub struct Reloading<T> {
    factory: Factory<T>,
    options: Options,
    state: Mutex<State<T>>,
    /// Aynı anda tek yükleme.
    loading: Mutex<()>,
}

struct State<T> {
    current: Arc<T>,
    /// Takılı çağrıların tuttuğu eski kopyalar (çağrı bitince düşer).
    stuck: Vec<Weak<T>>,
    reload_times: VecDeque<Instant>,
    /// Karakter (maliyet birimi) başına ortalama süre (sn); ilk başarılı çağrıyla öğrenilir.
    per_unit: Option<f64>,
    stats: Stats,
}

impl<T: Send + Sync + 'static> Reloading<T> {
    pub fn new(factory: Factory<T>, options: Options) -> Result<Reloading<T>, String> {
        let current = Arc::new(factory()?);
        Ok(Reloading {
            factory,
            options,
            state: Mutex::new(State { current, stuck: Vec::new(), reload_times: VecDeque::new(), per_unit: None, stats: Stats::default() }),
            loading: Mutex::new(()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `cost` biriminde (karakter) işin bütçesi.
    pub fn budget(&self, cost: f64) -> Duration {
        let state = self.lock();
        match state.per_unit {
            Some(per) => Duration::from_secs_f64(per * cost.max(1.0) * self.options.budget_factor).clamp(self.options.min_budget, self.options.max_budget),
            None => self.options.max_budget,
        }
    }

    /// `f`'i geçerli kopyayla, bütçe içinde çalıştırır. Bütçe aşılırsa taze kopyayla bir kez daha
    /// dener. `cost`: işin büyüklüğü (karakter sayısı).
    pub fn call<R: Send + 'static>(&self, cost: f64, f: impl Fn(&T) -> R + Send + Sync + 'static) -> Result<R, String> {
        let f = Arc::new(f);
        let budget = self.budget(cost);
        let (model, first) = {
            let mut state = self.lock();
            state.stats.calls += 1;
            let model = state.current.clone();
            (model.clone(), spawn(model, f.clone()))
        };
        let started = Instant::now();
        match first.recv_timeout(budget) {
            Ok(result) => {
                self.learn(started.elapsed(), cost);
                return Ok(result);
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(self.fail("yerel model çağrısı beklenmedik şekilde bitti (panik)")),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        // Takıldı: bu kopya bir daha kullanılmaz (yeniden yüklenebiliyorsa).
        match self.reload_after_stuck(&model) {
            Some(fresh) => {
                let second = spawn(fresh, f);
                let started = Instant::now();
                match second.recv_timeout(budget.max(self.options.min_budget) * 2) {
                    Ok(result) => {
                        self.learn(started.elapsed(), cost);
                        Ok(result)
                    }
                    Err(_) => Err(self.fail("yerel model yeniden yüklendikten sonra da yanıt vermedi")),
                }
            }
            None => {
                // Yeniden yüklenemiyor (sınır): asıl çağrı biraz daha beklenir.
                let left = self.options.hard_limit.saturating_sub(started.elapsed() + budget);
                match first.recv_timeout(left) {
                    Ok(result) => Ok(result),
                    Err(_) => Err(self.fail("yerel model takıldı; yeniden yükleme sınırı dolu")),
                }
            }
        }
    }

    fn learn(&self, took: Duration, cost: f64) {
        let mut state = self.lock();
        let per = took.as_secs_f64() / cost.max(1.0);
        state.per_unit = Some(state.per_unit.map_or(per, |old| old * 0.8 + per * 0.2));
    }

    fn fail(&self, message: &str) -> String {
        let mut state = self.lock();
        state.stats.errors += 1;
        state.stats.last_error = Some(message.to_string());
        message.to_string()
    }

    /// Takılan kopya hâlâ geçerliyse (başka çağrı yenilememişse) taze kopya yükler. Sınır doluysa
    /// `None`. Takılan kopya zaten değiştirildiyse geçerli (taze) kopyayı döner.
    fn reload_after_stuck(&self, stuck: &Arc<T>) -> Option<Arc<T>> {
        let _loading = self.loading.lock().unwrap_or_else(PoisonError::into_inner);
        {
            let mut state = self.lock();
            state.stats.stuck += 1;
            if !Arc::ptr_eq(&state.current, stuck) {
                return Some(state.current.clone());
            }
            state.stuck.retain(|weak| weak.strong_count() > 0);
            let window = self.options.window;
            state.reload_times.retain(|t| t.elapsed() < window);
            if state.stuck.len() >= self.options.max_stuck || state.reload_times.len() >= self.options.max_reloads {
                state.stats.refused_reloads += 1;
                return None;
            }
        }
        // Yükleme kilit dışında (state kilidi): teşhis okumaları beklemesin.
        let fresh = match (self.factory)() {
            Ok(model) => Arc::new(model),
            Err(error) => {
                self.fail(&format!("yerel model yeniden yüklenemedi: {error}"));
                return None;
            }
        };
        let mut state = self.lock();
        let old = std::mem::replace(&mut state.current, fresh.clone());
        state.stuck.push(Arc::downgrade(&old));
        state.reload_times.push_back(Instant::now());
        state.stats.reloads += 1;
        Some(fresh)
    }

    pub fn stats(&self) -> Stats {
        let mut state = self.lock();
        state.stuck.retain(|weak| weak.strong_count() > 0);
        let mut stats = state.stats.clone();
        stats.stuck_alive = state.stuck.len();
        stats
    }

    pub fn reloads(&self) -> u64 {
        self.lock().stats.reloads
    }
}

/// Model çağrısını yardımcı thread'de başlatır; sonuç kanaldan gelir. Thread kopyanın `Arc`'ını
/// tutar: takılırsa kopya onunla birlikte yaşar, bitince bırakılır.
fn spawn<T: Send + Sync + 'static, R: Send + 'static>(model: Arc<T>, f: Arc<impl Fn(&T) -> R + Send + Sync + 'static>) -> mpsc::Receiver<R> {
    let (send, receive) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("ceviri-yerel-model".into()).spawn(move || {
        let _ = send.send(f(&model));
    });
    if spawned.is_err() {
        // Thread açılamadıysa kanal kapanır; çağıran hata alır.
    }
    receive
}

#[cfg(test)]
mod tests {
    //! Bu testler SAHTE model kullanır (gerçek model testleri `lib.rs`'te).
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// CTranslate2 gibi tek kopya: aynı kopyaya gelen çağrılar sırayla çalışır.
    struct Fake {
        id: usize,
        serial: Mutex<()>,
        /// Bu kopyada ilk çağrı bu kadar takılır.
        hang: Option<Duration>,
        calls: AtomicUsize,
    }

    impl Fake {
        fn translate(&self) -> usize {
            let _one_at_a_time = self.serial.lock().unwrap_or_else(PoisonError::into_inner);
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                if let Some(hang) = self.hang {
                    std::thread::sleep(hang);
                }
            }
            self.id
        }
    }

    /// İlk kopya `first_hang` kadar takılır, sonrakiler takılmaz (ya da `all_hang`).
    fn reloading(created: Arc<AtomicUsize>, first_hang: Option<Duration>, all_hang: bool, options: Options) -> Reloading<Fake> {
        let factory: Factory<Fake> = Box::new(move || {
            let id = created.fetch_add(1, Ordering::SeqCst);
            let hang = if id == 0 || all_hang { first_hang } else { None };
            Ok(Fake { id, serial: Mutex::new(()), hang, calls: AtomicUsize::new(0) })
        });
        Reloading::new(factory, options).unwrap()
    }

    fn fast_options() -> Options {
        Options { min_budget: Duration::from_millis(100), max_budget: Duration::from_millis(300), hard_limit: Duration::from_millis(400), ..Options::default() }
    }

    #[test]
    fn takilan_cagri_ayni_istekte_taze_kopyayla_cevrilir() {
        let created = Arc::new(AtomicUsize::new(0));
        let model = reloading(created.clone(), Some(Duration::from_secs(2)), false, fast_options());
        let started = Instant::now();

        let result = model.call(10.0, Fake::translate);

        assert_eq!(result, Ok(1), "taze kopyanın sonucu");
        assert!(started.elapsed() < Duration::from_secs(1), "takılmayı beklememeli: {:?}", started.elapsed());
        let stats = model.stats();
        assert_eq!((stats.reloads, stats.stuck, stats.stuck_alive), (1, 1, 1));
        // Sonraki çağrılar taze kopyayla, yeniden yükleme olmadan.
        assert_eq!(model.call(10.0, Fake::translate), Ok(1));
        assert_eq!(created.load(Ordering::SeqCst), 2);
        // Takılı çağrı bitince eski kopya bırakılır.
        std::thread::sleep(Duration::from_millis(2100));
        assert_eq!(model.stats().stuck_alive, 0);
    }

    #[test]
    fn takili_kopya_siniri_doluysa_yeni_kopya_yuklenmez_ve_hata_doner() {
        let created = Arc::new(AtomicUsize::new(0));
        // Her kopyanın ilk çağrısı takılır: ilk takılmada yeniden yükleme olur, taze kopya da
        // takılınca sınır (1 takılı kopya) dolu olduğundan yeni kopya yüklenmez.
        let model = reloading(created.clone(), Some(Duration::from_secs(3)), true, fast_options());

        let first = model.call(10.0, Fake::translate);
        let second = model.call(10.0, Fake::translate);

        assert!(first.is_err(), "taze kopya da takıldı: {first:?}");
        assert!(second.is_err(), "{second:?}");
        let stats = model.stats();
        assert!(created.load(Ordering::SeqCst) <= 2, "en çok bir taze kopya yüklenmeli: {}", created.load(Ordering::SeqCst));
        assert!(stats.refused_reloads >= 1);
        assert!(stats.last_error.is_some());
    }

    #[test]
    fn pencerede_yeniden_yukleme_sayisi_sinirlidir() {
        let created = Arc::new(AtomicUsize::new(0));
        let options = Options { max_stuck: 10, max_reloads: 2, ..fast_options() };
        // Takılma, yeniden deneme penceresinden (2 × 300 ms) belirgin şekilde uzun: sonuç zamanlamaya bağlı değil.
        let model = reloading(created.clone(), Some(Duration::from_secs(2)), true, options);
        for _ in 0..4 {
            let _ = model.call(10.0, Fake::translate);
        }
        assert_eq!(model.stats().reloads, 2);
    }

    #[test]
    fn yavas_ama_saglikli_model_takilma_sayilmaz() {
        // Her çağrı 250 ms (yavaş bilgisayar), en küçük bütçe 100 ms: ilk (ölçümsüz) çağrı en büyük
        // bütçeyle yapılır ve süreyi öğretir; sonrakiler öğrenilen süreyle takılma sayılmaz.
        let factory: Factory<Duration> = Box::new(|| Ok(Duration::from_millis(250)));
        let options = Options { max_budget: Duration::from_secs(4), ..fast_options() };
        let model = Reloading::new(factory, options).unwrap();
        assert_eq!(model.budget(50.0), Duration::from_secs(4), "ölçüm yokken en büyük bütçe");
        for _ in 0..5 {
            model.call(50.0, |d: &Duration| std::thread::sleep(*d)).unwrap();
        }
        assert_eq!((model.stats().stuck, model.stats().reloads), (0, 0));
        // Öğrenilen bütçe ≈ 8 × 250 ms; en büyük bütçeden küçük (ölçüm gerçekten kullanılıyor).
        let budget = model.budget(50.0);
        assert!(budget >= Duration::from_millis(1500) && budget < Duration::from_millis(3500), "{budget:?}");
    }
}
