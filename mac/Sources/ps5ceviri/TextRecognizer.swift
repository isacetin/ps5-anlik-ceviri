import CoreGraphics
import Foundation
import PS5CeviriCore
import Vision

/// Apple Vision ile İngilizce metin satırlarını ve kutularını çıkarır.
struct TextRecognizer {
    func recognize(_ image: CGImage) async throws -> [TextLine] {
        var request = RecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.recognitionLanguages = [Locale.Language(identifier: "en-US")]
        request.usesLanguageCorrection = true

        let observations = try await request.perform(on: image)
        return observations.compactMap { observation in
            guard let candidate = observation.topCandidates(1).first else { return nil }
            let box = observation.boundingBox.cgRect
            return TextLine(
                text: candidate.string,
                box: Geometry.topLeftBox(fromVision: Rect(x: box.minX, y: box.minY, width: box.width, height: box.height)),
                confidence: Double(candidate.confidence)
            )
        }
    }
}
