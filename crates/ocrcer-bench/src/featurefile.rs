//! `expected/glyphs/<name>.feature.json`: the checked-in expectation for
//! the feature-extraction stage.
//!
//! **Float format assumption, load-bearing.** Values are written with
//! Rust's default `{}` `Display` for `f32` and read back with `f32`'s
//! `FromStr`. Rust guarantees this pair round-trips to the identical bit
//! pattern: `{}` prints the *shortest* decimal string that parses back to
//! that exact `f32`, never a nearby one. That is what lets the runner
//! compare "expected vs. actual" by parsing this file and comparing
//! `f32::to_bits()`, with zero tolerance, per `ARCHITECTURE.md` section 3.1
//! and 8.2 (no transcendental function in the extractor => bit-identical
//! output on x86 and wasm32 => the fixture may legitimately demand bit
//! equality).
//!
//! This module hand-writes and hand-parses the file instead of going
//! through `serde_json`'s float path, because `serde_json` serialises
//! `f32` by way of its own formatter, whose byte-for-byte output is not
//! contractually pinned to match `std`'s `{}`. The schema here is small
//! and fixed, so a purpose-built writer/reader is a few dozen lines and
//! removes that ambiguity entirely rather than trusting it.

use std::fmt;
use std::fs;
use std::path::Path;

#[derive(Debug)]
pub struct FeatureFileError(pub String);

impl fmt::Display for FeatureFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for FeatureFileError {}

#[derive(Debug, Clone)]
pub struct FeatureFile {
    pub fixture: String,
    pub stage: String,
    pub values: Vec<f32>,
}

/// Extract the substring of `text` between the first `open` after `from`
/// and its matching `close`, assuming no nesting of that delimiter pair
/// inside (true for this file's flat schema).
fn between(text: &str, from: usize, open: char, close: char) -> Option<(&str, usize)> {
    let start = text[from..].find(open)? + from;
    let end = text[start + 1..].find(close)? + start + 1;
    Some((&text[start + 1..end], end + 1))
}

fn quoted_field(text: &str, key: &str) -> Result<String, FeatureFileError> {
    let pat = format!("\"{key}\"");
    let key_pos = text
        .find(&pat)
        .ok_or_else(|| FeatureFileError(format!("missing field {key:?}")))?;
    let after_colon = text[key_pos..]
        .find(':')
        .map(|i| key_pos + i + 1)
        .ok_or_else(|| FeatureFileError(format!("malformed field {key:?} (no colon)")))?;
    let (value, _) = between(text, after_colon, '"', '"')
        .ok_or_else(|| FeatureFileError(format!("malformed string value for {key:?}")))?;
    Ok(value.to_string())
}

fn number_field(text: &str, key: &str) -> Result<usize, FeatureFileError> {
    let pat = format!("\"{key}\"");
    let key_pos = text
        .find(&pat)
        .ok_or_else(|| FeatureFileError(format!("missing field {key:?}")))?;
    let after_colon = key_pos + pat.len();
    let rest = &text[after_colon..];
    let colon = rest
        .find(':')
        .ok_or_else(|| FeatureFileError(format!("malformed field {key:?} (no colon)")))?;
    let tail = &rest[colon + 1..];
    let digits: String = tail
        .chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits
        .parse::<usize>()
        .map_err(|e| FeatureFileError(format!("field {key:?} is not an integer: {e}")))
}

pub fn parse(text: &str) -> Result<FeatureFile, FeatureFileError> {
    let fixture = quoted_field(text, "fixture")?;
    let stage = quoted_field(text, "stage")?;
    let dims = number_field(text, "dims")?;

    let values_key = text
        .find("\"values\"")
        .ok_or_else(|| FeatureFileError("missing field \"values\"".into()))?;
    let (inner, _) = between(text, values_key, '[', ']')
        .ok_or_else(|| FeatureFileError("malformed \"values\" array (no matching brackets)".into()))?;

    let mut values = Vec::new();
    for (i, tok) in inner.split(',').enumerate() {
        let tok = tok.trim();
        if tok.is_empty() {
            continue;
        }
        let v: f32 = tok.parse().map_err(|e| {
            FeatureFileError(format!("values[{i}] = {tok:?} does not parse as f32: {e}"))
        })?;
        values.push(v);
    }

    if values.len() != dims {
        return Err(FeatureFileError(format!(
            "\"dims\" says {dims} but \"values\" has {} entries",
            values.len()
        )));
    }

    Ok(FeatureFile {
        fixture,
        stage,
        values,
    })
}

pub fn read(path: &Path) -> Result<FeatureFile, FeatureFileError> {
    let text = fs::read_to_string(path)
        .map_err(|e| FeatureFileError(format!("reading {}: {e}", path.display())))?;
    parse(&text).map_err(|e| FeatureFileError(format!("{}: {e}", path.display())))
}

pub fn format_file(fixture: &str, stage: &str, values: &[f32]) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str(&format!("  \"fixture\": \"{fixture}\",\n"));
    s.push_str(&format!("  \"stage\": \"{stage}\",\n"));
    s.push_str(&format!("  \"dims\": {},\n", values.len()));
    s.push_str("  \"values\": [\n");
    for (i, v) in values.iter().enumerate() {
        s.push_str(&format!("    {}", v));
        if i + 1 < values.len() {
            s.push(',');
        }
        s.push('\n');
    }
    s.push_str("  ]\n}\n");
    s
}

pub fn write(path: &Path, fixture: &str, stage: &str, values: &[f32]) -> Result<(), FeatureFileError> {
    fs::write(path, format_file(fixture, stage, values))
        .map_err(|e| FeatureFileError(format!("writing {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_exact_bits() {
        // A value whose shortest round-tripping decimal is not "nice",
        // to exercise that {} <-> FromStr really is bit-exact.
        let values = [0.1_f32, -0.0_f32, 1.0_f32, f32::MIN_POSITIVE, 123_456.79_f32];
        let text = format_file("f", "feature", &values);
        let parsed = parse(&text).unwrap();
        for (a, b) in values.iter().zip(parsed.values.iter()) {
            assert_eq!(a.to_bits(), b.to_bits(), "{a} vs {b}");
        }
    }

    #[test]
    fn dims_mismatch_is_an_error() {
        let text = "{\n  \"fixture\": \"f\",\n  \"stage\": \"feature\",\n  \"dims\": 3,\n  \"values\": [\n    1,\n    2\n  ]\n}\n";
        assert!(parse(text).is_err());
    }

    #[test]
    fn corrupt_json_is_an_error() {
        let text = "{ this is not json";
        assert!(parse(text).is_err());
    }
}
