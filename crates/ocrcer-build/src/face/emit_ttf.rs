//! Writes the authored face out as a real, loadable TrueType (`.ttf`) font.
//!
//! The stroke data in [`super::glyphs`] is the sole source of truth for this
//! project's OCR model; this module is a one-way by-product of it, useful
//! for previewing the face, printing specimen sheets, or feeding a training
//! corpus renderer. Nothing in `ocrcer-core` or the prototype bank ever
//! reads a font — the bank rasterises stroke data directly through
//! [`super::raster`].
//!
//! # How a glyph becomes an outline
//!
//! This is not a centreline-to-outline offsetter. A stroke is flattened
//! with [`raster::flatten_stroke`] — the exact function [`raster::render`]
//! uses to rasterise the same stroke — and every flattened segment becomes
//! one rectangle, the segment swept sideways by half [`STROKE`] on each
//! side. A disc of the same radius, drawn as eight quadratic Béziers (see
//! [`circle_contour`]), is added only where the swept pen's round profile
//! is actually exposed: always at an open stroke's two endpoints (the round
//! cap); at an interior vertex when the flattened polyline has turned
//! there — counting *since the last disc*, not just since the previous
//! vertex — by more than [`CORNER_THRESHOLD_DEG`]; and at a vertex that is a
//! strict local extreme of `x` or `y` among its own flattened neighbours
//! (see [`is_axis_extremum`]) regardless of turn angle, because that is
//! exactly the point a bounding-box comparison reads a shape's extent from.
//! Counting the turn cumulatively matters on a tightly curved flattened arc:
//! no single step there turns enough to trip the threshold alone, but left
//! uncounted the rectangles drift away from the true swept boundary over the
//! run the same way one real corner would. See [`stroke_contours_with_quad_steps`] for the
//! exact rule, including how a stroke that closes on itself (flattened
//! start == end, e.g. [`super::glyphs::ring`]) has no exposed cap and has
//! its closing turn tested the same cumulative, extremum-checked way.
//!
//! Every contour is emitted clockwise (TrueType's y-up winding convention
//! for outer contours). Overlapping same-direction contours union under the
//! non-zero winding fill rule the same way overlapping pen stamps union
//! under [`raster`]'s own "is any segment within range" test — this module
//! never computes a union itself, it just emits enough overlapping clockwise
//! shapes that the rasteriser computes one for it.
//!
//! # What the file contains
//!
//! A minimal but complete SFNT: `head, hhea, hmtx, maxp, cmap` (format 4,
//! Windows/Unicode-BMP), `glyf, loca, name, post, OS/2`. Every glyph is a
//! simple (non-composite) glyph — the authored data has already flattened
//! accents onto their base glyph's stroke list by the time this module sees
//! it, so there is no base/mark split left to express as a composite.
//! `unitsPerEm` is [`UPM`]; every other metric is read from the authored
//! [`Glyph`] data, never invented. Glyph 0 is `.notdef` with an empty
//! outline; every other glyph index is one [`super::glyphs::glyphs`] entry,
//! assigned in ascending codepoint order.
//!
//! # What is left out, deliberately
//!
//! `achVendID` (`"OCRc"`) is not a registered vendor ID — none exists for
//! this project — and is included only because the field is mandatory. The
//! `OS/2 ulUnicodeRange` bits are set only for the two blocks this reader
//! is confident about (Basic Latin, Latin-1 Supplement); the charset also
//! reaches into Greek, General Punctuation, Currency Symbols, Letterlike
//! Symbols, Mathematical Operators and Miscellaneous Technical, and rather
//! than guess bit numbers those are left unset. `name` carries no nameID 14
//! (License URL) — the project has no published URL for its licence, only
//! the `LICENSE` file, whose text nameID 13 quotes.

use super::raster;
use super::{Glyph, Stroke, CAP, DESCENDER, STROKE, UPM, X_HEIGHT};

use crate::outline::Contour;

fn round_i16(v: f64) -> i16 {
    v.round() as i16
}

/// The pen swept sideways by `radius` along `a -> b`: a closed clockwise
/// quad, `A+rn, B+rn, B-rn, A-rn` for the unit normal `n` of `a -> b`. That
/// point order is clockwise in y-up space regardless of which way `a -> b`
/// points, by the shoelace formula.
fn rect_contour(a: (f64, f64), b: (f64, f64), radius: f64) -> Contour {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    let (nx, ny) = (-dy / len * radius, dx / len * radius);
    [(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)]
        .into_iter()
        .map(|(x, y)| (round_i16(x), round_i16(y), true))
        .collect()
}

/// A disc of `radius` at `c`, as eight clockwise quadratic Béziers, each
/// spanning a 45° arc with its control point at the tangent-line
/// intersection, `radius * tan(22.5°)` outward from the on-curve point
/// along the arc's tangent at that point.
///
/// This disc is stamped, unmodified, as the visible boundary itself — a
/// stroke's cap, or a genuine corner (see [`stroke_contours_with_quad_steps`]) — most
/// visibly at an isolated [`Stroke::dot`] where nothing else masks it, so
/// its own approximation error is not absorbed the way a centreline's is.
/// A tangent-intersection *90°* arc would overshoot: its midpoint sits
/// `0.75√2 ≈ 1.0607` of `radius` from the centre, a 6.07% bulge, easily
/// wide enough to flip a pixel at the small sizes this face renders at.
/// Halving the arc to 45° brings that down to `0.5·(cos 22.5° + sec 22.5°)
/// ≈ 1.0031` of `radius`: 0.31%, sub-pixel at every size this module has
/// been checked against (verified empirically against [`raster::render`]
/// by `emitted_contours_match_the_rasterised_ink`).
fn circle_contour(c: (f64, f64), radius: f64) -> Contour {
    let (cx, cy) = c;
    let r = radius;
    // Eight on-curve points at 45° steps, starting due east and walking
    // *clockwise* (angle decreasing): at angle 0 this gives on-curve
    // `(cx+r, cy)` and control `(cx+r, cy-r)`, the tangent-line
    // intersection for the arc leaving that point. Each control point is
    // its on-curve point pushed outward along the clockwise tangent by
    // `radius * tan(22.5°)`, the standard tangent-line-intersection
    // distance for a 45° arc.
    let tan = (std::f64::consts::PI / 8.0).tan();
    let mut out = Vec::with_capacity(16);
    for i in 0..8 {
        let a = -(i as f64) * std::f64::consts::FRAC_PI_4;
        let (ex, ey) = (cx + r * a.cos(), cy + r * a.sin());
        out.push((round_i16(ex), round_i16(ey), true));
        let (tx, ty) = (a.sin(), -a.cos()); // unit tangent, clockwise sense
        let (cxp, cyp) = (ex + r * tan * tx, ey + r * tan * ty);
        out.push((round_i16(cxp), round_i16(cyp), false));
    }
    out
}

/// Turn angle above which a vertex gets its own disc, to close the gap two
/// straight-sided rectangles leave outside the bend. Where two rectangles of
/// equal half-width `STROKE / 2 = 35` design units meet at turn angle
/// `theta`, the gap's depth is `(STROKE / 2) * (1 - cos(theta / 2))`. A
/// quarter-pixel at 64px/em — the smaller of the two sizes
/// `emitted_contours_match_the_rasterised_ink` checks — is `1000.0 / 64.0 /
/// 4.0 ≈ 3.9` design units, so the wedge stays under a quarter pixel while
/// `35.0 * (1.0 - (theta / 2.0).cos()) < 3.9`, i.e. `theta < 54°`. 20° sits
/// comfortably inside that margin.
///
/// [`stroke_contours_with_quad_steps`] compares each vertex's turn against this threshold
/// *cumulatively since the last disc* rather than one joint at a time. A
/// tightly curved flattened arc turns only a couple of degrees per vertex —
/// each one alone well under 20° — but a run of several such vertices with
/// no disc between them drifts the same wedge-shaped gap open as one real
/// corner would; measured directly against a brace glyph's tightly curved
/// nub, that drift was enough to flip which pixel column
/// `emitted_contours_match_the_rasterised_ink` cropped to at 32px/em before
/// this fix. Bounding the cumulative turn instead keeps that gap under the
/// same quarter-pixel bound the single-joint formula above targets, while
/// still removing nearly every disc from a smoothly (and shallowly) curved
/// run, since there resets happen only every few tens of vertices.
const CORNER_THRESHOLD_DEG: f64 = 20.0;

/// The unit direction from `a` to `b`, or `None` if they coincide — a
/// zero-length hop has no direction to test a turn against.
fn unit_dir(a: (f64, f64), b: (f64, f64)) -> Option<(f64, f64)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        None
    } else {
        Some((dx / len, dy / len))
    }
}

/// The angle in degrees between two unit direction vectors: `0` for a
/// straight continuation, up to `180` for a full reversal.
fn turn_angle_deg(d_in: (f64, f64), d_out: (f64, f64)) -> f64 {
    let cos_theta = (d_in.0 * d_out.0 + d_in.1 * d_out.1).clamp(-1.0, 1.0);
    cos_theta.acos().to_degrees()
}

/// One stroke's contours: a rectangle per flattened (non-degenerate)
/// segment, plus a disc only where the swept pen's round profile is
/// actually exposed.
///
/// A disc is always emitted at an open stroke's two endpoints — the round
/// cap, which nothing else covers. At an interior vertex a disc is emitted
/// when either of two things is true: the polyline's direction has turned,
/// *since the last disc placed*, by more than [`CORNER_THRESHOLD_DEG`] (see
/// that constant's doc for why the comparison is cumulative rather than one
/// joint at a time); or the vertex is a strict local extreme of `x` or `y`
/// among its own flattened neighbours (see [`is_axis_extremum`]), regardless
/// of turn angle. A run of vertices that is neither needs no disc, because
/// the rectangles already cover the join within the tolerance the threshold
/// constant's doc derives.
///
/// A stroke that closes on itself (flattened start == end, e.g. a
/// [`Stroke::dot`] or a closed loop such as [`super::glyphs::ring`]) has no
/// exposed cap — the pen never lifts there — so its closing vertex is tested
/// the same cumulative, extremum-checked way, once rather than twice. The
/// one exception is a `Stroke::dot` itself: flattening it leaves a single
/// point with no non-degenerate segment at all, and since nothing else puts
/// down ink there, it always gets a disc regardless of any turn.
///
/// Flattening reuses [`raster::flatten_stroke_with_quad_steps`], so the
/// vertex set walked here is exactly the one [`raster::render`]'s `is_ink`
/// test walks. `quad_steps` is a parameter, rather than this being a fixed
/// function called [`raster::flatten_stroke`] directly, only so the
/// coarsened-flattening experiment in the test module below
/// (`coarsened_flattening_fringe_ratio_report`) can call it with a coarser
/// value; every production caller, via [`glyph_outline`], passes the shipped
/// [`raster::QUAD_STEPS`], so this parameter changes nothing about the
/// emitted font.
fn stroke_contours_with_quad_steps(stroke: &Stroke, radius: f64, quad_steps: usize) -> Vec<Contour> {
    let pts = raster::flatten_stroke_with_quad_steps(stroke, quad_steps);
    let mut contours = Vec::new();
    for w in pts.windows(2) {
        if w[0] != w[1] {
            contours.push(rect_contour(w[0], w[1], radius));
        }
    }

    // Adjacent duplicates collapsed: a zero-length hop has no direction and
    // cannot take part in a turn-angle test.
    let mut verts: Vec<(f64, f64)> = Vec::with_capacity(pts.len());
    for &pt in &pts {
        if verts.last() != Some(&pt) {
            verts.push(pt);
        }
    }

    let n = verts.len();
    if n < 2 {
        contours.extend(verts.first().map(|&pt| circle_contour(pt, radius)));
        return contours;
    }

    let closed = verts[0] == verts[n - 1];
    let dir = |i: usize| unit_dir(verts[i], verts[i + 1]);

    if !closed {
        contours.push(circle_contour(verts[0], radius));
        contours.push(circle_contour(verts[n - 1], radius));
    }

    // `anchor` is the direction as of the last disc (the start cap, or the
    // last corner found). Each later vertex's outgoing direction is compared
    // against it, not against the immediately preceding vertex, so a run of
    // small turns accumulates instead of resetting every step; see
    // `CORNER_THRESHOLD_DEG`'s doc for why that matters.
    let mut anchor = dir(0);
    for i in 1..n - 1 {
        let Some(d_i) = dir(i) else { continue };
        let is_extremum = is_axis_extremum(verts[i - 1], verts[i], verts[i + 1]);
        let Some(a) = anchor else {
            anchor = Some(d_i);
            if is_extremum {
                contours.push(circle_contour(verts[i], radius));
            }
            continue;
        };
        if is_extremum || turn_angle_deg(a, d_i) > CORNER_THRESHOLD_DEG {
            contours.push(circle_contour(verts[i], radius));
            anchor = Some(d_i);
        }
    }
    if closed {
        let seam_extremum = is_axis_extremum(verts[n - 2], verts[0], verts[1]);
        if let (Some(a), Some(d0)) = (anchor, dir(0)) {
            if seam_extremum || turn_angle_deg(a, d0) > CORNER_THRESHOLD_DEG {
                contours.push(circle_contour(verts[0], radius));
            }
        }
    }

    contours
}

/// Whether `b` is a strict local extreme of `x` or `y` among its two
/// flattened neighbours (`a`, `b`, `c`): the point sits strictly beyond both
/// on one axis, e.g. the west point of a circle.
///
/// This exists because that kind of point is where a corner-skipping
/// optimisation is riskiest, not where it is most tempting. A ring's own
/// cardinal points already turn only a couple of degrees per flattened
/// vertex, so the cumulative-turn test above may go a while between discs
/// there without ever crossing the threshold — but the polygon's own
/// *extent* (its tight bounding box, which is what a crop reads) is reached
/// exactly at these points, half a pen further out than either flattened
/// neighbour. Two rectangles alone do not reach that far: they cover the
/// bend but not the disc's own outward bulge at its apex, so without a disc
/// forced there the emitted polygon is narrower, on that axis, than the
/// true swept pen — narrow enough to crop a column or row short. Measured
/// directly: disabling this criterion (leaving only the cumulative turn
/// test) reintroduces crop-size mismatches against [`raster::render`] on 10
/// glyph/size pairs in `emitted_contours_match_the_rasterised_ink`
/// (`~`, and every accented lowercase glyph whose accent bowl has an
/// unassisted west extremum, at 32px/em), while leaving the heaviest
/// glyph's contour and point counts unchanged and shrinking the emitted TTF
/// by only about 1.3%. The saving from dropping it is not worth a crop
/// defect, so it stays: at most four extra discs per closed loop (its own
/// cardinal points), none at all on a shape with no interior extremum such
/// as a straight-sided run.
fn is_axis_extremum(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    (b.0 < a.0 && b.0 < c.0)
        || (b.0 > a.0 && b.0 > c.0)
        || (b.1 < a.1 && b.1 < c.1)
        || (b.1 > a.1 && b.1 > c.1)
}

/// Every contour for one glyph. See [`stroke_contours_with_quad_steps`] for how each
/// stroke's own contours are chosen.
fn glyph_outline(glyph: &Glyph) -> Vec<Contour> {
    glyph_outline_with_quad_steps(glyph, raster::QUAD_STEPS)
}

/// As [`glyph_outline`], with the flattening resolution given explicitly.
/// See [`stroke_contours_with_quad_steps`].
fn glyph_outline_with_quad_steps(glyph: &Glyph, quad_steps: usize) -> Vec<Contour> {
    let radius = STROKE as f64 / 2.0;
    glyph.strokes.iter().flat_map(|s| stroke_contours_with_quad_steps(s, radius, quad_steps)).collect()
}

/// A big-endian byte buffer — the only encoding any SFNT table uses.
#[derive(Default)]
struct Buf(Vec<u8>);

impl Buf {
    fn new() -> Self {
        Buf(Vec::new())
    }
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }
    fn utf16be(&mut self, s: &str) {
        for u in s.encode_utf16() {
            self.u16(u);
        }
    }
}

fn table_checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
    }
    sum
}

/// `(largest power of two <= n, its log2)` — the pair every SFNT
/// binary-search header (the table directory, and `cmap` format 4) derives
/// `searchRange`/`entrySelector`/`rangeShift` from.
fn pow2_floor(n: u16) -> (u16, u16) {
    let mut p = 1u16;
    let mut k = 0u16;
    while p.checked_mul(2).is_some_and(|v| v <= n) {
        p *= 2;
        k += 1;
    }
    (p, k)
}

/// One glyph's raw (unpadded) `glyf` simple-glyph record, plus its bounding
/// box. An empty `contours` list — used for `.notdef` — yields an empty
/// record, TrueType's convention for a glyph with no outline.
fn build_simple_glyph(contours: &[Contour]) -> (Vec<u8>, (i16, i16, i16, i16)) {
    if contours.is_empty() {
        return (Vec::new(), (0, 0, 0, 0));
    }

    let mut bbox = (i16::MAX, i16::MAX, i16::MIN, i16::MIN);
    for &(x, y, _) in contours.iter().flatten() {
        bbox.0 = bbox.0.min(x);
        bbox.1 = bbox.1.min(y);
        bbox.2 = bbox.2.max(x);
        bbox.3 = bbox.3.max(y);
    }

    let mut buf = Buf::new();
    buf.i16(contours.len() as i16);
    buf.i16(bbox.0);
    buf.i16(bbox.1);
    buf.i16(bbox.2);
    buf.i16(bbox.3);
    let mut end = -1i32;
    for c in contours {
        end += c.len() as i32;
        buf.u16(end as u16);
    }
    buf.u16(0); // instructionLength: no hinting

    let mut flags = Vec::new();
    let mut dxs = Vec::new();
    let mut dys = Vec::new();
    let (mut px, mut py) = (0i32, 0i32);
    for &(x, y, on) in contours.iter().flatten() {
        flags.push(u8::from(on));
        dxs.push((x as i32 - px) as i16);
        dys.push((y as i32 - py) as i16);
        px = x as i32;
        py = y as i32;
    }
    for f in flags {
        buf.u8(f);
    }
    for d in dxs {
        buf.i16(d);
    }
    for d in dys {
        buf.i16(d);
    }
    (buf.0, bbox)
}

/// `cmap` format 4, mapping every `(codepoint, glyph index)` pair in
/// `entries` (sorted ascending by codepoint, one entry per codepoint).
/// Because glyph indices are assigned by the caller in that same
/// codepoint-sorted order, every maximal run of consecutive codepoints also
/// has consecutive glyph indices, so every segment's `idRangeOffset` is `0`
/// and `idDelta` alone reconstructs it — no `glyphIdArray` is needed.
fn build_cmap4(entries: &[(u32, u16)]) -> Vec<u8> {
    let mut segments: Vec<(u32, u32, i32)> = Vec::new();
    let mut i = 0;
    while i < entries.len() {
        let (start_cp, start_gid) = entries[i];
        let mut j = i;
        while j + 1 < entries.len() && entries[j + 1].0 == entries[j].0 + 1 {
            j += 1;
        }
        let delta = start_gid as i32 - start_cp as i32;
        segments.push((start_cp, entries[j].0, delta));
        i = j + 1;
    }
    segments.push((0xFFFF, 0xFFFF, 1));

    let seg_count = segments.len() as u16;
    let (pow2, log2) = pow2_floor(seg_count);
    let search_range = pow2 * 2;

    let mut sub = Buf::new();
    sub.u16(4); // format
    sub.u16(0); // length, patched below
    sub.u16(0); // language
    sub.u16(seg_count * 2);
    sub.u16(search_range);
    sub.u16(log2);
    sub.u16(seg_count * 2 - search_range);
    for &(_, end, _) in &segments {
        sub.u16(end as u16);
    }
    sub.u16(0); // reservedPad
    for &(start, _, _) in &segments {
        sub.u16(start as u16);
    }
    for &(_, _, delta) in &segments {
        sub.i16(delta as i16);
    }
    for _ in &segments {
        sub.u16(0); // idRangeOffset
    }

    let len = sub.0.len() as u16;
    sub.0[2..4].copy_from_slice(&len.to_be_bytes());

    let mut cmap = Buf::new();
    cmap.u16(0); // version
    cmap.u16(1); // numTables
    cmap.u16(3); // platformID: Windows
    cmap.u16(1); // encodingID: Unicode BMP
    cmap.u32(12); // offset to the subtable, right after this header
    cmap.bytes(&sub.0);
    cmap.0
}

fn build_name() -> Vec<u8> {
    let records: [(u16, &str); 7] = [
        (1, "OCRcer Technical"),
        (2, "Regular"),
        (3, "OCRcer Technical Regular 1.0"),
        (4, "OCRcer Technical"),
        (5, "Version 1.0"),
        (6, "OCRcerTechnical-Regular"),
        (13, "MIT License. Copyright (c) 2026 Ken Mantle."),
    ];
    let count = records.len() as u16;
    let header_len = 6 + 12 * count as usize;

    let mut storage = Buf::new();
    let mut dir = Buf::new();
    for &(name_id, text) in &records {
        let start = storage.0.len() as u16;
        storage.utf16be(text);
        let len = storage.0.len() as u16 - start;
        dir.u16(3); // platformID: Windows
        dir.u16(1); // encodingID: Unicode BMP
        dir.u16(0x0409); // languageID: en-US
        dir.u16(name_id);
        dir.u16(len);
        dir.u16(start);
    }

    let mut out = Buf::new();
    out.u16(0); // format
    out.u16(count);
    out.u16(header_len as u16);
    out.bytes(&dir.0);
    out.bytes(&storage.0);
    out.0
}

fn build_post() -> Vec<u8> {
    let mut buf = Buf::new();
    buf.u32(0x0003_0000); // version 3.0: no glyph names table
    buf.i32(0); // italicAngle: upright
    buf.i16(-100); // underlinePosition
    buf.i16(STROKE); // underlineThickness: one pen width
    buf.u32(0); // isFixedPitch: advances vary per glyph
    buf.u32(0);
    buf.u32(0);
    buf.u32(0);
    buf.u32(0);
    buf.0
}

fn build_head(bbox: (i16, i16, i16, i16), long_loca: bool) -> Vec<u8> {
    let mut buf = Buf::new();
    buf.u16(1); // version major
    buf.u16(0); // version minor
    buf.i32(0x0001_0000); // fontRevision 1.0
    buf.u32(0); // checkSumAdjustment, patched by `assemble`
    buf.u32(0x5F0F_3CF5); // magicNumber
    buf.u16(0x0003); // flags: baseline at y=0, lsb at x=0
    buf.u16(UPM as u16);
    buf.i64(0); // created: fixed at zero, so a rebuild is byte-identical
    buf.i64(0); // modified: same
    buf.i16(bbox.0);
    buf.i16(bbox.1);
    buf.i16(bbox.2);
    buf.i16(bbox.3);
    buf.u16(0); // macStyle
    buf.u16(8); // lowestRecPPEM
    buf.i16(2); // fontDirectionHint: deprecated, 2 = "fully mixed"
    buf.i16(i16::from(long_loca));
    buf.i16(0); // glyphDataFormat
    buf.0
}

fn build_hhea(advances: &[u16], lsbs: &[i16], bboxes: &[(i16, i16, i16, i16)]) -> Vec<u8> {
    let advance_max = *advances.iter().max().unwrap();
    let (mut min_lsb, mut min_rsb, mut x_max_extent) = (i16::MAX, i32::MAX, i32::MIN);
    for ((&adv, &lsb), bbox) in advances.iter().zip(lsbs).zip(bboxes) {
        let width = (bbox.2 - bbox.0) as i32;
        min_lsb = min_lsb.min(lsb);
        min_rsb = min_rsb.min(adv as i32 - lsb as i32 - width);
        x_max_extent = x_max_extent.max(lsb as i32 + width);
    }
    let ymin_all = bboxes.iter().map(|b| b.1).min().unwrap();
    let ymax_all = bboxes.iter().map(|b| b.3).max().unwrap();

    let mut buf = Buf::new();
    buf.u16(1); // version major
    buf.u16(0); // version minor
    buf.i16(ymax_all); // ascender
    buf.i16(ymin_all); // descender
    buf.i16(0); // lineGap
    buf.u16(advance_max);
    buf.i16(min_lsb);
    buf.i16(min_rsb as i16);
    buf.i16(x_max_extent as i16);
    buf.i16(1); // caretSlopeRise: upright
    buf.i16(0); // caretSlopeRun
    buf.i16(0); // caretOffset
    buf.i16(0);
    buf.i16(0);
    buf.i16(0);
    buf.i16(0); // reserved x4
    buf.i16(0); // metricDataFormat
    buf.u16(advances.len() as u16); // numberOfHMetrics: one entry per glyph
    buf.0
}

fn build_hmtx(advances: &[u16], lsbs: &[i16]) -> Vec<u8> {
    let mut buf = Buf::new();
    for (&a, &l) in advances.iter().zip(lsbs) {
        buf.u16(a);
        buf.i16(l);
    }
    buf.0
}

fn build_maxp(num_glyphs: u16, max_points: u16, max_contours: u16) -> Vec<u8> {
    let mut buf = Buf::new();
    buf.u32(0x0001_0000); // version 1.0: required for TrueType outlines
    buf.u16(num_glyphs);
    buf.u16(max_points);
    buf.u16(max_contours);
    buf.u16(0); // maxCompositePoints: no composite glyphs
    buf.u16(0); // maxCompositeContours
    buf.u16(1); // maxZones: no twilight-zone instructions used
    buf.u16(0); // maxTwilightPoints
    buf.u16(0); // maxStorage
    buf.u16(0); // maxFunctionDefs
    buf.u16(0); // maxInstructionDefs
    buf.u16(0); // maxStackElements
    buf.u16(0); // maxSizeOfInstructions: no hinting
    buf.u16(0); // maxComponentElements
    buf.u16(0); // maxComponentDepth
    buf.0
}

fn build_os2(codepoints: &[u32], advances: &[u16], bbox: (i16, i16, i16, i16)) -> Vec<u8> {
    let (_, ymin, _, ymax) = bbox;
    let avg_advance =
        (advances.iter().map(|&a| a as u32).sum::<u32>() / advances.len() as u32) as i16;
    let min_cp = *codepoints.iter().min().unwrap();
    let max_cp = *codepoints.iter().max().unwrap();

    let mut buf = Buf::new();
    buf.u16(0); // version 0
    buf.i16(avg_advance);
    buf.u16(400); // usWeightClass: Regular
    buf.u16(5); // usWidthClass: Medium
    buf.u16(0); // fsType: no embedding restrictions
    buf.i16(UPM / 5); // ySubscriptXSize
    buf.i16(UPM / 10); // ySubscriptYSize
    buf.i16(0); // ySubscriptXOffset
    buf.i16(0); // ySubscriptYOffset
    buf.i16(UPM / 5); // ySuperscriptXSize
    buf.i16(UPM / 10); // ySuperscriptYSize
    buf.i16(0); // ySuperscriptXOffset
    buf.i16(X_HEIGHT); // ySuperscriptYOffset
    buf.i16(STROKE); // yStrikeoutSize: one pen width
    buf.i16(X_HEIGHT / 2); // yStrikeoutPosition
    buf.i16(0); // sFamilyClass: unspecified
    buf.bytes(&[0u8; 10]); // panose: unspecified
    // ulUnicodeRange1: bit0 Basic Latin, bit1 Latin-1 Supplement. See the
    // module doc — the remaining blocks the charset touches are left unset
    // rather than guessed.
    buf.u32(0b11);
    buf.u32(0);
    buf.u32(0);
    buf.u32(0);
    buf.bytes(b"OCRc"); // achVendID: not a registered vendor ID, see module doc
    buf.u16(0x0040); // fsSelection: REGULAR
    buf.u16(min_cp as u16);
    buf.u16(max_cp.min(0xFFFF) as u16);
    buf.i16(CAP); // sTypoAscender
    buf.i16(DESCENDER); // sTypoDescender
    buf.i16(0); // sTypoLineGap
    buf.u16(ymax.max(0) as u16); // usWinAscent
    buf.u16((-ymin).max(0) as u16); // usWinDescent
    buf.0
}

/// Lays out the SFNT offset table, table directory and table data (tables
/// must already be given in ascending tag order, per spec), pads every
/// table to a 4-byte boundary, and patches `head.checkSumAdjustment`: the
/// whole file's checksum with that field zeroed, subtracted from the magic
/// constant `0xB1B0AFBA`.
fn assemble(tables: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let num_tables = tables.len() as u16;
    let (pow2, log2) = pow2_floor(num_tables);
    let search_range = pow2 * 16;

    let padded: Vec<Vec<u8>> = tables
        .iter()
        .map(|(_, data)| {
            let mut d = data.clone();
            while d.len() % 4 != 0 {
                d.push(0);
            }
            d
        })
        .collect();
    let checksums: Vec<u32> = padded.iter().map(|d| table_checksum(d)).collect();

    let data_start = 12u32 + 16 * u32::from(num_tables);
    let mut offsets = Vec::with_capacity(tables.len());
    let mut cursor = data_start;
    for d in &padded {
        offsets.push(cursor);
        cursor += d.len() as u32;
    }

    let mut out = Buf::new();
    out.u32(0x0001_0000); // sfnt version: TrueType outlines
    out.u16(num_tables);
    out.u16(search_range);
    out.u16(log2);
    out.u16(num_tables * 16 - search_range);
    let mut head_offset = 0u32;
    for (i, (tag, data)) in tables.iter().enumerate() {
        if tag == b"head" {
            head_offset = offsets[i];
        }
        out.bytes(tag);
        out.u32(checksums[i]);
        out.u32(offsets[i]);
        out.u32(data.len() as u32); // `length` is the unpadded size
    }
    for d in &padded {
        out.bytes(d);
    }

    let file_checksum = table_checksum(&out.0);
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(file_checksum);
    let patch_at = head_offset as usize + 8;
    out.0[patch_at..patch_at + 4].copy_from_slice(&adjustment.to_be_bytes());
    out.0
}

/// Builds a complete, self-contained TrueType font for the face. See the
/// module doc for the contract.
pub fn build_ttf() -> Vec<u8> {
    let mut authored = super::glyphs::glyphs();
    authored.sort_by_key(|g| g.codepoint);

    let mut glyf_records: Vec<Vec<u8>> = vec![Vec::new()]; // glyph 0: .notdef
    let mut bboxes: Vec<(i16, i16, i16, i16)> = vec![(0, 0, 0, 0)];
    let mut advances: Vec<u16> = vec![(UPM / 2) as u16];
    let mut lsbs: Vec<i16> = vec![0];
    let mut max_points = 0usize;
    let mut max_contours = 0usize;

    for g in &authored {
        let contours = glyph_outline(g);
        max_points = max_points.max(contours.iter().map(Vec::len).sum());
        max_contours = max_contours.max(contours.len());
        let (record, bbox) = build_simple_glyph(&contours);
        advances.push(g.advance as u16);
        lsbs.push(bbox.0);
        bboxes.push(bbox);
        glyf_records.push(record);
    }

    let num_glyphs = glyf_records.len();
    let mut glyf = Buf::new();
    let mut loca_offsets: Vec<u32> = Vec::with_capacity(num_glyphs + 1);
    for record in &glyf_records {
        loca_offsets.push(glyf.0.len() as u32);
        glyf.bytes(record);
        if !glyf.0.len().is_multiple_of(2) {
            glyf.u8(0); // keep every glyph record's length even, for short loca
        }
    }
    loca_offsets.push(glyf.0.len() as u32);

    let long_loca = *loca_offsets.last().unwrap() > u32::from(u16::MAX) * 2;
    let mut loca = Buf::new();
    for &o in &loca_offsets {
        if long_loca {
            loca.u32(o);
        } else {
            loca.u16((o / 2) as u16);
        }
    }

    let real_bboxes = &bboxes[1..];
    let font_bbox = (
        real_bboxes.iter().map(|b| b.0).min().unwrap(),
        real_bboxes.iter().map(|b| b.1).min().unwrap(),
        real_bboxes.iter().map(|b| b.2).max().unwrap(),
        real_bboxes.iter().map(|b| b.3).max().unwrap(),
    );

    let codepoints: Vec<u32> = authored.iter().map(|g| g.codepoint).collect();
    let cmap_entries: Vec<(u32, u16)> =
        codepoints.iter().enumerate().map(|(i, &cp)| (cp, (i + 1) as u16)).collect();

    assemble(&[
        (*b"OS/2", build_os2(&codepoints, &advances, font_bbox)),
        (*b"cmap", build_cmap4(&cmap_entries)),
        (*b"glyf", glyf.0),
        (*b"head", build_head(font_bbox, long_loca)),
        (*b"hhea", build_hhea(&advances, &lsbs, &bboxes)),
        (*b"hmtx", build_hmtx(&advances, &lsbs)),
        (*b"loca", loca.0),
        (*b"maxp", build_maxp(num_glyphs as u16, max_points as u16, max_contours as u16)),
        (*b"name", build_name()),
        (*b"post", build_post()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::glyphs::glyphs;
    use super::super::Seg;
    use ocrcer_core::image::components::{label, Connectivity};

    /// Sizes gate 3 compares at. 16 and 96 were added alongside 32/64: small
    /// sizes are where a structural defect (a missing stroke, a sealed
    /// aperture) is most likely to disappear into the pen's own footprint,
    /// and large sizes are where the emitted polygon's boundary-approximation
    /// error has the most perimeter to accumulate along. Both ends are worth
    /// checking, not just the middle.
    const TEST_SIZES: [f32; 4] = [16.0, 32.0, 64.0, 96.0];

    /// Fraction of a reference raster's own boundary-pixel count that the
    /// emitted outline's *fringe* disagreement (see [`is_fringe_pixel`]) may
    /// occupy, per glyph/size pair.
    ///
    /// Measured by `fringe_boundary_ratio_report` across every glyph at
    /// 16/32/64/96px/em (`cargo test -p ocrcer-build --all-targets
    /// fringe_boundary_ratio_report -- --ignored --nocapture`, 748
    /// glyph/size pairs, both renderers compared on [`raster::glyph_grid`]'s
    /// shared, uncropped grid via [`raster::render_raw`] and
    /// [`rasterize_outline_raw`]): max ratio 0.1538 (U+2022 '•' BULLET @
    /// 64px, 8 fringe px / 52 boundary px), 99th percentile 0.0909, median
    /// 0.0000. The two extra pairs relative to the previous, crop-based
    /// measurement (746) are U+201C/U+201D at 16px, which the old tight-crop
    /// comparison excluded outright as a crop-size mismatch — see
    /// `crop_before_compare_manufactures_a_zero_tolerance_equality` in the
    /// fonts RAG — rather than measuring; the max is unchanged and the p99
    /// moved only slightly (0.0833 → 0.0909), so removing the crop step did
    /// not itself inflate this distribution. The worst pairs are small marks
    /// and round/curved glyphs at small sizes, where the reference raster's
    /// own boundary is short enough that a handful of single-pixel rounding
    /// differences are a large fraction of it — exactly the population this
    /// ratio metric exists to size against, rather than against a flat pixel
    /// count. Set at roughly 1.7x the observed max (0.1538 * 1.7 ≈ 0.26) so
    /// an outline that is merely as approximate as today's passes, while a
    /// fringe blowout — the signature of an actually missing or malformed
    /// stroke — still fails it. That it does is demonstrated by
    /// [`deliberate_breakage_demo_missing_crossbar_is_caught`]; a whole mark
    /// dropped outright, which this ratio cannot see at all, is demonstrated
    /// separately by [`deliberate_breakage_demo_missing_mark_is_caught`] and
    /// caught by [`lost_components`] instead.
    const FRINGE_RATIO_MAX: f64 = 0.26;

    fn char_name(cp: u32) -> String {
        match char::from_u32(cp) {
            Some(c) => format!("U+{cp:04X} '{c}'"),
            None => format!("U+{cp:04X}"),
        }
    }

    /// The contour flattening and non-zero-winding fill this gate tests the
    /// emitted outline with are [`crate::outline`]'s, not a second copy
    /// living in this test module: the point of the gate is that the shape
    /// the emitter writes reads correctly to the code that reads *real*
    /// font outlines, so it has to be that code doing the reading.
    use crate::outline::{self, winding_number};

    /// Independently rasterises the emitted contours for `glyph` at
    /// `px_per_em`, on [`raster::glyph_grid`]'s shared, uncropped grid via
    /// [`raster::rasterize_raw`] — the identical grid, flattening, pixel
    /// sampling policy and best-pixel fallback as [`raster::render_raw`] —
    /// so a disagreement can only come from the fill rule itself: winding
    /// number against the emitted contours, vs. pen-radius distance to the
    /// flattened centreline. Returns `(ink, cols, rows, py_top)`, guaranteed
    /// the same `cols`/`rows` as `raster::render_raw`'s for the same glyph
    /// and size, by construction — there is no crop here for the two sides
    /// to disagree about the extent of.
    fn rasterize_outline_raw(glyph: &Glyph, px_per_em: f32) -> Option<(Vec<u8>, usize, usize, f64)> {
        rasterize_outline_raw_with_quad_steps(glyph, px_per_em, raster::QUAD_STEPS)
    }

    /// As [`rasterize_outline_raw`], but the emitter's own curve-flattening
    /// resolution (see [`glyph_outline_with_quad_steps`]) is given
    /// explicitly rather than fixed at [`raster::QUAD_STEPS`]. Coarsening
    /// this argument degrades only how finely the emitter turns a curved
    /// *stroke* into rectangle contours — the re-flattening of the
    /// resulting contour's own points a few lines below, via
    /// [`crate::outline::flatten_contours`], stays at the shipped resolution, since that step just resamples whatever contour
    /// the emitter produced for this function's own winding-number test,
    /// the same way a real third-party rasteriser would resample it — it is
    /// not part of what this experiment is degrading.
    fn rasterize_outline_raw_with_quad_steps(
        glyph: &Glyph,
        px_per_em: f32,
        quad_steps: usize,
    ) -> Option<(Vec<u8>, usize, usize, f64)> {
        let scale = f64::from(px_per_em) / f64::from(UPM);
        let outline = glyph_outline_with_quad_steps(glyph, quad_steps);
        if outline.is_empty() {
            return None;
        }

        let edges = outline::edges(&outline::flatten_contours(&outline), scale);
        if edges.is_empty() {
            return None;
        }

        // Coverage dropped here: this gate compares two fill rules'
        // *decisions* on a shared grid, and grey would only blur that.
        raster::rasterize_raw(glyph, px_per_em, |x, y| winding_number((x, y), &edges) != 0)
            .map(|(ink, _cov, cols, rows, py_top)| (ink, cols, rows, py_top))
    }

    /// Whether the emitted outline alone — no best-pixel fallback — puts
    /// down at least one ink pixel for `glyph` at `px_per_em`: what a
    /// third-party rasteriser with no fallback of its own (FreeType, a PDF
    /// viewer) would see. Reimplements [`raster::rasterize`]'s loop without
    /// its call to [`raster::apply_best_pixel_fallback`], sharing everything
    /// else — [`raster::glyph_grid`], [`raster::subgrid_hits`],
    /// [`raster::pixel_is_ink`] — so the only thing this measures is the
    /// fallback's own effect.
    #[cfg(test)]
    fn outline_has_ink_without_fallback(glyph: &Glyph, px_per_em: f32) -> bool {
        let scale = f64::from(px_per_em) / f64::from(UPM);
        let outline = glyph_outline(glyph);
        if outline.is_empty() {
            return false;
        }
        let edges = outline::edges(&outline::flatten_contours(&outline), scale);
        if edges.is_empty() {
            return false;
        }
        let Some(grid) = raster::glyph_grid(glyph, px_per_em) else {
            return false;
        };
        for r in 0..grid.rows {
            let cy = grid.py_top - r as f64 - 0.5;
            for c in 0..grid.cols {
                let cx = grid.col0 + c as f64 + 0.5;
                let hits = raster::subgrid_hits(cx, cy, |x, y| winding_number((x, y), &edges) != 0);
                if raster::pixel_is_ink(hits) {
                    return true;
                }
            }
        }
        false
    }

    /// Report only: the smallest whole `px_per_em`, up to 200, at which each
    /// of a set of fine marks first produces ink from the emitted outline
    /// alone (see [`outline_has_ink_without_fallback`]) — the minimum size
    /// at which the emitted TTF is legible to a rasteriser with no
    /// best-pixel fallback of its own. `cargo test -p ocrcer-build --
    /// --ignored --nocapture minimum_legible_size_without_fallback_report`
    #[test]
    #[ignore = "report, not a gate"]
    fn minimum_legible_size_without_fallback_report() {
        for &cp in &['.' as u32, ':' as u32, 0x00B7, 0x2026, 0x2022] {
            let glyph = glyphs().into_iter().find(|g| g.codepoint == cp).unwrap();
            let found = (1..=200u32).find(|&px| outline_has_ink_without_fallback(&glyph, px as f32));
            match found {
                Some(px) => println!("{}: first ink from the outline alone at {px}px/em", char_name(cp)),
                None => println!("{}: no ink from the outline alone up to 200px/em", char_name(cp)),
            }
        }
    }

    /// `ink`'s value at `(r, c)` on a `cols x rows` grid, treating anything
    /// outside the grid as non-ink. That one convention is what lets
    /// [`is_boundary_pixel`]'s "or to the grid edge" clause fall out of the
    /// ordinary 8-neighbour test rather than needing its own special case.
    fn ink_at(ink: &[u8], cols: usize, rows: usize, r: i64, c: i64) -> bool {
        if r < 0 || c < 0 || r >= rows as i64 || c >= cols as i64 {
            return false;
        }
        ink[r as usize * cols + c as usize] != 0
    }

    /// The eight neighbours of `(r, c)`, as `(row, col)` pairs that may be
    /// negative or past the raster's edge — [`ink_at`] treats those as
    /// non-ink rather than this function filtering them out.
    fn neighbors8(r: usize, c: usize) -> [(i64, i64); 8] {
        let (r, c) = (r as i64, c as i64);
        [
            (r - 1, c - 1), (r - 1, c), (r - 1, c + 1),
            (r, c - 1), (r, c + 1),
            (r + 1, c - 1), (r + 1, c), (r + 1, c + 1),
        ]
    }

    /// Whether `(r, c)` is an ink pixel of `want` that is 8-adjacent to at
    /// least one non-ink pixel *or to the grid edge* — the reference
    /// raster's own boundary, independent of any comparison to `got`. Used
    /// as the denominator [`FRINGE_RATIO_MAX`] bounds a glyph/size pair's
    /// fringe disagreement against, so that pair's tolerance scales with how
    /// much boundary the shape actually has instead of being a flat count.
    fn is_boundary_pixel(want: &[u8], cols: usize, rows: usize, r: usize, c: usize) -> bool {
        ink_at(want, cols, rows, r as i64, c as i64)
            && neighbors8(r, c).iter().any(|&(rr, cc)| !ink_at(want, cols, rows, rr, cc))
    }

    /// Whether `(r, c)` sits where the reference raster's own shape has both
    /// ink and non-ink among its 8 neighbours — i.e. squarely on `want`'s own
    /// boundary, regardless of which value it holds itself. A *differing*
    /// pixel there is what this module calls **fringe**: the two renderers
    /// disagreeing about which side of an already-thin boundary a pixel
    /// falls on. A differing pixel that fails this test is **interior** —
    /// its neighbourhood in the reference is uniformly ink or uniformly
    /// background, so the disagreement is not boundary noise, it is a real
    /// shape defect (a blob, a hole punched mid-stroke, a missing stroke).
    fn is_fringe_pixel(want: &[u8], cols: usize, rows: usize, r: usize, c: usize) -> bool {
        let (mut has_ink, mut has_non_ink) = (false, false);
        for &(rr, cc) in &neighbors8(r, c) {
            if ink_at(want, cols, rows, rr, cc) {
                has_ink = true;
            } else {
                has_non_ink = true;
            }
        }
        has_ink && has_non_ink
    }

    /// Count of `want`'s own 8-connected ink components (on the shared,
    /// uncropped grid) with no ink pixel anywhere in `got` at the same
    /// position: a mark the design puts down that the emitted outline drops
    /// *entirely*.
    ///
    /// [`FRINGE_RATIO_MAX`] cannot guarantee this on its own: a small mark
    /// lost outright has every one of its own pixels adjacent to
    /// background in `want`, so it classifies as pure fringe, and when it
    /// is a small fraction of the glyph's total boundary the ratio never
    /// trips (see `crop_before_compare_manufactures_a_zero_tolerance_equality`
    /// in the fonts RAG). This check asks the question the ratio cannot:
    /// did *each* mark survive at all, not how large the loss is relative
    /// to everything else in the glyph. Reuses
    /// [`ocrcer_core::image::components::label`] rather than a second
    /// labeller, per `CLAUDE.md` rule 4.
    fn lost_components(want: &[u8], got: &[u8], cols: usize, rows: usize) -> usize {
        let (labels, count) = label(want, cols as u32, rows as u32, Connectivity::Eight);
        if count == 0 {
            return 0;
        }
        let mut survives = vec![false; count as usize + 1];
        for (i, &l) in labels.iter().enumerate() {
            if l != 0 && got[i] != 0 {
                survives[l as usize] = true;
            }
        }
        (1..=count as usize).filter(|&l| !survives[l]).count()
    }

    /// One glyph/size pair's comparison on the shared, uncropped grid both
    /// [`raster::render_raw`] and [`rasterize_outline_raw`] build from
    /// [`raster::glyph_grid`] — so `want` and `got` are always equal in
    /// size by construction, and there is nothing left for this function
    /// to fail on that basis.
    struct PairDiff {
        /// See [`lost_components`].
        lost_components: usize,
        interior: usize,
        fringe: usize,
        boundary: usize,
    }

    fn compare_grids(want: &[u8], got: &[u8], cols: usize, rows: usize) -> PairDiff {
        let mut pd = PairDiff { lost_components: lost_components(want, got, cols, rows), interior: 0, fringe: 0, boundary: 0 };
        for r in 0..rows {
            for c in 0..cols {
                if is_boundary_pixel(want, cols, rows, r, c) {
                    pd.boundary += 1;
                }
                if want[r * cols + c] != got[r * cols + c] {
                    if is_fringe_pixel(want, cols, rows, r, c) {
                        pd.fringe += 1;
                    } else {
                        pd.interior += 1;
                    }
                }
            }
        }
        pd
    }

    /// Report only, not a gate: prints every glyph/size pair's
    /// fringe-to-boundary ratio and the distribution's max/p99/median, the
    /// measurement [`FRINGE_RATIO_MAX`]'s doc comment cites.
    /// `cargo test -p ocrcer-build -- --ignored --nocapture
    /// fringe_boundary_ratio_report`
    #[test]
    #[ignore = "report, not a gate"]
    fn fringe_boundary_ratio_report() {
        let mut ratios: Vec<(String, f32, usize, usize, f64)> = Vec::new();
        let mut total_lost = 0usize;
        for glyph in glyphs() {
            for &size in &TEST_SIZES {
                let (Some((want, _, cols, rows, _)), Some((got, _, _, _))) =
                    (raster::render_raw(&glyph, size), rasterize_outline_raw(&glyph, size))
                else {
                    continue;
                };
                let pd = compare_grids(&want, &got, cols, rows);
                total_lost += pd.lost_components;
                let ratio = if pd.boundary == 0 { 0.0 } else { pd.fringe as f64 / pd.boundary as f64 };
                ratios.push((char_name(glyph.codepoint), size, pd.fringe, pd.boundary, ratio));
            }
        }
        ratios.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap());
        println!("\n{} glyph/size pairs measured, worst 20 by fringe/boundary ratio:", ratios.len());
        for (name, size, fringe, boundary, ratio) in ratios.iter().take(20) {
            println!("  {name} @ {size}px: {fringe}/{boundary} = {ratio:.4}");
        }
        let mut sorted_ratios: Vec<f64> = ratios.iter().map(|r| r.4).collect();
        sorted_ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = sorted_ratios.len();
        let median = sorted_ratios[n / 2];
        let p99 = sorted_ratios[(n as f64 * 0.99) as usize];
        let max = sorted_ratios[n - 1];
        println!("max {max:.4}  p99 {p99:.4}  median {median:.4}  n {n}  lost_components {total_lost}");
    }

    /// Report only, not a gate: coarsens the emitter's own curve-flattening
    /// resolution (see [`glyph_outline_with_quad_steps`]) below the shipped
    /// [`raster::QUAD_STEPS`] and, at each step count, sweeps every
    /// glyph/[`TEST_SIZES`] pair to answer the question
    /// [`FRINGE_RATIO_MAX`]'s own doc comment leaves open: can the fringe
    /// ratio ever be the leg that rejects a build, i.e. is there an
    /// achievable degradation that pushes it past the bound while
    /// [`PairDiff::lost_components`] and `interior` both stay at zero — or
    /// does every degradation big enough to move the ratio trip one of
    /// those two first, making the fringe leg unable to bind in practice.
    ///
    /// For each step count this prints two things: the worst pair *among
    /// those with zero lost components and zero interior pixels* (the
    /// clean-degradation ceiling the fringe ratio alone would have to
    /// clear), and the worst pair overall (which may already have nonzero
    /// interior/lost_components, i.e. a defect the other two legs already
    /// catch). `cargo test -p ocrcer-build -- --ignored --nocapture
    /// coarsened_flattening_fringe_ratio_report`
    #[test]
    #[ignore = "report, not a gate"]
    fn coarsened_flattening_fringe_ratio_report() {
        for &quad_steps in &[16usize, 12, 10, 8, 7, 6, 5, 4, 3, 2, 1] {
            let mut clean_worst: Option<(String, f32, usize, usize, f64)> = None;
            let mut overall_worst: Option<(String, f32, usize, usize, usize, usize, f64)> = None;
            let mut pairs_with_interior = 0usize;
            let mut pairs_with_lost = 0usize;
            let mut n = 0usize;
            for glyph in glyphs() {
                for &size in &TEST_SIZES {
                    let (Some((want, _, cols, rows, _)), Some((got, _, _, _))) = (
                        raster::render_raw(&glyph, size),
                        rasterize_outline_raw_with_quad_steps(&glyph, size, quad_steps),
                    ) else {
                        continue;
                    };
                    let pd = compare_grids(&want, &got, cols, rows);
                    n += 1;
                    if pd.interior > 0 {
                        pairs_with_interior += 1;
                    }
                    if pd.lost_components > 0 {
                        pairs_with_lost += 1;
                    }
                    let ratio = if pd.boundary == 0 { 0.0 } else { pd.fringe as f64 / pd.boundary as f64 };
                    if pd.interior == 0 && pd.lost_components == 0 {
                        let better = clean_worst.as_ref().is_none_or(|w| ratio > w.4);
                        if better {
                            clean_worst = Some((char_name(glyph.codepoint), size, pd.fringe, pd.boundary, ratio));
                        }
                    }
                    let is_worse = overall_worst.as_ref().is_none_or(|w| ratio > w.6);
                    if is_worse {
                        overall_worst = Some((
                            char_name(glyph.codepoint),
                            size,
                            pd.lost_components,
                            pd.interior,
                            pd.fringe,
                            pd.boundary,
                            ratio,
                        ));
                    }
                }
            }
            print!("quad_steps={quad_steps} (n={n}, pairs_with_interior={pairs_with_interior}, pairs_with_lost={pairs_with_lost}): ");
            match clean_worst {
                Some((name, size, fringe, boundary, ratio)) => print!(
                    "clean worst (interior=0 lost=0) {name} @ {size}px fringe={fringe} boundary={boundary} ratio={ratio:.4} (limit {FRINGE_RATIO_MAX})"
                ),
                None => print!("no pair has both interior=0 and lost_components=0"),
            }
            if let Some((name, size, lc, interior, fringe, boundary, ratio)) = overall_worst {
                println!(
                    " | overall worst {name} @ {size}px lost_components={lc} interior={interior} fringe={fringe} boundary={boundary} ratio={ratio:.4}"
                );
            } else {
                println!();
            }
        }
    }

    /// Temporary demonstration that a missing stroke is caught.
    ///
    /// `H`'s crossbar (`Stroke::line(&[p(0, 350), p(470, 350)])`, see
    /// `glyphs/upper.rs`) is chosen deliberately: its x-extent exactly
    /// matches the two stems either side of it, so dropping it from the
    /// *outline* side only — simulating `emit_ttf` silently failing to emit
    /// one stroke's contours, without touching the shared glyph-authoring
    /// files this session must not edit — does not itself change
    /// [`raster::glyph_grid`]'s span (the stems alone already reach it), so
    /// `want` and `got` still land on the same grid. It stays attached to
    /// both stems in the reference, so it is one component with them, not
    /// its own: [`lost_components`] measures `0` here, same as a correct
    /// emitter, which is expected — that check exists for a mark dropped
    /// *whole*, not a partial defect in an otherwise-present shape (see
    /// [`deliberate_breakage_demo_missing_mark_is_caught`] for the case it
    /// does catch). MEASURED at 64px: `interior=52 fringe=52 boundary=256
    /// ratio=0.2031` — the bar is thick enough at this size that its own
    /// cross-section has pixels 8-adjacent to ink on every side in the
    /// reference, so removing it trips the zero-tolerance interior check
    /// directly, not only the fringe ratio.
    #[test]
    #[ignore = "one-off demonstration, not a gate"]
    fn deliberate_breakage_demo_missing_crossbar_is_caught() {
        let h = glyphs().into_iter().find(|g| g.codepoint == 'H' as u32).unwrap();
        let mut broken = h.clone();
        broken.strokes.retain(|s| s.start.y != 350 || s.segs != vec![Seg::Line(super::super::p(470, 350))]);
        assert_eq!(broken.strokes.len(), 2, "expected exactly the crossbar removed, two stems left");
        let (want, _, cols, rows, _) = raster::render_raw(&h, 64.0).unwrap();
        let (got, got_cols, got_rows, _) = rasterize_outline_raw(&broken, 64.0).unwrap();
        assert_eq!((cols, rows), (got_cols, got_rows), "shared-grid comparisons are equal in size by construction");
        let pd = compare_grids(&want, &got, cols, rows);
        let ratio = if pd.boundary == 0 { 0.0 } else { pd.fringe as f64 / pd.boundary as f64 };
        println!(
            "H missing crossbar @ 64px: lost_components={} interior={} fringe={} boundary={} ratio={ratio:.4} (limit {FRINGE_RATIO_MAX})",
            pd.lost_components, pd.interior, pd.fringe, pd.boundary
        );
        assert!(
            pd.lost_components > 0 || pd.interior > 0 || ratio > FRINGE_RATIO_MAX,
            "a missing crossbar must be caught by the component check, the interior-pixel check, or the fringe ratio, but got lost_components={} interior={} ratio={ratio:.4}",
            pd.lost_components, pd.interior
        );
    }

    /// Temporary demonstration that [`lost_components`] actually bites, for
    /// the specific failure mode neither [`FRINGE_RATIO_MAX`] nor the
    /// interior-pixel check can see: a whole separate mark dropped from the
    /// emitted side, rather than a stroke thinned or a boundary nudged.
    ///
    /// Uses a synthetic glyph built here rather than one of the authored
    /// marks in `glyphs/` (this session must not edit that directory): an
    /// L-shaped pair of long strokes plus an isolated dot well inside the
    /// L's own bounding box. The L alone already reaches every edge of the
    /// bbox the dot would otherwise extend, so dropping the dot from the
    /// *emitted* side only — as [`deliberate_breakage_demo_missing_crossbar_is_caught`]
    /// drops the crossbar — does not change [`raster::glyph_grid`]'s span,
    /// which both `want` and `got` still compute independently from
    /// whichever glyph they are given. The dot has no other ink pixel
    /// within 8-connectivity of it, so it is a component of its own: this
    /// is precisely the shape the fringe ratio is blind to, since a small
    /// wholly-missing mark is, pixel for pixel, all fringe.
    #[test]
    #[ignore = "one-off demonstration, not a gate"]
    fn deliberate_breakage_demo_missing_mark_is_caught() {
        let dot_at = super::super::p(200, 200);
        let full = Glyph {
            codepoint: 0,
            advance: 500,
            strokes: vec![
                Stroke::line(&[super::super::p(0, 0), super::super::p(400, 0)]),
                Stroke::line(&[super::super::p(0, 0), super::super::p(0, 400)]),
                Stroke::dot(dot_at),
            ],
        };
        let mut broken = full.clone();
        broken.strokes.retain(|s| s.start != dot_at);
        assert_eq!(broken.strokes.len(), 2, "expected exactly the dot removed, the two L-shape strokes left");

        let (want, _, cols, rows, _) = raster::render_raw(&full, 64.0).unwrap();
        let (got, got_cols, got_rows, _) = rasterize_outline_raw(&broken, 64.0).unwrap();
        assert_eq!((cols, rows), (got_cols, got_rows), "removing the dot must not change the L-shape's own grid span");
        let pd = compare_grids(&want, &got, cols, rows);
        let ratio = if pd.boundary == 0 { 0.0 } else { pd.fringe as f64 / pd.boundary as f64 };
        println!(
            "L-shape missing dot @ 64px: lost_components={} interior={} fringe={} boundary={} ratio={ratio:.4} (limit {FRINGE_RATIO_MAX})",
            pd.lost_components, pd.interior, pd.fringe, pd.boundary
        );
        assert!(
            pd.lost_components > 0,
            "a wholly dropped mark must be caught by the component check even when it moves neither the interior count nor the fringe ratio (interior={} fringe={} boundary={} ratio={ratio:.4})",
            pd.interior, pd.fringe, pd.boundary
        );
    }

    /// Temporary demonstration that [`FRINGE_RATIO_MAX`] actually bites, for
    /// the one failure mode neither [`lost_components`] nor the
    /// zero-tolerance interior check can see: the emitted outline's
    /// boundary drifting away from the swept pen along its whole perimeter,
    /// with no feature lost and no interior pixel wrong.
    ///
    /// `°` DEGREE SIGN (`degree()`, see `glyphs/symbols.rs`) is a small
    /// [`super::glyphs::ring`] — a closed loop built entirely from
    /// [`Seg::Quad`] segments — chosen because a ring has no straight run
    /// for a coarsened curve to fall back on: every part of its boundary is
    /// the thing being approximated, so degrading the flattening degrades
    /// the whole shape uniformly rather than one localised joint. Coarsening
    /// [`raster::QUAD_STEPS`] (16, the shipped value) down to `6` via
    /// [`rasterize_outline_raw_with_quad_steps`] leaves the ring's own hole
    /// open and every one of its contour rectangles anchored to the same
    /// flattened vertices used to build the reference — so
    /// [`lost_components`] and the interior check both read zero — while
    /// the polyline itself is coarse enough that the rectangles swept along
    /// it visibly bow away from the true annulus at 16px, moving
    /// `fringe / boundary` from this glyph's baseline 0.0000 at
    /// [`raster::QUAD_STEPS`] to `0.4000`, comfortably past
    /// [`FRINGE_RATIO_MAX`]. Measured by sweeping `quad_steps` down from 16
    /// (`coarsened_flattening_fringe_ratio_report`, `cargo test -p
    /// ocrcer-build -- --ignored --nocapture
    /// coarsened_flattening_fringe_ratio_report`): the fringe ratio stays
    /// under the bound with both interior and lost_components at zero
    /// through `quad_steps=7`, and first exceeds it, still with both other
    /// legs clean, at `quad_steps=6` — this test's value.
    #[test]
    #[ignore = "one-off demonstration, not a gate"]
    fn deliberate_breakage_demo_coarse_flattening_is_caught() {
        const COARSE_QUAD_STEPS: usize = 6;
        let degree = glyphs().into_iter().find(|g| g.codepoint == 0xB0).unwrap();
        let (want, _, cols, rows, _) = raster::render_raw(&degree, 16.0).unwrap();
        let (got, got_cols, got_rows, _) =
            rasterize_outline_raw_with_quad_steps(&degree, 16.0, COARSE_QUAD_STEPS).unwrap();
        assert_eq!((cols, rows), (got_cols, got_rows), "shared-grid comparisons are equal in size by construction");
        let pd = compare_grids(&want, &got, cols, rows);
        let ratio = if pd.boundary == 0 { 0.0 } else { pd.fringe as f64 / pd.boundary as f64 };
        println!(
            "degree sign quad_steps={COARSE_QUAD_STEPS} @ 16px: lost_components={} interior={} fringe={} boundary={} ratio={ratio:.4} (limit {FRINGE_RATIO_MAX})",
            pd.lost_components, pd.interior, pd.fringe, pd.boundary
        );
        assert_eq!(pd.lost_components, 0, "this demo isolates the fringe leg: no component should be dropped whole");
        assert_eq!(pd.interior, 0, "this demo isolates the fringe leg: no interior pixel should differ");
        assert!(
            ratio > FRINGE_RATIO_MAX,
            "coarsening flattening to {COARSE_QUAD_STEPS} quad steps must push the fringe ratio over {FRINGE_RATIO_MAX} while lost_components and interior both stay at zero, but got ratio={ratio:.4}"
        );
    }

    #[test]
    fn emitted_contours_match_the_rasterised_ink() {
        let mut lost_marks: Vec<(String, f32, usize)> = Vec::new();
        let mut interior_hits: Vec<(String, f32, usize)> = Vec::new();
        let mut worst_ratio: Vec<(String, f32, usize, usize, f64)> = Vec::new();
        // One side produced ink, the other none at all: a stronger
        // disagreement than any of the above, but still collected rather
        // than an immediate panic — a mid-sweep panic would hide every
        // other pair's diagnostics from this run's stdout, and this
        // category needs the same full-sweep visibility to adjudicate.
        let mut ink_only: Vec<(String, f32, &'static str)> = Vec::new();
        for glyph in glyphs() {
            for &size in &TEST_SIZES {
                let want = raster::render_raw(&glyph, size);
                let got = rasterize_outline_raw(&glyph, size);
                match (&want, &got) {
                    (None, None) => {}
                    (Some((w, _, cols, rows, _)), Some((g, _, _, _))) => {
                        let pd = compare_grids(w, g, *cols, *rows);
                        if pd.lost_components > 0 {
                            lost_marks.push((char_name(glyph.codepoint), size, pd.lost_components));
                        }
                        if pd.interior > 0 {
                            interior_hits.push((char_name(glyph.codepoint), size, pd.interior));
                        }
                        if pd.fringe > 0 || pd.boundary > 0 {
                            let ratio = if pd.boundary == 0 { 0.0 } else { pd.fringe as f64 / pd.boundary as f64 };
                            worst_ratio.push((char_name(glyph.codepoint), size, pd.fringe, pd.boundary, ratio));
                        }
                    }
                    (None, Some(_)) => {
                        ink_only.push((char_name(glyph.codepoint), size, "rasteriser produced no ink but the emitted outline does"));
                    }
                    (Some(_), None) => {
                        ink_only.push((char_name(glyph.codepoint), size, "emitted outline produced no ink but the rasteriser does"));
                    }
                }
            }
        }
        worst_ratio.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap());

        // Print every population before asserting on any of them, so one
        // run's stdout reports everything, not just whichever check fails
        // first.
        if !ink_only.is_empty() {
            println!("emit_ttf shape check: {} glyph/size pairs had ink on only one side:", ink_only.len());
            for (name, size, reason) in &ink_only {
                println!("  {name} @ {size}px: {reason}");
            }
        }
        if !lost_marks.is_empty() {
            println!("emit_ttf shape check: {} glyph/size pairs had a mark dropped entirely:", lost_marks.len());
            for (name, size, count) in &lost_marks {
                println!("  {name} @ {size}px: {count} lost component(s)");
            }
        }
        if !interior_hits.is_empty() {
            println!("emit_ttf shape check: {} glyph/size pairs had interior (non-boundary) differing pixels:", interior_hits.len());
            for (name, size, count) in &interior_hits {
                println!("  {name} @ {size}px: {count} interior px");
            }
        }
        if !worst_ratio.is_empty() {
            println!("emit_ttf shape check: {} glyph/size pairs had fringe disagreement, worst 30 by ratio:", worst_ratio.len());
            for (name, size, fringe, boundary, ratio) in worst_ratio.iter().take(30) {
                println!("  {name} @ {size}px: {fringe}/{boundary} = {ratio:.4}");
            }
        }

        // Assert in this order: a mark dropped whole is the most severe
        // defect (nothing survives to compare pixel-by-pixel), then a real
        // interior shape defect, then the boundary-approximation fringe
        // ratio — the least severe, most tolerant check, last.
        assert!(ink_only.is_empty(), "{} glyph/size pairs had ink on only one side (see stdout)", ink_only.len());
        assert!(
            lost_marks.is_empty(),
            "{} glyph/size pairs had at least one mark dropped entirely from the emitted outline (see stdout)",
            lost_marks.len()
        );
        assert!(
            interior_hits.is_empty(),
            "{} glyph/size pairs had interior differing pixels — a real shape defect, not boundary fuzz (see stdout)",
            interior_hits.len()
        );
        if let Some(&(_, _, _, _, max_ratio)) = worst_ratio.first() {
            assert!(
                max_ratio <= FRINGE_RATIO_MAX,
                "worst fringe/boundary ratio {max_ratio:.4} exceeds {FRINGE_RATIO_MAX} (see stdout for the full list)"
            );
        }
    }

    #[test]
    fn build_ttf_produces_a_well_formed_table_directory() {
        let bytes = build_ttf();
        assert_eq!(&bytes[0..4], &0x0001_0000u32.to_be_bytes(), "sfnt version");
        let num_tables = u16::from_be_bytes([bytes[4], bytes[5]]);
        assert_eq!(num_tables, 10);

        let required: [&[u8; 4]; 10] =
            [b"OS/2", b"cmap", b"glyf", b"head", b"hhea", b"hmtx", b"loca", b"maxp", b"name", b"post"];
        let mut seen = Vec::new();
        for i in 0..num_tables as usize {
            let rec = &bytes[12 + i * 16..12 + i * 16 + 16];
            seen.push([rec[0], rec[1], rec[2], rec[3]]);
        }
        for tag in required {
            assert!(seen.contains(tag), "missing required table {:?}", std::str::from_utf8(tag));
        }
        let mut sorted = seen.clone();
        sorted.sort();
        assert_eq!(seen, sorted, "table directory must be in ascending tag order");
    }
}

