//! Sets authored text with a real face and returns a page image plus the
//! ground truth of what is on it.
//!
//! # Contract
//!
//! [`render`] is deterministic: the same face bytes, the same text and the
//! same `px_per_em` give byte-identical pixels on any machine. It uses
//! [`crate::outline`]'s rasteriser — the one the prototype bank is built
//! with — so a glyph on a page here and the same glyph in the bank are the
//! same bitmap, and a disagreement between them cannot come from two
//! rasterisers having different opinions.
//!
//! Pixels are 8-bit grey, `0` ink and `255` paper, row-major, one byte per
//! pixel. That is what `ocrs` takes and what a scanned page reduces to.
//!
//! # Coverage, and why a two-valued page was a bug
//!
//! Ink is laid down as **coverage** -- the fraction of each pixel inside the
//! outline, from the same subgrid the bank's rasteriser thresholds -- and the
//! page is left grey for the engine's own binarizer to threshold. A page
//! written at the majority threshold instead loses every pixel a fine stem
//! covers less than half of, so a Light weight of a Condensed face at 14
//! px/em arrives with dotted stems and single-pixel capitals. That is not a
//! hard corpus, it is an *undeclared degradation*: no scanner, PDF rasteriser
//! or spec-conformant monochrome rasteriser emits it, because the TrueType
//! specification requires dropout control and scanning pipelines emit
//! coverage in the first place. It also meant the binarizer -- a whole
//! pipeline stage -- was never exercised by the corpus at all.
//!
//! Truth boxes follow from that: each glyph's box is the tight box of
//! **binarized** ink inside that glyph's own coverage footprint, so an
//! oracle box and the component an end-to-end run finds are the same box,
//! and a glyph that binarization erases is reported rather than asserted.
//!
//! # What this is not
//!
//! Not a typesetter. No kerning, no ligatures, no shaping, no justification,
//! no hyphenation. A page from here is a test corpus with known ground
//! truth, not a document. Antialiased edges on clean paper are still *unlike*
//! a scan -- no noise, no skew, no show-through, no compression -- and an
//! engine designed for photographed and scanned pages is not being shown its
//! best case. Any comparison run on these pages says so.

use crate::ttf_load::Face;

/// Where one character's ink landed, in page pixels, y down.
///
/// Absent for a character that put no ink on the page — a space, or a glyph
/// the face does not have. The text is still recoverable from
/// [`Page::lines`]; this list is the *ink*, and a caller doing oracle
/// segmentation wants exactly the boxes that exist.
#[derive(Clone, Debug)]
pub struct PageGlyph {
    pub ch: char,
    pub line: usize,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Baseline of the line this glyph sits on, in page pixels.
    pub baseline: u32,
    /// x-height of the face at this size, in pixels, as the face declares or
    /// measures it. The feature extractor takes this as an input.
    pub x_height: f32,
    /// 8-connected components of this glyph's **coverage** footprint.
    ///
    /// This is the quantity the generator asserts on, and it is coverage and
    /// not ink deliberately. The invariant being checked is about the
    /// *rasteriser* -- a single closed contour cannot draw two separate marks,
    /// so if it did, the stroke fell between samples. Thresholding is a later
    /// and separate stage, and a glyph that the binarizer breaks is a hard
    /// page rather than a broken one.
    ///
    /// The rule is one-directional: the converse says nothing, because `O` is
    /// two contours and one component and `8` is three and one.
    pub components: u32,
    /// 8-connected components of *binarized* ink inside the same footprint.
    ///
    /// Not asserted on, counted and reported. A serif `E` whose middle arm
    /// separates from its stem at 16 px/em is a real difficulty a real scan
    /// has, and refusing the page would quietly delete the hardest faces at
    /// the sizes the bank is built for. The count is the honest middle: the
    /// corpus keeps the page and says how many marks came apart.
    pub ink_components: u32,
}

/// A rendered page and everything known to be true about it.
pub struct Page {
    pub width: u32,
    pub height: u32,
    /// `width * height` bytes, `0` ink, `255` paper.
    pub grey: Vec<u8>,
    /// The text that was set, one entry per line, exactly as given.
    pub lines: Vec<String>,
    pub glyphs: Vec<PageGlyph>,
    /// Characters the face had no glyph for, so they are in [`Self::lines`]
    /// but not on the page. A page with a non-empty list here is not usable
    /// ground truth and the caller is expected to reject it rather than
    /// score against text that was never drawn.
    pub missing: Vec<char>,
    /// Characters that put coverage on the page and then left no binarized
    /// ink at all -- a mark too faint to survive thresholding.
    ///
    /// Separate from [`Self::missing`] because the cause is different and so
    /// is the fix: a missing glyph is a face that cannot set this text, a
    /// dropped one is a size at which this face's mark does not survive the
    /// pipeline. Both make the page unusable as ground truth; only the
    /// second is a statement about rendering.
    pub dropped: Vec<char>,
}

/// Blank margin around the text block, in pixels at any size. Enough that a
/// detector is not reading ink that touches the page edge.
const MARGIN: f32 = 16.0;

/// Sets `lines` in `face` at `px_per_em`.
///
/// `None` when nothing could be set at all — an empty text, or a face that
/// renders none of it.
/// Refuses a page whose own ground truth is not credible, before it reaches
/// disk.
///
/// These are the two properties that distinguish a hard page from a broken
/// one, and they are checked here rather than by a script run once because a
/// script run once is a thing that was true in September.
///
/// 1. **No letter or digit may claim a box two pixels tall or less.** A
///    capital `I` recorded as one pixel is an assertion that the page
///    contains a character it does not contain, and it scores as an engine
///    error forever after. Punctuation is exempt: a full stop really is two
///    pixels at 14 px/em.
/// 2. **A single closed contour must arrive as a single component.** One
///    contour cannot be two marks; if it is, the stem fell between samples
///    and the page is a rasterisation artefact rather than small text. The
///    converse is not checked because it is not true -- `O` is two contours
///    and one component.
///
/// A failure is an error, not a skip. Dropping the affected cells silently
/// would leave the corpus quietly missing its hardest faces at its smallest
/// sizes, which is the stratum every per-size figure is read off.
pub fn check(pg: &Page, face: &crate::ttf_load::Face<'_>) -> Result<(), String> {
    if !pg.dropped.is_empty() {
        return Err(format!(
            "binarization erased {:?}: coverage was laid down and no ink survived",
            pg.dropped
        ));
    }
    for g in &pg.glyphs {
        if g.ch.is_alphanumeric() && g.height <= 2 {
            return Err(format!(
                "{:?} has a {}x{} box: a letter two pixels tall or less is not a rendered glyph",
                g.ch, g.width, g.height
            ));
        }
        if g.components != 1 && face.contours(g.ch).is_some_and(|c| c.len() == 1) {
            return Err(format!(
                "{:?} is one closed contour and was drawn as {} separate marks: the stroke fell between samples",
                g.ch, g.components
            ));
        }
    }
    Ok(())
}

pub fn render(face: &Face<'_>, lines: &[String], px_per_em: f32) -> Option<Page> {
    let line_h = face.line_height_px(px_per_em).max(px_per_em * 1.2);
    let ascent = face.ascender_px(px_per_em).max(px_per_em * 0.8);
    let x_height = face.x_height_px(px_per_em).unwrap_or(px_per_em * 0.5);

    // Two passes: the first lays out and finds the extent, the second draws
    // into a buffer of the right size. Laying out twice is cheaper and far
    // less error-prone than growing a page buffer mid-draw.
    struct Placed {
        ch: char,
        line: usize,
        x: f32,
        top: f32,
        raster: crate::face::raster::Raster,
    }

    let mut placed: Vec<Placed> = Vec::new();
    let mut missing: Vec<char> = Vec::new();
    let mut max_right = 0.0f32;

    for (li, text) in lines.iter().enumerate() {
        let mut pen = MARGIN;
        let baseline = MARGIN + ascent + li as f32 * line_h;
        for ch in text.chars() {
            let advance = match face.advance_px(ch, px_per_em) {
                Some(a) => a,
                None => {
                    if !missing.contains(&ch) {
                        missing.push(ch);
                    }
                    continue;
                }
            };
            if let Some((raster, left_dx)) = face.render_placed(ch, px_per_em) {
                let x = pen + left_dx;
                let top = baseline - raster.baseline_dy;
                max_right = max_right.max(x + raster.width as f32);
                placed.push(Placed { ch, line: li, x, top, raster });
            }
            pen += advance;
        }
        max_right = max_right.max(pen);
    }

    if placed.is_empty() {
        return None;
    }

    let width = (max_right + MARGIN).ceil().max(1.0) as u32;
    let height = (MARGIN * 2.0 + ascent + (lines.len() as f32 - 1.0).max(0.0) * line_h
        + (line_h - ascent))
        .ceil()
        .max(1.0) as u32;

    let mut grey = vec![255u8; width as usize * height as usize];

    // Where each glyph's coverage grid sits on the page, in page pixels.
    // Signed because the antialiased fringe reaches left of and above the
    // ink box; the margin keeps it on the page, and anything that still
    // falls off is clipped.
    let mut origins: Vec<(i64, i64)> = Vec::with_capacity(placed.len());

    for p in &placed {
        let x0 = p.x.round().max(0.0) as i64 - i64::from(p.raster.cov_dx);
        let y0 = p.top.round().max(0.0) as i64 - i64::from(p.raster.cov_dy);
        origins.push((x0, y0));
        for r in 0..p.raster.cov_rows {
            let py = y0 + i64::from(r);
            if py < 0 || py >= i64::from(height) {
                continue;
            }
            for c in 0..p.raster.cov_cols {
                let px = x0 + i64::from(c);
                if px < 0 || px >= i64::from(width) {
                    continue;
                }
                let cov = p.raster.cov[(r * p.raster.cov_cols + c) as usize];
                if cov == 0 {
                    continue;
                }
                // Darkest wins where two marks overlap: coverage composites
                // as a max, and grey is its complement.
                let cell = &mut grey[py as usize * width as usize + px as usize];
                *cell = (*cell).min(255 - cov);
            }
        }
    }

    // One binarization of the whole page, with the shipped parameters, so a
    // truth box is the box the engine will actually find. Running it here
    // rather than trusting the rasteriser's own majority decision is the
    // whole point of the coverage change: the two disagree exactly where a
    // mark is fine, which is where the corpus was lying.
    let mask = ocrcer_core::image::binarize::binarize_with(
        &ocrcer_core::image::Gray { width, height, data: &grey },
        &ocrcer_core::params::Params::DEFAULT.binarize(),
    );

    let mut glyphs = Vec::with_capacity(placed.len());
    let mut dropped: Vec<char> = Vec::new();

    for (p, &(ox, oy)) in placed.iter().zip(origins.iter()) {
        // The tight box of binarized ink inside this glyph's own coverage
        // footprint. Restricting to the footprint is what keeps two marks
        // that touch from claiming each other's pixels.
        let (mut min_x, mut min_y) = (u32::MAX, u32::MAX);
        let (mut max_x, mut max_y) = (0u32, 0u32);
        let mut any = false;
        for r in 0..p.raster.cov_rows {
            let py = oy + i64::from(r);
            if py < 0 || py >= i64::from(height) {
                continue;
            }
            for c in 0..p.raster.cov_cols {
                let px = ox + i64::from(c);
                if px < 0 || px >= i64::from(width) {
                    continue;
                }
                if p.raster.cov[(r * p.raster.cov_cols + c) as usize] == 0 {
                    continue;
                }
                if mask[py as usize * width as usize + px as usize] == 0 {
                    continue;
                }
                any = true;
                min_x = min_x.min(px as u32);
                max_x = max_x.max(px as u32);
                min_y = min_y.min(py as u32);
                max_y = max_y.max(py as u32);
            }
        }
        if !any {
            if !dropped.contains(&p.ch) {
                dropped.push(p.ch);
            }
            continue;
        }

        // Count this glyph's components with the runtime's own labeller on a
        // crop of its box, so the corpus and the engine agree on what "one
        // mark" means rather than this file having a second opinion.
        let (bw, bh) = (max_x - min_x + 1, max_y - min_y + 1);
        let mut local = vec![0u8; (bw * bh) as usize];
        for r in 0..p.raster.cov_rows {
            let py = oy + i64::from(r);
            if py < i64::from(min_y) || py > i64::from(max_y) {
                continue;
            }
            for c in 0..p.raster.cov_cols {
                let px = ox + i64::from(c);
                if px < i64::from(min_x) || px > i64::from(max_x) {
                    continue;
                }
                if p.raster.cov[(r * p.raster.cov_cols + c) as usize] == 0 {
                    continue;
                }
                if mask[py as usize * width as usize + px as usize] == 0 {
                    continue;
                }
                let lr = py as u32 - min_y;
                let lc = px as u32 - min_x;
                local[(lr * bw + lc) as usize] = 1;
            }
        }
        let (_, ink_components) = ocrcer_core::image::components::label(
            &local,
            bw,
            bh,
            ocrcer_core::image::components::Connectivity::Eight,
        );

        // The same crop again, but of coverage alone: what the rasteriser
        // drew, before any thresholding decision.
        let mut drawn = vec![0u8; (bw * bh) as usize];
        for r in 0..p.raster.cov_rows {
            let py = oy + i64::from(r);
            if py < i64::from(min_y) || py > i64::from(max_y) {
                continue;
            }
            for c in 0..p.raster.cov_cols {
                let px = ox + i64::from(c);
                if px < i64::from(min_x) || px > i64::from(max_x) {
                    continue;
                }
                if p.raster.cov[(r * p.raster.cov_cols + c) as usize] == 0 {
                    continue;
                }
                let lr = py as u32 - min_y;
                let lc = px as u32 - min_x;
                drawn[(lr * bw + lc) as usize] = 1;
            }
        }
        let (_, components) = ocrcer_core::image::components::label(
            &drawn,
            bw,
            bh,
            ocrcer_core::image::components::Connectivity::Eight,
        );

        let baseline = (p.top + p.raster.baseline_dy).round().max(0.0) as u32;
        glyphs.push(PageGlyph {
            ch: p.ch,
            line: p.line,
            x: min_x,
            y: min_y,
            width: max_x - min_x + 1,
            height: max_y - min_y + 1,
            baseline,
            x_height,
            components,
            ink_components,
        });
    }

    Some(Page {
        width,
        height,
        grey,
        lines: lines.to_vec(),
        glyphs,
        missing,
        dropped,
    })
}

/// Serialises to binary PGM (`P5`), the smallest format that carries an
/// 8-bit grey image with no library and no ambiguity.
pub fn to_pgm(page: &Page) -> Vec<u8> {
    let mut out = format!("P5\n{} {}\n255\n", page.width, page.height).into_bytes();
    out.extend_from_slice(&page.grey);
    out
}

/// Reads back what [`to_pgm`] writes. Binary `P5`, maxval 255, with `#`
/// comment lines allowed between header fields because the format permits
/// them and a file that came from elsewhere may have one.
pub fn from_pgm(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut pos = 0usize;
    let mut field = || -> Result<String, String> {
        loop {
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos < bytes.len() && bytes[pos] == b'#' {
                while pos < bytes.len() && bytes[pos] != b'\n' {
                    pos += 1;
                }
                continue;
            }
            break;
        }
        let start = pos;
        while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if start == pos {
            return Err("truncated PGM header".into());
        }
        String::from_utf8(bytes[start..pos].to_vec()).map_err(|_| "bad PGM header".to_string())
    };
    let magic = field()?;
    if magic != "P5" {
        return Err(format!("not a binary PGM: {magic:?}"));
    }
    let w: u32 = field()?.parse().map_err(|_| "bad PGM width".to_string())?;
    let h: u32 = field()?.parse().map_err(|_| "bad PGM height".to_string())?;
    let maxval: u32 = field()?.parse().map_err(|_| "bad PGM maxval".to_string())?;
    if maxval != 255 {
        return Err(format!("PGM maxval {maxval} is not 255"));
    }
    // Exactly one whitespace byte separates the header from the raster.
    pos += 1;
    let want = w as usize * h as usize;
    let data = bytes
        .get(pos..pos + want)
        .ok_or_else(|| "PGM raster is short".to_string())?;
    Ok((w, h, data.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first parsable face `fonts.tsv` points at on this machine.
    ///
    /// `None` when there is none, and every test that uses it then skips
    /// *loudly*. A silently skipped test reports as a pass and checks
    /// nothing, which is the failure `CLAUDE.md` rule 4 warns about one
    /// level up; the printed line is what makes the skip visible in the
    /// test output.
    fn a_face() -> Option<Vec<u8>> {
        let entries = crate::tables::load_fonts(&crate::tables::model_dir()).ok()?;
        for e in entries {
            if let Some(p) = e.file() {
                if let Ok(b) = std::fs::read(p) {
                    if Face::parse(&b, 0).is_ok() {
                        return Some(b);
                    }
                }
            }
        }
        None
    }

    #[test]
    fn a_page_round_trips_through_pgm() {
        let Some(bytes) = a_face() else {
            eprintln!("SKIPPED: fonts.tsv points at no parsable face on this machine");
            return;
        };
        let face = Face::parse(&bytes, 0).unwrap();
        let page = render(&face, &["Invoice 1042".to_string()], 24.0).unwrap();
        let (w, h, grey) = from_pgm(&to_pgm(&page)).unwrap();
        assert_eq!((w, h), (page.width, page.height));
        assert_eq!(grey, page.grey);
    }

    /// The ground-truth boxes are the contract every comparison rests on: a
    /// box that does not contain its glyph's ink would silently feed the
    /// wrong pixels to a recogniser and score the result as if it were fair.
    #[test]
    fn every_glyph_box_contains_ink_and_only_its_own_rows() {
        let Some(bytes) = a_face() else {
            eprintln!("SKIPPED: fonts.tsv points at no parsable face on this machine");
            return;
        };
        let face = Face::parse(&bytes, 0).unwrap();
        let page = render(&face, &["Total 1,234.56".to_string()], 32.0).unwrap();
        assert!(page.missing.is_empty());
        for g in &page.glyphs {
            assert!(g.x + g.width <= page.width);
            assert!(g.y + g.height <= page.height);
            let inked = (g.y..g.y + g.height)
                .flat_map(|r| (g.x..g.x + g.width).map(move |c| (r, c)))
                .any(|(r, c)| page.grey[r as usize * page.width as usize + c as usize] == 0);
            assert!(inked, "{:?} has an empty box", g.ch);
        }
    }

    #[test]
    fn two_renders_of_the_same_text_are_byte_identical() {
        let Some(bytes) = a_face() else {
            eprintln!("SKIPPED: fonts.tsv points at no parsable face on this machine");
            return;
        };
        let face = Face::parse(&bytes, 0).unwrap();
        let lines = vec!["Account 4100".to_string(), "M8x1.25".to_string()];
        let a = render(&face, &lines, 21.0).unwrap();
        let b = render(&face, &lines, 21.0).unwrap();
        assert_eq!(a.grey, b.grey);
    }
}
