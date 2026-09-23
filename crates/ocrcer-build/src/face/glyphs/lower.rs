//! `a`–`z` plus `ß` and `æ`.
//!
//! Single-storey throughout: `a` is a bowl welded to a tangent right stem,
//! `g` is the same bowl with a descending hook, not the two-storey forms a
//! book face uses — the drafting-face convention this whole face follows.
//!
//! Three vertical bands, per `CLAUDE.md` chunk instructions, authored to
//! touch each metric exactly and left for the pen's own overshoot to fall
//! outside: `a c e m n o r s u v w x z` sit in the x-height band
//! (baseline..[`X_HEIGHT`]); `b d h k l` reach [`CAP`]; `g p q y` drop to
//! [`DESCENDER`]; `f` reaches `CAP` without descending, `j` descends without
//! reaching `CAP`. `i`/`j` are drawn with their own dots — a later pass
//! removes them to attach accent marks, but each letter has to stand on its
//! own first.
//!
//! `t` is charset `baseline_class=ascender` (it shares the pruning bucket)
//! but is drawn to [`T_TOP`], three-quarters of the way from `X_HEIGHT` to
//! `CAP` per ISO 3098 — shorter than the letters it shares a bucket with,
//! as is the dot on `i`. That is what the bucket means: `ascender` is
//! "rises past the x-line without descending", not "reaches the cap line",
//! and `raster::tests::stroke_data_agrees_with_its_baseline_class` asserts
//! it that way. The tight cap-line check lives in
//! `raster::tests::capitals_and_digits_are_drawn_baseline_to_cap`, which
//! this file's letters are correctly outside of.

use super::super::{p, Glyph, Seg, Stroke, CAP, DESCENDER, SIDE_BEARING, STROKE, X_HEIGHT};
use super::ring;

/// Half of [`X_HEIGHT`], and also the shared bowl's `cy`/`ry` — the bowl
/// touches the baseline and `X_HEIGHT` exactly, same discipline as
/// `digits::digit_0`.
const BOWL_R: i16 = X_HEIGHT / 2;

/// Shared x-height bowl radius (`a c e g o` and the ligature/eszett bowls),
/// chosen once so every x-height bowl in this file is the same oval rather
/// than five independent guesses.
const BOWL_RX: i16 = 170;

/// Three-quarters of the way from `X_HEIGHT` to `CAP` — see the module doc.
const T_TOP: i16 = X_HEIGHT + (CAP - X_HEIGHT) * 3 / 4;

/// Dot height for `i`/`j`: `CAP` less one stroke width, so the dot's own
/// ink (which extends a further half-stroke in every direction from this
/// centre) stops short of `CAP` rather than touching or crossing it — "at
/// about the same height as an ascender's top, or slightly below," and
/// deliberately not raised to make `i` match the full-height ascenders'
/// measured top: a dot sitting below the cap line is what the letterform
/// is, not a defect.
const DOT_Y: i16 = CAP - STROKE;

pub fn glyphs() -> Vec<Glyph> {
    vec![
        letter_a(),
        letter_b(),
        letter_c(),
        letter_d(),
        letter_e(),
        letter_f(),
        letter_g(),
        letter_h(),
        letter_i(),
        letter_j(),
        letter_k(),
        letter_lower_l(),
        letter_m(),
        letter_n(),
        letter_o(),
        letter_p(),
        letter_q(),
        letter_r(),
        letter_s(),
        letter_t(),
        letter_u(),
        letter_v(),
        letter_w(),
        letter_x(),
        letter_y(),
        letter_z(),
        letter_sharp_s(),
        letter_ae(),
    ]
}

/// `W -> N -> E`: the two quadrants [`super::ring`] would draw for the top
/// half of an oval, sharing its exact per-quadrant control points. The
/// spring point for `h m n r`'s arm — reused rather than a fourth hand-rolled
/// curve, same argument as `super::bowl_slash`'s doc makes for the slash.
fn arch(cx: i16, cy: i16, rx: i16, ry: i16) -> Stroke {
    let w = p(cx - rx, cy);
    let n = p(cx, cy + ry);
    let e = p(cx + rx, cy);
    Stroke {
        start: w,
        segs: vec![
            Seg::Quad { ctrl: p(cx - rx, cy + ry), to: n },
            Seg::Quad { ctrl: p(cx + rx, cy + ry), to: e },
        ],
    }
}

/// `E -> N -> W -> S`: three of [`super::ring`]'s four quadrants, leaving
/// the fourth (`S` back to `E`) as an open mouth. `c`'s whole shape and the
/// arc half of `e`.
fn bowl_open_se(cx: i16, cy: i16, rx: i16, ry: i16) -> Stroke {
    let e = p(cx + rx, cy);
    let n = p(cx, cy + ry);
    let w = p(cx - rx, cy);
    let s = p(cx, cy - ry);
    Stroke {
        start: e,
        segs: vec![
            Seg::Quad { ctrl: p(cx + rx, cy + ry), to: n },
            Seg::Quad { ctrl: p(cx - rx, cy + ry), to: w },
            Seg::Quad { ctrl: p(cx - rx, cy - ry), to: s },
        ],
    }
}

/// Bowl closed all the way around ([`ring`], one hole on its own, same as
/// `o`) plus a stem tangent to its east point at `(2*BOWL_RX, ..)`. The
/// tangent line sits outside the oval everywhere except that one point —
/// same distance-on-the-centreline weld [`digits::digit_6`] uses for its
/// hook, just a straight stem instead of a curve — so it adds no second
/// hole; it only flattens the bowl's right side into the single-storey
/// stroke this face draws `a` with instead of a two-storey book form.
fn letter_a() -> Glyph {
    let bowl = ring(BOWL_RX, BOWL_R, BOWL_RX, BOWL_R);
    let stem = Stroke::line(&[p(2 * BOWL_RX, 0), p(2 * BOWL_RX, X_HEIGHT)]);
    Glyph { codepoint: 'a' as u32, advance: 2 * BOWL_RX + 2 * SIDE_BEARING, strokes: vec![bowl, stem] }
}

/// Bowl on the right welded to a full-height stem on the left, the weld
/// tangent to the bowl's west point — same construction as `letter_p`, only
/// the stem's extent differs (ascender here, x-height-plus-descender there).
fn letter_b() -> Glyph {
    let cx = STROKE / 2 + BOWL_RX;
    let bowl = ring(cx, BOWL_R, BOWL_RX, BOWL_R);
    let stem = Stroke::line(&[p(STROKE / 2, CAP), p(STROKE / 2, 0)]);
    Glyph { codepoint: 'b' as u32, advance: cx + BOWL_RX + 2 * SIDE_BEARING, strokes: vec![stem, bowl] }
}

fn letter_c() -> Glyph {
    Glyph {
        codepoint: 'c' as u32,
        advance: 2 * BOWL_RX + 2 * SIDE_BEARING,
        strokes: vec![bowl_open_se(BOWL_RX, BOWL_R, BOWL_RX, BOWL_R)],
    }
}

/// Mirror of `letter_b`: bowl on the left, stem on the right.
fn letter_d() -> Glyph {
    let bowl = ring(BOWL_RX, BOWL_R, BOWL_RX, BOWL_R);
    let stem = Stroke::line(&[p(2 * BOWL_RX, CAP), p(2 * BOWL_RX, 0)]);
    Glyph { codepoint: 'd' as u32, advance: 2 * BOWL_RX + 2 * SIDE_BEARING, strokes: vec![bowl, stem] }
}

/// [`letter_c`]'s open arc plus a crossbar landing exactly on the arc's own
/// east endpoint — the same mouth stays open below the bar, so the counter
/// splits into one enclosed lobe (above the bar) and one lobe left open to
/// the background (below, through the mouth), not two enclosed holes.
fn letter_e() -> Glyph {
    let arc = bowl_open_se(BOWL_RX, BOWL_R, BOWL_RX, BOWL_R);
    let bar = Stroke::line(&[p(0, BOWL_R), p(2 * BOWL_RX, BOWL_R)]);
    Glyph { codepoint: 'e' as u32, advance: 2 * BOWL_RX + 2 * SIDE_BEARING, strokes: vec![arc, bar] }
}

fn letter_f() -> Glyph {
    let hook = Stroke { start: p(220, CAP), segs: vec![Seg::Quad { ctrl: p(60, CAP), to: p(60, 560) }, Seg::Line(p(60, 0))] };
    let bar = Stroke::line(&[p(0, 400), p(200, 400)]);
    Glyph { codepoint: 'f' as u32, advance: 220 + 2 * SIDE_BEARING, strokes: vec![hook, bar] }
}

/// Single-storey: the same bowl `a`/`o` use, plus a hook dropping from a
/// point on the bowl's own boundary (south-east, `(cos,sin) = (-1,-1)/sqrt2`
/// off the bowl's radius) down to [`DESCENDER`]. `ring` approximates an
/// oval with per-quadrant quadratics rather than a true ellipse, so this
/// start point is a few units off the rendered curve rather than exactly on
/// it — within the pen's own half-width, same tolerance `digit_6`'s doc
/// discusses, and confirmed by `unclamped_hole_count_matches_the_face_design`
/// rather than assumed.
fn letter_g() -> Glyph {
    let bowl = ring(BOWL_RX, BOWL_R, BOWL_RX, BOWL_R);
    let hook = Stroke {
        start: p(290, 72),
        segs: vec![Seg::Quad { ctrl: p(300, -100), to: p(200, DESCENDER) }],
    };
    Glyph { codepoint: 'g' as u32, advance: 2 * BOWL_RX + 2 * SIDE_BEARING, strokes: vec![bowl, hook] }
}

/// Full-height stem plus [`arch`], springing from partway down the stem
/// (not its very top) and continuing straight to the baseline — the same
/// arch `letter_n` uses, just off a taller stem.
fn letter_h() -> Glyph {
    let mut arm = arch(205, 350, 170, 140);
    arm.segs.push(Seg::Line(p(375, 0)));
    Glyph { codepoint: 'h' as u32, advance: 375 + 2 * SIDE_BEARING, strokes: vec![Stroke::line(&[p(35, CAP), p(35, 0)]), arm] }
}

/// The bare stem `i` and `j` share the shape of, with no dot: `letter_i`
/// adds its own dot after this, and `accents::dotless_i` builds `ì í î ï`
/// on it instead of on `letter_i`'s `Glyph` — that one carries the dot as a
/// second stroke, which an accented `i` must not keep.
pub(super) fn dotless_i_stem() -> Stroke {
    Stroke::line(&[p(35, X_HEIGHT), p(35, 0)])
}

/// Stem to [`X_HEIGHT`] only — the dot above it is what pushes the
/// bounding box into ascender territory, per the module doc.
fn letter_i() -> Glyph {
    Glyph {
        codepoint: 'i' as u32,
        advance: STROKE + 2 * SIDE_BEARING,
        strokes: vec![dotless_i_stem(), Stroke::dot(p(35, DOT_Y))],
    }
}

/// Stem from `X_HEIGHT` down through the baseline to a left-curling foot
/// touching [`DESCENDER`] exactly, plus its own dot — see [`letter_i`] for
/// why the dot, not the stem, is what makes this an ascender-bucketed
/// glyph.
fn letter_j() -> Glyph {
    let hook = Stroke {
        start: p(115, X_HEIGHT),
        segs: vec![Seg::Line(p(115, -140)), Seg::Quad { ctrl: p(115, DESCENDER), to: p(0, DESCENDER) }],
    };
    Glyph { codepoint: 'j' as u32, advance: 115 + 2 * SIDE_BEARING, strokes: vec![hook, Stroke::dot(p(115, DOT_Y))] }
}

/// Both diagonals meet the stem at one shared point, `(35, 260)` — a single
/// stroke through the junction, same pattern `upper::letter_k` uses.
fn letter_k() -> Glyph {
    Glyph {
        codepoint: 'k' as u32,
        advance: 320 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(35, CAP), p(35, 0)]), Stroke::line(&[p(320, X_HEIGHT), p(35, 260), p(320, 0)])],
    }
}

fn letter_lower_l() -> Glyph {
    Glyph { codepoint: 'l' as u32, advance: STROKE + 2 * SIDE_BEARING, strokes: vec![Stroke::line(&[p(35, 700), p(35, 0)])] }
}

/// Stem plus two arches sharing a leg — the middle leg is both the first
/// arch's own leg and the second arch's spring point, not two separate
/// strokes that merely touch.
fn letter_m() -> Glyph {
    let mut arm1 = arch(185, 350, 150, 140);
    arm1.segs.push(Seg::Line(p(335, 0)));
    let mut arm2 = arch(485, 350, 150, 140);
    arm2.segs.push(Seg::Line(p(635, 0)));
    Glyph {
        codepoint: 'm' as u32,
        advance: 635 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(35, X_HEIGHT), p(35, 0)]), arm1, arm2],
    }
}

fn letter_n() -> Glyph {
    let mut arm = arch(205, 350, 170, 140);
    arm.segs.push(Seg::Line(p(375, 0)));
    Glyph { codepoint: 'n' as u32, advance: 375 + 2 * SIDE_BEARING, strokes: vec![Stroke::line(&[p(35, X_HEIGHT), p(35, 0)]), arm] }
}

fn letter_o() -> Glyph {
    Glyph {
        codepoint: 'o' as u32,
        advance: 2 * BOWL_RX + 2 * SIDE_BEARING,
        strokes: vec![ring(BOWL_RX, BOWL_R, BOWL_RX, BOWL_R)],
    }
}

/// Same bowl as [`letter_b`], stem shortened to x-height-plus-descender —
/// the family resemblance is deliberate, not a coincidence of round
/// numbers: `b` and `p` differ only in how tall their shared stem is drawn.
fn letter_p() -> Glyph {
    let cx = STROKE / 2 + BOWL_RX;
    let bowl = ring(cx, BOWL_R, BOWL_RX, BOWL_R);
    let stem = Stroke::line(&[p(STROKE / 2, X_HEIGHT), p(STROKE / 2, DESCENDER)]);
    Glyph { codepoint: 'p' as u32, advance: cx + BOWL_RX + 2 * SIDE_BEARING, strokes: vec![stem, bowl] }
}

/// Same bowl as [`letter_d`], stem shortened the way [`letter_p`] shortens
/// `b`'s.
fn letter_q() -> Glyph {
    let bowl = ring(BOWL_RX, BOWL_R, BOWL_RX, BOWL_R);
    let stem = Stroke::line(&[p(2 * BOWL_RX, X_HEIGHT), p(2 * BOWL_RX, DESCENDER)]);
    Glyph { codepoint: 'q' as u32, advance: 2 * BOWL_RX + 2 * SIDE_BEARING, strokes: vec![bowl, stem] }
}

/// [`arch`] alone, with no leg appended — the stub ends at the arch's own
/// east point rather than continuing to the baseline, which is what keeps
/// `r`'s arm short instead of turning it into another `n`.
fn letter_r() -> Glyph {
    let arm = arch(145, 390, 110, 100);
    Glyph { codepoint: 'r' as u32, advance: 255 + 2 * SIDE_BEARING, strokes: vec![Stroke::line(&[p(35, X_HEIGHT), p(35, 0)]), arm] }
}

/// `upper::letter_s`'s path scaled into the body — same handedness, same
/// proportions, 0.7 of its `CAP` span.
///
/// The upper bowl descends on the **left** and the lower bowl on the
/// **right**; the terminals are upper-right and lower-left. Getting that
/// backwards does not produce a wrong-looking `s`, it produces a different
/// character, and the character it produces is `3`: two bowls that both
/// bulge right with both terminals on the left *is* the digit. An earlier
/// version of this glyph was `digit_3`'s literal path scaled to `X_HEIGHT`,
/// carried over on the reasoning that it shared the reverse-curve topology
/// and therefore the zero-hole guarantee. It did. It also made the two most
/// confusable entries in the charset the same shape, which is the one thing
/// a prototype bank must never contain — an `s`/`3` pair with zero margin
/// cannot be told apart by any distance, and the decoder would report high
/// confidence on whichever one it happened to reach first.
fn letter_s() -> Glyph {
    Glyph {
        codepoint: 's' as u32,
        advance: 260 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(320, X_HEIGHT),
            segs: vec![
                Seg::Quad { ctrl: p(60, X_HEIGHT), to: p(60, 364) },
                Seg::Quad { ctrl: p(60, 280), to: p(204, 252) },
                Seg::Quad { ctrl: p(320, 224), to: p(320, 126) },
                Seg::Quad { ctrl: p(320, 0), to: p(60, 0) },
            ],
        }],
    }
}

/// Drawn to [`T_TOP`], not `CAP` — see the module doc for why this is a
/// deliberate departure from the other charset-`ascender` letters.
fn letter_t() -> Glyph {
    Glyph {
        codepoint: 't' as u32,
        advance: 260 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(60, T_TOP), p(60, 0)]), Stroke::line(&[p(0, 400), p(260, 400)])],
    }
}

/// One continuous stroke: down the left stem, under through the bottom of
/// an oval (mirroring [`arch`]'s two quadrants vertically), up the right
/// stem — `n` upside down, drawn as the single pen path a `u` actually is
/// rather than assembled from separate pieces.
fn letter_u() -> Glyph {
    Glyph {
        codepoint: 'u' as u32,
        advance: 375 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(35, X_HEIGHT),
            segs: vec![
                Seg::Line(p(35, 140)),
                Seg::Quad { ctrl: p(35, 0), to: p(205, 0) },
                Seg::Quad { ctrl: p(375, 0), to: p(375, 140) },
                Seg::Line(p(375, X_HEIGHT)),
            ],
        }],
    }
}

fn letter_v() -> Glyph {
    Glyph { codepoint: 'v' as u32, advance: 380 + 2 * SIDE_BEARING, strokes: vec![Stroke::line(&[p(0, X_HEIGHT), p(190, 0), p(380, X_HEIGHT)])] }
}

fn letter_w() -> Glyph {
    Glyph {
        codepoint: 'w' as u32,
        advance: 560 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, X_HEIGHT), p(140, 0), p(280, X_HEIGHT), p(420, 0), p(560, X_HEIGHT)])],
    }
}

fn letter_x() -> Glyph {
    Glyph {
        codepoint: 'x' as u32,
        advance: 350 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, X_HEIGHT), p(350, 0)]), Stroke::line(&[p(350, X_HEIGHT), p(0, 0)])],
    }
}

/// Both diagonals meet at `(160, 140)`; the right one continues straight on
/// through that junction down to [`DESCENDER`] rather than stopping and
/// restarting, so the tail reads as one stroke changing direction, not two
/// strokes that happen to touch.
fn letter_y() -> Glyph {
    Glyph {
        codepoint: 'y' as u32,
        advance: 320 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, X_HEIGHT), p(160, 140)]),
            Stroke::line(&[p(320, X_HEIGHT), p(160, 140), p(40, DESCENDER)]),
        ],
    }
}

fn letter_z() -> Glyph {
    Glyph {
        codepoint: 'z' as u32,
        advance: 310 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, X_HEIGHT), p(310, X_HEIGHT), p(0, 0), p(310, 0)])],
    }
}

/// Full-height stem plus two closed bowls tangent to it — the same weld
/// [`letter_b`]/[`letter_d`] use, twice, rather than `digit_8`'s two
/// overlapping rings: `digit_8` needs its rings to overlap because neither
/// one has a stem of its own to weld to. Upper bowl narrower than the
/// lower one, same smaller-upper-bowl convention `upper::letter_b` uses for
/// capital `B`. Two holes because both bowls close all the way around; if a
/// rendering of this ever measures one, the honest read is that the upper
/// bowl isn't closing, not that the drawing is wrong — see the chunk
/// instructions.
fn letter_sharp_s() -> Glyph {
    let stem = Stroke::line(&[p(35, CAP), p(35, 0)]);
    let upper = ring(125, 595, 90, 105);
    let lower = ring(175, BOWL_R, 140, BOWL_R);
    Glyph { codepoint: 0x00DF, advance: 315 + 2 * SIDE_BEARING, strokes: vec![stem, upper, lower] }
}

/// `a`'s bowl-and-stem on the left, `e`'s open arc-and-bar on the right,
/// sharing the stem between them as the boundary the two halves are welded
/// to — not two independent letters simply placed side by side, which is
/// why the stem is authored once rather than twice.
fn letter_ae() -> Glyph {
    let a_bowl = ring(BOWL_RX, BOWL_R, BOWL_RX, BOWL_R);
    let stem = Stroke::line(&[p(2 * BOWL_RX, 0), p(2 * BOWL_RX, X_HEIGHT)]);
    let e_arc = bowl_open_se(2 * BOWL_RX + BOWL_RX, BOWL_R, BOWL_RX, BOWL_R);
    let e_bar = Stroke::line(&[p(2 * BOWL_RX, BOWL_R), p(4 * BOWL_RX, BOWL_R)]);
    Glyph { codepoint: 0x00E6, advance: 4 * BOWL_RX + 2 * SIDE_BEARING, strokes: vec![a_bowl, stem, e_arc, e_bar] }
}
