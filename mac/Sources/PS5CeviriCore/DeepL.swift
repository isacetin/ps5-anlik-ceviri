import Foundation

/// DeepL API v2 /translate istek ve yanıt formatı (EN → TR).
public enum DeepL {
    /// Free anahtarlar ":fx" ile biter ve ayrı sunucuya gider.
    public static func endpoint(forKey key: String) -> URL {
        URL(string: key.hasSuffix(":fx")
            ? "https://api-free.deepl.com/v2/translate"
            : "https://api.deepl.com/v2/translate")!
    }

    struct Request: Encodable {
        let text: [String]
        let source_lang = "EN"
        let target_lang = "TR"
    }

    struct Response: Decodable {
        struct Translation: Decodable { let text: String }
        let translations: [Translation]
    }

    public static func requestBody(texts: [String]) throws -> Data {
        try JSONEncoder().encode(Request(text: texts))
    }

    public static func parseResponse(_ data: Data) throws -> [String] {
        do {
            return try JSONDecoder().decode(Response.self, from: data).translations.map(\.text)
        } catch {
            throw TranslationError.invalidResponse
        }
    }

    /// HTTP durum kodu → hata (200 ise nil).
    public static func error(forStatus status: Int) -> TranslationError? {
        switch status {
        case 200: nil
        case 403: .unauthorized
        case 429: .rateLimited
        case 456: .quotaExceeded
        default: .http(status)
        }
    }
}
