/// Bir karenin "gerçekten görüntü var mı" kontrolü. Korumalı içerik yakalanınca
/// genelde tamamen siyah kare gelir; bunu ayırt etmek için kullanılır.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameStats {
    /// 0...255 ortalama parlaklık.
    pub mean_luma: f64,
    /// Siyah sayılmayan (luma > `DARK_THRESHOLD`) piksellerin oranı, 0...1.
    pub non_dark_fraction: f64,
}

impl FrameStats {
    pub const DARK_THRESHOLD: u8 = 16;

    /// Neredeyse tüm pikseller siyahsa kare boş/korumalı kabul edilir.
    pub fn looks_blank(&self) -> bool {
        self.non_dark_fraction < 0.01
    }

    /// `luma`: 8-bit gri tonlu pikseller.
    pub fn analyze(luma: impl IntoIterator<Item = u8>) -> FrameStats {
        let (mut sum, mut non_dark, mut count) = (0u64, 0u64, 0u64);
        for value in luma {
            sum += value as u64;
            if value > Self::DARK_THRESHOLD {
                non_dark += 1;
            }
            count += 1;
        }
        if count == 0 {
            return FrameStats { mean_luma: 0.0, non_dark_fraction: 0.0 };
        }
        FrameStats { mean_luma: sum as f64 / count as f64, non_dark_fraction: non_dark as f64 / count as f64 }
    }

    /// Satır satır RGBA kare. Hız için ~200 sütunluk bir ızgara örneklenir.
    pub fn analyze_rgba(rgba: &[u8], width: u32, height: u32) -> FrameStats {
        let (width, height) = (width as usize, height as usize);
        if rgba.len() < width * height * 4 {
            return FrameStats::analyze([]);
        }
        let step = (width / 200).max(1);
        let samples = (0..height).step_by(step).flat_map(move |y| {
            (0..width).step_by(step).map(move |x| {
                let i = (y * width + x) * 4;
                ((299 * rgba[i] as u32 + 587 * rgba[i + 1] as u32 + 114 * rgba[i + 2] as u32) / 1000) as u8
            })
        });
        FrameStats::analyze(samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tamamen_siyah_kare_bos_sayilir() {
        let stats = FrameStats::analyze(vec![0u8; 1000]);
        assert!(stats.looks_blank());
        assert_eq!(stats.mean_luma, 0.0);
    }

    #[test]
    fn siyaha_yakin_gurultu_bos_sayilir() {
        assert!(FrameStats::analyze(vec![10u8; 1000]).looks_blank());
    }

    #[test]
    fn karanlik_sahnede_bile_biraz_icerik_varsa_bos_degildir() {
        // Ekranın %5'i aydınlık (ör. gece sahnesinde bir altyazı).
        let mut luma = vec![0u8; 1000];
        luma[..50].fill(200);

        let stats = FrameStats::analyze(luma);

        assert!(!stats.looks_blank());
        assert!((stats.non_dark_fraction - 0.05).abs() < 1e-4);
        assert!((stats.mean_luma - 10.0).abs() < 1e-4);
    }

    #[test]
    fn bos_girdi_cokmez() {
        assert!(FrameStats::analyze([]).looks_blank());
    }

    #[test]
    fn rgba_beyaz_kare_bos_degildir() {
        let stats = FrameStats::analyze_rgba(&vec![255u8; 400 * 300 * 4], 400, 300);
        assert!(!stats.looks_blank());
        assert!(stats.mean_luma > 250.0);
    }

    #[test]
    fn rgba_eksik_tampon_cokmez() {
        assert!(FrameStats::analyze_rgba(&[255u8; 10], 400, 300).looks_blank());
    }
}
