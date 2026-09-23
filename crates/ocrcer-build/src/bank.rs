//! Builds the prototype bank: render every class from every usable face,
//! extract, standardise.
//!
//! # Contract
//!
//! Deterministic. Faces are visited in `model/fonts.tsv` order, classes in
//! `model/charset.tsv` order, sizes in the order given, and every number
//! comes from `ocrcer_core::feature::extract` — this crate owns no second
//! extractor (`CLAUDE.md` rule 4). Two runs on the same inputs produce the
//! same prototypes in the same order.
//!
//! Standardisation constants are computed **from the bank itself**, so they
//! are a property of the file rather than a constant in Rust source, and a
//! bank and its constants cannot be paired wrongly.
//!
//! Quantisation and the `.ocrw` container are not here.

use ocrcer_core::feature::{extract, holes_of, GlyphInput, FEATURE_DIMS};

use crate::face::raster::{self, Raster};
use crate::face::{Glyph, UPM, X_HEIGHT};
use crate::tables::{Class, Distribution, FontEntry};

/// The authored ISO 3098 face, which is stroke data compiled into this crate
/// rather than a file on disk, so it has no `fonts.tsv` row to be loaded from.
/// It is always face 0 of a bank.
pub const AUTHORED_FAMILY: &str = "OCRcer Technical";

/// The authored face is this project's own work and carries the project's
/// licence. It is named here rather than in `fonts.tsv` because it has no
/// row there: it is drawn in `face/glyphs`, not read from a file.
pub const AUTHORED_LICENCE: &str = "MIT";
pub const AUTHORED_LICENCE_SOURCE: &str = "this repository: LICENSE";
use crate::ttf_load;

/// A face the bank can render from.
pub enum Renderer<'a> {
    /// The authored ISO 3098 face, drawn from stroke data rather than a file.
    Authored(Vec<Glyph>),
    /// A parsed font file. Boxed because a parsed face is two orders of
    /// magnitude larger than a glyph list, and the bank holds one `Renderer`
    /// per face in a `Vec`.
    File(Box<ttf_load::Face<'a>>),
}

impl Renderer<'_> {
    pub fn render(&self, c: char, px_per_em: f32) -> Option<Raster> {
        match self {
            Renderer::Authored(glyphs) => {
                let g = glyphs.iter().find(|g| g.codepoint == c as u32)?;
                raster::render(g, px_per_em)
            }
            Renderer::File(f) => f.render(c, px_per_em),
        }
    }

    /// The line x-height in pixels this face implies at `px_per_em` — the
    /// context the extractor's baseline-relative dimensions are measured
    /// against.
    pub fn x_height_px(&self, px_per_em: f32) -> Option<f32> {
        match self {
            Renderer::Authored(_) => Some(X_HEIGHT as f32 * px_per_em / UPM as f32),
            Renderer::File(f) => f.x_height_px(px_per_em),
        }
    }
}

/// One rendered, extracted glyph.
#[derive(Clone)]
pub struct Prototype {
    /// `charset.tsv` index.
    pub class: u16,
    /// Index into the bank's face list.
    pub face: u16,
    pub px_per_em: u16,
    pub features: [f32; FEATURE_DIMS],
}

/// A face as the bank records it: enough to regenerate the prototype and to
/// state its provenance in the container's manifest.
#[derive(Clone, Debug)]
pub struct BankFace {
    pub family: String,
    pub style: String,
    pub distribution: Distribution,
    /// Licence identifier and where it was read from, carried through to
    /// `meta.faces` because the model file travels without this repository
    /// (`ARCHITECTURE.md` section 7.1).
    pub licence: String,
    pub licence_source: String,
}

/// Which of `ARCHITECTURE.md` section 4.1's pruning steps the matcher
/// applies. A mode, rather than a fixed policy, because which of these is
/// safe is a measurement and not a preference: a gate exists to save work,
/// and one that also removes the correct class has cost accuracy to buy
/// speed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    /// No pruning. Every prototype is measured.
    None,
    /// Step 1 only: exact hole-count match.
    Holes,
    /// Step 1, plus aspect and baseline-relative extent **measured** from the
    /// class's own prototypes.
    Measured,
}

impl Gate {
    pub fn parse(s: &str) -> Option<Gate> {
        Some(match s {
            "none" => Gate::None,
            "holes" => Gate::Holes,
            "measured" => Gate::Measured,
            _ => return None,
        })
    }
}

/// The coarse gate a class presents to the matcher.
///
/// Every band here is **measured from the class's own prototypes**, in the
/// extractor's own quantities. `charset.tsv`'s authored width-over-cap-height
/// column is deliberately absent: it is a different denominator from dim 103
/// and gating on it prunes the correct class away. See section 11's
/// 2026-09-22 denominator entry.
#[derive(Clone, Copy, Debug)]
pub struct ClassGate {
    /// Bit `n` set when a prototype of this class was measured with `n` holes.
    pub holes: u8,
    /// Measured `(min, max)` of the aspect feature.
    pub aspect: (f32, f32),
    /// Measured `(min, max)` of height above the baseline, in x-heights.
    pub above: (f32, f32),
    /// Measured `(min, max)` of depth below the baseline, in x-heights.
    pub below: (f32, f32),
}

impl ClassGate {
    fn empty() -> ClassGate {
        ClassGate {
            holes: 0,
            aspect: (f32::INFINITY, f32::NEG_INFINITY),
            above: (f32::INFINITY, f32::NEG_INFINITY),
            below: (f32::INFINITY, f32::NEG_INFINITY),
        }
    }

    fn widen(&mut self, f: &[f32; FEATURE_DIMS]) {
        self.holes |= 1 << holes_of(f);
        for (band, v) in [
            (&mut self.aspect, f[103]),
            (&mut self.above, f[105]),
            (&mut self.below, f[106]),
        ] {
            band.0 = band.0.min(v);
            band.1 = band.1.max(v);
        }
    }

    /// Whether a query could be this class under `gate`. A class with no
    /// prototypes admits nothing under any gate but `Gate::None`.
    pub fn admits(&self, f: &[f32; FEATURE_DIMS], gate: Gate) -> bool {
        if gate == Gate::None {
            return true;
        }
        if self.holes & (1 << holes_of(f)) == 0 {
            return false;
        }
        match gate {
            Gate::Holes | Gate::None => true,
            Gate::Measured => {
                (self.aspect.0..=self.aspect.1).contains(&f[103])
                    && (self.above.0..=self.above.1).contains(&f[105])
                    && (self.below.0..=self.below.1).contains(&f[106])
            }
        }
    }
}

/// The built bank, before quantisation.
pub struct Bank {
    pub faces: Vec<BankFace>,
    pub prototypes: Vec<Prototype>,
    /// Per-dimension mean and standard deviation over every prototype.
    pub mean: [f32; FEATURE_DIMS],
    pub sd: [f32; FEATURE_DIMS],
    /// `prototypes`' features standardised by `mean`/`sd`, in the same
    /// order. Held rather than recomputed: matching is the hot loop and
    /// standardising inside it would do the same divisions on every query.
    pub standardised: Vec<[f32; FEATURE_DIMS]>,
    /// One gate per class, indexed by `charset.tsv` index.
    pub gates: Vec<ClassGate>,
    /// Classes with no prototype in any face, by `charset.tsv` index.
    pub uncovered: Vec<u16>,
    /// Classes with no prototype in any **shippable** face. A non-empty list
    /// here is a build error for a shipping bank: dropping the optional
    /// segments would leave the engine blind to these characters rather than
    /// merely worse at them.
    pub uncovered_in_base: Vec<u16>,
}

/// Font-file bytes, held so the `Renderer::File` values that parse them have
/// something to borrow from. Load once, then call `renderers`.
pub struct Fonts {
    loaded: Vec<(BankFace, Vec<u8>)>,
    /// Rows that named a file this machine does not have, or whose bytes
    /// could not be read, as `family/style: reason`. Coverage data for the
    /// build report, not an error: the inventory is deliberately wider than
    /// any one machine.
    pub absent: Vec<String>,
}

impl Fonts {
    /// Reads every row whose distribution permits it and whose file exists.
    ///
    /// `include_local_only` decides whether `local-only` faces are read at
    /// all: a bank built for distribution must be built without them, and
    /// the difference between the two builds is exactly what
    /// `uncovered_in_base` reports.
    pub fn load(entries: &[FontEntry], include_local_only: bool) -> Fonts {
        let mut loaded = Vec::new();
        let mut absent = Vec::new();
        for e in entries {
            if !e.distribution.usable(include_local_only) {
                continue;
            }
            let Some(path) = e.file() else {
                absent.push(format!("{}/{}: {}", e.family, e.style, e.status));
                continue;
            };
            match std::fs::read(&path) {
                Ok(data) => loaded.push((
                    BankFace {
                        family: e.family.clone(),
                        style: e.style.clone(),
                        distribution: e.distribution,
                        licence: e.licence.clone(),
                        licence_source: e.licence_source.clone(),
                    },
                    data,
                )),
                Err(err) => absent.push(format!("{}/{}: {err}", e.family, e.style)),
            }
        }
        Fonts { loaded, absent }
    }

    /// The authored face followed by every font file that parses, in
    /// `fonts.tsv` order. A file that fails to parse is reported rather than
    /// silently dropped, because a face missing from the bank is invisible in
    /// an accuracy number.
    pub fn renderers(&self) -> (Vec<BankFace>, Vec<Renderer<'_>>, Vec<String>) {
        let mut faces = vec![BankFace {
            family: AUTHORED_FAMILY.to_string(),
            style: "Regular".to_string(),
            distribution: Distribution::Shippable,
            licence: AUTHORED_LICENCE.to_string(),
            licence_source: AUTHORED_LICENCE_SOURCE.to_string(),
        }];
        let mut renderers = vec![Renderer::Authored(crate::face::glyphs::glyphs())];
        let mut failed = Vec::new();
        for (face, data) in &self.loaded {
            match ttf_load::Face::parse(data, 0) {
                Ok(f) => {
                    faces.push(face.clone());
                    renderers.push(Renderer::File(Box::new(f)));
                }
                Err(e) => failed.push(format!("{}/{}: {e}", face.family, face.style)),
            }
        }
        (faces, renderers, failed)
    }
}

/// Renders `classes` from `renderers` at every size in `sizes`.
///
/// `renderers` is parallel to `faces`. A face that lacks a class simply
/// contributes no prototype for it; that is coverage data, not an error.
///
/// `classes` may be a subset of the charset — a size sweep over a handful of
/// classes is a useful measurement. What it may not be is *renumbered*:
/// `Class::index` is the class identity written into every prototype and
/// every `.ocrw` file, so the per-class tables here are indexed by that,
/// not by position in `classes`.
pub fn build(
    classes: &[Class],
    faces: &[BankFace],
    renderers: &[Renderer<'_>],
    sizes: &[f32],
) -> Bank {
    assert_eq!(faces.len(), renderers.len(), "faces and renderers must be parallel");

    let slots = classes.iter().map(|c| usize::from(c.index)).max().map_or(0, |m| m + 1);
    let mut prototypes = Vec::new();
    let mut covered = vec![false; slots];
    let mut covered_in_base = vec![false; slots];
    let mut gates = vec![ClassGate::empty(); slots];
    for c in classes {
        gates[usize::from(c.index)] = ClassGate::empty();
    }

    for (fi, renderer) in renderers.iter().enumerate() {
        let shippable = faces[fi].distribution == Distribution::Shippable;
        for class in classes {
            for &px in sizes {
                let Some(r) = renderer.render(class.codepoint, px) else {
                    continue;
                };
                let Some(x_height) = renderer.x_height_px(px).filter(|x| *x > 0.0) else {
                    continue;
                };
                let features = extract(&GlyphInput {
                    ink: &r.ink,
                    width: r.width,
                    height: r.height,
                    baseline_dy: r.baseline_dy,
                    x_height,
                });
                gates[usize::from(class.index)].widen(&features);
                covered[usize::from(class.index)] = true;
                covered_in_base[usize::from(class.index)] |= shippable;
                prototypes.push(Prototype {
                    class: class.index,
                    face: fi as u16,
                    px_per_em: px as u16,
                    features,
                });
            }
        }
    }

    let (mean, sd) = moments(&prototypes);
    let standardised = prototypes.iter().map(|p| standardise(&p.features, &mean, &sd)).collect();
    Bank {
        faces: faces.to_vec(),
        prototypes,
        mean,
        sd,
        standardised,
        gates,
        uncovered: uncovered_indices(classes, &covered),
        uncovered_in_base: uncovered_indices(classes, &covered_in_base),
    }
}

fn standardise(
    features: &[f32; FEATURE_DIMS],
    mean: &[f32; FEATURE_DIMS],
    sd: &[f32; FEATURE_DIMS],
) -> [f32; FEATURE_DIMS] {
    let mut out = [0.0f32; FEATURE_DIMS];
    for d in 0..FEATURE_DIMS {
        out[d] = (features[d] - mean[d]) / sd[d];
    }
    out
}

fn uncovered_indices(classes: &[Class], covered: &[bool]) -> Vec<u16> {
    classes
        .iter()
        .filter(|c| !covered[usize::from(c.index)])
        .map(|c| c.index)
        .collect()
}

/// Per-dimension mean and standard deviation, accumulated in `f64` and
/// narrowed once, for the reason `ARCHITECTURE.md` section 8.2 gives for the
/// decoder: an `f32` running sum over ~12,000 prototypes loses low bits in an
/// order-dependent way, and these constants have to come out the same on
/// every machine that rebuilds the bank.
///
/// A dimension with no variation gets `sd = 1.0`, not `0.0`: standardising
/// by zero is a division by zero, and a constant dimension carries no
/// information to scale in the first place.
fn moments(protos: &[Prototype]) -> ([f32; FEATURE_DIMS], [f32; FEATURE_DIMS]) {
    let mut mean = [0.0f32; FEATURE_DIMS];
    let mut sd = [1.0f32; FEATURE_DIMS];
    if protos.is_empty() {
        return (mean, sd);
    }
    let n = protos.len() as f64;
    for d in 0..FEATURE_DIMS {
        let mut sum = 0.0f64;
        for p in protos {
            sum += f64::from(p.features[d]);
        }
        let m = sum / n;
        let mut var = 0.0f64;
        for p in protos {
            let e = f64::from(p.features[d]) - m;
            var += e * e;
        }
        let s = (var / n).sqrt();
        mean[d] = m as f32;
        sd[d] = if s > 0.0 { s as f32 } else { 1.0 };
    }
    (mean, sd)
}

impl Bank {
    /// `features` standardised by this bank's own constants.
    pub fn standardise(&self, features: &[f32; FEATURE_DIMS]) -> [f32; FEATURE_DIMS] {
        standardise(features, &self.mean, &self.sd)
    }

    /// Nearest prototype by squared L2 over standardised features, returning
    /// `(class, d1, d2)` — the winner, its distance, and the distance to the
    /// best prototype of a *different* class, which is the margin section 4.2
    /// builds confidence from.
    ///
    /// Ties break by lowest class index, matching the decoder's rule in
    /// `ARCHITECTURE.md` section 8.2, so the same bank gives the same answer
    /// on x86 and wasm32.
    ///
    /// `d2` is `f32::INFINITY` when the bank holds only one class.
    pub fn nearest(&self, features: &[f32; FEATURE_DIMS], gate: Gate) -> Option<(u16, f32, f32)> {
        self.search(features, gate)
            .or_else(|| self.search(features, Gate::None))
    }

    /// `nearest` under exactly the gate given, with no fallback.
    fn search(&self, features: &[f32; FEATURE_DIMS], gate: Gate) -> Option<(u16, f32, f32)> {
        let q = self.standardise(features);
        let admitted: Vec<bool> = self
            .gates
            .iter()
            .map(|g| g.admits(features, gate))
            .collect();
        let mut best: Option<(u16, f32)> = None;
        let mut rival = f32::INFINITY;
        for (p, s) in self.prototypes.iter().zip(&self.standardised) {
            if !admitted[usize::from(p.class)] {
                continue;
            }
            let mut d = 0.0f32;
            for k in 0..FEATURE_DIMS {
                let e = q[k] - s[k];
                d += e * e;
            }
            match best {
                Some((bc, bd)) if !(d < bd || (d == bd && p.class < bc)) => {
                    // Not a new winner; it can still be the nearest rival.
                    if p.class != bc && d < rival {
                        rival = d;
                    }
                }
                Some((bc, bd)) => {
                    // A new winner. The old winner's distance is the best
                    // any other class has shown unless it was the same class,
                    // and it is never worse than the rival already held.
                    if p.class != bc {
                        rival = bd;
                    }
                    best = Some((p.class, d));
                }
                None => best = Some((p.class, d)),
            }
        }
        let (class, d1) = best?;
        Some((class, d1, rival))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::{load_charset, model_dir};

    /// A handful of classes, the authored face only: enough to exercise the
    /// contracts without spending a font load on each assertion.
    fn small() -> (Vec<Class>, Vec<BankFace>, Vec<Renderer<'static>>) {
        let all = load_charset(&model_dir()).unwrap();
        let classes: Vec<Class> = all
            .into_iter()
            .filter(|c| "oi8HxX".contains(c.codepoint))
            .collect();
        let faces = vec![BankFace {
            family: AUTHORED_FAMILY.into(),
            style: "Regular".into(),
            distribution: Distribution::Shippable,
            licence: AUTHORED_LICENCE.into(),
            licence_source: AUTHORED_LICENCE_SOURCE.into(),
        }];
        let renderers = vec![Renderer::Authored(crate::face::glyphs::glyphs())];
        (classes, faces, renderers)
    }

    /// The bank is a build artifact that has to reproduce byte for byte, so
    /// the same inputs must give the same vectors in the same order. A
    /// difference here would mean a rebuild silently invalidates every
    /// fixture blessed against the previous one.
    #[test]
    fn two_builds_of_the_same_inputs_agree_exactly() {
        let (classes, faces, renderers) = small();
        let a = build(&classes, &faces, &renderers, &[16.0, 32.0]);
        let b = build(&classes, &faces, &renderers, &[16.0, 32.0]);
        assert_eq!(a.prototypes.len(), b.prototypes.len());
        assert!(!a.prototypes.is_empty(), "no prototypes built");
        for (x, y) in a.prototypes.iter().zip(&b.prototypes) {
            assert_eq!((x.class, x.face, x.px_per_em), (y.class, y.face, y.px_per_em));
            assert_eq!(x.features, y.features);
        }
        assert_eq!(a.mean, b.mean);
        assert_eq!(a.sd, b.sd);
    }

    /// Standardising by a zero standard deviation is a division by zero, and
    /// a dimension that never varies carries nothing to scale.
    #[test]
    fn a_dimension_that_never_varies_gets_unit_scale_not_zero() {
        let one = Prototype {
            class: 0,
            face: 0,
            px_per_em: 16,
            features: [7.0; FEATURE_DIMS],
        };
        let (mean, sd) = moments(&[one.clone(), one]);
        assert_eq!(mean[0], 7.0);
        assert_eq!(sd[0], 1.0);
    }

    /// `d2` is the margin section 4.2 builds confidence from: it must come
    /// from a class other than the winner, never from a second prototype of
    /// the winning class.
    #[test]
    fn the_rival_distance_comes_from_a_different_class() {
        let (classes, faces, renderers) = small();
        let b = build(&classes, &faces, &renderers, &[24.0, 32.0]);
        for p in &b.prototypes {
            let (class, d1, d2) = b.nearest(&p.features, Gate::None).unwrap();
            assert_eq!(class, p.class, "a prototype must match its own class");
            assert_eq!(d1, 0.0, "a prototype is at zero distance from itself");
            assert!(d2 > 0.0, "rival distance from the same class");
            assert!(d2.is_finite(), "more than one class in the bank");
        }
    }

    /// A class every face declines to render is coverage information, not a
    /// crash — and it must be reported, because a silently missing class is
    /// invisible until an accuracy number moves.
    #[test]
    fn a_class_no_face_renders_is_reported_uncovered() {
        let all = load_charset(&model_dir()).unwrap();
        let classes: Vec<Class> = all
            .into_iter()
            .filter(|c| "o\u{2030}".contains(c.codepoint))
            .collect();
        assert_eq!(classes.len(), 2, "test needs both classes present");
        let faces = vec![BankFace {
            family: AUTHORED_FAMILY.into(),
            style: "Regular".into(),
            distribution: Distribution::Shippable,
            licence: AUTHORED_LICENCE.into(),
            licence_source: AUTHORED_LICENCE_SOURCE.into(),
        }];
        let renderers = vec![Renderer::Authored(vec![])];
        let b = build(&classes, &faces, &renderers, &[32.0]);
        assert!(b.prototypes.is_empty());
        assert_eq!(b.uncovered.len(), 2);
        assert_eq!(b.uncovered_in_base, b.uncovered);
    }

    /// The gate may narrow the search but must never leave it empty: a class
    /// that cannot be matched at all is worse than one matched slowly.
    #[test]
    fn a_gate_that_admits_nothing_falls_back_to_the_whole_bank() {
        let (classes, faces, renderers) = small();
        let mut b = build(&classes, &faces, &renderers, &[32.0]);
        for g in &mut b.gates {
            g.holes = 0;
        }
        let q = b.prototypes[0].features;
        assert!(b.search(&q, Gate::Holes).is_none(), "gate should admit nothing");
        assert!(b.nearest(&q, Gate::Holes).is_some(), "fallback did not fire");
    }
}
