//! `.ocrl` container: header, table directory, CRC check.
//!
//! Same style as `ocrcer-core`'s `.ocrw` reader (`ARCHITECTURE.md` section 7),
//! adapted for weight tensors instead of prototype tables. Little-endian
//! throughout, table data 64-byte aligned, a CRC-32 over the concatenated
//! table blob including interior padding.
//!
//! An unknown `version` is refused outright — there is no safe partial read
//! of a container whose table *meanings* may have changed. An unknown table
//! *name* is simply absent to a caller that does not ask for it, the same
//! asymmetry `.ocrw` documents and for the same reason.

use crate::json::Json;

pub const MAGIC: &[u8; 4] = b"OCRL";
pub const SUPPORTED_VERSION: u16 = 1;

#[derive(Debug)]
pub enum Error {
    BadMagic,
    UnsupportedVersion(u16),
    Truncated,
    MetaNotUtf8,
    Meta(crate::json::JsonError),
    TableNameNotUtf8,
    BadTable { name: String, why: &'static str },
    CrcMismatch { stored: u32, computed: u32 },
    MissingTable(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::BadMagic => write!(f, "not an .ocrl file (bad magic)"),
            Error::UnsupportedVersion(v) => write!(f, "unsupported .ocrl version {v}"),
            Error::Truncated => write!(f, "truncated .ocrl file"),
            Error::MetaNotUtf8 => write!(f, "meta block is not valid UTF-8"),
            Error::Meta(e) => write!(f, "meta JSON: {e}"),
            Error::TableNameNotUtf8 => write!(f, "a table name is not valid UTF-8"),
            Error::BadTable { name, why } => write!(f, "table {name:?}: {why}"),
            Error::CrcMismatch { stored, computed } => {
                write!(f, "blob CRC mismatch: file says {stored:#010x}, computed {computed:#010x}")
            }
            Error::MissingTable(name) => write!(f, "missing required table {name:?}"),
        }
    }
}

impl std::error::Error for Error {}

/// How a table's bytes are to be read. Discriminants are the on-disk values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    F32 = 0,
    /// Blocks of 32 `i8` values, one `f32` scale per block, flat over the
    /// row-major tensor. Every tensor this format quantises has an inner
    /// dimension that is a multiple of 32 (checked at write time), so a
    /// block never spans two rows.
    Q8 = 1,
    Opaque = 2,
}

impl Kind {
    pub fn from_u8(v: u8) -> Option<Kind> {
        match v {
            0 => Some(Kind::F32),
            1 => Some(Kind::Q8),
            2 => Some(Kind::Opaque),
            _ => None,
        }
    }
}

pub const Q8_BLOCK: usize = 32;

#[derive(Debug)]
pub struct RawTable<'a> {
    pub name: &'a str,
    pub kind: Kind,
    pub dims: Vec<u32>,
    /// One scale per 32-element block, present only for `Kind::Q8`.
    pub scales: Vec<f32>,
    pub data: &'a [u8],
}

impl RawTable<'_> {
    pub fn len(&self) -> usize {
        self.dims.iter().fold(1usize, |a, &d| a.saturating_mul(d as usize))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn f32s(&self) -> Result<Vec<f32>, Error> {
        if self.kind != Kind::F32 {
            return Err(Error::BadTable { name: self.name.into(), why: "expected an f32 table" });
        }
        if self.data.len() != self.len() * 4 {
            return Err(Error::BadTable { name: self.name.into(), why: "f32 data length disagrees with dims" });
        }
        Ok(self.data.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    }

    /// The `i8` values and per-block scales of a `Kind::Q8` table,
    /// undequantised: matmul dequantises one block at a time so that a
    /// multi-hundred-megabyte weight matrix does not cost 4x its file size in
    /// resident memory. Builds one owned `Vec<i8>` at load (a per-element
    /// cast, not a transmute — `forbid(unsafe_code)` binds this crate too),
    /// rather than reinterpreting the borrowed `&[u8]` in place, which the
    /// language gives no safe way to do.
    pub fn q8(&self) -> Result<(Vec<i8>, Vec<f32>), Error> {
        if self.kind != Kind::Q8 {
            return Err(Error::BadTable { name: self.name.into(), why: "expected a q8 table" });
        }
        if self.data.len() != self.len() {
            return Err(Error::BadTable { name: self.name.into(), why: "q8 data length disagrees with dims" });
        }
        let expect_blocks = self.len().div_ceil(Q8_BLOCK);
        if self.scales.len() != expect_blocks {
            return Err(Error::BadTable { name: self.name.into(), why: "scale count disagrees with block count" });
        }
        let data: Vec<i8> = self.data.iter().map(|&b| b as i8).collect();
        Ok((data, self.scales.clone()))
    }

    pub fn u32s(&self) -> Result<Vec<u32>, Error> {
        if self.data.len() != self.len() * 4 {
            return Err(Error::BadTable { name: self.name.into(), why: "u32 data length disagrees with dims" });
        }
        Ok(self.data.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    }

    pub fn bytes(&self) -> &[u8] {
        self.data
    }
}

/// A parsed `.ocrl` file: header, `meta`, and the table directory. Borrows
/// the file bytes.
pub struct Container<'a> {
    pub version: u16,
    pub meta_text: &'a str,
    pub meta: Json,
    pub tables: Vec<RawTable<'a>>,
}

struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.at.checked_add(n).ok_or(Error::Truncated)?;
        let s = self.b.get(self.at..end).ok_or(Error::Truncated)?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, Error> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&mut self) -> Result<u64, Error> {
        let s = self.take(8)?;
        Ok(u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
    }
}

fn slice_at(bytes: &[u8], off: u64, len: u64) -> Result<&[u8], Error> {
    let off = usize::try_from(off).map_err(|_| Error::Truncated)?;
    let len = usize::try_from(len).map_err(|_| Error::Truncated)?;
    let end = off.checked_add(len).ok_or(Error::Truncated)?;
    bytes.get(off..end).ok_or(Error::Truncated)
}

impl<'a> Container<'a> {
    pub fn load(bytes: &'a [u8]) -> Result<Container<'a>, Error> {
        let mut c = Cursor { b: bytes, at: 0 };
        if c.take(4)? != MAGIC {
            return Err(Error::BadMagic);
        }
        let version = c.u16()?;
        if version != SUPPORTED_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        let _arch_kind = c.u16()?;
        let n_tables = c.u32()? as usize;
        let meta_len = c.u32()? as usize;
        let meta_bytes = c.take(meta_len)?;
        let meta_text = std::str::from_utf8(meta_bytes).map_err(|_| Error::MetaNotUtf8)?;
        let blob_crc = c.u32()?;
        let _reserved = c.take(8)?;

        let mut tables = Vec::with_capacity(n_tables.min(1024));
        let mut blob_lo = usize::MAX;
        let mut blob_hi = 0usize;
        for _ in 0..n_tables {
            let name_len = c.u16()? as usize;
            let name = std::str::from_utf8(c.take(name_len)?).map_err(|_| Error::TableNameNotUtf8)?;
            let kind = Kind::from_u8(c.u8()?)
                .ok_or_else(|| Error::BadTable { name: name.into(), why: "unknown table kind" })?;
            let ndim = c.u8()? as usize;
            let mut dims = Vec::with_capacity(ndim);
            for _ in 0..ndim {
                dims.push(c.u32()?);
            }
            let n_scales = c.u32()? as usize;
            let scale_off = c.u64()?;
            let data_off = c.u64()?;
            let data_len = c.u64()?;

            let scale_bytes = slice_at(bytes, scale_off, (n_scales as u64) * 4)?;
            let scales = scale_bytes.chunks_exact(4).map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]])).collect();
            if data_off % 64 != 0 {
                return Err(Error::BadTable { name: name.into(), why: "table data is not 64-byte aligned" });
            }
            let data = slice_at(bytes, data_off, data_len)?;
            let lo = data_off as usize;
            blob_lo = blob_lo.min(lo);
            blob_hi = blob_hi.max(lo + data.len());

            tables.push(RawTable { name, kind, dims, scales, data });
        }

        let computed = if blob_lo == usize::MAX {
            crc32(&[])
        } else {
            crc32(bytes.get(blob_lo..blob_hi).ok_or(Error::Truncated)?)
        };
        if computed != blob_crc {
            return Err(Error::CrcMismatch { stored: blob_crc, computed });
        }

        let meta = Json::parse(meta_text).map_err(Error::Meta)?;
        Ok(Container { version, meta_text, meta, tables })
    }

    pub fn table(&self, name: &str) -> Option<&RawTable<'a>> {
        self.tables.iter().find(|t| t.name == name)
    }

    pub fn need(&self, name: &'static str) -> Result<&RawTable<'a>, Error> {
        self.table(name).ok_or(Error::MissingTable(name))
    }
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}
