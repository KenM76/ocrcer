//! `<name>.meta.json`: the per-line numbers a glyph fixture needs besides
//! its bitmap, plus which character it is (for a reviewer, not consumed by
//! the extractor). Parsed with `serde_json`; these are hand-authored,
//! simple decimal values, so `serde_json`'s f64-based float path never
//! loses precision on them (unlike the machine-generated feature vectors —
//! see `featurefile.rs` for why those use a different path).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlyphMeta {
    /// The character this bitmap depicts, for a reviewer's benefit.
    pub character: String,
    /// Baseline, in pixels, measured downward from the bitmap's top edge.
    pub baseline_dy: f32,
    /// The line's x-height in pixels, strictly positive.
    pub x_height: f32,
}

pub fn read(path: &Path) -> Result<GlyphMeta, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("parsing {}: {e}", path.display()))
}

pub fn write(path: &Path, meta: &GlyphMeta) -> Result<(), String> {
    let text = serde_json::to_string_pretty(meta)
        .map_err(|e| format!("serialising {}: {e}", path.display()))?;
    fs::write(path, text + "\n").map_err(|e| format!("writing {}: {e}", path.display()))
}
