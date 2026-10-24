import Foundation
@testable import PS5CeviriCore
import Testing

/// Çağrıları sayan sahte DeepL.
private actor FakeBackend {
    var calls: [[String]] = []
    var failWith: Error?

    func translate(_ texts: [String]) throws -> [String] {
        calls.append(texts)
        if let failWith { throw failWith }
        return texts.map { "TR(\($0))" }
    }

    func fail(with error: Error?) { failWith = error }
}

struct TranslatorTests {
    @Test func onbellektekiCeviriHemenDoner() async {
        let translator = Translator(cache: ["Door is locked": "Kapı kilitli"]) { _ in [] }

        let result = await translator.lookup(["Door is locked"], now: 0)

        #expect(result.found == ["Door is locked": "Kapı kilitli"])
        #expect(result.toFetch.isEmpty)
    }

    @Test func eksikCeviriIstenirVeOnbellegeYazilir() async {
        let fake = FakeBackend()
        let translator = Translator { try await fake.translate($0) }

        let first = await translator.lookup(["Hello", "Goodbye"], now: 0)
        #expect(first.found.isEmpty)
        #expect(first.toFetch == ["Goodbye", "Hello"])

        await translator.fetch(first.toFetch, now: 0)
        let second = await translator.lookup(["Hello", "Goodbye"], now: 1)

        #expect(second.found == ["Hello": "TR(Hello)", "Goodbye": "TR(Goodbye)"])
        #expect(second.toFetch.isEmpty)
        #expect(await fake.calls == [["Goodbye", "Hello"]])
    }

    @Test func istekSurerkenAyniMetinTekrarIstenmez() async {
        let translator = Translator { _ in [] }

        _ = await translator.lookup(["Hello"], now: 0)
        let again = await translator.lookup(["Hello"], now: 0.5)

        #expect(again.toFetch.isEmpty)
    }

    @Test func hatadanSonraBekleyipTekrarDener() async {
        let fake = FakeBackend()
        await fake.fail(with: TranslationError.http(500))
        let translator = Translator { try await fake.translate($0) }

        let first = await translator.lookup(["Hello"], now: 0)
        await translator.fetch(first.toFetch, now: 0)

        #expect(await translator.lookup(["Hello"], now: 1).toFetch.isEmpty)
        #expect(await translator.lookup(["Hello"], now: 6).toFetch == ["Hello"])
    }

    @Test func kotaDolunca5DakikaIstekAtilmaz() async {
        let fake = FakeBackend()
        await fake.fail(with: TranslationError.quotaExceeded)
        let translator = Translator { try await fake.translate($0) }

        let first = await translator.lookup(["Hello"], now: 0)
        await translator.fetch(first.toFetch, now: 0)

        #expect(await translator.lookup(["Hello"], now: 299).toFetch.isEmpty)
        #expect(await translator.lookup(["Hello"], now: 301).toFetch == ["Hello"])
    }

    @Test func elliMetindenFazlasiPartilereBolunur() async {
        let fake = FakeBackend()
        let translator = Translator { try await fake.translate($0) }
        let texts = (1...120).map { "line \($0)" }

        let lookup = await translator.lookup(texts, now: 0)
        await translator.fetch(lookup.toFetch, now: 0)

        #expect(await fake.calls.map(\.count) == [50, 50, 20])
        #expect(await translator.snapshot().count == 120)
    }

    @Test func deepLIstekVeYanitFormati() throws {
        let body = try DeepL.requestBody(texts: ["The gate is locked."])
        let json = try JSONSerialization.jsonObject(with: body) as? [String: Any]
        #expect(json?["text"] as? [String] == ["The gate is locked."])
        #expect(json?["source_lang"] as? String == "EN")
        #expect(json?["target_lang"] as? String == "TR")

        let response = Data(#"{"translations":[{"detected_source_language":"EN","text":"Kapı kilitli."}]}"#.utf8)
        #expect(try DeepL.parseResponse(response) == ["Kapı kilitli."])
        #expect(throws: TranslationError.invalidResponse) { try DeepL.parseResponse(Data("{}".utf8)) }
    }

    @Test func deepLSunucuSecimiVeHataKodlari() {
        #expect(DeepL.endpoint(forKey: "abc:fx").host == "api-free.deepl.com")
        #expect(DeepL.endpoint(forKey: "abc").host == "api.deepl.com")
        #expect(DeepL.error(forStatus: 200) == nil)
        #expect(DeepL.error(forStatus: 456) == .quotaExceeded)
        #expect(DeepL.error(forStatus: 429) == .rateLimited)
        #expect(DeepL.error(forStatus: 403) == .unauthorized)
    }
}
