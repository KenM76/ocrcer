//! `ocrcer-bench`'s fixture harness. Never ships (see `CLAUDE.md` rule 4 —
//! this crate holds the correctness infrastructure, not a pipeline stage).
//!
//! Layout: `pbm`/`meta`/`featurefile` are the fixture file formats;
//! `fixture` discovers fixtures on disk; `stage` is the one seam that
//! calls into `ocrcer-core`; `compare` is exact, diagnostic comparison;
//! `provenance` names the build a number came from; `runner` ties those
//! together for `ocrcer-bench`'s own binary and for
//! the perturbation self-tests in `tests/`; `bless_logic` is the same for
//! the separate `bless` binary; `pages` (feature-gated) is the
//! oracle-segmented corpus reader the comparison binaries share. See
//! `fixtures/README.md` for the on-disk contract this code implements.

pub mod bless_logic;
pub mod cer;
pub mod compare;
pub mod decodefile;
pub mod featurefile;
pub mod fixture;
#[cfg(feature = "pages")]
pub mod ident;
#[cfg(feature = "pages")]
pub mod ident_corpus;
pub mod knobs;
pub mod meta;
pub mod pbm;
pub mod provenance;
#[cfg(feature = "pages")]
pub mod pages;
pub mod runner;
pub mod splits;
pub mod stage;

use std::path::PathBuf;

/// `fixtures/` at the repo root, resolved from this crate's manifest
/// directory so it works regardless of the caller's current directory.
pub fn default_fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// `bench/splits/` at the repo root, same reasoning as
/// [`default_fixtures_root`].
pub fn default_splits_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/splits")
}
