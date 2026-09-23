//! `A`–`Z` plus `Æ` and `Ø`.
//!
//! `A V W X Y K M N Z` and `E F H I L T` are straight-line only, per ISO
//! 3098 Type B. `B D P R` are a stem plus one or two bowls welded onto the
//! stem's own centreline (the same weld rule [`super::ring`]'s doc uses for
//! `digit_6`/`digit_9`, applied to a straight stem instead of a closed
//! ring: each bowl quadrant's control point is the exact tangent-corner of
//! its ellipse quadrant, which keeps the bowl's x-coordinate monotonic
//! along the curve, so it touches the stem only at its two authored
//! endpoints and never crosses it early). `C G S J U` are open curves.
//! `Q` and `Æ` reuse another letter's shape rather than redrawing it.

use super::super::{p, Glyph, Seg, Stroke, CAP, SIDE_BEARING};
use super::{bowl_slash, ring};

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
        letter_cap_i(),
        letter_j(),
        letter_k(),
        letter_l(),
        letter_m(),
        letter_n(),
        letter_o(),
        letter_o_slash(),
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
        letter_ae(),
    ]
}

fn letter_a() -> Glyph {
    Glyph {
        codepoint: 'A' as u32,
        advance: 470 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, 0), p(235, CAP), p(470, 0)]),
            Stroke::line(&[p(84, 250), p(386, 250)]),
        ],
    }
}

/// Upper bowl welds to the stem at `(0, CAP)` and `(0, 390)`; lower bowl at
/// `(0, 390)` and `(0, 0)` — two closed loops sharing one point on the
/// stem, not one loop split in half. The lower bowl is wider (`400` vs
/// `340`), per ISO 3098's smaller-upper-bowl convention.
fn letter_b() -> Glyph {
    Glyph {
        codepoint: 'B' as u32,
        advance: 400 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, CAP), p(0, 0)]),
            Stroke {
                start: p(0, CAP),
                segs: vec![
                    Seg::Quad { ctrl: p(340, CAP), to: p(340, 545) },
                    Seg::Quad { ctrl: p(340, 390), to: p(0, 390) },
                ],
            },
            Stroke {
                start: p(0, 390),
                segs: vec![
                    Seg::Quad { ctrl: p(400, 390), to: p(400, 195) },
                    Seg::Quad { ctrl: p(400, 0), to: p(0, 0) },
                ],
            },
        ],
    }
}

/// Open curve, mouth on the east side spanning `y=140..460` (measured at
/// the mouth's own x); the arc otherwise touches `x=0` at its west point
/// and `y=0`/`y=CAP` at south/north, same discipline as [`super::ring`]'s
/// cardinal points.
fn letter_c() -> Glyph {
    Glyph {
        codepoint: 'C' as u32,
        advance: 430 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(430, 560),
            segs: vec![
                Seg::Quad { ctrl: p(430, CAP), to: p(215, CAP) },
                Seg::Quad { ctrl: p(0, CAP), to: p(0, 350) },
                Seg::Quad { ctrl: p(0, 0), to: p(215, 0) },
                Seg::Quad { ctrl: p(430, 0), to: p(430, 140) },
            ],
        }],
    }
}

fn letter_d() -> Glyph {
    Glyph {
        codepoint: 'D' as u32,
        advance: 470 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, CAP), p(0, 0)]),
            Stroke {
                start: p(0, CAP),
                segs: vec![
                    Seg::Quad { ctrl: p(470, CAP), to: p(470, 350) },
                    Seg::Quad { ctrl: p(470, 0), to: p(0, 0) },
                ],
            },
        ],
    }
}

/// Middle arm shorter than the top/bottom bars (`320` vs `400`) — not
/// vertically symmetric by eye, which is why `E` is not in
/// `raster::tests::round_glyphs_are_vertically_symmetric` even though this
/// particular pair of bar lengths happens to reflect cleanly row-for-row.
fn letter_e() -> Glyph {
    Glyph {
        codepoint: 'E' as u32,
        advance: 400 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(400, CAP), p(0, CAP), p(0, 0), p(400, 0)]),
            Stroke::line(&[p(0, 350), p(320, 350)]),
        ],
    }
}

fn letter_f() -> Glyph {
    Glyph {
        codepoint: 'F' as u32,
        advance: 370 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, 0), p(0, CAP), p(370, CAP)]),
            Stroke::line(&[p(0, 350), p(300, 350)]),
        ],
    }
}

/// Same arc as [`letter_c`] scaled to `G`'s own width, its mouth closed
/// down to a `120`-unit gap and continued as a flat bar into the counter —
/// no downward spur, per ISO 3098.
fn letter_g() -> Glyph {
    Glyph {
        codepoint: 'G' as u32,
        advance: 440 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(440, 460),
            segs: vec![
                Seg::Quad { ctrl: p(440, CAP), to: p(220, CAP) },
                Seg::Quad { ctrl: p(0, CAP), to: p(0, 350) },
                Seg::Quad { ctrl: p(0, 0), to: p(220, 0) },
                Seg::Quad { ctrl: p(440, 0), to: p(440, 260) },
                Seg::Line(p(240, 260)),
            ],
        }],
    }
}

fn letter_h() -> Glyph {
    Glyph {
        codepoint: 'H' as u32,
        advance: 470 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, 0), p(0, CAP)]),
            Stroke::line(&[p(470, 0), p(470, CAP)]),
            Stroke::line(&[p(0, 350), p(470, 350)]),
        ],
    }
}

/// Serifed, unlike `lower::letter_lower_l`: real single-stroke technical
/// faces leave capital I and lowercase l visually identical, which is
/// exactly the ambiguity that motivates giving I a top and bottom bar
/// here, and it exercises the tight-crop logic on a T-junction shape
/// rather than a bare stem.
fn letter_cap_i() -> Glyph {
    Glyph {
        codepoint: 'I' as u32,
        advance: 140 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, CAP), p(140, CAP)]),
            Stroke::line(&[p(70, CAP), p(70, 0)]),
            Stroke::line(&[p(0, 0), p(140, 0)]),
        ],
    }
}

/// Stays within `y=0..CAP`, unlike a cursive J's below-baseline hook —
/// `J` is charset `baseline_class=ascender`, not `descender`, so its hook
/// has to land on the baseline rather than cross it.
fn letter_j() -> Glyph {
    Glyph {
        codepoint: 'J' as u32,
        advance: 280 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(280, CAP),
            segs: vec![
                Seg::Line(p(280, 160)),
                Seg::Quad { ctrl: p(280, 0), to: p(80, 0) },
                Seg::Quad { ctrl: p(0, 0), to: p(0, 110) },
            ],
        }],
    }
}

/// The two diagonals meet the stem at one shared point, `(0, 350)`, not at
/// two different heights — a single stroke through the junction, the same
/// T/V-junction pattern `letter_y` uses.
fn letter_k() -> Glyph {
    Glyph {
        codepoint: 'K' as u32,
        advance: 440 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, CAP), p(0, 0)]),
            Stroke::line(&[p(440, CAP), p(0, 350), p(440, 0)]),
        ],
    }
}

fn letter_l() -> Glyph {
    Glyph {
        codepoint: 'L' as u32,
        advance: 370 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, CAP), p(0, 0), p(370, 0)])],
    }
}

/// The middle vertex dips to `y=200`, not the baseline — a dip to `0`
/// would read as two triangles rather than one letter.
fn letter_m() -> Glyph {
    Glyph {
        codepoint: 'M' as u32,
        advance: 580 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, 0), p(0, CAP), p(290, 0), p(580, CAP), p(580, 0)])],
    }
}

fn letter_n() -> Glyph {
    Glyph {
        codepoint: 'N' as u32,
        advance: 470 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, 0), p(0, CAP), p(470, 0), p(470, CAP)])],
    }
}

/// An oval, not a circle (`rx=245 != ry=350`): the centreline touches
/// `y = 0` and `y = CAP` exactly, the same rule as every other ascender
/// here, and `CAP` is taller than a circle of this width would allow.
///
/// `pub(super)`, not private: `symbols::diameter_sign` shares this exact
/// bowl, and a second hand-drawn `O` would be precisely the silent-
/// divergence failure this face's write-once rule exists to prevent.
pub(super) fn letter_o() -> Glyph {
    Glyph { codepoint: 'O' as u32, advance: 490 + 2 * SIDE_BEARING, strokes: vec![ring(245, 350, 245, 350)] }
}

/// Same bowl as [`letter_o`] — true of the real characters, not a
/// shortcut — plus a slash crossing the *full* counter. That full crossing
/// is load-bearing: it is what severs the enclosed background into two
/// regions instead of one, which is the entire reason this face carries the
/// diameter signs.
fn letter_o_slash() -> Glyph {
    let mut g = letter_o();
    g.codepoint = 0x00D8; // Ø LATIN CAPITAL LETTER O WITH STROKE
    g.strokes.push(bowl_slash());
    g
}

fn letter_p() -> Glyph {
    Glyph {
        codepoint: 'P' as u32,
        advance: 400 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, CAP), p(0, 0)]),
            Stroke {
                start: p(0, CAP),
                segs: vec![
                    Seg::Quad { ctrl: p(400, CAP), to: p(400, 525) },
                    Seg::Quad { ctrl: p(400, 350), to: p(0, 350) },
                ],
            },
        ],
    }
}

/// [`letter_o`]'s bowl plus a tail that leaves it at the lower right and
/// runs *outward* at 45 degrees. `(408, 144)` sits 32 units inside the
/// bowl's own centreline — under half a stroke, so the tail's inner end is
/// already covered by the ring's ink and the crossing reads as a join
/// rather than a spur — and `(544, 8)` is clear of the bowl entirely. A
/// segment from an interior point out through the wall leaves the counter
/// one region (the free end can always be walked around), so the
/// unclamped hole count stays at `Q`'s 1; a tail that reached the *far*
/// wall would sever it the way [`bowl_slash`] deliberately does for `Ø`.
/// The outer end stops at `y = 8` so the tail's ink bottoms out level with
/// the bowl's, keeping `Q` in its declared `ascender` baseline class.
fn letter_q() -> Glyph {
    let mut g = letter_o();
    g.codepoint = 'Q' as u32;
    g.strokes.push(Stroke::line(&[p(408, 144), p(544, 8)]));
    g
}

/// [`letter_p`]'s bowl (narrower, `380` not `400`) plus a straight leg from
/// the bowl's stem weld at `(0, 350)` out to the bottom-right corner.
fn letter_r() -> Glyph {
    Glyph {
        codepoint: 'R' as u32,
        advance: 440 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, CAP), p(0, 0)]),
            Stroke {
                start: p(0, CAP),
                segs: vec![
                    Seg::Quad { ctrl: p(380, CAP), to: p(380, 525) },
                    Seg::Quad { ctrl: p(380, 350), to: p(0, 350) },
                ],
            },
            Stroke::line(&[p(0, 350), p(440, 0)]),
        ],
    }
}

/// [`digit_3`](super::digits::digit_3)'s top bump unchanged, its bottom
/// bump mirrored left instead of continuing right — the opposite-handed
/// bumps are what separates `S` from `3`, which bulges the same way twice.
/// Upper bowl descending on the left, lower bowl on the right, terminals
/// upper-right and lower-left. An earlier version had every one of those
/// reversed — a clean, well-proportioned `Ƨ`, which passed every height,
/// aspect and hole-count check in the suite because a mirror image changes
/// none of them. Handedness is only ever caught by looking at the glyph.
fn letter_s() -> Glyph {
    Glyph { codepoint: 'S' as u32, advance: 380 + 2 * SIDE_BEARING, strokes: vec![s_curve(20, 400, 0, CAP)] }
}

/// The `S` stroke above, generalised to any box `[x0, x1] x [y0, y1]` and
/// exposed to `symbols::dollar` (`CLAUDE.md` rule 4 — one `S` shape, not a
/// second hand-mirrored copy). Control-point fractions are `letter_s`'s own
/// original coordinates (box `x: 20..400`, `y: 0..CAP`) divided by that
/// box's width/height, so stretching the box reproduces the exact same
/// curve, right-handed at every size.
pub(super) fn s_curve(x0: i16, x1: i16, y0: i16, y1: i16) -> Stroke {
    let w = (x1 - x0) as f32;
    let h = (y1 - y0) as f32;
    let x = |f: f32| x0 + (f * w).round() as i16;
    let y = |f: f32| y0 + (f * h).round() as i16;
    Stroke {
        start: p(x(1.0), y(1.0)),
        segs: vec![
            Seg::Quad { ctrl: p(x(0.0), y(1.0)), to: p(x(0.0), y(0.742_857)) },
            Seg::Quad { ctrl: p(x(0.0), y(0.571_429)), to: p(x(0.552_632), y(0.514_286)) },
            Seg::Quad { ctrl: p(x(1.0), y(0.457_143)), to: p(x(1.0), y(0.257_143)) },
            Seg::Quad { ctrl: p(x(1.0), y(0.0)), to: p(x(0.0), y(0.0)) },
        ],
    }
}

fn letter_t() -> Glyph {
    Glyph {
        codepoint: 'T' as u32,
        advance: 400 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, CAP), p(400, CAP)]), Stroke::line(&[p(200, CAP), p(200, 0)])],
    }
}

fn letter_u() -> Glyph {
    Glyph {
        codepoint: 'U' as u32,
        advance: 470 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(0, CAP),
            segs: vec![
                Seg::Line(p(0, 245)),
                Seg::Quad { ctrl: p(0, 0), to: p(235, 0) },
                Seg::Quad { ctrl: p(470, 0), to: p(470, 245) },
                Seg::Line(p(470, CAP)),
            ],
        }],
    }
}

fn letter_v() -> Glyph {
    Glyph {
        codepoint: 'V' as u32,
        advance: 470 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, CAP), p(235, 0), p(470, CAP)])],
    }
}

/// The middle vertex (`325, 450`) sits above the baseline, unlike `M`'s
/// dip: two full-height V's meeting at the baseline would read as
/// disconnected triangles rather than one letter.
fn letter_w() -> Glyph {
    Glyph {
        codepoint: 'W' as u32,
        advance: 650 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, CAP), p(163, 0), p(325, CAP), p(487, 0), p(650, CAP)])],
    }
}

fn letter_x() -> Glyph {
    Glyph {
        codepoint: 'X' as u32,
        advance: 470 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, CAP), p(470, 0)]), Stroke::line(&[p(0, 0), p(470, CAP)])],
    }
}

/// The junction point `(235, 350)` is shared between the two arm strokes
/// (as one polyline, the same trick [`letter_k`] uses) and the stem below
/// it — three strokes meeting at a point, not a closed loop.
fn letter_y() -> Glyph {
    Glyph {
        codepoint: 'Y' as u32,
        advance: 470 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, CAP), p(235, 350), p(470, CAP)]),
            Stroke::line(&[p(235, 350), p(235, 0)]),
        ],
    }
}

fn letter_z() -> Glyph {
    Glyph {
        codepoint: 'Z' as u32,
        advance: 400 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, CAP), p(400, CAP), p(0, 0), p(400, 0)])],
    }
}

/// `A` and `E` sharing the `A`'s right leg as `E`'s stem: the leg runs
/// straight from `(0, 0)` to the shared stem's top `(260, CAP)` instead of
/// forming its own apex, and `E`'s three bars hang off that stem to the
/// right. Only the `A` half closes a loop (crossbar to apex to stem), so
/// the hole count is 1, not 2.
fn letter_ae() -> Glyph {
    Glyph {
        codepoint: 0x00C6, // Æ LATIN CAPITAL LETTER AE
        advance: 680 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, 0), p(260, CAP)]),
            Stroke::line(&[p(260, CAP), p(260, 0)]),
            Stroke::line(&[p(93, 250), p(260, 250)]),
            Stroke::line(&[p(260, CAP), p(680, CAP)]),
            Stroke::line(&[p(260, 350), p(600, 350)]),
            Stroke::line(&[p(260, 0), p(680, 0)]),
        ],
    }
}
