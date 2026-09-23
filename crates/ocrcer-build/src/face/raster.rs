//! Rasterises a [`Glyph`] into the bitmap the feature extractor consumes.
//!
//! # Contract
//!
//! Flattening is fixed-step, never adaptive, so a glyph rasterises to the
//! same bytes on every machine that rebuilds the bank (`CLAUDE.md` rule 1).
//! A pixel is ink when more than half of its area lies within the pen
//! radius of the flattened polyline, estimated on a fixed subgrid (see
//! [`subgrid_hits`]), and the result is always cropped tight: the
//! runtime's connected-component labeller never hands the extractor a
//! padded box, so a padded fixture here would exercise a shape the engine
//! never sees in production.

use super::{Glyph, Seg, Stroke, STROKE, UPM};

/// A rasterised glyph, cropped tight to its ink.
pub struct Raster {
    /// Row-major, `width * height` bytes. `0` background, `1` ink.
    pub ink: Vec<u8>,
    /// **Coverage** rather than a decision: `0` paper, `255` fully covered,
    /// linear in the fraction of the subgrid inside the outline.
    ///
    /// On the *uncropped* rasterisation grid, [`Self::cov_cols`] x
    /// [`Self::cov_rows`], because the antialiased fringe of a mark reaches
    /// outside the box its majority-threshold ink occupies -- and in the
    /// pathological case that motivates this plane, a stem finer than the
    /// sampling pitch, almost the whole mark is outside it. Cropping
    /// coverage to the ink box would throw away exactly the pixels it exists
    /// to keep. [`Self::cov_dx`] and [`Self::cov_dy`] say where the ink box
    /// sits inside this grid.
    ///
    /// The prototype bank reads `ink` and never this: a prototype is the
    /// shape a binarised glyph has, and giving the extractor grey would
    /// change what every stored vector means. This plane exists for the
    /// *page* renderer, because a bilevel page is an undeclared degradation
    /// -- a stem thinner than the sampling pitch loses the pixels that fall
    /// under half coverage and arrives dotted, which no scanner, PDF
    /// rasteriser or spec-conformant monochrome rasteriser produces. Pages
    /// carry coverage and are thresholded by the engine's own binariser,
    /// which is both the faithful pipeline and the only way that stage gets
    /// exercised at all.
    pub cov: Vec<u8>,
    pub cov_cols: u32,
    pub cov_rows: u32,
    /// Column of the ink box's left edge within the coverage grid.
    pub cov_dx: u32,
    /// Row of the ink box's top edge within the coverage grid.
    pub cov_dy: u32,
    pub width: u32,
    pub height: u32,
    /// Baseline in pixels, measured downward from the top of the cropped
    /// box. May be negative (glyph entirely below the baseline) or exceed
    /// `height` (glyph entirely above it).
    pub baseline_dy: f32,
}

/// Equal-`t` steps used to flatten every [`Seg::Quad`]. Fixed rather than
/// adaptive: an error-tolerance comparison would make the flattened
/// polyline depend on floating-point rounding, and this face has to
/// rasterise identically on every machine that rebuilds the bank. 16 is
/// ample at the sizes this face renders at.
///
/// `pub(crate)` only so `emit_ttf`'s coarsened-flattening experiment (see
/// [`flatten_stroke_with_quad_steps`]) can name the shipped value as its
/// baseline rather than re-stating `16` as a second, driftable constant.
pub(crate) const QUAD_STEPS: usize = 16;

/// Renders `glyph` at `px_per_em`, cropped tight. `None` only when the
/// glyph has no strokes at all: a glyph whose pen is too fine to cover
/// half of any pixel still marks its best-covered pixel, so ink present
/// in the design is present in the bitmap at every size.
pub fn render(glyph: &Glyph, px_per_em: f32) -> Option<Raster> {
    let (ink, cov, cols, rows, py_top) = render_raw(glyph, px_per_em)?;
    crop(&ink, &cov, cols, rows, py_top)
}

/// [`render`] before the tight crop: `(ink, cols, rows, py_top)` on
/// [`glyph_grid`]'s shared, uncropped grid. `emit_ttf`'s emitted-outline
/// gate calls this instead of `render` so both sides of the comparison
/// are guaranteed equal in size by construction — a crop computed
/// separately on each side is a tight box around that side's *own* ink,
/// and the two boxes can differ by a column or row from an approximation
/// difference of a single boundary pixel, which is not a shape defect.
/// Every other caller wants the crop and should keep calling [`render`].
pub(crate) fn render_raw(
    glyph: &Glyph,
    px_per_em: f32,
) -> Option<(Vec<u8>, Vec<u8>, usize, usize, f64)> {
    let scale = (px_per_em / UPM as f32) as f64;
    let radius = STROKE as f64 * scale / 2.0;
    let radius_sq = radius * radius;

    let segments = flatten_glyph(glyph, scale);
    if segments.is_empty() {
        return None;
    }

    rasterize_raw(glyph, px_per_em, |x, y| {
        segments
            .iter()
            .any(|&(x0, y0, x1, y1)| dist_sq_point_segment(x, y, x0, y0, x1, y1) <= radius_sq)
    })
}

/// The uncropped pixel lattice a shape rasterises onto: `cols x rows`
/// unit cells, the leftmost starting at `col0` and the topmost row's
/// upper edge at `py_top`, all in scaled (pixel) design-unit space.
///
/// Named rather than a tuple because it is the one thing a glyph-shaped
/// rasterisation and an outline-shaped one must agree on exactly, and it
/// now has two sources: [`glyph_grid`] derives it from a [`Glyph`]'s pen
/// centreline, and [`Grid::covering`] derives it from any bounding box —
/// which is what a third-party font's contours can offer, having no
/// `Glyph` behind them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Grid {
    /// Left edge of column 0.
    pub col0: f64,
    pub cols: usize,
    pub rows: usize,
    /// Upper edge of row 0. Design-unit y is upward and bitmap rows
    /// increase downward, so row `r` spans `[py_top - r - 1, py_top - r]`.
    pub py_top: f64,
}

impl Grid {
    /// The grid covering `[px_min, px_max] x [py_min, py_max]`, sized by
    /// [`symmetric_span`] on both axes so the symmetry property that
    /// function exists to protect holds however the box falls against the
    /// pixel lattice. `None` for a degenerate box.
    pub(crate) fn covering(px_min: f64, px_max: f64, py_min: f64, py_max: f64) -> Option<Grid> {
        let (col0, cols) = symmetric_span(px_min, px_max);
        let (y_low, rows) = symmetric_span(py_min, py_max);
        if cols == 0 || rows == 0 {
            return None;
        }
        Some(Grid {
            col0,
            cols,
            rows,
            py_top: y_low + rows as f64,
        })
    }
}

/// The pixel grid both [`render`] and `emit_ttf`'s emitted-outline gate
/// sample against: bounds from the glyph's own flattened centreline
/// expanded by the pen radius, computed once here rather than separately
/// from whichever polygon a caller happens to have on hand. A generous
/// grid, guaranteed to contain every ink pixel with margin to spare (the
/// tight crop trims it back down), built symmetric about each axis's own
/// centre rather than floor/ceil of its extent: flooring `min` and
/// ceiling `max` independently gives each edge a different, generally
/// unequal, amount of quantization slack, which visibly distorts round
/// glyphs once cropped — not just top/bottom (`0`, `O`, `8`), but
/// left/right too (a pen dot, a disc, otherwise renders 3 wide by 4 tall
/// instead of square). `symmetric_span` instead anchors the grid so
/// sample `i` and sample `count-1-i` are exactly equidistant from the
/// centre by construction, on both axes, so a shape symmetric about its
/// own centre rasterises symmetric in pixels on that axis too.
///
/// `None` only when the glyph has no strokes at all.
///
/// Using the centreline's extent for both callers means their pixel
/// centres land at identical positions, so a disagreement between them
/// can only come from the fill rule under test, not from a sub-pixel
/// registration offset between two differently-sourced grids.
pub(crate) fn glyph_grid(glyph: &Glyph, px_per_em: f32) -> Option<Grid> {
    let scale = (px_per_em / UPM as f32) as f64;
    let radius = STROKE as f64 * scale / 2.0;
    let segments = flatten_glyph(glyph, scale);
    if segments.is_empty() {
        return None;
    }
    let (px_min, px_max, py_min, py_max) = segment_bounds(&segments, radius);
    Grid::covering(px_min, px_max, py_min, py_max)
}

/// Rasterises `inside` onto [`glyph_grid`]'s shared grid for `glyph` at
/// `px_per_em`, applying the shared sampling, threshold and
/// best-sub-threshold-pixel fallback (see [`apply_best_pixel_fallback`]),
/// with no crop: `(ink, cols, rows, py_top)`. This is the one rasterisation
/// loop [`render_raw`] and `emit_ttf`'s emitted-outline gate both call: two
/// different answers to "is this point inside" — pen-radius distance to the
/// centreline vs. winding number against emitted contours — sampled on the
/// identical grid and thresholded by the identical majority rule (see
/// [`SUBSAMPLE`]), so a disagreement between the two can only be about the
/// fill rule itself. [`render`] wraps this with [`crop`] for callers that
/// want the tight box; `emit_ttf`'s gate calls this directly so the two
/// sides it compares are the same grid by construction — see
/// [`render_raw`]'s doc for why two separately-cropped tight boxes were not
/// good enough.
pub(crate) fn rasterize_raw(
    glyph: &Glyph,
    px_per_em: f32,
    inside: impl FnMut(f64, f64) -> bool,
) -> Option<(Vec<u8>, Vec<u8>, usize, usize, f64)> {
    let grid = glyph_grid(glyph, px_per_em)?;
    let (ink, cov, cols, rows) = rasterize_on_grid(grid, inside);
    Some((ink, cov, cols, rows, grid.py_top))
}

/// [`rasterize_raw`]'s loop, against a [`Grid`] from any source rather
/// than a [`Glyph`]'s: sampling, threshold and best-sub-threshold-pixel
/// fallback, returning `(ink, coverage, cols, rows)` uncropped.
///
/// Split out so an outline that has no `Glyph` behind it — a third-party
/// font's contours — is rasterised by **this** code and not by a second
/// rasteriser with its own sampling policy. Two rasterisers would decide
/// which pixels are ink by different rules, and the prototype bank would
/// then be measured with two different rulers one level below the feature
/// extractor `CLAUDE.md` rule 4 names, with nothing to report the
/// disagreement.
pub(crate) fn rasterize_on_grid(
    grid: Grid,
    mut inside: impl FnMut(f64, f64) -> bool,
) -> (Vec<u8>, Vec<u8>, usize, usize) {
    let Grid {
        col0,
        cols,
        rows,
        py_top,
    } = grid;
    let mut ink = vec![0u8; cols * rows];
    let mut cov = vec![0u8; cols * rows];
    let mut best: Option<(usize, u32)> = None;
    for r in 0..rows {
        let cy = py_top - r as f64 - 0.5;
        for c in 0..cols {
            let cx = col0 + c as f64 + 0.5;
            let hits = subgrid_hits(cx, cy, &mut inside);
            cov[r * cols + c] = coverage_byte(hits);
            if pixel_is_ink(hits) {
                ink[r * cols + c] = 1;
            } else if hits > 0 && best.is_none_or(|(_, b)| hits > b) {
                best = Some((r * cols + c, hits));
            }
        }
    }
    apply_best_pixel_fallback(&mut ink, &mut cov, best);
    (ink, cov, cols, rows)
}

/// `hits` of [`SUBSAMPLE_CELLS`] as a byte, rounded to nearest: `0` for no
/// coverage and `255` for full, with no value in between reachable by two
/// different hit counts.
///
/// Integer arithmetic on purpose -- the corpus this feeds has to be
/// byte-identical on every machine that regenerates it, and a float scale
/// would make that a question about rounding mode.
pub(crate) fn coverage_byte(hits: u32) -> u8 {
    ((hits * 255 + SUBSAMPLE_CELLS / 2) / SUBSAMPLE_CELLS) as u8
}

/// Inks `best`'s pixel — the single highest sub-threshold hit count found
/// while rasterising, if any — when nothing else reached the majority
/// threshold, so a mark that exists in the design still exists in the
/// bitmap. Part of the shared rasterisation contract in [`rasterize`]: a
/// pen dot or a punctuation mark's whole ink can be too fine to cover half
/// of any pixel, and without this fallback such a mark would vanish from
/// one side of a two-renderer comparison and not the other, which is a
/// sampling artifact rather than a shape disagreement worth failing a
/// consistency check over.
pub(crate) fn apply_best_pixel_fallback(
    ink: &mut [u8],
    cov: &mut [u8],
    best: Option<(usize, u32)>,
) {
    if let Some((i, _)) = best {
        if !ink.contains(&1) {
            ink[i] = 1;
            // Coverage goes to full here, not to its measured value. This is
            // the one place the rasteriser overrides what it sampled, and it
            // is dropout control: the mark exists in the design, so it must
            // exist in the output, and leaving it at its true 30% would let
            // the page binariser delete a glyph the bank keeps.
            cov[i] = 255;
        }
    }
}

/// Every stroke's flattened polyline, in scaled (pixel) design-unit
/// coordinates, as `(x0, y0, x1, y1)` segments.
fn flatten_glyph(glyph: &Glyph, scale: f64) -> Vec<(f64, f64, f64, f64)> {
    let mut segments = Vec::new();
    for stroke in &glyph.strokes {
        let pts = flatten_stroke(stroke);
        for w in pts.windows(2) {
            let (x0, y0) = w[0];
            let (x1, y1) = w[1];
            segments.push((x0 * scale, y0 * scale, x1 * scale, y1 * scale));
        }
    }
    segments
}

/// One stroke's centreline as a polyline in design units. [`Seg::Quad`] is
/// subdivided by de Casteljau into [`QUAD_STEPS`] equal steps in `t`.
pub(crate) fn flatten_stroke(stroke: &Stroke) -> Vec<(f64, f64)> {
    flatten_stroke_with_quad_steps(stroke, QUAD_STEPS)
}

/// As [`flatten_stroke`], with the quadratic subdivision step count taken as
/// a parameter instead of fixed at [`QUAD_STEPS`].
///
/// Exists only so `emit_ttf`'s coarsened-flattening experiment can degrade
/// the resolution the emitter uses to turn a curved stroke into rectangle
/// contours, without perturbing the shipped path: [`flatten_stroke`] is the
/// only caller [`render`], `emit_ttf`'s production `glyph_outline`, and the
/// prototype bank ever use, and it always passes [`QUAD_STEPS`], so this
/// parameter is unreachable from anywhere but a test that asks for it by
/// name.
pub(crate) fn flatten_stroke_with_quad_steps(stroke: &Stroke, quad_steps: usize) -> Vec<(f64, f64)> {
    let mut pts = Vec::with_capacity(stroke.segs.len() + 1);
    let mut cur = (stroke.start.x as f64, stroke.start.y as f64);
    pts.push(cur);
    for seg in &stroke.segs {
        match *seg {
            Seg::Line(q) => {
                cur = (q.x as f64, q.y as f64);
                pts.push(cur);
            }
            Seg::Quad { ctrl, to } => {
                let p0 = cur;
                let p1 = (ctrl.x as f64, ctrl.y as f64);
                let p2 = (to.x as f64, to.y as f64);
                for i in 1..=quad_steps {
                    let t = i as f64 / quad_steps as f64;
                    pts.push(crate::outline::quad_point(p0, p1, p2, t));
                }
                cur = p2;
            }
        }
    }
    pts
}

/// A pixel-count span over `[min, max]`, anchored symmetric about the
/// span's own centre rather than at `min.floor()`/`max.ceil()`. Returns
/// `(low_edge, count)`: `count` cells of width 1, the first starting at
/// `low_edge`, sized with a full spare pixel of margin on each side so the
/// grid is guaranteed to cover `[min, max]` regardless of how its centre
/// falls against the pixel lattice. Used for both axes so a shape
/// symmetric about its own centre on an axis rasterises symmetric in
/// pixels on that axis, independent of where `min`/`max` happen to land.
pub(crate) fn symmetric_span(min: f64, max: f64) -> (f64, usize) {
    let center = (min + max) / 2.0;
    let half_span = (max - min) / 2.0;
    let count = half_span.ceil() as usize * 2 + 2;
    (center - count as f64 / 2.0, count)
}

/// Bounding box of the pen's ink in scaled (pixel) design-unit space: the
/// flattened segments' extent, expanded by the pen radius. Returns
/// `(min_x, max_x, min_y, max_y)`.
fn segment_bounds(segments: &[(f64, f64, f64, f64)], radius: f64) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for &(x0, y0, x1, y1) in segments {
        min_x = min_x.min(x0.min(x1));
        max_x = max_x.max(x0.max(x1));
        min_y = min_y.min(y0.min(y1));
        max_y = max_y.max(y0.max(y1));
    }
    (min_x - radius, max_x + radius, min_y - radius, max_y + radius)
}

/// Sub-samples per axis, so each pixel is estimated from
/// `SUBSAMPLE * SUBSAMPLE` points.
///
/// Testing a pixel's *centre* alone makes ink a function of the mark's
/// sub-pixel phase: a stroke 4.48px wide renders 4px or 5px depending on
/// where it lands, and a pen dot narrower than a pixel disappears
/// entirely at some sizes but not others, non-monotonically. Both
/// distort exactly the features the extractor reads. Sampling on a
/// subgrid and thresholding on coverage replaces that with an error
/// bounded by the subgrid pitch.
///
/// **Odd on purpose.** With an even count the offsets straddle the pixel
/// centre, so an edge anywhere in the middle `1 / SUBSAMPLE` of the pixel
/// scores exactly half the samples; taking those as ink then adds a
/// `1 / (2 * SUBSAMPLE)` px band of ink along *every* edge, which on this
/// face's pen measured as 2-5% more ink than the same outline rendered by
/// FreeType. An odd count puts a sample exactly on the centre, and a
/// strict majority then flips precisely when the edge crosses it: no bias,
/// in either direction.
///
/// 5 costs 25x on a build-time render of ~150 glyphs, which is nothing.
///
/// This constant, [`SUBSAMPLE_CELLS`] and the offsets and threshold in
/// [`subgrid_hits`]/[`pixel_is_ink`] are the one sampling policy shared by
/// [`render`] and `emit_ttf`'s `emitted_contours_match_the_rasterised_ink`
/// gate. The two ask a different "is this point inside" question — pen
/// radius vs. winding number — but must resolve it on the identical
/// subgrid, or a disagreement between them would measure the sampling
/// policy instead of the fill rule the gate exists to check.
const SUBSAMPLE: usize = 5;

/// Sample points per pixel. A pixel is ink at a strict **majority** of
/// these — see [`SUBSAMPLE`] for why a tie must not count as ink.
const SUBSAMPLE_CELLS: u32 = (SUBSAMPLE * SUBSAMPLE) as u32;

/// How many of a pixel centred at `(px, py)`'s `SUBSAMPLE * SUBSAMPLE`
/// sample points satisfy `inside`. Offsets are `(i + 0.5) / SUBSAMPLE`,
/// symmetric about the pixel centre, so a shape symmetric about a pixel
/// centre still rasterises symmetric — the property [`symmetric_span`]
/// exists to protect.
///
/// `inside` is the one thing that may legitimately differ between
/// callers — see [`SUBSAMPLE`]'s doc for why the subgrid itself must not.
pub(crate) fn subgrid_hits(px: f64, py: f64, mut inside: impl FnMut(f64, f64) -> bool) -> u32 {
    let mut hits = 0;
    for sy in 0..SUBSAMPLE {
        let y = py + (sy as f64 + 0.5) / SUBSAMPLE as f64 - 0.5;
        for sx in 0..SUBSAMPLE {
            let x = px + (sx as f64 + 0.5) / SUBSAMPLE as f64 - 0.5;
            if inside(x, y) {
                hits += 1;
            }
        }
    }
    hits
}

/// Whether `hits` sample points out of [`SUBSAMPLE_CELLS`] make a pixel
/// ink: a strict majority, per [`SUBSAMPLE`]'s doc.
pub(crate) fn pixel_is_ink(hits: u32) -> bool {
    hits * 2 > SUBSAMPLE_CELLS
}

/// Squared distance from `(px, py)` to the segment `(x0,y0)-(x1,y1)`:
/// project onto the segment and clamp the parameter to `[0, 1]`. A
/// zero-length segment — how [`Stroke::dot`] is authored — has
/// `len_sq == 0`, so `t` is fixed at `0` rather than dividing by it;
/// distance degenerates to point-to-point, which is exactly what a
/// zero-length segment means.
fn dist_sq_point_segment(px: f64, py: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq <= 0.0 {
        0.0
    } else {
        (((px - x0) * dx + (py - y0) * dy) / len_sq).clamp(0.0, 1.0)
    };
    let cx = x0 + t * dx;
    let cy = y0 + t * dy;
    let ex = px - cx;
    let ey = py - cy;
    ex * ex + ey * ey
}

/// Crops the generous `cols x rows` grid to its tight ink bounding box.
/// `py_top` is the generous grid's top edge, in the same scaled
/// design-unit y used to build it, needed to recompute `baseline_dy`
/// against the new (cropped) top.
pub(crate) fn crop(
    ink: &[u8],
    cov: &[u8],
    cols: usize,
    rows: usize,
    py_top: f64,
) -> Option<Raster> {
    let mut min_r = usize::MAX;
    let mut max_r = 0usize;
    let mut min_c = usize::MAX;
    let mut max_c = 0usize;
    let mut any = false;
    for r in 0..rows {
        for c in 0..cols {
            if ink[r * cols + c] != 0 {
                any = true;
                min_r = min_r.min(r);
                max_r = max_r.max(r);
                min_c = min_c.min(c);
                max_c = max_c.max(c);
            }
        }
    }
    if !any {
        return None;
    }

    let width = (max_c - min_c + 1) as u32;
    let height = (max_r - min_r + 1) as u32;
    let mut cropped = vec![0u8; (width * height) as usize];
    for r in 0..height as usize {
        for c in 0..width as usize {
            cropped[r * width as usize + c] = ink[(min_r + r) * cols + (min_c + c)];
        }
    }

    // The baseline (design-unit y = 0) sits at py_top - min_r pixels above
    // the cropped top, in this scaled coordinate system. Downward-positive
    // by construction: row indices grow downward, and py_top is the
    // y-coordinate of row 0's top edge.
    let baseline_dy = (py_top - min_r as f64) as f32;

    // The ink plane is cropped and the coverage plane is not: the bank wants
    // the tight box the component labeller would hand the extractor, and the
    // page renderer wants every pixel the mark touched.
    Some(Raster {
        ink: cropped,
        cov: cov.to_vec(),
        cov_cols: cols as u32,
        cov_rows: rows as u32,
        cov_dx: min_c as u32,
        cov_dy: min_r as u32,
        width,
        height,
        baseline_dy,
    })
}

#[cfg(test)]
mod tests {
    use super::super::glyphs::glyphs;
    use super::super::{p, CAP, DESCENDER, STROKE as FACE_STROKE, UPM as FACE_UPM, X_HEIGHT};
    use super::*;
    use ocrcer_core::feature::{extract, GlyphInput, FEATURE_DIMS, HOLE_COUNT};
    use ocrcer_core::image::components::{label, Connectivity};

    const TEST_SIZES: [f32; 2] = [32.0, 64.0];

    fn char_name(cp: u32) -> String {
        char::from_u32(cp).map(|c| c.to_string()).unwrap_or_else(|| format!("U+{cp:04X}"))
    }

    // ---- the zero-length-segment path ----

    #[test]
    fn dot_renders() {
        let g = Glyph { codepoint: 0, advance: 0, strokes: vec![Stroke::dot(p(0, 0))] };
        let px_per_em = 100.0;
        let r = render(&g, px_per_em).expect("a dot must put down ink");
        let scale = px_per_em / FACE_UPM as f32;
        let expected = STROKE as f32 * scale;
        assert!(
            (r.width as f32 - expected).abs() <= 1.5,
            "dot width {} should be roughly STROKE ({expected}) pixels wide",
            r.width
        );
        assert!(
            (r.height as f32 - expected).abs() <= 1.5,
            "dot height {} should be roughly STROKE ({expected}) pixels tall",
            r.height
        );
    }

    /// A dot is a disc: the cheapest possible check that the sampling grid
    /// treats both axes the same way. An anisotropic grid (one axis
    /// floor/ceil'd, the other centred) passes `dot_renders`'s loose
    /// STROKE-ish tolerance while still rendering visibly non-square.
    #[test]
    fn dot_renders_square() {
        for &px_per_em in &TEST_SIZES {
            let g = Glyph { codepoint: 0, advance: 0, strokes: vec![Stroke::dot(p(0, 0))] };
            let r = render(&g, px_per_em).expect("a dot must put down ink");
            assert_eq!(
                r.width, r.height,
                "a dot must render square (isotropic grid): got {}x{} at {px_per_em}px",
                r.width, r.height
            );
        }
    }

    // ---- tight crop ----

    #[test]
    fn every_glyph_crops_tight() {
        for glyph in glyphs() {
            for &size in &TEST_SIZES {
                let r = render(&glyph, size)
                    .unwrap_or_else(|| panic!("{} produced no ink at {size}px", char_name(glyph.codepoint)));
                let w = r.width as usize;
                let h = r.height as usize;
                let row_has_ink = |row: usize| (0..w).any(|c| r.ink[row * w + c] != 0);
                let col_has_ink = |col: usize| (0..h).any(|row| r.ink[row * w + col] != 0);
                let name = char_name(glyph.codepoint);
                assert!(row_has_ink(0), "{name} top row blank at {size}px");
                assert!(row_has_ink(h - 1), "{name} bottom row blank at {size}px");
                assert!(col_has_ink(0), "{name} left column blank at {size}px");
                assert!(col_has_ink(w - 1), "{name} right column blank at {size}px");
            }
        }
    }

    // ---- orientation ----

    #[test]
    fn cap_height_stroke_is_taller_than_x_height_stroke() {
        let tall = Glyph { codepoint: 0, advance: 0, strokes: vec![Stroke::line(&[p(0, 0), p(0, CAP)])] };
        let short = Glyph { codepoint: 0, advance: 0, strokes: vec![Stroke::line(&[p(0, 0), p(0, X_HEIGHT)])] };
        let rt = render(&tall, 64.0).unwrap();
        let rs = render(&short, 64.0).unwrap();
        assert!(
            rt.height > rs.height,
            "a stroke to CAP ({}) must render taller than one to X_HEIGHT ({})",
            rt.height,
            rs.height
        );
    }

    #[test]
    fn l_shape_foot_lands_in_the_bottom_row_not_the_top() {
        // Vertical stroke plus a horizontal foot at the baseline (y = 0).
        // Design-unit y is upward, so the foot — the wide part — must land
        // in the BOTTOM row of the bitmap. A flip would put it at the top,
        // and every other test in this module would still read as passing.
        let l = Glyph {
            codepoint: 0,
            advance: 0,
            strokes: vec![Stroke::line(&[p(0, CAP), p(0, 0), p(200, 0)])],
        };
        let r = render(&l, 64.0).unwrap();
        let w = r.width as usize;
        let h = r.height as usize;
        let ink_in_row = |row: usize| (0..w).filter(|&c| r.ink[row * w + c] != 0).count();
        let top = ink_in_row(0);
        let bottom = ink_in_row(h - 1);
        assert!(bottom > top, "bottom row ink ({bottom}) must exceed top row ink ({top})");
    }

    // ---- hole counts, the reason this face exists ----

    fn extract_for(glyph: &Glyph, px_per_em: f32) -> [f32; FEATURE_DIMS] {
        let r = render(glyph, px_per_em).unwrap();
        let x_height = X_HEIGHT as f32 * px_per_em / FACE_UPM as f32;
        extract(&GlyphInput { ink: &r.ink, width: r.width, height: r.height, baseline_dy: r.baseline_dy, x_height })
    }

    /// The render size at which each glyph's hole count stops matching its
    /// design, taking the largest render as the design intent.
    ///
    /// Hole count is the first pruning stage, so a counter that closes at a
    /// given size does not merely degrade that glyph's score — it moves the
    /// glyph into a different bucket, and the right prototype is discarded
    /// before any distance is computed. This report gives the number a
    /// minimum-DPI or upsampling decision has to be made against, per glyph,
    /// rather than for the face as a whole.
    ///
    /// `cargo test -p ocrcer-build -- --ignored --nocapture hole_count_survival`
    #[test]
    #[ignore = "report, not a gate"]
    fn hole_count_survival_report() {
        let sizes = [16.0f32, 20.0, 24.0, 28.0, 32.0, 40.0, 48.0, 64.0, 96.0];
        let head: String = sizes.iter().map(|s| format!("{s:>5}")).collect();
        println!("\nglyph  want {head}");
        let mut clean = 0usize;
        for glyph in glyphs() {
            let counts: Vec<u32> =
                sizes.iter().map(|&px| match render(&glyph, px) {
                    Some(_) => extract_for(&glyph, px)[HOLE_COUNT] as u32,
                    None => u32::MAX,
                }).collect();
            let want = *counts.last().unwrap();
            if counts.iter().all(|&c| c == want) {
                clean += 1;
                continue;
            }
            let row: String = counts.iter().map(|c| format!("{c:>5}")).collect();
            println!("{:>5} {want:>5} {row}", char_name(glyph.codepoint));
        }
        println!("{clean} glyphs hold their hole count at every size down to {}px", sizes[0]);
    }

    #[test]
    fn hole_counts_match_the_face_design() {
        let expected: &[(u32, u32)] = &[
            ('1' as u32, 0),
            ('I' as u32, 0),
            ('l' as u32, 0),
            ('.' as u32, 0),
            ('2' as u32, 0),
            ('3' as u32, 0),
            ('4' as u32, 0),
            ('5' as u32, 0),
            ('7' as u32, 0),
            ('0' as u32, 1),
            ('O' as u32, 1),
            ('6' as u32, 1),
            ('9' as u32, 1),
            ('8' as u32, 2),
            (0x00D8, 2), // Oslash
            (0x2300, 2), // diameter sign
            ('A' as u32, 1),
            ('B' as u32, 2),
            ('C' as u32, 0),
            ('D' as u32, 1),
            ('E' as u32, 0),
            ('F' as u32, 0),
            ('G' as u32, 0),
            ('H' as u32, 0),
            ('J' as u32, 0),
            ('K' as u32, 0),
            ('L' as u32, 0),
            ('M' as u32, 0),
            ('N' as u32, 0),
            ('P' as u32, 1),
            ('Q' as u32, 1),
            ('R' as u32, 1),
            ('S' as u32, 0),
            ('T' as u32, 0),
            ('U' as u32, 0),
            ('V' as u32, 0),
            ('W' as u32, 0),
            ('X' as u32, 0),
            ('Y' as u32, 0),
            ('Z' as u32, 0),
            (0x00C6, 1), // AE
            ('a' as u32, 1),
            ('b' as u32, 1),
            ('c' as u32, 0),
            ('d' as u32, 1),
            ('e' as u32, 1),
            ('f' as u32, 0),
            ('g' as u32, 1),
            ('h' as u32, 0),
            ('i' as u32, 0),
            ('j' as u32, 0),
            ('k' as u32, 0),
            ('m' as u32, 0),
            ('n' as u32, 0),
            ('o' as u32, 1),
            ('p' as u32, 1),
            ('q' as u32, 1),
            ('r' as u32, 0),
            ('s' as u32, 0),
            ('t' as u32, 0),
            ('u' as u32, 0),
            ('v' as u32, 0),
            ('w' as u32, 0),
            ('x' as u32, 0),
            ('y' as u32, 0),
            ('z' as u32, 0),
            (0x00DF, 2), // sharp s
            (0x00E6, 2), // ae
            // punctuation — every mark in the slice is open or solid;
            // nothing in it closes a counter.
            ('!' as u32, 0),
            ('"' as u32, 0),
            ('\'' as u32, 0),
            ('(' as u32, 0),
            (')' as u32, 0),
            (',' as u32, 0),
            ('-' as u32, 0),
            ('/' as u32, 0),
            (':' as u32, 0),
            (';' as u32, 0),
            ('?' as u32, 0),
            ('[' as u32, 0),
            ('\\' as u32, 0),
            (']' as u32, 0),
            ('_' as u32, 0),
            ('`' as u32, 0),
            ('{' as u32, 0),
            ('}' as u32, 0),
            (0x00B7, 0), // middle dot
            (0x2022, 0), // bullet — two nested loops, solid, no annulus
            (0x2013, 0), // en dash
            (0x2014, 0), // em dash
            (0x2018, 0), // left single quote
            (0x2019, 0), // right single quote
            (0x201C, 0), // left double quote
            (0x201D, 0), // right double quote
            (0x2026, 0), // ellipsis
            // accented Latin-1 — a composed form inherits its base's hole
            // count: none of grave, acute, circumflex, tilde, diaeresis or
            // cedilla closes a loop. The ring on A-ring does, so those two
            // gain one.
            (0x00C7, 0), // Ccedilla
            (0x00C8, 0), // Egrave
            (0x00C9, 0), // Eacute
            (0x00CA, 0), // Ecircumflex
            (0x00CB, 0), // Ediaeresis
            (0x00CC, 0), // Igrave
            (0x00CD, 0), // Iacute
            (0x00CE, 0), // Icircumflex
            (0x00CF, 0), // Idiaeresis
            (0x00D1, 0), // Ntilde
            (0x00D9, 0), // Ugrave
            (0x00DA, 0), // Uacute
            (0x00DB, 0), // Ucircumflex
            (0x00DC, 0), // Udiaeresis
            (0x00DD, 0), // Yacute
            (0x00E7, 0), // ccedilla
            (0x00EC, 0), // igrave
            (0x00ED, 0), // iacute
            (0x00EE, 0), // icircumflex
            (0x00EF, 0), // idiaeresis
            (0x00F1, 0), // ntilde
            (0x00F9, 0), // ugrave
            (0x00FA, 0), // uacute
            (0x00FB, 0), // ucircumflex
            (0x00FC, 0), // udiaeresis
            (0x00FD, 0), // yacute
            (0x00FF, 0), // ydiaeresis
            (0x00C0, 1), // Agrave
            (0x00C1, 1), // Aacute
            (0x00C2, 1), // Acircumflex
            (0x00C3, 1), // Atilde
            (0x00C4, 1), // Adiaeresis
            (0x00D2, 1), // Ograve
            (0x00D3, 1), // Oacute
            (0x00D4, 1), // Ocircumflex
            (0x00D5, 1), // Otilde
            (0x00D6, 1), // Odiaeresis
            (0x00E0, 1), // agrave
            (0x00E1, 1), // aacute
            (0x00E2, 1), // acircumflex
            (0x00E3, 1), // atilde
            (0x00E4, 1), // adiaeresis
            (0x00E8, 1), // egrave
            (0x00E9, 1), // eacute
            (0x00EA, 1), // ecircumflex
            (0x00EB, 1), // ediaeresis
            (0x00F2, 1), // ograve
            (0x00F3, 1), // oacute
            (0x00F4, 1), // ocircumflex
            (0x00F5, 1), // otilde
            (0x00F6, 1), // odiaeresis
            (0x00C5, 2), // Aring
            (0x00E5, 2), // aring
            // symbols — measured at 64px after the glyph-defect fixes (see
            // `glyphs/symbols.rs` doc comments for the per-glyph derivation).
            (0x2300, 2), // diameter sign
            ('#' as u32, 1), // hash
            ('%' as u32, 2), // percent
            (0x2030, 2), // per mille — 3 discs, clamped from an unclamped 3
            ('&' as u32, 2), // ampersand — stable 2 from 20px up; drops to 1
            // at 16px only (below the coordinator's 24px-and-up target, not
            // chased further — see `ampersand`'s doc comment).
            ('*' as u32, 0), // asterisk
            ('@' as u32, 2), // at sign — stable 2 across the whole 16-96px sweep
            ('^' as u32, 0), // caret
            ('|' as u32, 0), // pipe
            ('~' as u32, 0), // tilde
            (0x00B0, 1), // degree
            (0x00B5, 0), // micro
            (0x00BC, 0), // quarter
            (0x00BD, 0), // half
            (0x00BE, 0), // three quarters
            (0x00A7, 2), // section
            (0x00B6, 1), // pilcrow
            (0x2020, 0), // dagger
            (0x2021, 0), // double dagger
            (0x00A9, 1), // copyright — stable 1 across the whole 16-96px sweep
            (0x00AE, 2), // registered — stable 2 across the whole 16-96px sweep
            (0x2122, 0), // trademark
            (0x03A9, 0), // Omega
            ('+' as u32, 0),
            ('<' as u32, 0),
            ('=' as u32, 0),
            ('>' as u32, 0),
            (0x00B1, 0), // plus-minus
            (0x00D7, 0), // multiply
            (0x00F7, 0), // divide
            (0x221A, 0), // radical
            (0x2264, 0), // less-equal
            (0x2265, 0), // greater-equal
            (0x2248, 0), // approx
            (0x2260, 0), // not-equal
            ('$' as u32, 2), // dollar
            (0x00A2, 1), // cent
            (0x00A3, 0), // pound
            (0x20AC, 0), // euro — stable 0 across the whole 16-96px sweep
            (0x00A5, 0), // yen
        ];
        for glyph in glyphs() {
            let want = expected
                .iter()
                .find(|&&(cp, _)| cp == glyph.codepoint)
                .unwrap_or_else(|| panic!("{} missing from the expected hole-count table", char_name(glyph.codepoint)))
                .1;
            let features = extract_for(&glyph, 64.0);
            assert_eq!(
                features[HOLE_COUNT] as u32,
                want,
                "{} expected {want} holes, glyph is drawn wrong if this fails",
                char_name(glyph.codepoint)
            );
        }
    }

    #[test]
    fn diameter_sign_is_nearer_to_eight_than_zero_is() {
        let by_cp = |cp: u32| glyphs().into_iter().find(|g| g.codepoint == cp).unwrap();
        let zero = extract_for(&by_cp('0' as u32), 64.0);
        let eight = extract_for(&by_cp('8' as u32), 64.0);
        let oslash = extract_for(&by_cp(0x00D8), 64.0);

        let dist = |a: &[f32; FEATURE_DIMS], b: &[f32; FEATURE_DIMS]| -> f64 {
            a.iter().zip(b.iter()).map(|(x, y)| ((*x - *y) as f64).powi(2)).sum::<f64>().sqrt()
        };
        let d_zero_eight = dist(&zero, &eight);
        let d_oslash_eight = dist(&oslash, &eight);
        let d_oslash_zero = dist(&oslash, &zero);
        println!(
            "distances: 0-8={d_zero_eight:.4} Oslash-8={d_oslash_eight:.4} Oslash-0={d_oslash_zero:.4}"
        );
        assert!(
            d_oslash_eight < d_zero_eight,
            "Oslash-to-8 distance ({d_oslash_eight}) must be less than 0-to-8 distance ({d_zero_eight})"
        );
    }

    /// Unweighted L2 between two raw feature vectors — the same formula as
    /// the `dist` closure in [`diameter_sign_is_nearer_to_eight_than_zero_is`]
    /// above, pulled out to a named function because
    /// [`pairwise_margin_report`] calls it ~70,000 times.
    ///
    /// This is *not* the calibrated weighted-L2 `ARCHITECTURE.md` section
    /// 4.1 step 3 specifies for the shipped matcher — see that function's
    /// doc comment for why unweighted is what is measurable today.
    fn l2(a: &[f32; FEATURE_DIMS], b: &[f32; FEATURE_DIMS]) -> f64 {
        a.iter().zip(b.iter()).map(|(x, y)| ((*x - *y) as f64).powi(2)).sum::<f64>().sqrt()
    }

    /// Linear interpolation between order statistics of an ascending-sorted
    /// slice. Standard definition; `p` in `0.0..=100.0`.
    fn percentile(sorted: &[f64], p: f64) -> f64 {
        if sorted.is_empty() {
            return f64::NAN;
        }
        let rank = (p / 100.0) * (sorted.len() as f64 - 1.0);
        let lo = rank.floor() as usize;
        let hi = rank.ceil() as usize;
        if lo == hi {
            sorted[lo]
        } else {
            let frac = rank - lo as f64;
            sorted[lo] * (1.0 - frac) + sorted[hi] * frac
        }
    }

    /// Face-wide pairwise separation in feature space: for every unordered
    /// pair of distinct glyphs, at every render size in `SIZES`, the
    /// distance between their [`extract`]ed feature vectors.
    ///
    /// # Why this exists
    ///
    /// `ARCHITECTURE.md` rule 5 defines confidence as the *match margin* —
    /// how much better the winning class scored than its nearest rival of a
    /// different class. That makes the smallest inter-class distance in the
    /// prototype bank the ceiling on the engine's achievable confidence, and
    /// nothing before this test measured it: `aspect_report` above is a
    /// *screen* (flags aspect-band overlap within a hole-count bucket), not
    /// a measurement of the quantity confidence actually depends on. A
    /// screen can both over-report (flag a pair that is in fact well
    /// separated in the full 107 dims) and under-report (miss a pair that
    /// overlaps in every dim except aspect). This test measures the real
    /// quantity directly.
    ///
    /// # Metric: unweighted L2, and why not weighted-L2
    ///
    /// `ocrcer-core::match` (`crates/ocrcer-core/src/match.rs`) is an empty
    /// stub today — a module doc comment and nothing else, chunk 5 work not
    /// yet started. `ARCHITECTURE.md` section 4.1 step 3 calls the matcher's
    /// metric "weighted-L2" but the weighting is the per-dimension
    /// `feature_norm` standardisation table (section 2), which is
    /// "computed over the finished bank" — and no bank exists yet either
    /// (chunk 3, not started; `ocrcer-build`'s `main` prints "nothing to
    /// build yet"). There is therefore no weighted-L2 to call, reimplement,
    /// or even a documented weight to approximate it with; inventing one
    /// now would be exactly the "plausible-looking number" `CLAUDE.md` rule
    /// 1 rules out. This report instead uses plain unweighted L2 over the
    /// raw `extract()` output — the same metric `diameter_sign_is_nearer_to_eight_than_zero_is`
    /// above already uses as its stand-in for match distance, so this
    /// report's numbers are on the same footing as that test's and directly
    /// comparable to them. **This is a measured proxy, not the shipped
    /// matcher's metric**, and every distance printed below should be read
    /// with that caveat; re-run this report once `feature_norm` and
    /// `match.rs` exist, because per-dimension standardisation can reorder
    /// close pairs (a dimension with low natural variance is weighted up).
    ///
    /// # Hole-count prune survival
    ///
    /// `ARCHITECTURE.md` section 4.1 step 1 prunes candidates to an exact
    /// integer match on [`HOLE_COUNT`] before any distance is computed. A
    /// pair whose hole counts differ *at the size where they are closest*
    /// never reaches the distance stage at that size, so a small distance
    /// there costs nothing; a pair with matching hole counts is a real
    /// rival. Each pair below is marked accordingly, using the hole count
    /// [`extract`] itself reports at that size (which can differ across
    /// sizes — see `hole_count_survival_report`), not a static charset
    /// property.
    ///
    /// `cargo test -p ocrcer-build -- --ignored --nocapture pairwise_margin_report`
    #[test]
    #[ignore = "report, not a gate"]
    fn pairwise_margin_report() {
        // The gated render range per the coordinator's brief: at minimum
        // 16/32/48/64px, because a margin comfortable at 64px and vanishing
        // at 16px is the case that matters and a single-size measurement
        // would hide it.
        const SIZES: [f32; 4] = [16.0, 32.0, 48.0, 64.0];

        let all_glyphs = glyphs();
        let n = all_glyphs.len();
        let name_of = |idx: usize| char_name(all_glyphs[idx].codepoint);
        let idx_of = |cp: u32| all_glyphs.iter().position(|g| g.codepoint == cp).unwrap();

        // features[size_idx][glyph_idx]: extracted once per glyph per size
        // and reused for every pair, per the coordinator's brief (187
        // glyphs is ~17,000 pairs per size; re-extracting per pair would be
        // ~35,000 redundant extractions per size instead of 187).
        let features: Vec<Vec<[f32; FEATURE_DIMS]>> =
            SIZES.iter().map(|&size| all_glyphs.iter().map(|g| extract_for(g, size)).collect()).collect();

        struct PairMin {
            i: usize,
            j: usize,
            dist: f64,
            size: f32,
            same_bucket: bool,
        }

        let mut mins: Vec<PairMin> = Vec::with_capacity(n * (n - 1) / 2);
        let mut per_size_dists: Vec<Vec<f64>> = vec![Vec::with_capacity(n * (n - 1) / 2); SIZES.len()];

        for i in 0..n {
            for j in (i + 1)..n {
                let mut best_dist = f64::INFINITY;
                let mut best_size = SIZES[0];
                let mut best_same_bucket = false;
                for (s_idx, &size) in SIZES.iter().enumerate() {
                    let a = &features[s_idx][i];
                    let b = &features[s_idx][j];
                    let d = l2(a, b);
                    per_size_dists[s_idx].push(d);
                    if d < best_dist {
                        best_dist = d;
                        best_size = size;
                        best_same_bucket = (a[HOLE_COUNT] as i32) == (b[HOLE_COUNT] as i32);
                    }
                }
                mins.push(PairMin { i, j, dist: best_dist, size: best_size, same_bucket: best_same_bucket });
            }
        }

        // ---- distribution summary, per size ----
        println!("\n=== pairwise_margin_report: distribution summary (unweighted L2) ===");
        println!("{n} glyphs, {} pairs per size", mins.len());
        for (s_idx, &size) in SIZES.iter().enumerate() {
            let mut d = per_size_dists[s_idx].clone();
            d.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "  {size:>5.0}px  min={:.6}  p1={:.6}  p5={:.6}  median={:.6}",
                d[0],
                percentile(&d, 1.0),
                percentile(&d, 5.0),
                percentile(&d, 50.0)
            );
        }

        // ---- 30 closest pairs face-wide (by each pair's own minimum over
        // sizes, since the smallest margin anywhere in the gated range is
        // the one that caps confidence) ----
        let mut ranked: Vec<&PairMin> = mins.iter().collect();
        ranked.sort_by(|a, b| a.dist.partial_cmp(&b.dist).unwrap());
        println!("\n=== 30 closest pairs face-wide ===");
        println!("{:<28} {:>9} {:>6} hole-count prune", "pair (codepoints)", "dist", "size");
        for pm in ranked.iter().take(30) {
            let bucket = if pm.same_bucket { "same bucket (reaches distance stage)" } else { "different buckets (pruned, costs nothing)" };
            println!(
                "  {} U+{:04X} / {} U+{:04X}   {:>11.6} {:>5.0}px  {bucket}",
                name_of(pm.i),
                all_glyphs[pm.i].codepoint,
                name_of(pm.j),
                all_glyphs[pm.j].codepoint,
                pm.dist,
                pm.size
            );
        }

        // ---- each glyph's nearest rival ----
        // O(n * pairs); n=187 so ~3.2M comparisons, trivial. Sorted
        // ascending so a glyph uniformly close to everything (many small
        // margins) is as visible as one with a single dangerous twin (one
        // small margin, otherwise well separated) — both show up, just at
        // different rows.
        struct NearestRival {
            glyph: usize,
            rival: usize,
            dist: f64,
            size: f32,
            same_bucket: bool,
        }
        let mut nearest: Vec<NearestRival> = Vec::with_capacity(n);
        for k in 0..n {
            let mut best: Option<&PairMin> = None;
            for pm in &mins {
                if (pm.i == k || pm.j == k) && best.is_none_or(|b| pm.dist < b.dist) {
                    best = Some(pm);
                }
            }
            let pm = best.expect("n >= 2, every glyph has at least one rival");
            let rival = if pm.i == k { pm.j } else { pm.i };
            nearest.push(NearestRival { glyph: k, rival, dist: pm.dist, size: pm.size, same_bucket: pm.same_bucket });
        }
        nearest.sort_by(|a, b| a.dist.partial_cmp(&b.dist).unwrap());
        println!("\n=== each glyph's nearest rival, ascending margin ===");
        println!("{:<10} {:<10} {:>9} {:>6} hole-count prune", "glyph", "rival", "dist", "size");
        for nr in &nearest {
            let bucket = if nr.same_bucket { "same bucket" } else { "different buckets" };
            println!(
                "  {:<10} {:<10} {:>11.6} {:>5.0}px  {bucket}",
                name_of(nr.glyph),
                name_of(nr.rival),
                nr.dist,
                nr.size
            );
        }

        // ---- the five named pairs, wherever they land ----
        let named_pairs: [(u32, u32, &str); 5] = [
            (0x00D8, 0x2300, "O-with-stroke / diameter sign"),
            ('l' as u32, '|' as u32, "l / pipe"),
            ('.' as u32, 0x00B7, "period / middle dot"),
            (0x00A7, '8' as u32, "section / 8"),
            ('0' as u32, 'O' as u32, "digit zero / capital O"),
        ];
        println!("\n=== the five named pairs ===");
        for &(cp_a, cp_b, label) in &named_pairs {
            let ia = idx_of(cp_a);
            let ib = idx_of(cp_b);
            let rank = ranked
                .iter()
                .position(|pm| (pm.i == ia && pm.j == ib) || (pm.i == ib && pm.j == ia))
                .map(|p| p + 1)
                .expect("named pair must exist among the computed pairs");
            let pm = ranked[rank - 1];
            let bucket = if pm.same_bucket { "same bucket (reaches distance stage)" } else { "different buckets (pruned, costs nothing)" };
            println!(
                "  {label} ({} U+{:04X} / {} U+{:04X}): rank {rank} of {}, min dist {:.6} at {:.0}px, {bucket}",
                name_of(ia),
                cp_a,
                name_of(ib),
                cp_b,
                ranked.len(),
                pm.dist,
                pm.size
            );
            print!("    per-size:");
            for (s_idx, &size) in SIZES.iter().enumerate() {
                let d = l2(&features[s_idx][ia], &features[s_idx][ib]);
                print!("  {size:.0}px={d:.6}");
            }
            println!();
        }

        // ---- '.' vs '·': do they differ ONLY by baseline offset, and does
        // the feature vector actually carry that separation? ----
        // charset.tsv files '.' as baseline_class "low" and '·' as "above"
        // — different classes, so if this is right they should differ in
        // GEOMETRY dims 105/106 (height above baseline, depth below) and
        // nowhere else, the same shape as the apostrophe/comma case this
        // extractor's own unit test (`same_shape_at_different_heights_separates_only_on_geometry`)
        // exercises directly.
        let period = idx_of('.' as u32);
        let middot = idx_of(0x00B7);
        println!("\n=== '.' vs '·': is the separation carried by the feature vector? ===");
        for (s_idx, &size) in SIZES.iter().enumerate() {
            let a = &features[s_idx][period];
            let b = &features[s_idx][middot];
            let identical = a == b;
            let non_geometry_identical = a[..103] == b[..103];
            let d = l2(a, b);
            println!(
                "  {size:>5.0}px  dist={d:.6}  full_vector_identical={identical}  dims_0..103_identical={non_geometry_identical}  geometry a={:?} b={:?}",
                &a[103..107],
                &b[103..107]
            );
        }
    }

    /// The face-wide **render-size collision floor**: at every integer
    /// `px_per_em` from 16 to 40 inclusive, over every same-hole-count-bucket
    /// pair of distinct glyphs — the same `n*(n-1)/2` pairs
    /// [`pairwise_margin_report`] enumerates, restricted here to the subset
    /// that shares a hole-count bucket at that size, because a
    /// different-bucket collision is pruned before the distance stage and
    /// costs the matcher nothing (see that function's "Hole-count prune
    /// survival" section) — records the count and identity of every pair at
    /// L2 exactly `0.0`, plus the minimum non-zero L2 and its pair.
    ///
    /// # Metric
    ///
    /// Unweighted L2 over the raw 107-dim [`extract`] output, the same proxy
    /// [`pairwise_margin_report`]/[`l2`] use and for the same reason: no
    /// `feature_norm` table and no `match.rs` exist yet to define the
    /// shipped matcher's weighted-L2 (see that function's doc comment).
    ///
    /// # What this produces, and what it does not
    ///
    /// This is a derived *input* to the minimum-DPI decision
    /// `docs/ARCHITECTURE.md`'s "Feature survival is gated at the smallest
    /// supported render" entry (2026-09-18) leaves open — not the decision
    /// itself, and nothing here asserts a threshold (`CLAUDE.md` rule 1: a
    /// guessed constant is not a number).
    ///
    /// The headline is the largest size in `[16, 40]` at which any
    /// same-bucket pair collides, plus one: the smallest `S` such that every
    /// size from `S` through `40` is collision-free *within this sweep*.
    /// Collision is **not monotonic in render size** — two known pairs
    /// collide, separate, and re-collide as size increases — so this sweeps
    /// every integer size exhaustively; a bisection or "first passing size"
    /// search would silently miss a higher colliding size and understate the
    /// floor. The headline is bounded by the top of the sweep: "no collision
    /// from `S` to 40px" says nothing about any size above 40px.
    ///
    /// # Verification, not just distance
    ///
    /// Every same-bucket zero-distance pair found is re-rendered and its raw
    /// rasters compared byte-for-byte. A zero L2 must come from a
    /// bit-identical raster; identical *features* over *different* rasters
    /// would be a feature-extractor bug, a far more serious finding than a
    /// render-size collision, and this test stops loudly (via `assert!`) if
    /// it ever finds one, rather than silently reporting a false floor.
    ///
    /// `cargo test -p ocrcer-build -- --ignored --nocapture render_size_collision_floor_report`
    #[test]
    #[ignore = "report, not a gate"]
    fn render_size_collision_floor_report() {
        const MIN_SIZE: i32 = 16;
        const MAX_SIZE: i32 = 40;
        const SECTION_SIGN: u32 = 0x00A7;

        // symbols.rs may be under active redraw by another agent right now;
        // report the mtime actually observed rather than assume.
        let symbols_path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/face/glyphs/symbols.rs");
        let mtime_before = std::fs::metadata(symbols_path).and_then(|m| m.modified()).ok();

        let all_glyphs = glyphs();
        let n = all_glyphs.len();
        let name_of = |idx: usize| char_name(all_glyphs[idx].codepoint);

        struct SizeReport {
            size: i32,
            zero_pairs: Vec<(usize, usize)>,
            different_bucket_zero_pairs: usize,
            min_nonzero: Option<(f64, usize, usize)>,
        }

        let mut reports: Vec<SizeReport> = Vec::with_capacity((MAX_SIZE - MIN_SIZE + 1) as usize);

        for size in MIN_SIZE..=MAX_SIZE {
            let features: Vec<[f32; FEATURE_DIMS]> =
                all_glyphs.iter().map(|g| extract_for(g, size as f32)).collect();

            let mut zero_pairs = Vec::new();
            let mut different_bucket_zero_pairs = 0usize;
            let mut min_nonzero: Option<(f64, usize, usize)> = None;

            for i in 0..n {
                for j in (i + 1)..n {
                    let same_bucket = (features[i][HOLE_COUNT] as i32) == (features[j][HOLE_COUNT] as i32);
                    let d = l2(&features[i], &features[j]);
                    if !same_bucket {
                        if d == 0.0 {
                            different_bucket_zero_pairs += 1;
                        }
                        continue;
                    }
                    if d == 0.0 {
                        zero_pairs.push((i, j));
                    } else if min_nonzero.is_none_or(|(best, _, _)| d < best) {
                        min_nonzero = Some((d, i, j));
                    }
                }
            }
            reports.push(SizeReport { size, zero_pairs, different_bucket_zero_pairs, min_nonzero });
        }

        // ---- verify every same-bucket zero-distance pair by rendering, not
        // only by distance (see doc comment above) ----
        for r in &reports {
            for &(i, j) in &r.zero_pairs {
                let ra = render(&all_glyphs[i], r.size as f32)
                    .unwrap_or_else(|| panic!("{} produced no ink at {}px", name_of(i), r.size));
                let rb = render(&all_glyphs[j], r.size as f32)
                    .unwrap_or_else(|| panic!("{} produced no ink at {}px", name_of(j), r.size));
                let ink_equal = ra.ink == rb.ink;
                let rasters_identical = ra.width == rb.width && ra.height == rb.height && ink_equal;
                assert!(
                    rasters_identical,
                    "SERIOUS: {} vs {} at {}px reports L2==0.0 (identical FEATURES) but the \
                     rasters differ ({}x{} vs {}x{}, ink equal={ink_equal}) — this is a \
                     feature-extractor bug, not a render-size collision, and needs \
                     `ocrcer-architect` before this report can be trusted",
                    name_of(i),
                    name_of(j),
                    r.size,
                    ra.width,
                    ra.height,
                    rb.width,
                    rb.height
                );
            }
        }

        println!(
            "\n=== render_size_collision_floor_report: same-bucket L2 collisions, {MIN_SIZE}..={MAX_SIZE}px ==="
        );
        println!("metric: unweighted L2 over raw extract() output (no feature_norm/match.rs yet)");
        println!("{:<8} {:>6} {:>7} {:>14}  min_nonzero pair", "size", "zeros", "diffbkt", "min_nonzero");
        for r in &reports {
            match &r.min_nonzero {
                Some((d, i, j)) => println!(
                    "  {:>4}px {:>6} {:>7} {:>14.6}  {} / {}",
                    r.size,
                    r.zero_pairs.len(),
                    r.different_bucket_zero_pairs,
                    d,
                    name_of(*i),
                    name_of(*j)
                ),
                None => println!(
                    "  {:>4}px {:>6} {:>7} {:>14}  (every same-bucket pair collided at this size)",
                    r.size, r.zero_pairs.len(), r.different_bucket_zero_pairs, "n/a"
                ),
            }
        }
        println!(
            "(\"diffbkt\" = different-hole-count-bucket pairs also at L2==0.0 at that size; these \
             are pruned before the matcher's distance stage per ARCHITECTURE.md section 4.1 step \
             1 and do NOT count toward the collision floor — shown only for transparency)"
        );

        println!(
            "\n=== every same-bucket colliding pair, and every size in [{MIN_SIZE},{MAX_SIZE}] it collides at ==="
        );
        let mut pair_sizes: std::collections::BTreeMap<(usize, usize), Vec<i32>> = std::collections::BTreeMap::new();
        for r in &reports {
            for &(i, j) in &r.zero_pairs {
                pair_sizes.entry((i, j)).or_default().push(r.size);
            }
        }
        if pair_sizes.is_empty() {
            println!("  (none)");
        }
        for (&(i, j), sizes) in &pair_sizes {
            let is_section_sign = all_glyphs[i].codepoint == SECTION_SIGN || all_glyphs[j].codepoint == SECTION_SIGN;
            let flag = if is_section_sign {
                "  <-- SECTION SIGN: symbols.rs may be under redraw, re-run after it lands"
            } else {
                ""
            };
            println!("  {} / {}: collides at {sizes:?}{flag}", name_of(i), name_of(j));
        }

        // ---- headline ----
        let largest_colliding_size = reports.iter().filter(|r| !r.zero_pairs.is_empty()).map(|r| r.size).max();
        match largest_colliding_size {
            Some(s) => println!(
                "\n=== HEADLINE: largest same-bucket colliding size in [{MIN_SIZE},{MAX_SIZE}]px is \
                 {s}px -> no collision found from {}px through {MAX_SIZE}px in this sweep ===\n\
                 This is bounded by the top of the sweep: it says nothing about any size above {MAX_SIZE}px.",
                s + 1
            ),
            None => println!(
                "\n=== HEADLINE: no same-bucket collision anywhere in [{MIN_SIZE},{MAX_SIZE}]px in this sweep ===\n\
                 This is bounded by the swept range: it says nothing about sizes outside [{MIN_SIZE},{MAX_SIZE}]."
            ),
        }

        // ---- verify the two known pairs by rendering, not only by
        // distance: print ASCII rasters at 16px, where Ken's own by-hand
        // measurement found both COLLIDE. ----
        println!("\n=== ASCII verification at 16px: i/idiaeresis and period/ellipsis ===");
        let print_raster = |idx: usize, size: f32| {
            let r = render(&all_glyphs[idx], size).unwrap();
            println!("  {} (U+{:04X}), {}x{}:", name_of(idx), all_glyphs[idx].codepoint, r.width, r.height);
            for row in 0..r.height as usize {
                let mut line = String::from("    ");
                for col in 0..r.width as usize {
                    line.push(if r.ink[row * r.width as usize + col] != 0 { '#' } else { '.' });
                }
                println!("{line}");
            }
        };
        let i_idx = all_glyphs.iter().position(|g| g.codepoint == 'i' as u32).unwrap();
        let idiaeresis_idx = all_glyphs.iter().position(|g| g.codepoint == 0x00EF).unwrap();
        let period_idx = all_glyphs.iter().position(|g| g.codepoint == '.' as u32).unwrap();
        let ellipsis_idx = all_glyphs.iter().position(|g| g.codepoint == 0x2026).unwrap();
        print_raster(i_idx, 16.0);
        print_raster(idiaeresis_idx, 16.0);
        print_raster(period_idx, 16.0);
        print_raster(ellipsis_idx, 16.0);

        // ---- section-sign redraw status ----
        let mtime_after = std::fs::metadata(symbols_path).and_then(|m| m.modified()).ok();
        println!("\n=== symbols.rs redraw status ===");
        println!("  symbols.rs mtime before sweep: {mtime_before:?}");
        println!("  symbols.rs mtime after sweep:  {mtime_after:?}");
        if mtime_before != mtime_after {
            println!(
                "  WARNING: symbols.rs changed DURING this sweep — the numbers above are a MIX of \
                 pre- and post-redraw and must be re-run"
            );
        }
    }

    // ---- charset cross-checks: reads model/charset.tsv, does not write it ----

    struct CharsetRow {
        category: String,
        baseline_class: String,
        aspect_min: f32,
        aspect_max: f32,
    }

    /// `codepoint -> row`, parsed from `model/charset.tsv` at test time so
    /// these checks track the authored table rather than a second, private
    /// copy of it.
    fn charset_rows() -> std::collections::HashMap<u32, CharsetRow> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../model/charset.tsv");
        let text = std::fs::read_to_string(path).expect("model/charset.tsv must be readable");
        text.lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                let cols: Vec<&str> = line.split('\t').collect();
                let codepoint = u32::from_str_radix(cols[1].trim_start_matches("U+"), 16)
                    .unwrap_or_else(|_| panic!("bad codepoint column in charset.tsv: {}", cols[1]));
                let row = CharsetRow {
                    category: cols[3].to_string(),
                    baseline_class: cols[4].to_string(),
                    aspect_min: cols[5].parse().unwrap(),
                    aspect_max: cols[6].parse().unwrap(),
                };
                (codepoint, row)
            })
            .collect()
    }

    /// A glyph's centreline extent in design units, `(lowest, highest)`,
    /// taken from the same flattening the rasteriser uses rather than from
    /// the authored control points — a quadratic's extreme is bounded by
    /// its control polygon but is not generally attained at a control
    /// point, so reading the literals would overstate a curved glyph's
    /// reach by a few units and quietly weaken every check below.
    fn centreline_extent(glyph: &Glyph) -> (f64, f64) {
        let segments = flatten_glyph(glyph, 1.0);
        assert!(!segments.is_empty(), "{} flattens to nothing", char_name(glyph.codepoint));
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for &(_, y0, _, y1) in &segments {
            lo = lo.min(y0).min(y1);
            hi = hi.max(y0).max(y1);
        }
        (lo, hi)
    }

    /// The rasteriser puts ink where the stroke data says, for every glyph
    /// in the face, at every test size.
    ///
    /// Ink reaches half a pen past the centreline in each direction, so the
    /// predicted top and bottom follow from the flattened extent and
    /// `STROKE` alone. That makes this the one height check in this file
    /// with no authored list and no authored constant in it: it cannot be
    /// satisfied by adjusting a letterform, only by the rasteriser and the
    /// stroke data agreeing. It is what catches a crop, grid-anchoring or
    /// scale defect — and a grid-anchoring defect is not hypothetical here,
    /// it is what made round glyphs vertically lopsided and pen dots taller
    /// than they were wide.
    ///
    /// It deliberately says nothing about whether a glyph was drawn at the
    /// *intended* height; that is what [`stroke_data_agrees_with_its_baseline_class`]
    /// is for, and the two questions need separate answers.
    #[test]
    fn rendered_ink_matches_the_stroke_data_extent() {
        for glyph in glyphs() {
            let (lo, hi) = centreline_extent(&glyph);
            let pen = FACE_STROKE as f64 / 2.0;
            for &size in &TEST_SIZES {
                let scale = size as f64 / FACE_UPM as f64;
                let r = render(&glyph, size)
                    .unwrap_or_else(|| panic!("{} produced no ink at {size}px", char_name(glyph.codepoint)));
                let want_above = (hi + pen) * scale;
                let want_below = -(lo - pen) * scale;
                let got_below = r.height as f64 - r.baseline_dy as f64;
                assert!(
                    (r.baseline_dy as f64 - want_above).abs() <= 1.0,
                    "{} at {size}px: ink rises {:.2}px above the baseline, stroke data says {want_above:.2}px",
                    char_name(glyph.codepoint),
                    r.baseline_dy
                );
                assert!(
                    (got_below - want_below).abs() <= 1.0,
                    "{} at {size}px: ink falls {got_below:.2}px below the baseline, stroke data says {want_below:.2}px",
                    char_name(glyph.codepoint)
                );
            }
        }
    }

    /// Every glyph sits where its `charset.tsv` `baseline_class` says it
    /// does.
    ///
    /// `baseline_class` is a *pruning bucket*, not a metric line
    /// (`ARCHITECTURE.md` section 4.1 step 2 prunes by it), and that
    /// distinction is the whole reason this test is shaped the way it is. An
    /// earlier version asserted that all 98 `ascender` glyphs rendered to
    /// the same height within a pixel. That is false about typography, not
    /// about this face: an accented capital puts ink above the cap line, ISO
    /// 3098 draws `t` short of it, and the dot on `i` sits lower still. It
    /// went red the moment lowercase landed, and its first effect was to
    /// talk an author into raising `i`'s dot to the cap line to make it
    /// green.
    ///
    /// What the six buckets actually distinguish, read off their membership,
    /// is two independent bits: whether the glyph drops below the baseline,
    /// and where it reaches relative to the x-line. Each arm below asserts
    /// exactly that and nothing more, so the test cannot be satisfied by
    /// moving a letterform a few units. The tight cap-line check that this
    /// arm deliberately does not make lives in
    /// [`capitals_and_digits_are_drawn_baseline_to_cap`], where it is true.
    ///
    /// Nothing here is a tuned threshold: every bound is `CAP`, `X_HEIGHT`,
    /// `DESCENDER` or the baseline itself.
    ///
    /// **If a composed accented glyph fails here, the charset row is wrong,
    /// not the glyph.** `à` is filed `xheight` though its acute plainly rises
    /// above the x-line, and a segmented image of it will measure as
    /// something taller — so that row will prune the right prototype away.
    /// The fix is to reclassify the row. Flattening the accent to make this
    /// test green would be the fifth time on this project a number deformed
    /// a letterform.
    #[test]
    fn stroke_data_agrees_with_its_baseline_class() {
        let charset = charset_rows();
        let cap = CAP as f64;
        let xh = X_HEIGHT as f64;
        let desc = DESCENDER as f64;
        // A mark may reach a metric line with its ink rather than with its
        // centreline: a stem's round cap overshoots the line it is authored
        // to by half a pen, and a dot, whose ink *is* the whole mark, is
        // authored half a pen inside the line instead. Both are correct, so
        // the arms that say "reaches this line" allow either. Derived from
        // the face's own STROKE, not tuned.
        let pen = FACE_STROKE as f64 / 2.0;
        for glyph in glyphs() {
            let (lo, hi) = centreline_extent(&glyph);
            let name = char_name(glyph.codepoint);
            let class = charset[&glyph.codepoint].baseline_class.clone();
            match class.as_str() {
                // Never reaches the x-line. `, . _ …`, and `,` descends,
                // which is why nothing is said here about the baseline.
                "low" => assert!(
                    hi < xh,
                    "{name} is class low but its stroke data tops out at {hi}, at or above X_HEIGHT {xh}"
                ),
                // Sits on or above the baseline, no higher than the cap
                // line. The relations, the dashes, the quotes, the degree
                // sign.
                "above" => assert!(
                    lo >= 0.0 && hi <= cap + 1.0,
                    "{name} is class above but its stroke data spans {lo}..{hi}; wanted on or above the baseline and no higher than CAP {cap}"
                ),
                // Body height: reaches the x-line, stays on the baseline.
                "xheight" => assert!(
                    lo >= 0.0 && hi >= xh - pen,
                    "{name} is class xheight but its stroke data spans {lo}..{hi}; wanted on the baseline and up to X_HEIGHT {xh}"
                ),
                // Rises past the x-line without descending. Most reach the
                // cap line; `t` and the dot on `i` legitimately stop short,
                // so this is a floor at the x-line, not an equality at the
                // cap line.
                "ascender" => assert!(
                    lo >= 0.0 && hi > xh,
                    "{name} is class ascender but its stroke data spans {lo}..{hi}; wanted on the baseline and above X_HEIGHT {xh}"
                ),
                // Descends, and never deeper than the face's declared
                // descender depth.
                "descender" => assert!(
                    lo < 0.0 && lo >= desc - 1.0,
                    "{name} is class descender but its stroke data bottoms out at {lo}; wanted below the baseline and no deeper than DESCENDER {desc}"
                ),
                // Spans the whole body: below the baseline and up to the cap
                // line. The brackets, `#`, `$`, `√`.
                "full" => assert!(
                    lo < 0.0 && hi >= cap - pen,
                    "{name} is class full but its stroke data spans {lo}..{hi}; wanted below the baseline and up to CAP {cap}"
                ),
                other => panic!("{name} has unknown baseline_class {other:?} in charset.tsv"),
            }
        }
    }

    /// Every unaccented capital and every digit is drawn from the baseline
    /// to the cap line exactly.
    ///
    /// This is the tight height check the face can actually support, and it
    /// is where a capital drawn to the wrong height gets caught —
    /// [`stroke_data_agrees_with_its_baseline_class`] can only say "above
    /// the x-line", because `t` and `i` share that bucket. Scoped by the
    /// charset's `category` column to the two groups where a single height
    /// is a design fact rather than a coincidence, and restricted to ASCII
    /// so that accented capitals, whose marks sit above the cap line, are
    /// not swept in.
    ///
    /// `M` and `W` both failed a check of this shape when it was run by
    /// hand: their middle vertices stopped short of the metric line, which
    /// gives a vertical projection profile unlike any real face's and wastes
    /// the prototype.
    ///
    /// A cross-glyph version of this — group glyphs by their stroke-data
    /// extent, assert the group renders to one height — was tried and
    /// removed. [`rendered_ink_matches_the_stroke_data_extent`] already pins
    /// each edge of each glyph to its design-unit prediction within a pixel,
    /// so two glyphs drawn to the same extent can still differ by two, and
    /// the group check fires on that stacking rather than on any design
    /// error. It failed on `!` against `?` — both individually correct, both
    /// reaching `y=700`, one rendering 46px tall and the other 48px.
    #[test]
    fn capitals_and_digits_are_drawn_baseline_to_cap() {
        let charset = charset_rows();
        let cap = CAP as f64;
        for glyph in glyphs() {
            if glyph.codepoint >= 0x80 {
                continue;
            }
            let category = charset[&glyph.codepoint].category.clone();
            if category != "upper" && category != "digit" {
                continue;
            }
            let (lo, hi) = centreline_extent(&glyph);
            let name = char_name(glyph.codepoint);
            assert!(
                (hi - cap).abs() <= 1.0,
                "{name} is a {category} but its stroke data tops out at {hi}, not CAP {cap}"
            );
            assert!(
                lo.abs() <= 1.0,
                "{name} is a {category} but its stroke data bottoms out at {lo}, not the baseline"
            );
        }
    }

    #[test]
    fn round_glyphs_are_vertically_symmetric() {
        let by_cp = |cp: u32| glyphs().into_iter().find(|g| g.codepoint == cp).unwrap();
        for cp in ['0' as u32, 'O' as u32, '8' as u32] {
            let glyph = by_cp(cp);
            let r = render(&glyph, 64.0).unwrap();
            let w = r.width as usize;
            let h = r.height as usize;
            let row_width = |row: usize| (0..w).filter(|&c| r.ink[row * w + c] != 0).count() as i64;
            for i in 0..h {
                let top = row_width(i);
                let bottom = row_width(h - 1 - i);
                assert!(
                    (top - bottom).abs() <= 1,
                    "{}: row {i} from top has {top} ink px, row {i} from bottom has {bottom} (h={h}); should be symmetric about the horizontal midline",
                    char_name(cp)
                );
            }
        }
    }

    /// Counts background regions that do not touch the bitmap border,
    /// 4-connected, with no clamp — the number `feature::extract`'s
    /// `HOLE_COUNT` dimension clamps to `0..=2`. Mirrors that private
    /// function's logic (border-touch scan over the four edges) against the
    /// public `components::label`, not a second copy of the extractor.
    fn unclamped_hole_count(r: &Raster) -> u32 {
        let w = r.width as usize;
        let h = r.height as usize;
        let bg: Vec<u8> = r.ink.iter().map(|&v| if v == 0 { 1 } else { 0 }).collect();
        let (labels, count) = label(&bg, r.width, r.height, Connectivity::Four);
        if count == 0 {
            return 0;
        }
        let mut touches_border = vec![false; count as usize + 1];
        let mut mark = |idx: usize| {
            let l = labels[idx];
            if l != 0 {
                touches_border[l as usize] = true;
            }
        };
        for x in 0..w {
            mark(x);
            mark((h - 1) * w + x);
        }
        for y in 0..h {
            mark(y * w);
            mark(y * w + w - 1);
        }
        (1..=count).filter(|&l| !touches_border[l as usize]).count() as u32
    }

    #[test]
    fn unclamped_hole_count_matches_the_face_design() {
        let expected: &[(u32, u32)] = &[
            ('4' as u32, 0),
            ('5' as u32, 0),
            ('7' as u32, 0),
            ('0' as u32, 1),
            ('O' as u32, 1),
            ('6' as u32, 1),
            ('9' as u32, 1),
            ('8' as u32, 2),
            (0x00D8, 2), // Oslash
            (0x2300, 2), // diameter sign
            ('A' as u32, 1),
            ('B' as u32, 2),
            ('C' as u32, 0),
            ('D' as u32, 1),
            ('E' as u32, 0),
            ('F' as u32, 0),
            ('G' as u32, 0),
            ('H' as u32, 0),
            ('J' as u32, 0),
            ('K' as u32, 0),
            ('L' as u32, 0),
            ('M' as u32, 0),
            ('N' as u32, 0),
            ('P' as u32, 1),
            ('Q' as u32, 1),
            ('R' as u32, 1),
            ('S' as u32, 0),
            ('T' as u32, 0),
            ('U' as u32, 0),
            ('V' as u32, 0),
            ('W' as u32, 0),
            ('X' as u32, 0),
            ('Y' as u32, 0),
            ('Z' as u32, 0),
            (0x00C6, 1), // AE
            ('a' as u32, 1),
            ('b' as u32, 1),
            ('c' as u32, 0),
            ('d' as u32, 1),
            ('e' as u32, 1),
            ('f' as u32, 0),
            ('g' as u32, 1),
            ('h' as u32, 0),
            ('i' as u32, 0),
            ('j' as u32, 0),
            ('k' as u32, 0),
            ('m' as u32, 0),
            ('n' as u32, 0),
            ('o' as u32, 1),
            ('p' as u32, 1),
            ('q' as u32, 1),
            ('r' as u32, 0),
            ('s' as u32, 0),
            ('t' as u32, 0),
            ('u' as u32, 0),
            ('v' as u32, 0),
            ('w' as u32, 0),
            ('x' as u32, 0),
            ('y' as u32, 0),
            ('z' as u32, 0),
            (0x00DF, 2), // sharp s
            (0x00E6, 2), // ae
            // punctuation — every mark in the slice is open or solid;
            // nothing in it closes a counter.
            ('!' as u32, 0),
            ('"' as u32, 0),
            ('\'' as u32, 0),
            ('(' as u32, 0),
            (')' as u32, 0),
            (',' as u32, 0),
            ('-' as u32, 0),
            ('/' as u32, 0),
            (':' as u32, 0),
            (';' as u32, 0),
            ('?' as u32, 0),
            ('[' as u32, 0),
            ('\\' as u32, 0),
            (']' as u32, 0),
            ('_' as u32, 0),
            ('`' as u32, 0),
            ('{' as u32, 0),
            ('}' as u32, 0),
            (0x00B7, 0), // middle dot
            (0x2022, 0), // bullet — two nested loops, solid, no annulus
            (0x2013, 0), // en dash
            (0x2014, 0), // em dash
            (0x2018, 0), // left single quote
            (0x2019, 0), // right single quote
            (0x201C, 0), // left double quote
            (0x201D, 0), // right double quote
            (0x2026, 0), // ellipsis
            // accented Latin-1 — a composed form inherits its base's hole
            // count: none of grave, acute, circumflex, tilde, diaeresis or
            // cedilla closes a loop. The ring on A-ring does, so those two
            // gain one.
            (0x00C7, 0), // Ccedilla
            (0x00C8, 0), // Egrave
            (0x00C9, 0), // Eacute
            (0x00CA, 0), // Ecircumflex
            (0x00CB, 0), // Ediaeresis
            (0x00CC, 0), // Igrave
            (0x00CD, 0), // Iacute
            (0x00CE, 0), // Icircumflex
            (0x00CF, 0), // Idiaeresis
            (0x00D1, 0), // Ntilde
            (0x00D9, 0), // Ugrave
            (0x00DA, 0), // Uacute
            (0x00DB, 0), // Ucircumflex
            (0x00DC, 0), // Udiaeresis
            (0x00DD, 0), // Yacute
            (0x00E7, 0), // ccedilla
            (0x00EC, 0), // igrave
            (0x00ED, 0), // iacute
            (0x00EE, 0), // icircumflex
            (0x00EF, 0), // idiaeresis
            (0x00F1, 0), // ntilde
            (0x00F9, 0), // ugrave
            (0x00FA, 0), // uacute
            (0x00FB, 0), // ucircumflex
            (0x00FC, 0), // udiaeresis
            (0x00FD, 0), // yacute
            (0x00FF, 0), // ydiaeresis
            (0x00C0, 1), // Agrave
            (0x00C1, 1), // Aacute
            (0x00C2, 1), // Acircumflex
            (0x00C3, 1), // Atilde
            (0x00C4, 1), // Adiaeresis
            (0x00D2, 1), // Ograve
            (0x00D3, 1), // Oacute
            (0x00D4, 1), // Ocircumflex
            (0x00D5, 1), // Otilde
            (0x00D6, 1), // Odiaeresis
            (0x00E0, 1), // agrave
            (0x00E1, 1), // aacute
            (0x00E2, 1), // acircumflex
            (0x00E3, 1), // atilde
            (0x00E4, 1), // adiaeresis
            (0x00E8, 1), // egrave
            (0x00E9, 1), // eacute
            (0x00EA, 1), // ecircumflex
            (0x00EB, 1), // ediaeresis
            (0x00F2, 1), // ograve
            (0x00F3, 1), // oacute
            (0x00F4, 1), // ocircumflex
            (0x00F5, 1), // otilde
            (0x00F6, 1), // odiaeresis
            (0x00C5, 2), // Aring
            (0x00E5, 2), // aring
            // symbols — measured at 64px, unclamped (border-touch 4-connected
            // region count, not the clamped `HOLE_COUNT` feature dimension).
            (0x2300, 2), // diameter sign
            ('#' as u32, 1), // hash
            ('%' as u32, 2), // percent
            (0x2030, 3), // per mille — 3 discs, none clamped here
            // Ampersand: 2 real regions (ring counter + upper-loop counter)
            // at 24px/48px, but 3 at 64px — a 4-connectivity artifact, the
            // same class as the accepted Å/å 16px finding (a diagonal-only
            // background touch reads as a separate region under strict
            // 4-connectivity), not a third intentional counter. Reported as
            // measured, not tuned away.
            ('&' as u32, 3),
            ('*' as u32, 0), // asterisk
            ('@' as u32, 2), // at sign
            ('^' as u32, 0), // caret
            ('|' as u32, 0), // pipe
            ('~' as u32, 0), // tilde
            (0x00B0, 1), // degree
            (0x00B5, 0), // micro
            (0x00BC, 0), // quarter
            (0x00BD, 0), // half
            (0x00BE, 0), // three quarters
            (0x00A7, 2), // section
            (0x00B6, 1), // pilcrow
            (0x2020, 0), // dagger
            (0x2021, 0), // double dagger
            (0x00A9, 1), // copyright
            (0x00AE, 2), // registered
            (0x2122, 0), // trademark
            (0x03A9, 0), // Omega
            ('+' as u32, 0),
            ('<' as u32, 0),
            ('=' as u32, 0),
            ('>' as u32, 0),
            (0x00B1, 0), // plus-minus
            (0x00D7, 0), // multiply
            (0x00F7, 0), // divide
            (0x221A, 0), // radical
            (0x2264, 0), // less-equal
            (0x2265, 0), // greater-equal
            (0x2248, 0), // approx
            (0x2260, 0), // not-equal
            ('$' as u32, 2), // dollar
            (0x00A2, 1), // cent
            (0x00A3, 0), // pound
            (0x20AC, 0), // euro
            (0x00A5, 0), // yen
        ];
        let by_cp = |cp: u32| glyphs().into_iter().find(|g| g.codepoint == cp).unwrap();
        for &(cp, want) in expected {
            let r = render(&by_cp(cp), 64.0).unwrap();
            let got = unclamped_hole_count(&r);
            assert_eq!(
                got, want,
                "{}: unclamped region count is {got}, expected {want} (this is measured directly, not the clamped extractor dimension)",
                char_name(cp)
            );
        }
    }

    /// Report only, per the coordinator: prints rendered width/CAP next to
    /// the charset's authored `aspect_min`/`aspect_max` and flags glyphs
    /// outside that band. Does not assert — an out-of-band glyph may mean
    /// the band is wrong, and that call belongs upstairs, not to this test.
    /// Run with `cargo test -p ocrcer-build -- --ignored --nocapture aspect_report`.
    #[test]
    #[ignore = "prints a visual report; not an assertion"]
    fn aspect_report() {
        let charset = charset_rows();
        let cap_px = CAP as f32 * 64.0 / FACE_UPM as f32;
        println!("\nglyph  width/CAP  band            in-band?");
        for glyph in glyphs() {
            let r = render(&glyph, 64.0).unwrap();
            let aspect = r.width as f32 / cap_px;
            let row = &charset[&glyph.codepoint];
            let in_band = aspect >= row.aspect_min && aspect <= row.aspect_max;
            println!(
                "{:<6} {:<10.3} {:.2}-{:.2}       {}",
                char_name(glyph.codepoint),
                aspect,
                row.aspect_min,
                row.aspect_max,
                if in_band { "yes" } else { "NO" }
            );
        }
    }

    // ---- visual report: `cargo test -p ocrcer-build -- --ignored --nocapture print_ascii_art` ----

    #[test]
    #[ignore = "prints a visual report; not an assertion"]
    fn print_ascii_art() {
        for glyph in glyphs() {
            let r = render(&glyph, 48.0).unwrap();
            println!("\n{} (U+{:04X}), {}x{}, baseline_dy={:.1}", char_name(glyph.codepoint), glyph.codepoint, r.width, r.height, r.baseline_dy);
            for row in 0..r.height as usize {
                let mut line = String::new();
                for col in 0..r.width as usize {
                    line.push(if r.ink[row * r.width as usize + col] != 0 { '#' } else { '.' });
                }
                println!("{line}");
            }
        }
    }
}
