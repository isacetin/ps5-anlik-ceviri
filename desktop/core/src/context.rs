//! Çeviri bağlamı: az önce ekranda geçen konuşma replikleri ve oyun profilinin kısa ipucu. DeepL
//! bunu `context` alanında alır (çevrilmez, kotadan düşmez); yerel model kullanmaz.
//!
//! Kurallar:
//! - Sınırlıdır: en çok `max_lines` replik ve `max_chars` karakter; eskiler önden düşer.
//! - Yalnızca konuşma (altyazı/diyalog) ve cümle gibi duran metin girer; menü öğeleri sonraki
//!   repliğin çevirisini yanlış yöne çekebilir.
//! - Sahne değişince (pencere kayboldu, uyku, okuyucu yenilendi, profil değişti) sıfırlanır; eski
//!   konuşma yeni sahnenin çevirisini etkilemez.

use std::collections::VecDeque;

/// Varsayılan sınırlar (eski DeepL istemcisindekiyle aynı).
pub const DEFAULT_LINES: usize = 4;
pub const DEFAULT_CHARS: usize = 600;
/// Profil ipucunun en fazla uzunluğu.
const MAX_HINT_CHARS: usize = 200;

#[derive(Clone, Debug)]
pub struct ContextWindow {
    lines: VecDeque<String>,
    max_lines: usize,
    max_chars: usize,
    hint: String,
    enabled: bool,
    resets: u64,
}

impl Default for ContextWindow {
    fn default() -> ContextWindow {
        ContextWindow::new(DEFAULT_LINES, DEFAULT_CHARS)
    }
}

impl ContextWindow {
    pub fn new(max_lines: usize, max_chars: usize) -> ContextWindow {
        ContextWindow { lines: VecDeque::new(), max_lines: max_lines.min(10), max_chars: max_chars.min(1500), hint: String::new(), enabled: true, resets: 0 }
    }

    /// Bağlam hiç gönderilmez (yerel model ya da profil ayarı).
    pub fn disabled() -> ContextWindow {
        ContextWindow { enabled: false, ..ContextWindow::default() }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Oyun ipucu ("Video game: Elden Ring (action RPG).") her bağlamın başına eklenir; kısaltılır.
    pub fn set_hint(&mut self, hint: &str) {
        self.hint = hint.chars().take(MAX_HINT_CHARS).collect::<String>().trim().to_string();
    }

    /// Cümle gibi duran metin mi (en az üç kelime).
    pub fn is_worthy(text: &str) -> bool {
        text.split_whitespace().count() >= 3
    }

    /// Çevrilen bir konuşma repliğini (düz metin) hatırlar.
    pub fn remember(&mut self, plain: &str) {
        if !self.enabled || !ContextWindow::is_worthy(plain) || self.lines.iter().any(|l| l == plain) {
            return;
        }
        self.lines.push_back(plain.to_string());
        while self.lines.len() > self.max_lines || (self.chars() > self.max_chars && self.lines.len() > 1) {
            self.lines.pop_front();
        }
        // Tek replik bile sınırı aşıyorsa sonu kesilir (bellek ve istek boyu sınırlı kalsın).
        if self.chars() > self.max_chars {
            if let Some(line) = self.lines.back_mut() {
                *line = line.chars().rev().take(self.max_chars).collect::<Vec<_>>().into_iter().rev().collect();
            }
        }
    }

    fn chars(&self) -> usize {
        self.lines.iter().map(|t| t.chars().count()).sum()
    }

    /// İsteğe eklenecek bağlam; kapalıysa ya da boşsa "".
    pub fn text(&self) -> String {
        if !self.enabled {
            return String::new();
        }
        let recent = self.lines.iter().cloned().collect::<Vec<_>>().join(" ");
        [self.hint.as_str(), recent.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ")
    }

    /// Konuşma geçmişi var mı (ipucu sayılmaz). Kısa metnin çevirisi buna bağlı olabilir.
    pub fn has_history(&self) -> bool {
        self.enabled && !self.lines.is_empty()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Sahne değişti: geçmiş silinir, ipucu kalır.
    pub fn reset(&mut self) {
        if !self.lines.is_empty() {
            self.resets += 1;
        }
        self.lines.clear();
    }

    pub fn resets(&self) -> u64 {
        self.resets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yalnizca_cumle_gibi_metin_baglama_girer() {
        let mut context = ContextWindow::default();
        context.remember("Are you ready to leave?");
        context.remember("New Game");
        context.remember("Are you ready to leave?");

        assert_eq!(context.text(), "Are you ready to leave?");
        assert_eq!(context.len(), 1);
    }

    #[test]
    fn baglam_sinirli_kalir() {
        let mut context = ContextWindow::new(4, 600);
        for i in 0..20 {
            context.remember(&format!("This is dialogue line {i}"));
        }
        assert_eq!(context.len(), 4);
        assert!(context.text().starts_with("This is dialogue line 16"));

        let mut small = ContextWindow::new(4, 50);
        small.remember(&"word ".repeat(40));
        assert!(small.text().chars().count() <= 50);
        // Aşırı ayarlar sınırlanır.
        assert_eq!(ContextWindow::new(1000, 100_000).max_lines, 10);
    }

    #[test]
    fn sahne_degisince_sifirlanir_ipucu_kalir() {
        let mut context = ContextWindow::default();
        context.set_hint("Video game: Elden Ring.");
        context.remember("The ancient kingdom fell.");
        assert_eq!(context.text(), "Video game: Elden Ring. The ancient kingdom fell.");
        assert!(context.has_history());

        context.reset();

        assert_eq!(context.text(), "Video game: Elden Ring.");
        assert!(!context.has_history());
        assert_eq!(context.resets(), 1);
    }

    #[test]
    fn kapali_baglam_bos_doner() {
        let mut context = ContextWindow::disabled();
        context.set_hint("Video game: X.");
        context.remember("Are you ready to leave?");
        assert_eq!(context.text(), "");
        assert!(!context.has_history());
    }
}
