import Foundation
import PS5CeviriCore
import Testing

struct OverlayMessageTests {
    @Test func tvProtokoluIleAyniJSONUretilir() throws {
        let message = OverlayMessage(seq: 123, blocks: [
            OverlayBlock(id: "abc123", x: 0.12, y: 0.81, w: 0.6, h: 0.08, lineH: 0.03, text: "KAPI KİLİTLİ"),
        ])

        let json = try message.json()

        #expect(json == #"{"blocks":[{"h":0.08,"id":"abc123","lineH":0.03,"text":"KAPI KİLİTLİ","w":0.6,"x":0.12,"y":0.81}],"seq":123,"v":1}"#)
    }

    @Test func bosListeGecerliMesajdir() throws {
        #expect(try OverlayMessage(seq: 1, blocks: []).json() == #"{"blocks":[],"seq":1,"v":1}"#)
    }
}
