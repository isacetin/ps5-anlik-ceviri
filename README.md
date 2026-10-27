# Anlık Çeviri

PS5'te oynadığınız oyunlardaki İngilizce yazıları **anında Türkçeye çevirir** ve çeviriyi orijinal
yazının **tam üstünde** gösterir: TV'de ya da bilgisayar ekranında.

```
 PS5 ──── HDMI ────▶ TV  ◀──── Türkçe çeviri ────┐
  │                                               │
  └── PS Remote Play ──▶ Bilgisayar (Mac / Windows)
                         yazıları okur ve çevirir
```

Oyunu her zamanki gibi TV'den, gecikmesiz izlersiniz. Bilgisayar aynı görüntüyü PS Remote Play
üzerinden okur, yazıları bulup çevirir ve TV'ye (ya da kendi ekranına) gönderir.

> Geliştirme aşamasında. Mac + Vestel Android TV + PS5 ile denendi; Windows sürümü henüz gerçek bir
> bilgisayarda denenmedi.

## Neler gerekiyor?

- **PS5** ve ücretsiz **PS Remote Play** uygulaması
- **Mac** (macOS 15 ya da üstü, Apple Silicon) veya **Windows 10/11** bilgisayar
- Çeviriyi TV'de görmek için **Android TV / Google TV** (ör. Vestel, TCL, Philips, Sony).
  Samsung ve LG TV'lerde çeviri bilgisayar ekranında gösterilir.
- Ücretsiz **DeepL API Free** hesabı (ayda 500.000 karakter)
- TV, bilgisayar ve PS5 **aynı ev ağında**

## İndirme

| Sistem | Dosya | Boyut |
|---|---|---|
| Mac (Apple Silicon) | `Anlık Çeviri_0.1.0_aarch64.dmg` | 13 MB |
| Windows (64 bit) | `Anlık Çeviri_0.1.0_x64-setup.exe` | 12 MB |

Kurulum dosyaları şimdilik GitHub'da **Actions → Paketler** sayfasındaki son çalışmanın
**Artifacts** bölümündedir. TV uygulaması dosyalara gömülüdür; ayrıca indirmeniz gerekmez.

## Kurulum

**Mac:** `.dmg` dosyasını açın ve **Anlık Çeviri**'yi Uygulamalar klasörüne sürükleyin. İlk açılışta
"doğrulanamadı" uyarısı çıkarsa **Sistem Ayarları → Gizlilik ve Güvenlik** altındaki
**Yine de Aç** düğmesine basın.

**Windows:** `.exe` dosyasını çalıştırın. "Windows bilgisayarınızı korudu" uyarısı çıkarsa
**Ek bilgi → Yine de çalıştır** deyin. Kurulum Türkçedir.

> Bu uyarılar, uygulama henüz Apple ve Microsoft tarafından imzalanmadığı için çıkar.

Uygulama ilk açıldığında **kurulum sihirbazı** her şeyi adım adım yaptırır:

1. Çevirileri nerede görmek istediğinizi seçersiniz (TV, bilgisayar ekranı ya da ikisi).
2. Mac'te ekranı okuma iznini verirsiniz.
3. PS Remote Play'i açıp PS5'e bağlanırsınız.
4. **TV uygulamasını sihirbaz kurar.** Siz yalnızca TV'de bir ayarı açarsınız (Ayarlar → Geliştirici
   seçenekleri → **Ağ üzerinden hata ayıklama**) ve TV'nin sorduğu izni onaylarsınız. Kurulum,
   TV'de bir test yazısıyla biter.
5. Kontrolcüyü (DualSense) bilgisayara bağlarsınız.
6. DeepL anahtarınızı girersiniz ya da önce çevirisiz deneme moduyla başlarsınız.

## Kullanım

1. PS5'i ve TV'yi açın; bilgisayarda PS Remote Play'i açıp PS5'e bağlanın.
2. **Anlık Çeviri**'yi açıp **Başlat**'a basın.
3. TV'de PS5'in HDMI girişine geçip oynayın. İlk açılışta yazıların görünmesi ~30 saniye sürer.

**Ayarlar**'dan şunları değiştirebilirsiniz: çevirilerin nerede gösterileceği, çevrilmemesi gereken
adlar (oyun, karakter, yer adları), çevirisiz test modu ve okuma hızı. Kurulum sihirbazı da buradan
yeniden açılır.

## TV uygulamasını kaldırma

TV uygulaması sıradan bir uygulamadır; TV'nize zarar vermez ve istediğiniz zaman kaldırılır:

- TV'de **Ayarlar → Uygulamalar → PS5 Çeviri → Kaldır**, ya da
- Anlık Çeviri'de **Ayarlar → TV uygulamasını kaldır**.

Kurulumdan sonra TV'deki "Ağ üzerinden hata ayıklama"yı kapatabilirsiniz; TV uygulaması çalışmaya
devam eder (yalnızca güncellemek için yeniden açmanız gerekir).

## Sık karşılaşılan sorunlar

| Sorun | Çözüm |
|---|---|
| Sihirbaz TV'yi bulamıyor | TV'de "Ağ üzerinden hata ayıklama" açık mı? TV ve bilgisayar aynı modemde mi? TV'nin adresini (Ayarlar → Ağ ve İnternet) elle girin. |
| Oynamaya başlayınca Remote Play kopuyor | Kontrolcü PS5'e bağlı. Bilgisayara bağlayın (kablo ya da Bluetooth). |
| Yazılar yanlış okunuyor | Remote Play penceresini büyütün ya da tam ekran yapın. |
| TV'de kutular yazının üstüne tam oturmuyor | TV'nin resim formatını "Tam Piksel" ya da "Just Scan" yapın. |
| Oyun ya da karakter adı çevriliyor | Ayarlar → **Çevrilmeyecek adlar**'a ekleyin. |
| "DeepL: kota doldu" | Aylık 500.000 karakter bitti; yeni ayda açılır. Bu arada çevirisiz modu kullanabilirsiniz. |

## Bilinen sınırlar

- Çeviri, görüntünün yaklaşık **0,5–1,5 saniye** gerisinden gelir; çok hızlı akan altyazılarda geride
  kalabilir.
- Oyun logolarındaki süslü yazılar bazen yanlış okunur.
- Mac'te Remote Play **tam ekrandayken** bilgisayar ekranı modunun çalışması henüz denenmedi.
- Kurulum dosyaları henüz imzasızdır (yukarıdaki uyarılar).

## Geliştiriciler

Mimari, iş hattının ayrıntıları, TV protokolü, kaynak koddan derleme ve testler:
**[docs/TEKNIK.md](docs/TEKNIK.md)**.

```bash
./gradlew assembleRelease
```

```bash
cd desktop && cargo run --release -p ceviri-app
```

```bash
cd desktop && cargo test --workspace
```
