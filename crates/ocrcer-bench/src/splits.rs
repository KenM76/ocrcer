//! The training/scoring split manifest (`PLAN.md` chunk 12; the firewall in
//! `PLAN.md` section 2c, amended 2026-09-24; `CLAUDE.md` rule 1).
//!
//! # What this module is for
//!
//! Every corpus row OCRcer could ever touch gets exactly one label: `train`,
//! `validation`, `score`, or `excluded`. The label is fixed once, in the
//! committed `bench/splits/manifest.tsv`, and nothing downstream is allowed
//! to move a row across that line. [`assert_not_score`] is the enforcement
//! point a fitting script calls before it reads anything; everything else
//! here is how the manifest gets built in the first place.
//!
//! # Where the fourth split comes from
//!
//! The chunk row and `PLAN.md` section 2c both describe three splits --
//! `train | validation | score`. This module adds a fourth, `excluded`, and
//! says so here rather than silently dropping rows: a row that is neither
//! sampled nor scored still needs a recorded reason, or "why isn't this row
//! anywhere" is a question nobody committed an answer to. An `excluded` row
//! never appears in `train`, `validation`, or `score` for the same
//! (dataset, row_id) pair -- the split is a partition, not a subset.
//!
//! # Determinism, and what it rests on
//!
//! [`sample_fraction`] is [`fnv1a64`] of `"{dataset}\x1f{row_id}"`, mapped to
//! `[0, 1)`. No RNG, no seed, no external state -- the same row id always
//! lands at the same fraction, forever, on any machine. FNV-1a rather than a
//! cryptographic hash because nothing here needs collision resistance, only
//! a reproducible spread; `CLAUDE.md` rule 3 also makes a dependency-free
//! hash the cheaper choice for a crate that need not carry one otherwise.
//!
//! CORD-v2 and SROIE need no such sampling: this project uses their entire
//! official splits verbatim (`PLAN.md` section 2c, chunk 12 instructions),
//! so [`cord_rows`] and [`sroie_rows`] just enumerate every row of every
//! shard the dataset ships. The per-shard row counts are constants read
//! directly from each shard's Parquet metadata on 2026-09-24 (`pyarrow`'s
//! `ParquetFile(...).metadata.num_rows`); they describe an immutable,
//! versioned, already-downloaded file and are not expected to change under
//! this project. If a shard is ever replaced, these constants -- and the
//! comment recording how they were read -- are the thing to re-derive.

use std::collections::{HashMap, HashSet};
use std::fmt;

/// Where a row sits, permanently, once the manifest is committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Split {
    Train,
    Validation,
    Score,
    /// Considered and rejected -- a near-duplicate of a scoring row, or too
    /// short to have been rendered in the first place. Recorded rather than
    /// silently omitted; see the module doc.
    Excluded,
}

impl fmt::Display for Split {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Split::Train => "train",
            Split::Validation => "validation",
            Split::Score => "score",
            Split::Excluded => "excluded",
        })
    }
}

impl std::str::FromStr for Split {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "train" => Ok(Split::Train),
            "validation" => Ok(Split::Validation),
            "score" => Ok(Split::Score),
            "excluded" => Ok(Split::Excluded),
            other => Err(format!("not a split: {other:?}")),
        }
    }
}

/// One line of the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestRow {
    pub dataset: String,
    pub row_id: String,
    pub split: Split,
    pub licence: String,
    pub reason: String,
}

/// FNV-1a, 64-bit. Public domain algorithm, no crate needed
/// (<http://www.isthe.com/chongo/tech/comp/fnv/>).
///
/// **Not used alone** -- see [`sample_fraction`]. FNV-1a's own avalanche is
/// weak exactly on the input shape this module hashes: a long shared prefix
/// (`"{dataset}\x1f{shard}#"`) followed by a handful of decimal digits that
/// differ from one row id to the next. Measured directly: `row=0` through
/// `row=19` of the same shard produced 20 *distinct* 64-bit values, but all
/// twenty, read as a fraction of `u64::MAX`, rounded to the same six decimal
/// digits, because FNV-1a folds each new byte into the *low* bits before one
/// more multiply, so a change confined to the string's last few bytes barely
/// moves the *high* bits that dominate the value used as a fraction. The
/// symptom in a first cut of this function was silent and specific: every
/// row of a shard landed in the same ~0.005-wide band, so an entire shard
/// sampled as a block -- 396 rows to `train`, and `validation` empty, not
/// merely short of 100. [`fmix64`] is applied to the FNV-1a output before
/// use for exactly this reason.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// MurmurHash3's 64-bit finalizer (Austin Appleby, public domain). Spreads
/// every input bit across every output bit, which is what [`fnv1a64`]'s own
/// doc comment explains this module cannot do without it.
fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51afd7ed558ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ceb9fe1a85ec53);
    k ^= k >> 33;
    k
}

/// A row id's position in `[0, 1)`, deterministic and RNG-free. See the
/// module doc for why a hash rather than an RNG, and [`fnv1a64`]'s doc for
/// why its output is passed through [`fmix64`] rather than used directly.
pub fn sample_fraction(dataset: &str, row_id: &str) -> f64 {
    let key = format!("{dataset}\u{1f}{row_id}");
    fmix64(fnv1a64(key.as_bytes())) as f64 / u64::MAX as f64
}

/// Target train fraction of the post-exclusion MultiFinBen candidate pool
/// (7,654 rows measured 2026-09-24), chosen so the expected sample is
/// approximately 400 rows: `400 / 7654 ≈ 0.0523`. The realised count is
/// reported by `bin/split`, not assumed to hit the target exactly -- a
/// hash-based sample has sampling variance like any other, and "about 400"
/// is what `PLAN.md` chunk 12 asks for.
pub const MULTIFINBEN_TRAIN_FRAC: f64 = 0.0523;
/// Target validation fraction, `100 / 7654 ≈ 0.0131`, in the same slice of
/// `[0, 1)` immediately above the train fraction (see [`assign_multifinben`]).
pub const MULTIFINBEN_VAL_FRAC: f64 = 0.0131;

pub const MULTIFINBEN_DATASET: &str = "multifinben-englishocr";
pub const MULTIFINBEN_LICENCE: &str = "Apache-2.0";
const MULTIFINBEN_MIN_CHARS: u32 = 200;

/// One parsed line of `multifinben_index.tsv`: `shard, row, sha1, chars`.
#[derive(Debug, Clone)]
pub struct IndexRow {
    pub shard: String,
    pub row: u32,
    pub sha1: String,
    pub chars: u32,
}

/// Parses `multifinben_index.tsv` (or `finfilings_rows.tsv`'s two-column
/// prefix of it): a `#`-commented header, then TSV data lines. Blank lines
/// are skipped; anything else that fails to parse is a hard error, because a
/// silently-dropped row would change the sample without saying so.
pub fn parse_index_tsv(text: &str) -> Result<Vec<IndexRow>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 4 {
            return Err(format!("multifinben_index.tsv line {}: expected 4 columns, got {}",
                                n + 1, cols.len()));
        }
        let row = cols[1].parse::<u32>()
            .map_err(|e| format!("multifinben_index.tsv line {}: bad row number: {e}", n + 1))?;
        let chars = cols[3].parse::<u32>()
            .map_err(|e| format!("multifinben_index.tsv line {}: bad char count: {e}", n + 1))?;
        out.push(IndexRow { shard: cols[0].to_string(), row, sha1: cols[2].to_string(), chars });
    }
    Ok(out)
}

/// Parses `finfilings_rows.tsv`: `#`-commented header, then `shard, row`.
pub fn parse_finfilings_rows(text: &str) -> Result<Vec<(String, u32)>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 2 {
            return Err(format!("finfilings_rows.tsv line {}: expected 2 columns, got {}",
                                n + 1, cols.len()));
        }
        let row = cols[1].parse::<u32>()
            .map_err(|e| format!("finfilings_rows.tsv line {}: bad row number: {e}", n + 1))?;
        out.push((cols[0].to_string(), row));
    }
    Ok(out)
}

fn row_id(shard: &str, row: u32) -> String {
    format!("{shard}#{row:06}")
}

/// Builds every MultiFinBen manifest row: the 60 permanent `score` rows,
/// the rows excluded as duplicates of one of those 60 or as too short, and
/// a deterministic `train`/`validation` sample of what remains. Order is
/// index order (shard, then row), which is why re-running this against the
/// same two input files always emits the same manifest bytes.
///
/// `index` is the full row population (`multifinben_index.tsv`);
/// `finfilings` is the 60 `(shard, row)` pairs already spent on
/// `pages/finfilings` (`finfilings_rows.tsv`).
pub fn assign_multifinben(index: &[IndexRow], finfilings: &[(String, u32)]) -> Vec<ManifestRow> {
    let score_set: HashSet<(&str, u32)> =
        finfilings.iter().map(|(s, r)| (s.as_str(), *r)).collect();

    // sha1 of every finfilings row's text, looked up by (shard, row) in the
    // index -- this is the "same source document" proxy the dataset's own
    // schema cannot support directly (no document-id column; see module doc
    // and the README section this feeds).
    let by_key: HashMap<(&str, u32), &IndexRow> =
        index.iter().map(|r| ((r.shard.as_str(), r.row), r)).collect();
    let score_hashes: HashSet<&str> = finfilings
        .iter()
        .filter_map(|(s, r)| by_key.get(&(s.as_str(), *r)).map(|ix| ix.sha1.as_str()))
        .collect();

    let mut out = Vec::with_capacity(index.len());
    for ix in index {
        let id = row_id(&ix.shard, ix.row);
        if score_set.contains(&(ix.shard.as_str(), ix.row)) {
            out.push(ManifestRow {
                dataset: MULTIFINBEN_DATASET.into(),
                row_id: id,
                split: Split::Score,
                licence: MULTIFINBEN_LICENCE.into(),
                reason: "pages/finfilings scoring corpus; permanent (PLAN.md section 2c)".into(),
            });
            continue;
        }
        if score_hashes.contains(ix.sha1.as_str()) {
            out.push(ManifestRow {
                dataset: MULTIFINBEN_DATASET.into(),
                row_id: id,
                split: Split::Excluded,
                licence: MULTIFINBEN_LICENCE.into(),
                reason: format!(
                    "identical transcript text (sha1 {}) to a pages/finfilings score row; \
                     treated as the same source document -- the dataset carries no \
                     document-id column to check directly",
                    ix.sha1
                ),
            });
            continue;
        }
        if ix.chars < MULTIFINBEN_MIN_CHARS {
            out.push(ManifestRow {
                dataset: MULTIFINBEN_DATASET.into(),
                row_id: id,
                split: Split::Excluded,
                licence: MULTIFINBEN_LICENCE.into(),
                reason: format!(
                    "{} chars after whitespace normalisation, below the {}-char \
                     corpus min-chars floor (parquet_corpus.py's own --min-chars default)",
                    ix.chars, MULTIFINBEN_MIN_CHARS
                ),
            });
            continue;
        }
        let frac = sample_fraction(MULTIFINBEN_DATASET, &id);
        if frac < MULTIFINBEN_TRAIN_FRAC {
            out.push(ManifestRow {
                dataset: MULTIFINBEN_DATASET.into(),
                row_id: id,
                split: Split::Train,
                licence: MULTIFINBEN_LICENCE.into(),
                reason: "deterministic hash-of-row-id sample of the post-exclusion \
                          candidate pool, target ~400 rows"
                    .into(),
            });
        } else if frac < MULTIFINBEN_TRAIN_FRAC + MULTIFINBEN_VAL_FRAC {
            out.push(ManifestRow {
                dataset: MULTIFINBEN_DATASET.into(),
                row_id: id,
                split: Split::Validation,
                licence: MULTIFINBEN_LICENCE.into(),
                reason: "deterministic hash-of-row-id sample of the post-exclusion \
                          candidate pool, target ~100 rows"
                    .into(),
            });
        }
        // Otherwise: a candidate row this round simply did not sample. It is
        // not listed -- the manifest records assignments, not the entire
        // unused pool. README states the unlisted-candidate count.
    }
    out
}

/// One CORD-v2/SROIE shard: filename and its row count, as read from
/// `ParquetFile(...).metadata.num_rows` on 2026-09-24 (module doc).
struct Shard {
    file: &'static str,
    rows: u32,
}

const CORD_TRAIN_SHARDS: &[Shard] = &[
    Shard { file: "train-00000-of-00004-b4aaeceff1d90ecb.parquet", rows: 200 },
    Shard { file: "train-00001-of-00004-7dbbe248962764c5.parquet", rows: 200 },
    Shard { file: "train-00002-of-00004-688fe1305a55e5cc.parquet", rows: 200 },
    Shard { file: "train-00003-of-00004-2d0cd200555ed7fd.parquet", rows: 200 },
];
const CORD_VALIDATION_SHARD: Shard =
    Shard { file: "validation-00000-of-00001-cc3c5779fe22e8ca.parquet", rows: 100 };
const CORD_TEST_SHARD: Shard =
    Shard { file: "test-00000-of-00001-9c204eb3f4e11791.parquet", rows: 100 };

pub const CORD_DATASET: &str = "cord-v2";
pub const CORD_LICENCE: &str = "CC-BY-4.0";

/// Every CORD-v2 row, split exactly as CORD-v2 itself splits it: its train
/// shards are `train`, its validation shard is `validation`, and its test
/// shard -- all 100 rows, a superset of the 95-row sample already rendered
/// to `pages/cord` -- is `score` (`PLAN.md` chunk 12 instructions: "Their
/// official test splits ... are score").
pub fn cord_rows() -> Vec<ManifestRow> {
    let mut out = Vec::new();
    for shard in CORD_TRAIN_SHARDS {
        push_shard(&mut out, CORD_DATASET, CORD_LICENCE, shard, Split::Train,
                   "CORD-v2 official train split");
    }
    push_shard(&mut out, CORD_DATASET, CORD_LICENCE, &CORD_VALIDATION_SHARD, Split::Validation,
               "CORD-v2 official validation split");
    push_shard(&mut out, CORD_DATASET, CORD_LICENCE, &CORD_TEST_SHARD, Split::Score,
               "CORD-v2 official test split; pages/cord rendered a 95-row sample of it");
    out
}

const SROIE_TRAIN_SHARD: Shard = Shard { file: "train-00000-of-00001.parquet", rows: 626 };
const SROIE_TEST_SHARD: Shard = Shard { file: "test-00000-of-00001.parquet", rows: 361 };

pub const SROIE_DATASET: &str = "sroie";
pub const SROIE_LICENCE: &str = "CC-BY-4.0";

/// Every SROIE row. SROIE ships no official validation split -- only
/// `train` and `test` -- so this manifest carries none either; the task's
/// "per their own official splits" governs. Test (all 361 rows, a superset
/// of the 120-row sample already rendered to `pages/sroie`) is `score`.
pub fn sroie_rows() -> Vec<ManifestRow> {
    let mut out = Vec::new();
    push_shard(&mut out, SROIE_DATASET, SROIE_LICENCE, &SROIE_TRAIN_SHARD, Split::Train,
               "SROIE official train split (no official validation split exists)");
    push_shard(&mut out, SROIE_DATASET, SROIE_LICENCE, &SROIE_TEST_SHARD, Split::Score,
               "SROIE official test split; pages/sroie rendered a 120-row sample of it");
    out
}

fn push_shard(out: &mut Vec<ManifestRow>, dataset: &str, licence: &str, shard: &Shard,
              split: Split, reason: &str) {
    for row in 0..shard.rows {
        out.push(ManifestRow {
            dataset: dataset.into(),
            row_id: row_id(shard.file, row),
            split,
            licence: licence.into(),
            reason: reason.into(),
        });
    }
}

/// Renders a manifest as the committed TSV, in the fixed column order
/// `dataset, row_id, split, licence, reason`. Deterministic in the rows'
/// given order -- callers that need a stable overall order build one before
/// calling this.
pub fn render_tsv(rows: &[ManifestRow]) -> String {
    let mut out = String::from("#dataset\trow_id\tsplit\tlicence\treason\n");
    for r in rows {
        out.push_str(&format!("{}\t{}\t{}\t{}\t{}\n", r.dataset, r.row_id, r.split, r.licence,
                               r.reason));
    }
    out
}

/// Parses a committed `manifest.tsv` back into rows. The inverse of
/// [`render_tsv`] modulo trailing whitespace.
pub fn parse_manifest_tsv(text: &str) -> Result<Vec<ManifestRow>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.splitn(5, '\t').collect();
        if cols.len() != 5 {
            return Err(format!("manifest.tsv line {}: expected 5 columns, got {}",
                                n + 1, cols.len()));
        }
        let split = cols[2].parse::<Split>()
            .map_err(|e| format!("manifest.tsv line {}: {e}", n + 1))?;
        out.push(ManifestRow {
            dataset: cols[0].to_string(),
            row_id: cols[1].to_string(),
            split,
            licence: cols[3].to_string(),
            reason: cols[4].to_string(),
        });
    }
    Ok(out)
}

/// The check a fitting script runs before it touches anything
/// (`CLAUDE.md` rule 1: "Splits are fixed in a committed manifest before any
/// fitting runs"). `Err` means "do not fit on this row" -- a `score` row,
/// or a row the manifest has no opinion about at all, since an unlisted row
/// is the *shape* of the failure this whole module exists to prevent: a
/// script that reads whatever is on disk rather than what the manifest
/// permits. Only an explicit `train` or `validation` row passes.
pub fn assert_fittable(manifest: &[ManifestRow], dataset: &str, row_id: &str) -> Result<(), String> {
    match manifest.iter().find(|r| r.dataset == dataset && r.row_id == row_id) {
        None => Err(format!(
            "{dataset}:{row_id} is not in the split manifest -- refusing to fit on an \
             unlisted row (CLAUDE.md rule 1: nothing is fitted until a committed split \
             manifest names it train or validation)"
        )),
        Some(r) if r.split == Split::Train || r.split == Split::Validation => Ok(()),
        Some(r) => Err(format!(
            "{dataset}:{row_id} is a {} row -- refusing to fit on it (PLAN.md section 2c \
             firewall: a corpus used to score the engine must never contribute a value to \
             the model)",
            r.split
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression for the avalanche bug the `fnv1a64` doc comment describes:
    /// consecutive row ids sharing a shard prefix must not collapse onto the
    /// same fraction bucket. Caught originally because a real 8-shard,
    /// ~1000-row-per-shard run sampled zero validation rows against a target
    /// of ~100.
    #[test]
    fn consecutive_row_ids_in_one_shard_spread_across_many_buckets() {
        let shard = "train-00000-of-00008.parquet";
        let mut buckets = HashSet::new();
        for row in 0..200u32 {
            let frac = sample_fraction("multifinben-englishocr", &row_id(shard, row));
            buckets.insert((frac * 200.0) as u32); // 200 buckets across [0, 1)
        }
        assert!(
            buckets.len() > 50,
            "200 consecutive row ids landed in only {} of 200 buckets -- \
             the hash is not avalanching on the varying suffix",
            buckets.len()
        );
    }

    #[test]
    fn sample_fraction_is_deterministic_and_in_range() {
        let a = sample_fraction("multifinben-englishocr", "train-00003-of-00008.parquet#000512");
        let b = sample_fraction("multifinben-englishocr", "train-00003-of-00008.parquet#000512");
        assert_eq!(a, b);
        assert!((0.0..1.0).contains(&a));
        // A different row id lands somewhere else -- not a proof of good
        // spread, just a guard against a constant-function bug.
        let c = sample_fraction("multifinben-englishocr", "train-00003-of-00008.parquet#000513");
        assert_ne!(a, c);
    }

    #[test]
    fn finfilings_rows_are_always_score_never_excluded_or_sampled() {
        let index = vec![
            IndexRow { shard: "s.parquet".into(), row: 0, sha1: "aaa".into(), chars: 500 },
            IndexRow { shard: "s.parquet".into(), row: 1, sha1: "bbb".into(), chars: 500 },
        ];
        let finfilings = vec![("s.parquet".to_string(), 0u32)];
        let rows = assign_multifinben(&index, &finfilings);
        let r0 = rows.iter().find(|r| r.row_id == row_id("s.parquet", 0)).unwrap();
        assert_eq!(r0.split, Split::Score);
    }

    #[test]
    fn a_row_matching_a_score_rows_text_is_excluded_not_sampled() {
        let index = vec![
            IndexRow { shard: "s.parquet".into(), row: 0, sha1: "same".into(), chars: 500 },
            IndexRow { shard: "s.parquet".into(), row: 1, sha1: "same".into(), chars: 500 },
        ];
        let finfilings = vec![("s.parquet".to_string(), 0u32)];
        let rows = assign_multifinben(&index, &finfilings);
        let r1 = rows.iter().find(|r| r.row_id == row_id("s.parquet", 1)).unwrap();
        assert_eq!(r1.split, Split::Excluded);
        assert!(r1.reason.contains("same source document") || r1.reason.contains("sha1"));
    }

    #[test]
    fn a_too_short_row_is_excluded() {
        let index = vec![
            IndexRow { shard: "s.parquet".into(), row: 0, sha1: "aaa".into(), chars: 500 },
            IndexRow { shard: "s.parquet".into(), row: 1, sha1: "bbb".into(), chars: 50 },
        ];
        let rows = assign_multifinben(&index, &[]);
        let r1 = rows.iter().find(|r| r.row_id == row_id("s.parquet", 1)).unwrap();
        assert_eq!(r1.split, Split::Excluded);
    }

    #[test]
    fn assign_multifinben_never_emits_two_splits_for_one_row() {
        // Build a moderately sized synthetic index and confirm the
        // partition property: each row id appears at most once.
        let index: Vec<IndexRow> = (0..500)
            .map(|i| IndexRow {
                shard: "s.parquet".into(),
                row: i,
                sha1: format!("h{i}"),
                chars: 500,
            })
            .collect();
        let finfilings: Vec<(String, u32)> =
            (0..10).map(|i| ("s.parquet".to_string(), i * 10)).collect();
        let rows = assign_multifinben(&index, &finfilings);
        let mut seen = HashSet::new();
        for r in &rows {
            assert!(seen.insert(r.row_id.clone()), "duplicate row_id {}", r.row_id);
        }
    }

    #[test]
    fn manifest_tsv_round_trips() {
        let rows = vec![ManifestRow {
            dataset: "d".into(),
            row_id: "s.parquet#000001".into(),
            split: Split::Train,
            licence: "Apache-2.0".into(),
            reason: "a reason with no tabs".into(),
        }];
        let tsv = render_tsv(&rows);
        let back = parse_manifest_tsv(&tsv).unwrap();
        assert_eq!(rows, back);
    }

    #[test]
    fn assert_fittable_refuses_score_and_unlisted_rows() {
        let rows = vec![
            ManifestRow {
                dataset: "d".into(), row_id: "a".into(), split: Split::Score,
                licence: "X".into(), reason: "r".into(),
            },
            ManifestRow {
                dataset: "d".into(), row_id: "b".into(), split: Split::Train,
                licence: "X".into(), reason: "r".into(),
            },
        ];
        assert!(assert_fittable(&rows, "d", "a").is_err());
        assert!(assert_fittable(&rows, "d", "b").is_ok());
        assert!(assert_fittable(&rows, "d", "not-listed").is_err());
    }

    #[test]
    fn cord_and_sroie_row_counts_match_their_official_split_sizes() {
        let cord = cord_rows();
        assert_eq!(cord.iter().filter(|r| r.split == Split::Train).count(), 800);
        assert_eq!(cord.iter().filter(|r| r.split == Split::Validation).count(), 100);
        assert_eq!(cord.iter().filter(|r| r.split == Split::Score).count(), 100);

        let sroie = sroie_rows();
        assert_eq!(sroie.iter().filter(|r| r.split == Split::Train).count(), 626);
        assert_eq!(sroie.iter().filter(|r| r.split == Split::Validation).count(), 0);
        assert_eq!(sroie.iter().filter(|r| r.split == Split::Score).count(), 361);
    }
}
