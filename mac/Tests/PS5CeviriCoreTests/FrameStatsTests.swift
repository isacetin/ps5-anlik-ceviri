import PS5CeviriCore
import Testing

struct FrameStatsTests {
    @Test func tamamenSiyahKareBosSayilir() {
        let stats = FrameStats.analyze(luma: [UInt8](repeating: 0, count: 1000))

        #expect(stats.looksBlank)
        #expect(stats.meanLuma == 0)
    }

    @Test func siyahaYakinGurultuBosSayilir() {
        let stats = FrameStats.analyze(luma: [UInt8](repeating: 10, count: 1000))

        #expect(stats.looksBlank)
    }

    @Test func karanlikSahnedeBileBirazIcerikVarsaBosDegildir() {
        // Ekranın %5'i aydınlık (ör. gece sahnesinde bir altyazı).
        var luma = [UInt8](repeating: 0, count: 1000)
        for index in 0..<50 { luma[index] = 200 }

        let stats = FrameStats.analyze(luma: luma)

        #expect(!stats.looksBlank)
        #expect(abs(stats.nonDarkFraction - 0.05) < 0.0001)
        #expect(abs(stats.meanLuma - 10) < 0.0001)
    }

    @Test func bosGirdiCokmez() {
        #expect(FrameStats.analyze(luma: []).looksBlank)
    }
}
