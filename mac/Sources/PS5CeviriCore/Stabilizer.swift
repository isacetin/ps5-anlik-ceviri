import Foundation

/// Ekranda gösterilmeye hazır, kimliği kareler boyunca sabit kalan blok.
public struct StableBlock: Equatable, Sendable {
    public var id: String
    public var box: Rect
    public var lineHeight: Double
    /// Kabul edilmiş (çeviriye gidecek) kaynak metin.
    public var text: String

    public init(id: String, box: Rect, lineHeight: Double, text: String) {
        self.id = id
        self.box = box
        self.lineHeight = lineHeight
        self.text = text
    }
}

/// Karelerden gelen blokları zaman içinde takip eder:
/// - Metin [minStableFrames] kare boyunca değişmeden kalınca "hazır" olur (daktilo efektiyle
///   harf harf beliren metin, yazımı bitmeden çevrilmez).
/// - OCR'ın tek karelik harf oynamaları ("Kupalar" / "Kupalar.") metni değiştirmiş sayılmaz.
/// - Ekrandaki bir bloğun metni değişirse, yeni metin oturana kadar eski metin gösterilmeye
///   devam eder (diyalog geçişlerinde ve OCR oynamalarında boşluk/titreme olmaz).
/// - Bir kare görünmeyen blok [holdSeconds] boyunca tutulur; titreme olmaz.
public struct Stabilizer: Sendable {
    public var minStableFrames = 2
    public var holdSeconds = 0.8
    /// Yeni blok, takip edilen blokla en az bu kadar örtüşürse (IoU) aynı blok sayılır.
    public var minOverlap = 0.3

    private struct Tracked {
        var id: String
        var block: MergedBlock
        /// Gösterilen metin; henüz hiçbir metin oturmadıysa nil.
        var acceptedText: String?
        /// Oturması beklenen metin ve kaç karedir aynı kaldığı.
        var candidateText: String
        var candidateFrames: Int
        var lastSeen: Double

        mutating func observe(_ text: String, minStableFrames: Int) {
            if let accepted = acceptedText, Stabilizer.sameText(accepted, text) {
                candidateText = accepted
                candidateFrames = minStableFrames
                return
            }
            if Stabilizer.sameText(candidateText, text) {
                candidateFrames += 1
            } else {
                candidateText = text
                candidateFrames = 1
            }
            if candidateFrames >= minStableFrames { acceptedText = candidateText }
        }
    }

    private var tracked: [Tracked] = []
    private var nextID = 1

    public init() {}

    public mutating func update(_ blocks: [MergedBlock], now: Double) -> [StableBlock] {
        var matched = Set<Int>()
        for block in blocks {
            let candidate = tracked.indices
                .filter { !matched.contains($0) }
                .map { ($0, iou(tracked[$0].block.box, block.box)) }
                .filter { $0.1 >= minOverlap }
                .max { $0.1 < $1.1 }

            if let (index, _) = candidate {
                matched.insert(index)
                tracked[index].block = block
                tracked[index].lastSeen = now
                tracked[index].observe(block.text, minStableFrames: minStableFrames)
            } else {
                var new = Tracked(id: "b\(nextID)", block: block, acceptedText: nil, candidateText: block.text, candidateFrames: 0, lastSeen: now)
                new.observe(block.text, minStableFrames: minStableFrames)
                tracked.append(new)
                matched.insert(tracked.count - 1)
                nextID += 1
            }
        }

        tracked.removeAll { now - $0.lastSeen > holdSeconds }
        return tracked.compactMap { item in
            item.acceptedText.map { StableBlock(id: item.id, box: item.block.box, lineHeight: item.block.lineHeight, text: $0) }
        }
    }

    /// Noktalama/boşluk/büyük-küçük harf farkı ve aynı uzunlukta tek tük harf oynaması aynı
    /// metin sayılır. Uzunluk değişirse (daktilo efekti) farklı metindir.
    static func sameText(_ a: String, _ b: String) -> Bool {
        let left = normalize(a)
        let right = normalize(b)
        if left == right { return true }
        guard left.count == right.count, !left.isEmpty else { return false }
        let differences = zip(left, right).filter { $0 != $1 }.count
        return differences <= max(1, left.count / 20)
    }

    /// Türkçe yerel ayarla: "KAPI KİLİTLİ" ile "kapı kilitli" aynı sayılır (I→ı, İ→i).
    static func normalize(_ text: String) -> String {
        // Önce küçük harf: aksan katlaması önce yapılırsa İ noktasını kaybedip ı olur.
        text.lowercased(with: Locale(identifier: "tr_TR"))
            .folding(options: .diacriticInsensitive, locale: Locale(identifier: "tr_TR"))
            .unicodeScalars
            .filter { CharacterSet.alphanumerics.contains($0) || $0 == " " }
            .map(String.init)
            .joined()
            .split(separator: " ")
            .joined(separator: " ")
    }

    private func iou(_ a: Rect, _ b: Rect) -> Double {
        let width = min(a.x + a.width, b.x + b.width) - max(a.x, b.x)
        let height = min(a.y + a.height, b.y + b.height) - max(a.y, b.y)
        guard width > 0, height > 0 else { return 0 }
        let intersection = width * height
        return intersection / (a.width * a.height + b.width * b.height - intersection)
    }
}
