//! Minimal streaming primitives and websocket client helpers used by the
//! `deribit_trade_classifier` example.

mod engine;
mod source;
pub mod sources;

pub use engine::{Engine, EngineBuilder, EngineSource};
pub use source::{Source, Stream};
pub use source::{TimedBuffer, TimedEmitter};
