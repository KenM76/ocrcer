//! `! " ' ( ) , - . / : ; ? [ \ ] _ ` { } · • – — ‘ ’ “ ” …`
//!
//! Straight verticals for the typewriter marks (`! ' "`), a shared `curl`
//! helper for the four typographic quotes (`‘ ’ “ ”` — two mirrored
//! shapes, doubled for the double-quote pair rather than four independent
//! curves), and mirrored point-lists for every glyph that comes in a
//! left/right pair (`( ) [ ] { }`). `?` is drawn as an open hook with no
//! enclosed counter — a real ring-and-weld construction was considered per
//! the chunk instructions, but an open hook is simpler and gives the same
//! zero-hole result — plus a separate dot at period height, per the design
//! note. `,` and `;` share the same downward tail shape at different
//! heights; `:` and `;` share the same raised dot. `-`/`–`/`—` are one
//! `dash` helper at three lengths, all at the same height, ordered by
//! length. `•` is [`super::ring`] at a radius at or under the pen's own
//! (`STROKE / 2`), which is what makes the pen's overlap fill the counter
//! solid instead of leaving a hole — see [`bullet`]'s doc for the ceiling
//! that puts on how large a hole-free bullet can be in a single-stroke
//! face, and why it lands outside its charted band.

use super::super::{p, Glyph, Seg, Stroke, CAP, DESCENDER, SIDE_BEARING, STROKE};
use super::ring;

/// Vertical centre shared by the parenthesis/bracket/brace's west (or
/// bullet/middle-dot) reference points: halfway between [`CAP`] and
/// [`DESCENDER`], `(700 + -210) / 2`.
const MID_Y: i16 = (CAP + DESCENDER) / 2;

/// Height shared by every dash-like mark (`- – —`).
const DASH_Y: i16 = 280;

/// Top of the raised marks in the apostrophe/quote family (`' " ` ‘ ’ “
/// ”`) — high enough to clear x-height comfortably, short of [`CAP`] so
/// these stay `above`, not `ascender`.
const RAISED_TOP: i16 = 650;
/// Bottom of the straight-vertical raised marks (`'`, `"`).
const RAISED_BOT: i16 = 480;

pub fn glyphs() -> Vec<Glyph> {
    vec![
        exclamation(),
        quote_straight_double(),
        apostrophe(),
        paren_open(),
        paren_close(),
        comma(),
        hyphen(),
        period(),
        slash(),
        colon(),
        semicolon(),
        question(),
        bracket_open(),
        backslash(),
        bracket_close(),
        underscore(),
        backtick(),
        brace_open(),
        brace_close(),
        middle_dot(),
        bullet(),
        en_dash(),
        em_dash(),
        quote_left_single(),
        quote_right_single(),
        quote_left_double(),
        quote_right_double(),
        ellipsis(),
    ]
}

/// A dot straddling the baseline, drawn as [`Stroke::dot`] — the
/// zero-length-segment path.
fn period() -> Glyph {
    Glyph { codepoint: '.' as u32, advance: STROKE + 2 * SIDE_BEARING, strokes: vec![Stroke::dot(p(35, 35))] }
}

/// Bar from [`CAP`] down to `220` — clear of the dot below by twice
/// [`STROKE`], so the gap survives rasterisation at small sizes — plus a
/// separate dot at period height, same column.
fn exclamation() -> Glyph {
    Glyph {
        codepoint: '!' as u32,
        advance: STROKE + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(35, CAP), p(35, 220)]), Stroke::dot(p(35, 35))],
    }
}

/// The straight typewriter form: two plain verticals, not the curved
/// typographic quotes ([`quote_left_double`]/[`quote_right_double`]).
fn quote_straight_double() -> Glyph {
    Glyph {
        codepoint: '"' as u32,
        advance: 245 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(35, RAISED_TOP), p(35, RAISED_BOT)]),
            Stroke::line(&[p(280, RAISED_TOP), p(280, RAISED_BOT)]),
        ],
    }
}

/// The straight typewriter form — see [`quote_right_single`] for the
/// curved typographic mark this is not.
fn apostrophe() -> Glyph {
    Glyph {
        codepoint: '\'' as u32,
        advance: STROKE + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(35, RAISED_TOP), p(35, RAISED_BOT)])],
    }
}

/// Full class: touches [`CAP`] and [`DESCENDER`] exactly. West point at
/// `x=0`, the same cardinal-touch discipline [`super::ring`] uses, at
/// [`MID_Y`] rather than the ring's own centre because a paren's bulge
/// point is the middle of its own vertical span, not a circle's cardinal
/// point.
fn paren_open() -> Glyph {
    Glyph {
        codepoint: '(' as u32,
        advance: 175 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(175, CAP),
            segs: vec![
                Seg::Quad { ctrl: p(0, 560), to: p(0, MID_Y) },
                Seg::Quad { ctrl: p(0, -70), to: p(175, DESCENDER) },
            ],
        }],
    }
}

/// Mirror of [`paren_open`] about `x = 87.5`.
fn paren_close() -> Glyph {
    Glyph {
        codepoint: ')' as u32,
        advance: 175 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(0, CAP),
            segs: vec![
                Seg::Quad { ctrl: p(175, 560), to: p(175, MID_Y) },
                Seg::Quad { ctrl: p(175, -70), to: p(0, DESCENDER) },
            ],
        }],
    }
}

/// The tail shared with [`semicolon`], anchored so its top sits just above
/// the baseline and its bottom dips into `low`-class territory without
/// reaching [`DESCENDER`] — unlike `;`, `,` is not charset
/// `baseline_class=descender`, so nothing requires it to touch that line.
fn comma() -> Glyph {
    Glyph {
        codepoint: ',' as u32,
        advance: 60 + 2 * SIDE_BEARING,
        strokes: vec![tail(70, -140)],
    }
}

/// A downward curl from `(60, top)` to `(0, bottom)`, curving through a
/// control point at the same `x` as its start — [`comma`]'s shape and
/// [`semicolon`]'s tail, parameterised on height so the two glyphs don't
/// carry two copies of the same curve.
fn tail(top: i16, bottom: i16) -> Stroke {
    let ctrl_y = top - (top - bottom) * 110 / 210;
    Stroke { start: p(60, top), segs: vec![Seg::Quad { ctrl: p(60, ctrl_y), to: p(0, bottom) }] }
}

/// One [`dash`] at the shortest of the three lengths this face draws at
/// [`DASH_Y`] — see [`en_dash`]/[`em_dash`] for the other two; all three
/// share the same height and differ only in length, ordered
/// hyphen < en < em.
fn hyphen() -> Glyph {
    Glyph { codepoint: '-' as u32, advance: 330 + 2 * SIDE_BEARING, strokes: vec![dash(330)] }
}

/// A horizontal run of `len` at [`DASH_Y`], shared by [`hyphen`],
/// [`en_dash`], and [`em_dash`].
fn dash(len: i16) -> Stroke {
    Stroke::line(&[p(0, DASH_Y), p(len, DASH_Y)])
}

/// Ascender class: touches [`CAP`] at its top end, baseline at its bottom
/// — stays within `y=0..CAP`, the same discipline `upper::letter_j` uses
/// for the same reason (charset `baseline_class=ascender`, not
/// `descender`).
fn slash() -> Glyph {
    Glyph {
        codepoint: '/' as u32,
        advance: 245 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, 0), p(245, CAP)])],
    }
}

/// Mirror of [`slash`]: top-left to bottom-right instead of bottom-left to
/// top-right.
fn backslash() -> Glyph {
    Glyph {
        codepoint: '\\' as u32,
        advance: 245 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, CAP), p(245, 0)])],
    }
}

/// Both dots in the same column (`x=35`), each resting *inside* the metric
/// line it belongs to: the lower one is [`period`]'s own dot, sitting on
/// the baseline, and the upper one's ink stops on the x-line.
///
/// The face-wide convention — author to the metric line, let the pen
/// overshoot by its radius — is a rule about stroke *terminals*, where the
/// overshoot is the round cap of a stem that genuinely ends there. A dot is
/// not a terminal: its ink is the whole mark, so putting its centre on the
/// line pushes half a pen past it, and a colon built that way pokes above
/// the x-line while its own lower dot rests neatly on the baseline. The two
/// halves would be following different rules.
///
/// Column width alone puts this glyph's aspect below its charted band — a
/// mark one pen wide cannot be wider.
fn colon() -> Glyph {
    use super::super::X_HEIGHT;
    Glyph {
        codepoint: ':' as u32,
        advance: STROKE + 2 * SIDE_BEARING,
        strokes: vec![Stroke::dot(p(35, 35)), Stroke::dot(p(35, X_HEIGHT - 35))],
    }
}

/// [`colon`]'s upper dot — inset the same way, and for the same reason —
/// plus [`comma`]'s tail shape, stretched down to touch [`DESCENDER`] exactly — the `descender`-class discipline
/// `lower::letter_g`/`p`/`q`/`y` use, applied to this tail instead of a
/// hook.
fn semicolon() -> Glyph {
    use super::super::X_HEIGHT;
    Glyph {
        codepoint: ';' as u32,
        advance: 60 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::dot(p(35, X_HEIGHT - 35)), tail(70, DESCENDER)],
    }
}

/// An open hook — up from `(60, 600)`, over the top, curling back down and
/// in to a short stub at `(220, 260)` — plus a separate dot at period
/// height, the same two-piece construction as [`exclamation`]. No ring is
/// welded in: the hook never closes, so the unclamped hole count is zero
/// without needing the weld rule at all.
fn question() -> Glyph {
    Glyph {
        codepoint: '?' as u32,
        advance: 300 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke {
                start: p(60, 600),
                segs: vec![
                    Seg::Quad { ctrl: p(60, CAP), to: p(210, CAP) },
                    Seg::Quad { ctrl: p(360, CAP), to: p(360, 560) },
                    Seg::Quad { ctrl: p(360, 420), to: p(220, 380) },
                    Seg::Line(p(220, 260)),
                ],
            },
            Stroke::dot(p(220, 35)),
        ],
    }
}

/// Full class: a single polyline, tick-stem-tick, the same T-junction
/// construction [`upper::letter_cap_i`] uses turned on its side and
/// stretched to [`CAP`]/[`DESCENDER`].
fn bracket_open() -> Glyph {
    Glyph {
        codepoint: '[' as u32,
        advance: 150 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(150, CAP), p(0, CAP), p(0, DESCENDER), p(150, DESCENDER)])],
    }
}

/// Mirror of [`bracket_open`]: stem on the right, ticks pointing left.
fn bracket_close() -> Glyph {
    Glyph {
        codepoint: ']' as u32,
        advance: 150 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, CAP), p(150, CAP), p(150, DESCENDER), p(0, DESCENDER)])],
    }
}

/// `low` class: a bar below the baseline rather than on it — far enough
/// down to read as distinct from the baseline itself, nowhere near
/// x-height.
fn underscore() -> Glyph {
    Glyph {
        codepoint: '_' as u32,
        advance: 460 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, -70), p(460, -70)])],
    }
}

/// A short grave-accent-direction diagonal (high on the left, low on the
/// right) in the same raised zone as the apostrophe, not touching
/// [`RAISED_TOP`]/[`RAISED_BOT`] exactly since it is drawn shorter and
/// lower than the straight quote marks.
fn backtick() -> Glyph {
    Glyph {
        codepoint: '`' as u32,
        advance: 120 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, 650), p(120, 520)])],
    }
}

/// One continuous stroke: down from [`CAP`] to a spine at `x=70`, out to a
/// nub tip at `x=0` at [`MID_Y`] and back, then on down to [`DESCENDER`].
/// The nub's approach and return arcs are vertically separated (`320..245`
/// on the way out, `245..170` on the way back) rather than retracing the
/// same points, so they form a bump, not a closed loop — zero holes.
fn brace_open() -> Glyph {
    Glyph {
        codepoint: '{' as u32,
        advance: 235 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(235, CAP),
            segs: vec![
                Seg::Quad { ctrl: p(70, CAP), to: p(70, 560) },
                Seg::Line(p(70, 320)),
                Seg::Quad { ctrl: p(0, 290), to: p(0, MID_Y) },
                Seg::Quad { ctrl: p(0, 200), to: p(70, 170) },
                Seg::Line(p(70, -70)),
                Seg::Quad { ctrl: p(70, DESCENDER), to: p(235, DESCENDER) },
            ],
        }],
    }
}

/// Mirror of [`brace_open`] about `x = 117.5`: spine on the left, nub
/// pointing right.
fn brace_close() -> Glyph {
    Glyph {
        codepoint: '}' as u32,
        advance: 235 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(0, CAP),
            segs: vec![
                Seg::Quad { ctrl: p(165, CAP), to: p(165, 560) },
                Seg::Line(p(165, 320)),
                Seg::Quad { ctrl: p(235, 290), to: p(235, MID_Y) },
                Seg::Quad { ctrl: p(235, 200), to: p(165, 170) },
                Seg::Line(p(165, -70)),
                Seg::Quad { ctrl: p(165, DESCENDER), to: p(0, DESCENDER) },
            ],
        }],
    }
}

/// A plain dot at [`MID_Y`] — same size as [`period`]'s, raised rather
/// than straddling the baseline. Single-column, same aspect-vs-band tension
/// as [`colon`].
fn middle_dot() -> Glyph {
    Glyph { codepoint: 0x00B7, advance: STROKE + 2 * SIDE_BEARING, strokes: vec![Stroke::dot(p(35, MID_Y))] }
}

/// A filled disc, drawn as two concentric loops rather than one.
///
/// A pen of diameter [`STROKE`] sweeping a loop of radius `r` covers the
/// annulus `[r - STROKE/2, r + STROKE/2]`, so a single loop is solid only
/// while `r <= STROKE/2` — a covered diameter of `2*STROKE`, aspect `0.20`,
/// short of this glyph's charted band. Nesting a second loop inside closes
/// the gap: `r = 70` covers `[35, 105]`, `r = 35` covers `[0, 70]`, and the
/// union is a solid disc of radius `105`. Diameter `210`, aspect `0.30`,
/// three times [`period`]'s bare `STROKE`-wide dot and in band.
///
/// That is also how it would be drawn by hand: a drafter fills a bullet by
/// going round twice, not by finding a fatter pen.
fn bullet() -> Glyph {
    Glyph {
        codepoint: 0x2022,
        advance: 140 + 2 * SIDE_BEARING,
        strokes: vec![ring(70, MID_Y, 70, 70), ring(70, MID_Y, 35, 35)],
    }
}

/// [`dash`] at the middle of the three lengths.
fn en_dash() -> Glyph {
    Glyph { codepoint: 0x2013, advance: 700 + 2 * SIDE_BEARING, strokes: vec![dash(700)] }
}

/// [`dash`] at the longest of the three lengths.
fn em_dash() -> Glyph {
    Glyph { codepoint: 0x2014, advance: 1260 + 2 * SIDE_BEARING, strokes: vec![dash(1260)] }
}

/// A single raised curl — see [`curl`] — sized and placed to mirror
/// [`quote_right_single`], not the straight [`apostrophe`].
fn quote_left_single() -> Glyph {
    Glyph { codepoint: 0x2018, advance: 60 + 2 * SIDE_BEARING, strokes: vec![curl(0, true)] }
}

/// A single raised curl, thick end up, tail curling down and to the left —
/// the typographic mark [`apostrophe`] is the straight, unmirrored
/// alternative to.
fn quote_right_single() -> Glyph {
    Glyph { codepoint: 0x2019, advance: 60 + 2 * SIDE_BEARING, strokes: vec![curl(0, false)] }
}

/// Two [`quote_left_single`] curls side by side — the opening double quote
/// is two opening singles, not a mirror of [`quote_right_double`] as a
/// whole.
fn quote_left_double() -> Glyph {
    Glyph {
        codepoint: 0x201C,
        advance: 250 + 2 * SIDE_BEARING,
        strokes: vec![curl(0, true), curl(190, true)],
    }
}

/// Two [`quote_right_single`] curls side by side.
fn quote_right_double() -> Glyph {
    Glyph {
        codepoint: 0x201D,
        advance: 250 + 2 * SIDE_BEARING,
        strokes: vec![curl(0, false), curl(190, false)],
    }
}

/// One raised curl, `offset` units to the right of the origin.
/// `mirrored = false` gives the `’`/`”` shape (thick end at `x=offset+60`,
/// tail curling down-left to `x=offset`); `mirrored = true` gives the
/// `‘`/`“` shape, the same curve reflected about the glyph's own centre
/// (thick end at `x=offset`, tail curling down-right to `x=offset+60`).
fn curl(offset: i16, mirrored: bool) -> Stroke {
    let (x_start, x_end) = if mirrored { (0, 60) } else { (60, 0) };
    Stroke {
        start: p(offset + x_start, RAISED_TOP),
        segs: vec![Seg::Quad { ctrl: p(offset + x_start, 540), to: p(offset + x_end, 480) }],
    }
}

/// Three [`period`]-height dots spaced far enough apart (`350` units,
/// `280` clear between adjacent discs) to read as three separate marks
/// rather than a merged blob at small render sizes.
fn ellipsis() -> Glyph {
    Glyph {
        codepoint: 0x2026,
        advance: 700 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::dot(p(35, 35)), Stroke::dot(p(385, 35)), Stroke::dot(p(735, 35))],
    }
}
