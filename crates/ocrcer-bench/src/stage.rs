//! The single call-out to `ocrcer-core`'s feature extractor. Every other
//! module in this crate is agnostic to whether that extractor exists yet;
//! this is the one seam.
//!
//! `ocrcer-core::feature` is being written concurrently with this harness
//! (see `CLAUDE.md` rule 4 — the extractor is written exactly once, and
//! this crate calls it rather than reimplementing it). Until it lands, the
//! `extractor` Cargo feature is off, so this module returns a clear error
//! instead of a compile failure for the rest of the crate — `pbm`, `meta`,
//! `featurefile` and the bless-planning logic all have real unit-test
//! coverage regardless of extractor readiness.
//!
//! Once `crates/ocrcer-core/src/feature.rs` implements the documented
//! `GlyphInput` / `FEATURE_DIMS` / `extract`, build with
//! `--features extractor` (or flip the default on in `Cargo.toml`) to wire
//! this in for real.

#[cfg(feature = "extractor")]
mod real {
    pub use ocrcer_core::feature::{extract, GlyphInput, FEATURE_DIMS};
}

/// 107, per `ARCHITECTURE.md` section 3. Re-exported from `ocrcer-core`
/// when the `extractor` feature is on; hardcoded here (same value, by
/// contract) so fixture-format code has something to check dims against
/// even when the feature is off.
#[cfg(feature = "extractor")]
pub const FEATURE_DIMS: usize = real::FEATURE_DIMS;
#[cfg(not(feature = "extractor"))]
pub const FEATURE_DIMS: usize = 107;

/// Owned twin of `ocrcer_core::feature::GlyphInput`, so callers do not need
/// the `extractor` feature enabled just to construct a stage input.
pub struct GlyphStageInput {
    pub ink: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub baseline_dy: f32,
    pub x_height: f32,
}

/// Run the feature-extraction stage on one glyph. `Err` means the stage
/// itself could not run (extractor unavailable, or — if `ocrcer-core`
/// changes `extract`'s signature, e.g. to return a `Result` — a real
/// extraction failure); it is distinct from a fixture *mismatch*, which
/// `compare.rs` reports separately.
pub fn run_feature_stage(input: &GlyphStageInput) -> Result<Vec<f32>, String> {
    #[cfg(feature = "extractor")]
    {
        let gi = real::GlyphInput {
            ink: &input.ink,
            width: input.width,
            height: input.height,
            baseline_dy: input.baseline_dy,
            x_height: input.x_height,
        };
        let out: [f32; real::FEATURE_DIMS] = real::extract(&gi);
        Ok(out.to_vec())
    }
    #[cfg(not(feature = "extractor"))]
    {
        let _ = input;
        Err(
            "ocrcer-bench was built without the \"extractor\" feature: \
             ocrcer_core::feature::extract is not available yet. Build/test with \
             `--features extractor` once crates/ocrcer-core/src/feature.rs implements \
             GlyphInput, FEATURE_DIMS and extract() per ARCHITECTURE.md section 3.1."
                .to_string(),
        )
    }
}
