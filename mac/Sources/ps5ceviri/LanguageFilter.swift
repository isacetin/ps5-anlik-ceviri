import NaturalLanguage

/// PS5'in kendi arayüzü Türkçe; Türkçe metni DeepL'e göndermek kota harcar ve Türkçe yazının
/// üstüne yine Türkçe çizer. Türkçe görünen metinler çeviriye gitmez.
enum LanguageFilter {
    private static let turkishLetters = Set("çğışöüÇĞİŞÖÜ")

    static func looksTurkish(_ text: String) -> Bool {
        if text.contains(where: turkishLetters.contains) { return true }
        let recognizer = NLLanguageRecognizer()
        recognizer.languageConstraints = [.english, .turkish]
        recognizer.processString(text)
        let hypotheses = recognizer.languageHypotheses(withMaximum: 2)
        return (hypotheses[.turkish] ?? 0) > 0.8
    }
}
