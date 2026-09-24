//! Writes the `.ocrl` LLM add-on container, in the same style as
//! `ocrw.rs` writes `.ocrw` (`ARCHITECTURE.md` section 7, section 11's
//! 2026-09-24 entry). Reuses `ocrcer_llm::container`'s format constants —
//! `Kind`, `Q8_BLOCK`, `crc32` — the same way `ocrw.rs` reuses
//! `ocrcer_core::ocrw`'s: the writer and the reader must never independently
//! reinvent what a discriminant or a block size means.

use std::io::Write;

pub use ocrcer_llm::container::{Kind, Q8_BLOCK};

pub struct Table {
    pub name: String,
    pub kind: Kind,
    pub dims: Vec<u32>,
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

    pub fn u32s(name: &str, dims: Vec<u32>, values: &[u32]) -> Table {
        let mut data = Vec::with_capacity(values.len() * 4);
        for v in values {
            data.extend_from_slice(&v.to_le_bytes());
        }
        Table { name: name.into(), kind: Kind::Opaque, dims, scales: Vec::new(), data }
    }

    pub fn opaque(name: &str, dims: Vec<u32>, data: Vec<u8>) -> Table {
        Table { name: name.into(), kind: Kind::Opaque, dims, scales: Vec::new(), data }
    }

    /// Quantises a flat `f32` slice (length a multiple of 32) into blocks of
    /// 32 `i8` values with one `f32` scale each: `scale = max|v| / 127` over
    /// the block, so the largest magnitude lands on the range's edge and
    /// nothing clips. A block that is all zeros gets scale `1.0`, never
    /// `0.0` — dequantising by zero would turn a harmless constant block
    /// into a division by zero at load.
    pub fn q8(name: &str, dims: Vec<u32>, values: &[f32]) -> Table {
        assert_eq!(values.len() % Q8_BLOCK, 0, "{name}: length must be a multiple of {Q8_BLOCK}");
        let n_blocks = values.len() / Q8_BLOCK;
        let mut scales = Vec::with_capacity(n_blocks);
        let mut data = Vec::with_capacity(values.len());
        for block in values.chunks_exact(Q8_BLOCK) {
            let peak = block.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let scale = if peak > 0.0 { peak / 127.0 } else { 1.0 };
            scales.push(scale);
            for v in block {
                let q = (f64::from(*v) / f64::from(scale)).round();
                data.push(q.clamp(-127.0, 127.0) as i8 as u8);
            }
        }
        Table { name: name.into(), kind: Kind::Q8, dims, scales, data }
    }

    fn directory_len(&self) -> u64 {
        2 + self.name.len() as u64 + 1 + 1 + 4 * self.dims.len() as u64 + 4 + 8 + 8 + 8
    }
}

pub use ocrcer_llm::container::crc32;

const ALIGN: u64 = 64;

fn pad_to(offset: u64) -> u64 {
    offset.div_ceil(ALIGN) * ALIGN
}

/// Writes an `.ocrl` file. `arch_kind` is `1` for the Qwen decoder family
/// (the only one this format's runtime implements); `meta` is the UTF-8 JSON
/// block carrying model id, upstream revision, architecture config,
/// quantisation, and the full licence text (`ARCHITECTURE.md` section 11).
pub fn write(path: &std::path::Path, version: u16, arch_kind: u16, meta: &str, tables: &[Table]) -> std::io::Result<()> {
    let header_len: u64 = 4 + 2 + 2 + 4 + 4 + meta.len() as u64 + 4 + 8;
    let directory_len: u64 = tables.iter().map(Table::directory_len).sum();

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
    out.extend_from_slice(b"OCRL");
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&arch_kind.to_le_bytes());
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
    fn a_written_file_round_trips_through_the_reader() {
        let dir = std::env::temp_dir().join("ocrcer-ocrl-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.ocrl");
        let values: Vec<f32> = (0..64).map(|i| (i as f32 - 32.0) / 5.0).collect();
        let tables = vec![
            Table::f32s("model.norm.weight", vec![4], &[1.0, 2.0, 3.0, 4.0]),
            Table::q8("model.layers.0.mlp.up_proj.weight", vec![2, 32], &values),
            Table::u32s("tok.byte_id", vec![4], &[1, 2, 3, 4]),
        ];
        write(&path, 1, 1, "{\"config\":{}}", &tables).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let c = ocrcer_llm::container::Container::load(&bytes).unwrap();
        assert_eq!(c.version, 1);
        let m = ocrcer_llm::tensor::Matrix::from_table(c.table("model.layers.0.mlp.up_proj.weight").unwrap()).unwrap();
        assert_eq!(m.out_dim(), 2);
        assert_eq!(m.in_dim(), 32);
        std::fs::remove_file(&path).ok();
    }
}
