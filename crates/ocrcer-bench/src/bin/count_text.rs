//! `count-text`: chunk 14's counting stage (`PLAN.md` row 14, `ARCHITECTURE.md`
//! section 11's 2026-09-24 "do (a)-(c) in order" entry, item (c)).
//!
//! Counts word frequencies and character-bigram frequencies from
//! **training-split text only**, for a later, separate authoring pass to
//! consult when adding lexicon entries or blending counted bigrams with the
//! authored priors in `model/bigram_priors.tsv`. This binary only counts and
//! reports; it does not touch `model/lexicon.txt` or `model/bigram_priors.tsv`.
//!
//! # The firewall, enforced in code
//!
//! Every row read is checked against the committed split manifest with
//! [`ocrcer_bench::splits::assert_fittable`] before its text is touched, and
//! the input directory's name must end in `-train` ([`assert_train_dir_name`]).
//! Both checks are redundant with the dataset/split filter above them on
//! purpose -- CLAUDE.md rule 1 wants this caught by code, not by whoever
//! remembers to filter correctly.
//!
//! # Sources
//!
//! Only `multifinben-englishocr`'s `train` split rows are counted. CORD-v2
//! and SROIE `train` rows are licence-clean (CC-BY-4.0) but are excluded
//! here on a documented domain-fit finding, not a licence one:
//! `ARCHITECTURE.md`'s 2026-09-24 entry records "An earlier measurement
//! found the receipts (CORD, SROIE) out of domain. They are training
//! candidates only for their glyph appearance, not for layout or language."
//! Chunk 13 already spends them on glyph appearance; chunk 14 is language,
//! so they stay out here. They are also not yet rendered to text form at
//! all (`tools/manifest_render.py` only ever handled MultiFinBen).
//!
//! # Tokenisation
//!
//! Matches the shapes the runtime actually asks the two tables about:
//!
//! - **Words** (for `word_counts.tsv`): a whitespace-delimited token that is
//!   (1) not identifier-shaped by `ocrcer_core::params::identifier_shape` --
//!   the same gate `ocrcer-build`'s lexicon compiler applies (CLAUDE.md rule
//!   6) -- and (2) entirely letters, after trimming leading/trailing
//!   non-letter characters, using the charset's own `lower`/`upper`
//!   categories as the letter set. Case-folded to lowercase, matching how
//!   `model/lexicon.txt` stores base forms. A token failing either test
//!   contributes nothing to `word_counts.tsv` -- in particular every
//!   identifier-shaped token (a part number, a form number, a dimension)
//!   is excluded before it is ever counted as a word candidate, not merely
//!   suppressed later at decode time.
//! - **Character bigrams** (for `char_bigrams.tsv`): every whitespace token,
//!   including identifier-shaped ones, split into maximal runs of characters
//!   that are in `model/charset.tsv` (a character absent from the charset
//!   breaks the run and is otherwise discarded). Each run is counted the
//!   same way `ocrcer-build`'s `bigrams` module counts a lexicon word:
//!   boundary -> first char, each adjacent pair, last char -> boundary.
//!   `^` is reserved for the boundary marker in `model/bigram_priors.tsv`'s
//!   own convention, so it cannot also mark a boundary here without
//!   colliding with the real class `U+005E` (`^` is charset index 61); this
//!   file instead spells the boundary as the literal string `<BOUND>`,
//!   which cannot collide with any single-character column value.
//!
//! # Determinism
//!
//! All three outputs are written from sorted `BTreeMap`/`Vec` order, and the
//! only inputs are the committed manifest and the already-rendered training
//! pages -- no RNG, no wall-clock, no filesystem iteration order leaking
//! into the output (files are named explicitly from the manifest, not
//! globbed). Re-running against the same inputs reproduces the same bytes;
//! this is checked by running twice and diffing, not asserted here.
//!
//! # Usage
//!
//! ```text
//! cargo run -p ocrcer-bench --bin count-text -- <pages-train-dir> <out-dir>
//! ```
//!
//! `<pages-train-dir>` defaults to
//! `D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-train` and `<out-dir>` to
//! `D:/Dev/ExcludedPrivate/ocrcer/counts` when omitted.

use ocrcer_bench::splits;
use ocrcer_core::params::{identifier_shape, Params};

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const MULTIFINBEN_DATASET: &str = splits::MULTIFINBEN_DATASET;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let pages_dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-train"));
    let out_dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("D:/Dev/ExcludedPrivate/ocrcer/counts"));

    if let Err(e) = assert_train_dir_name(&pages_dir) {
        return fail(&e);
    }

    let manifest_path = ocrcer_bench::default_splits_root().join("manifest.tsv");
    let manifest_text = match std::fs::read_to_string(&manifest_path) {
        Ok(t) => t,
        Err(e) => return fail(&format!("reading {}: {e}", manifest_path.display())),
    };
    let manifest = match splits::parse_manifest_tsv(&manifest_text) {
        Ok(m) => m,
        Err(e) => return fail(&e),
    };

    let model_dir = ocrcer_build::tables::model_dir();
    let classes = match ocrcer_build::tables::load_charset(&model_dir) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    let charset_all: BTreeSet<char> = classes.iter().map(|c| c.codepoint).collect();
    let charset_letters: BTreeSet<char> = classes
        .iter()
        .filter(|c| c.category == "lower" || c.category == "upper")
        .map(|c| c.codepoint)
        .collect();

    let train_rows: Vec<&splits::ManifestRow> = manifest
        .iter()
        .filter(|r| r.dataset == MULTIFINBEN_DATASET && r.split == splits::Split::Train)
        .collect();
    if train_rows.is_empty() {
        return fail("no multifinben-englishocr train rows in the manifest");
    }

    let decode_defaults = Params::default().decode;

    let mut word_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut word_docs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut bigram_counts: BTreeMap<(Option<char>, Option<char>), u64> = BTreeMap::new();
    let mut sources: Vec<SourceRow> = Vec::new();

    let (mut total_tokens, mut identifier_tokens, mut word_tokens, mut discarded_tokens) =
        (0u64, 0u64, 0u64, 0u64);
    let mut docs_read = 0usize;

    for row in &train_rows {
        // Redundant with the filter above by design (module doc): a fitting
        // script must not be able to read a row the manifest does not name
        // train/validation, even after a future refactor of the filter.
        if let Err(e) = splits::assert_fittable(&manifest, MULTIFINBEN_DATASET, &row.row_id) {
            return fail(&e);
        }

        let (shard, row_num) = match split_row_id(&row.row_id) {
            Ok(v) => v,
            Err(e) => return fail(&e),
        };
        let shard_idx = match shard_index(&shard) {
            Ok(v) => v,
            Err(e) => return fail(&e),
        };
        let stem = format!("filing__s{shard_idx}__r{row_num:06}");
        let truth_path = pages_dir.join(format!("{stem}.truth.json"));

        let bytes = match std::fs::read(&truth_path) {
            Ok(b) => b,
            Err(e) => return fail(&format!("reading {}: {e}", truth_path.display())),
        };
        let sha256 = sha256_hex(&bytes);

        let value: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(e) => return fail(&format!("parsing {}: {e}", truth_path.display())),
        };
        let lines = match value.get("lines").and_then(|v| v.as_array()) {
            Some(a) => a,
            None => return fail(&format!("{}: no `lines` array", truth_path.display())),
        };

        docs_read += 1;
        sources.push(SourceRow {
            dataset: MULTIFINBEN_DATASET.to_string(),
            row_id: row.row_id.clone(),
            stem: stem.clone(),
            sha256,
            licence: row.licence.clone(),
            path: truth_path.display().to_string(),
        });

        for line in lines {
            let Some(text) = line.as_str() else { continue };
            for tok in text.split_whitespace() {
                total_tokens += 1;
                let len = tok.chars().count();
                let digits = tok.chars().filter(char::is_ascii_digit).count();
                let letters = tok.chars().filter(|c| c.is_alphabetic()).count();
                let is_identifier =
                    identifier_shape(len, digits, letters, &decode_defaults);
                if is_identifier {
                    identifier_tokens += 1;
                } else {
                    let trimmed = tok.trim_matches(|c: char| !charset_letters.contains(&c));
                    if !trimmed.is_empty()
                        && trimmed.chars().all(|c| charset_letters.contains(&c))
                    {
                        let canon = trimmed.to_lowercase();
                        *word_counts.entry(canon.clone()).or_insert(0) += 1;
                        word_docs.entry(canon).or_default().insert(row.row_id.clone());
                        word_tokens += 1;
                    } else {
                        discarded_tokens += 1;
                    }
                }
                count_bigrams(tok, &charset_all, &mut bigram_counts);
            }
        }
    }

    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        return fail(&format!("creating {}: {e}", out_dir.display()));
    }

    if let Err(e) = write_word_counts(&out_dir.join("word_counts.tsv"), &word_counts, &word_docs) {
        return fail(&e);
    }
    if let Err(e) = write_bigrams(&out_dir.join("char_bigrams.tsv"), &bigram_counts) {
        return fail(&e);
    }
    if let Err(e) = write_sources(&out_dir.join("sources.tsv"), &sources) {
        return fail(&e);
    }

    println!("count-text: {docs_read} training documents read from {}", pages_dir.display());
    println!("  total whitespace tokens:   {total_tokens}");
    println!("  identifier-shaped tokens:  {identifier_tokens} (excluded from word_counts.tsv)");
    println!("  word tokens counted:       {word_tokens}");
    println!("  discarded (not all-letter after trim): {discarded_tokens}");
    println!("  distinct candidate words:  {}", word_counts.len());
    println!("  distinct bigram pairs:     {}", bigram_counts.len());
    report_candidate_additions(&model_dir, &word_counts, &word_docs);

    ExitCode::SUCCESS
}

struct SourceRow {
    dataset: String,
    row_id: String,
    stem: String,
    sha256: String,
    licence: String,
    path: String,
}

/// Guards against pointing this tool at a score or validation directory by
/// argument mistake. An allow-list on the directory's own name, not a
/// denylist of known-bad names, so a future scoring directory added under
/// `pages/` is refused by default rather than by omission.
fn assert_train_dir_name(dir: &Path) -> Result<(), String> {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: cannot read a directory name", dir.display()))?;
    if name.ends_with("-train") {
        Ok(())
    } else {
        Err(format!(
            "{}: refusing to count from a directory not named `*-train` -- this tool must \
             never read a scoring or validation directory (PLAN.md section 2c firewall)",
            dir.display()
        ))
    }
}

fn split_row_id(row_id: &str) -> Result<(String, u32), String> {
    let (shard, row) = row_id
        .split_once('#')
        .ok_or_else(|| format!("row id {row_id:?} has no '#'"))?;
    let row_num: u32 = row.parse().map_err(|_| format!("bad row number in {row_id:?}"))?;
    Ok((shard.to_string(), row_num))
}

/// `train-00000-of-00008.parquet` -> `0`. Mirrors `tools/manifest_render.py`'s
/// `shard_index`, which is what named the files this tool reads.
fn shard_index(shard: &str) -> Result<u32, String> {
    let after_dash = shard
        .split('-')
        .nth(1)
        .ok_or_else(|| format!("shard name {shard:?} has no '-NNNNN-' segment"))?;
    after_dash.parse().map_err(|_| format!("bad shard index in {shard:?}"))
}

fn count_bigrams(
    tok: &str,
    charset_all: &BTreeSet<char>,
    counts: &mut BTreeMap<(Option<char>, Option<char>), u64>,
) {
    let mut run: Vec<char> = Vec::new();
    for c in tok.chars() {
        if charset_all.contains(&c) {
            run.push(c);
        } else {
            flush_run(&run, counts);
            run.clear();
        }
    }
    flush_run(&run, counts);
}

fn flush_run(run: &[char], counts: &mut BTreeMap<(Option<char>, Option<char>), u64>) {
    if run.is_empty() {
        return;
    }
    let mut prev: Option<char> = None;
    for &c in run {
        *counts.entry((prev, Some(c))).or_insert(0) += 1;
        prev = Some(c);
    }
    *counts.entry((prev, None)).or_insert(0) += 1;
}

fn render_symbol(c: Option<char>) -> String {
    match c {
        Some(c) => c.to_string(),
        None => "<BOUND>".to_string(),
    }
}

fn write_word_counts(
    path: &Path,
    counts: &BTreeMap<String, u64>,
    docs: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), String> {
    let mut rows: Vec<(&String, u64, usize)> = counts
        .iter()
        .map(|(w, &c)| (w, c, docs.get(w).map_or(0, BTreeSet::len)))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

    let mut out = String::from("#word\tcount\tdistinct_docs\n");
    for (w, c, d) in rows {
        out.push_str(&format!("{w}\t{c}\t{d}\n"));
    }
    std::fs::write(path, out).map_err(|e| format!("writing {}: {e}", path.display()))
}

fn write_bigrams(
    path: &Path,
    counts: &BTreeMap<(Option<char>, Option<char>), u64>,
) -> Result<(), String> {
    // BTreeMap<(Option<char>, ...)> already sorts with None < Some(_), which
    // reads oddly (every boundary-first row before any real character), but
    // stays deterministic; re-sort into a friendlier, still fully
    // deterministic order: literal characters first by codepoint, `<BOUND>`
    // last, same on both columns.
    let mut rows: Vec<((Option<char>, Option<char>), u64)> =
        counts.iter().map(|(&k, &v)| (k, v)).collect();
    let key = |c: Option<char>| -> (u8, u32) {
        match c {
            Some(c) => (0, c as u32),
            None => (1, 0),
        }
    };
    rows.sort_by(|a, b| {
        key(a.0 .0).cmp(&key(b.0 .0)).then_with(|| key(a.0 .1).cmp(&key(b.0 .1)))
    });

    let mut out = String::from("#prev\tnext\tcount\n");
    for ((prev, next), c) in rows {
        out.push_str(&format!("{}\t{}\t{c}\n", render_symbol(prev), render_symbol(next)));
    }
    std::fs::write(path, out).map_err(|e| format!("writing {}: {e}", path.display()))
}

fn write_sources(path: &Path, sources: &[SourceRow]) -> Result<(), String> {
    let mut rows: Vec<&SourceRow> = sources.iter().collect();
    rows.sort_by(|a, b| a.row_id.cmp(&b.row_id));
    let mut out = String::from("#dataset\trow_id\tstem\tsha256\tlicence\tpath\n");
    for r in rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            r.dataset, r.row_id, r.stem, r.sha256, r.licence, r.path
        ));
    }
    std::fs::write(path, out).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Informational only -- prints, to stdout, how many counted words would
/// pass a couple of candidate lexicon-addition thresholds and are not
/// already covered by the shipped lexicon (base forms plus their expansion).
/// Nothing here is written to `model/lexicon.txt`; this round is counting
/// only (see the module doc and the task instructions this binary was
/// written under).
fn report_candidate_additions(
    model_dir: &Path,
    counts: &BTreeMap<String, u64>,
    docs: &BTreeMap<String, BTreeSet<String>>,
) {
    let entries = match ocrcer_build::lexicon::load(model_dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("count-text: could not load model/lexicon.txt for overlap report: {e}");
            return;
        }
    };
    let existing: BTreeSet<String> = ocrcer_build::lexicon::expand(&entries)
        .into_iter()
        .map(|(w, _)| w.to_lowercase())
        .collect();

    println!("  existing shipped lexicon (expanded, lowercased): {} forms", existing.len());
    for &(min_count, min_docs) in &[(3u64, 2usize), (5, 3), (10, 5)] {
        let candidates: Vec<&String> = counts
            .iter()
            .filter(|(w, &c)| {
                c >= min_count
                    && docs.get(*w).map_or(0, BTreeSet::len) >= min_docs
                    && !existing.contains(*w)
            })
            .map(|(w, _)| w)
            .collect();
        println!(
            "  candidates under count>={min_count} & distinct_docs>={min_docs}, not already \
             covered: {}",
            candidates.len()
        );
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("count-text: {msg}");
    ExitCode::FAILURE
}

// --- SHA-256, pure std, no dependency -----------------------------------
//
// `sources.tsv` records a hash of each file actually read, so the manifest
// can be checked against what is on disk without shipping a copy of the
// corpus text. `ocrcer-bench` carries no cryptographic-hash crate today
// (`Cargo.lock` has no `sha2`/`digest`/`blake3`), and this chunk must not
// touch the shared workspace `Cargo.toml`/`Cargo.lock` while another agent's
// dependency edits are in flight there -- so this is the standard FIPS
// 180-4 algorithm, written out directly. It is not in the hot path of
// anything that ships (this binary never ships, per `CLAUDE.md` rule 4's
// `ocrcer-bench` carve-out), so a hand-rolled implementation costs nothing
// in the runtime.

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    h.iter().map(|v| format!("{v:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"[..64].to_string()
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"[..64].to_string()
        );
    }

    #[test]
    fn shard_index_reads_the_shard_number() {
        assert_eq!(shard_index("train-00000-of-00008.parquet").unwrap(), 0);
        assert_eq!(shard_index("train-00007-of-00008.parquet").unwrap(), 7);
    }

    #[test]
    fn train_dir_name_gate_requires_the_train_suffix() {
        assert!(assert_train_dir_name(Path::new("D:/x/finfilings-train")).is_ok());
        assert!(assert_train_dir_name(Path::new("D:/x/finfilings-val")).is_err());
        assert!(assert_train_dir_name(Path::new("D:/x/finfilings")).is_err());
        assert!(assert_train_dir_name(Path::new("D:/x/cord")).is_err());
    }

    #[test]
    fn bigram_run_splits_on_a_non_charset_character() {
        let charset: BTreeSet<char> = "abc".chars().collect();
        let mut counts = BTreeMap::new();
        count_bigrams("ab\u{1234}c", &charset, &mut counts);
        // "ab" and "c" are two separate runs because U+1234 is not in the
        // charset, so `b`->`c` must NOT be counted as adjacent.
        assert_eq!(counts.get(&(None, Some('a'))), Some(&1));
        assert_eq!(counts.get(&(Some('a'), Some('b'))), Some(&1));
        assert_eq!(counts.get(&(Some('b'), None)), Some(&1));
        assert_eq!(counts.get(&(None, Some('c'))), Some(&1));
        assert_eq!(counts.get(&(Some('c'), None)), Some(&1));
        assert_eq!(counts.get(&(Some('b'), Some('c'))), None);
    }
}
