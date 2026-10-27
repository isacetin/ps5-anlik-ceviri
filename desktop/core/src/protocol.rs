//! Masaüstü → TV WebSocket mesajı. Android TV uygulamasındaki `TranslationMessage` ile birebir aynı.

use serde::{Deserialize, Serialize};

/// Koordinatlar 0...1, origin sol üst. Alanlar alfabetik sırada: JSON çıktısı Swift
/// sürümünün (sortedKeys) ürettiğiyle aynı olsun diye.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayBlock {
    pub h: f64,
    pub id: String,
    #[serde(rename = "lineH")]
    pub line_h: f64,
    pub text: String,
    pub w: f64,
    pub x: f64,
    pub y: f64,
}

/// `blocks` ekranda olması gereken listenin tamamıdır; `[]` ekranı temizler.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayMessage {
    pub blocks: Vec<OverlayBlock>,
    /// Bağlantı içinde artan sayı; TV daha küçüğünü yok sayar.
    pub seq: i64,
    pub v: i32,
}

impl OverlayMessage {
    pub const VERSION: i32 = 1;

    pub fn new(seq: i64, blocks: Vec<OverlayBlock>) -> OverlayMessage {
        OverlayMessage { blocks, seq, v: Self::VERSION }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("OverlayMessage her zaman serileştirilebilir")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tv_protokolu_ile_ayni_json_uretilir() {
        let message = OverlayMessage::new(
            123,
            vec![OverlayBlock { id: "abc123".into(), x: 0.12, y: 0.81, w: 0.6, h: 0.08, line_h: 0.03, text: "KAPI KİLİTLİ".into() }],
        );

        assert_eq!(
            message.to_json(),
            r#"{"blocks":[{"h":0.08,"id":"abc123","lineH":0.03,"text":"KAPI KİLİTLİ","w":0.6,"x":0.12,"y":0.81}],"seq":123,"v":1}"#
        );
    }

    #[test]
    fn bos_liste_gecerli_mesajdir() {
        assert_eq!(OverlayMessage::new(1, vec![]).to_json(), r#"{"blocks":[],"seq":1,"v":1}"#);
    }
}
