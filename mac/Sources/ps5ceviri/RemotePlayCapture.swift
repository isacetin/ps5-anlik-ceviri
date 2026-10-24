import CoreGraphics
import Foundation
import ImageIO
import PS5CeviriCore
import ScreenCaptureKit
import UniformTypeIdentifiers

let remotePlayBundleID = "com.playstation.RemotePlay"

enum CaptureError: Error, CustomStringConvertible {
    case windowNotFound(String)
    case pngWriteFailed(URL)

    var description: String {
        switch self {
        case .windowNotFound(let bundleID):
            "Ekranda \(bundleID) penceresi bulunamadı. PS Remote Play açık ve PS5'e bağlı mı?"
        case .pngWriteFailed(let url):
            "PNG yazılamadı: \(url.path)"
        }
    }
}

/// PS Remote Play penceresini ScreenCaptureKit ile yakalar.
struct RemotePlayCapture {
    let bundleID: String

    /// Uygulamanın ekrandaki en büyük penceresi (küçük yardımcı pencereler elenir).
    func findWindow() async throws -> SCWindow {
        try await findWindowWithDisplayInfo().window
    }

    /// Pencere ve tam ekranda olup olmadığı (tam ekranda başlık çubuğu yoktur).
    func findWindowWithDisplayInfo() async throws -> (window: SCWindow, fullscreen: Bool) {
        let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
        let candidates = content.windows.filter {
            $0.owningApplication?.bundleIdentifier == bundleID && $0.frame.width >= 320 && $0.frame.height >= 180
        }
        guard let window = candidates.max(by: { $0.frame.width * $0.frame.height < $1.frame.width * $1.frame.height }) else {
            throw CaptureError.windowNotFound(bundleID)
        }
        let fullscreen = content.displays.contains { $0.frame == window.frame }
        return (window, fullscreen)
    }

    /// Pencerenin tek karesi, ekranın gerçek piksel çözünürlüğünde.
    func captureFrame(of window: SCWindow) async throws -> CGImage {
        let filter = SCContentFilter(desktopIndependentWindow: window)
        let scale = CGFloat(filter.pointPixelScale)
        let config = SCStreamConfiguration()
        config.width = Int(filter.contentRect.width * scale)
        config.height = Int(filter.contentRect.height * scale)
        config.showsCursor = false
        config.ignoreShadowsSingleWindow = true
        // Remote Play bağlantısı koparken bu çağrı hiç dönmeyebiliyor; araç takılmasın.
        // Filtre ve ayar yalnızca bu çağrıda okunur, başka yerde değiştirilmez.
        nonisolated(unsafe) let captureFilter = filter
        nonisolated(unsafe) let captureConfig = config
        return try await withTimeout(seconds: 3) {
            try await SCScreenshotManager.captureImage(contentFilter: captureFilter, configuration: captureConfig)
        }
    }
}

/// Görüntüyü küçültülmüş gri tonlamaya çevirip parlaklık istatistiği çıkarır.
func frameStats(of image: CGImage) -> FrameStats {
    let width = 192
    let height = max(1, Int(Double(width) * Double(image.height) / Double(max(image.width, 1))))
    var luma = [UInt8](repeating: 0, count: width * height)
    luma.withUnsafeMutableBytes { buffer in
        guard let context = CGContext(
            data: buffer.baseAddress,
            width: width,
            height: height,
            bitsPerComponent: 8,
            bytesPerRow: width,
            space: CGColorSpaceCreateDeviceGray(),
            bitmapInfo: CGImageAlphaInfo.none.rawValue
        ) else { return }
        context.interpolationQuality = .low
        context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    }
    return FrameStats.analyze(luma: luma)
}

func writePNG(_ image: CGImage, to url: URL) throws {
    guard let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil) else {
        throw CaptureError.pngWriteFailed(url)
    }
    CGImageDestinationAddImage(destination, image, nil)
    guard CGImageDestinationFinalize(destination) else { throw CaptureError.pngWriteFailed(url) }
}

/// Debug için: OCR kutularını görüntünün üzerine kırmızı çerçeve olarak çizer.
func drawBoxes(on image: CGImage, boxes: [Rect]) -> CGImage? {
    let width = image.width
    let height = image.height
    guard let context = CGContext(
        data: nil,
        width: width,
        height: height,
        bitsPerComponent: 8,
        bytesPerRow: 0,
        space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else { return nil }
    context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    context.setStrokeColor(red: 1, green: 0, blue: 0, alpha: 1)
    context.setLineWidth(2)
    for box in boxes {
        // Kutular sol üst orijinli; CGContext sol alt orijinli.
        context.stroke(CGRect(
            x: box.x * Double(width),
            y: (1 - box.y - box.height) * Double(height),
            width: box.width * Double(width),
            height: box.height * Double(height)
        ))
    }
    return context.makeImage()
}
