# Uygulama Planı — Akıllı Altyazı Tanıma, Oyuna Duyarlı Çeviri, Kendiliğinden Toparlanma

Tarih: 2026-11-19 · Taban: `main` @ 743b38f · Kapsam: masaüstü ürün yolu (`desktop/` Rust çekirdeği +
`mac/` Swift köprüsü). Android TV uygulaması ve eski Swift CLI (`mac/Sources/ps5ceviri`) değişmez.

## 1. Mevcut mimari (kod üzerinden doğrulandı)

```
Remote Play penceresi
  └─ Swift  PS5CeviriMac.FrameStream (SCStream, en son kare, stall → yeniden başlat, 4→60 sn)
  └─ Swift  ScreenReader.read() @MainActor
        pencere (1 sn önbellek) → 16:9 kırp → kopya → OCRGate (tek Vision isteği) → Vision (5 sn / ilk 45 sn)
  └─ Swift  CeviriBridge (@_cdecl, JSON, 60 sn son güvence)        ← tek okuyucu: `reading` bayrağı
  └─ Rust   ceviri-mac::MacScreenReader : ScreenReader
  └─ Rust   engine::Pipeline::run  (tek thread, fps tur/sn)
        ocr_text::clean → block_merger (çok satır → blok) → Stabilizer (IoU takip, 2 kare + 0,3 sn)
        → focus::classify (Diyalog / Menü / Diğer) → compose():
            Türkçe/ad/sayı ayıkla → Terms::protect (<x>ad</x>) → glossary::lookup (menü kelimesi)
            → TranslationService::request (yalnızca ekrandaki metinler)
  └─ Rust   engine::translation  (1–2 işçi thread, takılan işçi yenilenir, panik yakalanır)
        core::Translator: önbellek + kuyruk + in-flight(iş kimliği) + metin bazlı bekleme + üstel bekleme
        + rakam şablonu (geri sayım istek atmaz)
        Backend: DeepLClient (ureq 3/8 sn, bağlam = son 4 replik) | Opus-MT (opus_text yer tutucu + ek uyumu)
  └─ Rust   Delivery → TvClient (WebSocket, her tur heartbeat) ve/veya ekran overlay'i (Tauri)
  └─ Rust   health::Diagnostics (saniyede bir) → app/ui teşhis paneli
```

Korunan stabilite çalışmaları: OCRGate, tek okuyucu (`reading` bayrağı + köprü süre sınırı), akış
stall yeniden başlatma, okuyucu yenileme (20 sn → ×2 → 300 sn), sınırlı kuyruk (yalnızca ekrandaki
metin), işçi yenileme (10 dk'da en çok 3), üstel bekleme (429/kota ayrı), bırakılan iş (expire) ve geç
sonucun önbelleğe yazılması, rakam kardeş çeviri, teşhis paneli, heartbeat, soak testi.

## 2. Tespit edilen sorunlar

| # | Sorun | Etki |
|---|-------|------|
| S1 | Yarım metin: daktilo/parça parça gelen altyazı 0,3 sn durursa kabul edilip çevriliyor | Gereksiz API, yarım Türkçe |
| S2 | Büyüyen metnin kutusu genişleyince IoU < 0,3 → yeni takip; eski kısa metin 0,5 sn asılı kalır | Titreme, iki blok |
| S3 | Önbellek yalnızca birebir anahtar: `The door is locked.` / `THE DOOR IS LOCKED` ayrı istek | Gereksiz API |
| S4 | Güven (confidence) bloğa taşınmıyor; stabilizasyon her metin için sabit | Kısa/net metin gereksiz bekler |
| S5 | **Bayat sonuç:** okuma başarısızken (uyku, akış ölü) gelen geç çeviri, `ready` yolundan eski blokları yeniden TV'ye gönderiyor (`state.stable` temizlenmiyor) | Eski metin ekrana döner |
| S6 | Bağlam DeepL istemcisinde, sıfırlanamıyor; sahne/oyun değişiminde eski replikler yeni çeviriye karışır; bağlamla çevrilen kısa metin kalıcı önbelleğe yazılıyor | Yanlış bağlamda yeniden kullanım |
| S7 | Oyun profili, terim sözlüğü (kelime düzeyi), kullanıcı düzeltmesi yok; yalnızca tam-metin menü sözlüğü | Terim tutarsızlığı |
| S8 | 429 `Retry-After` okunmuyor; 403/456 her 5 dk sonsuza dek denenir; 4xx istek hataları tüm kuyruğu bekletir | Gereksiz istek, yanlış sınıflandırma |
| S9 | Devre kesici yok: servis düzelince ilk istek tüm ekran metinleri (50'ye kadar) | Yeniden çöküş riski |
| S10 | Uyku/uyanma algılanmıyor: uyanınca eski bloklar, eski bağlam, eski bekleme süreleri | Bayat çeviri, gecikmeli toparlanma |
| S11 | Okuyucu toparlanması dağınık sayaçlarla; durum makinesi, "doğrulama" ve "kullanılamıyor" durumu yok | Gözlemlenebilirlik zayıf |
| S12 | Swift: kare değişmese de (statik sahne/menü) her tur Vision OCR çalışıyor | Boşa CPU |
| S13 | Teşhis: aday/sözlük/devre kesici/toparlanma aşaması görünmüyor | Sorunun yeri belirsiz |

## 3. Entegrasyon tasarımı (yeni sistem yazılmaz, mevcutlar geliştirilir)

### F1 Akıllı altyazı tanıma — `core/stabilizer.rs`, `core/block_merger.rs`, `core/focus.rs`
- `MergedBlock.confidence` (satırların ortalaması) eklenir.
- `Stabilizer` uyarlamalı bekleme: taban 2 kare + 0,3 sn korunur; **büyüyen** aday (önceki metnin
  devamı, cümle sonu yok) +0,5 sn; daktilo davranışı görülmüş oyunda noktalamasız çok kelimeli metin
  +0,3 sn; düşük güven (+1 kare, +0,2 sn); **hızlı yol**: cümle sonu + güven ≥ 0,9 + büyüme değil →
  0,15 sn; üst sınır 1,5 sn (sonsuza dek bekletme yok).
- Takip eşleştirmesi: IoU düşükse aynı satır bandında ve metin devamıysa aynı takip (S2).
- Aday yaşam döngüsü (`CandidateState`: Tracking → Stabilizing → Ready; iş hattı: Translating →
  Translated → Displayed; düşen: Expired) teşhise açılır; sayaçlar: birleşen blok, kopya, düşen aday.
- `focus::Category` (Subtitle, Dialogue, Objective, Warning, Menu, Hud, Other) deterministik kurallarla;
  yalnızca kuyruk önceliği ve bağlam için kullanılır, yanlış sınıflandırma çeviriyi engellemez.

### F2 Oyuna duyarlı çeviri — yeni `core/profile.rs`, `core/term_glossary.rs`, `core/context.rs`
- `GameProfile` (ad, tür, dil çifti, üslup, karakter adları, sözlük, tercih edilen motor, altyazı/OCR/
  bağlam ayarları, düzeltmeler) JSON olarak `app_dir/profiles/<slug>.json`; doğrulama.
- `Glossary`: öncelik **kullanıcı (oyun) > profil > kullanıcı (genel) > yerleşik genel**; uzun ifade
  önce, kelime sınırı, büyük/küçük harf, İngilizce çoğul (-s/-es → Türkçe -lar/-ler), çakışmalar.
  Uygulama kaynak metinde `<g>Türkçe</g>` işaretiyle (DeepL `ignore_tags`, Opus yer tutucusu); kesme
  işaretli ek (`Görev'i`) cins isimde düzeltilir (`Görevi`). Türkçe çıktıda alt dize değiştirme yok.
- Tam metin düzeltmesi (kullanıcı) ve tam metin sözlük eşleşmesi modele gitmez.
- Bağlam (`ContextWindow`) DeepL istemcisinden çekirdeğe taşınır: sınırlı (4 replik/600 karakter),
  yalnızca diyalog, profil ipucu (oyun, tür, üslup), sahne/pencere/uyku/profil değişiminde sıfırlanır.
  `Backend` imzası `(&[String], &str)` olur (bağlam; yerel model yok sayar).
- Çeviri belleği (`Translator`): birebir → normalize (boşluk, büyük harf, sondaki nokta; `?`/`!` korunur)
  → rakam şablonu. **Bulanık eşleştirme bilinçli olarak yok** (yanlış eşleşme riski). Kısa metinler
  (≤ 3 kelime) profil kapsamlı anahtarla (`<profil>␟metin`); bağlamla çevrilen kısa metin yalnızca oturum
  belleğinde (diske yazılmaz, bağlam sıfırlanınca silinir). Sözlük/ad sürümü anahtarın içindedir
  (`<x>`, `<g>`), değişince eski çeviri kendiliğinden kullanılmaz. Motor sürümü: ayrı dosyalar (mevcut).

### F3 Kendiliğinden toparlanma — yeni `core/recovery.rs`, `core/translator.rs`, `engine/pipeline.rs`
- `RecoveryCoordinator` (tek koordinatör): `Initializing, WaitingForSource, Running, Degraded,
  Recovering, Verifying, Unavailable`; yasadışı geçiş reddedilir; aynı anda tek toparlanma; bekleme
  (taban × 2ⁿ, ≤ 300 sn), pencere başına en çok N yenileme, başarı ölçütü (yenilemeden sonra 2 başarılı
  okuma), sebep metni. Mevcut okuyucu yenileme davranışı birebir korunur.
- `SleepDetector`: duvar saati − monoton saat sıçraması (macOS `Instant` uykuda ilerlemez). Uyanınca:
  bloklar/bağlam temizlenir, çeviri beklemeleri sıfırlanır, devre yarı-açık (tek prob), okuyucu hemen
  yenilenir. Pencere kaybolup gelince (Remote Play kapat/aç) bağlam sıfırlanır.
- Hata sınıflandırma (`ErrorClass`); 429 `Retry-After`; 403/456 artan bekleme (5 dk → 1 sa) ve
  "kullanıcı müdahalesi gerekli"; 4xx istek hatası yalnızca o metinleri bekletir.
- Devre kesici (`Closed/Open/HalfOpen`) Translator içinde: art arda 3 hata → açık; süre dolunca tek
  metinlik prob (ekrandaki en öncelikli metin); başarı → kapalı. Eski kuyruk birden gönderilmez.
- S5 düzeltmesi: `ready` yolu yalnızca son başarılı okuma `HOLD_ON_READ_ERROR` içindeyse gönderir.
- Swift S12: kare sıra numarası ve kırpma aynıysa OCR atlanır, önceki satırlar döner (`OCRReuse`).

### F5 Teşhis — `core/health.rs`, `app/ui/app.js`
Yeni alanlar: sistem durumu, toparlanma aşaması/sebebi/sayısı, son başarılı toparlanma, son kare/OCR/
çeviri, devre kesici, hata sınıfı, aktif aday (durum, yaş, kararlılık, güven), birleşme/kopya/düşen
aday, profil, sözlük eşleşmesi, normalize önbellek isabeti, bağlam uzunluğu. Yeni aşamalar
`Recovering`, `Unavailable`. Oyun metni ve anahtar loglanmaz (yalnızca sayılar ve durumlar).

## 4. Dosyalar

Değişecek: `core/{lib,stabilizer,block_merger,focus,translator,health,terms,opus_text,deepl,storage}.rs`,
`engine/{pipeline,translation,deepl_client}.rs`, `app/src/{controller,main}.rs`, `app/ui/{app.js,index.html}`,
`cli/src/main.rs`, `opus/src/lib.rs`, `mac/Sources/PS5CeviriMac/ScreenReader.swift`.
Yeni: `core/src/{profile,term_glossary,context,recovery}.rs`, `mac/Tests/PS5CeviriMacTests/OCRReuseTests.swift`.
Köprü (C ABI/JSON) değişmez.

## 5. Riskler

- Uyarlamalı bekleme bazı oyunlarda ilk çeviriyi 0,3–0,5 sn geciktirebilir (yalnızca daktilo görülen
  oyunda ve noktalamasız metinde). Üst sınır 1,5 sn.
- `<g>` ile enjekte edilen Türkçe terimde DeepL eki adın gibi kesmeyle koyabilir; düzeltilir. Opus'ta
  iyelik ekli terimlerde ek uyumu kusurlu olabilir.
- Uyku algılaması saat farkına dayanır; Windows'ta `Instant` uykuda da ilerleyebilir → orada yalnızca
  çok uzun tur boşluğu (> 120 sn) uyku sayılır.
- OCR atlama, akışın "idle" kare davranışına dayanır (SCStream'de doğrulanmış davranış); kare sayacı
  değişmezse görüntü de değişmemiştir.
- Gerçek PS5/Remote Play, gerçek uyku/uyanma, DeepL canlı 429/Retry-After burada denenemez.

## 6. Test planı

Sahte okuyucu/OCR/çeviri ile (gerçek ekran gerekmez): F1'in 15, F2'nin 13, F3'ün 20 senaryosu;
regresyon (OCRGate, tek okuyucu, kuyruk sınırı, işçi yenileme, rakam kardeş, bırakılan iş, bekleme,
heartbeat, teşhis); simülasyonla API çağrı sayısı önce/sonra (aynı senaryo eski koda karşı da koşulur);
soak testi; önemli düzeltmelerde mutasyon doğrulaması (düzeltme geri alınınca test kırılmalı).
Komutlar: `cargo test` (core, engine, app), `cargo clippy --all-targets`, `swift test`.

## 7. Performans etkisi (beklenen)

- API: yarım metin, normalize kopya ve kapsam kuralıyla daha az istek; bağlamla çevrilen kısa metin
  oturumlar arasında yeniden istenir (küçük artış, doğruluk için).
- CPU: statik sahnede OCR atlanır (en büyük kazanç); stabilizasyon/sözlük hesapları µs düzeyi.
- Bellek: tüm yeni yapılar sınırlı (bağlam 600 karakter, oturum belleği 2000, bekleme kaydı 1000,
  aday geçmişi takip sayısı kadar).
