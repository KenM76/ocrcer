//! Compiles `model/feature_weights.tsv` into the `feature_weights` table.
//!
//! # What the table does
//!
//! The matcher's distance is a weighted sum over the 107 dimensions of
//! `ARCHITECTURE.md` section 3.1. Absent this table every weight is `1.0`, so
//! each dimension speaks with the same voice — and 103 of them describe the
//! glyph *after* it has been normalised onto a 32x32 grid, where a case pair
//! like `o`/`O` is the same shape. Only the four baseline-relative geometry
//! dimensions can separate such a pair, and at equal weight their four votes
//! are outvoted by 103 dimensions of shape noise.
//!
//! # Why the file is per block, with named dimensions on top
//!
//! A flat 107-row file would be 107 numbers nobody could justify one at a
//! time, which is what `CLAUDE.md` rule 1 forbids. The blocks are the units
//! the architecture already names, so a weight on a block is a claim about
//! what that block measures. But section 11's 2026-09-21 weighting entry
//! measured that one scalar over the geometry block is the wrong
//! parameterisation on its own, so a row may also name a single dimension
//! whose meaning section 3.1 states individually, and that row overrides its
//! block for that dimension alone.
//!
//! # File format
//!
//! Six tab-separated columns, `#` comments and blank lines ignored:
//!
//! ```text
//! key     weight  provenance  note
//! ```
//!
//! `key` is a section 3.1 block name or a nameable dimension; `weight` is a
//! non-negative float; `provenance` is `authored`, `measured` or `guess` and
//! is reported by `census` exactly as `params.tsv`'s is, so a weight that is
//! really a guess cannot read as a result.

use std::path::Path;

use ocrcer_core::feature::{
    CROSSINGS, FEATURE_DIMS, GEOMETRY, GRADIENT, HOLE_COUNT, H_PROJECTION, V_PROJECTION,
    ZONE_DENSITY,
};

use crate::ocrw::Table;
use crate::params::Provenance;

/// The name a `feature_weights.tsv` row uses, and the dimensions it covers.
///
/// Every dimension of `ARCHITECTURE.md` section 3.1 appears in exactly one
/// block, and `load` checks that: a file that names ten blocks but leaves the
/// eleventh out would silently ship `1.0` for the missing one.
const BLOCKS: [(&str, std::ops::Range<usize>); 7] = [
    ("zone_density", ZONE_DENSITY),
    ("gradient", GRADIENT),
    ("h_projection", H_PROJECTION),
    ("v_projection", V_PROJECTION),
    ("hole_count", HOLE_COUNT..HOLE_COUNT + 1),
    ("crossings", CROSSINGS),
    ("geometry", GEOMETRY),
];

/// The dimensions a row may name individually, overriding its block's weight.
///
/// Only the four baseline-relative dimensions are nameable, because they are
/// the only ones whose individual meaning is stated in section 3.1 and can
/// therefore carry a sentence of its own. Section 11's 2026-09-21 weighting
/// entry is why this exists at all: one scalar over the geometry block cannot
/// separate a pair that differs in height without also amplifying a pair that
/// does not, so the group's fix has to be reachable per dimension.
const DIMS: [(&str, usize); 4] = [
    ("geometry.aspect", GEOMETRY.start),
    ("geometry.ink_fraction", GEOMETRY.start + 1),
    ("geometry.height_above_baseline", GEOMETRY.start + 2),
    ("geometry.depth_below_baseline", GEOMETRY.start + 3),
];

/// One row of `feature_weights.tsv`: a block name or a single named dimension.
pub struct BlockWeight {
    pub key: String,
    pub weight: f32,
    pub provenance: Provenance,
    pub note: String,
}

/// Reads `model/feature_weights.tsv`.
///
/// Returns `Ok(None)` when the file is absent, which is a legitimate model:
/// the reader defaults every weight to `1.0` and the written file simply
/// carries no `feature_weights` table. A file that exists but is malformed is
/// an error, never a silent fallback to the default — a typo that quietly
/// reverted the matcher to uniform weights would cost accuracy with nothing
/// reporting it.
pub fn load(dir: &Path) -> Result<Option<Vec<BlockWeight>>, String> {
    let path = dir.join("feature_weights.tsv");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };

    let mut out: Vec<BlockWeight> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bad = |why: &str| format!("feature_weights.tsv line {}: {why}", n + 1);
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 4 {
            return Err(bad("expected four tab-separated columns"));
        }
        let key = cols[0].trim();
        let known = BLOCKS.iter().any(|(b, _)| *b == key) || DIMS.iter().any(|(d, _)| *d == key);
        if !known {
            return Err(bad(&format!(
                "{key:?} is neither a section 3.1 block name nor a nameable dimension"
            )));
        }
        if out.iter().any(|w| w.key == key) {
            return Err(bad(&format!("{key:?} appears twice")));
        }
        let weight: f32 = cols[1]
            .trim()
            .parse()
            .map_err(|_| bad(&format!("{:?} is not a float", cols[1])))?;
        if !weight.is_finite() || weight < 0.0 {
            return Err(bad("a weight must be finite and non-negative"));
        }
        let provenance = Provenance::parse(cols[2].trim())
            .ok_or_else(|| bad("provenance must be authored, measured or guess"))?;
        let note = cols[3].trim();
        if note.is_empty() {
            return Err(bad("a weight with no sentence behind it is a guess nobody can audit"));
        }
        out.push(BlockWeight {
            key: key.to_string(),
            weight,
            provenance,
            note: note.to_string(),
        });
    }

    for (b, _) in BLOCKS {
        if !out.iter().any(|w| w.key == b) {
            return Err(format!(
                "feature_weights.tsv names no weight for {b:?}; every block must be stated, \
                 including the ones left at 1.0, so a missing row cannot read as a default"
            ));
        }
    }
    Ok(Some(out))
}

/// Expands the rows into the per-dimension vector the table carries.
///
/// Blocks are applied first and named dimensions second regardless of the
/// order they appear in the file, so a dimension row always overrides the
/// block it belongs to and the file cannot mean two things depending on how
/// it is sorted.
pub fn expand(rows: &[BlockWeight]) -> [f32; FEATURE_DIMS] {
    let mut w = [1.0f32; FEATURE_DIMS];
    for row in rows {
        if let Some((_, range)) = BLOCKS.iter().find(|(b, _)| *b == row.key) {
            for d in range.clone() {
                w[d] = row.weight;
            }
        }
    }
    for row in rows {
        if let Some((_, d)) = DIMS.iter().find(|(n, _)| *n == row.key) {
            w[*d] = row.weight;
        }
    }
    w
}

/// Builds the table, or `None` when every block is at `1.0` — a table of ones
/// is exactly what the reader already defaults to, and omitting it keeps the
/// file honest about carrying no opinion.
pub fn build(rows: &[BlockWeight]) -> Option<Table> {
    let w = expand(rows);
    if w.iter().all(|v| *v == 1.0) {
        return None;
    }
    Some(Table::f32s(
        ocrcer_core::ocrw::T_FEATURE_WEIGHTS,
        vec![FEATURE_DIMS as u32],
        &w,
    ))
}

/// Counts rows by provenance, as `params::census` does, so a build can say
/// how many of its weights are measured and how many are still guesses.
pub fn census(rows: &[BlockWeight]) -> (usize, usize, usize) {
    let c = |p: Provenance| rows.iter().filter(|r| r.provenance == p).count();
    (c(Provenance::Authored), c(Provenance::Measured), c(Provenance::Guess))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blocks_partition_every_feature_dimension() {
        let mut seen = [0u8; FEATURE_DIMS];
        for (_, range) in BLOCKS {
            for d in range {
                seen[d] += 1;
            }
        }
        let missing: Vec<usize> = (0..FEATURE_DIMS).filter(|d| seen[*d] == 0).collect();
        let doubled: Vec<usize> = (0..FEATURE_DIMS).filter(|d| seen[*d] > 1).collect();
        assert!(missing.is_empty(), "no block covers dims {missing:?}");
        assert!(doubled.is_empty(), "two blocks cover dims {doubled:?}");
    }

    #[test]
    fn the_shipped_file_parses_and_covers_every_block() {
        let rows = load(&crate::tables::model_dir())
            .expect("feature_weights.tsv parses")
            .expect("feature_weights.tsv is present");
        for (b, _) in BLOCKS {
            assert!(rows.iter().any(|r| r.key == b), "no row for block {b:?}");
        }
    }

    #[test]
    fn a_weight_reaches_exactly_the_dimensions_its_block_names() {
        let rows = load(&crate::tables::model_dir()).unwrap().unwrap();
        let w = expand(&rows);
        let geom = rows.iter().find(|r| r.key == "geometry").unwrap().weight;
        for d in GEOMETRY {
            let named = DIMS.iter().find(|(_, i)| *i == d).map(|(n, _)| *n);
            let overridden = named.is_some_and(|n| rows.iter().any(|r| r.key == n));
            if !overridden {
                assert_eq!(w[d], geom, "dim {d} is in the geometry block");
            }
        }
        for d in ZONE_DENSITY {
            assert_ne!(d, 103, "sanity");
            assert!(w[d] > 0.0);
        }
    }

    #[test]
    fn a_named_dimension_overrides_its_block_and_nothing_else() {
        let mut rows: Vec<BlockWeight> = BLOCKS
            .iter()
            .map(|(b, _)| BlockWeight {
                key: b.to_string(),
                weight: 2.0,
                provenance: Provenance::Authored,
                note: "flat".into(),
            })
            .collect();
        rows.push(BlockWeight {
            key: "geometry.aspect".into(),
            weight: 9.0,
            provenance: Provenance::Measured,
            note: "aspect alone".into(),
        });
        let w = expand(&rows);
        assert_eq!(w[GEOMETRY.start], 9.0, "the named dimension takes its own weight");
        for d in GEOMETRY.start + 1..GEOMETRY.end {
            assert_eq!(w[d], 2.0, "dim {d} keeps the block weight");
        }
        for d in 0..GEOMETRY.start {
            assert_eq!(w[d], 2.0, "dim {d} is untouched by a geometry override");
        }
    }

    #[test]
    fn every_nameable_dimension_lies_inside_a_block() {
        for (name, d) in DIMS {
            assert!(d < FEATURE_DIMS, "{name} names dim {d}, past the vector");
            assert!(
                BLOCKS.iter().any(|(_, r)| r.contains(&d)),
                "{name} names dim {d}, which no block covers"
            );
        }
    }

    #[test]
    fn an_all_ones_file_writes_no_table() {
        let rows: Vec<BlockWeight> = BLOCKS
            .iter()
            .map(|(b, _)| BlockWeight {
                key: b.to_string(),
                weight: 1.0,
                provenance: Provenance::Authored,
                note: "flat".into(),
            })
            .collect();
        assert!(build(&rows).is_none());
    }
}
