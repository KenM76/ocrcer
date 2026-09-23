//! `# % & * @ ^ | ~ ⌀ ° Ω µ ¼ ½ ¾ § ¶ † ‡ © ® ™ ‰`, math
//! (`+ < = > ± × ÷ √ ≤ ≥ ≈ ≠`), currency (`$ ¢ £ € ¥`).
//!
//! # Shared math metric
//!
//! `+ = ± ÷ ≠ ≈` are built around one shared coordinate system so their
//! strokes land on the same rows: [`MATH_MID`] is the single centre row a
//! lone bar (`+`, `÷`) sits on; [`MATH_HI`] and [`MATH_LO`] are the two rows
//! `=`/`≈` split onto, symmetric about `MATH_MID`. `≠` is [`equals`] plus a
//! slash. `≤`/`≥` are built from the shared [`chevron`] primitive, called
//! twice — once at full height for [`less_than`]/[`greater_than`], once
//! compressed into the box's upper part for [`less_equal`]/[`greater_equal`]
//! with a bar below — so the chevron itself is authored once (`CLAUDE.md`
//! rule 4) and the two pairs cannot drift apart. `±` is [`plus`]'s cross
//! (compressed) plus a second bar separated from the cross's lower arm by
//! more than `STROKE`, not a one-stroke gap, so it reads as `+` over `−`
//! rather than a taller `+`.
//!
//! # Weld-heavy glyphs
//!
//! `&`, `@`, `§`, `%`/`‰` all weld an open curve or a second closed ring
//! onto a first ring, per `mod.rs`'s weld rule (see `digits::digit_6`) —
//! their doc comments below record the specific weld points and why each
//! one avoids the early-dip failure mode.
//!
//! # Currency
//!
//! `$ ¢ £ € ¥` cross a base letterform with one or two bars. Where the bar
//! is a straight line through an open arc ([`open_c`]), the arc's mouth is
//! sized wide enough that the bar cannot cross the *same* unbroken arc
//! segment twice — two independent single crossings, not a chord that
//! seals part of the counter.

use super::super::{p, Glyph, Seg, Stroke, CAP, DESCENDER, SIDE_BEARING, STROKE, X_HEIGHT};
use super::digits;
use super::ring;
use super::upper::letter_o;
use super::upper::s_curve;

pub fn glyphs() -> Vec<Glyph> {
    vec![
        diameter_sign(),
        hash(),
        percent(),
        per_mille(),
        ampersand(),
        asterisk(),
        at_sign(),
        caret(),
        pipe(),
        tilde(),
        degree(),
        micro(),
        quarter(),
        half(),
        three_quarters(),
        section(),
        pilcrow(),
        dagger(),
        double_dagger(),
        copyright(),
        registered(),
        trademark(),
        omega(),
        plus(),
        less_than(),
        equals(),
        greater_than(),
        plus_minus(),
        multiply(),
        divide(),
        radical(),
        less_equal(),
        greater_equal(),
        approx(),
        not_equal(),
        dollar(),
        cent(),
        pound(),
        euro(),
        yen(),
    ]
}

/// Same bowl as `upper::letter_o_slash` (Ø) — the two share a circle by
/// definition. They no longer share a slash: Ø's stroke
/// (`super::bowl_slash`) stays close to the bowl on purpose (a letterform
/// diagonal, not a rule through it). A diameter sign is conventionally
/// drawn with the diagonal overshooting the circle at *both* ends, so
/// [`diameter_slash`] runs corner-to-corner of `letter_o`'s own bounding
/// box (`(0,0)` to `(490,700)`) instead — an ellipse never reaches its own
/// bounding-box corners, so this guarantees visible overshoot past the
/// ring on both ends without dropping below the baseline or rising above
/// the bowl's own top (`ascender` class only requires `lo >= 0.0`, which
/// `(0,0)` satisfies exactly). `upper::letter_o_slash` and `bowl_slash`
/// are untouched by this glyph's redraw.
fn diameter_sign() -> Glyph {
    let mut g = letter_o();
    g.codepoint = 0x2300; // ⌀ DIAMETER SIGN
    g.strokes.push(diameter_slash());
    g
}

/// See [`diameter_sign`]: corner-to-corner of `letter_o`'s bounding box,
/// longer on both ends than `bowl_slash` by construction.
fn diameter_slash() -> Stroke {
    Stroke::line(&[p(0, 0), p(490, 700)])
}

// ---- shared local helpers (kept in this file rather than `mod.rs`
// because no other slice needs them) ----

/// Three of [`ring`]'s four quadrants (`E_hi -> N -> W -> S -> E_lo`),
/// mouth open on the *east* side between `E_hi` and `E_lo` (a vertical gap
/// of `2*mouth`, not a diagonal quadrant slice) — the same wide-mouth style
/// `upper::letter_c` uses, rewritten here so this file's currency and
/// copyright glyphs don't reach into another author's module. A bar drawn
/// through the gap crosses only the unbroken west arc once; see the module
/// doc.
fn open_c(cx: i16, cy: i16, rx: i16, ry: i16, mouth: i16) -> Stroke {
    let e_hi = p(cx + rx, cy + mouth);
    let n = p(cx, cy + ry);
    let w = p(cx - rx, cy);
    let s = p(cx, cy - ry);
    let e_lo = p(cx + rx, cy - mouth);
    Stroke {
        start: e_hi,
        segs: vec![
            Seg::Quad { ctrl: p(cx + rx, cy + ry), to: n },
            Seg::Quad { ctrl: p(cx - rx, cy + ry), to: w },
            Seg::Quad { ctrl: p(cx - rx, cy - ry), to: s },
            Seg::Quad { ctrl: p(cx + rx, cy - ry), to: e_lo },
        ],
    }
}

/// A plain crossbar at `y`, `w` wide starting at `x=0` — [`+`]'s and
/// [`÷`]'s own bar, and the second bar `≤`/`≥`/`±` add to a chevron/cross.
fn bar(y: i16, w: i16) -> Stroke {
    Stroke::line(&[p(0, y), p(w, y)])
}

/// A crossbar at `y` spanning `[x0, x1]` explicitly — for bars that don't
/// start at `x = 0` (the dagger family's short high bars).
fn hbar(x0: i16, x1: i16, y: i16) -> Stroke {
    Stroke::line(&[p(x0, y), p(x1, y)])
}

/// A vertical bar at `x`, centred on `cy` with half-extent `half`.
fn vbar(x: i16, cy: i16, half: i16) -> Stroke {
    Stroke::line(&[p(x, cy - half), p(x, cy + half)])
}

/// One hump of a tilde/approx wave: a shallow S starting and ending on
/// `cy`, `w` wide, peak/trough offset by `amp` (the control points, not
/// the rendered extreme — a quadratic's midpoint reaches only half that).
/// [`tilde`]'s whole shape and one row of [`approx`], reused rather than
/// two independent wavy curves.
fn wave(cx: i16, cy: i16, w: i16, amp: i16) -> Stroke {
    let half = w / 2;
    Stroke {
        start: p(cx - half, cy),
        segs: vec![
            Seg::Quad { ctrl: p(cx - half / 2, cy + amp), to: p(cx, cy) },
            Seg::Quad { ctrl: p(cx + half / 2, cy - amp), to: p(cx + half, cy) },
        ],
    }
}

/// Looks a digit up from [`digits::glyphs`] by codepoint and returns its
/// strokes scaled by `scale` then translated by `(ox, oy)` — how `¼ ½ ¾`
/// get a numerator and denominator without a second hand-rolled numeral
/// (`CLAUDE.md` rule 4): the fraction glyphs reuse `digits::digit_N`'s own
/// authored curve, read through the module's public `glyphs()` rather than
/// a private function, instead of redrawing it at a smaller size.
fn small_digit(codepoint: u32, scale: f32, ox: i16, oy: i16) -> Vec<Stroke> {
    let digit = digits::glyphs()
        .into_iter()
        .find(|g| g.codepoint == codepoint)
        .unwrap_or_else(|| panic!("digit U+{codepoint:04X} not found for fraction reuse"));
    let t = |pt: super::super::P| p(ox + (pt.x as f32 * scale).round() as i16, oy + (pt.y as f32 * scale).round() as i16);
    digit
        .strokes
        .into_iter()
        .map(|s| Stroke {
            start: t(s.start),
            segs: s
                .segs
                .into_iter()
                .map(|seg| match seg {
                    Seg::Line(q) => Seg::Line(t(q)),
                    Seg::Quad { ctrl, to } => Seg::Quad { ctrl: t(ctrl), to: t(to) },
                })
                .collect(),
        })
        .collect()
}

// ---- symbol ----

/// Two verticals crossed by two horizontals, each overshooting the other
/// pair — the overshoot is what makes it read as `#` rather than a plain
/// grid. `full` class: verticals run from below the baseline to [`CAP`]
/// exactly. Encloses exactly one square region where all four strokes
/// cross; see the module's deliverable report for the measured count.
fn hash() -> Glyph {
    let (v1, v2) = (210, 420);
    let (h1, h2) = (260, 480);
    Glyph {
        codepoint: '#' as u32,
        advance: 600 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(v1, -70), p(v1, CAP)]), Stroke::line(&[p(v2, -70), p(v2, CAP)]), bar(h1, 600), bar(h2, 600)],
    }
}

// ---- math (shared row/width constants — see module doc) ----

/// Centre row shared by every lone-bar math sign (`+`, `÷`).
const MATH_MID: i16 = 350;
/// Upper of the two rows `=`/`≈` split onto; also `≤`/`≥`'s bar sits on
/// [`MATH_LO`], not this one — see the module doc.
const MATH_HI: i16 = 440;
/// Lower of the two rows `=`/`≈` split onto, and the row `≤`/`≥`'s added
/// bar reuses.
const MATH_LO: i16 = 260;
/// Width shared by the full-bar math relations (`+ = ± ÷ ≠ ≈` and the bars
/// of `≤ ≥`).
const MATH_W: i16 = 520;
/// Width/reach shared by the chevrons (`< >` and their `≤ ≥` derivatives).
const CHEV_W: i16 = 490;

fn plus() -> Glyph {
    Glyph {
        codepoint: '+' as u32,
        advance: MATH_W + 2 * SIDE_BEARING,
        strokes: vec![vbar(MATH_W / 2, MATH_MID, 260), bar(MATH_MID, MATH_W)],
    }
}

fn equals() -> Glyph {
    Glyph {
        codepoint: '=' as u32,
        advance: MATH_W + 2 * SIDE_BEARING,
        strokes: vec![bar(MATH_HI, MATH_W), bar(MATH_LO, MATH_W)],
    }
}

/// `=` plus a diagonal slash overshooting both bars — built from
/// [`equals`] itself so the two rows can never drift apart from `=`'s own.
fn not_equal() -> Glyph {
    let mut g = equals();
    g.codepoint = 0x2260; // ≠
    g.strokes.push(Stroke::line(&[p(60, 190), p(460, 510)]));
    g
}

/// `+`'s cross, compressed, with a second bar well clear of the lower
/// arm — real separation (`STROKE`-plus margin), not a one-stroke gap,
/// so `±` reads as `+` over `−` rather than a taller cross.
fn plus_minus() -> Glyph {
    Glyph {
        codepoint: 0xB1, // ±
        advance: MATH_W + 2 * SIDE_BEARING,
        strokes: vec![vbar(MATH_W / 2, 350, 150), bar(350, MATH_W), bar(60, MATH_W)],
    }
}

fn multiply() -> Glyph {
    let w = 460;
    Glyph {
        codepoint: 0xD7, // ×
        advance: w + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, 180), p(w, 520)]), Stroke::line(&[p(0, 520), p(w, 180)])],
    }
}

fn divide() -> Glyph {
    Glyph {
        codepoint: 0xF7, // ÷
        advance: MATH_W + 2 * SIDE_BEARING,
        strokes: vec![bar(MATH_MID, MATH_W), Stroke::dot(p(MATH_W / 2, 500)), Stroke::dot(p(MATH_W / 2, 200))],
    }
}

/// A checkmark tick dipping below the baseline into a long rising stroke,
/// then a horizontal overbar — `full` class: bottom below the baseline,
/// top touching [`CAP`] exactly.
fn radical() -> Glyph {
    Glyph {
        codepoint: 0x221A, // √
        advance: 600 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(0, 210), p(90, -70), p(220, CAP), p(600, CAP)])],
    }
}

/// A `<`/`>`-style chevron: a two-segment polyline `base -> apex -> base`
/// spanning `[y_lo, y_hi]`, apex on the vertical midline of that span. The
/// one authored shape behind `<`, `>`, `≤` and `≥` (`CLAUDE.md` rule 4) —
/// [`less_than`]/[`greater_than`] call it at the glyph's full height,
/// [`less_equal`]/[`greater_equal`] call it compressed into the upper part
/// of the same total span, leaving room for a bar below.
fn chevron(apex_x: i16, base_x: i16, y_lo: i16, y_hi: i16) -> Stroke {
    let mid = (y_lo + y_hi) / 2;
    Stroke::line(&[p(base_x, y_hi), p(apex_x, mid), p(base_x, y_lo)])
}

/// Apex on the left, opening right — point faces the smaller side, per
/// convention (`a < b` reads point-toward-`a`).
fn less_than() -> Glyph {
    Glyph { codepoint: '<' as u32, advance: CHEV_W + 2 * SIDE_BEARING, strokes: vec![chevron(0, CHEV_W, 140, 560)] }
}

/// Mirror of [`less_than`]: apex on the right, opening left.
fn greater_than() -> Glyph {
    Glyph { codepoint: '>' as u32, advance: CHEV_W + 2 * SIDE_BEARING, strokes: vec![chevron(CHEV_W, 0, 140, 560)] }
}

/// [`chevron`] compressed into `[300, 560]` (the upper part of `<`/`>`'s own
/// `[140, 560]` span) plus a bar at `y = 140` — the same lowest point `<`/`>`
/// themselves reach, so the overall glyph height is unchanged. Chevron base
/// to bar is a 160-unit gap; net clearance after the 70-unit pen eats 35
/// units from each side is 90 — comfortably past `STROKE`, unlike a bar
/// dropped through the chevron's own span (the earlier defect: `MATH_LO`,
/// 260, fell inside `[140, 560]` and cut through the lower arm).
fn less_equal() -> Glyph {
    Glyph { codepoint: 0x2264, advance: CHEV_W + 2 * SIDE_BEARING, strokes: vec![chevron(0, CHEV_W, 300, 560), bar(140, CHEV_W)] }
}

/// [`greater_than`]'s compressed twin — see [`less_equal`].
fn greater_equal() -> Glyph {
    Glyph { codepoint: 0x2265, advance: CHEV_W + 2 * SIDE_BEARING, strokes: vec![chevron(CHEV_W, 0, 300, 560), bar(140, CHEV_W)] }
}

/// Two stacked [`wave`] humps on [`MATH_HI`]/[`MATH_LO`] — the same two
/// rows `=` uses, so `≈` reads as a wavy `=`.
fn approx() -> Glyph {
    Glyph {
        codepoint: 0x2248, // ≈
        advance: MATH_W + 2 * SIDE_BEARING,
        strokes: vec![wave(MATH_W / 2, MATH_HI, MATH_W, 70), wave(MATH_W / 2, MATH_LO, MATH_W, 70)],
    }
}

// ---- weld-heavy symbols ----

/// A rising slash (lower-left to upper-right, `/`-direction — the real-world
/// convention, not its mirror) with a small ring at each end, upper-left and
/// lower-right. The two rings are far enough apart (centre distance well
/// past the sum of their radii) that they never touch the slash: three
/// independent closed/open pieces, two holes. `ascender` class: the slash's
/// lower end touches the baseline exactly.
fn percent() -> Glyph {
    Glyph {
        codepoint: '%' as u32,
        advance: 610 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(30, 0), p(610, CAP)]), ring(150, 570, 80, 80), ring(490, 130, 80, 80)],
    }
}

/// [`percent`] plus a third ring — built by calling it, not redrawing the
/// slash, so `%` and `‰` cannot drift apart.
fn per_mille() -> Glyph {
    let mut g = percent();
    g.codepoint = 0x2030; // ‰
    g.strokes.push(ring(850, 130, 80, 80));
    g.advance = 930 + 2 * SIDE_BEARING;
    g
}

/// A ring (bottom loop, touching the baseline) with a hook welded onto its
/// upper-right — same weld technique as `digits::digit_6`: the hook's
/// terminal control point lands exactly `r` from the ring's centre, and the
/// approach (checked by sampling `t = 0.5, 0.8, 0.95`) stays strictly
/// outside the ring's radius until touchdown, so no spurious sliver is
/// fenced off. A second, independent tail exits from a point *inside* the
/// ring's own centreline (the same ~30-unit inset `upper::letter_q` uses
/// for its tail) out through the wall — an interior-to-exterior crossing
/// adds no hole, per that precedent. `ascender` class: the ring touches
/// the baseline exactly.
///
/// The upper loop's terminal point `(252, 349)` sits at distance `~180.0`
/// from the ring's own centre `(190, 180)` — exactly the ring's radius, an
/// intentional tangent weld (same technique as `digits::digit_6`'s hook).
/// The defect was not the weld itself: with the second segment's control
/// point at `(300, 470)`, the *whole approach curve* ran within a few
/// units of the ring's boundary, not just its endpoint — at the curve's
/// own midpoint the centreline was only `~65` units from the ring versus
/// the `STROKE` (70) needed for the two ink bands to clear each other, so
/// the enclosed "aperture" was negative (overlapping) or a few units
/// positive along nearly its whole length. That is a corridor "far under
/// one pen" per the survival sweep, not a clean weld — it read as sealed
/// (1 hole total) at every practical size, only cracking open at 96px.
///
/// Moving the control point out to `(460, 600)` — away from the ring
/// rather than hugging it — leaves the curve's own midpoint `~330` units
/// from the ring's centre, a net clearance of `~80` units past `STROKE`
/// (more than one full pen width of clear background), tapering back down
/// to the same exact tangent touchdown only in the curve's final ~15%,
/// the way a normal loop narrows into its join. Endpoints and the weld
/// point are unchanged, so the ring's own hole and the leg are unaffected.
/// Widened, not restroked, per the aperture-not-a-new-stroke instruction.
/// Verified with `hole_count_survival_report` after the change (2 holes
/// from 24px up through the full sweep — see the deliverable report).
fn ampersand() -> Glyph {
    Glyph {
        codepoint: '&' as u32,
        advance: 555 + 2 * SIDE_BEARING,
        strokes: vec![
            ring(190, 180, 180, 180),
            Stroke { start: p(400, CAP), segs: vec![Seg::Quad { ctrl: p(60, CAP), to: p(60, 400) }, Seg::Quad { ctrl: p(460, 600), to: p(252, 349) }] },
            Stroke::line(&[p(319, 105), p(520, 20)]),
        ],
    }
}

/// Three spokes from a common centre, 60 degrees apart — `above` class.
fn asterisk() -> Glyph {
    let (cx, cy) = (310, 500);
    Glyph {
        codepoint: '*' as u32,
        advance: 320 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(cx, cy - 180), p(cx, cy + 180)]),
            Stroke::line(&[p(cx - 156, cy - 90), p(cx + 156, cy + 90)]),
            Stroke::line(&[p(cx - 156, cy + 90), p(cx + 156, cy - 90)]),
        ],
    }
}

/// Outer [`ring`] (touching the baseline and [`CAP`]) with a small,
/// *closed* inner [`ring`] standing in for the inner `a`'s bowl, plus a
/// stem tangent at the bowl's east point (the same single-point tangency
/// [`registered`]'s bowl and `upper::letter_b`/`letter_p` use — touching
/// only where the stem's straight run crosses the bowl's own rightmost
/// point adds no extra hole). Because the bowl is a full closed loop
/// (not [`open_c`]'s open mouth, which merged the counter into the outer
/// annulus and gave only 1 hole), it keeps its own counter separate from
/// the annulus between it and the outer ring: expected 2 holes. Nearest
/// approach between any two of the three strokes is the stem's top corner
/// to the outer ring, a 165-unit gap (net clearance 95 past `STROKE`) —
/// stable across the whole 16-96px sweep, not just the size checked by
/// hand. `ascender` class.
fn at_sign() -> Glyph {
    Glyph {
        codepoint: '@' as u32,
        advance: 670 + 2 * SIDE_BEARING,
        strokes: vec![ring(300, 350, 300, 350), ring(310, 300, 110, 150), Stroke::line(&[p(420, 190), p(420, 470)])],
    }
}

fn caret() -> Glyph {
    Glyph {
        codepoint: '^' as u32,
        advance: 320 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(40, 480), p(200, 650), p(360, 480)])],
    }
}

/// `ascender` class: touches the baseline exactly (`lo = 0`) — the class
/// assertion in `raster.rs` (`stroke_data_agrees_with_its_baseline_class`)
/// requires `lo >= 0.0`, so the fix that separates `|` from `l` extends the
/// *top* only, not the conventional both-ends overshoot; that is a
/// deliberate deviation from a literal "draw it taller on both ends" brief,
/// forced by that non-negotiable assertion. Top reaches `CAP + 2 * STROKE`
/// (840), the same height `accents.rs` already uses for the
/// circumflex/diaeresis marks above `CAP` — reused rather than invented.
/// `l` stops at `CAP` (700), so the two glyphs now differ in both cropped
/// bounding-box height and baseline offset, not just in name.
fn pipe() -> Glyph {
    Glyph {
        codepoint: '|' as u32,
        advance: 70 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(35, CAP + 2 * STROKE), p(35, 0)])],
    }
}

/// A single [`wave`] hump — `above` class.
fn tilde() -> Glyph {
    Glyph {
        codepoint: '~' as u32,
        advance: 460 + 2 * SIDE_BEARING,
        strokes: vec![wave(230, 350, 460, 90)],
    }
}

/// A small [`ring`] high on the body — radius (80) comfortably past half
/// the pen (`STROKE`/2 = 35), so it renders as a true annulus. `above`
/// class.
fn degree() -> Glyph {
    Glyph {
        codepoint: 0xB0, // °
        advance: 260 + 2 * SIDE_BEARING,
        strokes: vec![ring(130, 560, 130, 130)],
    }
}

/// Lowercase `u`'s bowl (redrawn locally — `lower::letter_u` is private),
/// with the left stem extended down to [`DESCENDER`] instead of stopping
/// at the x-line. `descender` class: touches [`DESCENDER`] exactly.
fn micro() -> Glyph {
    Glyph {
        codepoint: 0xB5, // µ
        advance: 410 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(35, X_HEIGHT), p(35, DESCENDER)]),
            Stroke { start: p(35, 140), segs: vec![Seg::Quad { ctrl: p(35, 0), to: p(205, 0) }, Seg::Quad { ctrl: p(375, 0), to: p(375, 140) }, Seg::Line(p(375, X_HEIGHT))] },
        ],
    }
}

const FRAC_SCALE: f32 = 0.34;

/// Numerator (via [`small_digit`]) + solidus (`/`-direction, matching
/// [`percent`]'s slash) + denominator. `ascender` class: the denominator
/// digit's own baseline point (`y = 0` in the source digit) lands at
/// `oy = 0`, so the fraction touches the baseline exactly, same as every
/// source digit does.
///
/// `digit_4`'s own diagonal-to-vertical weld at `(340, 230)` is a genuine
/// zero-gap touch — `raster.rs`'s table expects `'4'` at 0 holes, and it
/// passes at full scale — but reusing it through [`small_digit`] at
/// `FRAC_SCALE` (0.34) measured 1 spurious hole in both `¼` and `¾`
/// (`½` has no `4`): `STROKE` is a renderer-global constant that does not
/// shrink with a reused digit's scaled-down centreline, so a junction that
/// clears the pen at full scale can go negative once the centreline alone
/// is shrunk. [`small_four`] is drawn directly at final size instead, so a
/// numerator/denominator `4` never goes through `small_digit`.
fn fraction(codepoint: u32, num: u32, den: u32) -> Glyph {
    let mut strokes = if num == '4' as u32 { small_four(0, 460) } else { small_digit(num, FRAC_SCALE, 0, 460) };
    strokes.push(Stroke::line(&[p(20, 0), p(650, CAP)]));
    strokes.extend(if den == '4' as u32 { small_four(520, 0) } else { small_digit(den, FRAC_SCALE, 520, 0) });
    Glyph { codepoint, advance: 700 + 2 * SIDE_BEARING, strokes }
}

/// A purpose-built small `4`, standing in for [`small_digit`] reusing
/// `digit_4` at [`FRAC_SCALE`] — see [`fraction`]'s doc comment for why
/// that reuse pinches shut. Authored directly at final size, so it takes
/// ordinary full-scale clearance math rather than the shrunk-centreline
/// case: the arm is diagonal, matching [`digits::digit_4`]'s own top-right
/// to bottom-left lean (here `60` units over a `158`-unit drop, close to
/// `digit_4`'s own `160`-over-`470`), not the vertical stand-in the
/// previous pass left in place. Widened from 150 to 200 units so the
/// apex-to-stem gap (`200 - 60 = 140`) matches `digit_4`'s own untouched
/// clearance of exactly `2 * STROKE` — one full pen of air beyond the
/// touching threshold, not just short of it, per the two-parallel-strokes
/// rule (`personal_rag`/`fonts` clearance notes). At 150 wide the same
/// lean would put the apex only `90` units from the stem, `20` short of
/// `STROKE` itself, welding the open apex shut. See the doc comment on
/// [`fraction`] for the module-level context.
fn small_four(ox: i16, oy: i16) -> Vec<Stroke> {
    let t = |x: i16, y: i16| p(ox + x, oy + y);
    vec![Stroke::line(&[t(60, 238), t(0, 80), t(200, 80)]), Stroke::line(&[t(200, 238), t(200, 0)])]
}

fn quarter() -> Glyph {
    fraction(0xBC, '1' as u32, '4' as u32)
}

fn half() -> Glyph {
    fraction(0xBD, '1' as u32, '2' as u32)
}

fn three_quarters() -> Glyph {
    fraction(0xBE, '3' as u32, '4' as u32)
}

/// The upper bowl of `§`: a full [`ring`] — the counter, closed the same
/// proven way `digits::digit_8`'s two bowls are (a complete loop, no
/// delicate tangency), not an arc-plus-chord — plus a hook stroke that
/// lands exactly on the ring's own `N` point, `(200, 560)`: distance from
/// the ring's centre `(200, 440)` is `560 - 440 = 120 = r`, exactly, per
/// `mod.rs`'s weld rule (the same standard `digits::digit_6`'s hook is
/// held to). The hook's two quadratics each keep both endpoints and their
/// control point at `y >= 560` (the ring's own topmost point), so by the
/// convex-hull property of a quadratic Bézier the whole path stays at
/// `y >= 560` throughout — it cannot dip inside the ring before the one
/// exact touchdown, the "approach from outside" half of the same rule
/// (`a_near_tangent_stroke_opens_a_false_counter.md`: this is what a
/// verified approach looks like, not a box-containment guess).
///
/// A first design closed each bowl with a straight `S -> N` chord across
/// the ring's own diameter instead of using a full ring; rendered and read
/// per rule 8, the two chords stacked into one dominant vertical bar and
/// the glyph read as a circle bisected by a line, not a threaded `S`. This
/// ring-plus-hook redraw is the reported deviation from a literal
/// single-open-curve "bowl": the counter is a closed loop, and the hook
/// supplies the free terminal and the open, sweeping curl the brief
/// described, the same `digit_6` division of labour between a loop and its
/// hook.
///
/// `(365, 685)`, the hook's start, is the free terminal — the structural
/// fact separating `§`'s two lobes from `digit_8`'s two closed rings.
fn upper_bowl() -> [Stroke; 2] {
    let counter = ring(200, 440, 120, 120);
    let hook = Stroke {
        start: p(365, 685),
        segs: vec![
            Seg::Quad { ctrl: p(90, 685), to: p(90, 560) },
            Seg::Quad { ctrl: p(90, 610), to: p(200, 560) },
        ],
    };
    [counter, hook]
}

/// Point-reflects a stroke through `(cx, cy)` — 180-degree rotational
/// symmetry built as one transform applied to one construction
/// ([`upper_bowl`]), not a second hand-mirrored pair of paths (`CLAUDE.md`
/// rule 4).
fn rot180(s: &Stroke, cx: i16, cy: i16) -> Stroke {
    let reflect = |pt: super::super::P| p(2 * cx - pt.x, 2 * cy - pt.y);
    Stroke {
        start: reflect(s.start),
        segs: s
            .segs
            .iter()
            .map(|seg| match *seg {
                Seg::Line(to) => Seg::Line(reflect(to)),
                Seg::Quad { ctrl, to } => Seg::Quad { ctrl: reflect(ctrl), to: reflect(to) },
            })
            .collect(),
    }
}

/// [`upper_bowl`] plus its [`rot180`] reflection through the glyph's own
/// centre `(200, 350)`. The lower bowl's counter sits at `(200, 260)`,
/// r=120, spanning `140..380`; the upper counter spans `320..560`. The two
/// counters overlap `380 - 320 = 60` design units at the waist —
/// centreline-to-centreline, not box-to-box — the same overlap magnitude
/// `digit_8`'s own two rings already prove survives rasterisation as two
/// separate holes rather than a leaked one, not a fresh guess. At 16px/em
/// (the smallest gated size) that is `60 * 16 / 1000 = 0.96px` of genuine
/// overlap, comfortably more than one full pen (`STROKE = 70` design
/// units, `1.12px` at 16px) once the `-70` pen width is folded in
/// (`overlap - STROKE` is negative, i.e. the ink bands themselves
/// intersect, not merely approach). The lower bowl's terminal lands at the
/// reflection of `(365, 685)`, `(35, 15)` — lower left, near the baseline,
/// matching the upper terminal's near-CAP placement by construction rather
/// than by a second measurement.
///
/// Bounding box (centreline): x `35..365` (the two terminals), y `15..685`
/// (also the two terminals) — both hooks reach slightly short of `CAP`/the
/// baseline (`685`/`15`, not `700`/`0`); `ascender` class only requires
/// `lo >= 0 && hi > X_HEIGHT` (`raster.rs`), which this clears with margin,
/// so the 15-unit shortfall costs nothing. Ink half-width from the ring
/// alone is `120 + 35 = 155`, but the hook terminals reach further:
/// `365 - 35 = 330` centreline width, `330 + 70 = 400` ink width,
/// `aspect = 400 / 700 = 0.571` — inside the 0.55-0.58 target, inside the
/// `model/charset.tsv` 0.45-0.65 band.
fn section() -> Glyph {
    let [upper_ring, upper_hook] = upper_bowl();
    let cx = 200;
    let cy = CAP / 2;
    let lower_ring = rot180(&upper_ring, cx, cy);
    let lower_hook = rot180(&upper_hook, cx, cy);
    Glyph {
        codepoint: 0xA7, // §
        advance: 330 + 2 * SIDE_BEARING,
        strokes: vec![upper_ring, upper_hook, lower_ring, lower_hook],
    }
}

/// A [`ring`] with two stems tangent at its west and east points (the same
/// single-point tangency `upper::letter_b`/`letter_p` use against their own
/// bowls — adds no hole), both reaching [`DESCENDER`]. `descender` class.
fn pilcrow() -> Glyph {
    Glyph {
        codepoint: 0xB6, // ¶
        advance: 460 + 2 * SIDE_BEARING,
        strokes: vec![ring(300, 560, 140, 140), Stroke::line(&[p(160, 560), p(160, DESCENDER)]), Stroke::line(&[p(440, CAP), p(440, DESCENDER)])],
    }
}

/// Stem to [`CAP`] exactly, bar high (`y = 580`) — structurally distinct
/// from `lower::letter_t`, which stops at `T_TOP` (~647, short of `CAP`)
/// and crosses at `y = 400`. Margin: stem 700 vs 647 (53 units), bar 580 vs
/// 400 (180 units) — the bar gap alone is well over twice the stroke
/// diameter. `ascender` class: touches the baseline exactly.
fn dagger() -> Glyph {
    Glyph {
        codepoint: 0x2020, // †
        advance: 290 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(150, CAP), p(150, 0)]), hbar(40, 260, 580)],
    }
}

/// [`dagger`]'s stem with two bars, `y = 580` and `y = 360`, separated by
/// 220 units — unmistakably two, and neither collides with `t`'s single
/// bar at `y = 400` (nearest margin 40 units, but distinguished primarily
/// by *count*: `t` crosses its stem once, `‡` twice).
fn double_dagger() -> Glyph {
    Glyph {
        codepoint: 0x2021, // ‡
        advance: 290 + 2 * SIDE_BEARING,
        strokes: vec![Stroke::line(&[p(150, CAP), p(150, 0)]), hbar(40, 260, 580), hbar(40, 260, 360)],
    }
}

/// Outer [`ring`] (touching the baseline and [`CAP`]) with a non-touching
/// [`open_c`] inside standing in for `C` — real-letter reuse is unavailable
/// (`upper::letter_c` is private), so this is a local equivalent, not a
/// second hand-rolled `C`. Widened from `mouth = 60` (net clearance between
/// the mouth's floating endpoints only `2*60 - STROKE = 50`, which the
/// 5x5 subgrid sampler welded shut at 16px) to `mouth = 120`
/// (`2*120 - STROKE = 170` net clearance): the inner `C`'s mouth stays open
/// at every sweep size. Expected 2 holes — the inner counter, kept apart
/// from the annulus by the open mouth's own two endpoints never meeting.
/// `ascender` class.
fn copyright() -> Glyph {
    Glyph {
        codepoint: 0xA9, // ©
        advance: 620 + 2 * SIDE_BEARING,
        strokes: vec![ring(300, 350, 275, 350), open_c(300, 350, 130, 160, 120)],
    }
}

/// Outer [`ring`] with a closed `R` (stem, tangent bowl, open leg) inside,
/// clear of the ring on every side. The bowl closes fully against the
/// stem (a true second loop, unlike [`copyright`]'s open `C`): expected 2
/// holes — the bowl's counter and the outer annulus, kept separate because
/// the bowl never touches the ring. Bowl enlarged from a 120x80 loop
/// (`ctrl` reaching only to `x = 340`, `y` spanning `490..350`) to a
/// 220x120 loop (`ctrl` to `x = 440`, `y` spanning `490..270`): the
/// original bowl's counter was under one pen width tall/wide and sealed at
/// 16px; the enlarged bowl carries a full pen of clear interior at every
/// sweep size. `ascender` class.
fn registered() -> Glyph {
    Glyph {
        codepoint: 0xAE, // ®
        advance: 620 + 2 * SIDE_BEARING,
        strokes: vec![
            ring(300, 350, 275, 350),
            Stroke::line(&[p(220, 490), p(220, 210)]),
            Stroke { start: p(220, 490), segs: vec![Seg::Quad { ctrl: p(440, 490), to: p(440, 380) }, Seg::Quad { ctrl: p(440, 270), to: p(220, 270) }] },
            Stroke::line(&[p(220, 270), p(380, 210)]),
        ],
    }
}

/// A `T` over a widened `M` — `above` class. At 48px the `M`'s original
/// 180-unit width rendered to `180 * 48/1000 ≈ 8.6` px — under the roughly
/// four pen-widths (`4 * STROKE * 48/1000 ≈ 13.4` px) its three strokes and
/// two counters need to read as distinct — and that four-pen-width minimum
/// is a design-unit width of `4 * STROKE ≈ 280` regardless of render size,
/// since `width / STROKE` is scale-invariant. Widened to 360 (`280` plus
/// margin, checked at both 24px and 48px) — comfortably clear at both
/// because the same ratio argument means whatever holds at one size holds
/// at the other.
///
/// `T`'s stem used to stop at `y = 470`, two-thirds of the `M`'s own
/// `350..650` span, so the two letters sat at different cap heights on a
/// shared top edge. Lowered to `y = 350` — the `M`'s own baseline — so
/// both letters share one baseline and one cap height. No further
/// widening needed: the `T`'s footprint (`0..220`) was never the
/// constraint, the `M`'s was.
fn trademark() -> Glyph {
    Glyph {
        codepoint: 0x2122, // ™
        advance: 660 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, 650), p(220, 650)]),
            Stroke::line(&[p(110, 650), p(110, 350)]),
            Stroke::line(&[p(280, 350), p(280, 650), p(460, 470), p(640, 650), p(640, 350)]),
        ],
    }
}

/// Near-closed ring on two splayed feet, built as a single open arch (no
/// closed loop): expected 0 holes. `ascender` class: both feet touch the
/// baseline exactly. Each leg's straight vertical run (`x = 140`/`460`)
/// ends in a horizontal foot flaring out to `x = 0`/`600` — a 140-unit
/// outward reach, exactly two pen widths (`2 * STROKE`), comfortably more
/// than the one pen width the leg's own ink already occupies, so the foot
/// reads as a flare distinct from the leg rather than just its thickness.
/// Visible at 24px: `140 * 24/1000 ≈ 3.4` px of outward reach.
fn omega() -> Glyph {
    Glyph {
        codepoint: 0x3A9, // Ω
        advance: 600 + 2 * SIDE_BEARING,
        strokes: vec![Stroke {
            start: p(0, 0),
            segs: vec![
                Seg::Line(p(140, 130)),
                Seg::Line(p(140, 340)),
                Seg::Quad { ctrl: p(140, 650), to: p(300, CAP) },
                Seg::Quad { ctrl: p(460, 650), to: p(460, 340) },
                Seg::Line(p(460, 130)),
                Seg::Line(p(600, 0)),
            ],
        }],
    }
}

// ---- currency ----

/// [`upper::s_curve`] (the same `S` `letter_s` draws, not a second
/// hand-mirrored copy — `CLAUDE.md` rule 4) in box `x: 10..390, y: 90..590`,
/// crossed by one bar. The curve's own waist vertex lands at `x = 220`
/// (`10 + 0.552_632 * 380`, `s_curve`'s middle control fraction), 20 units
/// off the bar's `x = 240` — the bar cannot land exactly on the curve's own
/// cusp, which is what would risk an unstable, resolution-dependent seal
/// there. `full` class: bar below the baseline and touching [`CAP`] exactly.
fn dollar() -> Glyph {
    Glyph {
        codepoint: '$' as u32,
        advance: 450 + 2 * SIDE_BEARING,
        strokes: vec![s_curve(10, 390, 90, 590), vbar(240, 315, 385)],
    }
}

/// [`open_c`] crossed by one vertical bar off-centre (`x = 200`, not the
/// arc's own `cx = 170`) — for the same reason as [`dollar`]'s bar offset:
/// avoids the bar coinciding with a point already on the arc, which would
/// otherwise seal the west lobe into a false counter. `full` class: bar
/// below the baseline and touching [`CAP`] exactly.
fn cent() -> Glyph {
    Glyph {
        codepoint: 0xA2, // ¢
        advance: 370 + 2 * SIDE_BEARING,
        strokes: vec![open_c(170, 245, 150, 170, 70), vbar(200, 315, 385)],
    }
}

/// An `L` (single polyline, stem + foot) with a small curl at the top and
/// one crossbar through the stem. `ascender` class: the stem/foot polyline
/// touches the baseline exactly.
fn pound() -> Glyph {
    Glyph {
        codepoint: 0xA3, // £
        advance: 450 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(150, CAP), p(150, 0), p(400, 0)]),
            Stroke { start: p(150, CAP), segs: vec![Seg::Quad { ctrl: p(20, CAP), to: p(20, 600) }] },
            hbar(50, 280, 350),
        ],
    }
}

/// [`open_c`] with `mouth = 300`, so the arc's open endpoints
/// (`cy ± mouth` = 50 and 650) clear each crossbar (`y` = 260 and 440)
/// by 190 units — well more than `STROKE` (70), so the two ink discs never
/// touch. An earlier `mouth = 190` (endpoints 100 units from the bars, 30
/// units of net clearance past the pen) sealed both counters shut at 28px
/// only — a phase-dependent weld from the 5x5 subgrid sampler, not a clean
/// clearance running out, so the fix widens well past that margin rather
/// than chasing the specific size. `ascender` class: the arc's own south
/// point touches the baseline exactly (see [`open_c`]).
fn euro() -> Glyph {
    Glyph {
        codepoint: 0x20AC, // €
        advance: 470 + 2 * SIDE_BEARING,
        strokes: vec![open_c(280, 350, 190, 350, 300), hbar(70, 470, 440), hbar(70, 470, 260)],
    }
}

/// A `Y` (two diagonals to a junction, one stem to the baseline) crossed
/// by two bars through the stem, `y = 300` and `y = 120` — a 180-unit
/// centre separation, net clearance `180 - STROKE = 110` after the pen
/// eats 35 units from each bar. The original bars (`260`, `180`) were only
/// 80 units apart against the 70-unit pen — 10 units clear, which welded
/// the two into one bar. Both stay inside the stem region and above the
/// baseline. `ascender` class: the stem touches the baseline exactly.
fn yen() -> Glyph {
    Glyph {
        codepoint: 0xA5, // ¥
        advance: 470 + 2 * SIDE_BEARING,
        strokes: vec![
            Stroke::line(&[p(0, CAP), p(235, 350), p(470, CAP)]),
            Stroke::line(&[p(235, 350), p(235, 0)]),
            hbar(150, 320, 300),
            hbar(150, 320, 120),
        ],
    }
}
