//! Teşhis: iş hattının her aşamasının (yakalama, yazı tanıma, çeviri, çıkış) durumu ve "çeviri
//! neden görünmüyor?" sorusunun tek cümlelik cevabı. İş hattı ölçümleri doldurur, `summarize`
//! seviyeleri ve aşamayı hesaplar. Saf mantıktır; testlerle sabitlenir.
//!
//! Ayırt edilen durumlar (kullanıcının "çeviri durdu" dediği anda hangisi olduğunu görmek için):
//! görüntü geliyor / görüntü geliyor ama değişmiyor / yakalama çalışmıyor / yakalama var ama OCR
//! çalışmıyor / OCR yeni yazı bulamıyor / yazı bulundu ama ayıklandı / yazı çeviri kuyruğunda /
//! çeviri servisi çalışmıyor / çeviri hazır ama TV'ye gitmiyor.

use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// Kullanılmıyor ya da henüz veri yok.
    #[default]
    Idle,
    Ok,
    Warn,
    Bad,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// Henüz okuma yapılmadı (OCR modeli yükleniyor olabilir).
    #[default]
    Starting,
    /// Hedef pencere ekranda yok.
    WindowMissing,
    /// Görüntü alınamıyor.
    CaptureDown,
    /// Görüntü alınıyor ama yazı tanıma çalışmıyor.
    OcrDown,
    /// Görüntü geliyor ama bir süredir değişmiyor.
    FrameFrozen,
    /// Çeviri servisi hata veriyor.
    TranslationDown,
    /// Çeviriler hazır ama TV'ye ulaşılamıyor.
    OutputDown,
    /// Yazı bulundu, çeviri bekleniyor.
    Translating,
    /// Yazı bulundu ama hepsi ayıklandı (Türkçe, ad, logo, sayı).
    Filtered,
    /// Ekranda yazı yok.
    NoText,
    /// Çeviriler gösteriliyor.
    Ok,
}

/// Yakalama bu kadar süre başarısızsa "çalışmıyor" sayılır (sn).
pub const CAPTURE_DOWN_AFTER: f64 = 3.0;
/// Kare bu kadar süre değişmezse "donmuş" sayılır (sn).
pub const FROZEN_AFTER: f64 = 5.0;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub stage: Stage,
    /// Kullanıcıya gösterilecek tek cümle.
    pub summary: String,
    pub uptime_s: f64,

    // Yakalama
    pub capture: Level,
    pub waiting_for_window: bool,
    /// Saniyedeki başarılı okuma (tur) sayısı.
    pub reads_per_s: f64,
    /// Akıştan saniyede gelen yeni kare (platform bildiriyorsa).
    pub frames_per_s: Option<f64>,
    pub last_read_ago_s: Option<f64>,
    /// Okunan karenin yaşı (ms).
    pub frame_age_ms: Option<u64>,
    /// Kare kaç saniyedir aynı (yeni kare gelmiyor).
    pub frozen_s: Option<f64>,
    pub capture_ms: u64,
    pub stream_restarts: u64,
    pub read_errors: u64,
    /// Son okuma hatası yazı tanımadan mı geldi.
    pub ocr_failing: bool,
    pub last_read_error: Option<String>,

    // Yazı tanıma ve ayıklama
    pub ocr: Level,
    pub ocr_ms: u64,
    pub last_ocr_ago_s: Option<f64>,
    /// OCR'ın bulduğu satır, birleşmiş blok, sabitlenmiş blok sayısı.
    pub lines: usize,
    pub blocks: usize,
    pub stable: usize,
    /// Sabit olup çeviriye gönderilmeyen blok (Türkçe, yalnızca ad, sayı, odak dışı).
    pub filtered: usize,
    /// Önbellekten karşılanan (yeniden çevrilmeyen) metin sayısı, oturum boyunca.
    pub cache_hits: u64,

    // Çeviri
    pub translation: Level,
    pub translating_enabled: bool,
    pub queue: usize,
    pub in_flight: usize,
    /// "boşta", "çeviriyor", "takıldı" gibi.
    pub worker: String,
    pub last_translation_ago_s: Option<f64>,
    pub translation_ms: Option<u64>,
    pub translation_errors: u64,
    pub timeouts: u64,
    pub rate_limited: u64,
    pub retries: u64,
    pub rejected: u64,
    pub backoff_s: f64,
    pub last_translation_error: Option<String>,

    // Çıkış (TV ve/veya ekran overlay'i)
    pub output: Level,
    /// TV kullanılıyorsa son gönderim başarılı mı.
    pub tv_ok: Option<bool>,
    pub shown: usize,
    pub last_send_ago_s: Option<f64>,
    pub send_errors: u64,

    // Genel
    /// Son turun işlenme süresi (ms).
    pub round_ms: u64,
    /// Ekrandaki görüntüden gösterime yaklaşık gecikme: kare yaşı + tur süresi (ms).
    pub latency_ms: Option<u64>,
    /// Kendiliğinden toparlanma sayısı (tur hatası, okuyucu/çeviri işçisi yenileme).
    pub recoveries: u64,
    pub memory_mb: Option<f64>,
}

impl Diagnostics {
    /// Ölçümlerden seviyeleri, aşamayı ve özeti hesaplar.
    pub fn summarize(&mut self) {
        let never_read = self.last_read_ago_s.is_none();
        let read_stale = self.last_read_ago_s.is_none_or(|ago| ago > CAPTURE_DOWN_AFTER);
        let frozen = self.frozen_s.is_some_and(|s| s >= FROZEN_AFTER);

        self.capture = if self.waiting_for_window {
            Level::Warn
        } else if read_stale && !self.ocr_failing && (self.read_errors > 0 || !never_read) {
            Level::Bad
        } else if never_read {
            Level::Idle
        } else if frozen {
            Level::Warn
        } else {
            Level::Ok
        };

        self.ocr = if self.ocr_failing && read_stale {
            Level::Bad
        } else if never_read {
            Level::Idle
        } else {
            Level::Ok
        };

        let translation_failing = self.translating_enabled && (self.backoff_s > 0.0 || self.worker == WORKER_STUCK);
        self.translation = if !self.translating_enabled {
            Level::Idle
        } else if self.worker == WORKER_STUCK || (translation_failing && self.queue + self.in_flight > 0) {
            Level::Bad
        } else if translation_failing {
            Level::Warn
        } else if self.last_translation_ago_s.is_some() || self.cache_hits > 0 {
            Level::Ok
        } else {
            Level::Idle
        };

        self.output = match self.tv_ok {
            Some(false) => Level::Bad,
            Some(true) => Level::Ok,
            None if self.last_send_ago_s.is_some() => Level::Ok,
            None => Level::Idle,
        };

        let (stage, summary) = if self.waiting_for_window {
            (Stage::WindowMissing, "Remote Play penceresi ekranda yok; açılması bekleniyor.".to_string())
        } else if self.capture == Level::Bad {
            let reason = self.last_read_error.clone().unwrap_or_else(|| "yanıt yok".into());
            (Stage::CaptureDown, format!("Ekran görüntüsü alınamıyor ({reason}). Kendiliğinden yeniden deneniyor."))
        } else if self.ocr == Level::Bad {
            (Stage::OcrDown, "Görüntü alınıyor ama yazı tanıma çalışmıyor; yeniden deneniyor.".to_string())
        } else if never_read {
            (Stage::Starting, "Başlıyor: ilk okuma bekleniyor (yazı tanıma modeli ilk açılışta ~30 sn yüklenir).".to_string())
        } else if frozen {
            let seconds = self.frozen_s.unwrap_or_default().round();
            (Stage::FrameFrozen, format!("Görüntü {seconds} sn'dir değişmiyor: oyun duraklatılmış ya da Remote Play donmuş olabilir."))
        } else if self.translation == Level::Bad {
            let reason = self.last_translation_error.clone().unwrap_or_else(|| "yanıt yok".into());
            (Stage::TranslationDown, format!("Çeviri servisi çalışmıyor ({reason}); {} metin bekliyor.", self.queue + self.in_flight))
        } else if self.output == Level::Bad {
            (Stage::OutputDown, "Çeviriler hazır ama TV'ye ulaşılamıyor; yeniden bağlanılıyor.".to_string())
        } else if self.queue + self.in_flight > 0 {
            (Stage::Translating, format!("{} metin çevriliyor.", self.queue + self.in_flight))
        } else if self.shown > 0 {
            (Stage::Ok, format!("{} çeviri gösteriliyor.", self.shown))
        } else if self.lines > 0 && self.stable > 0 && self.filtered >= self.stable {
            (Stage::Filtered, "Yazı bulundu ama çevrilecek bir şey yok (Türkçe, ad, sayı ya da logo/tabela).".to_string())
        } else if self.lines > 0 {
            (Stage::NoText, "Yazı okunuyor, oturması bekleniyor.".to_string())
        } else {
            (Stage::NoText, "Ekranda yazı yok.".to_string())
        };
        self.stage = stage;
        self.summary = summary;
    }
}

/// Çeviri işçisi durumları (`Diagnostics::worker`).
pub const WORKER_IDLE: &str = "boşta";
pub const WORKER_BUSY: &str = "çeviriyor";
pub const WORKER_STUCK: &str = "takıldı";

#[cfg(test)]
mod tests {
    use super::*;

    /// Her şeyin yolunda olduğu bir an: okuma 0,2 sn önce, kare taze, çeviri gösteriliyor.
    fn healthy() -> Diagnostics {
        Diagnostics {
            last_read_ago_s: Some(0.2),
            frozen_s: Some(0.1),
            translating_enabled: true,
            worker: WORKER_IDLE.into(),
            last_translation_ago_s: Some(2.0),
            lines: 3,
            blocks: 2,
            stable: 2,
            shown: 2,
            tv_ok: Some(true),
            last_send_ago_s: Some(0.2),
            ..Diagnostics::default()
        }
    }

    fn stage(mut diagnostics: Diagnostics) -> Stage {
        diagnostics.summarize();
        diagnostics.stage
    }

    #[test]
    fn saglikli_durum() {
        let mut d = healthy();
        d.summarize();
        assert_eq!((d.stage, d.capture, d.ocr, d.translation, d.output), (Stage::Ok, Level::Ok, Level::Ok, Level::Ok, Level::Ok));
        assert_eq!(d.summary, "2 çeviri gösteriliyor.");
    }

    #[test]
    fn baslarken() {
        assert_eq!(stage(Diagnostics { translating_enabled: true, ..Diagnostics::default() }), Stage::Starting);
    }

    #[test]
    fn dokuz_durum_birbirinden_ayrilir() {
        // 1. Görüntü geliyor (sağlıklı).
        assert_eq!(stage(healthy()), Stage::Ok);
        // 2. Görüntü geliyor ama değişmiyor.
        assert_eq!(stage(Diagnostics { frozen_s: Some(12.0), ..healthy() }), Stage::FrameFrozen);
        // 3. Yakalama çalışmıyor: son başarılı okuma eski, hata yakalamadan.
        assert_eq!(stage(Diagnostics { last_read_ago_s: Some(8.0), read_errors: 4, ..healthy() }), Stage::CaptureDown);
        // 4. Yakalama var ama OCR çalışmıyor.
        assert_eq!(stage(Diagnostics { last_read_ago_s: Some(8.0), read_errors: 4, ocr_failing: true, ..healthy() }), Stage::OcrDown);
        // 5. OCR çalışıyor ama yazı yok.
        assert_eq!(stage(Diagnostics { lines: 0, blocks: 0, stable: 0, shown: 0, ..healthy() }), Stage::NoText);
        // 6. Yazı bulundu ama ayıklandı.
        assert_eq!(stage(Diagnostics { filtered: 2, shown: 0, ..healthy() }), Stage::Filtered);
        // 7. Yazı çeviri kuyruğunda.
        assert_eq!(stage(Diagnostics { queue: 1, shown: 0, ..healthy() }), Stage::Translating);
        // 8. Çeviri servisi çalışmıyor.
        assert_eq!(stage(Diagnostics { queue: 1, backoff_s: 4.0, ..healthy() }), Stage::TranslationDown);
        assert_eq!(stage(Diagnostics { worker: WORKER_STUCK.into(), ..healthy() }), Stage::TranslationDown);
        // 9. Çeviri başarılı ama TV güncellenmiyor.
        assert_eq!(stage(Diagnostics { tv_ok: Some(false), ..healthy() }), Stage::OutputDown);
    }

    #[test]
    fn pencere_yoksa_once_o_soylenir() {
        let d = Diagnostics { waiting_for_window: true, last_read_ago_s: Some(30.0), read_errors: 9, ..healthy() };
        assert_eq!(stage(d), Stage::WindowMissing);
    }

    #[test]
    fn bekleyen_metin_yokken_ceviri_hatasi_uyaridir() {
        let mut d = Diagnostics { backoff_s: 3.0, ..healthy() };
        d.summarize();
        assert_eq!(d.translation, Level::Warn);
        assert_eq!(d.stage, Stage::Ok);
    }

    #[test]
    fn cevirisiz_modda_ceviri_bosta() {
        let mut d = Diagnostics { translating_enabled: false, ..healthy() };
        d.summarize();
        assert_eq!(d.translation, Level::Idle);
    }

    #[test]
    fn json_bicimi() {
        let mut d = healthy();
        d.summarize();
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["stage"], "ok");
        assert_eq!(json["capture"], "ok");
        assert_eq!(json["lastReadAgoS"], 0.2);
    }
}
