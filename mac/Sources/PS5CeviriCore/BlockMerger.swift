import Foundation

/// Birleştirilmiş metin bloğu: bir veya birden çok OCR satırından oluşan paragraf.
public struct MergedBlock: Equatable, Sendable {
    /// Satır kutularının birleşimi; video alanına göre normalize, origin sol üst.
    public var box: Rect
    /// Medyan satır yüksekliği (normalize); TV'de font boyutu için başlangıç değeri.
    public var lineHeight: Double
    public var text: String

    public init(box: Rect, lineHeight: Double, text: String) {
        self.box = box
        self.lineHeight = lineHeight
        self.text = text
    }
}

/// OCR satırlarını filtreler ve aynı paragrafa ait olanları tek bloğa birleştirir, böylece
/// çok satırlı bir diyalog DeepL'e tek cümle olarak gider.
public enum BlockMerger {
    public struct Options: Sendable {
        public var minConfidence = 0.4
        /// Alt alta iki satır arasındaki boşluk, satır yüksekliğinin en fazla bu katı olabilir.
        public var maxLineGap = 0.7
        /// Satır yükseklikleri oranı (büyük / küçük) en fazla bu kadar olabilir.
        public var maxHeightRatio = 1.4
        /// Sol kenar ya da merkez hizası için tolerans (satır yüksekliği cinsinden).
        public var alignTolerance = 1.0
        /// Aynı satırda bölünmüş iki parça arasındaki yatay boşluk (satır yüksekliği cinsinden).
        public var maxWordGap = 1.2
        /// Üst satır en az bu genişlikteyse (satır yüksekliği cinsinden) kaydırılmış paragraf satırı
        /// sayılır. Daha kısa satırlar (menü öğeleri) yalnızca alt satır küçük harfle devam ediyorsa birleşir.
        public var minWrapWidth = 12.0
        /// x koordinatlarını y ile aynı birime çevirmek için video en-boy oranı.
        public var aspect = Geometry.videoAspect

        public init() {}
    }

    public static func merge(_ lines: [TextLine], options: Options = Options()) -> [MergedBlock] {
        let valid = lines.compactMap { line -> TextLine? in
            var line = line
            line.text = line.text.trimmingCharacters(in: .whitespacesAndNewlines)
            guard line.confidence >= options.minConfidence, line.text.count >= 2, line.text.contains(where: \.isLetter) else {
                return nil
            }
            return line
        }

        // Union-find: birleştirilebilir her satır çifti aynı gruba düşer.
        var parent = Array(valid.indices)
        func root(_ index: Int) -> Int {
            var index = index
            while parent[index] != index {
                parent[index] = parent[parent[index]]
                index = parent[index]
            }
            return index
        }
        for i in valid.indices {
            for j in valid.indices where j > i && belongTogether(valid[i], valid[j], options) {
                parent[root(j)] = root(i)
            }
        }

        let groups = Dictionary(grouping: valid.indices, by: root).values.map { $0.map { valid[$0] } }
        return groups
            .map { makeBlock(from: $0) }
            .sorted { ($0.box.y, $0.box.x) < ($1.box.y, $1.box.x) }
    }

    /// Satır kutusu, x ekseni en-boy oranıyla y birimine çevrilmiş haliyle.
    private struct Box {
        let left, right, top, bottom: Double
        var height: Double { bottom - top }
        var centerX: Double { (left + right) / 2 }
        var centerY: Double { (top + bottom) / 2 }

        init(_ rect: Rect, aspect: Double) {
            left = rect.x * aspect
            right = (rect.x + rect.width) * aspect
            top = rect.y
            bottom = rect.y + rect.height
        }
    }

    private static func belongTogether(_ a: TextLine, _ b: TextLine, _ options: Options) -> Bool {
        let boxA = Box(a.box, aspect: options.aspect)
        let boxB = Box(b.box, aspect: options.aspect)
        let small = min(boxA.height, boxB.height)
        let large = max(boxA.height, boxB.height)
        guard small > 0, large / small <= options.maxHeightRatio else { return false }
        let lineHeight = (boxA.height + boxB.height) / 2

        // Aynı satırın OCR tarafından bölünmüş parçaları.
        if abs(boxA.centerY - boxB.centerY) <= lineHeight * 0.3 {
            let gap = max(boxA.left, boxB.left) - min(boxA.right, boxB.right)
            return gap <= lineHeight * options.maxWordGap
        }

        // Alt alta satırlar: dikey boşluk küçük, yatayda örtüşüyor ve solda ya da ortada hizalı.
        let aIsUpper = boxA.top <= boxB.top
        let (upper, lower) = aIsUpper ? (boxA, boxB) : (boxB, boxA)
        let lowerText = aIsUpper ? b.text : a.text
        let verticalGap = lower.top - upper.bottom
        guard verticalGap <= lineHeight * options.maxLineGap, verticalGap > -lineHeight * 0.5 else { return false }
        let overlaps = min(upper.right, lower.right) > max(upper.left, lower.left)
        let tolerance = lineHeight * options.alignTolerance
        let leftAligned = abs(upper.left - lower.left) <= tolerance
        let centered = abs(upper.centerX - lower.centerX) <= tolerance
        guard overlaps, leftAligned || centered else { return false }

        // Menü listesi değil paragraf mı: üst satır kaydırılacak kadar uzun ya da alt satır cümleye
        // küçük harfle devam ediyor.
        let upperWrapped = upper.right - upper.left >= lineHeight * options.minWrapWidth
        let continuesSentence = lowerText.first?.isLowercase == true
        return upperWrapped || continuesSentence
    }

    private static func makeBlock(from lines: [TextLine]) -> MergedBlock {
        let heights = lines.map(\.box.height).sorted()
        let medianHeight = heights[heights.count / 2]

        // Satırları yukarıdan aşağı satır gruplarına, her grubu soldan sağa diz.
        var rows: [[TextLine]] = []
        for line in lines.sorted(by: { centerY($0) < centerY($1) }) {
            if let last = rows.last?.last, abs(centerY(line) - centerY(last)) <= medianHeight * 0.5 {
                rows[rows.count - 1].append(line)
            } else {
                rows.append([line])
            }
        }
        let rowTexts = rows.map { row in row.sorted { $0.box.x < $1.box.x }.map(\.text).joined(separator: " ") }

        let left = lines.map(\.box.x).min()!
        let top = lines.map(\.box.y).min()!
        let right = lines.map { $0.box.x + $0.box.width }.max()!
        let bottom = lines.map { $0.box.y + $0.box.height }.max()!
        return MergedBlock(
            box: Rect(x: left, y: top, width: right - left, height: bottom - top),
            lineHeight: medianHeight,
            text: joinRows(rowTexts)
        )
    }

    /// Satırları boşlukla birleştirir; satır sonu tirelemesini ("trav-" + "eler") geri alır.
    static func joinRows(_ rows: [String]) -> String {
        var result = ""
        for row in rows {
            if result.isEmpty {
                result = row
            } else if result.hasSuffix("-"), let first = row.first, first.isLowercase {
                result.removeLast()
                result += row
            } else {
                result += " " + row
            }
        }
        return result
    }

    private static func centerY(_ line: TextLine) -> Double { line.box.y + line.box.height / 2 }
}
