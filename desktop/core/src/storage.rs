//! Kalıcı dosyalar: DeepL anahtarı, çeviri önbellekleri ve yerel çeviri modeli.
//! Konum: Mac'te `~/Library/Application Support/PS5Ceviri`, Windows'ta `%APPDATA%\PS5Ceviri`
//! (Mac'teki Swift aracıyla aynı klasör; mevcut anahtar ve önbellek kullanılmaya devam eder).

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};

/// `CEVIRI_DATA_DIR` verilmişse o klasör (testler ve geliştirme için).
pub fn app_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CEVIRI_DATA_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    dirs::data_dir().map(|dir| dir.join("PS5Ceviri"))
}

pub fn settings_path() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("settings.json"))
}

/// Anahtarı dosyaya yazar; Mac/Linux'ta yalnızca kullanıcı okuyabilir (600).
pub fn save_deepl_key(key: &str) -> io::Result<()> {
    let path = deepl_key_path().ok_or_else(|| io::Error::other("uygulama klasörü bulunamadı"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, key.trim())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn deepl_key_path() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("deepl-key.txt"))
}

/// DeepL çevirileri.
pub fn cache_path() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("translations.json"))
}

/// Yerel modelin çevirileri; DeepL önbelleğine karışmaz (DeepL'e dönülünce daha iyi çeviriler kalır).
pub fn local_cache_path() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("translations-local.json"))
}

/// İndirilen yerel çeviri modelinin klasörü.
pub fn local_model_dir() -> Option<PathBuf> {
    app_dir().map(|dir| dir.join("models").join("opus-mt-en-tr-int8"))
}

/// Önce `DEEPL_AUTH_KEY` ortam değişkeni, yoksa anahtar dosyası.
pub fn deepl_key() -> Option<String> {
    if let Ok(key) = std::env::var("DEEPL_AUTH_KEY") {
        if !key.trim().is_empty() {
            return Some(key.trim().to_string());
        }
    }
    let key = std::fs::read_to_string(deepl_key_path()?).ok()?;
    let key = key.trim();
    (!key.is_empty()).then(|| key.to_string())
}

/// Dosya yoksa ya da bozuksa boş önbellek.
pub fn load_cache(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path).ok().and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default()
}

/// Yerel modelin başlangıç önbelleği: kendi çevirileri, üstüne DeepL'inkiler (aynı metin için
/// DeepL çevirisi daha iyidir ve bekletmeden gelir).
pub fn load_local_cache(local: &Path, deepl: Option<&Path>) -> HashMap<String, String> {
    let mut cache = load_cache(local);
    if let Some(deepl) = deepl {
        cache.extend(load_cache(deepl));
    }
    cache
}

/// Sıralı ve okunaklı JSON; yarım kalmış yazım önbelleği bozmasın diye önce geçici dosyaya yazılır.
pub fn save_cache(path: &Path, cache: &HashMap<String, String>) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let sorted: BTreeMap<_, _> = cache.iter().collect();
    let json = serde_json::to_string_pretty(&sorted).map_err(io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, json)?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ceviri-core-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn onbellek_kaydedilip_geri_okunur() {
        let dir = temp_dir("cache");
        let path = dir.join("alt").join("translations.json");
        let cache = HashMap::from([("Door is locked".to_string(), "Kapı kilitli".to_string())]);

        save_cache(&path, &cache).unwrap();

        assert_eq!(load_cache(&path), cache);
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn yerel_onbellekte_deepl_cevirisi_once_gelir() {
        let dir = temp_dir("local");
        let local = dir.join("translations-local.json");
        let deepl = dir.join("translations.json");
        save_cache(&local, &HashMap::from([("Back".into(), "Geri dön".into()), ("Jump".into(), "Zıpla".into())])).unwrap();
        save_cache(&deepl, &HashMap::from([("Back".into(), "Geri".into())])).unwrap();

        let cache = load_local_cache(&local, Some(&deepl));

        assert_eq!(cache, HashMap::from([("Back".into(), "Geri".into()), ("Jump".into(), "Zıpla".into())]));
        assert_eq!(load_local_cache(&local, None).len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn swift_surumunun_yazdigi_onbellek_okunur() {
        // Swift JSONEncoder .prettyPrinted çıktısı ("anahtar" : "değer").
        let dir = temp_dir("swift");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("translations.json");
        std::fs::write(&path, "{\n  \"Options\" : \"Seçenekler\"\n}").unwrap();

        assert_eq!(load_cache(&path), HashMap::from([("Options".to_string(), "Seçenekler".to_string())]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn eksik_ya_da_bozuk_dosya_bos_onbellektir() {
        let dir = temp_dir("broken");
        assert!(load_cache(&dir.join("yok.json")).is_empty());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bozuk.json"), "{bozuk").unwrap();
        assert!(load_cache(&dir.join("bozuk.json")).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn uygulama_klasoru_platforma_gore_secilir() {
        let dir = app_dir().unwrap();
        assert!(dir.ends_with("PS5Ceviri"));
        if cfg!(target_os = "macos") {
            assert!(dir.to_string_lossy().contains("Library/Application Support"));
        }
    }
}
