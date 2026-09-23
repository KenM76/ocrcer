//! ASCII PBM (P1) reader/writer for glyph bitmap fixtures.
//!
//! Chosen over PNG deliberately: chunk 1 has no image decoder, and a P1 file
//! is human-readable, diffable and greppable — a reviewer can open a fixture
//! and see the letter it encodes. Format: a `P1` magic, optional `#`
//! comments to end-of-line, whitespace-separated `width height`, then
//! `width*height` bits (`0`/`1`) in row-major order, background = 0.

use std::fmt;
use std::fs;
use std::path::Path;

#[derive(Debug)]
pub struct PbmError(pub String);

impl fmt::Display for PbmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for PbmError {}

/// A decoded ASCII PBM bitmap: `width * height` bytes, row-major, 0/1.
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub ink: Vec<u8>,
}

/// Strip `#`-to-end-of-line comments, per the PBM spec, then tokenize on
/// ASCII whitespace.
fn tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = match line.find('#') {
            Some(i) => &line[..i],
            None => line,
        };
        out.extend(line.split_ascii_whitespace());
    }
    out
}

pub fn parse(text: &str) -> Result<Bitmap, PbmError> {
    let toks = tokens(text);
    let mut it = toks.into_iter();
    let magic = it
        .next()
        .ok_or_else(|| PbmError("empty PBM file (missing magic)".into()))?;
    if magic != "P1" {
        return Err(PbmError(format!(
            "unsupported PBM magic {magic:?}, expected \"P1\" (ASCII PBM)"
        )));
    }
    let width: u32 = it
        .next()
        .ok_or_else(|| PbmError("PBM missing width".into()))?
        .parse()
        .map_err(|e| PbmError(format!("PBM width is not an integer: {e}")))?;
    let height: u32 = it
        .next()
        .ok_or_else(|| PbmError("PBM missing height".into()))?
        .parse()
        .map_err(|e| PbmError(format!("PBM height is not an integer: {e}")))?;
    let expected_n = (width as usize) * (height as usize);
    let mut ink = Vec::with_capacity(expected_n);
    for tok in it {
        let bit: u8 = match tok {
            "0" => 0,
            "1" => 1,
            other => {
                return Err(PbmError(format!(
                    "PBM bit token {other:?} is neither \"0\" nor \"1\""
                )))
            }
        };
        ink.push(bit);
    }
    if ink.len() != expected_n {
        return Err(PbmError(format!(
            "PBM declares {width}x{height} = {expected_n} pixels but found {} bit tokens",
            ink.len()
        )));
    }
    Ok(Bitmap { width, height, ink })
}

pub fn read(path: &Path) -> Result<Bitmap, PbmError> {
    let text = fs::read_to_string(path)
        .map_err(|e| PbmError(format!("reading {}: {e}", path.display())))?;
    parse(&text).map_err(|e| PbmError(format!("{}: {e}", path.display())))
}

/// Render a bitmap back to ASCII PBM text, one row per line for readability.
pub fn format(bmp: &Bitmap) -> String {
    let mut s = String::new();
    s.push_str("P1\n");
    s.push_str(&format!("{} {}\n", bmp.width, bmp.height));
    for row in 0..bmp.height as usize {
        let start = row * bmp.width as usize;
        let end = start + bmp.width as usize;
        let line: Vec<&str> = bmp.ink[start..end]
            .iter()
            .map(|&b| if b != 0 { "1" } else { "0" })
            .collect();
        s.push_str(&line.join(" "));
        s.push('\n');
    }
    s
}

pub fn write(path: &Path, bmp: &Bitmap) -> Result<(), PbmError> {
    fs::write(path, format(bmp)).map_err(|e| PbmError(format!("writing {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let text = "P1\n# a comment\n3 2\n1 0 1\n0 1 0\n";
        let bmp = parse(text).unwrap();
        assert_eq!(bmp.width, 3);
        assert_eq!(bmp.height, 2);
        assert_eq!(bmp.ink, vec![1, 0, 1, 0, 1, 0]);
        let text2 = format(&bmp);
        let bmp2 = parse(&text2).unwrap();
        assert_eq!(bmp.ink, bmp2.ink);
    }

    #[test]
    fn rejects_wrong_count() {
        let text = "P1\n2 2\n1 0 1\n";
        assert!(parse(text).is_err());
    }

    #[test]
    fn rejects_bad_magic() {
        let text = "P4\n2 2\n";
        assert!(parse(text).is_err());
    }
}
