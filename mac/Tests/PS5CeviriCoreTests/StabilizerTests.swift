@testable import PS5CeviriCore
import Testing

struct StabilizerTests {
    private func block(_ text: String, x: Double = 0.2, y: Double = 0.8) -> MergedBlock {
        MergedBlock(box: Rect(x: x, y: y, width: 0.5, height: 0.06), lineHeight: 0.03, text: text)
    }

    @Test func tekKaredeGorulenMetinHenuzGosterilmez() {
        var stabilizer = Stabilizer()

        #expect(stabilizer.update([block("Door is locked")], now: 0).isEmpty)
    }

    @Test func ikiKareAyniKalanMetinGosterilir() {
        var stabilizer = Stabilizer()
        _ = stabilizer.update([block("Door is locked")], now: 0)

        let shown = stabilizer.update([block("Door is locked")], now: 0.5)

        #expect(shown.map(\.text) == ["Door is locked"])
    }

    @Test func daktiloEfektindeMetinBitmedenGosterilmez() {
        var stabilizer = Stabilizer()
        #expect(stabilizer.update([block("You sho")], now: 0).isEmpty)
        #expect(stabilizer.update([block("You shouldn't")], now: 0.5).isEmpty)
        #expect(stabilizer.update([block("You shouldn't be here.")], now: 1.0).isEmpty)

        let shown = stabilizer.update([block("You shouldn't be here.")], now: 1.5)

        #expect(shown.map(\.text) == ["You shouldn't be here."])
    }

    @Test func ocrHarfOynamasiMetniDegistirmez() {
        var stabilizer = Stabilizer()
        _ = stabilizer.update([block("The gate is locked.")], now: 0)
        _ = stabilizer.update([block("The gate is locked")], now: 0.5)

        // Tek harf oynaması: "1" / "l". Kabul edilen metin ilk okunduğu gibi kalır.
        let shown = stabilizer.update([block("The gate is 1ocked.")], now: 1.0)

        #expect(shown.map(\.text) == ["The gate is locked."])
    }

    @Test func yeniMetinOturanaKadarEskiMetinGosterilir() {
        var stabilizer = Stabilizer()
        _ = stabilizer.update([block("Hello, traveler.")], now: 0)
        _ = stabilizer.update([block("Hello, traveler.")], now: 0.5)

        // Diyalog değişiyor: yeni satır bir kare görüldü, henüz oturmadı.
        #expect(stabilizer.update([block("The gate is locked.")], now: 1.0).map(\.text) == ["Hello, traveler."])
        #expect(stabilizer.update([block("The gate is locked.")], now: 1.5).map(\.text) == ["The gate is locked."])
    }

    @Test func gosterilenMetinTekKarelikOcrHatasindaKaybolmaz() {
        var stabilizer = Stabilizer()
        _ = stabilizer.update([block("Konsol Depolama Alanı")], now: 0)
        _ = stabilizer.update([block("Konsol Depolama Alanı")], now: 0.5)

        #expect(stabilizer.update([block("Konsol Depolama")], now: 1.0).map(\.text) == ["Konsol Depolama Alanı"])
        #expect(stabilizer.update([block("Konsol Depolama Alanı")], now: 1.5).map(\.text) == ["Konsol Depolama Alanı"])
    }

    @Test func kimlikKarelerBoyuncaSabitKalir() {
        var stabilizer = Stabilizer()
        _ = stabilizer.update([block("Hello", y: 0.80)], now: 0)
        let first = stabilizer.update([block("Hello", y: 0.801)], now: 0.5)
        let second = stabilizer.update([block("Hello", y: 0.799)], now: 1.0)

        #expect(first.map(\.id) == second.map(\.id))
    }

    @Test func birKareKaybolanBlokTutulur() {
        var stabilizer = Stabilizer()
        _ = stabilizer.update([block("Hello")], now: 0)
        _ = stabilizer.update([block("Hello")], now: 0.5)

        #expect(stabilizer.update([], now: 1.0).map(\.text) == ["Hello"])
        #expect(stabilizer.update([], now: 1.4).isEmpty)
    }

    @Test func farkliKonumdakiBloklarAyriTakipEdilir() {
        var stabilizer = Stabilizer()
        _ = stabilizer.update([block("Top", y: 0.1), block("Bottom", y: 0.8)], now: 0)

        let shown = stabilizer.update([block("Top", y: 0.1), block("Bottom", y: 0.8)], now: 0.5)

        #expect(Set(shown.map(\.text)) == ["Top", "Bottom"])
        #expect(Set(shown.map(\.id)).count == 2)
    }

    @Test func normalizasyonNoktalamaVeBuyukHarfiYokSayar() {
        #expect(Stabilizer.sameText("KAPI KİLİTLİ!", "kapı  kilitli"))
        #expect(!Stabilizer.sameText("Hello", "Hello there"))
    }
}
