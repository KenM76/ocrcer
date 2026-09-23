//! OCRcer Technical — a single-stroke technical lettering face, authored as
//! stroke data rather than imported as a font file.
//!
//! # Why this exists
//!
//! The prototype bank needs at least one face covering every class in
//! `model/charset.tsv`, and it needs that face to carry a licence the project
//! can state in one sentence. The two candidate ISO 3098 faces on this machine
//! fail that test — one is GPL-with-font-exception, the other's licence could
//! not be established — and a class with no prototype is a class the engine is
//! blind to, not one it reads badly. Authoring the face removes both problems
//! at once.
//!
//! # The contract
//!
//! A glyph is a list of **pen strokes**. A stroke is the path the *centre* of a
//! circular pen follows; the pen's diameter is [`STROKE`] for every stroke in
//! the face, which is what makes this a single-stroke design and what ISO 3098
//! specifies. There are no filled regions and no variable widths anywhere.
//!
//! All coordinates are **integers in design units**, with the baseline at
//! `y = 0` and `y` increasing upward. Integers are deliberate: every number in
//! this face has to be explainable by a sentence and reproducible byte-for-byte
//! by anyone who re-runs the build (`CLAUDE.md` rule 1), and a float coordinate
//! invites a value that came from nowhere.
//!
//! Curves are quadratic Béziers only. This is not a stylistic choice — it keeps
//! flattening to polynomial arithmetic with no trigonometry, so rasterisation is
//! bit-reproducible on any target, and it is also exactly what a TrueType
//! outline stores, so the TTF emitter writes these control points through
//! almost unchanged.
//!
//! # Metrics
//!
//! ISO 3098 Type B, expressed against a character height `h` of [`CAP`]:
//! line thickness `d = h/10`, lowercase body `0.7h`, ascenders reaching full
//! `h`, descenders `0.3h` below the baseline, and a gap of `2d` between
//! adjacent characters.

pub mod emit_ttf;
pub mod glyphs;
pub mod raster;

/// Units per em. Everything below is expressed in these.
pub const UPM: i16 = 1000;

/// Character height `h` — cap height, and the height ascenders reach.
pub const CAP: i16 = 700;

/// Lowercase body height, `0.7h` per ISO 3098 Type B.
pub const X_HEIGHT: i16 = 490;

/// Descenders reach `0.3h` below the baseline.
pub const DESCENDER: i16 = -210;

/// Pen diameter `d = h/10`. Uniform across the whole face.
pub const STROKE: i16 = 70;

/// Left and right side bearing. Two of these between adjacent glyphs gives the
/// `2d` inter-character gap ISO 3098 calls for.
pub const SIDE_BEARING: i16 = STROKE;

/// A point in design units. Baseline at `y = 0`, `y` upward.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct P {
    pub x: i16,
    pub y: i16,
}

/// Shorthand for authoring stroke data.
pub const fn p(x: i16, y: i16) -> P {
    P { x, y }
}

/// One step of a pen path, continuing from wherever the pen currently is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Seg {
    /// Straight run to an absolute point.
    Line(P),
    /// Quadratic Bézier to `to`, shaped by the single control point `ctrl`.
    Quad { ctrl: P, to: P },
}

/// One continuous pen-down path. The pen lifts between strokes, so a glyph
/// drawn with two strokes has a visible break between them unless its
/// endpoints coincide.
#[derive(Clone, Debug)]
pub struct Stroke {
    pub start: P,
    /// Never empty. A stroke with no segments would be a lone dot; author that
    /// as a zero-length [`Seg::Line`] back to `start`, so the pen stamps once
    /// and the intent is explicit rather than implied by an empty list.
    pub segs: Vec<Seg>,
}

/// One character of the face.
#[derive(Clone, Debug)]
pub struct Glyph {
    /// The Unicode scalar this glyph draws. Must appear in `model/charset.tsv`.
    pub codepoint: u32,
    /// Pen advance from this glyph's origin to the next glyph's origin.
    /// Conventionally ink width plus twice [`SIDE_BEARING`].
    pub advance: i16,
    /// Drawn in order. Order does not affect the rendered result — the pen
    /// stamps the union of all strokes — but it is the order a human reads the
    /// construction in, so author it as it would be drawn by hand.
    pub strokes: Vec<Stroke>,
}

impl Stroke {
    /// A polyline through the given points. The common case.
    pub fn line(pts: &[P]) -> Stroke {
        assert!(pts.len() >= 2, "a polyline needs at least two points");
        Stroke {
            start: pts[0],
            segs: pts[1..].iter().copied().map(Seg::Line).collect(),
        }
    }

    /// A single dot: the pen set down once and lifted without moving.
    pub fn dot(at: P) -> Stroke {
        Stroke { start: at, segs: vec![Seg::Line(at)] }
    }

    /// The axis-aligned bounding box of the pen *centre* path, before the pen's
    /// own width is taken into account. Returns `(min_x, min_y, max_x, max_y)`.
    pub fn centre_bounds(&self) -> (i16, i16, i16, i16) {
        let mut b = (self.start.x, self.start.y, self.start.x, self.start.y);
        let mut acc = |q: P| {
            b.0 = b.0.min(q.x);
            b.1 = b.1.min(q.y);
            b.2 = b.2.max(q.x);
            b.3 = b.3.max(q.y);
        };
        for seg in &self.segs {
            match *seg {
                Seg::Line(q) => acc(q),
                // The control point is included deliberately. A quadratic
                // Bézier is contained in the convex hull of its three points,
                // so this over-estimates rather than clipping the curve, which
                // is the safe direction for a bound used to size a bitmap.
                Seg::Quad { ctrl, to } => {
                    acc(ctrl);
                    acc(to);
                }
            }
        }
        b
    }
}
