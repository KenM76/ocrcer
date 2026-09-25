//! Style-conditioned classification, diagnostic only (Sarkar & Nagy, IEEE
//! PAMI 27(1) 2005, eq. 14): for a field of glyphs known to share one style,
//! sum each candidate face's best-match distance over the field and keep the
//! face whose sum is lowest, then label every glyph in the field by its own
//! nearest prototype within that face. Nothing here is read by `run_bank`,
//! `run_write`, or any emitted table.
//!
//! # Distance
//!
//! Standardised, **unweighted** squared L2 over `bank.rs`'s own prototypes —
//! the same metric `Bank::nearest`/`run_bank` report against, not the shipped
//! runtime's weighted matcher (`ocrcer_core::r#match`). That matcher only
//! returns one winner per call; recomputing it once per candidate face per
//! query would multiply this diagnostic's cost by the face count for no
//! change in which metric is being measured. The caveat is repeated in the
//! measurement note this module's caller writes.
//!
//! # Determinism
//!
//! Field order comes from a fixed-seed LCG shuffle, not an RNG crate, so two
//! runs over the same bank produce the same fields. Every tie -- within a
//! face, across faces for the singlet classifier, and across faces for the
//! style classifier's own face choice -- breaks by a stated, fixed rule.

use crate::bank::{Bank, Gate};
use ocrcer_core::feature::FEATURE_DIMS;

/// Fixed seed for the field-order shuffle. Arbitrary; fixed so two runs over
/// the same bank produce the same fields.
pub const SHUFFLE_SEED: u64 = 0x5EED_0BED_15A0_CAFE;

/// A face's best (distance, class) against one query, or `None` when the
/// gate admits no prototype of that face for this glyph's hole count.
pub type FaceBest = Option<(f32, u16)>;

/// One eval glyph, scored against every face in the bank, ready to be
/// grouped into a field.
#[derive(Clone)]
pub struct Glyph {
    pub truth: u16,
    pub per_face: Vec<FaceBest>,
}

struct Lcg(u64);

impl Lcg {
    /// One step of a 64-bit LCG (the Knuth MMIX constants); good enough
    /// mixing for a shuffle, and needs no crate.
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
}

/// Deterministically reorders `items` in place (Fisher-Yates over a
/// fixed-seed LCG), so re-slicing the same group at a different field length
/// still comes from one reproducible order.
pub fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut rng = Lcg(seed);
    for i in (1..items.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

/// Every prototype's standardised squared-L2 distance folded into a running
/// best per face, under `gate`. Ties within a face break to the lower class
/// index, matching `Bank::search`'s rule, so combining faces afterward (see
/// [`singlet`]) reproduces it exactly.
fn score(bank: &Bank, raw: &[f32; FEATURE_DIMS], gate: Gate) -> Vec<FaceBest> {
    let q = bank.standardise(raw);
    let admitted: Vec<bool> = bank.gates.iter().map(|g| g.admits(raw, gate)).collect();
    let mut best: Vec<FaceBest> = vec![None; bank.faces.len()];
    for (p, s) in bank.prototypes.iter().zip(&bank.standardised) {
        if !admitted[usize::from(p.class)] {
            continue;
        }
        let mut d = 0.0f32;
        for k in 0..FEATURE_DIMS {
            let e = q[k] - s[k];
            d += e * e;
        }
        let slot = &mut best[usize::from(p.face)];
        let better = match slot {
            Some((bd, bc)) => d < *bd || (d == *bd && p.class < *bc),
            None => true,
        };
        if better {
            *slot = Some((d, p.class));
        }
    }
    best
}

/// [`score`] under `gate`, falling back to [`Gate::None`] when it admits
/// nothing anywhere in the bank -- the same fallback `Bank::nearest` makes,
/// so a glyph is never left unclassified by the gate alone.
pub fn per_face_best(bank: &Bank, raw: &[f32; FEATURE_DIMS], gate: Gate) -> Vec<FaceBest> {
    let scored = score(bank, raw, gate);
    if gate != Gate::None && scored.iter().all(Option::is_none) {
        score(bank, raw, Gate::None)
    } else {
        scored
    }
}

/// The singlet (global 1-NN) classifier: the best-of-bests among `faces`,
/// ties breaking to the lower class index. Combining every face's own best
/// (itself tie-broken the same way) reproduces a flat scan over the whole
/// set, because a minimum-of-minimums is associative under one fixed order.
pub fn singlet(per_face: &[FaceBest], faces: impl Iterator<Item = usize>) -> Option<u16> {
    let mut best: Option<(f32, u16)> = None;
    for k in faces {
        if let Some((d, c)) = per_face[k] {
            best = Some(match best {
                Some((bd, bc)) if !(d < bd || (d == bd && c < bc)) => (bd, bc),
                _ => (d, c),
            });
        }
    }
    best.map(|(_, c)| c)
}

/// Labels one field under the style-conditioned (LS) classifier: sums each
/// candidate face's distance over the field, keeps the lowest sum (ties to
/// the lowest face index), then labels every glyph by its own best match
/// within that face. `excluded`, when set, removes one face from every
/// candidate sum -- the leave-one-face-out simulation of "the true font is
/// not in the bank". A face missing even one glyph's coverage (that glyph's
/// `per_face[k]` is `None`) cannot be the field's style at all.
///
/// Returns the chosen face index (`None` when no face covers the whole
/// field) and one label per glyph, in field order.
pub fn label_field(field: &[Glyph], n_faces: usize, excluded: Option<u16>) -> (Option<usize>, Vec<Option<u16>>) {
    let mut sum = vec![0.0f64; n_faces];
    let mut viable = vec![true; n_faces];
    for k in 0..n_faces {
        if Some(k as u16) == excluded {
            viable[k] = false;
            continue;
        }
        for g in field {
            match g.per_face[k] {
                Some((d, _)) => sum[k] += f64::from(d),
                None => {
                    viable[k] = false;
                    break;
                }
            }
        }
    }
    let mut k_star: Option<usize> = None;
    for k in 0..n_faces {
        if !viable[k] {
            continue;
        }
        k_star = Some(match k_star {
            Some(bk) if sum[k] >= sum[bk] => bk,
            _ => k,
        });
    }
    match k_star {
        None => (None, vec![None; field.len()]),
        Some(k) => (Some(k), field.iter().map(|g| g.per_face[k].map(|(_, c)| c)).collect()),
    }
}

/// The mandatory sanity check: with a field of exactly one glyph, LS's own
/// face-first tie-break and the singlet classifier's class-first tie-break
/// must still land on the same label, because a one-glyph field's sum is
/// just that glyph's own per-face distance. Returns `(singlet, ls)` so a
/// caller that finds them unequal can report which glyph and how.
pub fn l1_agreement(per_face: &[FaceBest], excluded: Option<u16>) -> (Option<u16>, Option<u16>) {
    let n_faces = per_face.len();
    let glyph = Glyph { truth: 0, per_face: per_face.to_vec() };
    let (_, labels) = label_field(std::slice::from_ref(&glyph), n_faces, excluded);
    let faces = (0..n_faces).filter(|&k| Some(k as u16) != excluded);
    (singlet(per_face, faces), labels[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::{self, BankFace, Renderer};
    use crate::tables::{load_charset, model_dir, Distribution};

    /// Two faces that render identically (the authored glyph set, loaded
    /// twice): every prototype ties exactly between them, which is the
    /// ordinary, benign tie a real bank produces (same class, same
    /// distance) rather than the adversarial cross-class tie the next test
    /// constructs by hand.
    fn duplicated_authored_bank() -> Bank {
        let all = load_charset(&model_dir()).unwrap();
        let classes: Vec<_> = all.into_iter().filter(|c| "oi8H".contains(c.codepoint)).collect();
        let face = BankFace {
            family: bank::AUTHORED_FAMILY.into(),
            style: "Regular".into(),
            distribution: Distribution::Shippable,
            licence: bank::AUTHORED_LICENCE.into(),
            licence_source: bank::AUTHORED_LICENCE_SOURCE.into(),
        };
        let faces = vec![face.clone(), face];
        let renderers = vec![
            Renderer::Authored(crate::face::glyphs::glyphs()),
            Renderer::Authored(crate::face::glyphs::glyphs()),
        ];
        bank::build(&classes, &faces, &renderers, &[16.0, 32.0])
    }

    /// Sanity check 1 (`ARCHITECTURE.md`-adjacent task spec, chunk
    /// style-probe): on ordinary data, a one-glyph field must classify the
    /// same way under LS as under the singlet classifier, in both the
    /// in-bank and leave-one-face-out conditions.
    #[test]
    fn l1_ls_equals_singlet_in_both_conditions() {
        let b = duplicated_authored_bank();
        for p in &b.prototypes {
            let per_face = per_face_best(&b, &p.features, Gate::Holes);
            let (single, ls) = l1_agreement(&per_face, None);
            assert_eq!(single, ls, "in-bank: LS and singlet must agree at L=1");
            let (single_loo, ls_loo) = l1_agreement(&per_face, Some(0));
            assert_eq!(single_loo, ls_loo, "leave-one-out: LS and singlet must agree at L=1");
        }
    }

    /// The two classifiers break ties by *different* rules on purpose: LS
    /// picks the lower face index so it can name a style even when nothing
    /// else distinguishes two candidates; the singlet classifier picks the
    /// lower class index, matching `Bank::search`. This is the adversarial
    /// case sanity check 1 does not exercise: two faces at the exact same
    /// summed distance but disagreeing on which class won.
    #[test]
    fn ls_and_singlet_break_ties_by_different_rules() {
        // face 0 says class 9 at distance 1.0, face 1 says class 5 at the
        // same distance: LS keeps face 0 (lowest face index) and reports 9;
        // singlet ignores which face and keeps the lower class, 5.
        let per_face: Vec<FaceBest> = vec![Some((1.0, 9)), Some((1.0, 5))];
        let glyph = Glyph { truth: 0, per_face: per_face.clone() };
        let (k_star, labels) = label_field(std::slice::from_ref(&glyph), 2, None);
        assert_eq!(k_star, Some(0), "LS ties break to the lower face index");
        assert_eq!(labels, vec![Some(9)]);

        let single = singlet(&per_face, 0..2);
        assert_eq!(single, Some(5), "singlet ties break to the lower class index");
    }

    /// A face missing coverage for even one glyph in the field cannot be the
    /// field's style at all, matching "if face k has no admitted prototype
    /// for some glyph, d_k = +inf" from the task spec.
    #[test]
    fn a_face_that_cannot_see_every_glyph_in_a_field_is_never_chosen() {
        let field = vec![
            Glyph { truth: 0, per_face: vec![Some((0.5, 0)), Some((0.1, 0))] },
            Glyph { truth: 1, per_face: vec![Some((0.5, 1)), None] },
        ];
        let (k_star, labels) = label_field(&field, 2, None);
        assert_eq!(k_star, Some(0), "face 1 is missing glyph 1 and must be excluded");
        assert_eq!(labels, vec![Some(0), Some(1)]);
    }

    /// Excluding the only viable face leaves the field unclassifiable rather
    /// than silently falling back to a different one.
    #[test]
    fn excluding_the_only_viable_face_yields_no_labels() {
        let field = vec![Glyph { truth: 0, per_face: vec![Some((0.5, 0)), None] }];
        let (k_star, labels) = label_field(&field, 2, Some(0));
        assert_eq!(k_star, None);
        assert_eq!(labels, vec![None]);
    }

    /// The shuffle is a pure function of the seed: two calls over equal
    /// inputs must produce equal orders, which is what lets a group be
    /// shuffled once and re-sliced at every field length.
    #[test]
    fn the_shuffle_is_deterministic() {
        let mut a: Vec<u32> = (0..50).collect();
        let mut b = a.clone();
        shuffle(&mut a, SHUFFLE_SEED);
        shuffle(&mut b, SHUFFLE_SEED);
        assert_eq!(a, b);
        assert_ne!(a, (0..50).collect::<Vec<u32>>(), "a 50-item shuffle landing on the identity is not credible");
    }
}
