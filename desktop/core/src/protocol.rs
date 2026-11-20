//! Masaüstü → TV WebSocket mesajı. Android TV uygulamasındaki `TranslationMessage` ile birebir aynı.

use serde::{Deserialize, Serialize};

/// Koordinatlar 0...1, origin sol üst. Alanlar alfabetik sırada: JSON çıktısı Swift
/// sürümünün (sortedKeys) ürettiğiyle aynı olsun diye.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayBlock {
    pub h: f64,
    pub id: String,
    /// Yazının türü; şimdilik yalnızca `"subtitle"` (ekranın alt-ortasındaki konuşma). TV,
    /// altyazının önceki satırlarını üstünde küçük ve soluk tutar. Yoksa alan gönderilmez: eski
    /// TV uygulaması bilinmeyen alanı yok sayar, yeni TV alanı olmayan mesajda yığın kurmaz.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(rename = "lineH")]
    pub line_h: f64,
    /// Çeviri orijinal yazının altına/üstüne konduysa orijinal yazının üst kenarı (0...1). TV önceki
    /// altyazı satırlarını bunun da üstünden başlatır, orijinal altyazı kapanmaz. Yoksa gönderilmez.
    #[serde(rename = "srcY", default, skip_serializing_if = "Option::is_none")]
    pub src_y: Option<f64>,
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

impl OverlayBlock {
    /// `kind` değeri: altyazı.
    pub const SUBTITLE: &'static str = "subtitle";
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
            vec![OverlayBlock { id: "abc123".into(), x: 0.12, y: 0.81, w: 0.6, h: 0.08, line_h: 0.03, text: "KAPI KİLİTLİ".into(), kind: None, src_y: None }],
        );

        assert_eq!(
            message.to_json(),
            r#"{"blocks":[{"h":0.08,"id":"abc123","lineH":0.03,"text":"KAPI KİLİTLİ","w":0.6,"x":0.12,"y":0.81}],"seq":123,"v":1}"#
        );
    }

    #[test]
    fn altyazi_turu_yalnizca_varsa_gonderilir_ve_eski_mesaj_okunur() {
        let block = OverlayBlock { id: "s".into(), x: 0.3, y: 0.8, w: 0.4, h: 0.05, line_h: 0.04, text: "Gidelim.".into(), kind: Some(OverlayBlock::SUBTITLE.into()), src_y: None };
        assert_eq!(
            OverlayMessage::new(2, vec![block.clone()]).to_json(),
            r#"{"blocks":[{"h":0.05,"id":"s","kind":"subtitle","lineH":0.04,"text":"Gidelim.","w":0.4,"x":0.3,"y":0.8}],"seq":2,"v":1}"#
        );
        let below = OverlayBlock { src_y: Some(0.72), ..block };
        assert_eq!(
            OverlayMessage::new(3, vec![below]).to_json(),
            r#"{"blocks":[{"h":0.05,"id":"s","kind":"subtitle","lineH":0.04,"srcY":0.72,"text":"Gidelim.","w":0.4,"x":0.3,"y":0.8}],"seq":3,"v":1}"#
        );
        let old: OverlayMessage = serde_json::from_str(r#"{"blocks":[{"h":0.08,"id":"a","lineH":0.03,"text":"X","w":0.6,"x":0.1,"y":0.8}],"seq":1,"v":1}"#).unwrap();
        assert_eq!(old.blocks[0].kind, None);
        assert_eq!(old.blocks[0].src_y, None);
    }

    #[test]
    fn bos_liste_gecerli_mesajdir() {
        assert_eq!(OverlayMessage::new(1, vec![]).to_json(), r#"{"blocks":[],"seq":1,"v":1}"#);
    }
}
