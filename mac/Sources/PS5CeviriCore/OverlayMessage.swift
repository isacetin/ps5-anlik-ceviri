import Foundation

/// TV'deki protokolle birebir aynı blok: koordinatlar 0...1, origin sol üst.
public struct OverlayBlock: Codable, Equatable, Sendable {
    public var id: String
    public var x: Double
    public var y: Double
    public var w: Double
    public var h: Double
    public var lineH: Double
    public var text: String

    public init(id: String, x: Double, y: Double, w: Double, h: Double, lineH: Double, text: String) {
        self.id = id
        self.x = x
        self.y = y
        self.w = w
        self.h = h
        self.lineH = lineH
        self.text = text
    }
}

/// Mac → TV mesajı. `blocks` ekranda olması gereken listenin tamamıdır.
public struct OverlayMessage: Codable, Equatable, Sendable {
    public var v: Int = 1
    public var seq: Int64
    public var blocks: [OverlayBlock]

    public init(seq: Int64, blocks: [OverlayBlock]) {
        self.seq = seq
        self.blocks = blocks
    }

    public func json() throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return String(decoding: try encoder.encode(self), as: UTF8.self)
    }
}

/// OCR'ın bulduğu tek satır; `box` video alanına göre normalize, origin sol üst.
public struct TextLine: Equatable, Sendable {
    public var text: String
    public var box: Rect
    public var confidence: Double

    public init(text: String, box: Rect, confidence: Double) {
        self.text = text
        self.box = box
        self.confidence = confidence
    }
}
