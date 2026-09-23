//! Combining-mark geometry and composition, not authoring: the 53 composed
//! Latin-1 forms `glyphs()` returns are each an authored base plus one of
//! the seven marks below, at an offset chosen per base. A hand-drawn
//! duplicate base could drift from the real one, so `Ä` is `A` plus
//! [`diaeresis`] at an offset rather than a second hand-drawn `A` — the
//! same reasoning `upper::letter_o_slash` uses for its slash. It also means
//! the eventual TTF emitter gets composite glyphs for free, which is how
//! TrueType stores accented forms natively.

use std::collections::HashMap;

use super::super::{p, Glyph, Seg, Stroke, CAP, P, STROKE, X_HEIGHT};
use super::{lower, ring, upper};

/// `codepoint -> Glyph` for every authored `A`-`Z`/`a`-`z`, built once per
/// [`glyphs()`] call so composition reads the real letterform rather than a
/// second hand-drawn copy of it (`CLAUDE.md` rule 4).
fn base_map() -> HashMap<u32, Glyph> {
    let mut m = HashMap::new();
    for g in upper::glyphs().into_iter().chain(lower::glyphs()) {
        m.insert(g.codepoint, g);
    }
    m
}

/// `i` with no dot, for `ì í î ï` to compose onto: [`lower::dotless_i_stem`]
/// alone, not `lower::letter_i`'s `Glyph` with its last stroke dropped —
/// slicing `strokes` would be a silent dependency on stroke order, so this
/// rebuilds the two non-stroke fields explicitly instead.
fn dotless_i(bases: &HashMap<u32, Glyph>) -> Glyph {
    let i = &bases[&('i' as u32)];
    Glyph { codepoint: i.codepoint, advance: i.advance, strokes: vec![lower::dotless_i_stem()] }
}

// ---- mark resting heights ----
//
// Every mark clears its base by roughly one stroke width of white: base ink
// top sits at `CAP + STROKE/2` (or `X_HEIGHT + STROKE/2` in the x-height
// band), and the target for the mark's own lowest ink is one `STROKE`
// beyond that, i.e. `CAP + 1.5*STROKE`. Each mark function's own shape puts
// its lowest ink at a different offset from the `y` it is given —
// circumflex and diaeresis rest their low point exactly on `y`; grave and
// acute dip `10` past it; tilde rests its trough's centreline on `y`, so its
// rim hangs `STROKE/2` below; the ring hangs its rim
// `RING_R + STROKE/2` below its centre — so `y` is solved per mark to land on that same
// target rather than reusing one number across differently-shaped marks.
// Checked against the rendered forms in the report, not just this
// arithmetic.

const CAP_GRAVE_ACUTE_Y: i16 = CAP + 2 * STROKE - 10;
const CAP_CIRCUMFLEX_DIAERESIS_Y: i16 = CAP + 2 * STROKE;
const CAP_TILDE_Y: i16 = CAP + 2 * STROKE;
const CAP_RING_Y: i16 = CAP + 2 * STROKE + RING_R;

const X_GRAVE_ACUTE_Y: i16 = X_HEIGHT + 2 * STROKE - 10;
const X_CIRCUMFLEX_DIAERESIS_Y: i16 = X_HEIGHT + 2 * STROKE;
const X_TILDE_Y: i16 = X_HEIGHT + 2 * STROKE;
const X_RING_Y: i16 = X_HEIGHT + 2 * STROKE + RING_R;

/// Cedilla hangs from the baseline itself, not from a height above it.
const BASELINE: i16 = 0;

// ---- horizontal centres of each base's own ink (path coordinates, before
// the pen's half-width padding, which is symmetric enough not to move the
// centre) ----

const CX_CAP_A: i16 = 235; // apex of the A, which is also its bbox centre (0..470)
const CX_CAP_C: i16 = 215; // bbox 0..430
const CX_CAP_E: i16 = 200; // bbox 0..400
const CX_CAP_I: i16 = 70; // bbox 0..140
const CX_CAP_N: i16 = 235; // bbox 0..470
const CX_CAP_O: i16 = 245; // bbox 0..490
const CX_CAP_U: i16 = 235; // bbox 0..470
const CX_CAP_Y: i16 = 235; // bbox 0..470

const CX_LOW_A: i16 = 170; // bowl centre, bbox 0..340
const CX_LOW_C: i16 = 170; // bbox 0..340
const CX_LOW_E: i16 = 170; // bbox 0..340
const CX_LOW_I: i16 = 35; // bare stem centreline
const CX_LOW_N: i16 = 205; // stem-to-arch bbox 35..375
const CX_LOW_O: i16 = 170; // bbox 0..340
const CX_LOW_U: i16 = 205; // stem-to-stem bbox 35..375
const CX_LOW_Y: i16 = 160; // bbox 0..320

pub fn glyphs() -> Vec<Glyph> {
    let bases = base_map();
    let get = |cp: char| bases[&(cp as u32)].clone();
    let dot_i = dotless_i(&bases);

    vec![
        // A
        compose(&get('A'), 0x00C0, grave(CX_CAP_A, CAP_GRAVE_ACUTE_Y)),
        compose(&get('A'), 0x00C1, acute(CX_CAP_A, CAP_GRAVE_ACUTE_Y)),
        compose(&get('A'), 0x00C2, circumflex(CX_CAP_A, CAP_CIRCUMFLEX_DIAERESIS_Y)),
        compose(&get('A'), 0x00C3, tilde(CX_CAP_A, CAP_TILDE_Y)),
        compose(&get('A'), 0x00C4, diaeresis(CX_CAP_A, CAP_CIRCUMFLEX_DIAERESIS_Y)),
        compose(&get('A'), 0x00C5, combining_ring(CX_CAP_A, CAP_RING_Y)),
        // C
        compose(&get('C'), 0x00C7, cedilla(CX_CAP_C, BASELINE)),
        // E
        compose(&get('E'), 0x00C8, grave(CX_CAP_E, CAP_GRAVE_ACUTE_Y)),
        compose(&get('E'), 0x00C9, acute(CX_CAP_E, CAP_GRAVE_ACUTE_Y)),
        compose(&get('E'), 0x00CA, circumflex(CX_CAP_E, CAP_CIRCUMFLEX_DIAERESIS_Y)),
        compose(&get('E'), 0x00CB, diaeresis(CX_CAP_E, CAP_CIRCUMFLEX_DIAERESIS_Y)),
        // I (already dotless: capital I is drawn serifed, not with a dot)
        compose(&get('I'), 0x00CC, grave(CX_CAP_I, CAP_GRAVE_ACUTE_Y)),
        compose(&get('I'), 0x00CD, acute(CX_CAP_I, CAP_GRAVE_ACUTE_Y)),
        compose(&get('I'), 0x00CE, circumflex(CX_CAP_I, CAP_CIRCUMFLEX_DIAERESIS_Y)),
        compose(&get('I'), 0x00CF, diaeresis(CX_CAP_I, CAP_CIRCUMFLEX_DIAERESIS_Y)),
        // N
        compose(&get('N'), 0x00D1, tilde(CX_CAP_N, CAP_TILDE_Y)),
        // O
        compose(&get('O'), 0x00D2, grave(CX_CAP_O, CAP_GRAVE_ACUTE_Y)),
        compose(&get('O'), 0x00D3, acute(CX_CAP_O, CAP_GRAVE_ACUTE_Y)),
        compose(&get('O'), 0x00D4, circumflex(CX_CAP_O, CAP_CIRCUMFLEX_DIAERESIS_Y)),
        compose(&get('O'), 0x00D5, tilde(CX_CAP_O, CAP_TILDE_Y)),
        compose(&get('O'), 0x00D6, diaeresis(CX_CAP_O, CAP_CIRCUMFLEX_DIAERESIS_Y)),
        // U
        compose(&get('U'), 0x00D9, grave(CX_CAP_U, CAP_GRAVE_ACUTE_Y)),
        compose(&get('U'), 0x00DA, acute(CX_CAP_U, CAP_GRAVE_ACUTE_Y)),
        compose(&get('U'), 0x00DB, circumflex(CX_CAP_U, CAP_CIRCUMFLEX_DIAERESIS_Y)),
        compose(&get('U'), 0x00DC, diaeresis(CX_CAP_U, CAP_CIRCUMFLEX_DIAERESIS_Y)),
        // Y
        compose(&get('Y'), 0x00DD, acute(CX_CAP_Y, CAP_GRAVE_ACUTE_Y)),
        // a
        compose(&get('a'), 0x00E0, grave(CX_LOW_A, X_GRAVE_ACUTE_Y)),
        compose(&get('a'), 0x00E1, acute(CX_LOW_A, X_GRAVE_ACUTE_Y)),
        compose(&get('a'), 0x00E2, circumflex(CX_LOW_A, X_CIRCUMFLEX_DIAERESIS_Y)),
        compose(&get('a'), 0x00E3, tilde(CX_LOW_A, X_TILDE_Y)),
        compose(&get('a'), 0x00E4, diaeresis(CX_LOW_A, X_CIRCUMFLEX_DIAERESIS_Y)),
        compose(&get('a'), 0x00E5, combining_ring(CX_LOW_A, X_RING_Y)),
        // c
        compose(&get('c'), 0x00E7, cedilla(CX_LOW_C, BASELINE)),
        // e
        compose(&get('e'), 0x00E8, grave(CX_LOW_E, X_GRAVE_ACUTE_Y)),
        compose(&get('e'), 0x00E9, acute(CX_LOW_E, X_GRAVE_ACUTE_Y)),
        compose(&get('e'), 0x00EA, circumflex(CX_LOW_E, X_CIRCUMFLEX_DIAERESIS_Y)),
        compose(&get('e'), 0x00EB, diaeresis(CX_LOW_E, X_CIRCUMFLEX_DIAERESIS_Y)),
        // dotless i
        compose(&dot_i, 0x00EC, grave(CX_LOW_I, X_GRAVE_ACUTE_Y)),
        compose(&dot_i, 0x00ED, acute(CX_LOW_I, X_GRAVE_ACUTE_Y)),
        compose(&dot_i, 0x00EE, circumflex(CX_LOW_I, X_CIRCUMFLEX_DIAERESIS_Y)),
        compose(&dot_i, 0x00EF, diaeresis(CX_LOW_I, X_CIRCUMFLEX_DIAERESIS_Y)),
        // n
        compose(&get('n'), 0x00F1, tilde(CX_LOW_N, X_TILDE_Y)),
        // o
        compose(&get('o'), 0x00F2, grave(CX_LOW_O, X_GRAVE_ACUTE_Y)),
        compose(&get('o'), 0x00F3, acute(CX_LOW_O, X_GRAVE_ACUTE_Y)),
        compose(&get('o'), 0x00F4, circumflex(CX_LOW_O, X_CIRCUMFLEX_DIAERESIS_Y)),
        compose(&get('o'), 0x00F5, tilde(CX_LOW_O, X_TILDE_Y)),
        compose(&get('o'), 0x00F6, diaeresis(CX_LOW_O, X_CIRCUMFLEX_DIAERESIS_Y)),
        // u
        compose(&get('u'), 0x00F9, grave(CX_LOW_U, X_GRAVE_ACUTE_Y)),
        compose(&get('u'), 0x00FA, acute(CX_LOW_U, X_GRAVE_ACUTE_Y)),
        compose(&get('u'), 0x00FB, circumflex(CX_LOW_U, X_CIRCUMFLEX_DIAERESIS_Y)),
        compose(&get('u'), 0x00FC, diaeresis(CX_LOW_U, X_CIRCUMFLEX_DIAERESIS_Y)),
        // y
        compose(&get('y'), 0x00FD, acute(CX_LOW_Y, X_GRAVE_ACUTE_Y)),
        compose(&get('y'), 0x00FF, diaeresis(CX_LOW_Y, X_CIRCUMFLEX_DIAERESIS_Y)),
    ]
}

/// Clones `base`, retargets it at `codepoint`, and appends `mark`'s
/// strokes — the same clone-and-extend idiom `upper::letter_o_slash` uses
/// for its slash.
pub(super) fn compose(base: &Glyph, codepoint: u32, mark: Vec<Stroke>) -> Glyph {
    let mut g = base.clone();
    g.codepoint = codepoint;
    g.strokes.extend(mark);
    g
}

/// `` ` `` grave: a short stroke sloping from high-left down to low-right,
/// centred over `cx` with its low point at `y`.
pub(super) fn grave(cx: i16, y: i16) -> Vec<Stroke> {
    vec![Stroke::line(&[p(cx - 40, y + 90), p(cx + 20, y + 10)])]
}

/// `´` acute: the mirror of [`grave`], sloping from low-left up to
/// high-right.
pub(super) fn acute(cx: i16, y: i16) -> Vec<Stroke> {
    vec![Stroke::line(&[p(cx - 20, y + 10), p(cx + 40, y + 90)])]
}

/// `^` circumflex: two lines meeting at a peak above `cx`, resting on `y`.
pub(super) fn circumflex(cx: i16, y: i16) -> Vec<Stroke> {
    vec![Stroke::line(&[p(cx - 40, y), p(cx, y + 70), p(cx + 40, y)])]
}

/// `~` tilde: one period of a sine, walked as a polyline.
///
/// The amplitude is the whole glyph. A pen of width [`STROKE`] lays down ink
/// `STROKE / 2` either side of the centreline, so a wave whose crest and
/// trough are less than `STROKE` apart puts its rising ink and its falling
/// ink in the same pixels and rasterises as a filled diagonal lozenge with no
/// wave in it — indistinguishable from a sloped macron, and one bad
/// segmentation away from a circumflex. The first draft separated crest from
/// trough by `35`, exactly half a pen, and did precisely that at every size
/// tested; `TILDE_SWING` is now one and a half pens.
///
/// Two quadratics were tried first and are the reason this is a polyline: a
/// quadratic reaches only halfway to its control point, so a swing this large
/// needs controls far outside the mark, and those gave the left lobe a spike
/// and the right lobe a flat. Sampling the curve directly costs a dozen
/// points and puts the crest and trough exactly where they are specified.
///
/// The ends land at mid height, level with each other, which is where a
/// printed tilde puts them.
pub(super) fn tilde(cx: i16, y: i16) -> Vec<Stroke> {
    /// Crest-to-trough separation of the centreline.
    const TILDE_SWING: f64 = 1.3 * STROKE as f64;
    const TILDE_HALF_W: f64 = 110.0;
    /// Enough that the flattened corners are under a third of a pixel at any
    /// size this face is rendered at.
    const STEPS: usize = 12;

    let amp = TILDE_SWING / 2.0;
    let pts: Vec<P> = (0..=STEPS)
        .map(|i| {
            let t = i as f64 / STEPS as f64;
            let x = -TILDE_HALF_W + 2.0 * TILDE_HALF_W * t;
            let dy = amp + amp * (core::f64::consts::TAU * t).sin();
            p(cx + x.round() as i16, y + dy.round() as i16)
        })
        .collect();
    vec![Stroke::line(&pts)]
}

/// `¨` diaeresis: two dots straddling `cx` at height `y`.
pub(super) fn diaeresis(cx: i16, y: i16) -> Vec<Stroke> {
    vec![Stroke::dot(p(cx - 45, y)), Stroke::dot(p(cx + 45, y))]
}

/// `˚` combining ring, e.g. for `Å`: built from the same [`ring`] helper
/// every bowl in the face uses, at a fraction of `letter_o`'s radius —
/// the write-once rule applies to marks too, not only to letters.
pub(super) fn combining_ring(cx: i16, y: i16) -> Vec<Stroke> {
    vec![ring(cx, y, RING_R, RING_R)]
}

/// Ring radius for `Å`/`å`, set by what has to survive rasterisation rather
/// than by the drawing.
///
/// The pen is [`STROKE`] wide, so a loop of radius `r` leaves an open
/// interior only `2 * (r - STROKE / 2)` across — and that interior is the
/// entire feature, the one thing separating `Å` from `A` in the hole count
/// that prunes before any distance is computed. At `r = 45` the interior is
/// `20` units, a fifth of a pen: it survives a 64px render and closes at
/// 48px and below, so the glyph silently becomes an `A` at exactly the
/// sizes 300dpi body text arrives at. `70` gives an interior one full pen
/// wide and an outer diameter of `210`, which is also where a real face
/// puts it — about three tenths of the cap height.
const RING_R: i16 = 70;

/// `¸` cedilla, for `ç`-style forms: a small hook dropping below `y`
/// (ordinarily the base glyph's baseline) into descender territory.
pub(super) fn cedilla(cx: i16, y: i16) -> Vec<Stroke> {
    vec![Stroke { start: p(cx - 10, y), segs: vec![Seg::Quad { ctrl: p(cx + 30, y - 20), to: p(cx - 5, y - 60) }] }]
}
