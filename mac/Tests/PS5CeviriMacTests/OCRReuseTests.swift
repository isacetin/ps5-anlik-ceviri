@testable import PS5CeviriMac
import PS5CeviriCore
import Testing

/// Durağan sahnede aynı kare yeniden OCR'a gitmez; yeni kare ya da başka alan her zaman okunur.
struct OCRReuseTests {
    let crop = Rect(x: 0, y: 28, width: 1920, height: 1080)
    let lines = [TextLine(text: "Resume", box: Rect(x: 0.1, y: 0.5, width: 0.1, height: 0.03), confidence: 1)]

    @Test func ayniKareYenidenOkunmaz() {
        var cache = OCRReuse<String>()
        #expect(cache.lookup(sequence: 7, crop: crop) == nil)
        cache.store(sequence: 7, crop: crop, lines: lines, frame: "kare-7")

        let hit = cache.lookup(sequence: 7, crop: crop)

        #expect(hit?.lines == lines)
        #expect(hit?.frame == "kare-7")
        #expect(cache.reused == 1)
    }

    @Test func yeniKareYaDaBaskaAlanOkunur() {
        var cache = OCRReuse<String>()
        cache.store(sequence: 7, crop: crop, lines: lines, frame: "kare-7")

        #expect(cache.lookup(sequence: 8, crop: crop) == nil)
        #expect(cache.lookup(sequence: 7, crop: Rect(x: 0, y: 0, width: 1920, height: 1080)) == nil)
        cache.clear()
        #expect(cache.lookup(sequence: 7, crop: crop) == nil)
        #expect(cache.reused == 0)
    }
}
