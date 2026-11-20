//! Kendiliğinden toparlanma: ekran okumanın (yakalama + OCR) sağlık durumu ve okuyucu yenilemenin
//! tek koordinatörü. Saf mantıktır (zaman dışarıdan verilir), testlerle sabitlenir; iş hattı
//! olayları bildirir ve "şimdi yenile" kararını buradan alır.
//!
//! Durumlar: `Initializing → WaitingForSource ⇄ Running`, arıza: `Running → Degraded → Recovering →
//! Verifying → Running`; çok kez başarısız olursa `Unavailable` (yine de seyrek denenir). Yasadışı
//! geçiş reddedilir ve sayılır.
//!
//! Fırtına önleme: aynı anda tek toparlanma (kilit), denemeler arası artan bekleme (taban × 2ⁿ,
//! en çok `max_wait`), pencere başına en çok `max_restarts` deneme, başarı ölçütü (yenilemeden sonra
//! `verify_reads` başarılı okuma). Pencerenin kapalı olması arıza sayılmaz.

use std::collections::VecDeque;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SystemState {
    #[default]
    Initializing,
    /// Remote Play (ya da oyun) penceresi yok; açılması bekleniyor.
    WaitingForSource,
    Running,
    /// Okuma başarısız; yenileme zamanı bekleniyor.
    Degraded,
    /// Okuyucu yeniden kuruluyor.
    Recovering,
    /// Yenilendi; başarılı okumayla doğrulanıyor.
    Verifying,
    /// Defalarca denendi, düzelmedi: kullanıcı bakmalı (seyrek denemeye devam edilir).
    Unavailable,
}

impl SystemState {
    pub fn label(self) -> &'static str {
        match self {
            SystemState::Initializing => "başlıyor",
            SystemState::WaitingForSource => "pencere bekleniyor",
            SystemState::Running => "çalışıyor",
            SystemState::Degraded => "sorunlu",
            SystemState::Recovering => "toparlanıyor",
            SystemState::Verifying => "doğrulanıyor",
            SystemState::Unavailable => "kullanılamıyor",
        }
    }

    /// `self`'ten `to`'ya geçiş yasal mı.
    pub fn can_go(self, to: SystemState) -> bool {
        use SystemState::*;
        if self == to {
            return true;
        }
        matches!(
            (self, to),
            (Initializing, WaitingForSource | Running | Degraded)
                | (WaitingForSource, Running | Degraded)
                | (Running, Degraded | WaitingForSource | Recovering)
                | (Degraded, Running | Recovering | WaitingForSource)
                | (Recovering, Verifying | Degraded | Unavailable)
                | (Verifying, Running | Degraded | WaitingForSource | Recovering)
                | (Unavailable, Recovering | Running | WaitingForSource)
        )
    }
}

/// Arızanın yeri.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Component {
    #[default]
    Capture,
    Ocr,
}

#[derive(Clone, Debug)]
pub struct Options {
    /// İlk yenilemeden önce okumanın en az bu kadar süre başarısız olması gerekir (sn).
    pub restart_after: f64,
    /// Denemeler arası en uzun bekleme (sn).
    pub max_wait: f64,
    /// `window` saniyede en çok bu kadar yenileme.
    pub max_restarts: usize,
    pub window: f64,
    /// Yenilemeden sonra bu kadar başarılı okuma "düzeldi" sayılır.
    pub verify_reads: u32,
    /// Art arda bu kadar başarısız yenilemeden sonra `Unavailable`.
    pub unavailable_after: u32,
}

impl Default for Options {
    fn default() -> Options {
        Options { restart_after: 20.0, max_wait: 300.0, max_restarts: 6, window: 600.0, verify_reads: 2, unavailable_after: 5 }
    }
}

/// Teşhis için anlık durum.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryStatus {
    pub state: SystemState,
    /// Son arızanın/yenilemenin sebebi.
    pub reason: Option<String>,
    pub component: Component,
    /// Doğrulanmamış art arda yenileme sayısı.
    pub attempts: u32,
    pub restarts: u64,
    /// Doğrulanmış (başarılı) toparlanma sayısı.
    pub recoveries: u64,
    /// Bir sonraki yenilemeye kalan süre (arıza sürüyorsa).
    pub next_attempt_in: Option<f64>,
    pub last_recovered_ago: Option<f64>,
    /// Reddedilen yasadışı geçiş sayısı (0 olmalı).
    pub illegal_transitions: u64,
}

#[derive(Clone, Debug)]
pub struct RecoveryCoordinator {
    options: Options,
    state: SystemState,
    failing_since: Option<f64>,
    attempts: u32,
    in_progress: bool,
    verified_reads: u32,
    history: VecDeque<f64>,
    reason: Option<String>,
    component: Component,
    restarts: u64,
    recoveries: u64,
    last_recovered: Option<f64>,
    illegal: u64,
}

impl RecoveryCoordinator {
    pub fn new(options: Options) -> RecoveryCoordinator {
        RecoveryCoordinator {
            options,
            state: SystemState::Initializing,
            failing_since: None,
            attempts: 0,
            in_progress: false,
            verified_reads: 0,
            history: VecDeque::new(),
            reason: None,
            component: Component::Capture,
            restarts: 0,
            recoveries: 0,
            last_recovered: None,
            illegal: 0,
        }
    }

    pub fn state(&self) -> SystemState {
        self.state
    }

    fn go(&mut self, to: SystemState) -> bool {
        if self.state.can_go(to) {
            self.state = to;
            true
        } else {
            self.illegal += 1;
            false
        }
    }

    /// Başarılı okuma. Bir yenileme doğrulandıysa açıklama döner.
    pub fn on_read_ok(&mut self, now: f64) -> Option<String> {
        self.failing_since = None;
        match self.state {
            SystemState::Verifying => {
                self.verified_reads += 1;
                if self.verified_reads >= self.options.verify_reads {
                    self.go(SystemState::Running);
                    self.attempts = 0;
                    self.recoveries += 1;
                    self.last_recovered = Some(now);
                    return Some(format!("Ekran okuma toparlandı ({}).", self.reason.as_deref().unwrap_or("yenileme")));
                }
            }
            SystemState::Recovering => {}
            _ => {
                self.go(SystemState::Running);
                self.attempts = 0;
            }
        }
        None
    }

    /// Başarısız okuma (pencere yokluğu hariç).
    pub fn on_read_error(&mut self, now: f64, component: Component, message: &str) {
        self.failing_since.get_or_insert(now);
        self.component = component;
        if !self.in_progress {
            self.reason = Some(message.chars().take(160).collect());
        }
        if matches!(self.state, SystemState::Initializing | SystemState::Running | SystemState::WaitingForSource | SystemState::Verifying) {
            self.go(SystemState::Degraded);
        }
    }

    /// Hedef pencere yok: arıza değil, beklenir.
    pub fn on_source_missing(&mut self) {
        self.failing_since = None;
        if !self.in_progress && self.state != SystemState::Unavailable {
            self.go(SystemState::WaitingForSource);
        }
    }

    /// Bu deneme için gereken arıza süresi.
    pub fn wait(&self) -> f64 {
        (self.options.restart_after * 2f64.powi(self.attempts.min(8) as i32)).min(self.options.max_wait)
    }

    fn frequency_ok(&mut self, now: f64) -> bool {
        let window = self.options.window;
        self.history.retain(|t| now - t < window);
        self.history.len() < self.options.max_restarts
    }

    /// Okuyucu şimdi yenilenmeli mi; öyleyse yenileme başlar (`finish` ile bitirilmelidir).
    pub fn begin(&mut self, now: f64) -> Option<String> {
        if self.in_progress || !matches!(self.state, SystemState::Degraded | SystemState::Unavailable) {
            return None;
        }
        let failing = now - self.failing_since?;
        if failing < self.wait() || !self.frequency_ok(now) {
            return None;
        }
        let reason = format!("{} {failing:.0} sn'dir çalışmıyor", match self.component {
            Component::Capture => "Ekran okuma",
            Component::Ocr => "Yazı tanıma",
        });
        self.start(now, reason.clone());
        Some(reason)
    }

    /// Bekleme süresine bakmadan yenile (uykudan uyanma). Başka bir yenileme sürüyorsa `false`.
    pub fn force(&mut self, now: f64, reason: &str) -> bool {
        if self.in_progress {
            return false;
        }
        if !self.state.can_go(SystemState::Recovering) {
            // Pencere bekleniyordu ya da yeni başlıyordu: önce "arıza" sayılır.
            self.go(SystemState::Degraded);
        }
        self.start(now, reason.to_string());
        true
    }

    fn start(&mut self, now: f64, reason: String) {
        self.go(SystemState::Recovering);
        self.in_progress = true;
        self.attempts += 1;
        self.restarts += 1;
        self.history.push_back(now);
        self.reason = Some(reason);
        // Sonraki deneme bu andan itibaren (iki katı süre sonra).
        self.failing_since = Some(now);
    }

    /// Yenileme bitti: okuyucu kurulabildi mi.
    pub fn finish(&mut self, now: f64, ok: bool) {
        if !self.in_progress {
            return;
        }
        self.in_progress = false;
        if ok {
            self.verified_reads = 0;
            self.go(SystemState::Verifying);
        } else if self.attempts >= self.options.unavailable_after {
            self.go(SystemState::Unavailable);
        } else {
            self.go(SystemState::Degraded);
        }
        self.failing_since.get_or_insert(now);
    }

    pub fn status(&self, now: f64) -> RecoveryStatus {
        let next_attempt_in = match self.state {
            SystemState::Degraded | SystemState::Unavailable => self.failing_since.map(|since| (self.wait() - (now - since)).max(0.0)),
            _ => None,
        };
        RecoveryStatus {
            state: self.state,
            reason: self.reason.clone(),
            component: self.component,
            attempts: self.attempts,
            restarts: self.restarts,
            recoveries: self.recoveries,
            next_attempt_in,
            last_recovered_ago: self.last_recovered.map(|t| now - t),
            illegal_transitions: self.illegal,
        }
    }

    /// Okuma kaç saniyedir başarısız (yoksa `None`).
    pub fn failing_for(&self, now: f64) -> Option<f64> {
        self.failing_since.map(|since| now - since)
    }
}

/// Uyku/uyanma algılayıcı: macOS'ta monoton saat (`Instant`) uykuda ilerlemez, duvar saati
/// ilerler; iki tur arasında duvar saati monoton saatten belirgin fazla ilerlediyse sistem uyumuştur.
/// Monoton saatin uykuda da ilerlediği sistemler için çok uzun tur boşluğu da uyku sayılır.
#[derive(Clone, Debug)]
pub struct SleepDetector {
    last: Option<(f64, f64)>,
    /// Duvar − monoton farkı bundan büyükse uyku (sn).
    pub threshold: f64,
    /// İki tur arası monoton boşluk bundan büyükse de uyku (sn); okuma en çok 60 sn bloklar.
    pub long_gap: f64,
}

impl Default for SleepDetector {
    fn default() -> SleepDetector {
        SleepDetector { last: None, threshold: 5.0, long_gap: 120.0 }
    }
}

impl SleepDetector {
    /// Bir tur başında çağrılır; uyku algılandıysa uyunan süre (sn).
    pub fn observe(&mut self, wall: f64, mono: f64) -> Option<f64> {
        let previous = self.last.replace((wall, mono));
        let (last_wall, last_mono) = previous?;
        let (wall_gap, mono_gap) = (wall - last_wall, mono - last_mono);
        if wall_gap - mono_gap > self.threshold {
            Some(wall_gap - mono_gap)
        } else if mono_gap > self.long_gap {
            Some(mono_gap)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coordinator(restart_after: f64) -> RecoveryCoordinator {
        RecoveryCoordinator::new(Options { restart_after, ..Options::default() })
    }

    #[test]
    fn saglikli_okumada_calisiyor() {
        let mut c = coordinator(20.0);
        assert_eq!(c.state(), SystemState::Initializing);
        c.on_read_ok(0.0);
        assert_eq!(c.state(), SystemState::Running);
        assert_eq!(c.begin(100.0), None);
    }

    #[test]
    fn ariza_suresi_dolunca_bir_kez_yenilenir_ve_dogrulanir() {
        let mut c = coordinator(20.0);
        c.on_read_ok(0.0);
        c.on_read_error(1.0, Component::Capture, "akış durdu");
        assert_eq!(c.state(), SystemState::Degraded);
        assert_eq!(c.begin(10.0), None, "süre dolmadan yenilenmez");

        let reason = c.begin(21.0).expect("yenilenmeli");
        assert!(reason.contains("Ekran okuma"), "{reason}");
        assert_eq!(c.state(), SystemState::Recovering);
        // Kilit: yenileme sürerken ikinci yenileme yok (toparlanma üst üste binmez).
        assert_eq!(c.begin(100.0), None);
        assert!(!c.force(100.0, "uyku"));

        c.finish(21.5, true);
        assert_eq!(c.state(), SystemState::Verifying);
        assert_eq!(c.on_read_ok(22.0), None);
        assert!(c.on_read_ok(22.2).is_some(), "ikinci başarılı okumada toparlandı");
        let status = c.status(23.0);
        assert_eq!((status.state, status.recoveries, status.attempts, status.illegal_transitions), (SystemState::Running, 1, 0, 0));
    }

    #[test]
    fn basarisiz_yenilemede_bekleme_ikiye_katlanir() {
        let mut c = coordinator(1.0);
        c.on_read_error(0.0, Component::Ocr, "OCR takıldı");
        assert!(c.begin(1.0).is_some());
        c.finish(1.0, true);
        // Yeni okuyucu da çalışmıyor: doğrulama başarısız.
        c.on_read_error(1.1, Component::Ocr, "OCR takıldı");
        assert_eq!(c.state(), SystemState::Degraded);
        assert_eq!(c.begin(2.5), None, "ikinci deneme 2 sn sonra");
        assert!(c.begin(3.0).is_some());
        c.finish(3.0, false);
        assert_eq!(c.begin(6.0), None, "üçüncü deneme 4 sn sonra");
        assert!(c.begin(7.0).is_some());
        assert_eq!(c.status(7.0).component, Component::Ocr);
    }

    #[test]
    fn yenileme_firtinasi_engellenir() {
        let options = Options { restart_after: 0.0, max_restarts: 3, window: 60.0, unavailable_after: 100, ..Options::default() };
        let mut c = RecoveryCoordinator::new(options);
        c.on_read_error(0.0, Component::Capture, "x");
        let mut restarts = 0;
        for i in 0..100 {
            let now = i as f64 * 0.1;
            if c.begin(now).is_some() {
                restarts += 1;
                c.finish(now, false);
            }
        }
        assert_eq!(restarts, 3, "pencere başına en çok 3 yenileme");
        // Pencere geçince yeniden denenir.
        assert!(c.begin(70.0).is_some());
    }

    #[test]
    fn surekli_basarisizlikta_kullanilamaz_olur_ama_denemeye_devam_eder() {
        let mut c = RecoveryCoordinator::new(Options { restart_after: 1.0, max_wait: 2.0, unavailable_after: 2, max_restarts: 100, ..Options::default() });
        c.on_read_error(0.0, Component::Capture, "x");
        let mut now = 0.0;
        for _ in 0..2 {
            now += 3.0;
            assert!(c.begin(now).is_some());
            c.finish(now, false);
        }
        assert_eq!(c.state(), SystemState::Unavailable);
        assert!(c.begin(now + 1.0).is_none());
        assert!(c.begin(now + 2.0).is_some(), "en uzun beklemeyle denemeye devam");
        c.finish(now + 2.0, true);
        c.on_read_ok(now + 3.0);
        assert!(c.on_read_ok(now + 3.2).is_some());
        assert_eq!(c.state(), SystemState::Running);
    }

    #[test]
    fn pencere_yoklugu_ariza_degildir() {
        let mut c = coordinator(1.0);
        c.on_read_error(0.0, Component::Capture, "x");
        c.on_source_missing();
        assert_eq!(c.state(), SystemState::WaitingForSource);
        assert_eq!(c.begin(100.0), None);
        c.on_read_ok(101.0);
        assert_eq!(c.state(), SystemState::Running);
    }

    #[test]
    fn uyanmada_zorla_yenilenir() {
        let mut c = coordinator(20.0);
        c.on_read_ok(0.0);
        assert!(c.force(5.0, "uykudan uyanıldı"));
        assert_eq!(c.state(), SystemState::Recovering);
        assert_eq!(c.status(5.0).reason.as_deref(), Some("uykudan uyanıldı"));
        c.finish(5.0, true);
        assert_eq!(c.state(), SystemState::Verifying);
        // Pencere bekleniyorken de zorlanabilir (Degraded üzerinden, yasadışı geçiş yok).
        let mut waiting = coordinator(20.0);
        waiting.on_source_missing();
        assert!(waiting.force(1.0, "uyku"));
        assert_eq!(waiting.status(1.0).illegal_transitions, 0);
    }

    #[test]
    fn yasadisi_gecis_reddedilir() {
        assert!(!SystemState::Running.can_go(SystemState::Verifying));
        assert!(!SystemState::WaitingForSource.can_go(SystemState::Unavailable));
        assert!(!SystemState::Initializing.can_go(SystemState::Recovering));
        assert!(SystemState::Recovering.can_go(SystemState::Verifying));
        let mut c = coordinator(1.0);
        c.finish(0.0, true);
        assert_eq!(c.state(), SystemState::Initializing, "yenileme yokken bitir çağrısı etkisiz");
        assert!(!c.go(SystemState::Verifying));
        assert_eq!(c.status(0.0).illegal_transitions, 1);
    }

    #[test]
    fn uyku_saat_farkindan_algilanir() {
        let mut d = SleepDetector::default();
        assert_eq!(d.observe(1000.0, 10.0), None);
        assert_eq!(d.observe(1000.2, 10.2), None);
        // 2 saat uyku: duvar saati ilerledi, monoton saat ilerlemedi.
        let slept = d.observe(8200.4, 10.4).unwrap();
        assert!((slept - 7200.0).abs() < 1.0);
        // Uzun tur boşluğu (monoton saat uykuda da ilerleyen sistem).
        assert!(d.observe(8400.0, 210.0).is_some());
        // Saat geri alındı: uyku değil.
        assert_eq!(d.observe(8000.0, 210.2), None);
    }
}
