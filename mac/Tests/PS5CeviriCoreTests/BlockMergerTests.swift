@testable import PS5CeviriCore
import Testing

struct BlockMergerTests {
    private func line(_ text: String, _ x: Double, _ y: Double, _ w: Double, _ h: Double, confidence: Double = 0.9) -> TextLine {
        TextLine(text: text, box: Rect(x: x, y: y, width: w, height: h), confidence: confidence)
    }

    @Test func soldanHizaliCokSatirliDiyalogTekBlokOlur() {
        let blocks = BlockMerger.merge([
            line("You shouldn't have come here,", 0.20, 0.780, 0.50, 0.035),
            line("traveler. The gate is locked.", 0.20, 0.822, 0.46, 0.035),
        ])

        #expect(blocks.count == 1)
        #expect(blocks[0].text == "You shouldn't have come here, traveler. The gate is locked.")
        #expect(abs(blocks[0].box.x - 0.20) < 1e-9)
        #expect(abs(blocks[0].box.y - 0.780) < 1e-9)
        #expect(abs(blocks[0].box.width - 0.50) < 1e-9)
        #expect(abs(blocks[0].box.height - 0.077) < 1e-9)
        #expect(abs(blocks[0].lineHeight - 0.035) < 1e-9)
    }

    @Test func ortalanmisUzunAltyaziSatirlariBirlesir() {
        let blocks = BlockMerger.merge([
            line("I left the key by the old well near the", 0.27, 0.85, 0.46, 0.04),
            line("Village gate.", 0.43, 0.895, 0.14, 0.04),
        ])

        #expect(blocks.map(\.text) == ["I left the key by the old well near the Village gate."])
    }

    @Test func kucukHarfleDevamEdenKisaSatirBirlesir() {
        // Gerçek PS5 ana ekranı: "PS5 sisteminiz" / "kullanışlı hale".
        let blocks = BlockMerger.merge([
            line("PS5 sisteminiz", 0.925, 0.563, 0.073, 0.022),
            line("kullanışlı hale", 0.925, 0.593, 0.073, 0.022),
        ])

        #expect(blocks.map(\.text) == ["PS5 sisteminiz kullanışlı hale"])
    }

    @Test func menuOgeleriAyriKalir() {
        // Gerçek oyun menüsü (It Takes Two): alt alta, soldan hizalı kısa öğeler.
        let blocks = BlockMerger.merge([
            line("Play Local", 0.115, 0.591, 0.087, 0.030),
            line("Play Online", 0.099, 0.655, 0.085, 0.028),
            line("Options", 0.099, 0.717, 0.061, 0.028),
            line("Accessibility Options", 0.099, 0.779, 0.165, 0.028),
            line("Credits", 0.099, 0.841, 0.057, 0.028),
        ])

        #expect(blocks.map(\.text) == ["Play Local", "Play Online", "Options", "Accessibility Options", "Credits"])
    }

    @Test func farkliCumleylerBaslayanKisaAltyazilarAyriKalir() {
        let blocks = BlockMerger.merge([
            line("Where is the key?", 0.40, 0.85, 0.20, 0.04),
            line("I left it by the well.", 0.38, 0.895, 0.24, 0.04),
        ])

        #expect(blocks.count == 2)
    }

    @Test func ayniSatirdaBolunmusParcalarSoldanSagaBirlesir() {
        // Gerçek PS5 OCR çıktısından: "EA SPORTS FC™" + "27 Standart Sürüm PS4 ve PS5".
        let blocks = BlockMerger.merge([
            line("27 Standart Sürüm PS4 ve PS5", 0.733, 0.715, 0.150, 0.019),
            line("EA SPORTS FC™", 0.654, 0.715, 0.074, 0.019),
        ])

        #expect(blocks.map(\.text) == ["EA SPORTS FC™ 27 Standart Sürüm PS4 ve PS5"])
    }

    @Test func uzaktakiFarkliKartlarAyriKalir() {
        // PS5 ana ekranı: yan yana iki kart başlığı ve altlarındaki uzak satırlar.
        let blocks = BlockMerger.merge([
            line("Kupalar", 0.106, 0.511, 0.060, 0.022),
            line("Konsol Depolama Alanı", 0.381, 0.510, 0.138, 0.028),
            line("Son Oynananlar", 0.104, 0.674, 0.104, 0.026),
        ])

        #expect(blocks.map(\.text) == ["Konsol Depolama Alanı", "Kupalar", "Son Oynananlar"])
    }

    @Test func boyutuCokFarkliSatirlarBirlesmez() {
        let blocks = BlockMerger.merge([
            line("CHAPTER ONE", 0.30, 0.40, 0.40, 0.08),
            line("The beginning", 0.30, 0.49, 0.20, 0.03),
        ])

        #expect(blocks.count == 2)
    }

    @Test func gecersizSatirlarElenir() {
        let blocks = BlockMerger.merge([
            line("Hello there", 0.1, 0.1, 0.2, 0.03),
            line("Bulanık", 0.1, 0.5, 0.2, 0.03, confidence: 0.1),
            line("A", 0.5, 0.5, 0.01, 0.03),
            line("2,799.30", 0.7, 0.7, 0.1, 0.03),
        ])

        #expect(blocks.map(\.text) == ["Hello there"])
    }

    @Test func satirSonuTirelemesiGeriAlinir() {
        #expect(BlockMerger.joinRows(["The trav-", "eler waits."]) == "The traveler waits.")
        #expect(BlockMerger.joinRows(["Mid-", "Autumn"]) == "Mid- Autumn")
    }
}
