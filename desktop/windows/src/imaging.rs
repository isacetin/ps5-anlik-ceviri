//! Platformdan bağımsız görüntü işlemleri (her işletim sisteminde derlenir ve test edilir).

use ceviri_core::Rect;
use image::{imageops, GenericImageView, Rgba, RgbaImage};

/// Piksel dikdörtgeni görüntü sınırlarına kırparak keser; en az 1x1 döner.
pub fn crop(image: &RgbaImage, rect: Rect) -> RgbaImage {
    let x = (rect.x.round().max(0.0) as u32).min(image.width().saturating_sub(1));
    let y = (rect.y.round().max(0.0) as u32).min(image.height().saturating_sub(1));
    let w = (rect.width.round() as u32).min(image.width() - x).max(1);
    let h = (rect.height.round() as u32).min(image.height() - y).max(1);
    imageops::crop_imm(image, x, y, w, h).to_image()
}

/// Kare neredeyse tamamen siyah mı? Özel tam ekran (exclusive fullscreen) oyunlar ve korumalı
/// içerik yakalamada siyah gelir. Hız için seyrek bir ızgara örneklenir.
pub fn is_mostly_black(image: &impl GenericImageView<Pixel = Rgba<u8>>) -> bool {
    const STEP: usize = 16;
    let (w, h) = image.dimensions();
    let (mut bright, mut total) = (0u32, 0u32);
    for y in (0..h).step_by(STEP) {
        for x in (0..w).step_by(STEP) {
            let [r, g, b, _] = image.get_pixel(x, y).0;
            total += 1;
            if r.max(g).max(b) > 16 {
                bright += 1;
            }
        }
    }
    // Tamamen siyah bir yükleme ekranında bile genelde birkaç piksel (logo, ipucu) parlaktır.
    bright * 200 < total
}

/// OCR'dan önce `scale` kat büyütür/küçültür; uzun kenar `max_dimension`'ı aşmaz
/// (Windows OCR daha büyük görüntüyü tanımaz). Oran korunduğu için normalize kutular değişmez.
pub fn prepare_for_ocr(image: &RgbaImage, scale: f64, max_dimension: u32) -> RgbaImage {
    let (w, h) = image.dimensions();
    let mut factor = scale;
    let longest = w.max(h) as f64 * factor;
    if max_dimension > 0 && longest > max_dimension as f64 {
        factor *= max_dimension as f64 / longest;
    }
    if (factor - 1.0).abs() <= 1e-3 {
        return image.clone();
    }
    let nw = ((w as f64 * factor).round() as u32).max(1);
    let nh = ((h as f64 * factor).round() as u32).max(1);
    imageops::resize(image, nw, nh, imageops::FilterType::CatmullRom)
}

/// Debug için: normalize kutuları (origin sol üst) kırmızı çerçeve olarak çizer.
pub fn draw_boxes(image: &RgbaImage, boxes: &[Rect]) -> RgbaImage {
    let mut out = image.clone();
    let (w, h) = (out.width() as f64, out.height() as f64);
    let red = Rgba([255, 40, 40, 255]);
    for b in boxes {
        let x0 = (b.x * w) as i64;
        let y0 = (b.y * h) as i64;
        let x1 = (b.right() * w) as i64;
        let y1 = (b.bottom() * h) as i64;
        for t in 0..2 {
            for x in x0..=x1 {
                put(&mut out, x, y0 - t, red);
                put(&mut out, x, y1 + t, red);
            }
            for y in y0..=y1 {
                put(&mut out, x0 - t, y, red);
                put(&mut out, x1 + t, y, red);
            }
        }
    }
    out
}

fn put(image: &mut RgbaImage, x: i64, y: i64, color: Rgba<u8>) {
    if x >= 0 && y >= 0 && (x as u32) < image.width() && (y as u32) < image.height() {
        image.put_pixel(x as u32, y as u32, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_alani_kesilir() {
        let image = RgbaImage::new(1280, 751);

        let video = crop(&image, Rect::new(0.0, 31.0, 1280.0, 720.0));

        assert_eq!(video.dimensions(), (1280, 720));
    }

    #[test]
    fn tasan_dikdortgen_sinira_kirpilir() {
        let image = RgbaImage::new(100, 100);

        assert_eq!(crop(&image, Rect::new(90.0, -5.0, 50.0, 50.0)).dimensions(), (10, 50));
        assert_eq!(crop(&image, Rect::new(500.0, 500.0, 10.0, 10.0)).dimensions(), (1, 1));
    }

    #[test]
    fn siyah_kare_taninir_altyazili_karanlik_sahne_taninmaz() {
        let black = RgbaImage::from_pixel(640, 360, image::Rgba([3, 3, 3, 255]));
        assert!(is_mostly_black(&black));

        // Karanlık sahne, altta beyaz altyazı bandı: siyah sayılmaz.
        let mut scene = black.clone();
        for y in 300..330 {
            for x in 100..540 {
                scene.put_pixel(x, y, image::Rgba([240, 240, 240, 255]));
            }
        }
        assert!(!is_mostly_black(&scene));
    }

    #[test]
    fn ocr_icin_azami_boyuta_kucultulur_ve_oran_korunur() {
        // Gerçekte 4K kare ve ~2600 px sınır; test hızlı olsun diye 1/10 ölçek.
        let image = RgbaImage::new(384, 216);

        assert_eq!(prepare_for_ocr(&image, 1.0, 260).dimensions(), (260, 146));
        assert_eq!(prepare_for_ocr(&image, 1.0, 1000).dimensions(), (384, 216));
        // Büyütme istense de azami boyut aşılmaz.
        assert_eq!(prepare_for_ocr(&RgbaImage::new(160, 90), 2.0, 260).dimensions(), (260, 146));
    }

    #[test]
    fn kutu_cercevesi_cizilir() {
        let image = RgbaImage::new(100, 100);

        let boxed = draw_boxes(&image, &[Rect::new(0.1, 0.1, 0.5, 0.5)]);

        assert_eq!(boxed.get_pixel(10, 30), &Rgba([255, 40, 40, 255]));
        assert_eq!(boxed.get_pixel(30, 30), &Rgba([0, 0, 0, 0]));
    }
}
