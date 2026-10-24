import Foundation
import PS5CeviriCore

/// DeepL API anahtarı: önce DEEPL_AUTH_KEY ortam değişkeni, sonra anahtar dosyası.
enum DeepLKey {
    static let file = appSupportDirectory.appendingPathComponent("deepl-key.txt")

    static func load() -> String? {
        if let key = ProcessInfo.processInfo.environment["DEEPL_AUTH_KEY"], !key.isEmpty { return key }
        let key = (try? String(contentsOf: file, encoding: .utf8))?.trimmingCharacters(in: .whitespacesAndNewlines)
        return key?.isEmpty == false ? key : nil
    }
}

/// DeepL /v2/translate çağrısı (EN → TR).
struct DeepLClient {
    let key: String

    func translate(_ texts: [String]) async throws -> [String] {
        var request = URLRequest(url: DeepL.endpoint(forKey: key))
        request.httpMethod = "POST"
        request.setValue("DeepL-Auth-Key \(key)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try DeepL.requestBody(texts: texts)
        request.timeoutInterval = 10

        let (data, response) = try await URLSession.shared.data(for: request)
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        if let error = DeepL.error(forStatus: status) { throw error }
        return try DeepL.parseResponse(data)
    }
}

/// Çeviri önbelleği diskte tutulur; tekrar eden metin ikinci kez kota harcamaz.
enum TranslationCacheFile {
    static let file = appSupportDirectory.appendingPathComponent("translations.json")

    static func load() -> [String: String] {
        guard let data = try? Data(contentsOf: file) else { return [:] }
        return (try? JSONDecoder().decode([String: String].self, from: data)) ?? [:]
    }

    static func save(_ cache: [String: String]) {
        do {
            try FileManager.default.createDirectory(at: appSupportDirectory, withIntermediateDirectories: true)
            let encoder = JSONEncoder()
            encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
            try encoder.encode(cache).write(to: file, options: .atomic)
        } catch {
            FileHandle.standardError.write(Data("Önbellek kaydedilemedi: \(error.localizedDescription)\n".utf8))
        }
    }
}

let appSupportDirectory = FileManager.default.homeDirectoryForCurrentUser
    .appendingPathComponent("Library/Application Support/PS5Ceviri")
