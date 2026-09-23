//! Fixture discovery for the `glyphs` stage. Adding a later stage (page
//! fixtures, chunk 2) means adding a sibling `discover_*` function and a
//! sibling match arm in `runner.rs` — this module does not need to change
//! shape to support that.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub struct GlyphFixture {
    pub name: String,
    pub pbm_path: PathBuf,
    pub meta_path: PathBuf,
    pub expected_path: PathBuf,
}

/// Every `*.{ext}` under `dir`, with `strip_suffix` (e.g. `.meta`,
/// `.feature`) removed from the file stem if present. A missing directory
/// yields no names rather than an error — one of the three directories a
/// fixture's files live in may legitimately be absent (a freshly deleted
/// `.pbm` still needs its name to surface from the other two, which is the
/// whole point: see `discover_glyph_fixtures`).
fn collect_stems(dir: &Path, ext: &str, strip_suffix: Option<&str>) -> Result<Vec<String>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("reading directory entry in {}: {e}", dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(ext) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let name = match strip_suffix {
            Some(suffix) => stem.strip_suffix(suffix).unwrap_or(stem),
            None => stem,
        };
        out.push(name.to_string());
    }
    Ok(out)
}

/// The union of names named by `fixtures/glyphs/*.pbm`,
/// `fixtures/glyphs/*.meta.json` and `fixtures/expected/glyphs/*.feature.json`
/// — deliberately a union, not just the `.pbm` list. A fixture whose
/// bitmap was deleted but whose expectation still exists (or vice versa)
/// must still be discovered by name, so the runner can report exactly
/// which file is missing, rather than the fixture silently vanishing from
/// the run and inflating the pass count. Sorted by name (via `BTreeSet`)
/// for a deterministic run order.
pub fn discover_glyph_fixtures(fixtures_root: &Path) -> Result<Vec<GlyphFixture>, String> {
    let glyphs_dir = fixtures_root.join("glyphs");
    let expected_dir = fixtures_root.join("expected").join("glyphs");

    if !glyphs_dir.is_dir() && !expected_dir.is_dir() {
        return Err(format!(
            "neither {} nor {} exists",
            glyphs_dir.display(),
            expected_dir.display()
        ));
    }

    let mut names: BTreeSet<String> = BTreeSet::new();
    for n in collect_stems(&glyphs_dir, "pbm", None)? {
        names.insert(n);
    }
    for n in collect_stems(&glyphs_dir, "json", Some(".meta"))? {
        names.insert(n);
    }
    for n in collect_stems(&expected_dir, "json", Some(".feature"))? {
        names.insert(n);
    }

    Ok(names
        .into_iter()
        .map(|name| GlyphFixture {
            pbm_path: glyphs_dir.join(format!("{name}.pbm")),
            meta_path: glyphs_dir.join(format!("{name}.meta.json")),
            expected_path: expected_dir.join(format!("{name}.feature.json")),
            name,
        })
        .collect())
}
