//! Mac ve Windows masaüstü uygulamalarının paylaştığı, platformdan bağımsız mantık.
//!
//! Mac'teki Swift `PS5CeviriCore` paketinin karşılığıdır; testler de aynı senaryolarla oradan
//! taşındı. Burada async runtime, işletim sistemi API'si ya da ağ kodu yoktur: yakalama, OCR ve
//! HTTP platform katmanında yapılır, sonuçlar bu modüllere verilir.

pub mod block_merger;
pub mod deepl;
pub mod frame_stats;
pub mod geometry;
pub mod language;
pub mod protocol;
pub mod screen;
pub mod stabilizer;
pub mod storage;
pub mod terms;
pub mod translator;

pub use block_merger::{MergedBlock, TextLine};
pub use frame_stats::FrameStats;
pub use geometry::Rect;
pub use protocol::{OverlayBlock, OverlayMessage};
pub use screen::{ReadError, Reading, ScreenReader};
pub use stabilizer::{StableBlock, Stabilizer};
pub use terms::Terms;
pub use translator::{TranslationError, Translator};
