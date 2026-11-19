import CoreGraphics
import Foundation
import PS5CeviriCore
// macOS 15 SDK'sında SCWindow henüz Sendable değil; Swift 6 bunu hata sayar (CI, Xcode 16).
@preconcurrency import ScreenCaptureKit

public enum ReaderError: Error, CustomStringConvertible {
    /// Akış açıldı ama henüz kare gelmedi (açılıştan hemen sonra olur, bir sonraki turda geçer).
    case noFrameYet
    case cropFailed(Rect)
    /// Önceki okuma hâlâ sürüyor (köprünün süre sınırı dolmuş ama iş bitmemiş); bu tur atlanır.
    case busy
    /// Kare alındı ama yazı tanıma başarısız oldu ya da süre sınırını aştı.
    case ocrFailed(String)

    public var description: String {
        switch self {
        case .noFrameYet: "akıştan henüz kare gelmedi"
        case .cropFailed(let rect): "Video alanı kırpılamadı: \(rect)"
        case .busy: "önceki okuma hâlâ sürüyor"
        case .ocrFailed(let message): message
        }
    }
}

/// Bir turun sonucu. `frame` değiştirilmeyen bir görüntüdür; turlar arasında paylaşılabilir.
public struct Reading: @unchecked Sendable {
    /// Video alanına göre normalize satırlar, origin sol üst.
    public let lines: [TextLine]
    /// OCR'a verilen 16:9 video alanı (akışın tamponundan bağımsız kopya).
    public let frame: CGImage
    public let fullscreen: Bool
    public let captureMilliseconds: Int64
    public let ocrMilliseconds: Int64
    /// Video alanının ekrandaki yeri (nokta, global koordinat, origin ana ekranın sol üstü).
    public let screenRect: Rect
    /// Akıştan gelen tam karenin sıra numarası; değişmiyorsa görüntü yenilenmiyor.
    public let frameSequence: UInt64
    /// Okunan karenin yaşı (sn).
    public let frameAge: Double
    /// Akışın takıldığı için kaç kez yeniden başlatıldığı.
    public let streamRestarts: UInt64
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
    /// Bir okuma sürerken ikincisi başlamaz. Köprü süre sınırı dolunca bir sonraki turu başlatır;
    /// eski sürümde iki okuma iç içe geçip ikinci bir yakalama akışı açabiliyordu.
    private var reading = false
    /// Pencere listesi sorgusu (SCShareableContent) her turda yapılmaz: ara sıra hiç dönmüyor ve
    /// pahalı. Pencere `windowRefresh` aralıkla, akış durunca ya da kare boyutu değişince yeniden aranır.
    private var window: (window: SCWindow, fullscreen: Bool, at: ContinuousClock.Instant)?
    private static let windowRefresh: Duration = .seconds(1)
    private var lastImageSize: CGSize?
    /// İlk yazı tanıma çağrısı modeli yükler (~30 sn); o tamamlanana kadar süre sınırı geniştir.
    private var recognizerWarm = false

    public init(bundleID: String = remotePlayBundleID, fps: Double = 2, titleBarOverride: Double? = nil) {
        capture = RemotePlayCapture(bundleID: bundleID)
        self.fps = fps
        self.titleBarOverride = titleBarOverride
    }

    /// Pencere yoksa `CaptureError.windowNotFound` fırlatır; çağıran bekleyip tekrar dener.
    public func read() async throws -> Reading {
        guard !reading else { throw ReaderError.busy }
        reading = true
        defer { reading = false }

        let start = clock.now
        let (window, fullscreen) = try await currentWindow()
        try await frames.ensureRunning(for: window, fps: fps)
        guard let snapshot = frames.latestFrame() else { throw ReaderError.noFrameYet }
        let image = snapshot.image
        let size = CGSize(width: image.width, height: image.height)
        if let lastImageSize, lastImageSize != size {
            // Pencere boyutu ya da ekran ölçeği değişti: pencere bilgisi bir sonraki turda tazelenir.
            self.window = nil
        }
        lastImageSize = size
        let scale = Double(image.width) / window.frame.width
        let titleBar = (titleBarOverride ?? (fullscreen ? 0 : 28)) * scale
        let video = Geometry.videoRect(imageWidth: Double(image.width), imageHeight: Double(image.height), topInset: titleBar)
        // Kopya: akışın tampon havuzu küçüktür; OCR ve son kare (dump) tamponu tutmasın.
        guard
            let cropped = image.cropping(to: CGRect(x: video.x, y: video.y, width: video.width, height: video.height)),
            let frame = copyImage(cropped)
        else {
            throw ReaderError.cropFailed(video)
        }
        let captured = clock.now
        // Vision da nadiren yanıt vermeyebilir; takılan tur bırakılır, döngü devam eder.
        let recognizer = recognizer
        let lines: [TextLine]
        do {
            lines = try await withTimeout(seconds: recognizerWarm ? 5 : 45) { try await recognizer.recognize(frame) }
        } catch {
            throw ReaderError.ocrFailed("yazı tanıma: \(error)")
        }
        recognizerWarm = true
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
            ),
            frameSequence: snapshot.sequence,
            frameAge: snapshot.age,
            streamRestarts: frames.stallRestarts
        )
    }

    public func stop() async {
        window = nil
        await frames.stop()
    }

    /// Önbellekteki pencere; süresi dolduysa ya da akış durduysa yeniden sorulur.
    private func currentWindow() async throws -> (SCWindow, Bool) {
        if let window, clock.now - window.at < Self.windowRefresh, !frames.isStopped {
            return (window.window, window.fullscreen)
        }
        do {
            let found = try await capture.findWindowWithDisplayInfo()
            window = (found.window, found.fullscreen, clock.now)
            return (found.window, found.fullscreen)
        } catch {
            window = nil
            throw error
        }
    }

    private static func milliseconds(_ duration: Duration) -> Int64 {
        duration.components.seconds * 1000 + duration.components.attoseconds / 1_000_000_000_000_000
    }
}
