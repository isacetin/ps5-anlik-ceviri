//! PC oyununda oyundan çıkmadan kullanılan global kısayollar. Yalnızca PC oyunu okunurken kayıtlıdır;
//! durunca bırakılır, böylece aynı tuşları kullanan başka uygulamalara (ör. tarayıcıda Ctrl+Shift+T)
//! oyun dışında karışılmaz.

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{Builder, Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Ctrl+Shift+T: çevirileri gizle/göster.
const TOGGLE: Code = Code::KeyT;
/// Ctrl+Shift+R: okunacak bölgeyi seç.
const REGION: Code = Code::KeyR;
const MODIFIERS: Modifiers = Modifiers::CONTROL.union(Modifiers::SHIFT);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ToggleOverlay,
    SelectRegion,
}

/// Kısayollar basılınca `on_action` çağrılır.
pub fn plugin(on_action: impl Fn(&AppHandle, Action) + Send + Sync + 'static) -> tauri::plugin::TauriPlugin<tauri::Wry> {
    Builder::new()
        .with_handler(move |app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            if shortcut.matches(MODIFIERS, TOGGLE) {
                on_action(app, Action::ToggleOverlay);
            } else if shortcut.matches(MODIFIERS, REGION) {
                on_action(app, Action::SelectRegion);
            }
        })
        .build()
}

/// Kısayolları kaydeder; başka bir uygulama aynı tuşları aldıysa hata döner.
pub fn register(app: &AppHandle) -> Result<(), String> {
    let shortcuts = [Shortcut::new(Some(MODIFIERS), TOGGLE), Shortcut::new(Some(MODIFIERS), REGION)];
    app.global_shortcut()
        .register_multiple(shortcuts)
        .map_err(|e| format!("Kısayollar (Ctrl+Shift+T, Ctrl+Shift+R) kaydedilemedi; başka bir uygulama kullanıyor olabilir: {e}"))
}

pub fn unregister(app: &AppHandle) {
    let _ = app.global_shortcut().unregister_all();
}
