# PS5 Anlık Türkçe Çeviri

PS5'te oynadığın İngilizce oyunlardaki yazıları **gerçek zamanlı olarak Türkçeye çevirip TV ekranında,
orijinal yazının tam üstünde** gösteren kişisel bir sistem.

TV'de PS5 görüntüsü HDMI'dan geldiği gibi, gecikmesiz akar. Bir Mac aynı görüntüyü PS Remote Play
üzerinden okur, metni bulup çevirir ve Android TV'deki uygulamaya gönderir. TV uygulaması da çeviriyi
oyun görüntüsünün üstüne yarı saydam kutular halinde çizer.

```
          ┌──────────────── HDMI ────────────────┐
          │                                      ▼
   ┌─────────┐                           ┌───────────────┐
   │   PS5   │                           │  Android TV   │  ← oyunu buradan izlersin
   └─────────┘                           │  + çeviri     │
          │                              │    overlay'i  │
          │ PS Remote Play               └───────────────┘
          ▼                                      ▲
   ┌──────────────────────────────────┐          │
   │ Mac mini                         │          │ WebSocket (yerel ağ)
   │ ekranı oku → metni bul → çevir ──┼──────────┘
   │ (ScreenCaptureKit, Vision, DeepL)│
   └──────────────────────────────────┘
```

> Kişisel kullanım için geliştirildi; mağazada yok. Vestel Android TV (Android 14) + PS5 + Mac mini
> kurulumunda gerçek bir oyunla (It Takes Two) uçtan uca test edildi.

---

## İçindekiler

1. [Neler gerekiyor?](#neler-gerekiyor)
2. [Kurulum](#kurulum)
   - [Adım 1 — Android TV uygulaması](#adım-1--android-tv-uygulaması)
   - [Adım 2 — Mac tarafı](#adım-2--mac-tarafı)
   - [Adım 3 — DeepL API anahtarı](#adım-3--deepl-api-anahtarı)
   - [Adım 4 — Kontrolcü](#adım-4--kontrolcü)
3. [Her gün nasıl kullanılır?](#her-gün-nasıl-kullanılır)
4. [Nasıl çalışıyor? (basit anlatım)](#nasıl-çalışıyor-basit-anlatım)
5. [Teknik detaylar](#teknik-detaylar)
6. [Sorun giderme](#sorun-giderme)
7. [Bilinen sınırlar](#bilinen-sınırlar)
8. [Proje yapısı ve testler](#proje-yapısı-ve-testler)

---

## Neler gerekiyor?

| Ne | Neden | Not |
|---|---|---|
| **PS5** | Oyun | HDMI ile TV'ye bağlı |
| **Android TV** | Oyunu izlediğin ekran; çeviri bunun üstüne çizilir | Android 8.0+ (test: Vestel, Android 14) |
| **Mac** | Ekranı okuyup çeviren taraf | macOS 15+, Apple Silicon önerilir, Xcode kurulu |
| **PS Remote Play** (Mac uygulaması) | Mac'in PS5 görüntüsünü görebilmesi için | [playstation.com](https://www.playstation.com/tr-tr/remote-play/)'dan ücretsiz |
| **DualSense** | Oyunu oynamak için | **Mac'e** bağlanır (bkz. [Adım 4](#adım-4--kontrolcü)) |
| **DeepL API Free** hesabı | Çeviri | Ayda 500.000 karakter ücretsiz, kayıtta kart istenir |
| **Aynı ev ağı** | Mac ↔ TV iletişimi | Mac, TV ve PS5 aynı modeme bağlı olmalı |
| **Android Studio** veya `adb` | TV'ye uygulama kurmak için | Sadece kurulumda |

---

## Kurulum

### Adım 1 — Android TV uygulaması

**1.1 TV'de geliştirici seçeneklerini aç**

TV'de **Ayarlar → Cihaz Tercihleri → Hakkında** ekranına git ve **Yapı** satırına kumandayla
**7 kez** bas. "Artık bir geliştiricisiniz" mesajı çıkar.

Sonra **Ayarlar → Cihaz Tercihleri → Geliştirici seçenekleri** altında **USB hata ayıklama**'yı ve
(varsa) **Ağ üzerinden hata ayıklama**'yı aç.

**1.2 TV'nin IP adresini öğren**

**Ayarlar → Ağ ve İnternet → bağlı ağ** ekranında yazar (ör. `192.168.1.5`). Aşağıdaki komutlarda
`TV_IP` yerine bunu yaz.

**1.3 Uygulamayı derle ve kur**

Mac'te, proje klasöründe:

```bash
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
./gradlew assembleDebug
```

```bash
adb connect TV_IP:5555
```

TV'de "USB hata ayıklamaya izin verilsin mi?" sorusu çıkarsa **Her zaman izin ver** deyip onayla.

```bash
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

> Android Studio kullanıyorsan proje klasörünü **Open** ile açıp TV'yi cihaz olarak seçerek de
> çalıştırabilirsin.

**1.4 Overlay iznini ver**

Çoğu Android TV'de "diğer uygulamaların üzerinde göster" ayar ekranı yoktur; izni adb ile ver:

```bash
adb shell appops set com.isacetin.ps5ceviri SYSTEM_ALERT_WINDOW allow
```

**1.5 Uygulamayı aç ve test et**

TV'nin uygulama listesinden **PS5 Çeviri**'yi aç. Ekranda TV'nin bağlantı adresi
(`ws://TV_IP:8765`) ve durumlar görünür:

1. **Overlay'i başlat**'a bas → "Servis: Çalışıyor", "Overlay penceresi: Ekranda".
2. **Test yazısı göster**'e bas → ekranın ortasında **KAPI KİLİTLİ** kutusu çıkar.
3. Kumandanın **Kaynak/Input** tuşuyla PS5'in HDMI girişine geç → kutu PS5 görüntüsünün üstünde
   görünmeye devam etmeli.
4. **Test yazısını kaldır** ile temizle.

Overlay bir kez başlatıldıktan sonra arka planda çalışır; TV açılınca ve uygulama güncellenince
kendiliğinden yeniden başlar.

### Adım 2 — Mac tarafı

**2.1 Derle**

```bash
cd mac
swift build -c release
```

Çalıştırılabilir dosya: `mac/.build/release/ps5ceviri`.

**2.2 macOS izinleri**

Aracı hangi uygulamadan çalıştıracaksan (Terminal, iTerm...) ona iki izin ver:

- **Sistem Ayarları → Gizlilik ve Güvenlik → Ekran ve Sistem Sesi Kaydı**: ekranı okuyabilmesi için.
  İzni verdikten sonra o uygulamayı kapatıp yeniden aç.
- **Sistem Ayarları → Gizlilik ve Güvenlik → Yerel Ağ**: TV'ye bağlanabilmesi için.

**2.3 PS Remote Play**

Remote Play'i aç, PSN hesabınla giriş yap ve PS5'e bağlan. Pencereyi **büyük tut ya da tam ekran
yap**: Mac ne kadar büyük görüntü görürse yazıları o kadar iyi okur.

**2.4 Çevirisiz deneme (önerilir)**

DeepL anahtarı olmadan, sadece konumların doğru olup olmadığını görmek için:

```bash
.build/release/ps5ceviri run --dry-run
```

TV'de oyundaki İngilizce yazılar, kendi yerlerinin üstünde **İngilizce olarak** kutu içinde görünmeli.
Kutular orijinal yazıların tam üstüne oturuyorsa kurulum doğru demektir.

### Adım 3 — DeepL API anahtarı

1. [deepl.com/pro-api](https://www.deepl.com/pro-api) adresinden **DeepL API Free** planına kaydol.
   (Uygulama aboneliği olan "DeepL Pro" değil, **API** planı.) Kayıtta kart istenir ama Free planda
   ücret çekilmez.
2. Hesabındaki **API Keys** sekmesinden anahtarı kopyala. Free anahtarlar `:fx` ile biter.
3. Mac'te anahtarı **ekrana yazdırmadan** kaydet (komut anahtarı sorar, yapıştırıp Enter'a bas):

```bash
mkdir -p ~/Library/Application\ Support/PS5Ceviri && read -rs "KEY?DeepL API anahtarı: " && print -r -- "$KEY" > ~/Library/Application\ Support/PS5Ceviri/deepl-key.txt && chmod 600 ~/Library/Application\ Support/PS5Ceviri/deepl-key.txt; unset KEY
```

4. Dene:

```bash
.build/release/ps5ceviri translate "The door is locked. You need to find the key."
```

Çıktı: `… → Kapı kilitli. Anahtarı bulman gerekiyor.`

> Anahtar proje klasörünün dışında durur, GitHub'a gitmez. Anahtarı kimseyle paylaşma.

### Adım 4 — Kontrolcü

**DualSense'i PS5'e değil Mac'e bağla.** PS5'e bağlı bir kontrolcüyle oynamaya başlayınca PS5
"Bu kontrol cihazını kim kullanıyor?" diye sorar ve Remote Play bağlantısı kopar; çeviri durur.

- **Kablo (en kolayı):** DualSense'i USB-C kabloyla Mac'e tak. Kablo veri aktarabilen bir kablo
  olmalı (sadece şarj kablosu çalışmaz).
- **Bluetooth:** PS5'i kapat, DualSense'te **Create** (dokunmatik panelin solundaki küçük tuş) ve
  **PS** tuşlarına ışık hızlı hızlı yanıp sönene kadar birlikte bas, Mac'te **Sistem Ayarları →
  Bluetooth**'ta çıkan DualSense'e **Bağlan** de. Daha önce eşleştirilmiş ama bağlanmıyorsa önce
  "Bu Aygıtı Unut" de, sonra yeniden eşleştir.

Görüntüyü TV'den izlersin (gecikmesiz HDMI), tuşlar Remote Play üzerinden PS5'e gider.
Kontrolcüyü tekrar doğrudan PS5'le kullanmak için USB kabloyla PS5'e takıp PS tuşuna bas.

---

## Her gün nasıl kullanılır?

1. PS5'i ve TV'yi aç. (TV uygulaması arka planda kendiliğinden çalışır.)
2. Mac'te **PS Remote Play**'i aç ve PS5'e bağlan. DualSense Mac'e bağlı olsun.
3. Mac'te Terminal'i aç ve çalıştır:

```bash
cd ~/Desktop/ps5_anlik_ceviri/mac && .build/release/ps5ceviri run
```

4. TV'de PS5'in HDMI girişine geç ve oyna. İlk açılışta yaklaşık **30 saniye** bekle (OCR modeli
   yükleniyor); sonra İngilizce yazılar Türkçe olarak görünmeye başlar.
5. Bitirince Terminal'de **Ctrl+C**. TV'deki yazılar 3 saniye içinde kendiliğinden kaybolur.

Terminal'de TV'ye o an ne gönderildiği yazar:

```
TV: Yerel Oynat  |  Çevrimiçi Oynayın  |  Seçenekler  |  Erişilebilirlik Seçenekleri  |  Kaynakça
```

---

## Nasıl çalışıyor? (basit anlatım)

Saniyede 2 kez şu döngü döner:

1. **Bak:** Mac, Remote Play penceresinin anlık görüntüsünü alır ve içinden sadece oyun görüntüsünü
   (16:9 alan) keser; başlık çubuğu ve kenardaki siyah bantlar atılır.
2. **Oku:** Apple'ın yazı tanıma motoru (Vision) görüntüdeki tüm yazıları ve **ekrandaki yerlerini** bulur.
3. **Düzenle:** Alt alta duran ve aynı paragrafa ait satırlar birleştirilir (diyalog tek cümle olarak
   çevrilsin diye). Menü öğeleri ayrı kalır.
4. **Bekle:** Bir yazı iki tur boyunca değişmeden kalınca "hazır" sayılır. Böylece harf harf yazılan
   diyaloglar yazımı bitmeden çevrilmez, OCR'ın tek karelik hataları ekranda titreme yapmaz.
5. **Çevir:** Hazır yazı daha önce çevrildiyse hafızadan alınır; çevrilmediyse DeepL'e gönderilir.
   PS5'in kendi Türkçe menüleri çeviriye hiç gönderilmez.
6. **Gönder:** Çeviriler, ekrandaki konumlarıyla birlikte (ekranın yüzdesi olarak) TV'ye gönderilir.
7. **Çiz:** TV uygulaması her yazıyı orijinalin tam üstüne, koyu bir kutu içinde, okunaklı boyutta çizer.

Konumlar piksel değil **ekranın oranı** olarak gönderildiği için (ör. "soldan %25, üstten %42"),
Mac'teki küçük Remote Play penceresiyle 4K TV birebir örtüşür.

---

## Teknik detaylar

### Bileşenler

| Taraf | Dil / Teknoloji | Görev |
|---|---|---|
| **Android TV uygulaması** (`app/`) | Kotlin, Compose for TV, Ktor CIO, kotlinx.serialization, Coroutines | Ayar ekranı, overlay penceresi, WebSocket server, NSD yayını |
| **Mac aracı** (`mac/`) | Swift 6, ScreenCaptureKit, Vision, Network, NaturalLanguage | Ekran yakalama, OCR, blok birleştirme, stabilizasyon, DeepL, WebSocket istemcisi |
| **Test scripti** (`tools/ws_test.py`) | Python + `websockets` | Mac olmadan TV'ye sahte mesaj göndermek |

İki tarafta da bağımlılıklar minimumda tutuldu: Mac aracı yalnızca Apple framework'lerini kullanır,
TV uygulamasında Hilt / repository katmanları yoktur.

### Mac tarafı: iş hattı

```
SCStream (pencere akışı) ─▶ 16:9 video alanı ─▶ Vision OCR ─▶ BlockMerger ─▶ Stabilizer
   ─▶ Türkçe filtresi ─▶ Translator (önbellek + DeepL) ─▶ OverlayClient (WebSocket) ─▶ TV
```

- **Yakalama (`FrameStream`):** ScreenCaptureKit `SCStream` ile Remote Play penceresinden sürekli
  kare akışı alınır; döngü her turda en son kareyi okur. İlk sürümde her turda ayrı ekran görüntüsü
  isteniyordu; Remote Play bağlantısı koparken bir istek takılınca sonrakiler de kilitleniyordu.
  Pencere değişirse ya da akış durursa akış yeniden başlatılır. Yakalama ~28 ms sürer.
- **Video alanı (`Geometry.videoRect`):** Pencereden 28 pt başlık çubuğu çıkarılır (tam ekranda 0),
  kalan alana 16:9 video ortalanarak sığdırılır. 21:9 monitörde tam ekranın yan bantları böylece atılır.
- **OCR (`TextRecognizer`):** Vision `RecognizeTextRequest`, `.accurate`, `en-US`. Vision kutuları
  sol-alt orijinlidir; protokol için sol-üst orijine çevrilir. Tur başına ~100 ms (ilk çağrıda model
  yüklendiği için ~28 sn).
- **Blok birleştirme (`BlockMerger`):** Union-find ile satır çiftleri gruplanır. Aynı satırda bölünmüş
  parçalar soldan sağa birleşir. Alt alta iki satır şu koşullarla birleşir: dikey boşluk ≤ 0,7 satır
  yüksekliği, yükseklik oranı ≤ 1,4, yatayda örtüşme ve sol/orta hizası, **ve** üst satır kaydırılacak
  kadar uzun ya da alt satır küçük harfle devam ediyor. Bu son kural menü listelerinin paragraf
  sanılmasını önler. Satır sonu tirelemesi geri alınır.
- **Stabilizasyon (`Stabilizer`):** Bloklar kareler boyunca IoU ile eşlenir ve sabit kimlik alır.
  Metin 2 kare aynı kalınca gösterilir. Ekrandaki bir bloğun metni değişirse yeni metin oturana kadar
  eskisi gösterilir. Aynı uzunlukta tek tük harf oynaması ("l"/"1") aynı metin sayılır. Karşılaştırma
  Türkçe yerel ayarla yapılır (I→ı, İ→i). Kaybolan blok 0,8 sn tutulur.
- **Türkçe filtresi (`LanguageFilter`):** Türkçeye özgü harfler veya `NLLanguageRecognizer`
  (İngilizce/Türkçe, > %80) ile Türkçe görünen metinler (PS5 arayüzü, bildirimler) DeepL'e gönderilmez.
- **Çeviri (`Translator`, `DeepLClient`):** `actor` içinde önbellek tutulur; aynı metin aynı anda iki
  kez istenmez; en fazla 50 metinlik partiler gönderilir. Hatada bekleme: genel 5 sn, 429 (hız sınırı)
  10 sn, 456 (kota) ve 403 (geçersiz anahtar) 5 dk. Önbellek
  `~/Library/Application Support/PS5Ceviri/translations.json` dosyasında kalıcıdır. Free anahtarlar
  (`:fx`) `api-free.deepl.com`'a, diğerleri `api.deepl.com`'a gider.
- **WebSocket (`OverlayClient`):** `URLSessionWebSocketTask` kullanılır. Liste değişmese de her tur
  gönderilir (heartbeat). Sunucunun ping'lerine cevap verilebilmesi için sürekli bekleyen bir
  `receive` tutulur; bu olmadan TV bağlantıyı ~30 sn'de bir kesiyordu. Gönderim başarısız olursa
  hemen yeniden bağlanıp bir kez daha denenir.
- **TV'yi bulma (`Discovery`):** `NWBrowser` ile `_ps5ceviri._tcp` aranır ve IPv4'e zorlanarak
  çözülür (link-local IPv6 adres zone bilgisi olmadan URL'de kullanılamıyordu). Bulunamazsa
  `--tv IP` ile elle verilir.
- **Zaman aşımı (`withTimeout`):** ScreenCaptureKit çağrıları bazen hiç dönmüyor; task group yerine
  ilk sonuçla devam eden bir continuation yarışı kullanılır.

### TV tarafı: uygulama

- **Overlay penceresi:** `TYPE_APPLICATION_OVERLAY`, `FLAG_NOT_FOCUSABLE | FLAG_NOT_TOUCHABLE |
  FLAG_LAYOUT_IN_SCREEN`, `PixelFormat.TRANSLUCENT`, tam ekran. Pencere odak almadığı ve dokunulamadığı
  için kumanda, DPAD ve BACK tuşları altta çalışan uygulamaya gider. Android 11+'da overlay tipine bağlı
  bir window context ile oluşturulur. Ekran boyutu `WindowManager.currentWindowMetrics` ile alınır.
- **Çizim (`TranslationOverlayView`):** Compose değil, özel `View` + `Canvas`. Her blok için
  `StaticLayout` (ortalanmış, kalın, beyaz, gölgeli) bloklar değiştiğinde bir kez hesaplanır, `onDraw`
  sadece çizer. Font boyutu `lineH × ekran yüksekliği × 1,15`'ten başlar; `FontFitter` sığana kadar
  küçültür ama 1080p'de ~30 px'in altına inmez. Türkçe çeviri yine sığmazsa kutu merkezini koruyarak
  yatayda genişler (ekranın en fazla %90'ı).
- **Ortak state (`OverlayState`):** Demo blokları ve WebSocket blokları ayrı tutulur; `StateFlow` ile
  overlay'e ve ana ekrana yansır. WebSocket blokları son geçerli mesajdan **3 sn** sonra otomatik silinir
  (Mac kapanırsa ekranda eski yazı kalmasın). Demo bloğu bu süreden muaftır.
- **WebSocket server:** Ktor CIO, `0.0.0.0:8765`, ping 15 sn / timeout 30 sn. Her bağlantının kendi
  `seq` sayacı vardır; eski `seq`'li mesaj yok sayılır. Bozuk JSON loglanıp atlanır, bilinmeyen alanlar
  görmezden gelinir.
- **NSD:** `_ps5ceviri._tcp`, ad "PS5 Ceviri", port 8765.
- **Foreground service:** `foregroundServiceType="specialUse"`; overlay, server ve NSD bu servisin
  içinde yaşar, uygulama ekranı kapatılınca da çalışır. `BOOT_COMPLETED` ve `MY_PACKAGE_REPLACED` ile
  kendiliğinden başlar (ikisi de Android 12+ arka plan FGS kısıtından muaf). Kullanıcı en son
  "durdur" dediyse başlamaz.

### Mesaj protokolü (Mac → TV)

```json
{
  "v": 1,
  "seq": 123,
  "blocks": [
    { "id": "b7", "x": 0.12, "y": 0.81, "w": 0.60, "h": 0.08, "lineH": 0.03, "text": "Kapı kilitli." }
  ]
}
```

| Alan | Anlamı |
|---|---|
| `x, y, w, h` | Kutunun konumu ve boyutu, ekranın oranı olarak (0–1), origin sol üst |
| `lineH` | Orijinal satır yüksekliği (oran); font boyutunun başlangıç değeri |
| `seq` | Bağlantı içinde artan sayı; daha küçüğü gelirse yok sayılır |
| `blocks` | Ekranda olması gereken listenin **tamamı**; `[]` ekranı temizler |

### Tasarım kararları

- **Neden HDMI + Remote Play birlikte?** TV'de oyun görüntüsü hiç gecikmeden akar; çeviri ayrı bir
  katman olarak üstüne biner. Mac yalnızca okur, görüntünün yoluna girmez.
- **Neden normalize koordinat?** Remote Play penceresi 960×540 de olabilir, TV 4K da; oranlar
  her çözünürlükte aynıdır.
- **Neden 3 sn fail-safe?** Mac çökerse, ağ koparsa ya da Remote Play kapanırsa TV'de eski bir çeviri
  asılı kalmaz. Mac bu yüzden liste değişmese de saniyede 2 kez gönderir.
- **Neden önbellek + stabilizasyon?** DeepL kotası karakter başına sayılır; aynı yazının her karede
  yeniden çevrilmesi kotayı dakikalar içinde bitirirdi.

### Doğrulanan kritik testler

| Test | Sonuç |
|---|---|
| Overlay, Vestel TV'nin HDMI girişinde PS5 görüntüsünün üstünde görünüyor mu? | ✅ Görünüyor, kumandalar normal çalışıyor |
| ScreenCaptureKit, Remote Play penceresini yakalayabiliyor mu (korumalı içerik diye siyah kare vermeden)? | ✅ Yakalıyor |
| Mac'te bulunan konumlar TV'de orijinal yazının üstüne oturuyor mu? | ✅ Oturuyor |
| Gerçek oyunda uçtan uca çeviri (It Takes Two menüleri) | ✅ Çalışıyor, TV'de okunaklı |

---

## Sorun giderme

| Belirti | Sebep / Çözüm |
|---|---|
| TV uygulamasında "Overlay izni verilmedi" | `adb shell appops set com.isacetin.ps5ceviri SYSTEM_ALERT_WINDOW allow` |
| Mac'te `TCC'leri reddetti` (-3801) | Terminal'e **Ekran ve Sistem Sesi Kaydı** izni ver ve Terminal'i yeniden başlat |
| Mac'te `No route to host` veya TV bulunamadı | Terminal'e **Yerel Ağ** izni ver; TV'de overlay'in çalıştığını kontrol et; `--tv TV_IP` ile dene |
| Oynamaya başlayınca Remote Play kopuyor | DualSense PS5'e bağlı. Mac'e bağla ([Adım 4](#adım-4--kontrolcü)) |
| DualSense Mac'e bağlanmıyor | Bluetooth'ta "Bu Aygıtı Unut" de, PS5'i kapat, Create + PS ile yeniden eşleştir; ya da veri kablosuyla tak |
| İlk 30 sn TV'de hiçbir şey yok | Normal: Vision modeli yükleniyor |
| Kutular yazının üstüne tam oturmuyor | TV'nin resim formatını "Tam Piksel / Just Scan" yap (overscan). Remote Play pencere modundaysa `--titlebar` ile başlık yüksekliğini ayarla |
| Yazılar yanlış okunuyor | Remote Play penceresini büyüt veya tam ekran yap |
| `DeepL hatası: quotaExceeded` | Aylık 500.000 karakter doldu; yeni dönemde açılır. `--dry-run` ile çevirisiz kullanılabilir |

Faydalı komutlar:

```bash
.build/release/ps5ceviri run --verbose
```

```bash
.build/release/ps5ceviri run --dump dump
```

```bash
adb logcat -s OverlayService WebSocketServer NsdAdvertiser
```

Tüm Mac seçenekleri için: [mac/README.md](mac/README.md).

---

## Bilinen sınırlar

- Çeviri, TV görüntüsünün yaklaşık **0,5–1,5 sn** gerisinden gelir. Diyaloglar ve menüler için uygun;
  çok hızlı akan sinematik altyazılarda geride kalabilir.
- Android 12+ diğer uygulamaların üstüne çizilen pencereleri biraz saydam gösterir; kutu hafif
  saydam görünür.
- Kontrolcü Mac'e bağlı olduğu için tuşlar Remote Play üzerinden gider; çok az giriş gecikmesi olur.
- Yalnızca POSITION modu var (çeviri orijinal yazının yerinde). Alt şeritte gösteren SUBTITLE modu
  ileride eklenecek.

---

## Proje yapısı ve testler

```
.
├── app/                         Android TV uygulaması
│   └── src/main/java/com/isacetin/ps5ceviri/
│       ├── ui/                  MainActivity (Compose for TV), MainViewModel
│       ├── overlay/             OverlayService, TranslationOverlayView, OverlayState, FontFitter
│       ├── network/             WebSocketServer, TranslationMessage
│       ├── discovery/           NsdAdvertiser
│       └── BootReceiver.kt
├── mac/                         Mac aracı (Swift paketi); ayrıntılı başvuru: mac/README.md
│   ├── Sources/PS5CeviriCore/   Saf mantık: geometri, blok birleştirme, stabilizasyon, çeviri, protokol
│   ├── Sources/PS5CeviriMac/    Ekran okuma: ScreenCaptureKit akışı, Vision OCR, ScreenReader
│   ├── Sources/CeviriBridge/    PS5CeviriMac'i Rust'a açan C ABI (statik kütüphane)
│   └── Sources/ps5ceviri/       DeepL, WebSocket, TV bulma, komut satırı
├── desktop/                     Mac + Windows masaüstü uygulaması (Rust workspace, geliştiriliyor)
│   ├── core/                    Platformdan bağımsız mantık: PS5CeviriCore'un Rust karşılığı
│   ├── engine/                  Çalışan iş hattı: TV istemcisi, mDNS ile TV bulma, DeepL, ana döngü
│   ├── mac/                     Mac platform katmanı: CeviriBridge'i derleyip bağlar (build.rs)
│   ├── windows/                 Windows platform katmanı: Windows.Graphics.Capture + Windows.Media.Ocr
│   ├── cli/                     `ceviri` komut satırı aracı: `ps5ceviri` ile aynı komutlar
│   └── winprobe/                Windows'ta Remote Play yakalama + OCR test aracı (Faz 0)
└── tools/ws_test.py             Mac olmadan TV'ye sahte mesaj gönderen test scripti
```

`desktop/` iki işletim sistemi için yazılır: CI (`.github/workflows/desktop.yml`) her değişiklikte
Rust kodunu macOS **ve** Windows'ta derleyip test eder. Mac'te ekran okuma, gerçek oyunda doğrulanmış
Swift kodudur ve Rust'tan köprüyle çağrılır; Rust tarafı ana thread'i olay döngüsüne bırakıp okumayı
ayrı bir thread'de yapar. Rust aracı Swift aracıyla aynı komutları sunar (Remote Play açıkken):

```bash
cd desktop && cargo run --release -p ceviri-cli -- run --dry-run
```

TV olmadan denemek için sahte TV (gelen mesajları yazar, gerçek TV gibi ping atar):

```bash
cd desktop && cargo run -p ceviri-engine --example fake_tv -- 18765
```

```bash
cd desktop && cargo run -p ceviri-cli -- run --dry-run --tv 127.0.0.1:18765
```

**Unit testler:**

```bash
./gradlew testDebugUnitTest
```

```bash
cd mac && swift test
```

```bash
cd desktop && cargo test --workspace
```

TV tarafında 31 test bulunur: JSON parse, piksel dönüşümü, 3 sn timeout ve heartbeat (sanal zamanla),
seq kuralı, FontFitter ve kutu genişletme. Mac tarafında 38 test bulunur: video alanı, Vision koordinat
dönüşümü, blok birleştirme, stabilizasyon, Türkçe karakter karşılaştırma, çeviri önbelleği ve hata
beklemesi, DeepL istek/yanıt formatı.

**Mac olmadan TV'yi test etmek:**

```bash
pip3 install websockets
python3 tools/ws_test.py TV_IP
```

Script sırasıyla heartbeat, liste değişimi, boş listeyle temizleme, 3 sn timeout, eski `seq` ve bozuk
JSON senaryolarını gösterir.
