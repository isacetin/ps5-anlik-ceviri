import CoreGraphics
import Foundation
import PS5CeviriCore
// macOS 15 SDK'sında SCWindow henüz Sendable değil; Swift 6 bunu hata sayar (CI, Xcode 16).
@preconcurrency import ScreenCaptureKit

public enum ReaderError: Error, CustomStringConvertible {
    /// Akış açıldı ama henüz kare gelmedi (açılıştan hemen sonra olur, bir sonraki turda geçer).
    case noFrameYet
    case cropFailed(Rect)

    public var description: String {
        switch self {
        case .noFrameYet: "akıştan henüz kare gelmedi"
        case .cropFailed(let rect): "Video alanı kırpılamadı: \(rect)"
        }
    }
}

/// Bir turun sonucu. `frame` değiştirilmeyen bir görüntüdür; turlar arasında paylaşılabilir.
public struct Reading: @unchecked Sendable {
    /// Video alanına göre normalize satırlar, origin sol üst.
    public let lines: [TextLine]
    /// OCR'a verilen 16:9 video alanı.
    public let frame: CGImage
    public let fullscreen: Bool
    public let captureMilliseconds: Int64
    public let ocrMilliseconds: Int64
    /// Video alanının ekrandaki yeri (nokta, global koordinat, origin ana ekranın sol üstü).
    public let screenRect: Rect
}

/// Bir tur ekran okuma: pencereyi bul → akışı açık tut → en son karenin 16:9 video alanını kes → OCR.
/// Swift aracı (`ps5ceviri run`) ve Rust'a açılan köprü (`CeviriBridge`) aynı mantığı kullanır.
@MainActor
public final class ScreenReader {
    public let capture: RemotePlayCapture
    public var fps: Double
    /// Başlık çubuğu yüksekliği (pt); nil ise tam ekranda 0, pencerede 28.
    public var titleBarOverride: Double?
    private let frames = FrameStream()
    private let recognizer = TextRecognizer()
    private let clock = ContinuousClock()

    public init(bundleID: String = remotePlayBundleID, fps: Double = 2, titleBarOverride: Double? = nil) {
        capture = RemotePlayCapture(bundleID: bundleID)
        self.fps = fps
        self.titleBarOverride = titleBarOverride
    }

    /// Pencere yoksa `CaptureError.windowNotFound` fırlatır; çağıran bekleyip tekrar dener.
    public func read() async throws -> Reading {
        let start = clock.now
        let (window, fullscreen) = try await capture.findWindowWithDisplayInfo()
        try await frames.ensureRunning(for: window, fps: fps)
        guard let image = frames.latestFrame() else { throw ReaderError.noFrameYet }
        let scale = Double(image.width) / window.frame.width
        let titleBar = (titleBarOverride ?? (fullscreen ? 0 : 28)) * scale
        let video = Geometry.videoRect(imageWidth: Double(image.width), imageHeight: Double(image.height), topInset: titleBar)
        guard let frame = image.cropping(to: CGRect(x: video.x, y: video.y, width: video.width, height: video.height)) else {
            throw ReaderError.cropFailed(video)
        }
        let captured = clock.now
        let lines = try await recognizer.recognize(frame)
        return Reading(
            lines: lines,
            frame: frame,
            fullscreen: fullscreen,
            captureMilliseconds: Self.milliseconds(captured - start),
            ocrMilliseconds: Self.milliseconds(clock.now - captured),
            screenRect: Rect(
                x: window.frame.minX + video.x / scale,
                y: window.frame.minY + video.y / scale,
                width: video.width / scale,
                height: video.height / scale
            )
        )
    }

    public func stop() async {
        await frames.stop()
    }

    private static func milliseconds(_ duration: Duration) -> Int64 {
        duration.components.seconds * 1000 + duration.components.attoseconds / 1_000_000_000_000_000
    }
}
