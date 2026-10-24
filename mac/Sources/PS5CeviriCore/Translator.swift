import Foundation

public enum TranslationError: Error, Equatable, Sendable {
    /// 429: çok sık istek.
    case rateLimited
    /// 456: aylık karakter kotası doldu.
    case quotaExceeded
    /// 403: API anahtarı geçersiz.
    case unauthorized
    case http(Int)
    case invalidResponse
}

/// Çeviri önbelleği ve istek yönetimi. Ana döngü her turda [lookup] ile hazır çevirileri alır,
/// eksikleri [fetch] ile arka planda ister; aynı metin aynı anda iki kez istenmez ve hata
/// sonrasında bir süre yeni istek atılmaz.
public actor Translator {
    public typealias Backend = @Sendable ([String]) async throws -> [String]

    /// DeepL tek istekte en fazla 50 metin kabul eder.
    public static let maxBatch = 50

    private let backend: Backend
    private var cache: [String: String]
    private var inFlight = Set<String>()
    private var retryAfter = -Double.infinity

    public init(cache: [String: String] = [:], backend: @escaping Backend) {
        self.cache = cache
        self.backend = backend
    }

    /// Önbellekteki çeviriler ve çevrilmesi gereken (henüz istenmemiş) metinler.
    public func lookup(_ texts: [String], now: Double) -> (found: [String: String], toFetch: [String]) {
        var found: [String: String] = [:]
        var toFetch: [String] = []
        for text in Set(texts).sorted() {
            if let translation = cache[text] {
                found[text] = translation
            } else if !inFlight.contains(text) {
                toFetch.append(text)
            }
        }
        if now < retryAfter { toFetch = [] }
        inFlight.formUnion(toFetch)
        return (found, toFetch)
    }

    /// Metinleri çevirip önbelleğe yazar. Hata olursa metinler sonraki turda tekrar istenebilir.
    @discardableResult
    public func fetch(_ texts: [String], now: Double) async -> Result<Void, Error> {
        defer { inFlight.subtract(texts) }
        do {
            for start in stride(from: 0, to: texts.count, by: Self.maxBatch) {
                let batch = Array(texts[start..<min(start + Self.maxBatch, texts.count)])
                let translations = try await backend(batch)
                guard translations.count == batch.count else { throw TranslationError.invalidResponse }
                for (source, translation) in zip(batch, translations) { cache[source] = translation }
            }
            return .success(())
        } catch {
            retryAfter = now + Self.backoff(for: error)
            return .failure(error)
        }
    }

    public func snapshot() -> [String: String] { cache }

    static func backoff(for error: Error) -> Double {
        switch error as? TranslationError {
        case .quotaExceeded, .unauthorized: 300
        case .rateLimited: 10
        default: 5
        }
    }
}
