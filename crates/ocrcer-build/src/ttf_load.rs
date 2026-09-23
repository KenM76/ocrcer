//! Reads a font file into contours. **Parsing only** — never rasterising.
//!
//! # The division of labour, and why it is not negotiable
//!
//! A font file is two things at once: a set of curve coordinates, and a
//! rendering intent. Reading the coordinates is a parsing problem with one
//! right answer, which `ttf-parser` (MIT OR Apache-2.0, zero dependencies of
//! its own) already solves — writing a second `glyf`/`CFF`/`cmap` reader
//! here would buy nothing.
//!
//! Deciding which pixels are ink is not that. It is a policy: sample
//! positions, a coverage threshold, what happens to a mark finer than a
//! pixel, where the pixel lattice is anchored. Every rasteriser answers
//! those differently and all the answers are defensible. If the authored
//! face went through [`crate::face::raster`]'s policy and a licensed face
//! went through someone else's, the bank would hold prototypes measured with
//! two different rulers — `CLAUDE.md` rule 4's hazard, one level below the
//! feature extractor it names, and with nothing in the format to report it.
//! Accuracy would simply be quieter and worse.
//!
//! So this module hands [`crate::outline`] polylines in design units and
//! stops there.
//!
//! # Curves
//!
//! `glyf` stores quadratics, `CFF`/`CFF2` cubics; both arrive here through
//! the same builder and are flattened at
//! [`crate::face::raster::QUAD_STEPS`] fixed steps, so a face rasterises to
//! the same bytes on every machine that rebuilds the bank.

use crate::face::raster::Raster;
use crate::outline::{self, Polyline};

/// A parsed font file, borrowing the bytes it was parsed from.
pub struct Face<'a> {
    inner: ttf_parser::Face<'a>,
}

/// Why a font file could not be used. Distinguished from "this face does not
/// have that character", which is not an error: a face covering part of the
/// charset is normal and the builder's job is to record which part.
#[derive(Debug)]
pub enum FaceError {
    /// The bytes are not a font this parser understands.
    Parse(ttf_parser::FaceParsingError),
    /// `head.unitsPerEm` is zero or absent, so no coordinate in the file has
    /// a defined scale.
    NoUnitsPerEm,
}

impl core::fmt::Display for FaceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FaceError::Parse(e) => write!(f, "not a parsable font file: {e}"),
            FaceError::NoUnitsPerEm => write!(f, "font declares no units per em"),
        }
    }
}

impl<'a> Face<'a> {
    /// Parses `data`. `index` selects a face within a `.ttc` collection and
    /// is `0` for a plain `.ttf`/`.otf`.
    pub fn parse(data: &'a [u8], index: u32) -> Result<Self, FaceError> {
        let inner = ttf_parser::Face::parse(data, index).map_err(FaceError::Parse)?;
        if inner.units_per_em() == 0 {
            return Err(FaceError::NoUnitsPerEm);
        }
        Ok(Face { inner })
    }

    pub fn units_per_em(&self) -> u16 {
        self.inner.units_per_em()
    }

    /// Whether the face maps `c` to a glyph at all. A `.notdef` mapping is
    /// reported as absent: rendering the missing-glyph box as if it were the
    /// character would put a rectangle into the bank under that character's
    /// class, which is worse than having no prototype for it — a wrong
    /// prototype is matched, a missing one is only missed.
    pub fn has_char(&self, c: char) -> bool {
        self.inner.glyph_index(c).is_some_and(|g| g.0 != 0)
    }

    /// `c`'s outline as closed polylines in design units, y upward.
    ///
    /// `None` when the face has no glyph for `c`. `Some(vec![])` when it has
    /// one with no outline — a space — which is a different thing and the
    /// caller has to tell them apart.
    pub(crate) fn contours(&self, c: char) -> Option<Vec<Polyline>> {
        let id = self.inner.glyph_index(c)?;
        if id.0 == 0 {
            return None;
        }
        let mut builder = Builder::default();
        // `outline_glyph` returns None for a glyph with no outline, which is
        // an empty contour set rather than a missing glyph.
        if self.inner.outline_glyph(id, &mut builder).is_none() {
            return Some(Vec::new());
        }
        builder.finish();
        Some(builder.polys)
    }

    /// x-height in pixels at `px_per_em`: the OS/2 `sxHeight` the face
    /// declares, or — when it declares none — the measured ink height of the
    /// face's own `x`.
    ///
    /// Measured rather than assumed. The feature extractor takes x-height as
    /// an input and a fabricated ratio would be a number from nowhere
    /// (`CLAUDE.md` rule 1); rendering the letter and reading its height is a
    /// measurement of the same face the prototypes come from. `None` only
    /// when the face declares nothing *and* has no `x`.
    pub fn x_height_px(&self, px_per_em: f32) -> Option<f32> {
        if let Some(sx) = self.inner.x_height().filter(|&v| v > 0) {
            return Some(px_per_em * f32::from(sx) / f32::from(self.units_per_em()));
        }
        self.render('x', px_per_em).map(|r| r.height as f32)
    }

    /// `c` rasterised at `px_per_em` by [`crate::outline`], cropped tight —
    /// the same bitmap contract [`crate::face::raster::render`] delivers for
    /// the authored face. `None` when the face lacks the glyph or the glyph
    /// lands no ink (a space).
    pub fn render(&self, c: char, px_per_em: f32) -> Option<Raster> {
        let polys = self.contours(c)?;
        outline::rasterize_polylines(&polys, self.units_per_em(), px_per_em)
    }

    /// `c`'s bitmap together with its ink's left edge in pixels from the
    /// glyph origin — what [`Self::render`] gives, plus where to put it.
    ///
    /// `Some((raster, left_dx))`. `None` for a missing glyph *or* a glyph
    /// with no ink, which for placement purposes are the same: neither puts
    /// anything on the page. A caller setting type still has to advance the
    /// pen for a space, so it reads [`Self::advance_px`] separately.
    pub fn render_placed(&self, c: char, px_per_em: f32) -> Option<(Raster, f32)> {
        let polys = self.contours(c)?;
        outline::rasterize_polylines_placed(&polys, self.units_per_em(), px_per_em)
    }

    /// How far the pen moves after setting `c`, in pixels.
    ///
    /// Unkerned. Kerning is a face's opinion about specific pairs and the
    /// pages this produces are a test corpus, not typography; what matters
    /// is that the same text at the same size gives the same pixels on every
    /// machine, and `kern`/`GPOS` coverage varies by face in a way that
    /// would make the corpus depend on which faces happen to be installed.
    pub fn advance_px(&self, c: char, px_per_em: f32) -> Option<f32> {
        let id = self.inner.glyph_index(c)?;
        let adv = self.inner.glyph_hor_advance(id)?;
        Some(px_per_em * f32::from(adv) / f32::from(self.units_per_em()))
    }

    /// Baseline-to-baseline distance in pixels: `hhea`'s ascender, descender
    /// and line gap as the face declares them.
    pub fn line_height_px(&self, px_per_em: f32) -> f32 {
        let upem = f32::from(self.units_per_em());
        let a = f32::from(self.inner.ascender());
        let d = f32::from(self.inner.descender());
        let g = f32::from(self.inner.line_gap());
        px_per_em * (a - d + g) / upem
    }

    /// Distance in pixels from the top of a line's slug to its baseline.
    pub fn ascender_px(&self, px_per_em: f32) -> f32 {
        px_per_em * f32::from(self.inner.ascender()) / f32::from(self.units_per_em())
    }
}

/// Collects `ttf-parser`'s outline callbacks into closed polylines,
/// flattening curves as they arrive.
#[derive(Default)]
struct Builder {
    polys: Vec<Polyline>,
    current: Polyline,
}

impl Builder {
    /// Closes whatever contour is open, making the closing edge explicit.
    ///
    /// A contour that never received an explicit `close()` still closes: the
    /// winding-number fill counts crossings, and one missing edge would leak
    /// the fill across the whole scanline. Real files do omit it.
    fn finish(&mut self) {
        let poly = core::mem::take(&mut self.current);
        if poly.len() < 2 {
            return;
        }
        let mut poly = poly;
        if poly[0] != poly[poly.len() - 1] {
            poly.push(poly[0]);
        }
        self.polys.push(poly);
    }
}

impl ttf_parser::OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        self.current.push((f64::from(x), f64::from(y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.current.push((f64::from(x), f64::from(y)));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        if self.current.is_empty() {
            return;
        }
        outline::flatten_quad(
            &mut self.current,
            (f64::from(x1), f64::from(y1)),
            (f64::from(x), f64::from(y)),
        );
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        if self.current.is_empty() {
            return;
        }
        outline::flatten_cubic(
            &mut self.current,
            (f64::from(x1), f64::from(y1)),
            (f64::from(x2), f64::from(y2)),
            (f64::from(x), f64::from(y)),
        );
    }

    fn close(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocrcer_core::feature::{extract, GlyphInput, HOLE_COUNT};
    use std::path::PathBuf;

    /// Size the shape assertions below render at. Well above the 21 px/em
    /// floor the authored face was measured at, because these tests are
    /// about whether the *loader* reads a face correctly, not about how
    /// small a face stays legible — a counter that fuses at the floor would
    /// be a resolution finding reported as a parsing bug.
    const TEST_PX: f32 = 48.0;

    /// Every `eligible-present` face in `model/fonts.tsv` whose file is
    /// actually on this machine, as `(family, path)`.
    ///
    /// The table is the authority on which faces exist and where; a path
    /// typed into a test would be a second, driftable inventory.
    fn present_faces() -> Vec<(String, PathBuf)> {
        let tsv = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../model/fonts.tsv")
            .canonicalize()
            .expect("model/fonts.tsv");
        let text = std::fs::read_to_string(&tsv).expect("read model/fonts.tsv");
        let mut out = Vec::new();
        for line in text.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let f: Vec<&str> = line.split('\t').collect();
            assert_eq!(f.len(), 9, "fonts.tsv row width: {line}");
            if f[4] != "eligible-present" {
                continue;
            }
            let path = PathBuf::from(f[6]);
            if path.is_file() {
                out.push((f[0].to_string(), path));
            }
        }
        out
    }

    fn holes(face: &Face<'_>, c: char) -> Option<u32> {
        let r = face.render(c, TEST_PX)?;
        let x_height = face.x_height_px(TEST_PX).unwrap_or(0.0);
        let features = extract(&GlyphInput {
            ink: &r.ink,
            width: r.width,
            height: r.height,
            baseline_dy: r.baseline_dy,
            x_height,
        });
        Some(features[HOLE_COUNT] as u32)
    }

    /// Guards the test below from passing vacuously: a machine with no
    /// eligible face present cannot build the bank at all, and a suite that
    /// reported green in that state would be reporting nothing.
    #[test]
    fn at_least_one_eligible_face_is_present_on_this_machine() {
        let faces = present_faces();
        assert!(
            !faces.is_empty(),
            "no eligible-present face in model/fonts.tsv exists on disk; \
             the prototype bank cannot be built here"
        );
    }

    /// Reads every present face and checks the shapes the bank depends on
    /// being right: counters where the design has counters, none where it
    /// does not, caps taller than x-height letters, ink touching all four
    /// edges of the cropped box, and a glyph the face genuinely lacks
    /// reported as absent rather than as `.notdef`'s box.
    ///
    /// Covers both outline flavours: `.ttf` quadratics and `.otf` CFF
    /// cubics, whichever of each is installed.
    #[test]
    fn every_present_face_loads_with_the_shapes_its_design_has() {
        for (family, path) in present_faces() {
            let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{family}: {e}"));
            let face = Face::parse(&data, 0).unwrap_or_else(|e| panic!("{family}: {e}"));
            assert!(face.units_per_em() > 0, "{family}: units per em");

            for (c, want) in [('o', 1), ('8', 2), ('B', 2), ('x', 0), ('v', 0)] {
                let Some(got) = holes(&face, c) else {
                    // A face need not cover the whole charset; that is the
                    // bank's coverage question, not this test's.
                    continue;
                };
                assert_eq!(got, want, "{family}: {c:?} holes");
            }

            if let (Some(h), Some(x)) = (face.render('H', TEST_PX), face.render('x', TEST_PX)) {
                assert!(h.height > x.height, "{family}: 'H' should out-top 'x'");
                let w = h.width as usize;
                assert!(h.ink[..w].contains(&1), "{family}: 'H' top row has no ink");
                assert!(h.ink[h.ink.len() - w..].contains(&1), "{family}: 'H' bottom row bare");
                assert!(h.ink.chunks(w).any(|r| r[0] == 1), "{family}: 'H' left column bare");
                assert!(h.ink.chunks(w).any(|r| r[w - 1] == 1), "{family}: 'H' right column bare");
            }

            assert!(
                !face.has_char('\u{10FFFD}'),
                "{family}: reported a glyph for an unassigned private-use code point"
            );
        }
    }

    /// A space has a glyph and no ink. Distinguishing that from a missing
    /// glyph matters to the bank: a missing glyph is a coverage gap to
    /// record, an inkless one is a character with nothing to measure.
    #[test]
    fn a_space_is_present_but_inkless() {
        for (family, path) in present_faces() {
            let data = std::fs::read(&path).unwrap();
            let face = Face::parse(&data, 0).unwrap();
            if !face.has_char(' ') {
                continue;
            }
            assert_eq!(face.contours(' ').map(|c| c.len()), Some(0), "{family}: space outline");
            assert!(face.render(' ', TEST_PX).is_none(), "{family}: space rendered ink");
        }
    }

    #[test]
    fn a_non_font_is_a_parse_error_not_a_panic() {
        assert!(Face::parse(b"this is not a font", 0).is_err());
    }
}
