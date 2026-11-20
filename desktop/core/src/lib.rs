//! Mac ve Windows masaüstü uygulamalarının paylaştığı, platformdan bağımsız mantık.
//!
//! Mac'teki Swift `PS5CeviriCore` paketinin karşılığıdır; testler de aynı senaryolarla oradan
//! taşındı. Burada async runtime, işletim sistemi API'si ya da ağ kodu yoktur: yakalama, OCR ve
//! HTTP platform katmanında yapılır, sonuçlar bu modüllere verilir.

pub mod block_merger;
pub mod context;
pub mod deepl;
pub mod focus;
pub mod frame_stats;
pub mod geometry;
pub mod glossary;
pub mod health;
pub mod language;
pub mod ocr_text;
pub mod opus_text;
pub mod post_edit;
pub mod profile;
pub mod protocol;
pub mod recovery;
pub mod resolve;
pub mod screen;
pub mod speaker;
pub mod stabilizer;
pub mod storage;
pub mod term_glossary;
pub mod terms;
pub mod translator;

pub use block_merger::{MergedBlock, TextLine};
pub use frame_stats::FrameStats;
pub use geometry::Rect;
pub use protocol::{OverlayBlock, OverlayMessage};
pub use health::{Diagnostics, Level, Stage};
pub use screen::{FrameInfo, ReadError, Reading, ScreenReader, SharedRegion};
pub use stabilizer::{CandidateInfo, CandidateState, StableBlock, Stabilizer, SubtitleStats};
pub use terms::Terms;
pub use translator::{Batch, Breaker, ErrorClass, TranslationError, Translator, TranslatorStats, Wanted};
