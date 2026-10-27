import AppKit
import Foundation
import PS5CeviriCore
import PS5CeviriMac
// macOS 15 SDK'sında SCWindow henüz Sendable değil; Swift 6 bunu hata sayar (CI, Xcode 16).
@preconcurrency import ScreenCaptureKit

// Rust'a açılan C ABI (bkz. desktop/mac/src/lib.rs). Kurallar:
// - `ceviri_prepare` ve `ceviri_run_main_loop` ANA thread'den çağrılır; ana thread sonra olay
//   döngüsünü çalıştırır (ScreenReader @MainActor'dadır, ana kuyruk dönmezse her şey bekler).
// - Diğer tüm çağrılar ana thread DIŞINDAN yapılır ve sonuç gelene kadar bloklar.
// - Dönen `char *` JSON'dur ve `ceviri_string_free` ile bırakılır.

/// Komut satırı süreçlerinde pencere sunucusu bağlantısını açar; ScreenCaptureKit onsuz
/// CGS_REQUIRE_INIT assert'üyle çöker. GUI uygulamasında zaten açıktır, zararı yok.
@_cdecl("ceviri_prepare")
public func ceviriPrepare() {
    MainActor.assumeIsolated { _ = NSApplication.shared }
}

/// Ana thread'in olay döngüsü; dönmez.
@_cdecl("ceviri_run_main_loop")
public func ceviriRunMainLoop() {
    while true { CFRunLoopRun() }
}

@_cdecl("ceviri_string_free")
public func ceviriStringFree(_ pointer: UnsafeMutablePointer<CChar>?) {
    free(pointer)
}

@_cdecl("ceviri_list_windows")
public func ceviriListWindows() -> UnsafeMutablePointer<CChar>? {
    respond {
        let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
        return content.windows.filter { $0.frame.width >= 100 }.map { window in
            WindowJSON(
                bundleID: window.owningApplication?.bundleIdentifier ?? "?",
                title: window.title ?? "",
                width: Int(window.frame.width),
                height: Int(window.frame.height)
            )
        }
    }
}

/// `titleBar` < 0 ise otomatik (tam ekranda 0, pencerede 28 pt). Ana thread'den çağrılırsa NULL döner.
@_cdecl("ceviri_reader_new")
public func ceviriReaderNew(_ bundleID: UnsafePointer<CChar>?, _ fps: Double, _ titleBar: Double) -> UnsafeMutableRawPointer? {
    let bundle = bundleID.map { String(cString: $0) } ?? remotePlayBundleID
    let override = titleBar < 0 ? nil : titleBar
    guard case .success(let box) = blocking({ await MainActor.run { ReaderBox(ScreenReader(bundleID: bundle, fps: fps, titleBarOverride: override)) } }) else {
        return nil
    }
    return Unmanaged.passRetained(box).toOpaque()
}

@_cdecl("ceviri_reader_free")
public func ceviriReaderFree(_ handle: UnsafeMutableRawPointer?) {
    guard let handle else { return }
    let box = Unmanaged<ReaderBox>.fromOpaque(handle).takeRetainedValue()
    _ = blocking { await box.reader.stop() }
}

/// Bir tur: {"reading": {...}} ya da {"error": {"kind": "windowNotFound" | "noFrameYet" | "other", ...}}.
@_cdecl("ceviri_reader_read")
public func ceviriReaderRead(_ handle: UnsafeMutableRawPointer?) -> UnsafeMutablePointer<CChar>? {
    guard let handle else { return encode(ErrorJSON(kind: "other", message: "geçersiz okuyucu")) }
    let box = Unmanaged<ReaderBox>.fromOpaque(handle).takeUnretainedValue()
    let result = blocking { () async throws -> ReadingJSON in
        let reading = try await box.reader.read()
        box.last.withLock { $0 = reading.frame }
        return ReadingJSON(reading)
    }
    switch result {
    case .success(let reading): return encode(["reading": reading])
    case .failure(let error): return encode(["error": ErrorJSON(error)])
    }
}

/// Son okunan video karesini verilen kutularla (JSON: [{"x","y","w","h"}], normalize) PNG'ye yazar.
/// Başarıda NULL, hatada mesaj döner.
@_cdecl("ceviri_reader_dump")
public func ceviriReaderDump(_ handle: UnsafeMutableRawPointer?, _ path: UnsafePointer<CChar>?, _ boxesJSON: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    guard let handle, let path else { return strdup("geçersiz argüman") }
    let box = Unmanaged<ReaderBox>.fromOpaque(handle).takeUnretainedValue()
    guard let frame = box.last.withLock({ $0 }) else { return strdup("henüz okunmuş kare yok") }
    let boxes = boxesJSON
        .flatMap { try? JSONDecoder().decode([BoxJSON].self, from: Data(String(cString: $0).utf8)) }?
        .map { Rect(x: $0.x, y: $0.y, width: $0.w, height: $0.h) } ?? []
    guard let image = drawBoxes(on: frame, boxes: boxes) else { return strdup("kutular çizilemedi") }
    do {
        try writePNG(image, to: URL(fileURLWithPath: String(cString: path)))
        return nil
    } catch {
        return strdup("\(error)")
    }
}

// MARK: - Yardımcılar

private final class ReaderBox: @unchecked Sendable {
    let reader: ScreenReader
    /// Dump için son video karesi; CGImage değişmez, kilit sadece referans için.
    let last = NSLock.Protected<CGImage?>(nil)

    init(_ reader: ScreenReader) {
        self.reader = reader
    }
}

private struct WindowJSON: Encodable {
    let bundleID: String
    let title: String
    let width: Int
    let height: Int
}

private struct LineJSON: Encodable {
    let text: String
    let x, y, w, h: Double
    let confidence: Double
}

private struct ReadingJSON: Encodable {
    let lines: [LineJSON]
    let videoWidth: Int
    let videoHeight: Int
    let fullscreen: Bool
    let captureMs: Int64
    let ocrMs: Int64

    init(_ reading: Reading) {
        lines = reading.lines.map {
            LineJSON(text: $0.text, x: $0.box.x, y: $0.box.y, w: $0.box.width, h: $0.box.height, confidence: $0.confidence)
        }
        videoWidth = reading.frame.width
        videoHeight = reading.frame.height
        fullscreen = reading.fullscreen
        captureMs = reading.captureMilliseconds
        ocrMs = reading.ocrMilliseconds
    }
}

private struct ErrorJSON: Encodable {
    let kind: String
    let message: String

    init(kind: String, message: String) {
        self.kind = kind
        self.message = message
    }

    init(_ error: Error) {
        switch error {
        case CaptureError.windowNotFound: kind = "windowNotFound"
        case ReaderError.noFrameYet: kind = "noFrameYet"
        default: kind = "other"
        }
        message = "\(error)"
    }
}

private struct BoxJSON: Decodable {
    let x, y, w, h: Double
}

private struct MainThreadError: Error, CustomStringConvertible {
    var description: String { "köprü ana thread'den çağrıldı; ana thread olay döngüsünü çalıştırmalı" }
}

/// Async işi bitene kadar çağıran thread'i bloklar. Ana thread'de çağrılırsa @MainActor işler
/// hiç çalışamayacağı için kilitlenmek yerine hata döner.
private func blocking<T: Sendable>(_ operation: @escaping @Sendable () async throws -> T) -> Result<T, Error> {
    guard !Thread.isMainThread else { return .failure(MainThreadError()) }
    let done = DispatchSemaphore(value: 0)
    let result = NSLock.Protected<Result<T, Error>?>(nil)
    Task {
        let value: Result<T, Error>
        do { value = .success(try await operation()) } catch { value = .failure(error) }
        result.withLock { $0 = value }
        done.signal()
    }
    done.wait()
    return result.withLock { $0! }
}

private func respond<T: Encodable & Sendable>(_ operation: @escaping @Sendable () async throws -> T) -> UnsafeMutablePointer<CChar>? {
    switch blocking(operation) {
    case .success(let value): encode(["ok": value])
    case .failure(let error): encode(["error": ErrorJSON(error)])
    }
}

private func encode<T: Encodable>(_ value: T) -> UnsafeMutablePointer<CChar>? {
    guard let data = try? JSONEncoder().encode(value) else { return strdup(#"{"error":{"kind":"other","message":"JSON üretilemedi"}}"#) }
    return strdup(String(decoding: data, as: UTF8.self))
}

private extension NSLock {
    /// Basit kilitli kutu (Synchronization.Mutex ~Copyable olduğu için sınıf içinde saklamak zahmetli).
    final class Protected<Value>: @unchecked Sendable {
        private let lock = NSLock()
        private var value: Value

        init(_ value: Value) {
            self.value = value
        }

        func withLock<R>(_ body: (inout Value) -> R) -> R {
            lock.lock()
            defer { lock.unlock() }
            return body(&value)
        }
    }
}
