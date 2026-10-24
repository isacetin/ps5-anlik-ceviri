# PS5 Çeviri — Mac tarafı

PS Remote Play penceresini okur, İngilizce metni Türkçeye çevirir ve TV'deki overlay'e gönderir.

```
Remote Play penceresi ─ScreenCaptureKit─▶ 16:9 video alanı ─Vision OCR─▶ satırlar
  ─▶ paragraf blokları ─▶ stabilizasyon ─▶ DeepL EN→TR (diskte önbellek) ─WebSocket─▶ TV
```

Bağımlılık yok; yalnızca macOS framework'leri. macOS 15+, Xcode / Swift 6.

## Kurulum

```bash
cd mac
swift build -c release
```

Çalıştırılabilir dosya: `.build/release/ps5ceviri`.

### macOS izinleri

Aracı hangi uygulamadan çalıştırıyorsan (Terminal, iTerm, Claude...) ona şunlar gerekir:

- **Ekran ve Sistem Sesi Kaydı** — Sistem Ayarları → Gizlilik ve Güvenlik. İzni verdikten sonra
  uygulamayı yeniden başlat. İzin yoksa hata: `TCC'leri reddetti` (-3801).
- **Yerel Ağ** — Sistem Ayarları → Gizlilik ve Güvenlik → Yerel Ağ. TV'ye bağlanmak ve NSD ile bulmak için.

### DeepL API anahtarı

1. deepl.com'da **DeepL API Free** hesabı aç (ayda 500.000 karakter ücretsiz; kayıtta kart istenir,
   Free planda ücret çekilmez). Uygulama aboneliği olan "DeepL Pro" değil, **API** planı.
2. Hesabındaki **API Keys** sekmesinden anahtarı kopyala (Free anahtarlar `:fx` ile biter).
3. Mac mini'de anahtarı ekrana yazdırmadan dosyaya kaydet:

```bash
mkdir -p ~/Library/Application\ Support/PS5Ceviri && read -rs "KEY?DeepL API anahtarı: " && print -r -- "$KEY" > ~/Library/Application\ Support/PS5Ceviri/deepl-key.txt && chmod 600 ~/Library/Application\ Support/PS5Ceviri/deepl-key.txt; unset KEY
```

Alternatif: `DEEPL_AUTH_KEY` ortam değişkeni (dosyadan önce okunur). Anahtar repoya girmez.

Anahtarı dene:

```bash
.build/release/ps5ceviri translate "The door is locked. You need to find the key."
```

Beklenen çıktı: `… → Kapı kilitli. Anahtarı bulman gerekiyor.`

## Kullanım

Önce TV'de PS5 Çeviri uygulamasında overlay'i başlat, Mac'te Remote Play'i aç ve PS5'e bağlan.
Daha okunaklı OCR için Remote Play'i büyük pencerede veya tam ekranda çalıştır.

### Kontrolcü: DualSense Mac'e bağlı olmalı

PS5'e bağlı bir kontrolcüyle oynamaya başlayınca PS5 "Bu kontrol cihazını kim kullanıyor?" diye sorar ve
Remote Play bağlantısı kopar. Bu yüzden DualSense **Mac mini'ye** bağlanır, oyun Remote Play üzerinden
oynanır, görüntü TV'den (HDMI, gecikmesiz) izlenir:

- **Kablo:** DualSense'i USB-C kabloyla Mac mini'ye tak.
- **Bluetooth:** DualSense'te **Create** (sol üstteki küçük tuş) ve **PS** tuşlarına ışık çubuğu hızlı
  hızlı yanıp sönene kadar birlikte bas, sonra Mac'te Sistem Ayarları → Bluetooth → DualSense → Bağlan.

Kontrolcüyü tekrar doğrudan PS5'le kullanmak için USB kabloyla PS5'e takıp PS tuşuna bas.

Çeviriyle:

```bash
.build/release/ps5ceviri run
```

TV yerel ağda NSD ile bulunur. Bulunamazsa adresi elle ver:

```bash
.build/release/ps5ceviri run --tv 192.168.1.5
```

Çevirisiz hizalama testi (DeepL kotası harcamaz, İngilizce metni olduğu gibi gönderir):

```bash
.build/release/ps5ceviri run --dry-run
```

Seçenekler:

| Seçenek | Açıklama |
|---|---|
| `--tv HOST[:PORT]` | TV adresi; verilmezse NSD ile aranır, port varsayılan 8765 |
| `--dry-run` | DeepL'e gitmeden İngilizce metni gönderir |
| `--fps N` | Saniyedeki OCR sayısı (varsayılan 2, en az 1) |
| `--titlebar PT` | Başlık çubuğu yüksekliği (varsayılan: tam ekranda 0, pencerede 28) |
| `--dump KLASÖR` | Her turda OCR bloklarını çizilmiş kareyi kaydeder |
| `--verbose` | Her turun yakalama/OCR süresini yazar |
| `--once` | Tek tur çalışıp çıkar |

Yardımcı komutlar:

```bash
.build/release/ps5ceviri windows
```

```bash
.build/release/ps5ceviri capture --count 5 --out frames
```

## Nasıl çalışır

- **Video alanı:** Pencere görüntüsünden başlık çubuğu çıkarılır, kalan alana 16:9 video sığdırılır
  (tam ekranda ultra geniş monitördeki yan bantlar atılır). Koordinatlar bu alana göre normalize edilir,
  bu yüzden TV'deki HDMI görüntüsüyle birebir örtüşür.
- **Bloklar:** Alt alta, benzer boyutta ve solda/ortada hizalı satırlar tek paragrafa birleştirilir;
  DeepL'e cümle bütünlüğüyle gider. Aynı satırda bölünmüş parçalar soldan sağa birleştirilir.
- **Stabilizasyon:** Metin 2 kare aynı kalınca gösterilir (daktilo efekti bitmeden çevrilmez). Ekrandaki
  bir blok değişirse yeni metin oturana kadar eskisi kalır; bir kare kaybolan blok 0,8 sn tutulur.
- **Çeviri:** Önbellekte olmayan metinler toplu (en fazla 50) istenir; önbellek
  `~/Library/Application Support/PS5Ceviri/translations.json` dosyasında tutulur, tekrar eden metin
  kota harcamaz. Kota dolarsa (456) 5 dk, hız sınırında (429) 10 sn beklenir.
- **TV:** Liste değişmese de her tur gönderilir (heartbeat); TV 3 sn mesaj almazsa yazıları kaldırır.

## Bilinen sınırlar

- PS5'e bağlı kontrolcüyle oynanırken Remote Play bağlantısı kopar (bkz. "Kontrolcü"). Remote Play
  koparsa araç takılmaz; pencere tekrar oyuna bağlanınca kendiliğinden devam eder.

- Program her açılışta ilk OCR ~30 sn sürer (Vision modeli yüklenir); sonrasında tur başına ~170 ms.
- Çeviri HDMI görüntüsünün yaklaşık 0,5–1,5 sn gerisinden gelir: diyalog ve menüler için uygun,
  hızlı akan altyazılar için zayıf.
- 960x540 gibi küçük pencerelerde küçük yazılar yanlış okunabilir.

## Testler

```bash
swift test
```
