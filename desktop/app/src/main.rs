//! Masaüstü uygulaması (Tauri). Bu dosya yalnızca kabuktur: komutlar `Controller`'a gider,
//! iş hattı olayları "ceviri" adıyla arayüze yayınlanır.
//!
//! Mac'te ana thread Tauri'nin olay döngüsünü çalıştırır; ekran okuma (Swift köprüsü) iş hattının
//! kendi thread'inde yapılır. Swift tarafının ihtiyaç duyduğu düzen budur.

// Windows'ta sürüm derlemesinde arkada konsol penceresi açılmasın.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod controller;
mod overlay;
mod region;
mod setup;
mod shortcuts;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ceviri_core::{storage, terms, Rect, Terms};
use ceviri_engine::pipeline::Backend;
use ceviri_engine::DeepLClient;
use ceviri_opus::{model, OpusTranslator};
use controller::{BackendFactory, Caches, Controller, Engine, EventSink, ReaderFactory, Settings, Source, Status, UiEvent};
use serde::Serialize;
use overlay::Overlay;
use shortcuts::Action;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

const SAMPLE: &str = "The door is locked.";

#[tauri::command]
fn status(controller: State<Controller>) -> Status {
    controller.status()
}

#[tauri::command]
fn save_settings(controller: State<Controller>, settings: Settings) -> Result<(), String> {
    controller.update_settings(settings.clone())?;
    settings.save()
}

#[tauri::command]
fn start(controller: State<Controller>) -> Result<(), String> {
    controller.start()
}

#[tauri::command]
fn stop(controller: State<Controller>) {
    controller.stop()
}

/// Anahtarı kaydeder ve örnek bir cümleyle dener; başarıda çeviriyi döner. Anahtar hiçbir yere
/// yazdırılmaz.
#[tauri::command]
async fn save_deepl_key(key: String) -> Result<String, String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Anahtar boş.".into());
    }
    storage::save_deepl_key(&key).map_err(|e| format!("Anahtar kaydedilemedi: {e}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let translated = DeepLClient::new(key).translate(&[SAMPLE.to_string()]).map_err(|e| format!("Anahtar kaydedildi ama DeepL denemesi başarısız: {e}"))?;
        Ok(format!("{SAMPLE} → {}", translated.first().map(String::as_str).unwrap_or("")))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Clone, Serialize)]
struct ModelProgress {
    done: u64,
    total: u64,
}

/// Aynı anda tek indirme.
static DOWNLOADING: AtomicBool = AtomicBool::new(false);

/// Yerel çeviri modelini indirir (ilerleme "model" olayıyla gelir), sonra örnek cümleyle dener.
#[tauri::command]
async fn download_local_model(app: tauri::AppHandle) -> Result<String, String> {
    let dir = storage::local_model_dir().ok_or("uygulama klasörü bulunamadı")?;
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return Err("İndirme zaten sürüyor.".into());
    }
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut last_percent = u64::MAX;
        model::download(&dir, |done, total| {
            // Her bayt parçasında değil, yüzde değiştikçe.
            let percent = done * 100 / total.max(1);
            if percent != last_percent {
                last_percent = percent;
                let _ = app.emit_to("main", "model", ModelProgress { done, total });
            }
        })?;
        let opus = OpusTranslator::load(&dir)?;
        let translated = opus.translate(&[Terms::new(&[]).protect(SAMPLE)]).map_err(|e| format!("Model indirildi ama deneme çevirisi başarısız: {e}"))?;
        Ok(format!("{SAMPLE} → {}", terms::unprotect(translated.first().map(String::as_str).unwrap_or(""))))
    })
    .await
    .map_err(|e| e.to_string());
    DOWNLOADING.store(false, Ordering::SeqCst);
    result?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GameWindow {
    exe: String,
    title: String,
    width: u32,
    height: u32,
}

/// Bu uygulamanın kendisi ve oyun olamayacak sistem pencereleri listede gösterilmez.
const NOT_GAMES: &[&str] = &["anlik-ceviri", "explorer.exe", "textinputhost.exe", "applicationframehost.exe", "searchhost.exe", "shellexperiencehost.exe", "startmenuexperiencehost.exe", "systemsettings.exe"];

/// PC oyunu seçimi için ekrandaki pencereler (exe adına göre sıralı).
#[tauri::command]
async fn list_windows() -> Result<Vec<GameWindow>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut windows: Vec<GameWindow> = ceviri_platform::list_windows()?
            .into_iter()
            .filter(|w| w.width >= 300 && w.height >= 200 && !w.title.is_empty())
            .filter(|w| !NOT_GAMES.iter().any(|name| w.app.to_lowercase().starts_with(name)) && !w.title.starts_with("Anlık Çeviri"))
            .map(|w| GameWindow { exe: w.app, title: w.title, width: w.width, height: w.height })
            .collect();
        windows.sort_by_key(|w| (w.exe.to_lowercase(), w.title.clone()));
        Ok(windows)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Seçili oyunun okunan bölgesi (`None`: tüm alan).
#[tauri::command]
fn current_region(controller: State<Controller>) -> Option<Rect> {
    controller.region()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegionChanged {
    region: Option<Rect>,
    regions: std::collections::BTreeMap<String, Rect>,
}

/// Bölge seçme penceresinden: bölgeyi kaydeder (çalışıyorsa hemen uygulanır) ve pencereyi kapatır.
/// Pencere işleri ana thread'i beklettiği için komutlar async (bkz. `region`).
#[tauri::command]
async fn set_region(app: AppHandle, region: Option<Rect>) -> Result<(), String> {
    let controller = app.state::<Controller>();
    let settings = controller.set_region(region)?;
    settings.save()?;
    let _ = app.emit_to("main", "region", RegionChanged { region: controller.region(), regions: settings.regions });
    close_region_to_game(&app);
    Ok(())
}

#[tauri::command]
async fn close_region(app: AppHandle) {
    close_region_to_game(&app);
}

/// Bölge penceresi kapanınca odak oyuna döner (yoksa Windows ana pencereyi öne getirebilir).
/// Önce oyun öne alınır: o an öndeki uygulama biz olduğumuz için Windows buna izin verir.
fn close_region_to_game(app: &AppHandle) {
    ceviri_platform::focus_game(app.state::<Controller>().status().settings.window.trim());
    region::close(app);
}

/// Bölge seçme penceresini oyunun üstünde açar (oyun ekranda olmalı).
#[tauri::command]
async fn open_region(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || open_region_over_game(&app, &app.state::<Controller>())).await.map_err(|e| e.to_string())?
}

fn open_region_over_game(app: &AppHandle, controller: &Controller) -> Result<(), String> {
    let window = controller.status().settings.window.trim().to_string();
    if window.is_empty() {
        return Err("Önce oyun penceresini seçin.".into());
    }
    let area = ceviri_platform::game_client_area(&window)
        .ok_or_else(|| format!("{window} penceresi ekranda bulunamadı. Oyunu açın (simge durumunda olmasın) ve tekrar deneyin."))?;
    region::open(app, area)
}

fn on_shortcut(app: &AppHandle, action: Action) {
    match action {
        Action::ToggleOverlay => {
            let hidden = app.state::<Arc<Overlay>>().toggle();
            let _ = app.emit_to("main", "overlay-hidden", hidden);
        }
        // Kısayol işleyicisi ana thread'de çalışır; pencere ayrı thread'den açılır (bkz. `region`).
        Action::SelectRegion => {
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(message) = open_region_over_game(&app, &app.state::<Controller>()) {
                    let _ = app.emit_to("main", "ceviri", UiEvent::Error { message });
                }
            });
        }
    }
}

/// İş hattı PC oyununu okumaya başlayınca kısayolları kaydeder, durunca bırakır.
fn sync_shortcuts(app: &AppHandle, state: &str) {
    let pc_game = app.try_state::<Controller>().is_some_and(|c| c.status().settings.source == Source::PcGame);
    match state {
        "running" if pc_game => {
            app.state::<Arc<Overlay>>().set_hidden(false);
            let _ = app.emit_to("main", "overlay-hidden", false);
            if let Err(message) = shortcuts::register(app) {
                let _ = app.emit_to("main", "ceviri", UiEvent::Error { message });
            }
        }
        "stopped" => shortcuts::unregister(app),
        _ => {}
    }
}

fn local_backend() -> Result<Backend, String> {
    let dir = storage::local_model_dir().ok_or("uygulama klasörü bulunamadı")?;
    if !model::is_installed(&dir) {
        return Err("Yerel çeviri modeli indirilmemiş. Ayarlar'dan indirin ya da DeepL'i seçin.".into());
    }
    let opus = OpusTranslator::load(&dir)?;
    Ok(Arc::new(move |texts: &[String]| opus.translate(texts)))
}

fn deepl_backend() -> Result<Backend, String> {
    let key = storage::deepl_key().ok_or("DeepL anahtarı yok. Ayarlardan anahtarınızı ekleyin, yerel modeli seçin ya da çevirisiz test modunu deneyin.")?;
    let client = DeepLClient::new(key);
    Ok(Arc::new(move |texts: &[String]| client.translate(texts)))
}

fn main() {
    ceviri_platform::prepare_process();
    tauri::Builder::default()
        .plugin(shortcuts::plugin(on_shortcut))
        .setup(|app| {
            let handle = app.handle().clone();
            let overlay = Arc::new(Overlay::new(handle.clone()));
            app.manage(overlay.clone());
            // Overlay kareleri overlay penceresine, gerisi ana pencereye.
            let emit: EventSink = Arc::new(move |event| match event {
                UiEvent::Frame { blocks, screen } => overlay.show(&blocks, screen),
                other => {
                    if let UiEvent::State { state, .. } = &other {
                        sync_shortcuts(&handle, state);
                    }
                    let _ = handle.emit_to("main", "ceviri", other);
                }
            });
            let make_reader: ReaderFactory = Arc::new(|settings: &Settings, region| {
                let window = settings.window.trim();
                match settings.source {
                    Source::RemotePlay => ceviri_platform::screen_reader((!window.is_empty()).then_some(window), settings.fps, None),
                    Source::PcGame => ceviri_platform::game_reader(window, region.clone()),
                }
            });
            let make_backend: BackendFactory = Arc::new(|engine| match engine {
                Engine::Local => local_backend(),
                Engine::Deepl => deepl_backend(),
            });
            let caches = Caches { deepl: storage::cache_path(), local: storage::local_cache_path() };
            let controller = Controller::new(Settings::load(), emit, make_reader, make_backend, caches);
            // Geliştirme: arayüze dokunmadan iş hattını denemek için.
            if std::env::var_os("CEVIRI_AUTOSTART").is_some() {
                let _ = controller.start();
            }
            app.manage(controller);
            // Terminalden başlatılınca da pencere öne gelsin (Finder'dan açılınca zaten gelir).
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
            Ok(())
        })
        // Ana pencere kapanınca uygulama da kapansın (overlay penceresi açık kalsa bile).
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, WindowEvent::CloseRequested { .. }) {
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            status,
            save_settings,
            start,
            stop,
            save_deepl_key,
            download_local_model,
            list_windows,
            current_region,
            set_region,
            close_region,
            open_region,
            setup::setup_status,
            setup::request_screen_recording,
            setup::open_privacy_settings,
            setup::open_link,
            setup::tv_scan,
            setup::tv_install,
            setup::tv_test,
            setup::tv_uninstall,
            setup::finish_setup,
        ])
        .run(tauri::generate_context!())
        .expect("uygulama başlatılamadı");
}
