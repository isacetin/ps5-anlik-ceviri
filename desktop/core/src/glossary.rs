//! Yaygın oyun menüsü kelimelerinin hazır çevirisi. Tek kelimelik menü öğelerinde çeviri modeli
//! bağlamı bilemez ("Resume" → "Özgeçmiş", "Save" → "Kurtar"); bunlar modele hiç gönderilmez.

/// Menü öğesinin hazır çevirisi; listede yoksa `None` (çeviri modeline gider).
pub fn lookup(text: &str) -> Option<&'static str> {
    let key = normalize(text);
    ENTRIES.iter().find(|(english, _)| *english == key).map(|(_, turkish)| *turkish)
}

/// Küçük harf, noktalama yok; sondaki tek harflik parçalar atılır (OCR düğme simgesini harf
/// sanar: "Resume ⊗" → "Resume x").
fn normalize(text: &str) -> String {
    let cleaned: String = text.chars().map(|c| if c.is_alphanumeric() || c == '\'' { c.to_ascii_lowercase() } else { ' ' }).collect();
    let mut words: Vec<&str> = cleaned.split_whitespace().collect();
    while words.len() > 1 && words.last().is_some_and(|w| w.chars().count() == 1) {
        words.pop();
    }
    words.join(" ")
}

const ENTRIES: &[(&str, &str)] = &[
    ("resume", "Devam Et"),
    ("resume game", "Oyuna Devam Et"),
    ("continue", "Devam Et"),
    ("new game", "Yeni Oyun"),
    ("start", "Başla"),
    ("start game", "Oyunu Başlat"),
    ("play", "Oyna"),
    ("load", "Yükle"),
    ("load game", "Oyun Yükle"),
    ("save", "Kaydet"),
    ("save game", "Oyunu Kaydet"),
    ("options", "Seçenekler"),
    ("settings", "Ayarlar"),
    ("controls", "Kontroller"),
    ("audio", "Ses"),
    ("sound", "Ses"),
    ("display", "Görüntü"),
    ("graphics", "Grafikler"),
    ("video", "Görüntü"),
    ("language", "Dil"),
    ("subtitles", "Altyazılar"),
    ("brightness", "Parlaklık"),
    ("accessibility", "Erişilebilirlik"),
    ("gameplay", "Oynanış"),
    ("difficulty", "Zorluk"),
    ("easy", "Kolay"),
    ("normal", "Normal"),
    ("hard", "Zor"),
    ("map", "Harita"),
    ("stats", "İstatistikler"),
    ("statistics", "İstatistikler"),
    ("brief", "Özet"),
    ("briefs", "Özetler"),
    ("game", "Oyun"),
    ("inventory", "Envanter"),
    ("skills", "Yetenekler"),
    ("quests", "Görevler"),
    ("missions", "Görevler"),
    ("objectives", "Hedefler"),
    ("journal", "Günlük"),
    ("back", "Geri"),
    ("cancel", "İptal"),
    ("confirm", "Onayla"),
    ("accept", "Kabul Et"),
    ("select", "Seç"),
    ("apply", "Uygula"),
    ("close", "Kapat"),
    ("exit", "Çık"),
    ("quit", "Çık"),
    ("quit game", "Oyundan Çık"),
    ("exit game", "Oyundan Çık"),
    ("main menu", "Ana Menü"),
    ("pause", "Duraklat"),
    ("paused", "Duraklatıldı"),
    ("restart", "Yeniden Başlat"),
    ("retry", "Tekrar Dene"),
    ("skip", "Geç"),
    ("next", "İleri"),
    ("previous", "Önceki"),
    ("yes", "Evet"),
    ("no", "Hayır"),
    ("ok", "Tamam"),
    ("on", "Açık"),
    ("off", "Kapalı"),
    ("default", "Varsayılan"),
    ("defaults", "Varsayılanlar"),
    ("credits", "Emeği Geçenler"),
    ("extras", "Ekstralar"),
    ("loading", "Yükleniyor"),
    ("saving", "Kaydediliyor"),
    ("mission passed", "Görev Tamamlandı"),
    ("mission failed", "Görev Başarısız"),
    ("game over", "Oyun Bitti"),
    ("wasted", "Öldün"),
    ("busted", "Yakalandın"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_kelimesi_hazir_ceviriyle_doner() {
        assert_eq!(lookup("Resume"), Some("Devam Et"));
        assert_eq!(lookup("OPTIONS"), Some("Seçenekler"));
        assert_eq!(lookup("New Game"), Some("Yeni Oyun"));
    }

    #[test]
    fn dugme_simgesi_ve_noktalama_yok_sayilir() {
        // Gerçek OCR çıktısı: "Resume ⊗" düğme simgesiyle "ReSume x" okunur.
        assert_eq!(lookup("ReSume x"), Some("Devam Et"));
        assert_eq!(lookup("Back..."), Some("Geri"));
    }

    #[test]
    fn cumle_ve_bilinmeyen_kelime_modele_gider() {
        assert_eq!(lookup("Resume the mission before dawn."), None);
        assert_eq!(lookup("Ocean View Hotel"), None);
        assert_eq!(lookup("x"), None);
    }
}
