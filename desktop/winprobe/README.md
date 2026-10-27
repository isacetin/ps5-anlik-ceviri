# winprobe — Faz 0 Windows testi

Ürünleşmenin ilk riskini ölçen küçük bir Windows aracı. Mac'te doğrulanan "kritik test A"nın
(Remote Play penceresi siyah kare vermeden yakalanıyor mu?) Windows karşılığıdır. Ek olarak
Windows'un yerleşik OCR'ının (`Windows.Media.Ocr`) oyun yazılarındaki kalitesini ölçer.

| Mac aracı (`mac/`) | winprobe |
|---|---|
| ScreenCaptureKit `SCStream` | `Windows.Graphics.Capture` (D3D11 frame pool) |
| Vision `RecognizeTextRequest` | `Windows.Media.Ocr` |
| `Geometry.videoRect`, `FrameStats` | `ceviri-core` (`desktop/core`, ortak) |

Kullanıcı talimatları: [KULLANIM.txt](KULLANIM.txt) (exe ile birlikte verilir).

## Mac'ten derleme

```bash
brew install rustup mingw-w64
```

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH" && rustup target add x86_64-pc-windows-gnu
```

`desktop/` klasöründe:

```bash
cargo build --release --target x86_64-pc-windows-gnu -p winprobe
```

Çıktı: `desktop/target/x86_64-pc-windows-gnu/release/winprobe.exe`. CRT statik bağlanır; yalnızca
Windows 10+'da hazır bulunan sistem DLL'lerine ihtiyaç duyar. CI ayrıca her değişiklikte Windows
runner'ında MSVC ile derler.

## Sonuçları Vision ile karşılaştırma

Windows'tan gelen `tN_video_XX.png` karelerini Mac'teki Vision ile okut; çıktı biçimi aynıdır:

```bash
cd ../../mac && swift build && .build/debug/ps5ceviri ocr ~/Downloads/winprobe-sonuc-1/t1_video_*.png
```

## Faz 0 karar kriterleri

- **Yakalama:** Hem pencere hem tam ekran modunda karelerin çoğu siyah değilse geçer.
- **OCR:** Aynı karelerde Windows OCR, Vision'ın bulduğu İngilizce satırların büyük kısmını doğru
  okuyorsa yerleşik OCR yeterli. Değilse ürün PaddleOCR/ONNX gibi paketli bir OCR'a gider.
- **Süre:** Tur başına OCR süresi ~300 ms'nin altındaysa saniyede 2 tur hedefi korunur.
