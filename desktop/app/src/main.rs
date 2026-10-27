//! Masaüstü uygulaması (Tauri). Bu dosya yalnızca kabuktur: komutlar `Controller`'a gider,
//! iş hattı olayları "ceviri" adıyla arayüze yayınlanır.
//!
//! Mac'te ana thread Tauri'nin olay döngüsünü çalıştırır; ekran okuma (Swift köprüsü) iş hattının
//! kendi thread'inde yapılır. Swift tarafının ihtiyaç duyduğu düzen budur.

// Windows'ta sürüm derlemesinde arkada konsol penceresi açılmasın.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod controller;
mod overlay;
mod setup;

use std::sync::Arc;

use ceviri_core::storage;
use ceviri_engine::pipeline::Backend;
use ceviri_engine::DeepLClient;
use controller::{BackendFactory, Controller, EventSink, ReaderFactory, Settings, Status, UiEvent};
use overlay::Overlay;
use tauri::{Emitter, Manager, State, WindowEvent};

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

fn main() {
    ceviri_platform::prepare_process();
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let overlay = Overlay::new(handle.clone());
            // Overlay kareleri overlay penceresine, gerisi ana pencereye.
            let emit: EventSink = Arc::new(move |event| match event {
                UiEvent::Frame { blocks, screen } => overlay.show(&blocks, screen),
                other => {
                    let _ = handle.emit_to("main", "ceviri", other);
                }
            });
            let make_reader: ReaderFactory = Arc::new(|settings: &Settings| {
                let window = settings.window.trim();
                ceviri_platform::screen_reader((!window.is_empty()).then_some(window), settings.fps, None)
            });
            let make_backend: BackendFactory = Arc::new(|| {
                let key = storage::deepl_key().ok_or("DeepL anahtarı yok. Ayarlardan anahtarınızı ekleyin ya da çevirisiz test modunu seçin.")?;
                let client = DeepLClient::new(key);
                let backend: Backend = Arc::new(move |texts: &[String]| client.translate(texts));
                Ok(backend)
            });
            let controller = Controller::new(Settings::load(), emit, make_reader, make_backend, storage::cache_path());
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
