//! Çalışan iş hattı: ekran okuyucudan gelen satırları bloklara, sabit bloklara ve çevirilere
//! dönüştürüp TV'ye gönderir. Ağ kodu buradadır; `ceviri-core` saf kalır. İşletim sistemi API'si
//! kullanılmaz, Mac ve Windows'ta aynı çalışır. Her şey bloklayan (senkron) koddur ve ana thread
//! dışında, kendi worker thread'inde çalışır.

pub mod deepl_client;
pub mod discovery;
pub mod pipeline;
pub mod tv_client;

pub use deepl_client::DeepLClient;
pub use pipeline::{Config, Event, Pipeline, RoundStats};
pub use tv_client::TvClient;
