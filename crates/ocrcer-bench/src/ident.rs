//! The identifier-preservation harness (`bin/ident.rs`).
//!
//! `CLAUDE.md` rule 6 and `PLAN.md` chunk 8 both require a test that fails
//! loudly when an identifier-shaped string (`M8x1.25`, `71-4820-B`,
//! `INV-2026-0042`) is silently rewritten — into a different identifier, or
//! worse, into a dictionary word the lexicon bonus favoured. Two unit tests
//! existed before this module (`docs/measurements/2026-09-25_score_12b.md`
//! section 4): one on the `is_identifier` predicate in isolation, one
//! confirming the lexicon holds nothing the gate would suppress. Neither
//! ran an image through the engine. This does.
//!
//! # Reuse, not reimplementation (`CLAUDE.md` rule 4)
//!
//! Rendering calls `ocrcer_build::page::render`/`check`/`to_pgm` — the same
//! rasteriser `ocrcer-build pages` uses for `bench/pages-cov`. Reading a
//! generated page calls `ocrcer_bench::pages::{list_pages, load_truth_beside,
//! load_page, page_text}` — the same reader `bin/ocr.rs` uses. Identifier
//! shape is decided by `ocrcer_core::params::identifier_shape`, fed by
//! `ocrcer_core::decode::viterbi::ClassInfo::of`, both already `pub` in
//! `ocrcer-core` — nothing in `ocrcer-core` was touched to build this.
//! Lexicon-word rewrites are decided by `ocrcer_core::decode::lexicon::lookup`
//! against the loaded model's own lexicon, not a second word list.
//!
//! # What counts as "an identifier in ground truth"
//!
//! Every whitespace-separated word in a truth line for which
//! `identifier_shape` is true, using the *loaded engine's* `Decode` params
//! (so a `--set` override that moved `decode.identifier_min_length` or
//! `decode.identifier_digit_fraction` would change what gets tested — it
//! does not, for either configuration this harness ships numbers for, since
//! neither is in the pre-fold control's override list). This corpus was
//! authored to be dense in such words; see `ident_corpus.rs`'s header for
//! what that biases and why it is still the right test.
//!
//! One consequence worth stating rather than hiding: `identifier_shape`
//! requires at least one letter (`params.rs`: `letters > 0`), so a
//! purely-numeric code like the account number `4100-02` in this corpus is
//! *not* identifier-shaped by this predicate and is not tested here — a
//! true statement about what the authored predicate protects, not a gap in
//! this harness.

use std::collections::HashMap;
use std::path::Path;

use ocrcer_core::decode::lexicon;
use ocrcer_core::decode::viterbi::ClassInfo;
use ocrcer_core::ocrw::Model;
use ocrcer_core::params::{identifier_shape, Decode};
use ocrcer_core::pipeline::Engine;
use ocrcer_core::Gray;

use crate::ident_corpus;
use crate::pages;

/// Renders the identifier corpus into `out_dir`, one `.pgm` + `.truth.json`
/// pair per (face, block, size), same file shapes `ocrcer-build pages`
/// writes so every `ocrcer-bench` reader already understands them.
///
/// Mirrors `ocrcer-build`'s own `run_pages` (`crates/ocrcer-build/src/
/// main.rs`) deliberately closely — same skip/refuse rules, same stem
/// shape — because a second corpus generator that drifted from the first
/// in what counts as a refusable page would be exactly the kind of
/// undeclared-degradation bug `page.rs`'s own doc comment warns about.
pub struct GenerateReport {
    pub written: usize,
    pub faces_used: usize,
    pub skipped_missing: usize,
}

pub fn generate(out_dir: &str, px_list: &[f32], local: bool) -> Result<GenerateReport, String> {
    let dir = ocrcer_build::tables::model_dir();
    let entries = ocrcer_build::tables::load_fonts(&dir)?;
    let out = Path::new(out_dir);
    std::fs::create_dir_all(out).map_err(|e| format!("cannot create {out_dir}: {e}"))?;

    let slug = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect()
    };

    let mut written = 0usize;
    let mut skipped_missing = 0usize;
    let mut faces_used = 0usize;

    for e in &entries {
        if !e.distribution.usable(local) {
            continue;
        }
        let Some(path) = e.file() else { continue };
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(face) = ocrcer_build::ttf_load::Face::parse(&bytes, 0) else { continue };
        faces_used += 1;

        for block in ident_corpus::ASCII_BLOCKS.iter().chain(ident_corpus::EXTENDED_BLOCKS) {
            let lines: Vec<String> = block.lines.iter().map(|s| (*s).to_string()).collect();
            for &px in px_list {
                let Some(pg) = ocrcer_build::page::render(&face, &lines, px) else { continue };
                if !pg.missing.is_empty() {
                    skipped_missing += 1;
                    continue;
                }
                let stem =
                    format!("ident__{}__{}__{}__{}px", slug(&e.family), slug(&e.style), block.name, px);
                ocrcer_build::page::check(&pg, &face).map_err(|err| format!("{stem}: {err}"))?;
                std::fs::write(out.join(format!("{stem}.pgm")), ocrcer_build::page::to_pgm(&pg))
                    .map_err(|err| format!("writing {stem}.pgm: {err}"))?;
                std::fs::write(out.join(format!("{stem}.truth.json")), truth_json(&pg, e, px))
                    .map_err(|err| format!("writing {stem}.truth.json: {err}"))?;
                written += 1;
            }
        }
    }

    if written == 0 {
        return Err("no pages rendered".into());
    }
    Ok(GenerateReport { written, faces_used, skipped_missing })
}

/// Byte-for-byte the same schema `ocrcer-build`'s private `truth_json`
/// writes (`family`, `style`, `px_per_em`, `width`, `height`, `lines`,
/// `glyphs`), rebuilt here because that function is private to
/// `ocrcer-build` and `ocrcer_bench::pages::load_truth` is the reader this
/// must agree with, not the writer.
fn truth_json(pg: &ocrcer_build::page::Page, e: &ocrcer_build::tables::FontEntry, px: f32) -> String {
    let esc = ocrcer_build::ocrw::json_string;
    let mut s = String::new();
    s.push_str(&format!(
        "{{\"family\":{},\"style\":{},\"px_per_em\":{px},\"width\":{},\"height\":{},\"lines\":[",
        esc(&e.family),
        esc(&e.style),
        pg.width,
        pg.height
    ));
    for (i, l) in pg.lines.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&esc(l));
    }
    s.push_str("],\"glyphs\":[");
    for (i, g) in pg.glyphs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"ch\":{},\"line\":{},\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"baseline\":{},\"x_height\":{}}}",
            esc(&g.ch.to_string()),
            g.line,
            g.x,
            g.y,
            g.width,
            g.height,
            g.baseline,
            g.x_height
        ));
    }
    s.push_str("]}");
    s
}

/// Whether `word` is identifier-shaped under the engine's own `Decode`
/// params. Delegates entirely to `ocrcer_core::params::identifier_shape` —
/// the same function `ocrcer_core::decode::viterbi`'s private `is_identifier`
/// calls at decode time — fed by the same per-character classification
/// (`ClassInfo::of`) the decoder uses. Not a second definition of
/// "identifier-shaped"; the same one, called from outside the crate.
pub fn is_identifier_shaped(word: &str, decode: &Decode) -> bool {
    let chars: Vec<char> = word.chars().collect();
    let len = chars.len();
    let digits = chars.iter().filter(|c| ClassInfo::of(**c).digit).count();
    let letters = chars.iter().filter(|c| ClassInfo::of(**c).letter).count();
    identifier_shape(len, digits, letters, decode)
}

/// Whether `word` is a whole word in the loaded model's own lexicon —
/// case-folded the same way the decoder folds it (`lexicon::Cursor::step`
/// folds internally; nothing here repeats that logic). `false` when the
/// model carries no lexicon, or `word` contains a character outside the
/// model's charset (such a word cannot be a lexicon entry either).
pub fn is_lexicon_word(word: &str, model: &Model, char_to_class: &HashMap<char, u16>) -> bool {
    let Some(lex) = &model.lexicon else { return false };
    let mut classes = Vec::with_capacity(word.chars().count());
    for ch in word.chars() {
        match char_to_class.get(&ch) {
            Some(&c) => classes.push(c),
            None => return false,
        }
    }
    lexicon::lookup(lex, &classes).is_some()
}

/// How an identifier-shaped ground-truth word came back.
///
/// `RewrittenIdentifier` and `RewrittenLexicon` are both "REWRITTEN" for
/// gate purposes (`CLAUDE.md` rule 6: silent, confident, invisible) —
/// reported separately because they are different mechanisms an
/// `ocrcer-linguist` fix would address differently (a confusion/segmentation
/// fix for the former, a lexicon-suppression fix for the latter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Exact,
    DroppedOrGarbled,
    RewrittenIdentifier,
    RewrittenLexicon,
}

/// One identifier-shaped ground-truth word and what happened to it.
#[derive(Clone, Debug)]
pub struct IdentCase {
    pub stem: String,
    pub line: usize,
    pub truth_word: String,
    pub hyp_word: Option<String>,
    pub outcome: Outcome,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct IdentTally {
    pub exact: usize,
    pub dropped: usize,
    pub rewritten_identifier: usize,
    pub rewritten_lexicon: usize,
}

impl IdentTally {
    pub fn rewritten(&self) -> usize {
        self.rewritten_identifier + self.rewritten_lexicon
    }

    pub fn total(&self) -> usize {
        self.exact + self.dropped + self.rewritten()
    }

    pub fn add(&mut self, other: &IdentTally) {
        self.exact += other.exact;
        self.dropped += other.dropped;
        self.rewritten_identifier += other.rewritten_identifier;
        self.rewritten_lexicon += other.rewritten_lexicon;
    }
}

/// Aligns two word sequences by Levenshtein edit distance (substitution
/// cost 1, 0 when equal) and returns, per truth-word position, the hyp-word
/// index it aligns to — `None` when the truth word has no counterpart
/// (dropped, or the line gained/lost enough other words that nothing lines
/// up). A full matrix rather than `cer::levenshtein`'s two-row form: that
/// function returns a distance, not a traceback, and these lines are a
/// handful of words long, so the matrix costs nothing that matters.
///
/// Tie-break order — diagonal (match/substitute) before delete before
/// insert — matches `cer::levenshtein`'s own preference for substitution
/// over separate insert+delete, so a single wrong word aligns as one
/// substitution rather than a delete-then-insert pair that would report it
/// as dropped *and* an unrelated insertion.
fn align_words(truth: &[&str], hyp: &[&str]) -> Vec<Option<usize>> {
    let n = truth.len();
    let m = hyp.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in dp.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        dp[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let sub_cost = usize::from(truth[i - 1] != hyp[j - 1]);
            let sub = dp[i - 1][j - 1] + sub_cost;
            let del = dp[i - 1][j] + 1;
            let ins = dp[i][j - 1] + 1;
            dp[i][j] = sub.min(del).min(ins);
        }
    }

    let mut i = n;
    let mut j = m;
    let mut result = vec![None; n];
    while i > 0 || j > 0 {
        if i > 0 && j > 0 {
            let sub_cost = usize::from(truth[i - 1] != hyp[j - 1]);
            if dp[i][j] == dp[i - 1][j - 1] + sub_cost {
                result[i - 1] = Some(j - 1);
                i -= 1;
                j -= 1;
                continue;
            }
        }
        if i > 0 && dp[i][j] == dp[i - 1][j] + 1 {
            i -= 1;
            continue;
        }
        j -= 1;
    }
    result
}

fn classify_word(
    truth_word: &str,
    hyp_word: Option<&str>,
    model: &Model,
    char_to_class: &HashMap<char, u16>,
    decode: &Decode,
) -> Outcome {
    match hyp_word {
        Some(h) if h == truth_word => Outcome::Exact,
        Some(h) if is_identifier_shaped(h, decode) => Outcome::RewrittenIdentifier,
        Some(h) if is_lexicon_word(h, model, char_to_class) => Outcome::RewrittenLexicon,
        _ => Outcome::DroppedOrGarbled,
    }
}

/// Runs every page in `dir` through `engine` end to end (real segmentation,
/// real binarization — not the oracle reader `pages::read_with_bank` gives
/// other harnesses) and classifies every identifier-shaped ground-truth
/// word. `dir` must hold pages [`generate`] wrote.
pub fn score_dir(engine: &Engine, dir: &str) -> Result<(IdentTally, Vec<IdentCase>), String> {
    let model = engine.model();
    let char_to_class: HashMap<char, u16> =
        model.classes.iter().map(|c| (c.codepoint, c.index)).collect();
    let decode = &engine.params().decode;

    let pgms = pages::list_pages(dir)?;
    let mut tally = IdentTally::default();
    let mut offenders = Vec::new();

    for pgm in &pgms {
        let stem = pgm.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let truth = pages::load_truth_beside(pgm)?;
        let (w, h, grey) = pages::load_page(pgm)?;
        let lines = engine
            .recognize_lines(Gray { width: w, height: h, data: &grey })
            .map_err(|e| format!("{stem}: {e:?}"))?;
        let read = pages::page_text(&lines);
        let hyp_lines: Vec<&str> = read.lines().collect();

        for (li, truth_line) in truth.lines.iter().enumerate() {
            let truth_words: Vec<&str> = truth_line.split_whitespace().collect();
            let hyp_words: Vec<&str> =
                hyp_lines.get(li).map(|s| s.split_whitespace().collect()).unwrap_or_default();
            let alignment = align_words(&truth_words, &hyp_words);

            for (wi, tw) in truth_words.iter().enumerate() {
                if !is_identifier_shaped(tw, decode) {
                    continue;
                }
                let hyp_word = alignment[wi].map(|hi| hyp_words[hi]);
                let outcome = classify_word(tw, hyp_word, model, &char_to_class, decode);
                match outcome {
                    Outcome::Exact => tally.exact += 1,
                    Outcome::DroppedOrGarbled => tally.dropped += 1,
                    Outcome::RewrittenIdentifier => tally.rewritten_identifier += 1,
                    Outcome::RewrittenLexicon => tally.rewritten_lexicon += 1,
                }
                if matches!(outcome, Outcome::RewrittenIdentifier | Outcome::RewrittenLexicon) {
                    offenders.push(IdentCase {
                        stem: stem.clone(),
                        line: li,
                        truth_word: (*tw).to_string(),
                        hyp_word: hyp_word.map(|s| s.to_string()),
                        outcome,
                    });
                }
            }
        }
    }

    Ok((tally, offenders))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_a_single_substitution_as_one_pair_not_a_delete_and_insert() {
        let truth = vec!["4X", "M8x1.25", "THRU"];
        let hyp = vec!["4X", "IV18x1", ".25", "THRU"];
        // "M8x1.25" split into two hyp words by a segmentation error: the
        // aligner still owes the truth word *some* answer, and the
        // deterministic tie-break (diagonal before insert) puts it on the
        // first of the two.
        let a = align_words(&truth, &hyp);
        assert_eq!(a[0], Some(0)); // 4X -> 4X
        assert_eq!(a[2], Some(3)); // THRU -> THRU
    }

    #[test]
    fn a_dropped_word_aligns_to_none() {
        let truth = vec!["PART", "NO.", "71-4820-B", "REV", "C"];
        let hyp = vec!["PART", "NO.", "REV", "C"];
        let a = align_words(&truth, &hyp);
        assert_eq!(a[2], None);
    }
}
