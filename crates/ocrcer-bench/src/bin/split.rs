//! `split`: (re-)derives `bench/splits/manifest.tsv` from the two committed
//! index files. `PLAN.md` chunk 12: "a small deterministic tool ... that
//! re-derives the manifest byte-identically."
//!
//! Usage:
//!   cargo run -p ocrcer-bench --bin split                # writes bench/splits/manifest.tsv
//!   cargo run -p ocrcer-bench --bin split -- --check      # writes nothing; fails if the
//!                                                          # committed file would change
//!
//! Inputs (all committed, all metadata-only -- no corpus text; see
//! `ocrcer_bench::splits` module doc):
//!   bench/splits/multifinben_index.tsv    -- every MultiFinBen row's shard, row
//!                                             number, text sha1 and char count
//!   bench/splits/finfilings_rows.tsv      -- the 60 (shard, row) pairs already
//!                                             spent on pages/finfilings
//!   bench/splits/multifinben_near_dup.tsv -- every candidate row's best word-
//!                                             5-gram-shingle containment against
//!                                             a finfilings score row (produced by
//!                                             tools/multifinben_near_dup.py)
//!
//! CORD-v2 and SROIE need no index file: their rows are enumerated from the
//! fixed per-shard row-count constants in `ocrcer_bench::splits` (their
//! entire official splits are used verbatim, no sampling).
//!
//! `--check` is what a fitting script's CI step runs before trusting the
//! manifest on disk: it recomputes the manifest from the same two inputs and
//! diffs the bytes, rather than asking anyone to remember to re-run this
//! binary after an index file changes.

use ocrcer_bench::splits;
use std::process::ExitCode;

fn main() -> ExitCode {
    let check = std::env::args().skip(1).any(|a| a == "--check");
    let root = ocrcer_bench::default_splits_root();

    let index_path = root.join("multifinben_index.tsv");
    let finfilings_path = root.join("finfilings_rows.tsv");
    let near_dup_path = root.join("multifinben_near_dup.tsv");
    let manifest_path = root.join("manifest.tsv");

    let index_text = match std::fs::read_to_string(&index_path) {
        Ok(t) => t,
        Err(e) => return fail(&format!("reading {}: {e}", index_path.display())),
    };
    let finfilings_text = match std::fs::read_to_string(&finfilings_path) {
        Ok(t) => t,
        Err(e) => return fail(&format!("reading {}: {e}", finfilings_path.display())),
    };
    let near_dup_text = match std::fs::read_to_string(&near_dup_path) {
        Ok(t) => t,
        Err(e) => return fail(&format!("reading {}: {e}", near_dup_path.display())),
    };

    let index = match splits::parse_index_tsv(&index_text) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let finfilings = match splits::parse_finfilings_rows(&finfilings_text) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let near_dup = match splits::parse_near_dup_tsv(&near_dup_text) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };

    let mut rows = splits::assign_multifinben(&index, &finfilings, &near_dup);
    rows.extend(splits::cord_rows());
    rows.extend(splits::sroie_rows());
    let tsv = splits::render_tsv(&rows);

    report(&rows);

    if check {
        return match std::fs::read_to_string(&manifest_path) {
            Ok(existing) if existing == tsv => {
                println!("split --check: {} matches the derivation byte-for-byte",
                          manifest_path.display());
                ExitCode::SUCCESS
            }
            Ok(_) => fail(&format!(
                "split --check: {} does NOT match a fresh derivation -- re-run \
                 without --check to update it, then review the diff",
                manifest_path.display()
            )),
            Err(e) => fail(&format!("reading {}: {e}", manifest_path.display())),
        };
    }

    if let Err(e) = std::fs::write(&manifest_path, &tsv) {
        return fail(&format!("writing {}: {e}", manifest_path.display()));
    }
    println!("wrote {}", manifest_path.display());
    ExitCode::SUCCESS
}

fn report(rows: &[splits::ManifestRow]) {
    use splits::Split::*;
    let mut counts: std::collections::BTreeMap<(String, &'static str), usize> =
        std::collections::BTreeMap::new();
    for r in rows {
        let label = match r.split {
            Train => "train",
            Validation => "validation",
            Score => "score",
            Excluded => "excluded",
        };
        *counts.entry((r.dataset.clone(), label)).or_default() += 1;
    }
    println!("split counts by dataset:");
    for ((dataset, label), n) in &counts {
        println!("  {dataset:<24} {label:<10} {n}");
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("split: {msg}");
    ExitCode::FAILURE
}
