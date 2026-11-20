import PS5CeviriCore

/// Kare değişmediyse yazı tanımayı yeniden çalıştırmaz. ScreenCaptureKit, ekran değişmediğinde yeni
/// tam kare göndermez (sıra numarası artmaz); menüde, duraklatılmış oyunda ya da durağan bir
/// sahnede her tur aynı kareyi Vision'a vermek boşa işlemci harcar. Sıra numarası ve kırpılan alan
/// aynıysa önceki satırlar döner. Akış yeniden başlasa da sıra numarası sıfırlanmaz; yeni kare her
/// zaman yeni numara alır, eski sonuç yeni kareye uygulanmaz.
struct OCRReuse<Frame> {
    private var last: (sequence: UInt64, crop: Rect, lines: [TextLine], frame: Frame)?
    /// Atlanan OCR sayısı (teşhis).
    private(set) var reused: UInt64 = 0

    /// Aynı karenin aynı alanı daha önce okunduysa satırları ve kopyalanmış kare.
    mutating func lookup(sequence: UInt64, crop: Rect) -> (lines: [TextLine], frame: Frame)? {
        guard let last, last.sequence == sequence, last.crop == crop else { return nil }
        reused += 1
        return (last.lines, last.frame)
    }

    mutating func store(sequence: UInt64, crop: Rect, lines: [TextLine], frame: Frame) {
        last = (sequence, crop, lines, frame)
    }

    /// Akış/pencere değişti: eski sonuç kullanılmaz.
    mutating func clear() {
        last = nil
    }
}
