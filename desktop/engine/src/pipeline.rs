//! Ana döngü (Swift `ps5ceviri run` karşılığı): oku → temizle → birleştir → sabitle → Türkçeyi
//! ayıkla → çevir (önbellek + arka planda çeviri işçileri) → TV'ye gönder. Durum ekrana yazılmaz,
//! `Event` olarak bildirilir; komut satırı da masaüstü arayüzü de aynı döngüyü kullanır.
//!
//! Uzun süre (saatlerce) kesintisiz çalışması için:
//! - Bir turdaki beklenmeyen hata (panik) yalnızca o turu atlatır; döngü durmaz.
//! - Okuma üst üste `reader_restart_after` saniye başarısız olursa okuyucu (yakalama akışı + OCR)
//!   baştan kurulur; denemeler arası süre her seferinde ikiye katlanır (en çok 5 dk).
//! - Çeviri ayrı işçilerde yapılır (`translation`); çeviri takılsa ya da hata verse de okuma ve
//!   TV'ye gönderim sürer. Takılan işçi yenilenir.
//! - Okuma bir anlık başarısız olursa son çeviriler kısa süre ekranda tutulur (titreme olmasın).
//! - Saniyede bir `Event::Health` ile her aşamanın durumu bildirilir (teşhis paneli).

use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ceviri_core::{block_merger, focus, glossary, language, ocr_text, terms, Diagnostics, OverlayBlock, ReadError, Reading, Rect, ScreenReader, StableBlock, Stabilizer, Terms, TextLine, TranslationError};

use crate::sysinfo;
pub use crate::translation::Backend;
use crate::translation::{self, panic_message, TranslationService};
use crate::tv_client::TvClient;

/// Okuyucuyu baştan kurar (takılan yakalama akışından kurtulmak için).
pub type ReaderFactory = Box<dyn FnMut() -> Result<Box<dyn ScreenReader>, String> + Send>;

pub struct Config {
    /// Saniyedeki tur; TV 3 sn mesajsız kalırsa temizlediği için en az 1.
    pub fps: f64,
    /// `None`: çevirisiz (dry-run), İngilizce metin olduğu gibi gönderilir.
    pub backend: Option<Backend>,
    /// Başlangıç önbelleği ve çevirilerin (aralıklarla) yazılacağı dosya.
    pub cache: std::collections::HashMap<String, String>,
    pub cache_path: Option<PathBuf>,
    /// Her turda blok kutuları çizilmiş kareyi buraya kaydeder.
    pub dump_dir: Option<PathBuf>,
    /// Tek tur çalışıp ekranı temizler (hizalama testi).
    pub once: bool,
    /// Çevrilmeyecek adlar (oyun, karakter, marka).
    pub terms: Terms,
    /// Yalnızca altyazı ve menü çevrilir; logo, tabela gibi oyun dünyası yazıları atlanır.
    pub focus: bool,
    /// Altyazı çevirisi orijinal yazının üstüne değil hemen altına konur (ikisi de okunur).
    pub below: bool,
    /// Çeviri işçileri (sayı, takılma süresi, önbellek kayıt aralığı).
    pub translation: translation::Options,
    /// Okuma bu kadar saniye üst üste başarısızsa okuyucu yeniden kurulur (fabrika verildiyse).
    pub reader_restart_after: f64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            fps: 5.0,
            backend: None,
            cache: Default::default(),
            cache_path: None,
            dump_dir: None,
            once: false,
            terms: Terms::new(&[]),
            focus: false,
            below: false,
            translation: translation::Options::default(),
            reader_restart_after: 20.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RoundStats {
    pub round: u64,
    pub video_width: u32,
    pub video_height: u32,
    pub capture_ms: u64,
    pub ocr_ms: u64,
    pub merged: usize,
    pub stable: usize,
    pub sent: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Round(RoundStats),
    /// Hedef pencere kayboldu; açılması bekleniyor.
    WaitingForWindow(String),
    WindowFound,
    /// Gösterilen liste değişti (TV'ye gittiyse ya da TV yoksa).
    Shown(Vec<String>),
    /// Her tur: o an gösterilmesi gereken bloklar ve video alanının ekrandaki yeri (ekran
    /// overlay'i için; pencere yoksa boş liste ve `None`).
    Frame { blocks: Vec<OverlayBlock>, screen: Option<Rect> },
    TvConnected,
    TvUnreachable(String),
    TranslationFailed(TranslationError),
    RoundSkipped(String),
    /// Saniyede bir: her aşamanın durumu (teşhis).
    Health(Box<Diagnostics>),
    /// Kendiliğinden toparlanma (tur hatası atlandı, okuyucu/çeviri işçisi yenilendi).
    Recovered(String),
}

/// Okuma bir anlık başarısız olursa son çeviriler bu kadar süre ekranda tutulur.
const HOLD_ON_READ_ERROR: Duration = Duration::from_millis(1500);
/// Aynı hata mesajı arayüze en sık bu aralıkla bildirilir.
const REPEAT_ERROR_EVERY: Duration = Duration::from_secs(10);
/// Okuyucu yenilemeleri arasındaki en uzun bekleme.
const MAX_READER_RESTART_WAIT: f64 = 300.0;

pub struct Pipeline {
    reader: Box<dyn ScreenReader>,
    reader_factory: Option<ReaderFactory>,
    /// `None`: yalnızca ekran overlay'i (TV'ye gönderilmez).
    tv: Option<TvClient>,
    config: Config,
    stop: Arc<AtomicBool>,
}

impl Pipeline {
    pub fn new(reader: Box<dyn ScreenReader>, tv: Option<TvClient>, config: Config) -> Pipeline {
        Pipeline { reader, reader_factory: None, tv, config, stop: Arc::new(AtomicBool::new(false)) }
    }

    /// Döngüyü dışarıdaki bir bayrağa bağlar (döngü kurulmadan önce oluşturulan "Durdur" düğmesi gibi).
    pub fn with_stop(mut self, stop: Arc<AtomicBool>) -> Pipeline {
        self.stop = stop;
        self
    }

    /// Okuma uzun süre başarısız kalırsa okuyucuyu baştan kurmak için.
    pub fn with_reader_factory(mut self, factory: ReaderFactory) -> Pipeline {
        self.reader_factory = Some(factory);
        self
    }

    /// Başka bir thread'den `store(true)` ile döngü durdurulur; çıkarken TV ekranı temizlenir.
    pub fn stop_handle(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }

    /// Durdurulana (ya da `once` ise bir tur) kadar çalışır.
    pub fn run(&mut self, mut on_event: impl FnMut(Event)) {
        let fps = self.config.fps.max(1.0);
        // Arka plandaki çeviri bitince haber verir: sonuç bir sonraki turu beklemeden gösterilir.
        let (ready_tx, ready): (mpsc::Sender<()>, Receiver<()>) = mpsc::channel();
        let service = self.config.backend.clone().map(|backend| {
            TranslationService::start(backend, std::mem::take(&mut self.config.cache), self.config.cache_path.clone(), self.config.translation.clone(), ready_tx)
        });
        let mut state = RunState::new();
        let mut delivery = Delivery::default();
        let mut round = 0u64;

        while !self.stop.load(Ordering::Relaxed) {
            round += 1;
            let round_start = Instant::now();
            // Bu turdan önce biten çeviriler zaten bu turda görülecek.
            ready.try_iter().for_each(drop);
            if let Some(service) = &service {
                for message in service.maintain() {
                    state.recoveries += 1;
                    on_event(Event::Recovered(message));
                }
                state.report_translation_errors(service, &mut on_event);
            }

            let reader = &mut *self.reader;
            let config = &self.config;
            let outcome = catch_unwind(AssertUnwindSafe(|| state.round(reader, config, service.as_ref(), round, &mut on_event)));
            let (blocks, screen) = match outcome {
                Ok(composed) => composed,
                Err(panic) => {
                    state.recover_from_panic();
                    on_event(Event::Recovered(format!("Bir tur atlandı ({}); çeviri devam ediyor.", panic_message(&panic))));
                    (Vec::new(), None)
                }
            };
            self.restart_reader_if_stuck(&mut state, &mut on_event);

            // Liste değişmese de her tur gönderilir: TV 3 sn mesaj almazsa yazıları kaldırır.
            state.shown = blocks.len();
            let sent = delivery.send(&mut self.tv, blocks, screen, &mut on_event);
            state.round_ms = round_start.elapsed().as_millis() as u64;
            state.emit_health(service.as_ref(), &delivery, &mut on_event);

            if self.config.once {
                std::thread::sleep(Duration::from_secs(2));
                break;
            }
            let pause = if sent { Duration::from_secs_f64(1.0 / fps).saturating_sub(round_start.elapsed()) } else { Duration::from_secs(2) };
            let deadline = Instant::now() + pause;
            while !self.stop.load(Ordering::Relaxed) {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                // Durdurma isteğine hızlı cevap verebilmek için kısa parçalar halinde beklenir.
                if ready.recv_timeout(remaining.min(Duration::from_millis(50))).is_ok() && sent && !state.stable.is_empty() {
                    // Çeviri geldi: ekranı yeniden okumadan, son sabit bloklarla hemen gönder.
                    let composed = catch_unwind(AssertUnwindSafe(|| compose(&self.config, service.as_ref(), &state.stable, &state.below).0));
                    if let Ok(blocks) = composed {
                        state.shown = blocks.len();
                        delivery.send(&mut self.tv, blocks, state.screen, &mut on_event);
                    }
                }
            }
        }
        if let Some(service) = &service {
            service.shutdown();
        }
        if let Some(tv) = self.tv.as_mut() {
            tv.close();
        }
        on_event(Event::Frame { blocks: Vec::new(), screen: None });
    }

    /// Okuma uzun süredir başarısızsa okuyucuyu yeniden kurar. Eski okuyucu ayrı thread'de
    /// kapatılır (takılı bir akışın kapanması döngüyü bekletmesin).
    fn restart_reader_if_stuck(&mut self, state: &mut RunState, on_event: &mut impl FnMut(Event)) {
        let Some(factory) = self.reader_factory.as_mut() else { return };
        let Some(since) = state.failing_since else { return };
        let wait = (self.config.reader_restart_after * 2f64.powi(state.reader_restarts.min(8) as i32)).min(MAX_READER_RESTART_WAIT);
        let failing = since.elapsed().as_secs_f64();
        if failing < wait {
            return;
        }
        state.reader_restarts += 1;
        // Bir sonraki deneme bu andan itibaren (iki katı süre sonra) yapılır.
        state.failing_since = Some(Instant::now());
        match factory() {
            Ok(reader) => {
                let old = std::mem::replace(&mut self.reader, reader);
                let _ = std::thread::Builder::new().name("ceviri-eski-okuyucu".into()).spawn(move || drop(old));
                state.stabilizer = Stabilizer::new();
                state.recoveries += 1;
                on_event(Event::Recovered(format!("Ekran okuma {failing:.0} sn'dir çalışmıyor; okuyucu yeniden kuruldu.")));
            }
            Err(error) => on_event(Event::Recovered(format!("Okuyucu yeniden kurulamadı ({error}); daha sonra tekrar denenecek."))),
        }
    }
}

/// Bir çalıştırma boyunca tutulan durum: sabitleyici, son bloklar ve teşhis ölçümleri.
struct RunState {
    started: Instant,
    stabilizer: Stabilizer,
    /// Son sabit bloklar (çeviri gelince yeniden göndermek için) ve video alanının yeri.
    stable: Vec<StableBlock>,
    /// Çevirisi yazının altına konacak blokların (altyazıların) kimlikleri.
    below: HashSet<String>,
    screen: Option<Rect>,
    waiting_for_window: bool,

    last_ok_read: Option<Instant>,
    /// Okuma başarısız olmaya başladığı an (pencere yokluğu sayılmaz); başarılı okumada `None`.
    failing_since: Option<Instant>,
    read_errors: u64,
    last_read_error: Option<String>,
    ocr_failing: bool,
    reader_restarts: u32,
    last_reported: Option<(String, Instant)>,

    frame_seq: Option<u64>,
    seq_changed: Instant,
    frame_age_ms: Option<u64>,
    stream_restarts: u64,
    capture_ms: u64,
    ocr_ms: u64,
    lines: usize,
    merged: usize,
    /// Sabitlenmiş blok sayısı (ayıklamadan önce) ve ayıklananlar (odak dışı, Türkçe, ad, sayı).
    stable_total: usize,
    filtered: usize,
    shown: usize,
    round_ms: u64,
    recoveries: u64,

    /// Teşhis: son bildirim anı ve o andaki sayaçlar (saniyedeki hız için).
    last_health: Instant,
    reads_since_health: u64,
    seq_at_health: Option<u64>,
    memory: Option<(Instant, Option<f64>)>,
    translation_errors_seen: u64,
    last_translation_report: Option<(TranslationError, Instant)>,
}

impl RunState {
    fn new() -> RunState {
        let now = Instant::now();
        RunState {
            started: now,
            stabilizer: Stabilizer::new(),
            stable: Vec::new(),
            below: HashSet::new(),
            screen: None,
            waiting_for_window: false,
            last_ok_read: None,
            failing_since: None,
            read_errors: 0,
            last_read_error: None,
            ocr_failing: false,
            reader_restarts: 0,
            last_reported: None,
            frame_seq: None,
            seq_changed: now,
            frame_age_ms: None,
            stream_restarts: 0,
            capture_ms: 0,
            ocr_ms: 0,
            lines: 0,
            merged: 0,
            stable_total: 0,
            filtered: 0,
            shown: 0,
            round_ms: 0,
            recoveries: 0,
            last_health: now,
            reads_since_health: 0,
            seq_at_health: None,
            memory: None,
            translation_errors_seen: 0,
            last_translation_report: None,
        }
    }

    fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Bir tur: oku, işle, gösterilecek blokları kur.
    fn round(
        &mut self,
        reader: &mut dyn ScreenReader,
        config: &Config,
        service: Option<&TranslationService>,
        round: u64,
        on_event: &mut impl FnMut(Event),
    ) -> (Vec<OverlayBlock>, Option<Rect>) {
        match reader.read() {
            Ok(reading) => {
                self.on_read_ok(&reading, on_event);
                let merged = self.process(&reading, config);
                let (blocks, filtered) = compose(config, service, &self.stable, &self.below);
                self.filtered += filtered;
                on_event(Event::Round(RoundStats {
                    round,
                    video_width: reading.video_width,
                    video_height: reading.video_height,
                    capture_ms: reading.capture_ms,
                    ocr_ms: reading.ocr_ms,
                    merged: merged.len(),
                    stable: self.stable.len(),
                    sent: blocks.len(),
                }));
                if let Some(dir) = &config.dump_dir {
                    let boxes: Vec<_> = merged.iter().map(|m| m.bbox).collect();
                    if let Err(error) = reader.dump(&dir.join(format!("ocr_{round:04}.png")), &boxes) {
                        on_event(Event::RoundSkipped(format!("kare kaydedilemedi: {error}")));
                    }
                }
                (blocks, self.screen)
            }
            Err(ReadError::WindowNotFound(message)) => {
                if !self.waiting_for_window {
                    on_event(Event::WaitingForWindow(message));
                }
                self.waiting_for_window = true;
                // Pencere yokluğu bir arıza değil: okuyucu yenilenmez.
                self.failing_since = None;
                self.stabilizer = Stabilizer::new();
                self.stable.clear();
                self.screen = None;
                (Vec::new(), None)
            }
            Err(error) => {
                self.on_read_error(&error, on_event);
                // Bir anlık hata: son çeviriler kısa süre ekranda kalır.
                let recent = self.last_ok_read.is_some_and(|t| t.elapsed() < HOLD_ON_READ_ERROR);
                if recent && !self.stable.is_empty() {
                    (compose(config, service, &self.stable, &self.below).0, self.screen)
                } else {
                    (Vec::new(), self.screen)
                }
            }
        }
    }

    fn on_read_ok(&mut self, reading: &Reading, on_event: &mut impl FnMut(Event)) {
        if self.waiting_for_window {
            on_event(Event::WindowFound);
        }
        self.waiting_for_window = false;
        self.last_ok_read = Some(Instant::now());
        self.failing_since = None;
        self.ocr_failing = false;
        self.reader_restarts = 0;
        self.reads_since_health += 1;
        self.screen = reading.screen;
        self.capture_ms = reading.capture_ms;
        self.ocr_ms = reading.ocr_ms;
        self.frame_age_ms = reading.frame.age_ms;
        self.stream_restarts = reading.frame.restarts;
        if reading.frame.seq != self.frame_seq || reading.frame.seq.is_none() {
            self.seq_changed = Instant::now();
        }
        self.frame_seq = reading.frame.seq;
    }

    fn on_read_error(&mut self, error: &ReadError, on_event: &mut impl FnMut(Event)) {
        self.read_errors += 1;
        self.failing_since.get_or_insert_with(Instant::now);
        self.ocr_failing = matches!(error, ReadError::Ocr(_));
        let message = error.to_string();
        self.last_read_error = Some(message.clone());
        // Akış açılışı ve önceki okumanın sürmesi olağan; arayüze yalnızca sürerse bildirilir.
        let ordinary = matches!(error, ReadError::NoFrameYet | ReadError::Busy) && self.failing_since.is_some_and(|t| t.elapsed() < Duration::from_secs(3));
        let repeated = self.last_reported.as_ref().is_some_and(|(last, at)| *last == message && at.elapsed() < REPEAT_ERROR_EVERY);
        if !ordinary && !repeated {
            self.last_reported = Some((message.clone(), Instant::now()));
            on_event(Event::RoundSkipped(message));
        }
    }

    /// OCR satırlarını temizler, birleştirir, sabitler ve türüne göre ayıklar.
    fn process(&mut self, reading: &Reading, config: &Config) -> Vec<block_merger::MergedBlock> {
        let lines: Vec<TextLine> = reading
            .lines
            .iter()
            .filter_map(|line| {
                let text = ocr_text::clean(&line.text);
                (!text.is_empty()).then(|| TextLine { text, ..line.clone() })
            })
            .collect();
        self.lines = lines.len();
        // PC oyunlarında alan 16:9 olmayabilir (21:9 monitör, seçilen altyazı bandı).
        let aspect = reading.video_width as f64 / reading.video_height.max(1) as f64;
        let options = block_merger::Options { aspect, ..Default::default() };
        let merged = block_merger::merge_with(&lines, &options);
        self.merged = merged.len();
        let mut stable = self.stabilizer.update(&merged, self.elapsed());
        // Odak modunda tür dışı yazılar hiç ele alınmaz (çevrilmez, üstüne çizilmez).
        let kinds = focus::classify(&stable, &focus::Options { aspect, ..Default::default() });
        let mut kinds = kinds.into_iter();
        let mut below = HashSet::new();
        let before = stable.len();
        stable.retain(|block| {
            let kind = kinds.next().unwrap_or(focus::Kind::Other);
            if kind == focus::Kind::Dialogue {
                below.insert(block.id.clone());
            }
            !config.focus || kind != focus::Kind::Other
        });
        self.stable_total = before;
        self.filtered = before - stable.len();
        self.stable = stable;
        self.below = below;
        merged
    }

    fn recover_from_panic(&mut self) {
        self.recoveries += 1;
        self.stabilizer = Stabilizer::new();
        self.stable.clear();
        self.below.clear();
    }

    /// Yeni çeviri hatalarını bildirir; aynı hata en sık `REPEAT_ERROR_EVERY` aralıkla.
    fn report_translation_errors(&mut self, service: &TranslationService, on_event: &mut impl FnMut(Event)) {
        let stats = service.stats();
        if stats.translator.errors == self.translation_errors_seen {
            return;
        }
        self.translation_errors_seen = stats.translator.errors;
        let Some(error) = stats.translator.last_error else { return };
        let repeated = self.last_translation_report.as_ref().is_some_and(|(last, at)| *last == error && at.elapsed() < REPEAT_ERROR_EVERY);
        if !repeated {
            self.last_translation_report = Some((error.clone(), Instant::now()));
            on_event(Event::TranslationFailed(error));
        }
    }

    fn emit_health(&mut self, service: Option<&TranslationService>, delivery: &Delivery, on_event: &mut impl FnMut(Event)) {
        let since = self.last_health.elapsed().as_secs_f64();
        if since < 1.0 {
            return;
        }
        let memory_due = self.memory.as_ref().is_none_or(|(at, _)| at.elapsed() >= Duration::from_secs(10));
        if memory_due {
            self.memory = Some((Instant::now(), sysinfo::memory_mb()));
        }
        let frames_per_s = match (self.frame_seq, self.seq_at_health) {
            (Some(now), Some(before)) => Some(now.saturating_sub(before) as f64 / since),
            _ => None,
        };
        let mut diagnostics = Diagnostics {
            uptime_s: self.elapsed(),
            waiting_for_window: self.waiting_for_window,
            reads_per_s: self.reads_since_health as f64 / since,
            frames_per_s,
            last_read_ago_s: self.last_ok_read.map(|t| t.elapsed().as_secs_f64()),
            frame_age_ms: self.frame_age_ms,
            frozen_s: self.frame_seq.map(|_| self.seq_changed.elapsed().as_secs_f64()),
            capture_ms: self.capture_ms,
            stream_restarts: self.stream_restarts,
            read_errors: self.read_errors,
            ocr_failing: self.ocr_failing,
            last_read_error: self.failing_since.and(self.last_read_error.clone()),
            ocr_ms: self.ocr_ms,
            last_ocr_ago_s: self.last_ok_read.map(|t| t.elapsed().as_secs_f64()),
            lines: self.lines,
            blocks: self.merged,
            stable: self.stable_total,
            filtered: self.filtered,
            translating_enabled: service.is_some(),
            tv_ok: delivery.tv_ok,
            shown: self.shown,
            last_send_ago_s: delivery.last_ok.map(|t| t.elapsed().as_secs_f64()),
            send_errors: delivery.errors,
            round_ms: self.round_ms,
            latency_ms: self.frame_age_ms.map(|age| age + self.round_ms),
            recoveries: self.recoveries,
            memory_mb: self.memory.as_ref().and_then(|(_, mb)| *mb),
            ..Diagnostics::default()
        };
        if let Some(service) = service {
            let stats = service.stats();
            let now = service.now();
            diagnostics.queue = stats.queue;
            diagnostics.in_flight = stats.in_flight;
            diagnostics.worker = stats.worker.to_string();
            diagnostics.backoff_s = stats.backoff;
            diagnostics.recoveries += stats.recoveries;
            diagnostics.cache_hits = stats.translator.cache_hits;
            diagnostics.last_translation_ago_s = stats.translator.last_success.map(|t| (now - t).max(0.0));
            diagnostics.translation_ms = stats.translator.last_latency.map(|s| (s * 1000.0) as u64);
            diagnostics.translation_errors = stats.translator.errors;
            diagnostics.timeouts = stats.translator.timeouts;
            diagnostics.rate_limited = stats.translator.rate_limited;
            diagnostics.retries = stats.translator.retries;
            diagnostics.rejected = stats.translator.rejected;
            diagnostics.last_translation_error = stats.translator.last_error.map(|e| e.to_string());
        }
        diagnostics.summarize();
        self.last_health = Instant::now();
        self.reads_since_health = 0;
        self.seq_at_health = self.frame_seq;
        on_event(Event::Health(Box::new(diagnostics)));
    }
}

/// TV'ye gönderim ve "ne gösteriliyor" olayları; tur içinde birden çok kez çağrılabilir.
#[derive(Default)]
struct Delivery {
    tv_ok: Option<bool>,
    last_shown: Vec<String>,
    last_ok: Option<Instant>,
    errors: u64,
}

impl Delivery {
    /// Gönderim başarılıysa (ya da TV yoksa) `true`.
    fn send(&mut self, tv: &mut Option<TvClient>, blocks: Vec<OverlayBlock>, screen: Option<Rect>, on_event: &mut impl FnMut(Event)) -> bool {
        let sent = match tv.as_mut() {
            Some(tv) => tv.send(&blocks),
            None => Ok(()),
        };
        if tv.is_some() {
            match &sent {
                Ok(()) if self.tv_ok != Some(true) => on_event(Event::TvConnected),
                Err(error) if self.tv_ok != Some(false) => on_event(Event::TvUnreachable(error.clone())),
                _ => {}
            }
            self.tv_ok = Some(sent.is_ok());
        }
        if sent.is_ok() {
            self.last_ok = Some(Instant::now());
        } else {
            self.errors += 1;
        }
        let shown: Vec<String> = blocks.iter().map(|b| b.text.clone()).collect();
        if sent.is_ok() && shown != self.last_shown {
            on_event(Event::Shown(shown.clone()));
            self.last_shown = shown;
        }
        on_event(Event::Frame { blocks, screen });
        sent.is_ok()
    }
}

/// Sabit bloklardan gösterilecek listeyi kurar: çevirisi hazır olanlar gider, eksikler çeviri
/// işçilerine verilir. İkinci değer çeviriye gönderilmeyen (ayıklanan) blok sayısıdır.
fn compose(config: &Config, service: Option<&TranslationService>, stable: &[StableBlock], below: &HashSet<String>) -> (Vec<OverlayBlock>, usize) {
    let Some(service) = service else {
        // Çevirisiz test: kutuların yazıya oturduğu görülsün diye hep yazının üstüne çizilir.
        return (stable.iter().map(|b| overlay_block(b, &b.text, false)).collect(), 0);
    };
    // Türkçe (PS5 arayüzü), yalnızca addan ya da sayıdan oluşan metin çevrilmez, üstüne de
    // çizilmez: orijinali görünür kalır.
    let terms = &config.terms;
    let english: Vec<&StableBlock> =
        stable.iter().filter(|b| ocr_text::has_letters(&b.text) && !language::looks_turkish(&b.text) && !terms.is_only_terms(&b.text)).collect();
    let filtered = stable.len() - english.len();
    // Önbellek ve DeepL işaretli hali görür; ad listesi değişince eski çeviri kullanılmaz.
    let keys: Vec<String> = english.iter().map(|b| terms.protect(&b.text)).collect();
    // Yaygın menü kelimeleri hazır sözlükten gelir; yalnızca kalanlar çeviri modeline sorulur.
    let ready: Vec<Option<&str>> = english.iter().map(|b| glossary::lookup(&b.text)).collect();
    let wanted: Vec<String> = keys.iter().zip(&ready).filter(|(_, hit)| hit.is_none()).map(|(key, _)| key.clone()).collect();
    let found = service.request(&wanted);
    let blocks = english
        .iter()
        .zip(keys.iter().zip(&ready))
        .filter_map(|(b, (key, hit))| {
            let text = match hit {
                Some(text) => text.to_string(),
                None => terms::unprotect(found.get(key)?),
            };
            (!text.trim().is_empty()).then(|| overlay_block(b, &text, config.below && below.contains(&b.id)))
        })
        .collect();
    (blocks, filtered)
}

fn overlay_block(block: &StableBlock, text: &str, below: bool) -> OverlayBlock {
    let b = if below { focus::beside(block.bbox, block.line_height) } else { block.bbox };
    OverlayBlock { id: block.id.clone(), x: b.x, y: b.y, w: b.width, h: b.height, line_h: block.line_height, text: text.to_string() }
}
#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;
    use std::thread;

    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;

    use ceviri_core::{FrameInfo, OverlayMessage, ReadError, Reading, Rect, Stage, TextLine};

    use super::*;
    use crate::tv_client::tests::fake_tv;

    /// Her turda aynı satırları "okuyan" sahte ekran; ilk `missing` turda pencere yoktur.
    struct FakeReader {
        lines: Vec<TextLine>,
        missing: u32,
        calls: u32,
    }

    impl ScreenReader for FakeReader {
        fn read(&mut self) -> Result<Reading, ReadError> {
            self.calls += 1;
            if self.calls <= self.missing {
                return Err(ReadError::WindowNotFound("pencere yok".into()));
            }
            Ok(reading(self.lines.clone()))
        }
    }

    fn reading(lines: Vec<TextLine>) -> Reading {
        Reading { lines, video_width: 1920, video_height: 1080, fullscreen: true, capture_ms: 1, ocr_ms: 1, screen: None, frame: FrameInfo::default() }
    }

    fn line(text: &str, y: f64) -> TextLine {
        TextLine { text: text.into(), bbox: Rect::new(0.1, y, 0.1, 0.03), confidence: 0.9 }
    }

    fn menu() -> Vec<TextLine> {
        vec![line("Play Local", 0.5), line("Seçenekler", 0.6)]
    }

    fn texts(message: &OverlayMessage) -> Vec<&str> {
        message.blocks.iter().map(|b| b.text.as_str()).collect()
    }

    /// Döngüyü ayrı thread'de çalıştırır; `done` bir mesajda doğru dönene kadar TV'ye gelenleri
    /// toplar, sonra durdurur. Gelen tüm mesajları ve olayları döner.
    fn run_until(
        reader: FakeReader,
        config: Config,
        messages: &Receiver<OverlayMessage>,
        address: String,
        done: impl Fn(&OverlayMessage) -> bool,
    ) -> (Vec<OverlayMessage>, Vec<Event>) {
        let mut pipeline = Pipeline::new(Box::new(reader), Some(TvClient::new(address)), config);
        let stop = pipeline.stop_handle();
        let worker = thread::spawn(move || {
            let mut events = Vec::new();
            pipeline.run(|event| events.push(event));
            events
        });
        let mut received = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(message) = messages.recv_timeout(Duration::from_millis(100)) {
                let finished = done(&message);
                received.push(message);
                if finished {
                    break;
                }
            }
        }
        stop.store(true, Ordering::Relaxed);
        let events = worker.join().unwrap();
        received.extend(messages.try_iter());
        (received, events)
    }

    fn fast() -> Config {
        Config { fps: 20.0, ..Config::default() }
    }

    #[test]
    fn cevirisiz_modda_sabit_metin_tvye_gider() {
        let (address, messages) = fake_tv(usize::MAX);
        let reader = FakeReader { lines: menu(), missing: 0, calls: 0 };

        let (received, events) = run_until(reader, fast(), &messages, address, |m| !m.blocks.is_empty());

        // İlk tur metin henüz oturmadı: boş liste (heartbeat). Sonra iki blok da olduğu gibi gider.
        assert!(received[0].blocks.is_empty());
        let shown = received.iter().find(|m| !m.blocks.is_empty()).expect("metin TV'ye gitmedi");
        assert_eq!(texts(shown), ["Play Local", "Seçenekler"]);
        assert!(received.windows(2).all(|w| w[1].seq > w[0].seq), "seq artmalı");
        assert!(events.contains(&Event::TvConnected));
        assert!(events.contains(&Event::Shown(vec!["Play Local".into(), "Seçenekler".into()])));
    }

    #[test]
    fn ingilizce_cevrilir_turkce_ayiklanir() {
        let (address, messages) = fake_tv(usize::MAX);
        let asked = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = asked.clone();
        let backend: Backend = Arc::new(move |texts: &[String]| {
            log.lock().unwrap().extend(texts.iter().cloned());
            Ok(texts.iter().map(|t| format!("TR({t})")).collect())
        });
        let config = Config { backend: Some(backend), ..fast() };
        let reader = FakeReader { lines: menu(), missing: 0, calls: 0 };

        let (received, _) = run_until(reader, config, &messages, address, |m| !m.blocks.is_empty());

        let shown = received.iter().find(|m| !m.blocks.is_empty()).expect("çeviri TV'ye gitmedi");
        assert_eq!(texts(shown), ["TR(Play Local)"]);
        // PS5'in Türkçe arayüz yazısı çeviriye hiç gönderilmez; İngilizce metin bir kez istenir.
        assert_eq!(*asked.lock().unwrap(), ["Play Local"]);
    }

    #[test]
    fn adlar_korunur_ve_yalnizca_ad_olan_metin_gonderilmez() {
        let (address, messages) = fake_tv(usize::MAX);
        let asked = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = asked.clone();
        // DeepL gibi: <x> içine dokunmaz, gerisini "çevirir".
        let backend: Backend = Arc::new(move |texts: &[String]| {
            log.lock().unwrap().extend(texts.iter().cloned());
            Ok(texts.iter().map(|t| t.replace("Continue", "Devam et")).collect())
        });
        let config = Config { backend: Some(backend), terms: Terms::new(&["It Takes Two".into()]), ..fast() };
        let reader = FakeReader { lines: vec![line("Continue It Takes Two", 0.3), line("It Takes Two", 0.6)], missing: 0, calls: 0 };

        let (received, _) = run_until(reader, config, &messages, address, |m| !m.blocks.is_empty());

        let shown = received.iter().find(|m| !m.blocks.is_empty()).unwrap();
        assert_eq!(texts(shown), ["Devam et It Takes Two"]);
        assert_eq!(*asked.lock().unwrap(), ["Continue <x>It Takes Two</x>"]);
    }

    #[test]
    fn onbellekteki_ceviri_istek_atmadan_gider() {
        let (address, messages) = fake_tv(usize::MAX);
        let backend: Backend = Arc::new(|_: &[String]| panic!("önbellekte olan metin istenmemeli"));
        let cache = [("Play Local".to_string(), "Yerel Oyna".to_string())].into();
        let config = Config { backend: Some(backend), cache, ..fast() };
        let reader = FakeReader { lines: vec![line("Play Local", 0.5)], missing: 0, calls: 0 };

        let (received, _) = run_until(reader, config, &messages, address, |m| !m.blocks.is_empty());

        assert!(received.iter().any(|m| texts(m) == ["Yerel Oyna"]));
    }

    #[test]
    fn pencere_kaybolunca_beklenir_ve_bulununca_devam_edilir() {
        let (address, messages) = fake_tv(usize::MAX);
        let reader = FakeReader { lines: menu(), missing: 3, calls: 0 };

        let (received, events) = run_until(reader, fast(), &messages, address, |m| !m.blocks.is_empty());

        // Pencere yokken de heartbeat gider (TV eski yazıyı kaldırsın); olay yalnızca bir kez gelir.
        assert!(received.iter().take(3).all(|m| m.blocks.is_empty()));
        assert_eq!(events.iter().filter(|e| matches!(e, Event::WaitingForWindow(_))).count(), 1);
        assert!(events.contains(&Event::WindowFound));
        assert!(received.iter().any(|m| !m.blocks.is_empty()));
    }

    #[test]
    fn durdurulunca_tv_ekrani_temizlenir() {
        let (address, messages) = fake_tv(usize::MAX);
        let reader = FakeReader { lines: menu(), missing: 0, calls: 0 };

        let (received, _) = run_until(reader, fast(), &messages, address, |m| !m.blocks.is_empty());

        assert!(received.last().unwrap().blocks.is_empty(), "son mesaj ekranı temizlemeli");
    }

    #[test]
    fn tvye_ulasilamazsa_bir_kez_bildirilir_ve_durdurulabilir() {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let address = format!("127.0.0.1:{port}");
        // Kapalı porta tek bağlanma denemesinin süresi işletim sistemine göre değişir
        // (macOS: anında, Windows: ~2 sn). Durdurma en fazla süren bir denemeyi beklemeli.
        let attempt_started = Instant::now();
        assert!(TvClient::new(address.clone()).send(&[]).is_err());
        let one_attempt = attempt_started.elapsed();
        let mut pipeline = Pipeline::new(Box::new(FakeReader { lines: menu(), missing: 0, calls: 0 }), Some(TvClient::new(address)), fast());
        let stop = pipeline.stop_handle();
        let worker = thread::spawn(move || {
            let mut events = Vec::new();
            pipeline.run(|event| events.push(event));
            events
        });
        thread::sleep(Duration::from_millis(300));
        let stopped_at = Instant::now();
        stop.store(true, Ordering::Relaxed);
        let events = worker.join().unwrap();

        // 2 sn'lik bekleme kesilmeli ve kapanışta yeniden bağlanılmamalı.
        let limit = one_attempt + Duration::from_millis(700);
        assert!(stopped_at.elapsed() < limit, "durdurma {:?} sürdü, sınır {limit:?}", stopped_at.elapsed());
        assert_eq!(events.iter().filter(|e| matches!(e, Event::TvUnreachable(_))).count(), 1);
    }

    #[test]
    fn ceviri_gelince_sonraki_tur_beklenmeden_gonderilir() {
        let (address, messages) = fake_tv(usize::MAX);
        let backend: Backend = Arc::new(|texts: &[String]| {
            thread::sleep(Duration::from_millis(150));
            Ok(texts.iter().map(|t| format!("TR({t})")).collect())
        });
        // Saniyede 1 tur: metin 2. turda (1. sn) oturur, çeviri ~1.15 sn'de hazır olur. Bir sonraki
        // tur 2. sn'de; çeviri ondan çok önce TV'ye gitmiş olmalı.
        let config = Config { backend: Some(backend), fps: 1.0, ..Config::default() };
        let reader = FakeReader { lines: vec![line("Play Local", 0.5)], missing: 0, calls: 0 };
        let started = Instant::now();

        let (received, _) = run_until(reader, config, &messages, address, |m| !m.blocks.is_empty());

        assert!(received.iter().any(|m| texts(m) == ["TR(Play Local)"]));
        assert!(started.elapsed() < Duration::from_millis(1700), "çeviri {:?} sonra geldi", started.elapsed());
    }

    fn sentence(text: &str, y: f64) -> TextLine {
        TextLine { text: text.into(), bbox: Rect::new(0.3, y, 0.4, 0.04), confidence: 0.9 }
    }

    fn echo() -> Backend {
        Arc::new(|texts: &[String]| Ok(texts.iter().map(|t| format!("TR({t})")).collect()))
    }

    #[test]
    fn odak_modunda_tabela_cevrilmez_altyazi_cevrilir() {
        let (address, messages) = fake_tv(usize::MAX);
        let config = Config { backend: Some(echo()), focus: true, ..fast() };
        // Logo boyundaki yazı ve hemen altındaki satır (logonun parçası) atlanır.
        let logo = TextLine { text: "Vice City".into(), bbox: Rect::new(0.05, 0.1, 0.3, 0.12), confidence: 0.9 };
        let edition = TextLine { text: "DEFINITIVE EDITION".into(), bbox: Rect::new(0.08, 0.23, 0.2, 0.03), confidence: 0.9 };
        let reader = FakeReader { lines: vec![logo, edition, sentence("You shouldn't be here.", 0.8)], missing: 0, calls: 0 };

        let (received, _) = run_until(reader, config, &messages, address, |m| !m.blocks.is_empty());

        let shown = received.iter().find(|m| !m.blocks.is_empty()).unwrap();
        assert_eq!(texts(shown), ["TR(You shouldn't be here.)"]);
        // Odak tek başına yeri değiştirmez: çeviri yazının üstünde.
        assert_eq!(shown.blocks[0].y, 0.8);
    }

    #[test]
    fn altyazi_cevirisi_yazinin_altina_konur() {
        let (address, messages) = fake_tv(usize::MAX);
        let config = Config { backend: Some(echo()), below: true, ..fast() };
        let reader = FakeReader { lines: vec![sentence("You shouldn't be here.", 0.7)], missing: 0, calls: 0 };

        let (received, _) = run_until(reader, config, &messages, address, |m| !m.blocks.is_empty());

        let block = &received.iter().find(|m| !m.blocks.is_empty()).unwrap().blocks[0];
        // 0.7 + 0.04 yükseklik + 0.35 satırlık boşluk; boyut aynı.
        assert!((block.y - 0.754).abs() < 1e-9, "{}", block.y);
        assert!((block.x - 0.3).abs() < 1e-9 && (block.w - 0.4).abs() < 1e-9 && (block.h - 0.04).abs() < 1e-9);
    }

    #[test]
    fn yaygin_menu_kelimesi_modele_sorulmadan_cevrilir() {
        let (address, messages) = fake_tv(usize::MAX);
        let backend: Backend = Arc::new(|_: &[String]| panic!("sözlükteki kelime modele gitmemeli"));
        let config = Config { backend: Some(backend), ..fast() };
        let reader = FakeReader { lines: vec![line("ReSume x", 0.5)], missing: 0, calls: 0 };

        let (received, _) = run_until(reader, config, &messages, address, |m| !m.blocks.is_empty());

        assert!(received.iter().any(|m| texts(m) == ["Devam Et"]));
    }

    /// Her turu bir kapanışla üreten okuyucu (tur numarası 1'den başlar).
    struct ScriptReader<F: FnMut(u64) -> Result<Reading, ReadError> + Send> {
        script: F,
        round: u64,
    }

    impl<F: FnMut(u64) -> Result<Reading, ReadError> + Send> ScreenReader for ScriptReader<F> {
        fn read(&mut self) -> Result<Reading, ReadError> {
            self.round += 1;
            (self.script)(self.round)
        }
    }

    fn script(f: impl FnMut(u64) -> Result<Reading, ReadError> + Send + 'static) -> Box<dyn ScreenReader> {
        Box::new(ScriptReader { script: f, round: 0 })
    }

    /// TV'siz (ekran overlay'i) çalıştırır; `done` bir olayda doğru dönene ya da süre dolana kadar
    /// olayları toplar.
    fn run_events(pipeline: Pipeline, limit: Duration, done: impl Fn(&Event) -> bool + Send + 'static) -> Vec<Event> {
        let mut pipeline = pipeline;
        let stop = pipeline.stop_handle();
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let stopper = stop.clone();
        let worker = thread::spawn(move || {
            pipeline.run(|event| {
                if done(&event) {
                    stopper.store(true, Ordering::Relaxed);
                }
                sink.lock().unwrap().push(event);
            })
        });
        let deadline = Instant::now() + limit;
        while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        stop.store(true, Ordering::Relaxed);
        worker.join().unwrap();
        let events = events.lock().unwrap().clone();
        events
    }

    fn shown_text(event: &Event, wanted: &str) -> bool {
        matches!(event, Event::Shown(texts) if texts.iter().any(|t| t == wanted))
    }

    #[test]
    fn tur_panigi_donguyu_durdurmaz() {
        let reader = script(|round| {
            if round == 2 {
                panic!("beklenmeyen OCR çıktısı");
            }
            Ok(reading(vec![sentence("You shouldn't be here.", 0.8)]))
        });
        let config = Config { backend: Some(echo()), ..fast() };

        let events = run_events(Pipeline::new(reader, None, config), Duration::from_secs(5), |e| shown_text(e, "TR(You shouldn't be here.)"));

        assert!(events.iter().any(|e| matches!(e, Event::Recovered(m) if m.contains("beklenmeyen OCR çıktısı"))), "{events:?}");
        assert!(events.iter().any(|e| shown_text(e, "TR(You shouldn't be here.)")), "panikten sonra çeviri sürmeli");
    }

    #[test]
    fn okuma_takilirsa_okuyucu_yeniden_kurulur() {
        let broken = script(|_| Err(ReadError::Other("ekran okuma 20 sn içinde yanıt vermedi".into())));
        let created = Arc::new(AtomicU64::new(0));
        let counter = created.clone();
        let factory: ReaderFactory = Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(script(|_| Ok(reading(vec![sentence("The bridge is out.", 0.8)]))))
        });
        let config = Config { backend: Some(echo()), reader_restart_after: 0.3, ..fast() };
        let pipeline = Pipeline::new(broken, None, config).with_reader_factory(factory);

        let events = run_events(pipeline, Duration::from_secs(5), |e| shown_text(e, "TR(The bridge is out.)"));

        assert_eq!(created.load(Ordering::SeqCst), 1);
        assert!(events.iter().any(|e| matches!(e, Event::Recovered(m) if m.contains("okuyucu yeniden kuruldu"))));
        assert!(events.iter().any(|e| shown_text(e, "TR(The bridge is out.)")));
        // Aynı hata her turda arayüze yağmaz.
        let skipped = events.iter().filter(|e| matches!(e, Event::RoundSkipped(_))).count();
        assert_eq!(skipped, 1, "{events:?}");
    }

    #[test]
    fn okuyucu_yenileme_araligi_giderek_uzar() {
        let created = Arc::new(AtomicU64::new(0));
        let counter = created.clone();
        let factory: ReaderFactory = Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(script(|_| Err(ReadError::Busy)))
        });
        let config = Config { reader_restart_after: 0.2, ..fast() };
        let pipeline = Pipeline::new(script(|_| Err(ReadError::Busy)), None, config).with_reader_factory(factory);

        // 0,2 + 0,4 + 0,8 = 1,4 sn: 1,6 sn'de en fazla 3 yenileme (sonsuz döngü yok).
        run_events(pipeline, Duration::from_millis(1600), |_| false);

        let restarts = created.load(Ordering::SeqCst);
        assert!((2..=3).contains(&restarts), "{restarts} yenileme");
    }

    #[test]
    fn bir_anlik_okuma_hatasinda_ceviri_ekrandan_kalkmaz() {
        // 5. turdan sonra her 3 turda bir okuma başarısız (OCR süre sınırı).
        let reader = script(|round| {
            if round > 5 && round % 3 == 0 {
                return Err(ReadError::Ocr("5 sn içinde yanıt gelmedi".into()));
            }
            Ok(reading(vec![sentence("Hold the line.", 0.8)]))
        });
        let config = Config { backend: Some(echo()), ..fast() };
        let started = Instant::now();

        let events = run_events(Pipeline::new(reader, None, config), Duration::from_millis(1500), |_| false);

        let frames: Vec<usize> = events
            .iter()
            .filter_map(|e| match e {
                Event::Frame { blocks, .. } => Some(blocks.len()),
                _ => None,
            })
            .collect();
        let first_shown = frames.iter().position(|n| *n > 0).expect("çeviri gösterilmeli");
        // Son kare (durdurma temizliği) hariç, gösterildikten sonra hiç boş kare yok.
        assert!(frames[first_shown..frames.len() - 1].iter().all(|n| *n > 0), "{frames:?}");
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn ceviri_servisi_coktugunde_okuma_surer_ve_duzelince_ceviri_gelir() {
        let healthy = Arc::new(AtomicBool::new(false));
        let flag = healthy.clone();
        let backend: Backend = Arc::new(move |texts: &[String]| {
            if !flag.load(Ordering::SeqCst) {
                return Err(TranslationError::Http(500));
            }
            Ok(texts.iter().map(|t| format!("TR({t})")).collect())
        });
        let reader = script(|_| Ok(reading(vec![sentence("Get to the boat.", 0.8)])));
        let config = Config { backend: Some(backend), ..fast() };
        let mut pipeline = Pipeline::new(reader, None, config);
        let stop = pipeline.stop_handle();
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let worker = thread::spawn(move || pipeline.run(|event| sink.lock().unwrap().push(event)));

        thread::sleep(Duration::from_millis(1600));
        {
            let events = events.lock().unwrap();
            let health = events.iter().rev().find_map(|e| match e {
                Event::Health(d) => Some(d.clone()),
                _ => None,
            });
            let health = health.expect("teşhis gelmeli");
            assert_eq!(health.stage, Stage::TranslationDown, "{health:?}");
            assert!(health.reads_per_s > 5.0, "okuma sürmeli: {}", health.reads_per_s);
            assert!(events.iter().any(|e| matches!(e, Event::TranslationFailed(TranslationError::Http(500)))));
        }
        healthy.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !events.lock().unwrap().iter().any(|e| shown_text(e, "TR(Get to the boat.)")) {
            thread::sleep(Duration::from_millis(20));
        }
        stop.store(true, Ordering::Relaxed);
        worker.join().unwrap();
        assert!(events.lock().unwrap().iter().any(|e| shown_text(e, "TR(Get to the boat.)")), "servis düzelince çeviri gelmeli");
    }

    #[test]
    fn bos_ceviri_gosterilmez() {
        let backend: Backend = Arc::new(|texts: &[String]| Ok(texts.iter().map(|_| String::new()).collect()));
        let reader = script(|_| Ok(reading(vec![sentence("Nothing to see here.", 0.8)])));
        let config = Config { backend: Some(backend), ..fast() };

        let events = run_events(Pipeline::new(reader, None, config), Duration::from_millis(800), |_| false);

        assert!(!events.iter().any(|e| matches!(e, Event::Frame { blocks, .. } if !blocks.is_empty())), "boş kutu çizilmemeli");
    }

    #[test]
    fn donmus_kare_teshiste_gorunur() {
        let reader = script(|_| Ok(Reading { frame: FrameInfo { seq: Some(7), age_ms: Some(900), restarts: 0 }, ..reading(vec![]) }));

        let events = run_events(Pipeline::new(reader, None, fast()), Duration::from_millis(2300), |_| false);

        let last = events.iter().rev().find_map(|e| match e {
            Event::Health(d) => Some(d.clone()),
            _ => None,
        });
        let last = last.unwrap();
        assert!(last.frozen_s.unwrap() > 1.0, "{last:?}");
        assert_eq!(last.frames_per_s, Some(0.0));
        assert_eq!(last.frame_age_ms, Some(900));
    }

    #[test]
    fn hizli_diyaloglarda_ekrandaki_son_replik_cevrilir_eskiler_kuyrukta_beklemez() {
        let asked = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = asked.clone();
        // Yavaş çeviri: her parti 150 ms.
        let backend: Backend = Arc::new(move |texts: &[String]| {
            log.lock().unwrap().extend(texts.iter().cloned());
            thread::sleep(Duration::from_millis(150));
            Ok(texts.iter().map(|t| format!("TR({t})")).collect())
        });
        // Replik her 3 turda (150 ms) değişir; 12. replik ekranda kalır.
        let reader = script(|round| {
            let line = ((round - 1) / 3).min(11);
            Ok(reading(vec![sentence(&format!("This is dialogue line number {line}."), 0.8)]))
        });
        let config = Config { backend: Some(backend), ..fast() };

        let events = run_events(Pipeline::new(reader, None, config), Duration::from_secs(6), |e| shown_text(e, "TR(This is dialogue line number 11.)"));

        assert!(events.iter().any(|e| shown_text(e, "TR(This is dialogue line number 11.)")));
        // Ekrandan kalkmış replikler sıraya girmez: istenen her metin bir kez ve toplamda az.
        let asked = asked.lock().unwrap();
        let unique: HashSet<&String> = asked.iter().collect();
        assert_eq!(unique.len(), asked.len(), "aynı metin iki kez istenmemeli: {asked:?}");
        assert!(asked.len() < 12, "eski replikler kuyrukta birikmemeli: {asked:?}");
    }

    /// Uzun süreli çalışma: yakalama, OCR ve çeviri aralıklarla hata verirken (panik, zaman aşımı,
    /// 429/500, boş yanıt, takılan istek) yeni replikler çevrilmeye devam etmeli; kuyruklar ve
    /// thread sayısı büyümemeli. Varsayılan 6 sn; `CEVIRI_SOAK_SECS=1800` ile 30 dk koşar.
    #[test]
    fn uzun_sureli_calisma_yeni_metinleri_cevirmeye_devam_eder() {
        let seconds: u64 = std::env::var("CEVIRI_SOAK_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(6);
        let calls = Arc::new(AtomicU64::new(0));
        let counter = calls.clone();
        let backend: Backend = Arc::new(move |texts: &[String]| {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            match n % 23 {
                3 => Err(TranslationError::Http(500)),
                7 => Err(TranslationError::RateLimited),
                11 => Err(TranslationError::Timeout),
                13 => panic!("yerel model çöktü"),
                17 => Ok(texts.iter().map(|_| String::new()).collect()),
                19 => {
                    thread::sleep(Duration::from_millis(600));
                    Ok(texts.iter().map(|t| format!("TR({t})")).collect())
                }
                _ => Ok(texts.iter().map(|t| format!("TR({t})")).collect()),
            }
        });
        // Replik her 30 turda (0,6 sn) değişir; arada okuma hataları ve bir kez panik.
        let reader = script(|round| match round % 97 {
            10 => Err(ReadError::Ocr("5 sn içinde yanıt gelmedi".into())),
            20 => Err(ReadError::Busy),
            30 => Err(ReadError::NoFrameYet),
            40 => Err(ReadError::Other("akış durdu".into())),
            50 if round < 100 => panic!("beklenmeyen"),
            _ => {
                let dialogue = round / 30;
                let frame = FrameInfo { seq: Some(round), age_ms: Some(16), restarts: 0 };
                Ok(Reading { frame, ..reading(vec![sentence(&format!("Dialogue {dialogue}: keep moving forward."), 0.8), line("Inventory", 0.1)]) })
            }
        });
        let config = Config { backend: Some(backend), fps: 50.0, translation: translation::Options { stuck_after: 2.0, ..Default::default() }, ..Config::default() };
        let threads_before = thread_count();

        let events = run_events(Pipeline::new(reader, None, config), Duration::from_secs(seconds), |_| false);

        let translated: HashSet<String> = events
            .iter()
            .filter_map(|e| match e {
                Event::Shown(texts) => Some(texts.clone()),
                _ => None,
            })
            .flatten()
            .filter(|t| t.starts_with("TR(Dialogue"))
            .collect();
        let dialogues = seconds * 50 / 30;
        // Hataya denk gelen birkaç replik dışında hepsi çevrilip gösterilmeli.
        assert!(translated.len() as u64 * 10 >= dialogues * 6, "{} / {dialogues} replik çevrildi", translated.len());
        let healths: Vec<&Diagnostics> = events
            .iter()
            .filter_map(|e| match e {
                Event::Health(d) => Some(d.as_ref()),
                _ => None,
            })
            .collect();
        assert!(healths.len() as u64 >= seconds.saturating_sub(1));
        assert!(healths.iter().all(|d| d.queue + d.in_flight <= 4), "kuyruk büyümemeli");
        // Son birkaç saniyede de yeni çeviri gelmeli (sessizce durmamalı).
        let last = healths.last().unwrap();
        assert!(last.last_translation_ago_s.is_some_and(|s| s < 3.0), "{last:?}");
        if let (Some(before), Some(after)) = (threads_before, thread_count()) {
            assert!(after <= before + 8, "thread sayısı arttı: {before} → {after}");
        }
    }

    /// Linux'ta sürecin thread sayısı (diğer sistemlerde ölçülmez).
    fn thread_count() -> Option<usize> {
        std::fs::read_dir("/proc/self/task").ok().map(|d| d.count())
    }
}
