//! `ocrcer-build`: renders glyphs, builds the prototype bank, compiles the
//! authored tables, writes `.ocrw`, and renders the evaluation corpus. This
//! crate never ships. It links `ocrcer-core` and calls its
//! `ocrcer_core::feature::extract` for glyph features rather than owning a
//! second implementation of the extractor (see `CLAUDE.md` rule 4).
//!
//! # Why there is a library here and not only a binary
//!
//! `ocrcer-bench` has to put OCRcer and another engine in front of the same
//! pixels. That means it needs the bank and the page renderer, and the one
//! thing it must not do is grow its own copy of either — the hazard
//! `CLAUDE.md` rule 4 names. So the build stages are a library and the
//! command-line tool is a thin caller of it.

pub mod bank;
pub mod bigrams;
pub mod confusions;
pub mod corpus;
pub mod emit;
pub mod face;
pub mod lexicon;
pub mod ocrw;
pub mod outline;
pub mod params;
pub mod page;
pub mod tables;
pub mod ttf_load;
pub mod weights;
