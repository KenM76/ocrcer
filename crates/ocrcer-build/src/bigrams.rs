//! Compiles the `bigrams` table: character-pair log-probabilities counted
//! from the authored lexicon, sharpened by the authored priors in
//! `model/bigram_priors.tsv`, with a category backoff for every pair neither
//! source saw.
//!
//! # Contract
//!
//! [`load_priors`] reads the authored file and fails loudly on an unknown
//! category, an unknown character or a malformed row. [`build`] produces the
//! byte layout documented on [`Table`], which `ocrcer_core::decode::bigram`
//! reads. Two runs over the same inputs produce the same bytes.
//!
//! # Where the numbers come from, and what they are not
//!
//! Nothing here is fitted and nothing here is measured on running text. There
//! are exactly two sources, and both are auditable:
//!
//! - **Counts over `lexicon.txt`**, expanded and weighted by tier, which is a
//!   deterministic script over an authored file.
//! - **Pseudo-counts from `bigram_priors.tsv`**, which are authored guesses
//!   carrying the domain knowledge a word list structurally cannot hold: that
//!   a digit follows a digit, that a currency sign precedes one, that
//!   `M8x1.25` is an ordinary string here.
//!
//! What that buys is the thing `ARCHITECTURE.md` section 5 says the decoder
//! needs — `rnodern` scoring worse than `modern` — without a corpus of
//! unknown provenance entering the model (`CLAUDE.md` rule 2). What it does
//! not buy is a calibrated language model over English prose, and this file
//! does not claim one.

use crate::lexicon;
use crate::tables::Class;

use std::collections::BTreeMap;
use std::path::Path;

/// Categories, in the index order written into the table. The last is the
/// word boundary, which is a prev meaning "start of word" and a next meaning
/// "end of word".
pub const CATEGORIES: [&str; 8] =
    ["lower", "upper", "digit", "punct", "symbol", "math", "currency", "^"];

/// The boundary category's index.
pub const CAT_BOUNDARY: usize = 7;

/// Probability mass each row reserves for pairs it never saw.
///
/// **An authored guess, on chunk 8's tuning list.** What is defensible is
/// that it is neither zero nor large: zero would make an unseen pair
/// impossible, and the decoder would then be unable to read a string the
/// lexicon and the priors between them did not anticipate — which in this
/// domain is most part numbers. Large would dissolve the distinction the
/// table exists to draw.
pub const BACKOFF_MASS: f64 = 0.15;

/// How much more a tier-1 word counts than a tier-5 one.
///
/// **Authored guesses, on chunk 8's tuning list.** They stand in for the word
/// frequencies a corpus would supply and this project will not download. The
/// shape is the defensible part: `the` contributes more evidence about which
/// letters follow which than `polycarbonate` does.
pub const TIER_WEIGHT: [f64; 5] = [8.0, 4.0, 2.0, 1.0, 0.5];

/// How much a capitalised and an all-caps rendering of a lexicon word count,
/// relative to the authored form.
///
/// **Authored guesses, on chunk 8's tuning list.** They exist because the
/// lexicon is written in one case and the page is not: headings, title blocks
/// and CAD notes are frequently upper case, and a table counted only from
/// lowercase forms would score `INVOICE` as improbable.
pub const CASE_WEIGHT_TITLE: f64 = 0.30;
pub const CASE_WEIGHT_UPPER: f64 = 0.15;

/// One authored row.
#[derive(Debug, Clone)]
pub enum Prior {
    /// Every category pair not otherwise named.
    Default(f64),
    /// A category-pair pseudo-count.
    Cat { prev: usize, next: usize, weight: f64 },
    /// A specific character-pair pseudo-count.
    Pair { prev: char, next: char, weight: f64 },
}

fn category_index(name: &str) -> Option<usize> {
    CATEGORIES.iter().position(|c| *c == name)
}

/// Reads `model/bigram_priors.tsv`.
pub fn load_priors(dir: &Path) -> Result<Vec<Prior>, String> {
    let path = dir.join("bigram_priors.tsv");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim_end();
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        let bad = |why: &str| format!("bigram_priors.tsv line {}: {why}", n + 1);
        match f.as_slice() {
            ["default", w] => {
                out.push(Prior::Default(w.trim().parse().map_err(|_| bad("bad weight"))?))
            }
            ["cat", p, x, w] => {
                let prev = category_index(p).ok_or_else(|| bad("unknown prev category"))?;
                let next = category_index(x).ok_or_else(|| bad("unknown next category"))?;
                out.push(Prior::Cat {
                    prev,
                    next,
                    weight: w.trim().parse().map_err(|_| bad("bad weight"))?,
                });
            }
            ["pair", p, x, w] => {
                let one = |s: &str| -> Result<char, String> {
                    let mut it = s.chars();
                    match (it.next(), it.next()) {
                        (Some(c), None) => Ok(c),
                        _ => Err(bad("a pair column holds one character")),
                    }
                };
                out.push(Prior::Pair {
                    prev: one(p)?,
                    next: one(x)?,
                    weight: w.trim().parse().map_err(|_| bad("bad weight"))?,
                });
            }
            _ => return Err(bad("unrecognised row")),
        }
    }
    if out.is_empty() {
        return Err("bigram_priors.tsv holds no rows".into());
    }
    Ok(out)
}

/// The compiled table, in the byte layout the runtime reads.
///
/// ```text
/// 0   magic        [u8; 4]   "BGRM"
/// 4   version      u16       1
/// 6   n_cats       u16       8
/// 8   n_rows       u32       charset size + 1; the last row is the boundary
/// 12  n_entries    u32
/// 16  row_off      [u32; n_rows + 1]
///     col          [u16; n_entries]   next symbol, ascending within a row
///     logp         [f32; n_entries]   log2 P(next | prev)
///     row_backoff  [f32; n_rows]      log2 of the mass this row leaves unseen
///     cat_logp     [f32; n_cats * n_cats]
///     cat_of       [u8;  n_rows]
/// ```
///
/// The runtime's whole rule is: binary search the row for `next`; on a hit
/// the stored `logp` is the answer, and on a miss it is
/// `row_backoff[prev] + cat_logp[cat_of[prev]][cat_of[next]]`. One branch and
/// one add, which is what it costs to have this inside the decoder's inner
/// loop.
pub struct Table {
    pub bytes: Vec<u8>,
    pub rows: usize,
    pub entries: usize,
}

/// Counts, scores and encodes.
pub fn build(
    words: &[(String, u8)],
    priors: &[Prior],
    classes: &[Class],
) -> Result<Table, String> {
    let n_classes = classes.len();
    let boundary = n_classes as u16;
    let n_rows = n_classes + 1;

    let by_char: BTreeMap<char, u16> = classes.iter().map(|c| (c.codepoint, c.index)).collect();
    let cat_of: Vec<usize> = classes
        .iter()
        .map(|c| category_index(&c.category).unwrap_or(CAT_BOUNDARY))
        .chain(core::iter::once(CAT_BOUNDARY))
        .collect();

    // --- counts ----------------------------------------------------------
    let mut counts: BTreeMap<(u16, u16), f64> = BTreeMap::new();
    let mut add = |prev: u16, next: u16, w: f64| {
        *counts.entry((prev, next)).or_insert(0.0) += w;
    };

    for (word, tier) in words {
        let base = TIER_WEIGHT[(*tier as usize).clamp(1, 5) - 1];
        for (rendering, case_weight) in
            [(word.clone(), 1.0), (title_case(word), CASE_WEIGHT_TITLE), (word.to_uppercase(), CASE_WEIGHT_UPPER)]
        {
            let w = base * case_weight;
            if w <= 0.0 {
                continue;
            }
            let syms: Option<Vec<u16>> =
                rendering.chars().map(|c| by_char.get(&c).copied()).collect();
            let Some(syms) = syms else { continue };
            if syms.is_empty() {
                continue;
            }
            add(boundary, syms[0], w);
            for pair in syms.windows(2) {
                add(pair[0], pair[1], w);
            }
            add(syms[syms.len() - 1], boundary, w);
        }
    }

    // --- authored priors -------------------------------------------------
    let mut default = 0.0f64;
    let mut cat_weight = [[0.0f64; CATEGORIES.len()]; CATEGORIES.len()];
    let mut cat_named = [[false; CATEGORIES.len()]; CATEGORIES.len()];
    for p in priors {
        match *p {
            Prior::Default(w) => default = w,
            Prior::Cat { prev, next, weight } => {
                cat_weight[prev][next] = weight;
                cat_named[prev][next] = true;
            }
            Prior::Pair { prev, next, weight } => {
                let p = *by_char.get(&prev).ok_or_else(|| {
                    format!("bigram_priors.tsv names `{prev}`, which the charset does not have")
                })?;
                let n = *by_char.get(&next).ok_or_else(|| {
                    format!("bigram_priors.tsv names `{next}`, which the charset does not have")
                })?;
                add(p, n, weight);
            }
        }
    }
    for (i, row) in cat_weight.iter_mut().enumerate() {
        for (j, w) in row.iter_mut().enumerate() {
            if !cat_named[i][j] {
                *w = default;
            }
        }
    }

    // --- category backoff ------------------------------------------------
    // The per-class probability an unseen pair gets: the category-conditional
    // probability divided by how many classes share that category, so a
    // category with many members does not hand each of them the whole cell.
    let mut members = [0usize; CATEGORIES.len()];
    for c in classes {
        members[category_index(&c.category).unwrap_or(CAT_BOUNDARY)] += 1;
    }
    members[CAT_BOUNDARY] = 1;
    let mut cat_logp = vec![0.0f32; CATEGORIES.len() * CATEGORIES.len()];
    for i in 0..CATEGORIES.len() {
        let total: f64 = cat_weight[i].iter().sum();
        for j in 0..CATEGORIES.len() {
            let n = members[j].max(1) as f64;
            let p = if total > 0.0 { cat_weight[i][j] / total / n } else { 0.0 };
            cat_logp[i * CATEGORIES.len() + j] = log2_or_floor(p);
        }
    }

    // --- rows ------------------------------------------------------------
    let mut totals = vec![0.0f64; n_rows];
    for (&(prev, _), &c) in &counts {
        totals[prev as usize] += c;
    }

    let mut row_off = Vec::with_capacity(n_rows + 1);
    let mut col: Vec<u16> = Vec::with_capacity(counts.len());
    let mut logp: Vec<f32> = Vec::with_capacity(counts.len());
    let mut row_backoff = vec![0.0f32; n_rows];
    let mut it = counts.iter().peekable();
    for row in 0..n_rows {
        row_off.push(col.len() as u32);
        let total = totals[row];
        // A row that observed nothing gives its whole mass to the backoff;
        // one that observed something keeps all but `BACKOFF_MASS`. Folding
        // the factor in here is what lets the runtime add one number rather
        // than branch on whether the row is empty.
        row_backoff[row] =
            log2_or_floor(if total > 0.0 { BACKOFF_MASS } else { 1.0 });
        while let Some((&(p, n), &c)) = it.peek() {
            if p as usize != row {
                break;
            }
            col.push(n);
            logp.push(log2_or_floor((1.0 - BACKOFF_MASS) * c / total));
            it.next();
        }
    }
    row_off.push(col.len() as u32);

    Ok(encode(&row_off, &col, &logp, &row_backoff, &cat_logp, &cat_of))
}

/// The floor a zero probability takes, in log2.
///
/// Not negative infinity: an impossible bigram would make the decoder unable
/// to produce a string at all, and rule 6's principle — the language model
/// may discourage and may never forbid — applies to the bigram term for the
/// same reason it applies to the lexicon.
pub const LOG2_FLOOR: f32 = -40.0;

fn log2_or_floor(p: f64) -> f32 {
    if p > 0.0 {
        (p.log2() as f32).max(LOG2_FLOOR)
    } else {
        LOG2_FLOOR
    }
}

fn title_case(w: &str) -> String {
    let mut it = w.chars();
    match it.next() {
        Some(c) => c.to_uppercase().collect::<String>() + it.as_str(),
        None => String::new(),
    }
}

fn encode(
    row_off: &[u32],
    col: &[u16],
    logp: &[f32],
    row_backoff: &[f32],
    cat_logp: &[f32],
    cat_of: &[usize],
) -> Table {
    let rows = row_backoff.len();
    let mut b = Vec::new();
    b.extend_from_slice(b"BGRM");
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&(CATEGORIES.len() as u16).to_le_bytes());
    b.extend_from_slice(&(rows as u32).to_le_bytes());
    b.extend_from_slice(&(col.len() as u32).to_le_bytes());
    for &v in row_off {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for &v in col {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for &v in logp {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for &v in row_backoff {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for &v in cat_logp {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for &v in cat_of {
        b.push(v as u8);
    }
    Table { bytes: b, rows, entries: col.len() }
}

/// Builds the table straight from the authored files.
pub fn from_model_dir(dir: &Path, classes: &[Class]) -> Result<Table, String> {
    let words = lexicon::expand(&lexicon::load(dir)?);
    let priors = load_priors(dir)?;
    build(&words, &priors, classes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables;

    #[test]
    fn the_authored_priors_parse() {
        let priors = load_priors(&tables::model_dir()).expect("priors parse");
        assert!(priors.iter().any(|p| matches!(p, Prior::Default(_))), "no default row");
        assert!(priors.len() > 100, "only {} rows", priors.len());
    }

    #[test]
    fn the_table_builds_and_is_byte_identical_across_runs() {
        let dir = tables::model_dir();
        let classes = tables::load_charset(&dir).expect("charset");
        let a = from_model_dir(&dir, &classes).expect("table");
        let b = from_model_dir(&dir, &classes).expect("table");
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(a.rows, classes.len() + 1);
        assert!(a.entries > 1000, "only {} entries", a.entries);
        assert_eq!(&a.bytes[..4], b"BGRM");
    }

    /// The claim `ARCHITECTURE.md` section 5 makes about why the decoder
    /// works at all, checked on the table rather than asserted: `rn` has to
    /// look worse than the pairs a real word uses.
    #[test]
    fn a_real_bigram_outscores_the_confusion_that_produces_it() {
        let dir = tables::model_dir();
        let classes = tables::load_charset(&dir).expect("charset");
        let words = lexicon::expand(&lexicon::load(&dir).expect("lexicon"));
        let priors = load_priors(&dir).expect("priors");
        let table = build(&words, &priors, &classes).expect("table");

        let lookup = |a: char, b: char| -> f32 {
            let idx = |c: char| classes.iter().find(|k| k.codepoint == c).expect("class").index;
            read_pair(&table.bytes, idx(a), idx(b), classes.len())
        };
        // `mo` as in `modern`, against `rn` as in `rnodern`.
        assert!(lookup('m', 'o') > lookup('r', 'n'), "mo {} vs rn {}", lookup('m', 'o'), lookup('r', 'n'));
        // `cl` really does occur, so this is not a claim that it cannot: only
        // that inside `close` the following `o` is what separates them.
        assert!(lookup('o', 's') > lookup('0', 's'));
    }

    /// A pair neither source saw must still be readable, or the decoder
    /// cannot produce a part number.
    #[test]
    fn an_unseen_pair_falls_back_rather_than_becoming_impossible() {
        let dir = tables::model_dir();
        let classes = tables::load_charset(&dir).expect("charset");
        let table = from_model_dir(&dir, &classes).expect("table");
        let idx = |c: char| classes.iter().find(|k| k.codepoint == c).expect("class").index;
        let v = read_pair(&table.bytes, idx('Q'), idx('7'), classes.len());
        assert!(v > LOG2_FLOOR, "an unseen pair came back at the floor");
        assert!(v < 0.0);
    }

    /// The same lookup the runtime does, written here so the build-side test
    /// checks the bytes rather than the builder's own intermediate state.
    fn read_pair(bytes: &[u8], prev: u16, next: u16, n_classes: usize) -> f32 {
        let n_cats = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        let rows = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let entries = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        let off = 16;
        let col = off + 4 * (rows + 1);
        let logp = col + 2 * entries;
        let backoff = logp + 4 * entries;
        let catlog = backoff + 4 * rows;
        let catof = catlog + 4 * n_cats * n_cats;
        let u32at = |p: usize| u32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]);
        let f32at = |p: usize| f32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]);
        let lo = u32at(off + 4 * prev as usize) as usize;
        let hi = u32at(off + 4 * (prev as usize + 1)) as usize;
        for e in lo..hi {
            let c = u16::from_le_bytes([bytes[col + 2 * e], bytes[col + 2 * e + 1]]);
            if c == next {
                return f32at(logp + 4 * e);
            }
        }
        let cp = bytes[catof + prev as usize] as usize;
        let cn = bytes[catof + next as usize] as usize;
        let _ = n_classes;
        f32at(backoff + 4 * prev as usize) + f32at(catlog + 4 * (cp * n_cats + cn))
    }
}
