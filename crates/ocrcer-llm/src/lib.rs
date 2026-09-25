//! `ocrcer-llm`: a pure-Rust, `std`-only Qwen decoder for rescoring OCR
//! output, per `ARCHITECTURE.md` section 11's 2026-09-24 entry.
//!
//! `#![forbid(unsafe_code)]` and zero dependencies outside `core`/`alloc`/
//! `std` — not a default for pdfcer, an optional add-on it can build without
//! pulling in the OCR engine's own dependency graph, so this crate does not
//! even depend on `ocrcer-core` (see `json.rs`'s doc comment). Chunk 16a's
//! scope is tokenizer-exact and logit-exact against the `transformers`
//! reference, plus the `.ocrl` format they load from; wiring this into OCR
//! rescoring is chunk 16b.

#![forbid(unsafe_code)]

pub mod config;
pub mod container;
pub mod json;
pub mod model;
pub mod tensor;
pub mod tokenizer;

pub use config::Config;
pub use container::Error;
pub use model::{KvCache, Model};
pub use tokenizer::Tokenizer;

/// Loads a `.ocrl` file's model and tokenizer together, since both are
/// always needed and both are read from the same container.
pub fn load(bytes: &[u8]) -> Result<(Model, Tokenizer), Error> {
    let c = container::Container::load(bytes)?;
    let config = Config::from_container(&c)?;
    let tokenizer = Tokenizer::from_container(&c)?;
    let model = Model::from_container(&c, config)?;
    Ok((model, tokenizer))
}
