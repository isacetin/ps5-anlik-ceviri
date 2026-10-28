# Yerel çeviri modelleri ile DeepL karşılaştırması

Tarih: 28 Ekim 2026. Makine: Mac mini, Apple M4 Pro, 64 GB.

## Test seti

Uygulamanın çeviri önbelleğinden (`translations.json`, 285 kayıt) seçilmiş 129 İngilizce metin:

| Kategori | Adet | Örnek |
|---|---|---|
| D: Diyalog (It Takes Two altyazıları) | 31 | `- I know. Ive gotta work overtime.` |
| M: Menü / UI | 80 | `Master Volume`, `Press to Sprint`, `May 1 Wins` |
| N: Korunacak ad içeren | 7 | `It Takes Two`, `PlayStation Store` |
| O: OCR hatalı | 11 | `Play Local Pl1y Online`, `Hove you, cody.` |

Zaten Türkçe olan, yalnızca sayı ya da logo olan ve anlamsız OCR parçaları test setine alınmadı. Her metin için elle bir referans çeviri yazıldı.

## Yöntem

- **İnsan puanı:** Her çıktı tek tek okunup 2 (iyi), 1 (anlaşılır ama kusurlu) ya da 0 (yanlış/bozuk) ile notlandı. Tablodaki yüzde, alınan puanın en yüksek puana oranıdır. Notlandırmayı tek kişi yaptı.
- **chrF++:** Elle yazılan referansa göre otomatik benzerlik puanı. Kısa menü metinlerinde eş anlamlı doğru çevirileri ("Ana Ses" ile "Ana Ses Düzeyi" gibi) cezalandırdığı için yalnızca yardımcı ölçü olarak kullanıldı.
- **DeepL:** Çıktılar önbellekte duran gerçek DeepL çevirileridir. `It Takes Two` ve `PayStation Store` satırları, uygulamanın bugünkü ad koruması (`terms.rs`) varsayılarak notlandı. Önbellekteki eski çıktılar bu özellik eklenmeden önce üretilmişti.
- **LLM'ler:** Ollama üzerinden çalıştırıldı (Q4 nicemleme, sıcaklık 0, Metal GPU). Sistem mesajında oyun bağlamı, OCR uyarısı ve korunacak adlar listesi verildi.
- **Opus-MT ve NLLB:** `transformers` ile CPU'da, 4 iş parçacığı ve beam 4 ile çalıştırıldı.
- **Süre:** Tek metin için uçtan uca yanıt süresi. Model zaten belleğe yüklüydü; ilk yükleme süresi dahil değil.

## Sonuçlar

| Model | Tür | İndirme | İnsan % | Diyalog | Menü | Adlar | OCR | 0 alan satır | chrF++ | Süre (medyan) | Lisans |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **DeepL** (bulut) | API | – | **91** | 95 | 93 | 93 | 59 | 9 | 75.2 | ağ gecikmesi | Ücretli API, ayda 500 bin karakter ücretsiz |
| qwen3.6 35B-A3B | LLM | 22 GB | 86 | 79 | 92 | 64 | 73 | 7 | 70.4 | 1,0 sn (GPU) | Ürüne konamayacak kadar büyük, yalnızca üst sınır |
| gemma3 12B | LLM | 8,2 GB | 79 | 73 | 84 | 71 | 64 | 15 | 62.1 | 1,35 sn (GPU) | Gemma Kullanım Koşulları |
| **TranslateGemma 4B** | Çeviri LLM'i | 3,3 GB | **78** | 74 | 81 | 86 | 64 | 13 | 57.4 | 0,73 sn (GPU), 0,97 sn (CPU) | Gemma Kullanım Koşulları |
| **Opus-MT tc-big en→tr** | Çeviri modeli | ~250 MB (int8) | **74** | **94** | 74 | 71 | 18 | 22 | 62.4 | **0,05 sn (CPU)** | CC-BY 4.0, ticari kullanım serbest |
| NLLB-200 distilled 600M | Çeviri modeli | ~600 MB (int8) | 60 | 77 | 61 | 50 | 18 | 30 | 55.0 | 0,33 sn (CPU) | **CC-BY-NC 4.0, ticari kullanım yasak** |
| gemma3 4B | LLM | 3,4 GB | 58 | 42 | 66 | 50 | 50 | 36 | 49.9 | 0,68 sn (GPU) | Gemma Kullanım Koşulları |
| qwen2.5 7B | LLM | 4,7 GB | 49 | 34 | 55 | 50 | 45 | 45 | 42.6 | 0,36 sn (GPU) | |
| qwen2.5 3B | LLM | 1,9 GB | 23 | 19 | 24 | 29 | 27 | 88 | 29.4 | 0,22 sn (GPU) | |
| llama3.2 3B | LLM | 2,0 GB | 17 | 16 | 21 | 14 | 0 | 97 | 31.8 | 0,23 sn (GPU) | |
| gemma3 1B | LLM | 0,8 GB | 10 | 10 | 11 | 14 | 0 | 112 | 13.1 | 0,24 sn (GPU) | |

qwen3:4b teste alınamadı: Ollama'daki sürümü `think: false` ayarını yok sayıp çıktının başına uzun bir düşünme metni ekliyor, her istek ~7 saniye sürüyor.

## Gözlemler

- **Hiçbir yerel model DeepL'e yetişmiyor.** Ürüne konabilecek boyuttaki en iyi iki model, DeepL'in 91 puanına karşı 74–78 puanda.
- **Genel amaçlı küçük LLM'ler (1–4B) Türkçe için kullanılamaz.** Anlamı tersine çeviriyorlar ("No, I didn't forget" → "Hayır, hatırlamadım"), Çince karakterler basıyorlar (qwen2.5 3B), kendi kafalarından metin uyduruyorlar (gemma3 1B).
- **TranslateGemma 4B, aynı boyuttaki gemma3 4B'den 20 puan daha iyi.** Çeviriye özel eğitimin etkisi büyük. Korunacak adlarda en iyi sonucu o veriyor (86). Ama ara sıra uydurma ekliyor: "I'm not in the mood" → "şu an oyun oynamaya uygun değilim".
- **Opus-MT diyalogda DeepL ile başa baş (94'e 95), hem de 0,05 saniyede.** Zayıf olduğu iki alan var: oyun menüsü terimlerini İngilizce bırakıyor ("Master Volume", "Minigames", "Kamera Shakes") ve OCR hatalarını düzeltemiyor (18).
- **NLLB-200 her kategoride Opus-MT'nin gerisinde kalıyor.** Üstüne lisansı ticari kullanıma kapalı.
- **OCR hatalarında herkes zorlanıyor, DeepL dahil (59).** Bunu en iyi büyük LLM'ler çözüyor: "Pl1y" → "Play".
- **Her modelin yaptığı ortak hatalar:** "May 1 Wins" → "1 Mayıs" (DeepL, Opus, NLLB ve TranslateGemma'da var), "Draw" → "Çiz" (beraberlik değil), "Speakers" → "Konuşmacılar".

## Sınırlar

- Test setinin çoğu tek bir oyundan (It Takes Two) ve PS5 menülerinden geliyor. Başka türde oyunlarda sonuç farklı olabilir.
- Notlandırmayı tek kişi yaptı. Puanların ±5 civarında oynayabileceği kabul edilmeli.
- Süreler güçlü bir M4 Pro'da ölçüldü. GPU'suz, sıradan bir Windows dizüstünde 4B bir LLM'in birkaç kat yavaş (metin başına 2–4 sn) çalışması beklenir. Windows ölçümü sona bırakıldı.
- Opus-MT ve NLLB, `transformers` ile fp32 olarak çalıştırıldı. Uygulamaya CTranslate2 int8 ile gömülürse daha da hızlı ve küçük olurlar. Opus-MT için bu, aşağıda ölçüldü.

# Opus-MT ayrıntılı test

Karar Opus-MT'den yana olduğu için modeli, uygulamaya gömüleceği biçimde (CTranslate2, int8) ayrıca test ettim.

## int8 sürümü

| | fp32 (`transformers`) | int8 (CTranslate2) |
|---|---|---|
| Boyut | ~950 MB | **228 MB** |
| Süre, 1 iş parçacığı (medyan / en kötü) | 50 ms (4 iş parçacığı) | 72 ms / 402 ms |
| Süre, 4 iş parçacığı | | **37 ms** / 212 ms |
| Model yükleme + ilk çeviri | | 0,7 sn |
| Bellek (Python dahil) | | ~430 MB |

- 129 metinlik ilk sette 111 çıktı fp32 ile birebir aynı. Değişenler çoğunlukla nötr, birkaçı daha iyi ("X Proceed" → "X Devam Et"), birkaçı biraz daha kötü ("5 Wins" → "5 Zaferler"). Kalite kaybı yok sayılabilir.
- Beam 1 (greedy) belirgin şekilde hızlı değil (72 ms yerine 65 ms) ama daha çok değişiklik getiriyor. Beam 4 kalmalı.
- `•`, `©`, `›` gibi karakterler modelin sözlüğünde yok ve ` ⁇ ` olarak çıkıyor. Baştaki işaretler çeviriden önce ayrılmalı, sonra geri eklenmeli; ortada kalan ` ⁇ ` atılmalı.

## Yeni oyun diyaloglarıyla test (65 satır)

Önbellekteki diyalogların neredeyse hepsi tek bir oyundan geldiği için, değişik türlerden 65 yeni satır yazdım. DeepL bu sete uygulamadaki ayarlarla, canlı olarak çağrıldı (adlar `<x>` içinde).

| Kategori | Adet | DeepL | Opus-MT int8 |
|---|---|---|---|
| Aksiyon ("Get down!", "They're flanking us!") | 10 | 85 | 85 |
| Günlük konuşma | 20 | 100 | **98** |
| Hikâye / NPC | 10 | 100 | **95** |
| Görev / eğitim / sistem mesajı | 15 | 97 | 77 |
| Bölünmüş altyazı parçası | 5 | 100 | 90 |
| Deyim ("piece of cake") | 5 | 100 | **40** |
| **Toplam** | 65 | **97** | **85** |

- **Konuşmada Opus-MT, DeepL'e çok yakın.** Bazen DeepL'den daha doğal bile: "Reloading!" için DeepL "Yeniden yükleniyor!", Opus-MT "Dolduruyorum!" dedi.
- **Deyimleri kelimesi kelimesine çeviriyor:** "Break a leg" → "Bir bacağını kır", "It's a piece of cake" → "Bu bir parça kek", "Speak of the devil" → "Şeytandan söz et".
- **Sistem mesajlarında küçük kusurlar var:** "Kontrol noktası ulaşıldı", "Yeni beceri kilidi", "Ana menüden çıkmak" ("menüye" olmalı), "guardian" → "vasi".
- **Uydurma yok.** LLM'lerin aksine Opus-MT metne bir şey eklemiyor; en kötü hâlinde birebir çeviri yapıyor.
- **Uydurma adlar (Kai, Mira, Voss, Eldermoor, Brightwater) korumasız da aynen kaldı.**

## Ad koruma: yer tutucu

Opus-MT'nin sorunu, sıradan kelimelerden oluşan adlarla: "It Takes Two" → "İki tane alır", "May 1 Wins" → "1 Mayıs Kazandı", "Ghost of Tsushima" → "Tsushima Hayaleti". DeepL'deki `<x>` etiketinin karşılığı olarak, adı çeviriden önce bir yer tutucuyla değiştirip sonra geri koymayı denedim. 18 cümle ve 3 yöntem:

| Yöntem | "It Takes Two" | "May 1 Wins" | "Ghost of Tsushima" | Bozulan yer tutucu |
|---|---|---|---|---|
| Koruma yok | İki tane alır. | 1 Mayıs Kazandı | Tsushima Hayaleti | – |
| Simgesel belirteç (`ZXQ0`) | It Takes Two | May 1 Kazandı | Ghost of Tsushima | 0 / 18 |
| Vekil ad (`Alex`, `Sam`) | It Takes Two | May 1 Kazandı | Ghost of Tsushima | 0 / 18 |

Her iki yöntem de işe yarıyor. Tek kusur: addan sonraki ek, yer tutucuya göre çekildiği için ünlü uyumu bazen tutmuyor ("PlayStation Store'den", "The Shed'a"). Yer tutucu geri konduktan sonra, kesme işaretinden sonraki eki adın son hecesine göre düzelten küçük bir kural gerekiyor. Önerim simgesel belirteç; çünkü vekil adlar, gerçek bir karakter adıyla çakışabilir.

## Sonuç

Opus-MT int8 uygulamaya uygun: 228 MB, cümle başına ~40–100 ms, uydurma yok. Konuşma altyazılarında DeepL'e yakın kalite veriyor. Uygulamada ele alınması gerekenler:

1. **Ad koruma:** `terms.rs`'teki `<x>` etiketi yerine yer tutucu kullanılacak, ardından ek uyumu düzeltilecek.
2. **Baştaki işaretler:** `•`, `›`, `©` gibi işaretler çeviriden önce ayrılacak, sonra geri eklenecek.
3. **Menü terimleri ve deyimler:** Model bunları zayıf çeviriyor. Sık geçen oyun menüsü ifadeleri ("Master Volume", "Minigames", "Credits", "Speakers", "Level Up!") ve yaygın deyimler için, metnin tamamı birebir eşleşince kullanılan küçük bir sözlük kalan farkın önemli bir kısmını kapatır. Sözlüğün etkisi ölçülmedi; test setinden türetilen bir sözlükle ölçmek yanıltıcı olurdu.
4. **OCR hataları:** Opus-MT "Pl1y" gibi hataları düzeltemiyor (18 puan). Bu sorun çeviriden önce, OCR tarafında ele alınmalı.

