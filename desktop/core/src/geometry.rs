use serde::{Deserialize, Serialize};

/// Basit dikdörtgen. Bağlama göre piksel ya da 0...1 normalize değer tutar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
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

/// Pencerenin yakalanan karesinde istemci alanının (oyunun çizdiği yer: başlık çubuğu ve
/// kenarlar hariç) piksel karşılığı. `frame` ve `client` ekran pikseli; kare `frame` boyutundadır
/// ama ölçeklenmiş olabilir. Sonuç karenin içine kırpılır.
pub fn client_rect(frame: Rect, client: Rect, image_width: f64, image_height: f64) -> Rect {
    let scale_x = image_width / frame.width.max(1.0);
    let scale_y = image_height / frame.height.max(1.0);
    let left = ((client.x - frame.x) * scale_x).clamp(0.0, image_width);
    let top = ((client.y - frame.y) * scale_y).clamp(0.0, image_height);
    let right = ((client.right() - frame.x) * scale_x).clamp(left, image_width);
    let bottom = ((client.bottom() - frame.y) * scale_y).clamp(top, image_height);
    Rect::new(left, top, right - left, bottom - top)
}

/// Seçilen bölgenin en küçük boyutu (alanın oranı); daha küçüğü yanlışlıkla tıklamadır.
pub const MIN_REGION: f64 = 0.02;

/// Kullanıcının çizdiği bölgeyi 0...1 aralığına kırpar; çok küçükse `None` (tüm alan).
pub fn clamp_region(region: Rect) -> Option<Rect> {
    let left = region.x.min(region.right()).clamp(0.0, 1.0);
    let top = region.y.min(region.bottom()).clamp(0.0, 1.0);
    let right = region.x.max(region.right()).clamp(0.0, 1.0);
    let bottom = region.y.max(region.bottom()).clamp(0.0, 1.0);
    (right - left >= MIN_REGION && bottom - top >= MIN_REGION).then(|| Rect::new(left, top, right - left, bottom - top))
}

/// `area` içinde normalize `region`'ın kapladığı dikdörtgen (`area` ile aynı birimde).
pub fn sub_rect(area: Rect, region: Rect) -> Rect {
    Rect::new(area.x + region.x * area.width, area.y + region.y * area.height, region.width * area.width, region.height * area.height)
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
    fn istemci_alani_baslik_cubugu_ve_kenarlar_haric() {
        // 1296x759 pencere (8 px kenar, 31 px başlık), içinde 1280x720 istemci alanı.
        let frame = Rect::new(100.0, 50.0, 1296.0, 759.0);
        let client = Rect::new(108.0, 81.0, 1280.0, 720.0);
        assert_eq!(client_rect(frame, client, 1296.0, 759.0), Rect::new(8.0, 31.0, 1280.0, 720.0));
    }

    #[test]
    fn kenarliksiz_tam_ekranda_istemci_alani_tum_kare() {
        let screen = Rect::new(0.0, 0.0, 2560.0, 1080.0);
        assert_eq!(client_rect(screen, screen, 2560.0, 1080.0), Rect::new(0.0, 0.0, 2560.0, 1080.0));
    }

    #[test]
    fn istemci_alani_olceklenmis_kareye_uyar_ve_disari_tasmaz() {
        // Kare pencerenin yarı boyutunda; istemci alanı pencereden taşıyor (eksi ofset).
        let frame = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let client = Rect::new(-10.0, 100.0, 1100.0, 700.0);
        assert_eq!(client_rect(frame, client, 500.0, 400.0), Rect::new(0.0, 50.0, 500.0, 350.0));
    }

    #[test]
    fn bolge_kirpilir_ters_cizim_duzelir_kucuk_bolge_atilir() {
        assert!(close(clamp_region(Rect::new(0.9, 0.95, -0.8, -0.3)).unwrap(), Rect::new(0.1, 0.65, 0.8, 0.3)));
        assert!(close(clamp_region(Rect::new(-0.2, 0.7, 1.5, 0.5)).unwrap(), Rect::new(0.0, 0.7, 1.0, 0.3)));
        assert_eq!(clamp_region(Rect::new(0.5, 0.5, 0.01, 0.3)), None);
    }

    #[test]
    fn bolge_alanin_icinde_konumlanir() {
        // 1920x1080 istemci alanı (8,31'de), alt %25'lik altyazı bandı.
        let area = Rect::new(8.0, 31.0, 1920.0, 1080.0);
        assert_eq!(sub_rect(area, Rect::new(0.1, 0.75, 0.8, 0.25)), Rect::new(200.0, 841.0, 1536.0, 270.0));
    }

    #[test]
    fn vision_kutusu_sol_ust_orijine_cevrilir() {
        // Vision: sol alt orijin. Altta duran bir altyazı (y=0.1) üstten 0.82'de başlar.
        let flipped = top_left_from_bottom_left(Rect::new(0.2, 0.1, 0.6, 0.08));
        assert!(close(flipped, Rect::new(0.2, 0.82, 0.6, 0.08)));
    }
}
