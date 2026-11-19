@testable import PS5CeviriMac
import Synchronization
import Testing

/// Vision ya da ScreenCaptureKit hiç dönmezse: çağıran süre sınırında devam etmeli, takılan iş
/// bitene kadar yenisi başlatılmamalı (her tur yeni bir takılı istek birikmesin).
struct TimeoutTests {
    @Test func takilanIsSureSinirindaBirakilir() async {
        let clock = ContinuousClock()
        let start = clock.now
        await #expect(throws: TimeoutError.self) {
            try await withTimeout(seconds: 0.2) { try await Task.sleep(for: .seconds(5)) }
        }
        #expect(clock.now - start < .seconds(2))
    }

    @Test func zamanindaBitenIsSonucunuDoner() async throws {
        let value = try await withTimeout(seconds: 2) { 42 }
        #expect(value == 42)
    }

    @Test func ocrKapisiTakilanIsBitenekadarKapaliKalir() async throws {
        let gate = OCRGate()
        let started = Mutex(0)
        // Vision'ın takıldığı tur: süre sınırı dolar ama iş arka planda sürer.
        #expect(gate.enter())
        await #expect(throws: TimeoutError.self) {
            try await withTimeout(seconds: 0.1) {
                defer { gate.leave() }
                started.withLock { $0 += 1 }
                try await Task.sleep(for: .milliseconds(600))
            }
        }
        // Sonraki turlar yeni istek başlatmaz.
        #expect(!gate.enter())
        #expect(!gate.enter())
        try await Task.sleep(for: .milliseconds(900))
        // Takılan iş bitti: kapı yeniden açılır.
        #expect(gate.enter())
        gate.leave()
        #expect(started.withLock { $0 } == 1)
    }
}
