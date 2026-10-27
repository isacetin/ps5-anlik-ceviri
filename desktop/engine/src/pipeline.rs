//! Ana döngü (Swift `ps5ceviri run` karşılığı): oku → birleştir → sabitle → Türkçeyi ayıkla →
//! çevir (önbellek + arka planda DeepL) → TV'ye gönder. Durum ekrana yazılmaz, `Event` olarak
//! bildirilir; komut satırı da masaüstü arayüzü de aynı döngüyü kullanır.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ceviri_core::{block_merger, language, storage, translator, OverlayBlock, ReadError, ScreenReader, StableBlock, Stabilizer, TranslationError, Translator};

use crate::tv_client::TvClient;

/// Çeviri arka ucu (gerçekte DeepL, testlerde sahte).
pub type Backend = Arc<dyn Fn(&[String]) -> Result<Vec<String>, TranslationError> + Send + Sync>;

pub struct Config {
    /// Saniyedeki tur; TV 3 sn mesajsız kalırsa temizlediği için en az 1.
    pub fps: f64,
    /// `None`: çevirisiz (dry-run), İngilizce metin olduğu gibi gönderilir.
    pub backend: Option<Backend>,
    /// Başlangıç önbelleği ve her başarılı çeviriden sonra yazılacağı dosya.
    pub cache: std::collections::HashMap<String, String>,
    pub cache_path: Option<PathBuf>,
    /// Her turda blok kutuları çizilmiş kareyi buraya kaydeder.
    pub dump_dir: Option<PathBuf>,
    /// Tek tur çalışıp ekranı temizler (hizalama testi).
    pub once: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config { fps: 2.0, backend: None, cache: Default::default(), cache_path: None, dump_dir: None, once: false }
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
    /// TV'de gösterilen liste değişti.
    Shown(Vec<String>),
    TvConnected,
    TvUnreachable(String),
    TranslationFailed(TranslationError),
    RoundSkipped(String),
}

pub struct Pipeline {
    reader: Box<dyn ScreenReader>,
    tv: TvClient,
    config: Config,
    stop: Arc<AtomicBool>,
}

impl Pipeline {
    pub fn new(reader: Box<dyn ScreenReader>, tv: TvClient, config: Config) -> Pipeline {
        Pipeline { reader, tv, config, stop: Arc::new(AtomicBool::new(false)) }
    }

    /// Başka bir thread'den `store(true)` ile döngü durdurulur; çıkarken TV ekranı temizlenir.
    pub fn stop_handle(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }

    /// Durdurulana (ya da `once` ise bir tur) kadar çalışır.
    pub fn run(&mut self, mut on_event: impl FnMut(Event)) {
        let fps = self.config.fps.max(1.0);
        let started = Instant::now();
        let elapsed = move || started.elapsed().as_secs_f64();
        let translator = Arc::new(Mutex::new(Translator::new(std::mem::take(&mut self.config.cache))));
        let (failures_tx, failures): (Sender<TranslationError>, Receiver<TranslationError>) = mpsc::channel();
        let mut stabilizer = Stabilizer::new();
        let mut last_shown: Vec<String> = Vec::new();
        let mut waiting_for_window = false;
        let mut tv_ok: Option<bool> = None;
        let mut round = 0u64;

        while !self.stop.load(Ordering::Relaxed) {
            round += 1;
            let round_start = Instant::now();
            for error in failures.try_iter() {
                on_event(Event::TranslationFailed(error));
            }

            let mut blocks: Vec<OverlayBlock> = Vec::new();
            match self.reader.read() {
                Ok(reading) => {
                    if waiting_for_window {
                        on_event(Event::WindowFound);
                    }
                    waiting_for_window = false;

                    let merged = block_merger::merge(&reading.lines);
                    let stable = stabilizer.update(&merged, elapsed());
                    blocks = match &self.config.backend {
                        Some(backend) => {
                            let english: Vec<&StableBlock> = stable.iter().filter(|b| !language::looks_turkish(&b.text)).collect();
                            let texts: Vec<String> = english.iter().map(|b| b.text.clone()).collect();
                            let lookup = translator.lock().unwrap().lookup(&texts, elapsed());
                            if !lookup.to_fetch.is_empty() {
                                spawn_fetch(&translator, lookup.to_fetch, elapsed(), backend.clone(), self.config.cache_path.clone(), failures_tx.clone());
                            }
                            english.iter().filter_map(|b| lookup.found.get(&b.text).map(|t| overlay_block(b, t))).collect()
                        }
                        None => stable.iter().map(|b| overlay_block(b, &b.text)).collect(),
                    };

                    on_event(Event::Round(RoundStats {
                        round,
                        video_width: reading.video_width,
                        video_height: reading.video_height,
                        capture_ms: reading.capture_ms,
                        ocr_ms: reading.ocr_ms,
                        merged: merged.len(),
                        stable: stable.len(),
                        sent: blocks.len(),
                    }));
                    if let Some(dir) = &self.config.dump_dir {
                        let boxes: Vec<_> = merged.iter().map(|m| m.bbox).collect();
                        if let Err(error) = self.reader.dump(&dir.join(format!("ocr_{round:04}.png")), &boxes) {
                            on_event(Event::RoundSkipped(format!("kare kaydedilemedi: {error}")));
                        }
                    }
                }
                Err(ReadError::WindowNotFound(message)) => {
                    if !waiting_for_window {
                        on_event(Event::WaitingForWindow(message));
                    }
                    waiting_for_window = true;
                    stabilizer = Stabilizer::new();
                }
                Err(ReadError::NoFrameYet) => {}
                Err(error) => on_event(Event::RoundSkipped(error.to_string())),
            }

            // Liste değişmese de her tur gönderilir: TV 3 sn mesaj almazsa yazıları kaldırır.
            let sent = self.tv.send(&blocks);
            match &sent {
                Ok(()) if tv_ok != Some(true) => on_event(Event::TvConnected),
                Err(error) if tv_ok != Some(false) => on_event(Event::TvUnreachable(error.clone())),
                _ => {}
            }
            tv_ok = Some(sent.is_ok());
            let shown: Vec<String> = blocks.iter().map(|b| b.text.clone()).collect();
            if sent.is_ok() && shown != last_shown {
                on_event(Event::Shown(shown.clone()));
                last_shown = shown;
            }

            if self.config.once {
                std::thread::sleep(Duration::from_secs(2));
                break;
            }
            let pause = if sent.is_err() { Duration::from_secs(2) } else { Duration::from_secs_f64(1.0 / fps).saturating_sub(round_start.elapsed()) };
            sleep_unless_stopped(pause, &self.stop);
        }
        self.tv.close();
    }
}

fn overlay_block(block: &StableBlock, text: &str) -> OverlayBlock {
    let b = block.bbox;
    OverlayBlock { id: block.id.clone(), x: b.x, y: b.y, w: b.width, h: b.height, line_h: block.line_height, text: text.to_string() }
}

/// Eksik çevirileri arka planda ister; ana döngü bu sırada okumaya devam eder.
fn spawn_fetch(translator: &Arc<Mutex<Translator>>, texts: Vec<String>, now: f64, backend: Backend, cache_path: Option<PathBuf>, failures: Sender<TranslationError>) {
    let translator = translator.clone();
    std::thread::spawn(move || match translator::fetch(&translator, &texts, now, |batch| backend(batch)) {
        Ok(()) => {
            if let Some(path) = cache_path {
                let snapshot = translator.lock().unwrap().snapshot().clone();
                let _ = storage::save_cache(&path, &snapshot);
            }
        }
        Err(error) => {
            let _ = failures.send(error);
        }
    });
}

/// Durdurma isteğine hızlı cevap verebilmek için kısa parçalar halinde uyur.
fn sleep_unless_stopped(duration: Duration, stop: &AtomicBool) {
    let deadline = Instant::now() + duration;
    while !stop.load(Ordering::Relaxed) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(50)));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;
    use std::thread;

    use ceviri_core::{OverlayMessage, ReadError, Reading, Rect, TextLine};

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
            Ok(Reading { lines: self.lines.clone(), video_width: 1920, video_height: 1080, fullscreen: true, capture_ms: 1, ocr_ms: 1 })
        }
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
        let mut pipeline = Pipeline::new(Box::new(reader), TvClient::new(address), config);
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
        let mut pipeline = Pipeline::new(Box::new(FakeReader { lines: menu(), missing: 0, calls: 0 }), TvClient::new(address), fast());
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
}
