/// Bir karenin "gerçekten görüntü var mı" kontrolü. Korumalı içerik yakalanınca
/// ScreenCaptureKit genelde tamamen siyah kare döner; bunu ayırt etmek için kullanılır.
public struct FrameStats: Equatable, Sendable {
    /// 0...255 ortalama parlaklık.
    public let meanLuma: Double
    /// Siyah sayılmayan (luma > `darkThreshold`) piksellerin oranı, 0...1.
    public let nonDarkFraction: Double

    public static let darkThreshold: UInt8 = 16

    /// Neredeyse tüm pikseller siyahsa kare boş/korumalı kabul edilir.
    public var looksBlank: Bool { nonDarkFraction < 0.01 }

    /// `luma`: satır satır 8-bit gri tonlu pikseller.
    public static func analyze(luma: [UInt8]) -> FrameStats {
        guard !luma.isEmpty else { return FrameStats(meanLuma: 0, nonDarkFraction: 0) }
        var sum = 0
        var nonDark = 0
        for value in luma {
            sum += Int(value)
            if value > darkThreshold { nonDark += 1 }
        }
        let count = Double(luma.count)
        return FrameStats(meanLuma: Double(sum) / count, nonDarkFraction: Double(nonDark) / count)
    }
}
