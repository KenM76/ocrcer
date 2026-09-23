//! Reads `model/charset.tsv` and `model/fonts.tsv`.
//!
//! Both files are the project's authorities on what the engine recognises and
//! what it may be built from. Nothing here interprets them beyond parsing:
//! a class's index is the index the file gives it, and a face's distribution
//! is the word in its `distribution` column. A second inventory typed into
//! Rust would be the one that drifts.
//!
//! # Contract
//!
//! Both loaders fail loudly. A malformed row, a duplicate class index, a
//! gap in the index sequence or an unknown enum value is an error, not a
//! skipped line: the charset's index *is* the class identity written into
//! every prototype and every `.ocrw` file, so a silently dropped row would
//! renumber every class after it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Where a class's ink sits relative to the line's baseline and x-height.
/// A pruning bucket, not a metric dimension (`ARCHITECTURE.md` section 11).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaselineClass {
    /// Sits on the baseline, reaching the x-height.
    XHeight,
    /// Sits on the baseline, reaching the cap/ascender height.
    Ascender,
    /// Descends below the baseline.
    Descender,
    /// Spans ascender height to below the baseline.
    Full,
    /// Floats above the x-height without touching the baseline.
    Above,
    /// Sits at or just above the baseline, well below the x-height.
    Low,
}

impl BaselineClass {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "xheight" => BaselineClass::XHeight,
            "ascender" => BaselineClass::Ascender,
            "descender" => BaselineClass::Descender,
            "full" => BaselineClass::Full,
            "above" => BaselineClass::Above,
            "low" => BaselineClass::Low,
            _ => return None,
        })
    }
}

/// One recognised character class.
#[derive(Clone, Debug)]
pub struct Class {
    /// Position in `charset.tsv`, and the class identity in every prototype
    /// and every `.ocrw` file.
    pub index: u16,
    pub codepoint: char,
    pub category: String,
    pub baseline_class: BaselineClass,
    /// Inclusive band on **width over cap height**, exactly as authored.
    ///
    /// Not the extractor's aspect dimension and not convertible into it.
    /// Section 3's dim 103 is `(w - h) / (w + h)` over the glyph's *own ink
    /// height*; this column's denominator is the face's cap height, so a flat
    /// mark like `.` or `-` reads near `1.0` in one and near `0.1` in the
    /// other. Converting by `r -> (r - 1) / (r + 1)` changes the algebraic
    /// form and leaves the denominator wrong, which is why that conversion was
    /// removed; see section 11's 2026-09-22 denominator entry. Nothing gates
    /// on this today, and a band that could would have to be authored or
    /// measured in the extractor's own quantity.
    pub width_over_cap_min: f32,
    pub width_over_cap_max: f32,
    /// Index of the other-case class this pairs with, or `None` when the
    /// class has no case twin.
    pub case_twin: Option<u16>,
}

/// Whether what is derived from a face may ship.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Distribution {
    /// Licence permits shipping what is derived from it: the base segment.
    Shippable,
    /// Kept in scope, but everything derived from it is built on the end
    /// user's machine and never leaves it.
    LocalOnly,
    /// In no bank, shipped or local. Either the licence does not permit it
    /// or another row already carries the same shapes; the `notes` column
    /// says which.
    Excluded,
}

impl Distribution {
    /// Whether a face with this distribution may be read for a build.
    ///
    /// The one place that answers this. `excluded` is excluded from every
    /// build, `--local` included; a second copy of this test that got that
    /// wrong would put a face into a corpus that the bank does not carry,
    /// and the corpus is what the bank is judged on.
    pub fn usable(self, include_local_only: bool) -> bool {
        match self {
            Distribution::Shippable => true,
            Distribution::LocalOnly => include_local_only,
            Distribution::Excluded => false,
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "shippable" => Distribution::Shippable,
            "local-only" => Distribution::LocalOnly,
            "excluded" => Distribution::Excluded,
            _ => return None,
        })
    }
}

/// One font family/style in the inventory.
#[derive(Clone, Debug)]
pub struct FontEntry {
    pub family: String,
    pub style: String,
    /// SPDX-style identifier from the inventory, e.g. `OFL-1.1`.
    pub licence: String,
    /// Where that identifier was read from — embedded metadata, an
    /// accompanying licence file, or a published statement.
    pub licence_source: String,
    pub status: String,
    pub distribution: Distribution,
    /// The `path` column verbatim. Only meaningful when `status` is
    /// `eligible-present`; other rows carry prose there.
    pub path: String,
    /// Shape-space group — `sans`, `serif`, `mono`, `condensed`, `technical`.
    pub category: String,
}

impl FontEntry {
    /// The file on disk, when this row names one that exists.
    pub fn file(&self) -> Option<PathBuf> {
        if self.status != "eligible-present" {
            return None;
        }
        let p = PathBuf::from(&self.path);
        p.is_file().then_some(p)
    }
}

/// `model/`, resolved from this crate's location rather than the working
/// directory, so a test and a CLI run read the same files.
pub fn model_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../model")
}

fn rows(path: &Path, width: usize) -> Result<Vec<Vec<String>>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<String> = line.split('\t').map(str::to_string).collect();
        if f.len() != width {
            return Err(format!(
                "{}:{}: expected {width} tab-separated fields, found {}",
                path.display(),
                n + 1,
                f.len()
            ));
        }
        out.push(f);
    }
    Ok(out)
}

/// A `charset.tsv` width-over-height ratio in the extractor's
/// `(w - h) / (w + h)` form.
fn aspect(field: &str) -> Result<f32, String> {
    let r: f32 = field
        .parse()
        .map_err(|_| format!("bad aspect ratio {field:?}"))?;
    if !r.is_finite() || r <= 0.0 {
        return Err(format!("aspect ratio must be positive and finite, got {field:?}"));
    }
    // Returned as authored. The column is width over cap height and stays in
    // that denominator: see the field's doc comment.
    Ok(r)
}

/// Loads `model/charset.tsv`, in file order.
///
/// Errors if any index is out of sequence: class identity is positional, so
/// a hole or a repeat would silently rename classes.
pub fn load_charset(dir: &Path) -> Result<Vec<Class>, String> {
    let path = dir.join("charset.tsv");
    let mut classes = Vec::new();
    for (row, f) in rows(&path, 10)?.into_iter().enumerate() {
        let index: u16 = f[0].parse().map_err(|_| format!("bad class index {:?}", f[0]))?;
        if usize::from(index) != row {
            return Err(format!("charset.tsv: class index {index} at row {row}"));
        }
        let cp = f[1]
            .strip_prefix("U+")
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .and_then(char::from_u32)
            .ok_or_else(|| format!("bad codepoint {:?}", f[1]))?;
        let baseline_class = BaselineClass::parse(&f[4])
            .ok_or_else(|| format!("unknown baseline class {:?}", f[4]))?;
        let twin: i32 = f[8].parse().map_err(|_| format!("bad case twin {:?}", f[8]))?;
        classes.push(Class {
            index,
            codepoint: cp,
            category: f[3].clone(),
            baseline_class,
            width_over_cap_min: aspect(&f[5])?,
            width_over_cap_max: aspect(&f[6])?,
            case_twin: (twin >= 0).then_some(twin as u16),
        });
    }
    if classes.is_empty() {
        return Err("charset.tsv has no classes".into());
    }
    Ok(classes)
}

/// Loads `model/fonts.tsv`, in file order.
pub fn load_fonts(dir: &Path) -> Result<Vec<FontEntry>, String> {
    let path = dir.join("fonts.tsv");
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for f in rows(&path, 9)? {
        let key = (f[0].clone(), f[1].clone());
        if !seen.insert(key) {
            return Err(format!("fonts.tsv: duplicate family/style {:?} {:?}", f[0], f[1]));
        }
        out.push(FontEntry {
            family: f[0].clone(),
            style: f[1].clone(),
            licence: f[2].clone(),
            licence_source: f[3].clone(),
            status: f[4].clone(),
            distribution: Distribution::parse(&f[5])
                .ok_or_else(|| format!("unknown distribution {:?}", f[5]))?,
            path: f[6].clone(),
            category: f[7].clone(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_charset_loads_and_is_densely_indexed() {
        let classes = load_charset(&model_dir()).expect("charset.tsv");
        assert_eq!(classes.len(), 187, "class count");
        for (i, c) in classes.iter().enumerate() {
            assert_eq!(usize::from(c.index), i);
            assert!(
                c.width_over_cap_min <= c.width_over_cap_max,
                "{:?} aspect band",
                c.codepoint
            );
        }
    }

    /// A case twin must point at a class that exists and must point back,
    /// or the decoder's case reasoning would follow a dangling index.
    /// The band is authored as width over cap height and is loaded in that
    /// denominator. A load that "converted" it would leave every band in a
    /// form matching nothing downstream, and nothing would say so.
    #[test]
    fn the_aspect_band_is_loaded_in_the_denominator_it_was_authored_in() {
        let classes = load_charset(&model_dir()).unwrap();
        for c in &classes {
            assert!(
                c.width_over_cap_min > 0.0 && c.width_over_cap_max.is_finite(),
                "{:?} band {}..{} is not a positive width-over-cap-height ratio",
                c.codepoint,
                c.width_over_cap_min,
                c.width_over_cap_max
            );
        }
        // Authored at a tenth as wide as it is tall, and it stays that.
        let bang = classes.iter().find(|c| c.codepoint == '!').unwrap();
        assert!(bang.width_over_cap_max < 0.5, "{}", bang.width_over_cap_max);
    }

    #[test]
    fn case_twins_are_mutual() {
        let classes = load_charset(&model_dir()).unwrap();
        for c in &classes {
            let Some(t) = c.case_twin else { continue };
            let twin = classes.get(usize::from(t)).expect("twin index in range");
            assert_eq!(
                twin.case_twin,
                Some(c.index),
                "{:?} twins {:?} but not the reverse",
                c.codepoint,
                twin.codepoint
            );
        }
    }

    #[test]
    fn the_font_table_loads_with_a_usable_face_present() {
        let fonts = load_fonts(&model_dir()).expect("fonts.tsv");
        assert_eq!(fonts.len(), 47, "font row count");
        let usable = fonts
            .iter()
            .filter(|f| f.distribution != Distribution::Excluded && f.file().is_some())
            .count();
        assert!(usable > 0, "no usable face on this machine");
    }
}
