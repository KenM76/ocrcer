//! `0`–`9`. `6` and `9` weld an open hook/tail stroke to a [`super::ring`]
//! bowl by terminating exactly on the ring's own centreline — see
//! [`digit_6`] for why that is a different rule from `8`'s two overlapping
//! closed bowls. `4` is drawn **open** per ISO 3098 Type B: its diagonal's
//! top endpoint does not meet the vertical stroke, so no loop closes and
//! the unclamped hole count is zero.

use super::super::{p, Glyph, Seg, Stroke, CAP, SIDE_BEARING};
use super::ring;

pub fn glyphs() -> Vec<Glyph> {
    vec![
        digit_0(),
        digit_1(),
        digit_2(),
        digit_3(),
        digit_4(),
        digit_5(),
        digit_6(),
        digit_7(),
        digit_8(),
        digit_9(),
    ]
}

/// Narrower than [`super::upper::letter_o`]'s bowl — that is the case pair
/// the face exists to separate — but the *centreline* still touches
/// `y = 0` and `y = CAP` exactly, same as every other ascender in this
/// face. Insetting the centreline to make the outer (pen-radius-expanded)
/// edge land on the metric would be wrong: it is the centreline that is
/// authored, and the pen's overshoot is supposed to fall where it falls.
fn digit_0() -> Glyph {
    Glyph { codepoint: '0' as u32, advance: 370 + 2 * SIDE_BEARING, strokes: vec![ring(185, 350, 185, 350)] }
}

fn digit_1() -> Glyph {
    Glyph {
        codepoint: '1' as u32,
        advance: 120 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(20, 560), p(140, 700), p(140, 0)])],
    }
}

fn digit_2() -> Glyph {
    Glyph {
        codepoint: '2' as u32,
        advance: 340 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(40, 600),
            segs: vec![
                Seg::Quad { ctrl: p(40, 700), to: p(200, 700) },
                Seg::Quad { ctrl: p(360, 700), to: p(360, 560) },
                Seg::Line(p(40, 0)),
                Seg::Line(p(380, 0)),
            ],
        }],
    }
}

/// The start and end anchors sit at `y = 700` and `y = 0` exactly — not the
/// nearby control points, which a Bézier curve never passes through.
fn digit_3() -> Glyph {
    Glyph {
        codepoint: '3' as u32,
        advance: 260 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(60, 700),
            segs: vec![
                Seg::Quad { ctrl: p(320, 700), to: p(320, 520) },
                Seg::Quad { ctrl: p(320, 400), to: p(140, 360) },
                Seg::Quad { ctrl: p(320, 320), to: p(320, 180) },
                Seg::Quad { ctrl: p(320, 0), to: p(60, 0) },
            ],
        }],
    }
}

/// Open per ISO 3098 Type B: the diagonal's top point `(200, 700)` sits
/// far enough from the vertical stroke's top `(340, 700)` — 140 units,
/// twice `STROKE` — that the pen's overshoot cannot bridge them, unlike a
/// first attempt at `(300, 700)` that looked separated at the centreline
/// but welded into a closed triangle once the pen radius was added,
/// exactly the gap-vs-overlap distinction `digit_6`/`digit_8`/`digit_9`
/// rely on in the other direction. The two strokes meet only once — where
/// the crossbar ends, on the vertical — a T-junction, not a cycle. Zero
/// enclosed regions, asserted directly by
/// `raster::tests::unclamped_hole_count_matches_the_face_design`.
fn digit_4() -> Glyph {
    Glyph {
        codepoint: '4' as u32,
        advance: 340 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(200, 700), p(40, 230), p(340, 230)]),
            Stroke::line(&[p(340, 700), p(340, 0)]),
        ],
    }
}

fn digit_5() -> Glyph {
    Glyph {
        codepoint: '5' as u32,
        advance: 340 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(320, 700),
            segs: vec![
                Seg::Line(p(40, 700)),
                Seg::Line(p(40, 400)),
                Seg::Quad { ctrl: p(320, 400), to: p(320, 190) },
                Seg::Quad { ctrl: p(320, 0), to: p(60, 0) },
            ],
        }],
    }
}

/// One hole, and the hook's terminal is what decides it. `(288, 287)` sits
/// `145` units from the bowl [`ring`]'s centre `(190, 180)`, half a stroke
/// inside its `180` radius, so the terminal's own pen ink overlaps the
/// ring's ink band along its whole width rather than meeting it edge to
/// edge.
///
/// Both of the obvious alternatives have been measured and both fail. A
/// terminal short of the ring leaves a gap the counter leaks through. A
/// terminal exactly on the ring's centreline is covered — but the approach
/// then runs alongside the ring wall without crossing it, and the strip of
/// background trapped between the two flanks is itself a hole: `6` measured
/// 2 regions where the design says 1. That is also why the control point is
/// `(350, 500)` and not the earlier `(30, 150)`, which dipped the curve into
/// the counter and back out, fencing off a region on the inside instead.
///
/// The tangential case only appeared once [`ring`]'s arcs stopped bulging
/// 6% at the diagonals; the old bulge was silently swallowing a join that
/// had never been solid.
fn digit_6() -> Glyph {
    let r = 180;
    Glyph {
        codepoint: '6' as u32,
        advance: 2 * r + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke {
                start: p(340, 700),
                segs: vec![
                    Seg::Quad { ctrl: p(30, 700), to: p(30, 380) },
                    Seg::Quad { ctrl: p(350, 500), to: p(288, 287) },
                ],
            },
            ring(190, r, r, r),
        ],
    }
}

fn digit_7() -> Glyph {
    Glyph {
        codepoint: '7' as u32,
        advance: 300 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(40, 700), p(340, 700), p(140, 0)])],
    }
}

/// Two rings overlapping enough that the pen ink bridges solidly at the
/// pinch — the overlap is deliberate, not tangency: two outlines that only
/// touch at a point risk a rasterisation gap at coarse pixel sizes that
/// would leak the two interiors into the border-touching background and
/// erase both holes. `raster::tests::unclamped_hole_count_matches_the_face_design`
/// measures the waist directly, without the extractor's clamp, and asserts
/// exactly two enclosed regions — a solid waist, not a third lens.
fn digit_8() -> Glyph {
    let r = 190;
    Glyph {
        codepoint: '8' as u32,
        advance: 2 * r + 2 * SIDE_BEARING,
        strokes: vec![ring(195, CAP - r, r, r), ring(195, r, r, r)],
    }
}

/// One hole: mirrors [`digit_6`]'s weld — the tail starts at `(346, 430)`,
/// exactly on the top [`ring`]'s own centreline (`180` units from its
/// centre `(190, CAP - r)`, same as `digit_6`'s terminal), then drops on
/// the **right**, roughly below the loop's right edge, straight until a
/// slight leftward curl only at the very bottom — printed-type `9`, not a
/// handwritten swash down the left.
fn digit_9() -> Glyph {
    let r = 180;
    Glyph {
        codepoint: '9' as u32,
        advance: 2 * r + 2 * SIDE_BEARING,
        strokes: vec![
            ring(190, CAP - r, r, r),
            Stroke { start: p(346, 430), segs: vec![Seg::Quad { ctrl: p(346, 150), to: p(280, 0) }] },
        ],
    }
}
