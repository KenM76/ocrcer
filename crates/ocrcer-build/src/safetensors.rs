//! A minimal `safetensors` reader: the 8-byte little-endian header length,
//! the JSON header, then the raw tensor bytes. No new dependency — the JSON
//! header is parsed with `ocrcer_core::json::Json`, the same reader `.ocrw`
//! uses for `meta`.
//!
//! Only `BF16` and `F32` tensors are understood, which is what upstream Qwen
//! ships (`torch_dtype: bfloat16`); anything else is refused rather than
//! silently misread.

use ocrcer_core::json::Json;
use ocrcer_llm::tensor::bf16_to_f32;

pub struct SafeTensors<'a> {
    data: &'a [u8],
    header: Json,
    data_start: usize,
}

#[derive(Debug, Clone)]
pub struct TensorInfo {
    pub dtype: String,
    pub shape: Vec<usize>,
    pub start: usize,
    pub end: usize,
}

impl<'a> SafeTensors<'a> {
    pub fn parse(data: &'a [u8]) -> Result<SafeTensors<'a>, String> {
        if data.len() < 8 {
            return Err("safetensors file shorter than its own header length field".into());
        }
        let header_len = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
        let header_bytes = data.get(8..8 + header_len).ok_or("safetensors header length exceeds file size")?;
        let header_text = std::str::from_utf8(header_bytes).map_err(|e| format!("safetensors header not UTF-8: {e}"))?;
        let header = Json::parse(header_text).map_err(|e| format!("safetensors header JSON: {e}"))?;
        Ok(SafeTensors { data, header, data_start: 8 + header_len })
    }

    /// Every tensor name in the header, `__metadata__` excluded, in the
    /// order the header lists them (safetensors headers are JSON objects and
    /// this crate's `Json` keeps insertion order).
    pub fn names(&self) -> Vec<&str> {
        self.header.as_object().map_or(Vec::new(), |fields| {
            fields.iter().filter(|(k, _)| k != "__metadata__").map(|(k, _)| k.as_str()).collect()
        })
    }

    pub fn info(&self, name: &str) -> Result<TensorInfo, String> {
        let t = self.header.get(name).ok_or_else(|| format!("no such tensor: {name}"))?;
        let dtype = t.get("dtype").and_then(Json::as_str).ok_or_else(|| format!("{name}: missing dtype"))?.to_string();
        let shape: Vec<usize> = t
            .get("shape")
            .and_then(Json::as_array)
            .ok_or_else(|| format!("{name}: missing shape"))?
            .iter()
            .map(|v| v.as_u32().map(|u| u as usize).ok_or_else(|| format!("{name}: bad shape entry")))
            .collect::<Result<_, _>>()?;
        let offsets = t.get("data_offsets").and_then(Json::as_array).ok_or_else(|| format!("{name}: missing data_offsets"))?;
        if offsets.len() != 2 {
            return Err(format!("{name}: data_offsets must have two entries"));
        }
        let start = offsets[0].as_u32().ok_or_else(|| format!("{name}: bad data_offsets[0]"))? as usize;
        let end = offsets[1].as_u32().ok_or_else(|| format!("{name}: bad data_offsets[1]"))? as usize;
        Ok(TensorInfo { dtype, shape, start, end })
    }

    /// Reads a tensor's values as `f32`, widening `BF16` (the top 16 bits of
    /// an `f32`) or passing `F32` through unchanged.
    pub fn read_f32(&self, name: &str) -> Result<(Vec<usize>, Vec<f32>), String> {
        let info = self.info(name)?;
        let lo = self.data_start.checked_add(info.start).ok_or("offset overflow")?;
        let hi = self.data_start.checked_add(info.end).ok_or("offset overflow")?;
        let bytes = self.data.get(lo..hi).ok_or_else(|| format!("{name}: data range out of bounds"))?;
        let values: Vec<f32> = match info.dtype.as_str() {
            "BF16" => {
                if bytes.len() % 2 != 0 {
                    return Err(format!("{name}: BF16 data length is not a multiple of 2"));
                }
                bytes.chunks_exact(2).map(|c| bf16_to_f32(u16::from_le_bytes([c[0], c[1]]))).collect()
            }
            "F32" => {
                if bytes.len() % 4 != 0 {
                    return Err(format!("{name}: F32 data length is not a multiple of 4"));
                }
                bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
            }
            other => return Err(format!("{name}: unsupported dtype {other} (only BF16 and F32 are read)")),
        };
        let expect: usize = info.shape.iter().product();
        if values.len() != expect {
            return Err(format!("{name}: tensor element count disagrees with its shape"));
        }
        Ok((info.shape, values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(entries: &[(&str, &str, Vec<usize>, Vec<u8>)]) -> Vec<u8> {
        let mut header = String::from("{");
        let mut blob = Vec::new();
        for (i, (name, dtype, shape, data)) in entries.iter().enumerate() {
            if i > 0 {
                header.push(',');
            }
            let shape_str: Vec<String> = shape.iter().map(|d| d.to_string()).collect();
            header.push_str(&format!(
                "\"{name}\":{{\"dtype\":\"{dtype}\",\"shape\":[{}],\"data_offsets\":[{},{}]}}",
                shape_str.join(","),
                blob.len(),
                blob.len() + data.len()
            ));
            blob.extend_from_slice(data);
        }
        header.push('}');
        let mut out = Vec::new();
        out.extend_from_slice(&(header.len() as u64).to_le_bytes());
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(&blob);
        out
    }

    #[test]
    fn reads_f32_and_bf16_tensors() {
        let f32_bytes: Vec<u8> = [1.0f32, -2.5f32].iter().flat_map(|v| v.to_le_bytes()).collect();
        // bf16 for 1.0 is 0x3F80, for -2.0 is 0xC000.
        let bf16_bytes: Vec<u8> = [0x3F80u16, 0xC000u16].iter().flat_map(|v| v.to_le_bytes()).collect();
        let file = build(&[("a", "F32", vec![2], f32_bytes), ("b", "BF16", vec![2], bf16_bytes)]);
        let st = SafeTensors::parse(&file).unwrap();
        let (shape_a, vals_a) = st.read_f32("a").unwrap();
        assert_eq!(shape_a, vec![2]);
        assert_eq!(vals_a, vec![1.0, -2.5]);
        let (_, vals_b) = st.read_f32("b").unwrap();
        assert_eq!(vals_b, vec![1.0, -2.0]);
    }

    #[test]
    fn unknown_dtype_is_refused() {
        let file = build(&[("a", "I64", vec![1], vec![0; 8])]);
        let st = SafeTensors::parse(&file).unwrap();
        assert!(st.read_f32("a").is_err());
    }
}
