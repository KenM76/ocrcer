//! Every threshold in the pipeline, in one place, with the model file able to
//! override any of them.
//!
//! # Contract
//!
//! [`Params::DEFAULT`] is the authored starting point and is what the engine
//! uses when no model file carries a `params` table. [`Params::apply`] reads
//! such a table and overrides by name; a name the table carries and this
//! build does not know is **ignored**, and a name this build knows and the
//! table omits **keeps its default**.
//!
//! Both of those asymmetries are deliberate and they mirror
//! `ARCHITECTURE.md` section 7's rule for table names. A parameter is a
//! *number*, not a *meaning*: an old runtime that ignores a new threshold
//! runs the code path it always ran, which is by construction the path that
//! threshold does not control. Redefining what an existing name means is the
//! case that is not additive, and that needs a format version bump like any
//! other change of meaning.
//!
//! # Why the defaults are also in `model/params.tsv`
//!
//! They have to be in Rust or the engine cannot run without a file, and they
//! have to be in the TSV or there is nowhere to record that
//! `segment.valley_fraction` is a guess and `lines.x_height_per_cap` is a
//! measurement. `ocrcer-build`'s `params` test fails the day the two
//! disagree, which is what `CLAUDE.md` rule 4 actually asks for — not that a
//! number may never appear twice, but that nothing may silently diverge.

/// The magic the builder writes.
const MAGIC: &[u8; 4] = b"PARM";
/// The layout this reader understands.
const VERSION: u16 = 1;

/// Every tunable number in the pipeline.
///
/// Field names match the second half of the dotted names in
/// `model/params.tsv`; the stage prefix is the sub-struct.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    pub binarize: Binarize,
    pub deskew: Deskew,
    pub lines: Lines,
    pub words: Words,
    pub segment: Segment,
    pub matching: Matching,
    pub confidence: Confidence,
    pub decode: Decode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Binarize {
    pub window: u32,
    pub k: f32,
    pub r: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Deskew {
    pub max_slope: f32,
    pub min_corrected_slope: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lines {
    pub min_area: u32,
    pub overlap_fraction: f32,
    pub furniture_fraction: f32,
    pub rule_aspect: f32,
    pub mark_height_fraction: f32,
    pub mark_reach_fraction: f32,
    pub descender_fraction: f32,
    pub descender_reach_fraction: f32,
    pub x_height_per_cap: f32,
    pub x_height_floor_per_cap: f32,
    pub inherit_x_height_below: f32,
    pub column_gap_heights: f32,
    pub column_lone_guard: u32,
    pub baseline_split: u32,
    pub baseline_split_sep: f32,
    pub baseline_split_support: f32,
    pub rule_run_heights: f32,
    pub debris_heights: f32,
    pub thin_debris_heights: f32,
    pub underline_strip: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Words {
    pub min_gaps: u32,
    pub min_separability: f32,
    pub lone_gap_x_heights: f32,
    pub no_valley_x_heights: f32,
    pub min_valley_x_heights: f32,
    pub pitch_min_glyphs: u32,
    pub pitch_agreement: f32,
    pub pitch_tolerance: f32,
    pub pitch_cell_merge: u32,
    pub pitch_grid_check: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub max_merge: u32,
    pub max_merge_x_heights: f32,
    pub split_min_x_heights: f32,
    pub max_splits: u32,
    pub valley_fraction: f32,
    pub min_piece_x_heights: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matching {
    pub top_k: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Confidence {
    pub lm_floor: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decode {
    pub w_match: f32,
    /// The match distance a correct single character is expected to cost.
    ///
    /// Credited once per character before its distance is charged, so the
    /// match term is `w_match * (char_bonus - distance)`: an average
    /// character is score-neutral, a clean one earns, a poor one pays. Without
    /// it every character carries a strictly negative term and a path that
    /// explains the same ink with fewer characters wins before any evidence is
    /// weighed — which reads on a page as two narrow letters merged into one
    /// wide one.
    pub char_bonus: f32,
    pub w_bigram: f32,
    pub w_lex: f32,
    pub w_seg: f32,
    /// Lexicon bonus by tier, tier 1 first.
    pub lex_bonus: [f32; 5],
    pub identifier_digit_fraction: f32,
    pub identifier_min_length: u32,
    pub seg_ideal_aspect: f32,
    pub seg_aspect_tolerance: f32,
    /// Charged to an edge that takes several atoms as one character.
    pub seg_merge_penalty: f32,
    /// Charged to an edge bounded by a cut inside an atom.
    pub seg_split_penalty: f32,
    /// Global multiplier on the confusion table, which already stores log2
    /// units. One unless the whole table proves too loud.
    pub w_confusion: f32,
    /// Charged once per letter transition that leaves the three case shapes
    /// printed words take — `lower`, `Title`, `UPPER`.
    ///
    /// In the same units as `char_bonus`, so the value says how much better
    /// a case-anomalous reading's match has to be before it is preferred.
    /// Suppressed inside an identifier-shaped word, where an orthographic
    /// prior is most likely to be confidently wrong. See
    /// `crate::decode::viterbi::CaseShape`.
    pub case_shape_penalty: f32,
    /// Hypotheses kept per lattice node during the beam search.
    pub beam_width: u32,
}

/// True when a word of this shape is *identifier-shaped*, and the decoder's
/// orthographic terms are therefore suppressed inside it.
///
/// Written once and called from both sides on purpose. The decoder counts
/// digits and letters from class flags while a model build counts them from
/// characters, and those two really are different inputs — but the rule that
/// turns the counts into a verdict must not exist twice, or a word list
/// authored against one reading of it would carry entries the decoder can
/// never let fire (`CLAUDE.md` rule 4).
///
/// `letters > 0` is what keeps a pure number out: the lexicon holds no digit
/// strings, so suppressing the term there would change nothing and the flag
/// would only mislead a report reading it.
pub fn identifier_shape(len: usize, digits: usize, letters: usize, p: &Decode) -> bool {
    len >= p.identifier_min_length as usize
        && letters > 0
        && digits as f32 >= p.identifier_digit_fraction * len as f32
}

impl Params {
    /// The authored defaults. Mirrors `model/params.tsv`, which is where each
    /// one's provenance is recorded.
    pub const DEFAULT: Params = Params {
        binarize: Binarize { window: 25, k: 0.34, r: 128.0 },
        deskew: Deskew { max_slope: 0.0874, min_corrected_slope: 0.0015 },
        lines: Lines {
            min_area: 2,
            overlap_fraction: 0.5,
            furniture_fraction: 0.2,
            rule_aspect: 0.0,
            mark_height_fraction: 0.35,
            mark_reach_fraction: 0.6,
            descender_fraction: 0.12,
            descender_reach_fraction: 0.4,
            x_height_per_cap: 0.7431,
            // Measured, `ARCHITECTURE.md` section 11, 2026-09-23 ("Merged
            // lines: split on two baselines"): half of the smallest per-face
            // x-height/cap-height ratio across the shippable font set,
            // `ocrcer-build metrics` (2026-09-23, 32 faces) — 0.6335 on
            // `OCRcer Technical`. See
            // `docs/measurements/2026-09-23_line_merge_phase2.txt`.
            x_height_floor_per_cap: 0.3168,
            inherit_x_height_below: 0.5,
            column_gap_heights: 1.75,
            // Guess, `ARCHITECTURE.md` section 11, 2026-09-23 ("The narrowed
            // lone-glyph rule also fails the real-filings gate"): the guard
            // (1) and the plain greedy cut (0) have each cost real pages on
            // finfilings with neither corpus able to say why. Ships 0, the
            // configuration both bench/pages-cov and finfilings measured
            // best, until a per-page diff of the two picks a mechanism.
            column_lone_guard: 0,
            // Measured, `ARCHITECTURE.md` section 11, 2026-09-23 ("Merged lines
            // ship"): on, with the x-height floor.
            baseline_split: 1,
            // Guess, same entry.
            baseline_split_sep: 0.6,
            // Guess, same entry.
            baseline_split_support: 0.25,
            // Measured, `ARCHITECTURE.md` section 11, 2026-09-23
            // ("Underlines are stripped from the pixels of over-wide
            // components"): `ocrcer-build aspect` (extended 2026-09-23)
            // renders all 187 charset classes on all 32 shippable faces at
            // 256 px/em and reports, per class per face, the longest
            // horizontal ink run divided by that face's x-height; the
            // maximum is the em dash on `OCRcer Technical Regular` at
            // 340px / 125.44px = 2.7105 x-heights. Doubled for authored
            // headroom. See
            // `docs/measurements/2026-09-23_underline_strip.txt`.
            rule_run_heights: 5.4209,
            // Measured, `ARCHITECTURE.md` section 11, 2026-09-23 ("Underline
            // strip, second rule: straight bands only, keep crossing
            // strokes, drop strip debris, and keep what was stripped"): the
            // same `ocrcer-build aspect` run also reports, per class per
            // face, the tallest ink height divided by that face's x-height;
            // the maximum is the radical sign on `STIX Two Math Regular` at
            // 304px / 121.09px = 2.5106 x-heights. Doubled for the same
            // authored headroom `rule_run_heights` uses. See
            // `docs/measurements/2026-09-23_underline_strip.txt`.
            debris_heights: 5.0211,
            // Guess (headroom only), `ARCHITECTURE.md` section 11,
            // 2026-09-23 ("Two mechanisms named: the strip leaves a box-side
            // sliver..."): `ocrcer-build aspect` (extended 2026-09-23)
            // reports the tallest ink height, in x-heights, among classes
            // whose own ink width is `<= 0.5` x-height, over all 32
            // shippable faces -- that height is measured. The x1.5 factor
            // over it is a guess, smaller than `rule_run_heights`/
            // `debris_heights`'s x2 because x2 would sit too close to the
            // observed sliver (about 3.6-3.9 x h). See
            // `docs/measurements/2026-09-23_split_gate_and_strip_3b.txt`.
            thin_debris_heights: 0.0,
            // Guess: ships off pending the architect's read of the readings
            // in the same measurement file.
            underline_strip: 0,
        },
        words: Words {
            min_gaps: 3,
            min_separability: 0.7,
            lone_gap_x_heights: 0.7,
            no_valley_x_heights: 0.4,
            min_valley_x_heights: 0.3,
            pitch_min_glyphs: 6,
            pitch_agreement: 0.8,
            pitch_tolerance: 0.15,
            pitch_cell_merge: 1,
            // Bounded fallback, `ARCHITECTURE.md` section 11's third
            // amendment ("the grid vote failed too"): the fitted-grid
            // residual test met its own gate criteria on mono recall
            // (90.6% >= 88.3%) but not the rest -- measured proportional
            // false positives 2.48% at cg=0 against a required <=0.5%,
            // statement F1 67.204%/deletions 333 against a required
            // >=68.278%/<=296, and ACCOUNT STATEMENT fused at 37 of 55
            // font/size pairs. Shipped configuration reverts to cell merge
            // with the check off entirely -- both the fitted grid and the
            // second amendment's pairwise vote it replaced are gone from
            // this path, not merely disabled, per the amendment's own
            // pre-decided fallback text. Mono recall/prop FP land on the
            // "merge only" row (94.8%/1.62%) to the pixel; ACCOUNT
            // STATEMENT fusion under this exact configuration was not
            // previously isolated and reads 29 of 55 -- worse than the
            // second amendment's vote (4 of 55, what shipped immediately
            // before this amendment), better than the third amendment's
            // fitted grid (37 of 55), and an accepted, disclosed loss
            // either way: the vote itself failed its own mono-recall gate
            // (66.6% vs required 88.3%) and was never a candidate to keep.
            // `docs/measurements/2026-09-22_fixed_pitch_spaces.txt`,
            // section "Fitted grid".
            pitch_grid_check: 0,
        },
        segment: Segment {
            max_merge: 3,
            max_merge_x_heights: 1.8,
            split_min_x_heights: 1.15,
            max_splits: 3,
            valley_fraction: 0.5,
            min_piece_x_heights: 0.2,
        },
        matching: Matching { top_k: 5 },
        confidence: Confidence { lm_floor: 0.8 },
        decode: Decode {
            w_match: 1.0,
            char_bonus: 3.44,
            w_bigram: 0.1,
            w_lex: 0.6,
            w_seg: 0.25,
            lex_bonus: [1.0, 0.85, 0.7, 0.55, 0.4],
            identifier_digit_fraction: 0.2,
            identifier_min_length: 2,
            seg_ideal_aspect: 0.6,
            seg_aspect_tolerance: 0.55,
            seg_merge_penalty: 0.5,
            seg_split_penalty: 0.75,
            w_confusion: 1.0,
            case_shape_penalty: 3.44,
            beam_width: 24,
        },
    };

    /// Overrides from a `params` table, returning how many rows were applied.
    ///
    /// A malformed table leaves `self` untouched and reports `None`: a
    /// half-applied parameter block is a configuration nobody authored and
    /// nobody could reproduce, so it is all or nothing.
    ///
    /// **A row this build cannot honour — an unknown name, or a value it
    /// refuses — makes the whole block malformed.** `ARCHITECTURE.md` section
    /// 7 draws the line: an unknown *table* is skipped because the reader was
    /// never going to consume it, but an unknown *parameter row* exists to
    /// override a default, so dropping it does not fall back to no argument,
    /// it substitutes a different one and reads the page by a rule the file
    /// does not describe.
    pub fn apply(&mut self, bytes: &[u8]) -> Option<usize> {
        let mut staged = *self;
        let mut applied = 0usize;
        if bytes.len() < 12 || &bytes[..4] != MAGIC {
            return None;
        }
        if u16::from_le_bytes([bytes[4], bytes[5]]) != VERSION {
            return None;
        }
        let count = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        let mut at = 12usize;
        for _ in 0..count {
            if at + 3 > bytes.len() {
                return None;
            }
            let name_len = bytes[at] as usize;
            let tag = bytes[at + 1];
            at += 2;
            if at + name_len + 4 > bytes.len() {
                return None;
            }
            let name = core::str::from_utf8(&bytes[at..at + name_len]).ok()?;
            at += name_len;
            let raw = [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]];
            at += 4;
            let hit = match tag {
                0 => staged.set_f32(name, f32::from_le_bytes(raw)),
                1 => staged.set_u32(name, u32::from_le_bytes(raw)),
                _ => return None,
            };
            if !hit {
                return None;
            }
            applied += 1;
        }
        *self = staged;
        Some(applied)
    }

    /// Sets one named `f32` threshold, returning whether the name was known.
    ///
    /// Public so the tuning harness can measure a candidate value for a
    /// threshold `model/params.tsv` still labels a guess, without rewriting
    /// a model file per point of a sweep. A shipped reader never calls it:
    /// the parameters a page is read with are the ones in its model file.
    pub fn set_f32(&mut self, name: &str, v: f32) -> bool {
        if !v.is_finite() {
            return false;
        }
        let slot: &mut f32 = match name {
            "binarize.k" => &mut self.binarize.k,
            "binarize.r" => &mut self.binarize.r,
            "deskew.max_slope" => &mut self.deskew.max_slope,
            "deskew.min_corrected_slope" => &mut self.deskew.min_corrected_slope,
            "lines.overlap_fraction" => &mut self.lines.overlap_fraction,
            "lines.furniture_fraction" => &mut self.lines.furniture_fraction,
            "lines.rule_aspect" => &mut self.lines.rule_aspect,
            "lines.mark_height_fraction" => &mut self.lines.mark_height_fraction,
            "lines.mark_reach_fraction" => &mut self.lines.mark_reach_fraction,
            "lines.descender_fraction" => &mut self.lines.descender_fraction,
            "lines.descender_reach_fraction" => &mut self.lines.descender_reach_fraction,
            "lines.x_height_per_cap" => &mut self.lines.x_height_per_cap,
            "lines.x_height_floor_per_cap" => &mut self.lines.x_height_floor_per_cap,
            "lines.inherit_x_height_below" => &mut self.lines.inherit_x_height_below,
            "lines.column_gap_heights" => &mut self.lines.column_gap_heights,
            "lines.baseline_split_sep" => &mut self.lines.baseline_split_sep,
            "lines.baseline_split_support" => &mut self.lines.baseline_split_support,
            "lines.rule_run_heights" => &mut self.lines.rule_run_heights,
            "lines.debris_heights" => &mut self.lines.debris_heights,
            "lines.thin_debris_heights" => &mut self.lines.thin_debris_heights,
            "words.min_separability" => &mut self.words.min_separability,
            "words.lone_gap_x_heights" => &mut self.words.lone_gap_x_heights,
            "words.no_valley_x_heights" => &mut self.words.no_valley_x_heights,
            "words.min_valley_x_heights" => &mut self.words.min_valley_x_heights,
            "words.pitch_agreement" => &mut self.words.pitch_agreement,
            "words.pitch_tolerance" => &mut self.words.pitch_tolerance,
            "segment.max_merge_x_heights" => &mut self.segment.max_merge_x_heights,
            "segment.split_min_x_heights" => &mut self.segment.split_min_x_heights,
            "segment.valley_fraction" => &mut self.segment.valley_fraction,
            "segment.min_piece_x_heights" => &mut self.segment.min_piece_x_heights,
            "confidence.lm_floor" => &mut self.confidence.lm_floor,
            "decode.w_match" => &mut self.decode.w_match,
            "decode.char_bonus" => &mut self.decode.char_bonus,
            "decode.w_bigram" => &mut self.decode.w_bigram,
            "decode.w_lex" => &mut self.decode.w_lex,
            "decode.w_seg" => &mut self.decode.w_seg,
            "decode.lex_bonus_tier1" => &mut self.decode.lex_bonus[0],
            "decode.lex_bonus_tier2" => &mut self.decode.lex_bonus[1],
            "decode.lex_bonus_tier3" => &mut self.decode.lex_bonus[2],
            "decode.lex_bonus_tier4" => &mut self.decode.lex_bonus[3],
            "decode.lex_bonus_tier5" => &mut self.decode.lex_bonus[4],
            "decode.identifier_digit_fraction" => &mut self.decode.identifier_digit_fraction,
            "decode.seg_ideal_aspect" => &mut self.decode.seg_ideal_aspect,
            "decode.seg_aspect_tolerance" => &mut self.decode.seg_aspect_tolerance,
            "decode.seg_merge_penalty" => &mut self.decode.seg_merge_penalty,
            "decode.seg_split_penalty" => &mut self.decode.seg_split_penalty,
            "decode.w_confusion" => &mut self.decode.w_confusion,
            "decode.case_shape_penalty" => &mut self.decode.case_shape_penalty,
            _ => return false,
        };
        *slot = v;
        true
    }

    /// Sets one named `u32` threshold. See [`Params::set_f32`].
    pub fn set_u32(&mut self, name: &str, v: u32) -> bool {
        let slot: &mut u32 = match name {
            "binarize.window" => &mut self.binarize.window,
            "lines.min_area" => &mut self.lines.min_area,
            "lines.column_lone_guard" => &mut self.lines.column_lone_guard,
            "lines.baseline_split" => &mut self.lines.baseline_split,
            "lines.underline_strip" => &mut self.lines.underline_strip,
            "words.min_gaps" => &mut self.words.min_gaps,
            "words.pitch_min_glyphs" => &mut self.words.pitch_min_glyphs,
            "words.pitch_cell_merge" => &mut self.words.pitch_cell_merge,
            "words.pitch_grid_check" => &mut self.words.pitch_grid_check,
            "segment.max_merge" => &mut self.segment.max_merge,
            "segment.max_splits" => &mut self.segment.max_splits,
            "match.top_k" => &mut self.matching.top_k,
            "decode.identifier_min_length" => &mut self.decode.identifier_min_length,
            "decode.beam_width" => &mut self.decode.beam_width,
            _ => return false,
        };
        *slot = v;
        true
    }

    /// Every name this build understands, for a loader that wants to report
    /// which ones a file left at their defaults.
    pub const NAMES: [&'static str; 62] = [
        "binarize.window",
        "binarize.k",
        "binarize.r",
        "deskew.max_slope",
        "deskew.min_corrected_slope",
        "lines.min_area",
        "lines.overlap_fraction",
        "lines.furniture_fraction",
        "lines.rule_aspect",
        "lines.mark_height_fraction",
        "lines.mark_reach_fraction",
        "lines.descender_fraction",
        "lines.descender_reach_fraction",
        "lines.x_height_per_cap",
        "lines.x_height_floor_per_cap",
        "lines.inherit_x_height_below",
        "lines.column_gap_heights",
        "lines.column_lone_guard",
        "lines.baseline_split",
        "lines.baseline_split_sep",
        "lines.baseline_split_support",
        "lines.rule_run_heights",
        "lines.debris_heights",
        "lines.thin_debris_heights",
        "lines.underline_strip",
        "words.min_gaps",
        "words.min_separability",
        "words.lone_gap_x_heights",
        "words.no_valley_x_heights",
        "words.min_valley_x_heights",
        "words.pitch_min_glyphs",
        "words.pitch_agreement",
        "words.pitch_tolerance",
        "words.pitch_cell_merge",
        "words.pitch_grid_check",
        "segment.max_merge",
        "segment.max_merge_x_heights",
        "segment.split_min_x_heights",
        "segment.max_splits",
        "segment.valley_fraction",
        "segment.min_piece_x_heights",
        "match.top_k",
        "confidence.lm_floor",
        "decode.w_match",
        "decode.char_bonus",
        "decode.w_bigram",
        "decode.w_lex",
        "decode.w_seg",
        "decode.lex_bonus_tier1",
        "decode.lex_bonus_tier2",
        "decode.lex_bonus_tier3",
        "decode.lex_bonus_tier4",
        "decode.lex_bonus_tier5",
        "decode.identifier_digit_fraction",
        "decode.identifier_min_length",
        "decode.seg_ideal_aspect",
        "decode.seg_aspect_tolerance",
        "decode.seg_merge_penalty",
        "decode.seg_split_penalty",
        "decode.w_confusion",
        "decode.case_shape_penalty",
        "decode.beam_width",
    ];

    /// The value a name currently holds, as an `f32`. For a report, and for
    /// the build-side test that compares this against `model/params.tsv`.
    pub fn get(&self, name: &str) -> Option<f32> {
        let mut probe = *self;
        // Reading through the setters keeps one name-to-slot table rather
        // than two that could disagree about which field a name means.
        let sentinel = 12_345.678f32;
        if probe.set_f32(name, sentinel) {
            let mut mirror = *self;
            let _ = mirror.set_f32(name, sentinel);
            return Some(find_changed_f32(self, &mirror));
        }
        if probe.set_u32(name, 4_242) {
            let mut mirror = *self;
            let _ = mirror.set_u32(name, 4_242);
            return Some(find_changed_u32(self, &mirror));
        }
        None
    }
}

/// Finds the one `f32` field that differs, and returns the value it had
/// before. The fields are enumerated once here rather than in a second
/// name-to-slot table.
fn find_changed_f32(before: &Params, after: &Params) -> f32 {
    let b = f32_fields(before);
    let a = f32_fields(after);
    for i in 0..b.len() {
        if b[i].to_bits() != a[i].to_bits() {
            return b[i];
        }
    }
    f32::NAN
}

fn find_changed_u32(before: &Params, after: &Params) -> f32 {
    let b = u32_fields(before);
    let a = u32_fields(after);
    for i in 0..b.len() {
        if b[i] != a[i] {
            return b[i] as f32;
        }
    }
    f32::NAN
}

fn f32_fields(p: &Params) -> [f32; 48] {
    [
        p.binarize.k,
        p.binarize.r,
        p.deskew.max_slope,
        p.deskew.min_corrected_slope,
        p.lines.overlap_fraction,
        p.lines.furniture_fraction,
        p.lines.rule_aspect,
        p.lines.mark_height_fraction,
        p.lines.mark_reach_fraction,
        p.lines.descender_fraction,
        p.lines.descender_reach_fraction,
        p.lines.x_height_per_cap,
        p.lines.x_height_floor_per_cap,
        p.lines.inherit_x_height_below,
        p.lines.column_gap_heights,
        p.lines.baseline_split_sep,
        p.lines.baseline_split_support,
        p.lines.rule_run_heights,
        p.lines.debris_heights,
        p.lines.thin_debris_heights,
        p.words.min_separability,
        p.words.lone_gap_x_heights,
        p.words.no_valley_x_heights,
        p.words.min_valley_x_heights,
        p.words.pitch_agreement,
        p.words.pitch_tolerance,
        p.segment.max_merge_x_heights,
        p.segment.split_min_x_heights,
        p.segment.valley_fraction,
        p.segment.min_piece_x_heights,
        p.confidence.lm_floor,
        p.decode.w_match,
        p.decode.char_bonus,
        p.decode.w_bigram,
        p.decode.w_lex,
        p.decode.w_seg,
        p.decode.lex_bonus[0],
        p.decode.lex_bonus[1],
        p.decode.lex_bonus[2],
        p.decode.lex_bonus[3],
        p.decode.lex_bonus[4],
        p.decode.identifier_digit_fraction,
        p.decode.seg_ideal_aspect,
        p.decode.seg_aspect_tolerance,
        p.decode.seg_merge_penalty,
        p.decode.seg_split_penalty,
        p.decode.w_confusion,
        p.decode.case_shape_penalty,
    ]
}

fn u32_fields(p: &Params) -> [u32; 14] {
    [
        p.binarize.window,
        p.lines.min_area,
        p.lines.column_lone_guard,
        p.lines.baseline_split,
        p.lines.underline_strip,
        p.words.min_gaps,
        p.words.pitch_min_glyphs,
        p.words.pitch_cell_merge,
        p.words.pitch_grid_check,
        p.segment.max_merge,
        p.segment.max_splits,
        p.matching.top_k,
        p.decode.identifier_min_length,
        p.decode.beam_width,
    ]
}

/// Stage parameter blocks, built from the loaded thresholds.
///
/// Each pipeline stage keeps its own `Params` struct, because a stage has to
/// be testable without a model file. These converters are the one place the
/// engine's loaded thresholds are mapped onto them — if a stage read the
/// numbers itself, a model's parameter block and the code it is meant to
/// configure could disagree with nothing reporting it.
impl Params {
    pub fn binarize(&self) -> crate::image::binarize::Params {
        crate::image::binarize::Params {
            window: self.binarize.window,
            k: self.binarize.k,
            r: self.binarize.r,
            auto_polarity: true,
        }
    }

    pub fn lines(&self) -> crate::layout::lines::Params {
        crate::layout::lines::Params {
            min_area: self.lines.min_area,
            overlap_fraction: self.lines.overlap_fraction,
            furniture_fraction: self.lines.furniture_fraction,
            rule_aspect: self.lines.rule_aspect,
            mark_height_fraction: self.lines.mark_height_fraction,
            mark_reach_fraction: self.lines.mark_reach_fraction,
            descender_fraction: self.lines.descender_fraction,
            descender_reach_fraction: self.lines.descender_reach_fraction,
            x_height_per_cap: self.lines.x_height_per_cap,
            x_height_floor_per_cap: self.lines.x_height_floor_per_cap,
            inherit_x_height_below: self.lines.inherit_x_height_below,
            column_gap_heights: self.lines.column_gap_heights,
            column_lone_guard: self.lines.column_lone_guard != 0,
            baseline_split: self.lines.baseline_split != 0,
            baseline_split_sep: self.lines.baseline_split_sep,
            baseline_split_support: self.lines.baseline_split_support,
            rule_run_heights: self.lines.rule_run_heights,
            debris_heights: self.lines.debris_heights,
            thin_debris_heights: self.lines.thin_debris_heights,
            underline_strip: self.lines.underline_strip != 0,
        }
    }

    pub fn words(&self) -> crate::layout::words::Params {
        crate::layout::words::Params {
            min_gaps: self.words.min_gaps as usize,
            min_separability: f64::from(self.words.min_separability),
            lone_gap_x_heights: self.words.lone_gap_x_heights,
            no_valley_x_heights: self.words.no_valley_x_heights,
            min_valley_x_heights: self.words.min_valley_x_heights,
            pitch_min_glyphs: self.words.pitch_min_glyphs as usize,
            pitch_agreement: f64::from(self.words.pitch_agreement),
            pitch_tolerance: self.words.pitch_tolerance,
            pitch_cell_merge: self.words.pitch_cell_merge != 0,
            pitch_grid_check: self.words.pitch_grid_check != 0,
        }
    }

    pub fn segment(&self) -> crate::layout::segment::Params {
        crate::layout::segment::Params {
            max_merge: self.segment.max_merge as usize,
            max_merge_x_heights: self.segment.max_merge_x_heights,
            split_min_x_heights: self.segment.split_min_x_heights,
            max_splits: self.segment.max_splits as usize,
            valley_fraction: self.segment.valley_fraction,
            min_piece_x_heights: self.segment.min_piece_x_heights,
        }
    }
}

impl Default for Params {
    fn default() -> Self {
        Params::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[(&str, u8, [u8; 4])]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&VERSION.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&(rows.len() as u32).to_le_bytes());
        for (name, tag, raw) in rows {
            b.push(name.len() as u8);
            b.push(*tag);
            b.extend_from_slice(name.as_bytes());
            b.extend_from_slice(raw);
        }
        b
    }

    #[test]
    fn a_file_overrides_the_default_by_name() {
        let mut p = Params::DEFAULT;
        let t = table(&[("decode.w_bigram", 0, 0.9f32.to_le_bytes()), ("match.top_k", 1, 9u32.to_le_bytes())]);
        assert_eq!(p.apply(&t), Some(2));
        assert_eq!(p.decode.w_bigram, 0.9);
        assert_eq!(p.matching.top_k, 9);
        // Everything it did not name is untouched.
        assert_eq!(p.decode.w_lex, Params::DEFAULT.decode.w_lex);
    }

    /// A name a newer file carries and this build does not know is a row this
    /// build cannot honour. Ignoring it does not leave the engine where it was
    /// — it leaves the engine reading by a default the file overrode, which is
    /// a wrong answer that loads cleanly. Refusing does make every added
    /// parameter a breaking change for an older reader, and that is correct:
    /// an older reader genuinely cannot run this file.
    #[test]
    fn an_unknown_name_refuses_the_whole_block() {
        let mut p = Params::DEFAULT;
        let t = table(&[("decode.w_lex", 0, 0.1f32.to_le_bytes()), ("future.thing", 0, 1.0f32.to_le_bytes())]);
        assert_eq!(p.apply(&t), None);
        assert_eq!(p, Params::DEFAULT, "a refused block leaves nothing half-applied");
    }

    /// Half a parameter block is a configuration nobody authored.
    #[test]
    fn a_malformed_table_changes_nothing() {
        let good = table(&[("decode.w_lex", 0, 0.1f32.to_le_bytes())]);
        let mut p = Params::DEFAULT;
        assert_eq!(p.apply(&good[..good.len() - 2]), None);
        assert_eq!(p, Params::DEFAULT);

        let mut bad = good.clone();
        bad[0] = b'X';
        assert_eq!(p.apply(&bad), None);
        assert_eq!(p, Params::DEFAULT);

        // A value the setter refuses is a row that cannot be honoured, so it
        // refuses the block for the same reason an unknown name does.
        let mut nan = Params::DEFAULT;
        let t = table(&[("decode.w_lex", 0, f32::NAN.to_le_bytes())]);
        assert_eq!(nan.apply(&t), None);
        assert_eq!(nan, Params::DEFAULT);
    }

    #[test]
    fn every_declared_name_reads_back_its_own_value() {
        let p = Params::DEFAULT;
        for name in Params::NAMES {
            let v = p.get(name).unwrap_or_else(|| panic!("{name} has no slot"));
            assert!(v.is_finite(), "{name} read back as {v}");
        }
        // Spot-checks that the name reaches the right slot, one f32 and one
        // integer-typed row. Compared against the field rather than a literal:
        // what the value *should* be is asserted once, by the build's check
        // that `model/params.tsv` and `Params::DEFAULT` agree, and a second
        // copy here would only ever go stale in the direction of being edited
        // to match.
        assert_eq!(p.get("decode.w_bigram"), Some(p.decode.w_bigram));
        assert_eq!(p.get("match.top_k"), Some(p.matching.top_k as f32));
        assert_eq!(p.get("nothing.here"), None);
    }
}
