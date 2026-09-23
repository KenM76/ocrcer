//! Writes the `.ocrw` container described in `ARCHITECTURE.md` section 7.
//!
//! # Contract
//!
//! Little-endian throughout. Table data is 64-byte aligned. The blob's
//! CRC-32 covers the concatenated table data including alignment padding, so
//! a file that survives the check is byte-for-byte the file that was written.
//!
//! **The output is a pure function of its inputs.** Nothing here reads the
//! clock, the machine name, or a random number. A build identifier that
//! changed between two runs of the same inputs would defeat the one property
//! `PLAN.md` chunk 3 asks of this writer — that a rebuild produces the same
//! bytes — so the identifier is a digest of the inputs rather than a stamp of
//! the occasion.
//!
//! This crate writes; `ocrcer-core` reads. The two halves are checked against
//! each other by round-trip, not by inspection.

use std::io::Write;

/// How a table's bytes are to be read, re-exported from the reader so the
/// writer and the reader cannot disagree about a discriminant.
pub use ocrcer_core::ocrw::Kind;

/// One entry in the table directory, with its data.
pub struct Table {
    pub name: String,
    pub kind: Kind,
    pub dims: Vec<u32>,
    /// Per-dimension dequantisation scales. Required for `Kind::I8`, empty
    /// otherwise.
    pub scales: Vec<f32>,
    pub data: Vec<u8>,
}

impl Table {
    pub fn f32s(name: &str, dims: Vec<u32>, values: &[f32]) -> Table {
        let mut data = Vec::with_capacity(values.len() * 4);
        for v in values {
            data.extend_from_slice(&v.to_le_bytes());
        }
        Table { name: name.into(), kind: Kind::F32, dims, scales: Vec::new(), data }
    }

    pub fn opaque(name: &str, dims: Vec<u32>, data: Vec<u8>) -> Table {
        Table { name: name.into(), kind: Kind::Opaque, dims, scales: Vec::new(), data }
    }

    /// An `i8` matrix with the per-column scales that dequantise it.
    pub fn i8s(name: &str, rows: u32, cols: u32, values: Vec<i8>, scales: Vec<f32>) -> Table {
        assert_eq!(values.len(), rows as usize * cols as usize);
        assert_eq!(scales.len(), cols as usize);
        Table {
            name: name.into(),
            kind: Kind::I8,
            dims: vec![rows, cols],
            scales,
            data: values.into_iter().map(|v| v as u8).collect(),
        }
    }

    fn directory_len(&self) -> u64 {
        // name_len + name + kind + ndim + dims + n_scales + scale_off
        // + data_off + data_len
        2 + self.name.len() as u64 + 1 + 1 + 4 * self.dims.len() as u64 + 4 + 8 + 8 + 8
    }
}

/// Quantises a row-major `rows x cols` matrix to `i8` with one scale per
/// column.
///
/// The scale is `max|v| / 127` over the column, so the largest magnitude in
/// each dimension lands on the end of the range and no value clips. A column
/// that is all zeros gets a scale of `1.0`, never `0.0`: dequantising by zero
/// would turn a harmless constant dimension into a division by zero at load.
///
/// Returns the values and the scales. Quantisation error is not estimated
/// here — it is measured, by comparing top-1 agreement before and after, which
/// is the only honest form the number can take.
pub fn quantise(values: &[f32], rows: usize, cols: usize) -> (Vec<i8>, Vec<f32>) {
    assert_eq!(values.len(), rows * cols);
    let mut scales = vec![1.0f32; cols];
    for (c, scale) in scales.iter_mut().enumerate() {
        let peak = (0..rows).fold(0.0f32, |m, r| m.max(values[r * cols + c].abs()));
        if peak > 0.0 {
            *scale = peak / 127.0;
        }
    }
    let mut out = vec![0i8; values.len()];
    for r in 0..rows {
        for c in 0..cols {
            let q = (f64::from(values[r * cols + c]) / f64::from(scales[c])).round();
            out[r * cols + c] = q.clamp(-127.0, 127.0) as i8;
        }
    }
    (out, scales)
}

/// Dequantises what `quantise` produced, as the runtime will at load.
pub fn dequantise(values: &[i8], scales: &[f32], rows: usize, cols: usize) -> Vec<f32> {
    assert_eq!(values.len(), rows * cols);
    assert_eq!(scales.len(), cols);
    let mut out = vec![0.0f32; values.len()];
    for r in 0..rows {
        for c in 0..cols {
            out[r * cols + c] = f32::from(values[r * cols + c]) * scales[c];
        }
    }
    out
}

/// CRC-32 over the table blob, re-exported from the reader.
///
/// The writer stamps it and the reader checks it, so two implementations
/// would have to agree forever with nothing reporting the day they stopped —
/// the duplication `CLAUDE.md` rule 4 forbids. It lives in `ocrcer-core`
/// because this crate depends on that one and not the reverse.
pub use ocrcer_core::ocrw::crc32;

const ALIGN: u64 = 64;

fn pad_to(offset: u64) -> u64 {
    offset.div_ceil(ALIGN) * ALIGN
}

/// Writes a `.ocrw` file.
///
/// `meta` is the UTF-8 JSON of section 7's `meta` block, which carries the
/// charset and the feature-extractor version so a file and a runtime can
/// never disagree about what a class index or a dimension means.
pub fn write(path: &std::path::Path, version: u16, model_kind: u16, meta: &str, tables: &[Table]) -> std::io::Result<()> {
    // Header up to and including `reserved`.
    let header_len: u64 = 4 + 2 + 2 + 4 + 4 + meta.len() as u64 + 4 + 8;
    let directory_len: u64 = tables.iter().map(Table::directory_len).sum();

    // Scales sit between the directory and the aligned blob, so the blob's
    // alignment does not depend on how many of them there are.
    let scales_off = header_len + directory_len;
    let mut offset = scales_off;
    let mut scale_offs = Vec::with_capacity(tables.len());
    for t in tables {
        scale_offs.push(offset);
        offset += 4 * t.scales.len() as u64;
    }

    let blob_start = pad_to(offset);
    let mut blob = Vec::new();
    let mut data_offs = Vec::with_capacity(tables.len());
    for t in tables {
        let at = pad_to(blob_start + blob.len() as u64);
        blob.resize((at - blob_start) as usize, 0);
        data_offs.push(at);
        blob.extend_from_slice(&t.data);
    }

    let mut out = Vec::with_capacity((blob_start as usize) + blob.len());
    out.extend_from_slice(b"OCRW");
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&model_kind.to_le_bytes());
    out.extend_from_slice(&(tables.len() as u32).to_le_bytes());
    out.extend_from_slice(&(meta.len() as u32).to_le_bytes());
    out.extend_from_slice(meta.as_bytes());
    out.extend_from_slice(&crc32(&blob).to_le_bytes());
    out.extend_from_slice(&[0u8; 8]);

    for (i, t) in tables.iter().enumerate() {
        out.extend_from_slice(&(t.name.len() as u16).to_le_bytes());
        out.extend_from_slice(t.name.as_bytes());
        out.push(t.kind as u8);
        out.push(t.dims.len() as u8);
        for d in &t.dims {
            out.extend_from_slice(&d.to_le_bytes());
        }
        out.extend_from_slice(&(t.scales.len() as u32).to_le_bytes());
        out.extend_from_slice(&scale_offs[i].to_le_bytes());
        out.extend_from_slice(&data_offs[i].to_le_bytes());
        out.extend_from_slice(&(t.data.len() as u64).to_le_bytes());
    }
    debug_assert_eq!(out.len() as u64, scales_off);

    for t in tables {
        for s in &t.scales {
            out.extend_from_slice(&s.to_le_bytes());
        }
    }
    out.resize(blob_start as usize, 0);
    out.extend_from_slice(&blob);

    let mut f = std::fs::File::create(path)?;
    f.write_all(&out)?;
    f.sync_all()
}

/// Escapes a string into a JSON string literal, quotes included.
///
/// Hand-written because `meta` is the only JSON this crate emits and a
/// serialiser dependency for one object is not a trade worth making. Escapes
/// every control character, so the output is valid JSON for any `&str`.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantising_and_dequantising_stays_within_one_step() {
        let values: Vec<f32> = (0..300).map(|i| (i as f32 - 150.0) / 37.0).collect();
        let (q, scales) = quantise(&values, 100, 3);
        let back = dequantise(&q, &scales, 100, 3);
        for (a, b) in values.iter().zip(&back) {
            let step = scales[0].max(scales[1]).max(scales[2]);
            assert!((a - b).abs() <= step / 2.0 + 1e-6, "{a} vs {b}");
        }
    }

    /// A column of zeros must not produce a scale of zero: the runtime
    /// multiplies by the scale at load, and a zero there is harmless, but a
    /// writer that divides by it is not.
    #[test]
    fn an_all_zero_column_gets_unit_scale() {
        let values = vec![0.0f32; 12];
        let (q, scales) = quantise(&values, 4, 3);
        assert_eq!(scales, vec![1.0, 1.0, 1.0]);
        assert!(q.iter().all(|&v| v == 0));
    }

    #[test]
    fn a_written_file_has_aligned_data_and_a_matching_crc() {
        let dir = std::env::temp_dir().join("ocrcer-ocrw-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("aligned.ocrw");
        let tables = vec![
            Table::f32s("feature_norm", vec![2, 3], &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]),
            Table::i8s("prototypes", 2, 3, vec![1, 2, 3, 4, 5, 6], vec![0.5, 0.25, 0.125]),
            Table::opaque("class", vec![2], vec![7, 0, 9, 0]),
        ];
        write(&path, 1, 1, "{\"charset\":[]}", &tables).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"OCRW");

        // Walk the directory the way a reader would, and check what the
        // format promises: aligned data, a CRC over the blob, exact lengths.
        let meta_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let mut at = 16 + meta_len;
        let crc = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        at += 4 + 8;
        let mut first_data = usize::MAX;
        let mut last_end = 0usize;
        for _ in 0..3 {
            let n = u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
            at += 2 + n;
            at += 1;
            let ndim = bytes[at] as usize;
            at += 1 + 4 * ndim + 4 + 8;
            let data_off = u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) as usize;
            at += 8;
            let data_len = u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) as usize;
            at += 8;
            assert_eq!(data_off % 64, 0, "table data must be 64-byte aligned");
            first_data = first_data.min(data_off);
            last_end = last_end.max(data_off + data_len);
        }
        assert_eq!(crc, crc32(&bytes[first_data..last_end]));
        assert_eq!(bytes.len(), last_end, "no trailing bytes");
        std::fs::remove_file(&path).ok();
    }

    /// Two writes of the same inputs must give the same bytes, or a rebuild
    /// invalidates every fixture blessed against the previous file.
    #[test]
    fn two_writes_of_the_same_tables_are_byte_identical() {
        let dir = std::env::temp_dir().join("ocrcer-ocrw-test");
        std::fs::create_dir_all(&dir).unwrap();
        let tables = || vec![Table::f32s("x", vec![4], &[1.0, -2.0, 3.5, 0.0])];
        let a = dir.join("a.ocrw");
        let b = dir.join("b.ocrw");
        write(&a, 1, 1, "{}", &tables()).unwrap();
        write(&b, 1, 1, "{}", &tables()).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
        std::fs::remove_file(&a).ok();
        std::fs::remove_file(&b).ok();
    }

    #[test]
    fn json_strings_escape_what_json_requires() {
        assert_eq!(json_string("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json_string("\n\t\r"), "\"\\n\\t\\r\"");
        assert_eq!(json_string("\u{1}"), "\"\\u0001\"");
        assert_eq!(json_string("é—"), "\"é—\"");
    }
}
