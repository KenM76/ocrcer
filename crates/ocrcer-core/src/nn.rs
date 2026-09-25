//! The optional `nn` table: a small convolutional glyph classifier, trained
//! outside this crate by `tools/nn/` and shipped, at the operator's option,
//! alongside the prototype bank (`ARCHITECTURE.md` section 11, the
//! 2026-09-24 neural-classifier decision and its 2026-09-25 amendments).
//!
//! # Contract
//!
//! Read-only: parses `meta.nn` and the `nn` table and dequantises every
//! weight to `f32` once at load, the same as the prototype bank (section 7).
//! It hands back plain tensors and the layer spec; it does **not** run the
//! network. The forward pass is a separate piece of work and lives
//! elsewhere in this crate.
//!
//! **The one rule that makes this table safe to add:** nothing here can fail
//! [`crate::ocrw::Model::load`]. An `nn_version` this build does not know, a
//! malformed `nn` table, or a `meta.nn` that disagrees with the table it
//! sits beside all degrade to [`NnStatus::UnsupportedVersion`] or
//! [`NnStatus::Malformed`] with [`Model::nn`](crate::ocrw::Model) left
//! `None` -- never to a load error. A caller that wants to know why reads
//! [`Model::nn_status`](crate::ocrw::Model); a caller that only wants a
//! recogniser never has to.
//!
//! `#![forbid(unsafe_code)]` and zero dependencies hold here as everywhere
//! else in this crate (`CLAUDE.md` rule 3); this module adds nothing beyond
//! `crate::json`.

use crate::json::Json;
use crate::ocrw::{Container, RawTable};

/// The `nn_version` this build's forward pass and this parser agree on. A
/// file declaring anything else is not read (section 7's `version` rule,
/// narrowed to this one table).
pub const SUPPORTED_NN_VERSION: u32 = 1;

/// The table name the writer uses.
pub const T_NN: &str = "nn";

/// One layer kind, per the 2026-09-25 chunk 15 interfaces entry. A layer's
/// weight and bias are populated only for [`Conv3x3`](LayerKind::Conv3x3)
/// and [`Dense`](LayerKind::Dense); every other kind carries no parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Conv3x3,
    Relu,
    MaxPool2,
    Flatten,
    /// Concatenates the 107-dim standardised feature vector onto the
    /// flattened conv output, at the dense head.
    ConcatFeatures,
    Dense,
}

impl LayerKind {
    /// Parses a `meta.nn.layers[i].kind` string. `pub` so `ocrcer-build`'s
    /// writer (`crates/ocrcer-build/src/nn.rs`) reads the same vocabulary
    /// this parser does, rather than keeping an independent list that could
    /// drift from it (`CLAUDE.md` rule 4).
    pub fn parse(s: &str) -> Option<LayerKind> {
        Some(match s {
            "conv3x3" => LayerKind::Conv3x3,
            "relu" => LayerKind::Relu,
            "maxpool2" => LayerKind::MaxPool2,
            "flatten" => LayerKind::Flatten,
            "concat_features" => LayerKind::ConcatFeatures,
            "dense" => LayerKind::Dense,
            _ => return None,
        })
    }

    /// Whether this layer kind carries a weight/bias pair in the `nn` table.
    /// `pub` for the same reason as [`LayerKind::parse`]: the writer decides
    /// which layers to expect tensor files for from this, not from a second
    /// hardcoded list.
    pub fn has_params(self) -> bool {
        matches!(self, LayerKind::Conv3x3 | LayerKind::Dense)
    }
}

/// One layer of the network, in the order the trainer emitted it.
///
/// `shape` is the weight tensor's shape as the trainer recorded it:
/// `[out, in, 3, 3]` for [`Conv3x3`](LayerKind::Conv3x3), `[out, in]` for
/// [`Dense`](LayerKind::Dense), and whatever the trainer chose to record for
/// a parameter-free layer (informational only; this parser does not
/// interpret it).
#[derive(Debug, Clone)]
pub struct Layer {
    pub kind: LayerKind,
    pub shape: Vec<u32>,
    /// Dequantised weight, row-major `[out][in]` with `in` already
    /// flattened (`in_channels * 9` for a conv layer, `in_features` for a
    /// dense one). Empty for a layer with no learned parameters.
    pub weight: Vec<f32>,
    /// One bias per output channel, stored as `f32` in the file (never
    /// quantised -- section 7's int8 saving is on the weight matrices,
    /// which dwarf the bias vectors). Empty for a layer with no learned
    /// parameters.
    pub bias: Vec<f32>,
}

impl Layer {
    /// The output width: `shape[0]` for a layer with parameters, or `None`
    /// for one without (its output width is whatever the previous layer's
    /// was, which this module does not track).
    pub fn out_dim(&self) -> Option<usize> {
        self.kind.has_params().then(|| self.shape.first().copied().unwrap_or(0) as usize)
    }
}

/// The parsed, dequantised network: every tensor as `f32`, ready for a
/// forward pass to consume.
#[derive(Debug, Clone)]
pub struct Nn {
    pub nn_version: u32,
    /// The output index that means "not a character" (charset length).
    pub junk_index: u32,
    /// `junk_index + 1`: the width of the final dense layer's output.
    pub n_outputs: u32,
    pub layers: Vec<Layer>,
}

/// Why [`Model::nn`](crate::ocrw::Model) is what it is. `Loaded` is the only
/// variant that pairs with `Some`; every other variant means the file is
/// read as if it carried no `nn` table at all.
#[derive(Debug, Clone, PartialEq)]
pub enum NnStatus {
    /// The file carries neither `meta.nn` nor an `nn` table. The ordinary
    /// case: most files predate chunk 15, or were built without `--nn`.
    Absent,
    /// `meta.nn.nn_version` is not [`SUPPORTED_NN_VERSION`]. The table's
    /// bytes may mean anything under that version; they are not read.
    UnsupportedVersion(u32),
    /// `meta.nn` and/or the `nn` table are present but malformed, or
    /// disagree with each other. The reason is a plain-English sentence,
    /// never a struct, because nothing downstream branches on which
    /// malformation this was -- only a person reading a report does.
    Malformed(String),
    /// The network loaded and dequantised cleanly.
    Loaded,
}

impl core::fmt::Display for NnStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            NnStatus::Absent => write!(f, "no nn table in this file"),
            NnStatus::UnsupportedVersion(v) => {
                write!(f, "nn table is version {v}, this build reads version {SUPPORTED_NN_VERSION}; falling back to prototypes")
            }
            NnStatus::Malformed(why) => write!(f, "nn table malformed ({why}); falling back to prototypes"),
            NnStatus::Loaded => write!(f, "loaded"),
        }
    }
}

/// Magic the writer stamps at the start of the `nn` table's blob.
const MAGIC: &[u8; 4] = b"NNET";

/// Loads the `nn` table, if present, per the contract above: this never
/// returns an `Err` a caller has to propagate. The `Result` is internal
/// plumbing only.
pub(crate) fn load(c: &Container) -> (Option<Nn>, NnStatus) {
    let meta_nn = c.meta.get("nn");
    let table = c.table(T_NN);
    match (meta_nn, table) {
        (None, None) => (None, NnStatus::Absent),
        (None, Some(_)) => {
            (None, NnStatus::Malformed("an nn table is present but meta.nn is missing".into()))
        }
        (Some(_), None) => {
            (None, NnStatus::Malformed("meta.nn is present but there is no nn table".into()))
        }
        (Some(m), Some(t)) => match parse(m, t) {
            Ok(nn) => (Some(nn), NnStatus::Loaded),
            Err(ParseErr::UnsupportedVersion(v)) => (None, NnStatus::UnsupportedVersion(v)),
            Err(ParseErr::Malformed(why)) => (None, NnStatus::Malformed(why)),
        },
    }
}

enum ParseErr {
    UnsupportedVersion(u32),
    Malformed(String),
}

fn bad(why: impl Into<String>) -> ParseErr {
    ParseErr::Malformed(why.into())
}

fn parse(meta: &Json, t: &RawTable<'_>) -> Result<Nn, ParseErr> {
    let nn_version =
        meta.get("nn_version").and_then(Json::as_u32).ok_or_else(|| bad("meta.nn missing nn_version"))?;
    if nn_version != SUPPORTED_NN_VERSION {
        return Err(ParseErr::UnsupportedVersion(nn_version));
    }
    let junk_index =
        meta.get("junk_index").and_then(Json::as_u32).ok_or_else(|| bad("meta.nn missing junk_index"))?;
    let n_outputs =
        meta.get("n_outputs").and_then(Json::as_u32).ok_or_else(|| bad("meta.nn missing n_outputs"))?;
    if n_outputs != junk_index + 1 {
        return Err(bad("meta.nn n_outputs must be junk_index + 1"));
    }
    let layers_meta = meta
        .get("layers")
        .and_then(Json::as_array)
        .ok_or_else(|| bad("meta.nn missing layers"))?;
    if layers_meta.is_empty() {
        return Err(bad("meta.nn.layers is empty"));
    }
    let mut specs: Vec<(LayerKind, Vec<u32>)> = Vec::with_capacity(layers_meta.len());
    for (i, l) in layers_meta.iter().enumerate() {
        let kind_s = l.get("kind").and_then(Json::as_str).ok_or_else(|| bad(format!("layer {i} missing kind")))?;
        let kind = LayerKind::parse(kind_s).ok_or_else(|| bad(format!("layer {i} has unknown kind {kind_s:?}")))?;
        let shape: Vec<u32> = l
            .get("shape")
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(Json::as_u32).collect())
            .unwrap_or_default();
        specs.push((kind, shape));
    }

    if t.data.len() < 4 + 2 + 2 + 4 {
        return Err(bad("nn table shorter than its own header"));
    }
    if &t.data[0..4] != MAGIC {
        return Err(bad("nn table does not start with the NNET magic"));
    }
    let blob_version = u16::from_le_bytes([t.data[4], t.data[5]]);
    if u32::from(blob_version) != nn_version {
        return Err(bad("nn table's internal version disagrees with meta.nn.nn_version"));
    }
    let n_weighted = u32::from_le_bytes([t.data[8], t.data[9], t.data[10], t.data[11]]) as usize;

    let expected_weighted = specs.iter().filter(|(k, _)| k.has_params()).count();
    if n_weighted != expected_weighted {
        return Err(bad("nn table's layer count disagrees with meta.nn.layers"));
    }

    let mut cur = Cursor { b: t.data, at: 12 };
    let mut scale_at = 0usize;
    let mut layers = Vec::with_capacity(specs.len());
    for (idx, (kind, shape)) in specs.into_iter().enumerate() {
        if !kind.has_params() {
            layers.push(Layer { kind, shape, weight: Vec::new(), bias: Vec::new() });
            continue;
        }
        let layer_index = cur.u32().ok_or_else(|| bad("nn table truncated in a layer record"))?;
        if layer_index as usize != idx {
            return Err(bad("nn table layer_index disagrees with meta.nn.layers order"));
        }
        let out_dim = cur.u32().ok_or_else(|| bad("nn table truncated reading out_dim"))? as usize;
        let in_dim = cur.u32().ok_or_else(|| bad("nn table truncated reading in_dim"))? as usize;
        let expect_out = *shape.first().unwrap_or(&0) as usize;
        let expect_in: usize = match kind {
            LayerKind::Conv3x3 => shape.get(1..4).map(|s| s.iter().product::<u32>() as usize).unwrap_or(0),
            LayerKind::Dense => shape.get(1).copied().unwrap_or(0) as usize,
            _ => unreachable!("has_params() only admits Conv3x3 and Dense"),
        };
        if out_dim != expect_out || in_dim != expect_in {
            return Err(bad(format!("layer {idx} shape disagrees with its recorded out/in dims")));
        }
        let n = out_dim.checked_mul(in_dim).ok_or_else(|| bad("layer dims overflow"))?;
        let wbytes = cur.take(n).ok_or_else(|| bad(format!("nn table truncated in layer {idx} weight")))?;
        let bbytes =
            cur.take(out_dim * 4).ok_or_else(|| bad(format!("nn table truncated in layer {idx} bias")))?;

        let scales = t
            .scales
            .get(scale_at..scale_at + out_dim)
            .ok_or_else(|| bad(format!("layer {idx} has no scale for every output channel")))?;
        scale_at += out_dim;

        let mut weight = Vec::with_capacity(n);
        for r in 0..out_dim {
            let row = &wbytes[r * in_dim..(r + 1) * in_dim];
            for &b in row {
                weight.push(f32::from(b as i8) * scales[r]);
            }
        }
        let bias: Vec<f32> = bbytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        layers.push(Layer { kind, shape, weight, bias });
    }
    if scale_at != t.scales.len() {
        return Err(bad("nn table carries more scales than any layer uses"));
    }
    if cur.at != t.data.len() {
        return Err(bad("nn table has trailing bytes after its last layer"));
    }

    Ok(Nn { nn_version, junk_index, n_outputs, layers })
}

struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.b.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        let s = self.take(4)?;
        Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal, well-formed `nn` table by hand -- one conv layer,
    /// one relu, one dense layer -- and checks the parser reads back the
    /// exact values a writer with these scales would have quantised.
    fn hand_built() -> (String, Vec<u8>, Vec<f32>) {
        // conv3x3: out=2, in=1*3*3=9 -> weight 2x9, bias 2
        // dense:   out=3, in=2       -> weight 3x2, bias 3
        let conv_w: [i8; 18] = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, -1, -2, -3, -4, -5, -6, -7, -8, -9,
        ];
        let conv_scale = [0.1f32, 0.2f32];
        let conv_bias = [0.5f32, -0.5f32];
        let dense_w: [i8; 6] = [10, -20, 30, -40, 50, -60];
        let dense_scale = [0.01f32, 0.02f32, 0.03f32];
        let dense_bias = [1.0f32, 2.0f32, 3.0f32];

        let mut data = Vec::new();
        data.extend_from_slice(MAGIC);
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes()); // n_weighted layers

        // layer 0: conv3x3
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&9u32.to_le_bytes());
        data.extend(conv_w.iter().map(|&v| v as u8));
        for b in conv_bias {
            data.extend_from_slice(&b.to_le_bytes());
        }
        // layer 2: dense (layer 1 is relu, no params)
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend(dense_w.iter().map(|&v| v as u8));
        for b in dense_bias {
            data.extend_from_slice(&b.to_le_bytes());
        }

        let mut scales = Vec::new();
        scales.extend_from_slice(&conv_scale);
        scales.extend_from_slice(&dense_scale);

        let meta = r#"{"nn":{"nn_version":1,"junk_index":3,"n_outputs":4,"layers":[
            {"kind":"conv3x3","shape":[2,1,3,3]},
            {"kind":"relu"},
            {"kind":"dense","shape":[3,2]}
        ]}}"#
            .to_string();

        (meta, data, scales)
    }

    fn table<'a>(data: &'a [u8], scales: &'a [f32]) -> RawTable<'a> {
        RawTable {
            name: T_NN,
            kind: crate::ocrw::Kind::Opaque,
            dims: vec![data.len() as u32],
            scales: scales.to_vec(),
            data,
        }
    }

    #[test]
    fn a_hand_built_nn_table_round_trips_within_its_own_quantisation() {
        let (meta_text, data, scales) = hand_built();
        let meta = Json::parse(&meta_text).unwrap();
        let t = table(&data, &scales);
        let (nn, status) = load(&Container {
            version: 1,
            model_kind: 1,
            meta_text: &meta_text,
            meta: meta.clone(),
            tables: vec![RawTable { name: T_NN, kind: t.kind, dims: t.dims.clone(), scales: t.scales.clone(), data: t.data }],
        });
        assert_eq!(status, NnStatus::Loaded);
        let nn = nn.unwrap();
        assert_eq!(nn.junk_index, 3);
        assert_eq!(nn.n_outputs, 4);
        assert_eq!(nn.layers.len(), 3);
        assert_eq!(nn.layers[0].kind, LayerKind::Conv3x3);
        assert_eq!(nn.layers[0].weight.len(), 18);
        assert_eq!(nn.layers[0].bias, vec![0.5, -0.5]);
        // row 0 used scale 0.1: values 1..9 * 0.1
        assert!((nn.layers[0].weight[0] - 0.1).abs() < 1e-6);
        assert!((nn.layers[0].weight[8] - 0.9).abs() < 1e-6);
        // row 1 used scale 0.2: values -1..-9 * 0.2
        assert!((nn.layers[0].weight[9] - (-0.2)).abs() < 1e-6);
        assert_eq!(nn.layers[1].kind, LayerKind::Relu);
        assert!(nn.layers[1].weight.is_empty());
        assert_eq!(nn.layers[2].kind, LayerKind::Dense);
        assert_eq!(nn.layers[2].bias, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn an_unsupported_nn_version_falls_back_without_failing() {
        let (meta_text, data, scales) = hand_built();
        let meta_text = meta_text.replacen("\"nn_version\":1", "\"nn_version\":99", 1);
        let meta = Json::parse(&meta_text).unwrap();
        let t = table(&data, &scales);
        let (nn, status) = load(&Container {
            version: 1,
            model_kind: 1,
            meta_text: &meta_text,
            meta,
            tables: vec![RawTable { name: T_NN, kind: t.kind, dims: t.dims.clone(), scales: t.scales.clone(), data: t.data }],
        });
        assert!(nn.is_none());
        assert_eq!(status, NnStatus::UnsupportedVersion(99));
    }

    #[test]
    fn no_nn_table_is_absent_not_an_error() {
        let meta = Json::parse("{}").unwrap();
        let (nn, status) = load(&Container { version: 1, model_kind: 1, meta_text: "{}", meta, tables: vec![] });
        assert!(nn.is_none());
        assert_eq!(status, NnStatus::Absent);
    }

    #[test]
    fn a_truncated_nn_table_is_malformed_not_a_panic() {
        let (meta_text, data, scales) = hand_built();
        let meta = Json::parse(&meta_text).unwrap();
        for cut in 0..data.len() {
            let d = &data[..cut];
            let t = table(d, &scales);
            let (nn, status) = load(&Container {
                version: 1,
                model_kind: 1,
                meta_text: &meta_text,
                meta: meta.clone(),
                tables: vec![RawTable { name: T_NN, kind: t.kind, dims: t.dims.clone(), scales: t.scales.clone(), data: t.data }],
            });
            if cut < data.len() {
                assert!(nn.is_none(), "cut {cut} should not have parsed");
                assert_ne!(status, NnStatus::Loaded, "cut {cut} should not report Loaded");
            }
        }
    }
}
