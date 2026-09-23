//! Setting one named knob on a loaded `Engine`, including the pseudo-keys
//! that are not parameters.
//!
//! # Why a pseudo-key exists at all
//!
//! Most things a sweep wants to move are rows of `model/params.tsv` and go
//! through `Engine::set_param`. The feature weights are not: they are a
//! 107-entry table (`ARCHITECTURE.md` section 2's `feature_weights`), and a
//! harness that wants to ask "what would weighting the baseline-relative
//! block do?" has no parameter to move. `match.geometry_weight` is that
//! question in one number.
//!
//! # Contract
//!
//! [`set`] returns `false` for a name the engine refuses or a value it will
//! not take, and the caller is expected to treat that as an error rather than
//! carry on with the knob unset — a sweep that silently measured the same
//! configuration twice would report a flat curve and nobody would know why.
//!
//! It is written once, here, rather than in each binary: `tune` and `ocr`
//! measuring the same named knob through two code paths is how the two stop
//! agreeing about what the knob means.

use ocrcer_core::feature::{FEATURE_DIMS, GEOMETRY};
use ocrcer_core::pipeline::Engine;

/// The pseudo-key that weights the baseline-relative geometry block
/// (dims `103..107`) against every other dimension.
///
/// Not a parameter and not a shipped value: authored weights live in
/// `model/feature_weights.tsv` and compile into the optional
/// `feature_weights` table. A figure measured through this key is a
/// bench-harness measurement until it is authored there.
pub const GEOMETRY_WEIGHT: &str = "match.geometry_weight";

/// Whether `name` is a knob [`set`] can move on this engine.
pub fn known(engine: &Engine, name: &str) -> bool {
    name == GEOMETRY_WEIGHT || engine.params().get(name).is_some()
}

/// Sets one knob, returning `false` if the engine refused it.
pub fn set(engine: &mut Engine, name: &str, value: f32) -> bool {
    if name == GEOMETRY_WEIGHT {
        let mut w = [1.0f32; FEATURE_DIMS];
        w[GEOMETRY].iter_mut().for_each(|s| *s = value);
        return engine.set_feature_weights(&w);
    }
    engine.set_param(name, value)
}
