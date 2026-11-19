import CoreGraphics
import CoreMedia
import Foundation
import ScreenCaptureKit
import Synchronization
import VideoToolbox

/// Akıştan alınan son kare ve tazeliği.
public struct FrameSnapshot: @unchecked Sendable {
    public let image: CGImage
    /// Akıştan gelen tam karelerin sıra numarası (yeniden başlatmalarda sıfırlanmaz). Değişmiyorsa
    /// görüntü yenilenmiyor demektir.
    public let sequence: UInt64
    /// Karenin akıştan geldiği andan bu yana geçen süre (sn).
    public let age: Double
}

/// Remote Play penceresinden sürekli kare akışı. Her turda yeni ekran görüntüsü istemek
/// (SCScreenshotManager) bir istek takılınca sonrakileri de kilitliyordu; akış en son kareyi tutar,
/// ana döngü istediği an okur. Pencere değişirse, akış durursa ya da **takılırsa** (hiç geri çağrı
/// gelmiyorsa) yeniden başlatılır.
public final class FrameStream: NSObject, SCStreamOutput, SCStreamDelegate, @unchecked Sendable {
    /// Geri çağrı zamanları ve kare sayacı (yalnızca sayılar; kilit kısa tutulur).
    private struct Timing: Sendable {
        var sequence: UInt64 = 0
        /// Son tam karenin ve son geri çağrının (boş/"idle" kareler dahil) anı, ns (uptime).
        var latestAt: UInt64 = 0
        var lastCallbackAt: UInt64 = 0
    }

    /// Akış bu kadar saniye hiç geri çağrı göndermezse takılmış sayılır. Art arda takılmalarda süre
    /// ikiye katlanır (en çok `maxStallWait`); akış kare göndermeye başlayınca sıfırlanır.
    public static let stallAfter: Double = 4
    public static let maxStallWait: Double = 60

    private let queue = DispatchQueue(label: "ps5ceviri.frames")
    private let latest = Mutex<CGImage?>(nil)
    private let timing = Mutex(Timing())
    private let stopped = Mutex(true)
    // Yalnızca ana döngüden (MainActor) erişilir.
    private var stream: SCStream?
    private var windowID: CGWindowID?
    private var windowSize: CGSize = .zero
    private var startedAt: UInt64 = 0
    private var sequenceAtStart: UInt64 = 0
    private var consecutiveStalls = 0
    /// Takıldığı için kaç kez yeniden başlatıldı (teşhis).
    public private(set) var stallRestarts: UInt64 = 0

    override public init() {
        super.init()
    }

    /// Akış durmuş ya da hiç başlamamış mı (pencere yeniden aranmalı).
    public var isStopped: Bool {
        stream == nil || stopped.withLock { $0 }
    }

    /// Akışı verilen pencere için çalışır durumda tutar; pencere ya da boyutu değiştiyse ya da akış
    /// takıldıysa yeniden başlatır.
    @MainActor
    public func ensureRunning(for window: SCWindow, fps: Double) async throws {
        let isRunning = stream != nil && !stopped.withLock { $0 }
        let sameWindow = windowID == window.windowID && windowSize == window.frame.size
        if isRunning, sameWindow {
            let now = Self.uptime()
            let current = timing.withLock { $0 }
            if current.sequence &- sequenceAtStart >= 2 {
                // Akış kare gönderiyor: önceki takılmalar unutulur.
                consecutiveStalls = 0
            }
            let lastSign = max(current.lastCallbackAt, startedAt)
            let silent = Double(now &- lastSign) / 1_000_000_000
            let limit = min(Self.stallAfter * pow(2, Double(consecutiveStalls)), Self.maxStallWait)
            guard silent > limit else { return }
            consecutiveStalls += 1
            stallRestarts += 1
            FileHandle.standardError.write(Data("Kare akışı \(Int(silent)) sn'dir kare göndermiyor; yeniden başlatılıyor\n".utf8))
        }
        await stop()

        let filter = SCContentFilter(desktopIndependentWindow: window)
        let scale = CGFloat(filter.pointPixelScale)
        let config = SCStreamConfiguration()
        config.width = Int(filter.contentRect.width * scale)
        config.height = Int(filter.contentRect.height * scale)
        config.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(max(fps * 2, 2)))
        config.pixelFormat = kCVPixelFormatType_32BGRA
        // Okuma, karenin kopyasıyla çalışır; akış yalnızca en son kareyi tutar. 4 tampon, akışın
        // her zaman yeni kare yazacak boş tamponu olmasına yeter.
        config.queueDepth = 4
        config.showsCursor = false
        config.ignoreShadowsSingleWindow = true

        let newStream = SCStream(filter: filter, configuration: config, delegate: self)
        try newStream.addStreamOutput(self, type: .screen, sampleHandlerQueue: queue)
        latest.withLock { $0 = nil }
        nonisolated(unsafe) let starting = newStream
        do {
            try await withTimeout(seconds: 5) { try await starting.startCapture() }
        } catch {
            // Takılan başlatma sonradan tamamlanırsa akış sahipsiz çalışmasın (eski sürümde bu
            // durumda ikinci bir akış arka planda açık kalabiliyordu).
            try? newStream.removeStreamOutput(self, type: .screen)
            Task { try? await starting.stopCapture() }
            throw error
        }
        stopped.withLock { $0 = false }
        stream = newStream
        windowID = window.windowID
        windowSize = window.frame.size
        startedAt = Self.uptime()
        sequenceAtStart = timing.withLock { $0.sequence }
    }

    /// En son tam kare; akış henüz kare üretmediyse nil.
    public func latestFrame() -> FrameSnapshot? {
        guard let image = latest.withLock({ $0 }) else { return nil }
        let current = timing.withLock { $0 }
        let age = Double(Self.uptime() &- current.latestAt) / 1_000_000_000
        return FrameSnapshot(image: image, sequence: current.sequence, age: age)
    }

    @MainActor
    public func stop() async {
        guard let current = stream else { return }
        stream = nil
        stopped.withLock { $0 = true }
        // Eski akışın tamponu tutulmasın; yeni akışın ilk karesi beklenir.
        latest.withLock { $0 = nil }
        try? current.removeStreamOutput(self, type: .screen)
        nonisolated(unsafe) let stopping = current
        try? await withTimeout(seconds: 2) { try await stopping.stopCapture() }
    }

    public func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen else { return }
        let now = Self.uptime()
        guard isComplete(sampleBuffer), let pixelBuffer = sampleBuffer.imageBuffer else {
            // Ekran değişmediğinde gelen boş kare: akış canlı, görüntü aynı.
            timing.withLock { $0.lastCallbackAt = now }
            return
        }
        var image: CGImage?
        VTCreateCGImageFromCVPixelBuffer(pixelBuffer, options: nil, imageOut: &image)
        guard let image else {
            timing.withLock { $0.lastCallbackAt = now }
            return
        }
        latest.withLock { $0 = image }
        timing.withLock {
            $0.sequence += 1
            $0.latestAt = now
            $0.lastCallbackAt = now
        }
    }

    public func stream(_ stream: SCStream, didStopWithError error: Error) {
        stopped.withLock { $0 = true }
        FileHandle.standardError.write(Data("Kare akışı durdu: \(error.localizedDescription)\n".utf8))
    }

    /// Ekran değişmediğinde akış görüntüsüz "idle" kareler gönderir; yalnızca tam kareler alınır.
    private func isComplete(_ sampleBuffer: CMSampleBuffer) -> Bool {
        guard
            let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
            let rawStatus = attachments.first?[.status] as? Int,
            let status = SCFrameStatus(rawValue: rawStatus)
        else { return false }
        return status == .complete
    }

    private static func uptime() -> UInt64 {
        DispatchTime.now().uptimeNanoseconds
    }
}
