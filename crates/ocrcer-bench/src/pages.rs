//! Reading a generated corpus page with the oracle segmentation the
//! benchmark harness uses, and the ground truth that goes with it.
//!
//! # Why this is a module and not inlined in a binary
//!
//! Two binaries need to read a page exactly the same way — the head-to-head
//! against `ocrs`, and the charset-cost ablation — and "exactly the same
//! way" is not a thing two copies can promise. `CLAUDE.md` rule 4 is about
//! pipeline stages, and this is harness code rather than a stage, but the
//! failure mode is identical: two readers that drifted apart would produce
//! two numbers that were never comparable, and nothing would say so.
//!
//! # The oracle, stated plainly
//!
//! [`read_with_bank`] is handed the exact bounding box of every mark of ink
//! and the exact position of every space, from the page's own ground truth.
//! It does no binarisation, no component finding, no line finding and no
//! segmentation. Whatever it scores is a **ceiling** on an end-to-end
//! result, never a result. Every caller is obliged to say so.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ocrcer_build::bank;
use ocrcer_core::feature::{extract, GlyphInput};

/// Flattens an engine's line output into the string a scorer compares: words
/// joined by one space, and lines sharing a `band` (`ARCHITECTURE.md` section
/// 11, "The shipped fixed-pitch rule...") joined by one space too, with a
/// newline only where the band changes.
///
/// Here rather than in each benchmark because the end-to-end run and the
/// head-to-head against `ocrs` must flatten a page identically. A second
/// copy that joined words differently would report a spacing difference as
/// an engine error, and the two runs' numbers would never have been
/// comparable.
///
/// A band the column cut split into fragments is still one visual row; the
/// fragments arrive consecutively and left to right (`lines.rs`'s
/// `group_with_bands` settles that order before the cut), so joining them
/// with a space and withholding the newline until the band ends says exactly
/// that, and nothing else. It changes no word, no word order and no box.
pub fn page_text(lines: &[ocrcer_core::pipeline::Line]) -> String {
    let mut out = String::new();
    let mut prev_band: Option<usize> = None;
    for l in lines {
        let same_band = prev_band == Some(l.band);
        if prev_band.is_some() {
            out.push(if same_band { ' ' } else { '\n' });
        }
        let words = l.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
        out.push_str(&words);
        prev_band = Some(l.band);
    }
    out
}

/// One page's ground truth, as `ocrcer-build pages` wrote it.
pub struct Truth {
    pub family: String,
    pub px_per_em: f32,
    pub lines: Vec<String>,
    pub glyphs: Vec<TruthGlyph>,
}

/// One mark of ink, with the geometry the feature extractor needs. `x_height`
/// and `baseline` come from the typesetter rather than being measured back
/// off the page, which is part of what makes this an oracle.
pub struct TruthGlyph {
    pub ch: char,
    pub line: usize,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub baseline: u32,
    pub x_height: f32,
}

pub fn load_truth(path: &Path) -> Result<Truth, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let s = |k: &str| -> String { v[k].as_str().unwrap_or_default().to_string() };
    let glyphs = v["glyphs"]
        .as_array()
        .ok_or("truth has no glyphs array")?
        .iter()
        .map(|g| TruthGlyph {
            ch: g["ch"].as_str().and_then(|s| s.chars().next()).unwrap_or('?'),
            line: g["line"].as_u64().unwrap_or(0) as usize,
            x: g["x"].as_u64().unwrap_or(0) as u32,
            y: g["y"].as_u64().unwrap_or(0) as u32,
            w: g["w"].as_u64().unwrap_or(0) as u32,
            h: g["h"].as_u64().unwrap_or(0) as u32,
            baseline: g["baseline"].as_u64().unwrap_or(0) as u32,
            x_height: g["x_height"].as_f64().unwrap_or(0.0) as f32,
        })
        .collect();
    Ok(Truth {
        family: s("family"),
        px_per_em: v["px_per_em"].as_f64().unwrap_or(0.0) as f32,
        lines: v["lines"]
            .as_array()
            .ok_or("truth has no lines array")?
            .iter()
            .map(|l| l.as_str().unwrap_or_default().to_string())
            .collect(),
        glyphs,
    })
}

/// The `.pgm` beside a page's `.truth.json`, given the `.pgm` path.
pub fn load_page(pgm: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let bytes = std::fs::read(pgm).map_err(|e| format!("{}: {e}", pgm.display()))?;
    ocrcer_build::page::from_pgm(&bytes)
}

/// The truth file beside a page, accepting either naming the generator may
/// have produced.
pub fn load_truth_beside(pgm: &Path) -> Result<Truth, String> {
    let stem = pgm.file_stem().unwrap_or_default().to_string_lossy().to_string();
    load_truth(&pgm.with_extension("").with_extension("truth.json"))
        .or_else(|_| load_truth(&pgm.with_file_name(format!("{stem}.truth.json"))))
}

/// Every `.pgm` in a directory, sorted, so two runs visit pages in the same
/// order and their per-page reports line up.
pub fn list_pages(dir: &str) -> Result<Vec<std::path::PathBuf>, String> {
    let mut pgms: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{dir}: {e}"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "pgm"))
        .collect();
    pgms.sort();
    if pgms.is_empty() {
        return Err(format!("no .pgm pages in {dir}"));
    }
    Ok(pgms)
}

/// What one page's read produced: the text, and every character the matcher
/// got wrong, as `(expected, read)`.
pub struct Read {
    pub text: String,
    pub substitutions: Vec<(char, char)>,
}

/// Reads a page the way OCRcer can read one today: every glyph box and every
/// space handed to it, and only the classification left to do.
///
/// # `mask`, and why this takes one rather than a grey page
///
/// The caller passes an already-binarized page: one byte per pixel, non-zero
/// for ink. This used to take the grey page and threshold it at a fixed
/// `< 128`, which was exactly right while the corpus was two-valued and
/// became wrong the moment it was not. On a grey page a fixed global
/// threshold is a *worse* binarizer than the engine's own adaptive one, so
/// the column this feeds — whose entire claim is to be an upper bound on the
/// end-to-end column — stopped being one, and scored below it at two of five
/// render sizes.
///
/// The column's claim is that **layout and segmentation** are removed, not
/// that binarization is. So the caller binarizes the whole page once, with
/// the parameters its engine file carries, and hands the mask here. A caller
/// with no engine file passes the shipped defaults and is saying so.
///
/// `restrict`, when given, is the set of class indices the matcher is allowed
/// to answer with. It exists for the charset-cost ablation and must be
/// `None` for any figure reported as the engine's.
/// Binarizes a whole page the way the engine does, for the callers that hand
/// glyph boxes to [`read_with_bank`].
///
/// One call per page, not per glyph: a window that stops at a glyph's own
/// bounding box sees a different neighbourhood than the same window on the
/// page, and Sauvola's threshold is a function of that neighbourhood.
pub fn binarize_page(
    grey: &[u8],
    width: u32,
    height: u32,
    p: &ocrcer_core::params::Params,
) -> Vec<u8> {
    ocrcer_core::image::binarize::binarize_with(
        &ocrcer_core::Gray { width, height, data: grey },
        &p.binarize(),
    )
}

pub fn read_with_bank(
    b: &bank::Bank,
    index_to_char: &BTreeMap<u16, char>,
    truth: &Truth,
    mask: &[u8],
    width: u32,
    gate: bank::Gate,
    restrict: Option<&Restricted>,
) -> Read {
    let mut by_line: BTreeMap<usize, Vec<&TruthGlyph>> = BTreeMap::new();
    for g in &truth.glyphs {
        by_line.entry(g.line).or_default().push(g);
    }

    let mut substitutions = Vec::new();
    let mut out: Vec<String> = Vec::with_capacity(truth.lines.len());
    for (li, text) in truth.lines.iter().enumerate() {
        let empty = Vec::new();
        let glyphs = by_line.get(&li).unwrap_or(&empty);
        let mut next = 0usize;
        let mut line = String::new();
        for ch in text.chars() {
            match glyphs.get(next) {
                Some(g) if g.ch == ch => {
                    next += 1;
                    let mut ink = vec![0u8; (g.w * g.h) as usize];
                    for r in 0..g.h {
                        for c in 0..g.w {
                            let p = mask[((g.y + r) * width + g.x + c) as usize];
                            ink[(r * g.w + c) as usize] = u8::from(p != 0);
                        }
                    }
                    let input = GlyphInput {
                        ink: &ink,
                        width: g.w,
                        height: g.h,
                        baseline_dy: g.baseline as f32 - g.y as f32,
                        x_height: g.x_height,
                    };
                    let f = extract(&input);
                    let got = match restrict {
                        Some(r) => r.nearest(b, &f, gate),
                        None => b.nearest(&f, gate).map(|(class, _, _)| class),
                    };
                    let read = got
                        .and_then(|class| index_to_char.get(&class).copied())
                        .unwrap_or('?');
                    if read != ch {
                        substitutions.push((ch, read));
                    }
                    line.push(read);
                }
                // A character that put no ink on the page: a space, or one
                // the glyph list and the text disagree about. Passed through
                // rather than guessed at.
                _ => line.push(ch),
            }
        }
        out.push(line);
    }
    Read {
        text: out.join("\n"),
        substitutions,
    }
}

/// A class-index allowlist for the ablation, and the brute-force search that
/// honours it.
///
/// # Why the normalisation constants are deliberately *not* recomputed
///
/// A bank actually built from a narrower charset would standardise against
/// its own mean and standard deviation. This does not: it reuses the full
/// bank's, and removes only the competing prototypes. That is the right
/// control for the question being asked — *what does the presence of the
/// extra classes cost?* — because it changes exactly one thing. Rebuilding
/// the normalisation as well would confound the answer with a second effect
/// and neither could then be attributed.
///
/// The consequence to state when reporting: this measures the cost of the
/// extra *classes*, not the full difference between this bank and a bank
/// built ASCII-only.
pub struct Restricted {
    allowed: BTreeSet<u16>,
    /// Per-dimension multipliers applied to the squared difference, so a
    /// weight of `w` scales that dimension's contribution by `w`. All ones
    /// reproduces `bank::nearest`'s unweighted distance exactly.
    ///
    /// `ARCHITECTURE.md` section 4.1 step 3 says the real weights "are not
    /// yet authored"; this is the surface on which a candidate set is
    /// measured before anything is authored.
    weights: [f32; ocrcer_core::feature::FEATURE_DIMS],
}

impl Restricted {
    /// The classes whose character satisfies `keep`, unweighted.
    pub fn new(index_to_char: &BTreeMap<u16, char>, keep: impl Fn(char) -> bool) -> Restricted {
        Restricted {
            allowed: index_to_char
                .iter()
                .filter(|(_, c)| keep(**c))
                .map(|(i, _)| *i)
                .collect(),
            weights: [1.0; ocrcer_core::feature::FEATURE_DIMS],
        }
    }

    /// Multiplies one dimension group's contribution to the distance.
    ///
    /// Dimensions are standardised before the distance is taken, so every
    /// group starts at unit variance and a multiplier is the whole of the
    /// intervention — there is no scale confound to correct for.
    pub fn with_group_weight(mut self, range: std::ops::Range<usize>, w: f32) -> Restricted {
        for k in range {
            self.weights[k] = w;
        }
        self
    }

    pub fn len(&self) -> usize {
        self.allowed.len()
    }

    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }

    /// Nearest allowed class under `gate`, with `bank::nearest`'s fallback to
    /// the whole bank when the gate admits nothing.
    ///
    /// The gate decision is delegated to `ClassGate::admits` rather than
    /// reimplemented: a second copy of that predicate is exactly the drift
    /// `CLAUDE.md` rule 4 exists to prevent. Only the distance differs from
    /// `bank::search`, and only by the per-dimension weights.
    ///
    /// An ablation must pass `Gate::None` on *both* sides of its comparison —
    /// see `charset-cost`, which does — because gating and restricting
    /// interact and the run would otherwise measure two changes at once.
    fn nearest(
        &self,
        b: &bank::Bank,
        features: &[f32; ocrcer_core::feature::FEATURE_DIMS],
        gate: bank::Gate,
    ) -> Option<u16> {
        self.search(b, features, gate)
            .or_else(|| self.search(b, features, bank::Gate::None))
    }

    fn search(
        &self,
        b: &bank::Bank,
        features: &[f32; ocrcer_core::feature::FEATURE_DIMS],
        gate: bank::Gate,
    ) -> Option<u16> {
        let q = b.standardise(features);
        let admitted: Vec<bool> = b.gates.iter().map(|g| g.admits(features, gate)).collect();
        let mut best: Option<(u16, f32)> = None;
        for (p, s) in b.prototypes.iter().zip(&b.standardised) {
            if !self.allowed.contains(&p.class) || !admitted[usize::from(p.class)] {
                continue;
            }
            let mut d = 0.0f32;
            for k in 0..ocrcer_core::feature::FEATURE_DIMS {
                let e = q[k] - s[k];
                d += self.weights[k] * e * e;
            }
            match best {
                // Ties break by lowest class index, matching `bank::nearest`
                // and `ARCHITECTURE.md` section 8.2's determinism rule.
                Some((bc, bd)) if d < bd || (d == bd && p.class < bc) => best = Some((p.class, d)),
                Some(_) => {}
                None => best = Some((p.class, d)),
            }
        }
        best.map(|(c, _)| c)
    }
}

#[cfg(test)]
mod page_text_tests {
    use super::page_text;
    use ocrcer_core::pipeline::{Line, Rect, Word};

    fn word(text: &str) -> Word {
        Word { text: text.to_string(), rect: Rect::default(), confidence: 1.0, chars: Vec::new() }
    }

    fn line(band: usize, words: &[&str]) -> Line {
        Line {
            words: words.iter().map(|w| word(w)).collect(),
            rect: Rect::default(),
            baseline: 0.0,
            x_height: 0.0,
            confidence: 1.0,
            band,
        }
    }

    #[test]
    fn a_cut_band_of_two_fragments_renders_as_one_line() {
        let lines = vec![line(0, &["Name:"]), line(0, &["Jane", "Doe"]), line(1, &["Age:", "30"])];
        assert_eq!(page_text(&lines), "Name: Jane Doe\nAge: 30");
    }

    #[test]
    fn an_uncut_page_renders_byte_identically_to_before() {
        let lines = vec![line(0, &["Line", "one"]), line(1, &["Line", "two"]), line(2, &["Line", "three"])];
        assert_eq!(page_text(&lines), "Line one\nLine two\nLine three");
    }
}
