//! Glyph definitions, split into one file per charset slice so several
//! authors can extend disjoint parts of `model/charset.tsv` at once.
//! [`glyphs()`] concatenates every submodule's list. This file also holds
//! the stroke-building helpers shared across submodules — the point of
//! putting them here rather than in each file is that five authors each
//! building an oval independently would drift in the third decimal, and
//! the whole argument for a prototype bank is that its shapes agree.
//!
//! What an author should reach for before hand-rolling a `Stroke` literal:
//! - [`ring`] — a closed oval/circle bowl (`0`, `O`, `8`'s two bowls, the
//!   loop in `6`/`9`, …).
//! - [`bowl_slash`] — the diagonal that severs [`upper::letter_o`]'s bowl
//!   for `Ø`/`⌀`, unchanged from Phase A.
//! - `Stroke::line` / `Stroke::dot` (on [`super::Stroke`]) for straight
//!   runs and isolated dots.
//! - An open curve's terminal landing exactly **on** a [`ring`]'s own
//!   centreline — distance from the ring's centre equal to its radius, not
//!   short of it (a gap the counter leaks through) and not past it (a lump
//!   bulging into the counter that the real glyph doesn't have). The pen is
//!   `STROKE` wide and centred on the path, so a centreline terminal is
//!   already covered by the ring's own ink for half a stroke in every
//!   direction — the join is solid without adding ink the ring wasn't
//!   already putting there. Landing on the centreline by distance is
//!   necessary but not sufficient: the curve's *approach* must stay outside
//!   the ring until it touches down, or it fences off a sliver of
//!   background between where it dips in and where it terminates, and the
//!   hole count comes out one too many. `digits::digit_6` uses this weld for
//!   its hook and `digits::digit_9` for its tail — see `digits::digit_6` for
//!   a worked example, including the crossing failure mode, before
//!   inventing a different join for `b`/`p`/`%`. This is a different
//!   technique from `digits::digit_8`'s two bowls, which overlap as closed
//!   rings by genuinely more than tangency; there is no open-curve terminal
//!   involved there.

mod accents;
mod digits;
mod lower;
mod punct;
mod symbols;
mod upper;

use super::{p, Glyph, Seg, Stroke};

/// Every glyph in the face today. Order does not affect the rendered
/// result (see [`Glyph::strokes`]); grouped by submodule for readability.
pub fn glyphs() -> Vec<Glyph> {
    let mut all = Vec::new();
    all.extend(digits::glyphs());
    all.extend(upper::glyphs());
    all.extend(lower::glyphs());
    all.extend(punct::glyphs());
    all.extend(symbols::glyphs());
    all.extend(accents::glyphs());
    all
}

/// A closed loop centred at `(cx, cy)`, traced by four quadratic Béziers,
/// one per quadrant, with each control point pulled in from the quadrant's
/// outer corner by [`ARC_CTRL`].
///
/// Putting the control *at* the corner — the tangent-line intersection — is
/// the obvious construction and is wrong by more than it looks. A quadratic
/// reaches only halfway to its control, so that arc passes through
/// `0.75 * r * sqrt(2) = 1.0607 * r` at 45 degrees: every round glyph in the
/// face bulges 6.07% on its diagonals and is a squarish superellipse rather
/// than a circle. On-axis extents are unaffected, which is why no width,
/// height or aspect check ever saw it.
fn ring(cx: i16, cy: i16, rx: i16, ry: i16) -> Stroke {
    let kx = arc_ctrl(rx);
    let ky = arc_ctrl(ry);
    let e = p(cx + rx, cy);
    let n = p(cx, cy + ry);
    let w = p(cx - rx, cy);
    let s = p(cx, cy - ry);
    Stroke {
        start: e,
        segs: vec![
            Seg::Quad { ctrl: p(cx + kx, cy + ky), to: n },
            Seg::Quad { ctrl: p(cx - kx, cy + ky), to: w },
            Seg::Quad { ctrl: p(cx - kx, cy - ky), to: s },
            Seg::Quad { ctrl: p(cx + kx, cy - ky), to: e },
        ],
    }
}

/// Control-point offset for a 90-degree quadratic arc of radius `r`, as a
/// fraction of `r`.
///
/// Solved, not chosen: a quadratic's midpoint is `(p0 + 2*ctrl + p2) / 4`, so
/// for an arc from `(r, 0)` to `(0, r)` with the control at `(k, k)` that
/// midpoint sits at radius `(r + 2*k) * sqrt(2) / 4`. Setting it equal to `r`
/// gives `k = (2*sqrt(2) - 1) / 2`. The remaining radial error peaks near the
/// quadrant quarter-points at 0.8%, inward, against 6.07% outward for the
/// tangent-intersection control.
const ARC_CTRL: f64 = 0.914_213_562_373_095_1;

fn arc_ctrl(r: i16) -> i16 {
    (r as f64 * ARC_CTRL).round() as i16
}

/// A slash for [`upper::letter_o_slash`]/[`symbols::diameter_sign`]'s
/// shared bowl, extending only `STROKE` past the ellipse boundary along
/// the diagonal — enough to fully sever the counter into two regions
/// without the glyph's overall width leaving the charset's authored aspect
/// band the way a corner-to-corner slash would. Sized for exactly that one
/// bowl (`upper::letter_o`'s `rx=245, ry=350`); a differently-sized bowl
/// needing the same treatment should re-derive its own two endpoints by
/// the same method (ellipse boundary point on the `(rx, ry)` diagonal,
/// pushed out by `STROKE` along that direction) rather than reusing these
/// literals.
fn bowl_slash() -> Stroke {
    Stroke::line(&[p(32, 45), p(459, 655)])
}
