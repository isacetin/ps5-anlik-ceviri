/// Basit dikdörtgen. Bağlama göre piksel ya da 0...1 normalize değer tutar.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect { x, y, width, height }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
}

/// PS5 görüntüsünün en-boy oranı.
pub const VIDEO_ASPECT: f64 = 16.0 / 9.0;

/// Remote Play penceresinin yakalanan görüntüsünde oyun videosunun kapladığı alan (piksel).
/// Üstteki başlık çubuğu (`top_inset`) çıkarılır, kalan alana 16:9 video ortalanarak sığdırılır
/// (tam ekranda 21:9 monitördeki yan siyah bantlar böylece dışarıda kalır).
pub fn video_rect(image_width: f64, image_height: f64, top_inset: f64) -> Rect {
    let available_height = (image_height - top_inset).max(0.0);
    let mut width = image_width;
    let mut height = width / VIDEO_ASPECT;
    if height > available_height {
        height = available_height;
        width = height * VIDEO_ASPECT;
    }
    Rect::new((image_width - width) / 2.0, top_inset + (available_height - height) / 2.0, width, height)
}

/// Origin'i SOL ALTTA olan normalize kutu (Apple Vision) → protokol kutusu (origin SOL ÜST).
pub fn top_left_from_bottom_left(bbox: Rect) -> Rect {
    Rect::new(bbox.x, 1.0 - bbox.y - bbox.height, bbox.width, bbox.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Rect, b: Rect) -> bool {
        (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9 && (a.width - b.width).abs() < 1e-9 && (a.height - b.height).abs() < 1e-9
    }

    #[test]
    fn varsayilan_pencerede_28pt_baslik_cubugu_cikarilir() {
        // Gerçek Remote Play penceresi: 960x568, altındaki 960x540 alan video.
        assert_eq!(video_rect(960.0, 568.0, 28.0), Rect::new(0.0, 28.0, 960.0, 540.0));
    }

    #[test]
    fn ultra_genis_tam_ekranda_yan_bantlar_atilir() {
        // 3440x1440 monitörde tam ekran: 2560x1440 video ortada.
        assert_eq!(video_rect(3440.0, 1440.0, 0.0), Rect::new(440.0, 0.0, 2560.0, 1440.0));
    }

    #[test]
    fn uzun_pencerede_ust_ve_alt_bantlar_atilir() {
        assert_eq!(video_rect(1600.0, 1228.0, 28.0), Rect::new(0.0, 178.0, 1600.0, 900.0));
    }

    #[test]
    fn windows_penceresinde_baslik_cubugu_cikarilir() {
        // 1280x720 video + 31 px Windows başlık çubuğu.
        assert_eq!(video_rect(1280.0, 751.0, 31.0), Rect::new(0.0, 31.0, 1280.0, 720.0));
    }

    #[test]
    fn vision_kutusu_sol_ust_orijine_cevrilir() {
        // Vision: sol alt orijin. Altta duran bir altyazı (y=0.1) üstten 0.82'de başlar.
        let flipped = top_left_from_bottom_left(Rect::new(0.2, 0.1, 0.6, 0.08));
        assert!(close(flipped, Rect::new(0.2, 0.82, 0.6, 0.08)));
    }
}
