//! DeepL API v2 /translate istek ve yanıt formatı (EN → TR). HTTP çağrısının kendisi platform
//! katmanındadır; burada yalnızca format ve hata eşlemesi vardır.

use serde::{Deserialize, Serialize};

use crate::translator::TranslationError;

/// Free anahtarlar ":fx" ile biter ve ayrı sunucuya gider.
pub fn endpoint(key: &str) -> &'static str {
    if key.ends_with(":fx") {
        "https://api-free.deepl.com/v2/translate"
    } else {
        "https://api.deepl.com/v2/translate"
    }
}

/// `Authorization` başlığının değeri.
pub fn auth_header(key: &str) -> String {
    format!("DeepL-Auth-Key {key}")
}

/// Metinler XML olarak gider: `terms::Terms::protect` ile kaçışlanmış olmalı; `<x>` içi çevrilmez.
#[derive(Serialize)]
struct Request<'a> {
    text: &'a [String],
    source_lang: &'static str,
    target_lang: &'static str,
    tag_handling: &'static str,
    ignore_tags: [&'static str; 1],
}

#[derive(Deserialize)]
struct Response {
    translations: Vec<Translation>,
}

#[derive(Deserialize)]
struct Translation {
    text: String,
}

pub fn request_body(texts: &[String]) -> String {
    serde_json::to_string(&Request { text: texts, source_lang: "EN", target_lang: "TR", tag_handling: "xml", ignore_tags: ["x"] })
        .expect("istek serileştirilebilir")
}

pub fn parse_response(body: &str) -> Result<Vec<String>, TranslationError> {
    serde_json::from_str::<Response>(body)
        .map(|response| response.translations.into_iter().map(|t| t.text).collect())
        .map_err(|_| TranslationError::InvalidResponse)
}

/// HTTP durum kodu → hata (200 ise `None`).
pub fn error_for_status(status: u16) -> Option<TranslationError> {
    match status {
        200 => None,
        403 => Some(TranslationError::Unauthorized),
        429 => Some(TranslationError::RateLimited),
        456 => Some(TranslationError::QuotaExceeded),
        other => Some(TranslationError::Http(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn istek_ve_yanit_formati() {
        let body: serde_json::Value = serde_json::from_str(&request_body(&["The gate is locked.".into()])).unwrap();
        assert_eq!(body["text"], serde_json::json!(["The gate is locked."]));
        assert_eq!(body["source_lang"], "EN");
        assert_eq!(body["target_lang"], "TR");
        assert_eq!(body["tag_handling"], "xml");
        assert_eq!(body["ignore_tags"], serde_json::json!(["x"]));

        let response = r#"{"translations":[{"detected_source_language":"EN","text":"Kapı kilitli."}]}"#;
        assert_eq!(parse_response(response), Ok(vec!["Kapı kilitli.".to_string()]));
        assert_eq!(parse_response("{}"), Err(TranslationError::InvalidResponse));
    }

    #[test]
    fn sunucu_secimi_ve_hata_kodlari() {
        assert!(endpoint("abc:fx").starts_with("https://api-free.deepl.com/"));
        assert!(endpoint("abc").starts_with("https://api.deepl.com/"));
        assert_eq!(auth_header("abc:fx"), "DeepL-Auth-Key abc:fx");
        assert_eq!(error_for_status(200), None);
        assert_eq!(error_for_status(456), Some(TranslationError::QuotaExceeded));
        assert_eq!(error_for_status(429), Some(TranslationError::RateLimited));
        assert_eq!(error_for_status(403), Some(TranslationError::Unauthorized));
        assert_eq!(error_for_status(500), Some(TranslationError::Http(500)));
    }
}
