//! Turns a filled outline — a set of closed contours — into the bitmap the
//! feature extractor consumes, using the same sampling policy as the
//! authored face's pen rasteriser.
//!
//! # Why this is not a second rasteriser
//!
//! The authored face is a pen centreline: a point is ink when it lies within
//! the pen radius of the path. Every other face in the bank is a filled
//! outline: a point is ink when its winding number against the contours is
//! non-zero. Those are two different answers to "is this point inside", and
//! that difference is legitimate — it is what the two designs mean.
//!
//! Everything downstream of that question is **not** allowed to differ. The
//! pixel lattice ([`raster::Grid`]), the sub-pixel sample offsets, the
//! majority threshold, the best-sub-threshold-pixel fallback and the tight
//! crop all come from [`raster`] here, exactly as they do for the pen path.
//! A third-party rasteriser would decide all of those its own way, and the
//! prototype bank would then hold vectors measured with two different rulers
//! — the hazard `CLAUDE.md` rule 4 names for the feature extractor, one
//! level below it at the bitmap. Nothing in the file format would report the
//! disagreement; accuracy would simply be quietly worse.
//!
//! So: a font file may be **parsed** by a third-party crate, and is
//! **rasterised** here.

use crate::face::raster::{self, Grid, Raster};
#[cfg(test)]
use crate::face::{p, Seg, Stroke};

/// One contour as `glyf` stores it: on/off-curve points in order, in font
/// design units, implicitly closed back to the first point (TrueType never
/// stores the closing edge). `true` is on-curve.
pub(crate) type Contour = Vec<(i16, i16, bool)>;

/// A contour flattened to a polyline in font design units, closing edge
/// made explicit.
pub(crate) type Polyline = Vec<(f64, f64)>;

/// A directed edge `(x0, y0, x1, y1)` in scaled (pixel) design-unit space.
pub(crate) type Edge = (f64, f64, f64, f64);

/// Rebuilds the pen path a `glyf`-style point list represents, closing it
/// back to its own first point so flattening sees the closing edge.
///
/// An off-curve point is a quadratic control whose endpoint is the next
/// point in the list. This does not implement the on-curve-point *implication*
/// rule (two consecutive off-curve points implying a midpoint between them),
/// because no producer this crate reads emits one: the authored face's
/// emitter never writes consecutive off-curve points, and [`crate::ttf_load`]
/// resolves implied points while walking, before a `Contour` is built.
#[cfg(test)]
pub(crate) fn contour_to_stroke(points: &[(i16, i16, bool)]) -> Stroke {
    let (x0, y0, _) = points[0];
    let start = p(x0, y0);
    let mut segs = Vec::new();
    let mut i = 1;
    while i < points.len() {
        let (x, y, on) = points[i];
        if on {
            segs.push(Seg::Line(p(x, y)));
            i += 1;
        } else {
            let (nx, ny, _) = points[(i + 1) % points.len()];
            segs.push(Seg::Quad { ctrl: p(x, y), to: p(nx, ny) });
            i += 2;
        }
    }
    segs.push(Seg::Line(start));
    Stroke { start, segs }
}

/// Every contour flattened to a polyline in design units, at the shipped
/// [`raster::QUAD_STEPS`] resolution. Gate support, like
/// [`contour_to_stroke`].
#[cfg(test)]
pub(crate) fn flatten_contours(contours: &[Contour]) -> Vec<Polyline> {
    contours
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| raster::flatten_stroke(&contour_to_stroke(c)))
        .collect()
}

/// Every polyline's segments as scaled edges. `scale` converts design units
/// to pixels — `px_per_em / units_per_em`.
pub(crate) fn edges(polylines: &[Polyline], scale: f64) -> Vec<Edge> {
    let mut out = Vec::new();
    for poly in polylines {
        for w in poly.windows(2) {
            out.push((w[0].0 * scale, w[0].1 * scale, w[1].0 * scale, w[1].1 * scale));
        }
    }
    out
}

/// `(min_x, max_x, min_y, max_y)` over every edge endpoint. `None` for an
/// empty set.
///
/// Unlike the pen path, a filled outline needs no radius expansion: its
/// contours already *are* the ink boundary.
pub(crate) fn edge_bounds(edges: &[Edge]) -> Option<(f64, f64, f64, f64)> {
    if edges.is_empty() {
        return None;
    }
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for &(x0, y0, x1, y1) in edges {
        min_x = min_x.min(x0.min(x1));
        max_x = max_x.max(x0.max(x1));
        min_y = min_y.min(y0.min(y1));
        max_y = max_y.max(y0.max(y1));
    }
    Some((min_x, max_x, min_y, max_y))
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// A quadratic Bézier at `t`, by de Casteljau: two lerps, then a third
/// between their results.
///
/// The one quadratic evaluator in the crate. The authored face reaches it
/// through [`raster::flatten_stroke`] with i16 control points; a parsed font
/// reaches it through [`flatten_quad`] with f64 ones. Two evaluators would
/// have to agree to the last bit forever, and the only symptom of the day
/// they stopped would be prototypes shaped slightly differently from the
/// glyphs the runtime meets.
pub(crate) fn quad_point(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), t: f64) -> (f64, f64) {
    let a = (lerp(p0.0, p1.0, t), lerp(p0.1, p1.1, t));
    let b = (lerp(p1.0, p2.0, t), lerp(p1.1, p2.1, t));
    (lerp(a.0, b.0, t), lerp(a.1, b.1, t))
}

/// A cubic Bézier at `t`, by de Casteljau. Needed because CFF/CFF2 outlines
/// — every OpenType face with PostScript curves — store cubics, which no
/// `Seg` can hold.
pub(crate) fn cubic_point(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    t: f64,
) -> (f64, f64) {
    let a = quad_point(p0, p1, p2, t);
    let b = quad_point(p1, p2, p3, t);
    (lerp(a.0, b.0, t), lerp(a.1, b.1, t))
}

/// Appends a quadratic from the polyline's current last point, excluding
/// that start point, at the shipped [`raster::QUAD_STEPS`] resolution.
///
/// Fixed-step, never adaptive, for the reason [`raster::QUAD_STEPS`] gives:
/// an error-tolerance test makes the output depend on floating-point
/// rounding, and the bank has to rebuild to the same bytes on every machine.
pub(crate) fn flatten_quad(into: &mut Polyline, ctrl: (f64, f64), to: (f64, f64)) {
    let from = *into.last().expect("a curve needs a current point");
    for i in 1..=raster::QUAD_STEPS {
        let t = i as f64 / raster::QUAD_STEPS as f64;
        into.push(quad_point(from, ctrl, to, t));
    }
}

/// As [`flatten_quad`], for a cubic.
pub(crate) fn flatten_cubic(
    into: &mut Polyline,
    ctrl0: (f64, f64),
    ctrl1: (f64, f64),
    to: (f64, f64),
) {
    let from = *into.last().expect("a curve needs a current point");
    for i in 1..=raster::QUAD_STEPS {
        let t = i as f64 / raster::QUAD_STEPS as f64;
        into.push(cubic_point(from, ctrl0, ctrl1, to, t));
    }
}

fn is_left(a: (f64, f64), b: (f64, f64), q: (f64, f64)) -> f64 {
    (b.0 - a.0) * (q.1 - a.1) - (q.0 - a.0) * (b.1 - a.1)
}

/// Non-zero winding number of `q` against `edges` (Sunday's algorithm).
///
/// Non-zero rather than even-odd because that is the fill rule TrueType
/// specifies: a counter is a contour wound against its outer contour, and
/// under even-odd an overlapping pair of same-wound contours — which stroked
/// and hinted faces do emit — would punch a hole that the designer did not
/// draw.
pub(crate) fn winding_number(q: (f64, f64), edges: &[Edge]) -> i32 {
    let mut w = 0;
    for &(x0, y0, x1, y1) in edges {
        let (a, b) = ((x0, y0), (x1, y1));
        if y0 <= q.1 {
            if y1 > q.1 && is_left(a, b, q) > 0.0 {
                w += 1;
            }
        } else if y1 <= q.1 && is_left(a, b, q) < 0.0 {
            w -= 1;
        }
    }
    w
}

/// Fills `edges` onto `grid` by the non-zero winding rule, with [`raster`]'s
/// shared sampling, threshold and fallback. `(ink, coverage, cols, rows)`,
/// uncropped.
pub(crate) fn fill_on_grid(grid: Grid, edges: &[Edge]) -> (Vec<u8>, Vec<u8>, usize, usize) {
    raster::rasterize_on_grid(grid, |x, y| winding_number((x, y), edges) != 0)
}

/// Rasterises closed `polys` — in font design units, `units_per_em` to the
/// em — at `px_per_em`, cropped tight to its ink, exactly as
/// [`raster::render`] delivers the authored face.
///
/// `None` when the outline is empty or lands no ink at all; a mark too fine
/// to cover half of any pixel still keeps its best-covered pixel, via
/// [`raster`]'s fallback.
pub(crate) fn rasterize_polylines(
    polys: &[Polyline],
    units_per_em: u16,
    px_per_em: f32,
) -> Option<Raster> {
    let scale = f64::from(px_per_em) / f64::from(units_per_em);
    let edges = edges(polys, scale);
    let (min_x, max_x, min_y, max_y) = edge_bounds(&edges)?;
    let grid = Grid::covering(min_x, max_x, min_y, max_y)?;
    let (ink, cov, cols, rows) = fill_on_grid(grid, &edges);
    raster::crop(&ink, &cov, cols, rows, grid.py_top)
}

/// As [`rasterize_polylines`], but also reporting where the ink sits
/// horizontally relative to the glyph's own origin, in pixels.
///
/// [`rasterize_polylines`] crops tight and throws that away, which is right
/// for the prototype bank — a prototype is a shape, not a position. Setting
/// type on a page needs the position back: a left side bearing is the
/// difference between `AV` and `A V`. Recomputing it from `edge_bounds`
/// would not do, because the reported offset has to be the offset of the
/// *ink the fill produced*, not of the outline that was sampled; those
/// differ by up to a pixel, and a per-glyph horizontal error of that size is
/// what makes letters touch at small sizes.
pub(crate) fn rasterize_polylines_placed(
    polys: &[Polyline],
    units_per_em: u16,
    px_per_em: f32,
) -> Option<(Raster, f32)> {
    let scale = f64::from(px_per_em) / f64::from(units_per_em);
    let edges = edges(polys, scale);
    let (min_x, max_x, min_y, max_y) = edge_bounds(&edges)?;
    let grid = Grid::covering(min_x, max_x, min_y, max_y)?;
    let (col0, py_top) = (grid.col0, grid.py_top);
    let (ink, cov, cols, rows) = fill_on_grid(grid, &edges);
    let min_c = (0..cols).find(|&c| (0..rows).any(|r| ink[r * cols + c] != 0))?;
    let cropped = raster::crop(&ink, &cov, cols, rows, py_top)?;
    Some((cropped, (col0 + min_c as f64) as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A closed axis-aligned rectangle, wound counter-clockwise in y-up
    /// space, as a polyline in design units.
    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Polyline {
        vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)]
    }

    /// A rectangle's polyline built back from the `glyf` point list our own
    /// emitter would store for it, so the gate-support path stays exercised
    /// by something with a known answer.
    fn rect_via_contour(x0: i16, y0: i16, x1: i16, y1: i16) -> Vec<Polyline> {
        let c: Contour = vec![(x0, y0, true), (x1, y0, true), (x1, y1, true), (x0, y1, true)];
        flatten_contours(&[c])
    }

    #[test]
    fn a_glyf_point_list_flattens_to_the_same_ink_as_its_polyline() {
        let direct = rasterize_polylines(&[rect(250.0, 0.0, 750.0, 500.0)], 1000, 20.0).unwrap();
        let round_tripped = rasterize_polylines(&rect_via_contour(250, 0, 750, 500), 1000, 20.0)
            .expect("the same rectangle, read back from a point list");
        assert_eq!((direct.width, direct.height), (round_tripped.width, round_tripped.height));
        assert_eq!(direct.ink, round_tripped.ink);
    }

    #[test]
    fn a_filled_square_rasterises_solid_and_square() {
        // 500 units of a 1000-unit em at 20px/em is 10px.
        let r = rasterize_polylines(&[rect(250.0, 0.0, 750.0, 500.0)], 1000, 20.0).unwrap();
        assert_eq!((r.width, r.height), (10, 10), "square in, square out");
        assert!(r.ink.iter().all(|&b| b == 1), "no interior gaps in a solid fill");
    }

    #[test]
    fn a_counter_is_left_unfilled() {
        // Outer counter-clockwise, inner clockwise: the non-zero rule must
        // read the inner contour as a hole, which is the whole reason the
        // bank can tell 'o' from a blob.
        let outer = rect(0.0, 0.0, 1000.0, 1000.0);
        let mut inner = rect(300.0, 300.0, 700.0, 700.0);
        inner.reverse();
        let r = rasterize_polylines(&[outer, inner], 1000, 20.0).unwrap();
        assert_eq!((r.width, r.height), (20, 20));
        let at = |c: usize, rw: usize| r.ink[rw * r.width as usize + c];
        assert_eq!(at(1, 1), 1, "corner is inside the outer contour");
        assert_eq!(at(10, 10), 0, "centre falls in the counter");
    }

    #[test]
    fn baseline_is_measured_downward_from_the_cropped_top() {
        // A box from y=0 to y=500 at 20px/em sits entirely on the baseline,
        // so the baseline is its full 10px height below the cropped top.
        let r = rasterize_polylines(&[rect(0.0, 0.0, 500.0, 500.0)], 1000, 20.0).unwrap();
        assert!(
            (r.baseline_dy - r.height as f32).abs() < 0.001,
            "baseline_dy {} should equal height {}",
            r.baseline_dy,
            r.height
        );
    }

    #[test]
    fn an_empty_outline_is_none_not_a_panic() {
        assert!(rasterize_polylines(&[], 1000, 20.0).is_none());
    }
}
