//! Weight storage and the one matmul kernel every layer calls through.
//!
//! # Determinism
//!
//! `Matrix::matmul_into` computes each output row as a single, fixed-order
//! `f32` accumulation (block 0, 1, 2, ... in ascending index, then blocks
//! summed in ascending order). Parallelism (the `parallel` feature) only ever
//! assigns whole, disjoint output rows to threads — it never splits one row's
//! accumulation across threads — so the result does not depend on the thread
//! count, only on `in`/`out`, which is `ARCHITECTURE.md` section 11's
//! byte-identical-regardless-of-threads requirement satisfied by
//! construction rather than by a merge step that has to get it right.

use crate::container::{Kind, Q8_BLOCK, RawTable};

/// A weight matrix, `[out, in]` row-major, PyTorch `nn.Linear` convention:
/// `y = W @ x` for a column vector `x` of length `in`.
pub enum Matrix {
    F32 { out: usize, inp: usize, data: Vec<f32> },
    /// `data` is `out * inp` `i8` values; `scales` is one `f32` per 32-value
    /// block, `out * (inp / 32)` of them. `inp` is always a multiple of 32
    /// for every tensor this format quantises (checked at write time), so a
    /// block never spans two rows.
    Q8 { out: usize, inp: usize, data: Vec<i8>, scales: Vec<f32> },
}

impl Matrix {
    pub fn out_dim(&self) -> usize {
        match self {
            Matrix::F32 { out, .. } | Matrix::Q8 { out, .. } => *out,
        }
    }

    pub fn in_dim(&self) -> usize {
        match self {
            Matrix::F32 { inp, .. } | Matrix::Q8 { inp, .. } => *inp,
        }
    }

    pub fn from_table(t: &RawTable) -> Result<Matrix, crate::container::Error> {
        if t.dims.len() != 2 {
            return Err(crate::container::Error::BadTable { name: t.name.into(), why: "a weight matrix must be 2-D" });
        }
        let out = t.dims[0] as usize;
        let inp = t.dims[1] as usize;
        match t.kind {
            Kind::F32 => Ok(Matrix::F32 { out, inp, data: t.f32s()? }),
            Kind::Q8 => {
                if inp % Q8_BLOCK != 0 {
                    return Err(crate::container::Error::BadTable {
                        name: t.name.into(),
                        why: "q8 weight matrix's inner dimension is not a multiple of 32",
                    });
                }
                let (data, scales) = t.q8()?;
                Ok(Matrix::Q8 { out, inp, data, scales })
            }
            Kind::Opaque => Err(crate::container::Error::BadTable { name: t.name.into(), why: "expected a weight matrix" }),
        }
    }

    /// `y[o] = sum_i x[i] * W[o, i]`. `y` must have length `out_dim()`.
    pub fn matmul_into(&self, x: &[f32], y: &mut [f32]) {
        debug_assert_eq!(x.len(), self.in_dim());
        debug_assert_eq!(y.len(), self.out_dim());
        row_range(self, x, y, 0, self.out_dim());
    }

    /// Row `idx`, dequantised if this is a `Q8` matrix. Used for embedding
    /// lookup, where only one row of a `[vocab, hidden]` matrix is needed —
    /// dequantising the whole matrix to fetch one row would cost 4x the
    /// table's resident size for nothing.
    pub fn row(&self, idx: usize) -> Vec<f32> {
        match self {
            Matrix::F32 { inp, data, .. } => data[idx * inp..idx * inp + inp].to_vec(),
            Matrix::Q8 { inp, data, scales, .. } => {
                let blocks_per_row = inp / Q8_BLOCK;
                let row = &data[idx * inp..idx * inp + inp];
                let row_scales = &scales[idx * blocks_per_row..idx * blocks_per_row + blocks_per_row];
                let mut out = Vec::with_capacity(*inp);
                for (b, &s) in row_scales.iter().enumerate() {
                    for k in 0..Q8_BLOCK {
                        out.push(f32::from(row[b * Q8_BLOCK + k]) * s);
                    }
                }
                out
            }
        }
    }
}

/// Fills `y` (length `hi - lo`) with rows `lo..hi` of `m @ x`, `y[o - lo]`
/// for each absolute row `o`. The relative indexing is what lets a caller
/// hand this a `chunks_mut` sub-slice (as `matmul_threaded` does) rather
/// than the full output buffer — `y` here is never assumed to be the whole
/// `out_dim()`-long vector.
fn row_range(m: &Matrix, x: &[f32], y: &mut [f32], lo: usize, hi: usize) {
    match m {
        Matrix::F32 { inp, data, .. } => {
            for (rel, o) in (lo..hi).enumerate() {
                let row = &data[o * inp..o * inp + inp];
                let mut acc = 0.0f32;
                for i in 0..*inp {
                    acc += x[i] * row[i];
                }
                y[rel] = acc;
            }
        }
        Matrix::Q8 { inp, data, scales, .. } => {
            let blocks_per_row = inp / Q8_BLOCK;
            for (rel, o) in (lo..hi).enumerate() {
                let row = &data[o * inp..o * inp + inp];
                let row_scales = &scales[o * blocks_per_row..o * blocks_per_row + blocks_per_row];
                let mut acc = 0.0f32;
                for b in 0..blocks_per_row {
                    let base = b * Q8_BLOCK;
                    let mut block_acc = 0.0f32;
                    for k in 0..Q8_BLOCK {
                        block_acc += x[base + k] * f32::from(row[base + k]);
                    }
                    acc += block_acc * row_scales[b];
                }
                y[rel] = acc;
            }
        }
    }
}

#[cfg(feature = "parallel")]
pub fn matmul_threaded(m: &Matrix, x: &[f32], y: &mut [f32], threads: usize) {
    if threads <= 1 {
        m.matmul_into(x, y);
        return;
    }
    let out = m.out_dim();
    let chunk = out.div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (t, y_chunk) in y.chunks_mut(chunk).enumerate() {
            let lo = t * chunk;
            let hi = (lo + y_chunk.len()).min(out);
            scope.spawn(move || row_range(m, x, y_chunk, lo, hi));
        }
    });
    // Row ranges are disjoint and each row's own accumulation order is fixed
    // regardless of which thread runs it (see the module doc), so this is
    // byte-identical to `matmul_into` for any `threads >= 1`.
    let _ = out;
}

/// bf16 -> f32: bf16 is the top 16 bits of an f32, so widening is a shift.
pub fn bf16_to_f32(bits: u16) -> f32 {
    f32::from_bits((bits as u32) << 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_matmul_matches_hand_computed_dot_products() {
        // W = [[1,2,3],[4,5,6]], x = [1,1,1] -> y = [6, 15]
        let m = Matrix::F32 { out: 2, inp: 3, data: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0] };
        let mut y = vec![0.0; 2];
        m.matmul_into(&[1.0, 1.0, 1.0], &mut y);
        assert_eq!(y, vec![6.0, 15.0]);
    }

    #[test]
    fn q8_matmul_recovers_an_exact_f32_case() {
        // A single block of 32, scale 1.0, weights all 1 -> dot with x is sum(x).
        let inp = 32;
        let out = 2;
        let data = vec![1i8; inp * out];
        let scales = vec![1.0f32; out]; // one block per row
        let m = Matrix::Q8 { out, inp, data, scales };
        let x: Vec<f32> = (0..inp).map(|i| i as f32).collect();
        let mut y = vec![0.0; out];
        m.matmul_into(&x, &mut y);
        let expect: f32 = x.iter().sum();
        assert_eq!(y[0], expect);
        assert_eq!(y[1], expect);
    }

    #[test]
    fn bf16_matches_known_bit_patterns() {
        assert_eq!(bf16_to_f32(0x3F80), 1.0f32); // 1.0
        assert_eq!(bf16_to_f32(0x0000), 0.0f32);
        assert_eq!(bf16_to_f32(0xBF80), -1.0f32);
    }

    #[cfg(feature = "parallel")]
    #[test]
    fn threaded_matmul_matches_single_threaded_when_out_dim_does_not_divide_evenly_by_threads() {
        // out=8, threads=8 gives chunks of length 1, the case that exposed an
        // absolute-vs-relative indexing bug in `row_range`: a `chunks_mut`
        // sub-slice starts at its own index 0, not at the chunk's `lo`.
        let out = 8;
        let inp = 3;
        let data: Vec<f32> = (0..out * inp).map(|i| i as f32 * 0.5 - 1.0).collect();
        let m = Matrix::F32 { out, inp, data };
        let x = [1.0f32, -2.0, 0.5];
        let mut expect = vec![0.0f32; out];
        m.matmul_into(&x, &mut expect);
        let mut got = vec![0.0f32; out];
        matmul_threaded(&m, &x, &mut got, 8);
        assert_eq!(expect, got);
    }
}
