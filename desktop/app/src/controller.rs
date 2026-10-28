//! Uygulamanın Tauri'den bağımsız çekirdeği: ayarlar, başlat/durdur, arayüze giden olaylar.
//! Okuyucu ve çeviri arka ucu dışarıdan verilir; gerçekte platform okuyucusu ve yerel model ya da
//! DeepL, testlerde sahteleri.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ceviri_core::geometry::clamp_region;
use ceviri_core::{storage, OverlayBlock, Rect, ScreenReader, SharedRegion, Terms};
use ceviri_engine::pipeline::Backend;
use ceviri_engine::{discovery, tv_client, Config, Event, Pipeline, TvClient};
use serde::{Deserialize, Serialize};

/// Çevirilerin nerede gösterileceği.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Output {
    /// Android TV'deki overlay (HDMI görüntünün üstü).
    #[default]
    Tv,
    /// Bu bilgisayarın ekranında, oyun penceresinin üstünde.
    Screen,
    Both,
}

impl Output {
    pub fn tv(self) -> bool {
        matches!(self, Output::Tv | Output::Both)
    }

    pub fn screen(self) -> bool {
        matches!(self, Output::Screen | Output::Both)
    }
}

/// Yazıların okunduğu yer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// PS5 görüntüsü, PS Remote Play penceresinden.
    #[default]
    RemotePlay,
    /// Bu bilgisayarda oynanan oyunun penceresi (PS5'ten bağımsız; yalnızca Windows). Çeviriler
    /// her zaman bu ekranda gösterilir.
    PcGame,
}

/// Çeviriyi kim yapar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Engine {
    /// Bu bilgisayarda çalışan Opus-MT modeli: çevrimdışı, ücretsiz, cümle başına ~0,1 sn.
    #[default]
    Local,
    /// DeepL API: en iyi kalite, anahtar ve internet gerekir.
    Deepl,
}

impl Engine {
    pub fn label(self) -> &'static str {
        match self {
            Engine::Local => "yerel model",
            Engine::Deepl => "DeepL",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub source: Source,
    pub output: Output,
    /// Boşsa TV yerel ağda aranır.
    pub tv_address: String,
    /// `false`: çevirisiz test modu (İngilizce metin olduğu gibi gider, kota harcamaz).
    pub translate: bool,
    pub engine: Engine,
    pub fps: f64,
    /// Boşsa PS Remote Play. Mac'te bundle ID, Windows'ta başlıkta/exe adında geçen metin. PC
    /// oyununda zorunlu (genelde exe adı).
    pub window: String,
    /// PC oyununda okunacak bölge, oyuna göre (`region_key`): istemci alanına göre 0...1. Yoksa tüm alan.
    pub regions: BTreeMap<String, Rect>,
    /// Çevrilmeyecek adlar (oyun, karakter); PlayStation, PS5 vb. her zaman korunur.
    pub protected_terms: Vec<String>,
    /// İlk kurulum sihirbazı tamamlandı mı (değilse uygulama açılınca sihirbaz gösterilir).
    pub setup_done: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            source: Source::RemotePlay,
            output: Output::Tv,
            tv_address: String::new(),
            translate: true,
            engine: Engine::Local,
            fps: 2.0,
            window: String::new(),
            regions: BTreeMap::new(),
            protected_terms: Vec::new(),
            setup_done: false,
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        let json = storage::settings_path().and_then(|path| std::fs::read_to_string(path).ok());
        Settings::from_json(json.as_deref(), storage::deepl_key().is_some())
    }

    /// Yerel model eklenmeden önce kaydedilmiş ayarlarda `engine` yoktur: DeepL anahtarı olan
    /// kullanıcı, alıştığı DeepL'de kalır.
    fn from_json(json: Option<&str>, has_deepl_key: bool) -> Settings {
        let Some(value) = json.and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok()) else {
            return Settings::default();
        };
        let mut settings: Settings = serde_json::from_value(value.clone()).unwrap_or_default();
        if value.get("engine").is_none() && has_deepl_key {
            settings.engine = Engine::Deepl;
        }
        settings
    }

    /// Bölgelerin saklandığı anahtar: seçili oyun penceresi.
    pub fn region_key(&self) -> String {
        self.window.trim().to_lowercase()
    }

    /// PC oyununda çeviriler hep bu ekranda gösterilir; TV kullanılmaz.
    pub fn effective_output(&self) -> Output {
        match self.source {
            Source::RemotePlay => self.output,
            Source::PcGame => Output::Screen,
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = storage::settings_path().ok_or("uygulama klasörü bulunamadı")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self).unwrap()).map_err(|e| e.to_string())
    }
}

/// Arayüze giden olay (JSON'da `type` alanıyla ayrılır).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum UiEvent {
    /// "stopped" | "starting" | "running"; `detail` kullanıcıya gösterilecek açıklama.
    State { state: String, detail: String },
    Tv { connected: bool, address: String, error: Option<String> },
    Window { found: bool, message: String },
    Shown { texts: Vec<String> },
    Round { video: String, capture_ms: u64, ocr_ms: u64, blocks: usize, sent: usize },
    Error { message: String },
    /// Ekran overlay'ine: gösterilecek bloklar ve video alanının ekrandaki yeri (yoksa gizle).
    Frame { blocks: Vec<OverlayBlock>, screen: Option<ScreenRect> },
}

/// Mac'te nokta (mantıksal birim), Windows'ta fiziksel piksel; origin ana ekranın sol üstü.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub type EventSink = Arc<dyn Fn(UiEvent) + Send + Sync>;
/// Okuyucuyu kurar; PC oyunu okuyucusu bölgeyi her turda paylaşılan değerden okur.
pub type ReaderFactory = Arc<dyn Fn(&Settings, &SharedRegion) -> Result<Box<dyn ScreenReader>, String> + Send + Sync>;
/// Çeviri arka ucunu kurar (yerel modeli yükler ya da DeepL anahtarını okur); model ya da anahtar
/// yoksa hata.
pub type BackendFactory = Arc<dyn Fn(Engine) -> Result<Backend, String> + Send + Sync>;

/// Çeviri önbellek dosyaları; yerel modelinkiler DeepL'inkilere karışmaz.
#[derive(Clone, Debug, Default)]
pub struct Caches {
    pub deepl: Option<PathBuf>,
    pub local: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    pub settings: Settings,
    pub has_deepl_key: bool,
    pub has_local_model: bool,
    /// İndirilecek modelin boyutu (MB).
    pub local_model_mb: u64,
    /// Bu bilgisayarda oynanan oyun okunabilir mi (yalnızca Windows).
    pub game_source: bool,
}

pub struct Controller {
    inner: Arc<Inner>,
}

struct Inner {
    settings: Mutex<Settings>,
    /// Çalışan iş hattının durdurma bayrağı; `None` ise duruyor.
    running: Mutex<Option<Arc<AtomicBool>>>,
    emit: EventSink,
    make_reader: ReaderFactory,
    make_backend: BackendFactory,
    caches: Caches,
    /// Çalışan okuyucunun bölgesi; `set_region` çalışırken de değiştirir.
    region: SharedRegion,
}

impl Controller {
    pub fn new(settings: Settings, emit: EventSink, make_reader: ReaderFactory, make_backend: BackendFactory, caches: Caches) -> Controller {
        Controller {
            inner: Arc::new(Inner {
                settings: Mutex::new(settings),
                running: Mutex::new(None),
                emit,
                make_reader,
                make_backend,
                caches,
                region: SharedRegion::default(),
            }),
        }
    }

    pub fn status(&self) -> Status {
        Status {
            running: self.inner.running.lock().unwrap().is_some(),
            settings: self.inner.settings.lock().unwrap().clone(),
            has_deepl_key: storage::deepl_key().is_some(),
            has_local_model: storage::local_model_dir().is_some_and(|dir| ceviri_opus::model::is_installed(&dir)),
            local_model_mb: ceviri_opus::model::total_size().div_ceil(1_000_000),
            game_source: ceviri_platform::GAME_SOURCE_SUPPORTED,
        }
    }

    /// Ayarlar yalnızca dururken değişir; çalışırken değiştirmek yarım kalmış bir oturum bırakırdı.
    pub fn update_settings(&self, settings: Settings) -> Result<(), String> {
        if self.inner.running.lock().unwrap().is_some() {
            return Err("Ayarları değiştirmek için önce durdurun.".into());
        }
        *self.inner.settings.lock().unwrap() = settings;
        Ok(())
    }

    /// Seçili oyunun bölgesi (yoksa tüm alan).
    pub fn region(&self) -> Option<Rect> {
        let settings = self.inner.settings.lock().unwrap();
        settings.regions.get(&settings.region_key()).copied()
    }

    /// Seçili oyunun bölgesini değiştirir (`None`: tüm alan). Çalışırken de olur: okuyucu bir
    /// sonraki turda yeni bölgeyi okur. Kaydedilecek güncel ayarları döner.
    pub fn set_region(&self, region: Option<Rect>) -> Result<Settings, String> {
        let mut settings = self.inner.settings.lock().unwrap();
        let key = settings.region_key();
        if key.is_empty() {
            return Err("Önce oyun penceresini seçin.".into());
        }
        let region = region.and_then(clamp_region);
        match region {
            Some(region) => settings.regions.insert(key, region),
            None => settings.regions.remove(&key),
        };
        *self.inner.region.lock().unwrap() = region;
        Ok(settings.clone())
    }

    /// İş hattını kendi thread'inde başlatır ve hemen döner; ilerleme olaylarla bildirilir.
    pub fn start(&self) -> Result<(), String> {
        let stop = {
            let mut running = self.inner.running.lock().unwrap();
            if running.is_some() {
                return Err("Zaten çalışıyor.".into());
            }
            let stop = Arc::new(AtomicBool::new(false));
            *running = Some(stop.clone());
            stop
        };
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let result = inner.run(stop.clone());
            *inner.running.lock().unwrap() = None;
            if let Err(message) = result {
                (inner.emit)(UiEvent::Error { message });
            }
            (inner.emit)(UiEvent::State { state: "stopped".into(), detail: "Durduruldu".into() });
        });
        Ok(())
    }

    /// Durdurma isteği verir ve hemen döner; iş hattı bitince "stopped" olayı gelir.
    pub fn stop(&self) {
        if let Some(stop) = self.inner.running.lock().unwrap().as_ref() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

impl Inner {
    fn run(&self, stop: Arc<AtomicBool>) -> Result<(), String> {
        let settings = self.settings.lock().unwrap().clone();
        let state = |state: &str, detail: &str| (self.emit)(UiEvent::State { state: state.into(), detail: detail.into() });

        let output = settings.effective_output();
        if settings.source == Source::PcGame && settings.region_key().is_empty() {
            return Err("Oyun penceresi seçilmemiş. Ayarlar'dan oyunu seçin (oyun açıkken listede görünür).".into());
        }
        *self.region.lock().unwrap() = settings.regions.get(&settings.region_key()).copied();

        let address = if !output.tv() {
            None
        } else if settings.tv_address.trim().is_empty() {
            state("starting", "TV yerel ağda aranıyor…");
            Some(
                discovery::find_tv(Duration::from_secs(5))
                    .ok_or("TV bulunamadı. TV'de uygulama açık mı? Ayarlardan TV adresini elle girebilirsiniz.")?,
            )
        } else {
            Some(tv_client::with_default_port(settings.tv_address.trim()))
        };
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }

        let mut config = Config { fps: settings.fps, terms: Terms::new(&settings.protected_terms), ..Config::default() };
        if settings.translate {
            if settings.engine == Engine::Local {
                state("starting", "Çeviri modeli yükleniyor…");
            }
            config.backend = Some((self.make_backend)(settings.engine)?);
            let deepl = self.caches.deepl.clone();
            (config.cache_path, config.cache) = match settings.engine {
                Engine::Deepl => (deepl.clone(), deepl.as_deref().map(storage::load_cache).unwrap_or_default()),
                Engine::Local => {
                    let local = self.caches.local.clone();
                    let cache = local.as_deref().map(|path| storage::load_local_cache(path, deepl.as_deref())).unwrap_or_default();
                    (local, cache)
                }
            };
        }

        state("starting", "Ekran okuma hazırlanıyor…");
        let reader = (self.make_reader)(&settings, &self.region)?;
        let detail = if settings.translate { format!("Türkçe çeviri ({})", settings.engine.label()) } else { "Çevirisiz test modu".into() };
        state("running", &detail);

        let emit = self.emit.clone();
        let tv_url = address.as_ref().map(|a| format!("ws://{a}")).unwrap_or_default();
        let screen_output = output.screen();
        let mut pipeline = Pipeline::new(reader, address.map(TvClient::new), config).with_stop(stop);
        pipeline.run(move |event| {
            let ui = match event {
                Event::Frame { blocks, screen } if screen_output => UiEvent::Frame {
                    blocks,
                    screen: screen.map(|r| ScreenRect { x: r.x, y: r.y, width: r.width, height: r.height }),
                },
                Event::Frame { .. } => return,
                Event::Round(s) => UiEvent::Round {
                    video: format!("{}x{}", s.video_width, s.video_height),
                    capture_ms: s.capture_ms,
                    ocr_ms: s.ocr_ms,
                    blocks: s.merged,
                    sent: s.sent,
                },
                Event::WaitingForWindow(message) => UiEvent::Window { found: false, message },
                Event::WindowFound => UiEvent::Window { found: true, message: "Pencere bulundu".into() },
                Event::Shown(texts) => UiEvent::Shown { texts },
                Event::TvConnected => UiEvent::Tv { connected: true, address: tv_url.clone(), error: None },
                Event::TvUnreachable(error) => UiEvent::Tv { connected: false, address: tv_url.clone(), error: Some(error) },
                Event::TranslationFailed(error) => UiEvent::Error { message: format!("Çeviri hatası ({}): {error}", settings.engine.label()) },
                Event::RoundSkipped(message) => UiEvent::Error { message },
            };
            emit(ui);
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Instant;

    use ceviri_core::{OverlayMessage, ReadError, Reading, Rect, TextLine};
    use tungstenite::Message;

    use super::*;

    struct FakeReader;

    impl ScreenReader for FakeReader {
        fn read(&mut self) -> Result<Reading, ReadError> {
            let line = TextLine { text: "Play Local".into(), bbox: Rect::new(0.1, 0.5, 0.1, 0.03), confidence: 0.9 };
            Ok(Reading { lines: vec![line], video_width: 1920, video_height: 1080, fullscreen: true, capture_ms: 1, ocr_ms: 1, screen: None })
        }
    }

    fn fake_tv() -> (String, mpsc::Receiver<OverlayMessage>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let Ok(mut socket) = tungstenite::accept(stream) else { continue };
                while let Ok(message) = socket.read() {
                    if let Message::Text(text) = message {
                        let _ = sender.send(serde_json::from_str(&text).unwrap());
                    }
                }
            }
        });
        (address, receiver)
    }

    fn controller(settings: Settings, backend: BackendFactory) -> (Controller, mpsc::Receiver<UiEvent>) {
        let (sender, events) = mpsc::channel();
        let sender = Mutex::new(sender);
        let emit: EventSink = Arc::new(move |event| {
            let _ = sender.lock().unwrap().send(event);
        });
        let make_reader: ReaderFactory = Arc::new(|_, _| Ok(Box::new(FakeReader)));
        (Controller::new(settings, emit, make_reader, backend, Caches::default()), events)
    }

    fn no_backend() -> BackendFactory {
        Arc::new(|_| Err("anahtar yok".into()))
    }

    fn wait_for(events: &mpsc::Receiver<UiEvent>, wanted: impl Fn(&UiEvent) -> bool) -> Vec<UiEvent> {
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(event) = events.recv_timeout(Duration::from_millis(100)) {
                let done = wanted(&event);
                seen.push(event);
                if done {
                    return seen;
                }
            }
        }
        panic!("beklenen olay gelmedi; gelenler: {seen:?}");
    }

    #[test]
    fn baslatilinca_tvye_gider_ve_durdurulunca_durur() {
        let (address, messages) = fake_tv();
        let settings = Settings { tv_address: address, translate: false, fps: 20.0, ..Settings::default() };
        let (controller, events) = controller(settings, no_backend());

        controller.start().unwrap();
        assert_eq!(controller.start(), Err("Zaten çalışıyor.".into()));
        let seen = wait_for(&events, |e| matches!(e, UiEvent::Shown { texts } if !texts.is_empty()));
        assert!(seen.contains(&UiEvent::State { state: "running".into(), detail: "Çevirisiz test modu".into() }));
        assert!(seen.iter().any(|e| matches!(e, UiEvent::Tv { connected: true, .. })));
        assert!(controller.status().running);
        assert!(messages.recv_timeout(Duration::from_secs(1)).is_ok());

        controller.stop();
        wait_for(&events, |e| matches!(e, UiEvent::State { state, .. } if state == "stopped"));
        assert!(!controller.status().running);
        // Tekrar başlatılabilir.
        controller.start().unwrap();
        controller.stop();
    }

    #[test]
    fn ceviri_modunda_anahtar_yoksa_hata_verip_durur() {
        let settings = Settings { tv_address: "127.0.0.1:1".into(), translate: true, ..Settings::default() };
        let (controller, events) = controller(settings, no_backend());

        controller.start().unwrap();
        let seen = wait_for(&events, |e| matches!(e, UiEvent::State { state, .. } if state == "stopped"));

        assert!(seen.contains(&UiEvent::Error { message: "anahtar yok".into() }));
        assert!(!controller.status().running);
    }

    #[test]
    fn ceviri_arka_ucu_kullanilir() {
        let (address, messages) = fake_tv();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let log = asked.clone();
        let backend: BackendFactory = Arc::new(move |engine| {
            log.lock().unwrap().push(engine);
            let translate: Backend = Arc::new(|texts: &[String]| Ok(texts.iter().map(|t| format!("TR({t})")).collect()));
            Ok(translate)
        });
        let settings = Settings { tv_address: address, translate: true, engine: Engine::Deepl, fps: 20.0, ..Settings::default() };
        let (controller, events) = controller(settings, backend);

        controller.start().unwrap();
        let seen = wait_for(&events, |e| matches!(e, UiEvent::Shown { texts } if texts == &["TR(Play Local)"]));
        controller.stop();

        assert!(messages.try_iter().any(|m| m.blocks.iter().any(|b| b.text == "TR(Play Local)")));
        assert_eq!(*asked.lock().unwrap(), [Engine::Deepl]);
        assert!(seen.contains(&UiEvent::State { state: "running".into(), detail: "Türkçe çeviri (DeepL)".into() }));
    }

    #[test]
    fn ekran_modunda_tv_aranmaz_ve_overlay_kareleri_gelir() {
        let settings = Settings { output: Output::Screen, translate: false, fps: 20.0, ..Settings::default() };
        let (controller, events) = controller(settings, no_backend());

        controller.start().unwrap();
        let seen = wait_for(&events, |e| matches!(e, UiEvent::Frame { blocks, .. } if !blocks.is_empty()));
        controller.stop();

        assert!(!seen.iter().any(|e| matches!(e, UiEvent::Tv { .. })), "TV olayı gelmemeli");
        assert!(seen.iter().any(|e| matches!(e, UiEvent::Shown { texts } if texts == &["Play Local"])));
        let last = wait_for(&events, |e| matches!(e, UiEvent::State { state, .. } if state == "stopped"));
        // Durunca overlay'e "gizle" (boş liste, konum yok) gider.
        assert!(last.contains(&UiEvent::Frame { blocks: vec![], screen: None }));
    }

    #[test]
    fn tv_modunda_overlay_karesi_gonderilmez() {
        let (address, _messages) = fake_tv();
        let settings = Settings { tv_address: address, translate: false, fps: 20.0, ..Settings::default() };
        let (controller, events) = controller(settings, no_backend());

        controller.start().unwrap();
        let seen = wait_for(&events, |e| matches!(e, UiEvent::Shown { texts } if !texts.is_empty()));
        controller.stop();

        assert!(!seen.iter().any(|e| matches!(e, UiEvent::Frame { .. })));
    }

    #[test]
    fn pc_oyununda_tv_kullanilmaz_ve_bolge_okuyucuya_gider() {
        let band = Rect::new(0.0, 0.7, 1.0, 0.3);
        let settings = Settings {
            source: Source::PcGame,
            output: Output::Tv,
            translate: false,
            fps: 20.0,
            window: "EldenRing.exe".into(),
            regions: BTreeMap::from([("eldenring.exe".to_string(), band)]),
            ..Settings::default()
        };
        let (sender, events) = mpsc::channel();
        let sender = Mutex::new(sender);
        let emit: EventSink = Arc::new(move |event| {
            let _ = sender.lock().unwrap().send(event);
        });
        let seen_region = Arc::new(Mutex::new(None));
        let log = seen_region.clone();
        let make_reader: ReaderFactory = Arc::new(move |settings, region| {
            assert_eq!(settings.source, Source::PcGame);
            *log.lock().unwrap() = Some(region.clone());
            Ok(Box::new(FakeReader))
        });
        let controller = Controller::new(settings, emit, make_reader, no_backend(), Caches::default());

        controller.start().unwrap();
        let seen = wait_for(&events, |e| matches!(e, UiEvent::Frame { blocks, .. } if !blocks.is_empty()));
        assert!(!seen.iter().any(|e| matches!(e, UiEvent::Tv { .. })), "PC oyununda TV aranmamalı");
        let region = seen_region.lock().unwrap().clone().expect("okuyucu kurulmalı");
        assert_eq!(*region.lock().unwrap(), Some(band));

        // Çalışırken bölge değişir; okuyucunun gördüğü değer de.
        let saved = controller.set_region(Some(Rect::new(0.25, 0.75, 0.5, 0.125))).unwrap();
        assert_eq!(*region.lock().unwrap(), Some(Rect::new(0.25, 0.75, 0.5, 0.125)));
        assert_eq!(saved.regions["eldenring.exe"], Rect::new(0.25, 0.75, 0.5, 0.125));
        // Çok küçük bölge ya da `None`: tüm alan.
        assert!(controller.set_region(Some(Rect::new(0.5, 0.5, 0.001, 0.001))).unwrap().regions.is_empty());
        assert_eq!(*region.lock().unwrap(), None);
        assert_eq!(controller.region(), None);
        controller.stop();
    }

    #[test]
    fn pc_oyununda_pencere_secilmeden_baslamaz() {
        let settings = Settings { source: Source::PcGame, translate: false, ..Settings::default() };
        let (controller, events) = controller(settings, no_backend());

        controller.start().unwrap();
        let seen = wait_for(&events, |e| matches!(e, UiEvent::State { state, .. } if state == "stopped"));

        assert!(seen.iter().any(|e| matches!(e, UiEvent::Error { message } if message.starts_with("Oyun penceresi seçilmemiş"))));
        assert!(controller.set_region(None).is_err());
    }

    #[test]
    fn calisirken_ayar_degismez() {
        let (address, _messages) = fake_tv();
        let settings = Settings { tv_address: address, translate: false, fps: 20.0, ..Settings::default() };
        let (controller, events) = controller(settings.clone(), no_backend());

        controller.start().unwrap();
        wait_for(&events, |e| matches!(e, UiEvent::State { state, .. } if state == "running"));

        assert!(controller.update_settings(Settings::default()).is_err());
        assert_eq!(controller.status().settings, settings);
        controller.stop();
    }

    #[test]
    fn olaylar_arayuz_formatinda_serilesir() {
        let json = serde_json::to_value(UiEvent::Round { video: "1920x1080".into(), capture_ms: 3, ocr_ms: 95, blocks: 4, sent: 2 }).unwrap();
        assert_eq!(json, serde_json::json!({"type": "round", "video": "1920x1080", "captureMs": 3, "ocrMs": 95, "blocks": 4, "sent": 2}));

        let settings: Settings = serde_json::from_str(r#"{"tvAddress":"192.168.1.5"}"#).unwrap();
        assert_eq!(settings, Settings { tv_address: "192.168.1.5".into(), ..Settings::default() });
        let both: Settings = serde_json::from_str(r#"{"output":"both"}"#).unwrap();
        assert_eq!(both.output, Output::Both);
        assert_eq!(serde_json::to_value(Settings::default()).unwrap()["engine"], "local");
        // Kaynak eklenmeden önceki ayarlar Remote Play'de kalır.
        assert_eq!(settings.source, Source::RemotePlay);
        let game: Settings = serde_json::from_str(r#"{"source":"pcGame","window":"game.exe","regions":{"game.exe":{"x":0,"y":0.7,"width":1,"height":0.3}}}"#).unwrap();
        assert_eq!(game.source, Source::PcGame);
        assert_eq!(game.regions["game.exe"], Rect::new(0.0, 0.7, 1.0, 0.3));
    }

    #[test]
    fn eski_ayarlarda_anahtar_varsa_deepl_kalir() {
        let old = r#"{"tvAddress":"192.168.1.5","translate":true}"#;

        assert_eq!(Settings::from_json(Some(old), true).engine, Engine::Deepl);
        assert_eq!(Settings::from_json(Some(old), false).engine, Engine::Local);
        assert_eq!(Settings::from_json(Some(r#"{"engine":"local"}"#), true).engine, Engine::Local);
        assert_eq!(Settings::from_json(None, true), Settings::default());
        assert_eq!(Settings::from_json(Some("{bozuk"), true), Settings::default());
    }
}
