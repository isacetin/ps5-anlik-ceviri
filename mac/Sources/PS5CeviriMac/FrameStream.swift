import CoreGraphics
import CoreMedia
import Foundation
import ScreenCaptureKit
import Synchronization
import VideoToolbox

/// Remote Play penceresinden sürekli kare akışı. Her turda yeni ekran görüntüsü istemek
/// (SCScreenshotManager) bir istek takılınca sonrakileri de kilitliyordu; akış en son kareyi tutar,
/// ana döngü istediği an okur. Pencere değişirse ya da akış durursa yeniden başlatılır.
public final class FrameStream: NSObject, SCStreamOutput, SCStreamDelegate, @unchecked Sendable {
    private let queue = DispatchQueue(label: "ps5ceviri.frames")
    private let latest = Mutex<CGImage?>(nil)
    private let stopped = Mutex(true)
    // Yalnızca ana döngüden (MainActor) erişilir.
    private var stream: SCStream?
    private var windowID: CGWindowID?
    private var windowSize: CGSize = .zero

    override public init() {
        super.init()
    }

    /// Akışı verilen pencere için çalışır durumda tutar; pencere ya da boyutu değiştiyse yeniden başlatır.
    @MainActor
    public func ensureRunning(for window: SCWindow, fps: Double) async throws {
        let isRunning = stream != nil && !stopped.withLock { $0 }
        if isRunning, windowID == window.windowID, windowSize == window.frame.size { return }
        await stop()

        let filter = SCContentFilter(desktopIndependentWindow: window)
        let scale = CGFloat(filter.pointPixelScale)
        let config = SCStreamConfiguration()
        config.width = Int(filter.contentRect.width * scale)
        config.height = Int(filter.contentRect.height * scale)
        config.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(max(fps * 2, 2)))
        config.pixelFormat = kCVPixelFormatType_32BGRA
        config.queueDepth = 3
        config.showsCursor = false
        config.ignoreShadowsSingleWindow = true

        let newStream = SCStream(filter: filter, configuration: config, delegate: self)
        try newStream.addStreamOutput(self, type: .screen, sampleHandlerQueue: queue)
        latest.withLock { $0 = nil }
        nonisolated(unsafe) let starting = newStream
        try await withTimeout(seconds: 5) { try await starting.startCapture() }
        stopped.withLock { $0 = false }
        stream = newStream
        windowID = window.windowID
        windowSize = window.frame.size
    }

    /// En son tam kare; akış henüz kare üretmediyse nil.
    public func latestFrame() -> CGImage? {
        latest.withLock { $0 }
    }

    @MainActor
    public func stop() async {
        guard let current = stream else { return }
        stream = nil
        stopped.withLock { $0 = true }
        nonisolated(unsafe) let stopping = current
        try? await withTimeout(seconds: 2) { try await stopping.stopCapture() }
    }

    public func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, isComplete(sampleBuffer), let pixelBuffer = sampleBuffer.imageBuffer else { return }
        var image: CGImage?
        VTCreateCGImageFromCVPixelBuffer(pixelBuffer, options: nil, imageOut: &image)
        if let image { latest.withLock { $0 = image } }
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
}
