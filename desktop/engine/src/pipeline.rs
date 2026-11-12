//! Ana döngü (Swift `ps5ceviri run` karşılığı): oku → birleştir → sabitle → Türkçeyi ayıkla →
//! çevir (önbellek + arka planda DeepL) → TV'ye gönder. Durum ekrana yazılmaz, `Event` olarak
//! bildirilir; komut satırı da masaüstü arayüzü de aynı döngüyü kullanır.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ceviri_core::{block_merger, focus, glossary, language, storage, terms, translator, OverlayBlock, ReadError, Rect, ScreenReader, StableBlock, Stabilizer, Terms, TranslationError, Translator};

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
    /// Çevrilmeyecek adlar (oyun, karakter, marka).
    pub terms: Terms,
    /// Yalnızca altyazı ve menü çevrilir; logo, tabela gibi oyun dünyası yazıları atlanır.
    pub focus: bool,
    /// Altyazı çevirisi orijinal yazının üstüne değil hemen altına konur (ikisi de okunur).
    pub below: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config { fps: 5.0, backend: None, cache: Default::default(), cache_path: None, dump_dir: None, once: false, terms: Terms::new(&[]), focus: false, below: false }
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
}

pub struct Pipeline {
    reader: Box<dyn ScreenReader>,
    /// `None`: yalnızca ekran overlay'i (TV'ye gönderilmez).
    tv: Option<TvClient>,
    config: Config,
    stop: Arc<AtomicBool>,
}

impl Pipeline {
    pub fn new(reader: Box<dyn ScreenReader>, tv: Option<TvClient>, config: Config) -> Pipeline {
        Pipeline { reader, tv, config, stop: Arc::new(AtomicBool::new(false)) }
    }

    /// Döngüyü dışarıdaki bir bayrağa bağlar (döngü kurulmadan önce oluşturulan "Durdur" düğmesi gibi).
    pub fn with_stop(mut self, stop: Arc<AtomicBool>) -> Pipeline {
        self.stop = stop;
        self
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
        let (failures_tx, failures): (Sender<TranslationError>, Receiver<TranslationError>) = mpsc::channel();
        // Arka plandaki çeviri bitince haber verir: sonuç bir sonraki turu beklemeden gösterilir.
        let (ready_tx, ready): (Sender<()>, Receiver<()>) = mpsc::channel();
        let fetcher = Fetcher {
            translator: Arc::new(Mutex::new(Translator::new(std::mem::take(&mut self.config.cache)))),
            cache_path: self.config.cache_path.clone(),
            failures: failures_tx,
            ready: ready_tx,
        };
        let mut stabilizer = Stabilizer::new();
        let mut delivery = Delivery { tv_ok: None, last_shown: Vec::new() };
        let mut waiting_for_window = false;
        let mut round = 0u64;

        while !self.stop.load(Ordering::Relaxed) {
            round += 1;
            let round_start = Instant::now();
            for error in failures.try_iter() {
                on_event(Event::TranslationFailed(error));
            }
            // Bu turdan önce biten çeviriler zaten bu turda görülecek.
            ready.try_iter().for_each(drop);

            let mut blocks: Vec<OverlayBlock> = Vec::new();
            let mut stable: Vec<StableBlock> = Vec::new();
            // Çevirisi yazının altına konacak blokların (altyazıların) kimlikleri.
            let mut below: HashSet<String> = HashSet::new();
            let mut screen = None;
            match self.reader.read() {
                Ok(reading) => {
                    screen = reading.screen;
                    if waiting_for_window {
                        on_event(Event::WindowFound);
                    }
                    waiting_for_window = false;

                    // PC oyunlarında alan 16:9 olmayabilir (21:9 monitör, seçilen altyazı bandı).
                    let aspect = reading.video_width as f64 / reading.video_height.max(1) as f64;
                    let options = block_merger::Options { aspect, ..Default::default() };
                    let merged = block_merger::merge_with(&reading.lines, &options);
                    stable = stabilizer.update(&merged, elapsed());
                    // Odak modunda tür dışı yazılar hiç ele alınmaz (çevrilmez, üstüne çizilmez).
                    let kinds = focus::classify(&stable, &focus::Options { aspect, ..Default::default() });
                    let mut kinds = kinds.into_iter();
                    stable.retain(|block| {
                        let kind = kinds.next().unwrap_or(focus::Kind::Other);
                        if kind == focus::Kind::Dialogue {
                            below.insert(block.id.clone());
                        }
                        !self.config.focus || kind != focus::Kind::Other
                    });
                    blocks = compose(&self.config, &fetcher, &stable, &below, elapsed());

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
            let sent = delivery.send(&mut self.tv, blocks, screen, &mut on_event);

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
                if ready.recv_timeout(remaining.min(Duration::from_millis(50))).is_ok() && sent && !stable.is_empty() {
                    // Çeviri geldi: ekranı yeniden okumadan, son sabit bloklarla hemen gönder.
                    let blocks = compose(&self.config, &fetcher, &stable, &below, elapsed());
                    delivery.send(&mut self.tv, blocks, screen, &mut on_event);
                }
            }
        }
        if let Some(tv) = self.tv.as_mut() {
            tv.close();
        }
        on_event(Event::Frame { blocks: Vec::new(), screen: None });
    }
}

/// TV'ye gönderim ve "ne gösteriliyor" olayları; tur içinde birden çok kez çağrılabilir.
struct Delivery {
    tv_ok: Option<bool>,
    last_shown: Vec<String>,
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
        let shown: Vec<String> = blocks.iter().map(|b| b.text.clone()).collect();
        if sent.is_ok() && shown != self.last_shown {
            on_event(Event::Shown(shown.clone()));
            self.last_shown = shown;
        }
        on_event(Event::Frame { blocks, screen });
        sent.is_ok()
    }
}

/// Çeviri önbelleği ve arka plan isteklerinin ana döngüye haber verdiği kanallar.
struct Fetcher {
    translator: Arc<Mutex<Translator>>,
    cache_path: Option<PathBuf>,
    failures: Sender<TranslationError>,
    ready: Sender<()>,
}

/// Sabit bloklardan gösterilecek listeyi kurar: çevirisi hazır olanlar gider, eksikler arka
/// planda istenir.
fn compose(config: &Config, fetcher: &Fetcher, stable: &[StableBlock], below: &HashSet<String>, now: f64) -> Vec<OverlayBlock> {
    let Some(backend) = &config.backend else {
        // Çevirisiz test: kutuların yazıya oturduğu görülsün diye hep yazının üstüne çizilir.
        return stable.iter().map(|b| overlay_block(b, &b.text, false)).collect();
    };
    // Türkçe (PS5 arayüzü) ve yalnızca addan oluşan metin çevrilmez, üstüne de çizilmez:
    // orijinali görünür kalır.
    let terms = &config.terms;
    let english: Vec<&StableBlock> = stable.iter().filter(|b| !language::looks_turkish(&b.text) && !terms.is_only_terms(&b.text)).collect();
    // Önbellek ve DeepL işaretli hali görür; ad listesi değişince eski çeviri kullanılmaz.
    let keys: Vec<String> = english.iter().map(|b| terms.protect(&b.text)).collect();
    // Yaygın menü kelimeleri hazır sözlükten gelir; yalnızca kalanlar çeviri modeline sorulur.
    let ready: Vec<Option<&str>> = english.iter().map(|b| glossary::lookup(&b.text)).collect();
    let wanted: Vec<String> = keys.iter().zip(&ready).filter(|(_, hit)| hit.is_none()).map(|(key, _)| key.clone()).collect();
    let lookup = fetcher.translator.lock().unwrap().lookup(&wanted, now);
    if !lookup.to_fetch.is_empty() {
        spawn_fetch(fetcher, lookup.to_fetch, now, backend.clone());
    }
    english
        .iter()
        .zip(keys.iter().zip(&ready))
        .filter_map(|(b, (key, hit))| {
            let text = match hit {
                Some(text) => text.to_string(),
                None => terms::unprotect(lookup.found.get(key)?),
            };
            Some(overlay_block(b, &text, config.below && below.contains(&b.id)))
        })
        .collect()
}

fn overlay_block(block: &StableBlock, text: &str, below: bool) -> OverlayBlock {
    let b = if below { focus::beside(block.bbox, block.line_height) } else { block.bbox };
    OverlayBlock { id: block.id.clone(), x: b.x, y: b.y, w: b.width, h: b.height, line_h: block.line_height, text: text.to_string() }
}

/// Eksik çevirileri arka planda ister; ana döngü bu sırada okumaya devam eder.
fn spawn_fetch(fetcher: &Fetcher, texts: Vec<String>, now: f64, backend: Backend) {
    let translator = fetcher.translator.clone();
    let (cache_path, failures, ready) = (fetcher.cache_path.clone(), fetcher.failures.clone(), fetcher.ready.clone());
    std::thread::spawn(move || match translator::fetch(&translator, &texts, now, |batch| backend(batch)) {
        Ok(()) => {
            // Önce haber ver: diske yazma gösterimi geciktirmesin.
            let _ = ready.send(());
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
            Ok(Reading { lines: self.lines.clone(), video_width: 1920, video_height: 1080, fullscreen: true, capture_ms: 1, ocr_ms: 1, screen: None })
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
}
