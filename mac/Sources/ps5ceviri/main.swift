import AppKit
import Foundation
import PS5CeviriCore
import ScreenCaptureKit

// Komut satırı aracında pencere sunucusu bağlantısı kendiliğinden açılmaz; ScreenCaptureKit
// onsuz CGS_REQUIRE_INIT assert'üyle çöker.
_ = NSApplication.shared
// Çıktı pipe'a veya dosyaya yönlendirildiğinde de satır satır görünsün.
setvbuf(stdout, nil, _IOLBF, 0)

let usage = """
Kullanım:
  ps5ceviri windows                         Ekrandaki pencereleri ve bundle ID'lerini listeler
  ps5ceviri capture [seçenekler]            Remote Play penceresinden kare kaydeder
    --count N        Kare sayısı (varsayılan 5)
    --interval SN    Kareler arası saniye (varsayılan 1)
    --out KLASÖR     Çıktı klasörü (varsayılan ./frames)
    --bundle ID      Yakalanacak uygulama (varsayılan \(remotePlayBundleID))
  ps5ceviri translate "METİN"               DeepL anahtarını tek bir metinle dener (önbelleği kullanmaz)
  ps5ceviri ocr DOSYA.png...                Görüntü dosyalarında Vision OCR çalıştırır (winprobe ile karşılaştırma için)
  ps5ceviri run [seçenekler]                Remote Play'i okuyup Türkçe çeviriyi TV'ye gönderir
    --tv HOST[:PORT] TV adresi (verilmezse yerel ağda NSD ile aranır; port varsayılan 8765)
    --dry-run        DeepL'e gitmeden İngilizce metni gönderir (hizalama testi, kota harcamaz)
    --fps N          Saniyedeki OCR sayısı (varsayılan 2; en az 1, TV 3 sn mesajsız kalırsa temizler)
    --titlebar PT    Başlık çubuğu yüksekliği (varsayılan: tam ekranda 0, pencerede 28)
    --dump KLASÖR    Her turda OCR bloklarını çizilmiş kareyi kaydeder
    --verbose        Her turun süresini yazar
    --once           Tek tur çalışıp çıkar
"""

func option(_ name: String, in args: [String]) -> String? {
    guard let index = args.firstIndex(of: name), index + 1 < args.count else { return nil }
    return args[index + 1]
}

func listWindows() async throws {
    let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
    for window in content.windows where window.frame.width >= 100 {
        let bundle = window.owningApplication?.bundleIdentifier ?? "?"
        let size = "\(Int(window.frame.width))x\(Int(window.frame.height))"
        print("\(bundle)  \(size)  \"\(window.title ?? "")\"")
    }
}

func capture(args: [String]) async throws {
    let count = option("--count", in: args).flatMap(Int.init) ?? 5
    let interval = option("--interval", in: args).flatMap(Double.init) ?? 1
    let outDir = URL(fileURLWithPath: option("--out", in: args) ?? "frames")
    let capture = RemotePlayCapture(bundleID: option("--bundle", in: args) ?? remotePlayBundleID)

    try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)
    let window = try await capture.findWindow()
    print("Pencere: \"\(window.title ?? "")\" \(Int(window.frame.width))x\(Int(window.frame.height)) pt")

    var blankFrames = 0
    for index in 1...count {
        let image = try await capture.captureFrame(of: window)
        let stats = frameStats(of: image)
        if stats.looksBlank { blankFrames += 1 }
        let url = outDir.appendingPathComponent(String(format: "frame_%02d.png", index))
        try writePNG(image, to: url)
        print(String(
            format: "%@  %dx%d  ort. parlaklık %.0f  siyah olmayan %%%.1f%@",
            url.lastPathComponent, image.width, image.height, stats.meanLuma, stats.nonDarkFraction * 100,
            stats.looksBlank ? "  ⚠️ SİYAH" : ""
        ))
        if index < count { try await Task.sleep(for: .seconds(interval)) }
    }
    print(blankFrames == count
        ? "SONUÇ: Tüm kareler siyah — pencere korumalı içerik olarak yakalanıyor olabilir."
        : "SONUÇ: \(count - blankFrames)/\(count) karede görüntü var.")
}

@MainActor
func run(args: [String]) async throws {
    let dryRun = args.contains("--dry-run")
    let verbose = args.contains("--verbose")
    let once = args.contains("--once")
    let fps = max(option("--fps", in: args).flatMap(Double.init) ?? 2, 1)
    let titleBarOverride = option("--titlebar", in: args).flatMap(Double.init)
    let dumpDir = option("--dump", in: args).map { URL(fileURLWithPath: $0) }
    if let dumpDir { try FileManager.default.createDirectory(at: dumpDir, withIntermediateDirectories: true) }

    var translator: Translator?
    if !dryRun {
        guard let key = DeepLKey.load() else {
            throw CommandError("DeepL anahtarı bulunamadı. DEEPL_AUTH_KEY ortam değişkenini ayarla ya da anahtarı \(DeepLKey.file.path) dosyasına koy. Çevirisiz denemek için --dry-run.")
        }
        let deepL = DeepLClient(key: key)
        let cache = TranslationCacheFile.load()
        translator = Translator(cache: cache) { try await deepL.translate($0) }
        print("DeepL: \(DeepL.endpoint(forKey: key).host ?? "")  önbellekte \(cache.count) çeviri")
    }

    let url: URL
    if let tv = option("--tv", in: args) {
        guard let parsed = URL(string: "ws://\(tv.contains(":") ? tv : tv + ":8765")") else { throw CommandError("Geçersiz --tv: \(tv)") }
        url = parsed
    } else {
        print("TV yerel ağda aranıyor (_ps5ceviri._tcp)...")
        guard let found = await Discovery.findTV() else {
            throw CommandError("TV bulunamadı. TV'de overlay servisi açık mı? Adresi elle ver: --tv 192.168.1.5")
        }
        url = found
    }

    let capture = RemotePlayCapture(bundleID: option("--bundle", in: args) ?? remotePlayBundleID)
    let recognizer = TextRecognizer()
    let frames = FrameStream()
    let client = OverlayClient(url: url)
    let clock = ContinuousClock()
    let startedAt = clock.now
    let elapsed: @Sendable () -> Double = {
        let duration = clock.now - startedAt
        return Double(duration.components.seconds) + Double(duration.components.attoseconds) / 1e18
    }
    var stabilizer = Stabilizer()
    var lastShown: [String] = []
    var waitingForWindow = false

    print("TV: \(url.absoluteString)  fps: \(fps)  mod: \(dryRun ? "çevirisiz (dry-run)" : "DeepL EN→TR")  (Ctrl+C ile çık)")
    print("Vision modeli yükleniyor; ilk OCR ~30 sn sürebilir...")

    var round = 0
    while true {
        round += 1
        let roundStart = clock.now
        var blocks: [OverlayBlock] = []
        do {
            let (window, fullscreen) = try await capture.findWindowWithDisplayInfo()
            if waitingForWindow { print("Remote Play penceresi bulundu.") }
            waitingForWindow = false

            try await frames.ensureRunning(for: window, fps: fps)
            guard let image = frames.latestFrame() else { throw CommandError("akıştan henüz kare gelmedi") }
            let scale = Double(image.width) / window.frame.width
            let titleBar = (titleBarOverride ?? (fullscreen ? 0 : 28)) * scale
            let video = Geometry.videoRect(imageWidth: Double(image.width), imageHeight: Double(image.height), topInset: titleBar)
            guard let frame = image.cropping(to: CGRect(x: video.x, y: video.y, width: video.width, height: video.height)) else {
                throw CommandError("Video alanı kırpılamadı: \(video)")
            }
            let captured = clock.now

            let merged = BlockMerger.merge(try await recognizer.recognize(frame))
            let stable = stabilizer.update(merged, now: elapsed())
            let recognized = clock.now

            if let translator {
                let english = stable.filter { !LanguageFilter.looksTurkish($0.text) }
                let (found, toFetch) = await translator.lookup(english.map(\.text), now: elapsed())
                if !toFetch.isEmpty {
                    Task {
                        switch await translator.fetch(toFetch, now: elapsed()) {
                        case .success: TranslationCacheFile.save(await translator.snapshot())
                        case .failure(let error): FileHandle.standardError.write(Data("DeepL hatası: \(error)\n".utf8))
                        }
                    }
                }
                blocks = english.compactMap { block in found[block.text].map { overlayBlock(block, text: $0) } }
            } else {
                blocks = stable.map { overlayBlock($0, text: $0.text) }
            }

            if verbose {
                print("#\(round) video \(frame.width)x\(frame.height)  yakalama \(milliseconds(captured - roundStart)) ms  OCR \(milliseconds(recognized - captured)) ms  \(merged.count) blok, \(stable.count) sabit, \(blocks.count) gönderilen")
            }
            if let dumpDir, let boxed = drawBoxes(on: frame, boxes: merged.map(\.box)) {
                try writePNG(boxed, to: dumpDir.appendingPathComponent(String(format: "ocr_%04d.png", round)))
            }
        } catch CaptureError.windowNotFound {
            if !waitingForWindow { print("Remote Play penceresi yok; açılması bekleniyor...") }
            waitingForWindow = true
            stabilizer = Stabilizer()
            await frames.stop()
        } catch {
            FileHandle.standardError.write(Data("Tur #\(round) atlandı: \(error)\n".utf8))
        }

        // Liste değişmese de her tur gönderilir: TV 3 sn mesaj almazsa yazıları kaldırır.
        let sent = await client.send(blocks)
        let shown = blocks.map(\.text)
        if shown != lastShown {
            print(shown.isEmpty ? "TV: (boş)" : "TV: " + shown.joined(separator: "  |  "))
            lastShown = shown
        }
        if !sent { try await Task.sleep(for: .seconds(2)) }

        if once {
            try await Task.sleep(for: .seconds(2))
            await client.close()
            return
        }
        let remaining = Duration.seconds(1 / fps) - (clock.now - roundStart)
        if remaining > .zero { try await Task.sleep(for: remaining) }
    }
}

func overlayBlock(_ block: StableBlock, text: String) -> OverlayBlock {
    OverlayBlock(
        id: block.id,
        x: block.box.x,
        y: block.box.y,
        w: block.box.width,
        h: block.box.height,
        lineH: block.lineHeight,
        text: text
    )
}

func milliseconds(_ duration: Duration) -> Int64 {
    duration.components.seconds * 1000 + duration.components.attoseconds / 1_000_000_000_000_000
}

func translateOnce(args: [String]) async throws {
    guard args.count >= 2 else { throw CommandError("Kullanım: ps5ceviri translate \"Door is locked.\"") }
    guard let key = DeepLKey.load() else { throw CommandError("DeepL anahtarı bulunamadı: \(DeepLKey.file.path)") }
    let text = args.dropFirst().joined(separator: " ")
    let translated = try await DeepLClient(key: key).translate([text])
    print("\(text)  →  \(translated.first ?? "")")
}

/// Çıktı biçimi Windows'taki `winprobe ocr` ile aynıdır; iki OCR yan yana karşılaştırılabilsin diye.
func ocrFiles(args: [String]) async throws {
    let paths = Array(args.dropFirst())
    guard !paths.isEmpty else { throw CommandError("Kullanım: ps5ceviri ocr DOSYA.png ...") }
    let recognizer = TextRecognizer()
    let clock = ContinuousClock()
    for path in paths {
        guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            throw CommandError("Görüntü açılamadı: \(path)")
        }
        let start = clock.now
        let lines = try await recognizer.recognize(image)
        print("\(path): \(image.width)x\(image.height)  OCR \(milliseconds(clock.now - start)) ms, \(lines.count) satır")
        for line in lines {
            print(String(format: "    [x=%.3f y=%.3f w=%.3f h=%.3f] \"%@\"", line.box.x, line.box.y, line.box.width, line.box.height, line.text))
        }
    }
}

struct CommandError: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

let args = Array(CommandLine.arguments.dropFirst())
do {
    switch args.first {
    case "windows": try await listWindows()
    case "capture": try await capture(args: args)
    case "run": try await run(args: args)
    case "translate": try await translateOnce(args: args)
    case "ocr": try await ocrFiles(args: args)
    default:
        print(usage)
        exit(args.isEmpty ? 0 : 1)
    }
} catch {
    FileHandle.standardError.write(Data("Hata: \(error)\n".utf8))
    exit(1)
}
