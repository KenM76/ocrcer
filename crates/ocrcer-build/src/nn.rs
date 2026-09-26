//! Reads a trainer output directory and writes the `nn` table and its
//! `meta.nn` block (`ARCHITECTURE.md` section 11, the 2026-09-25 chunk 15
//! interfaces entry). The trainer itself -- and the forward pass that reads
//! what this module writes -- are separate work; this module only packages
//! what the trainer already produced.
//!
//! # The trainer output directory
//!
//! - `spec.json`: one JSON object with `nn_version`, `junk_index`,
//!   `n_outputs`, `layers` (in forward-pass order, each with a `kind` and,
//!   for a weighted layer, a full tensor shape) and the trainer's
//!   provenance: `charset_sha256`, `feature_extractor`, a training manifest
//!   id, seed, torch and Python versions, and the lock file's hash. Only the
//!   *meaning* of these fields is fixed by `ARCHITECTURE.md` section 11, not
//!   their literal JSON keys -- `tools/nn/train.py` (the trainer that
//!   shipped) names the weighted-layer shape `weight_shape`, the manifest id
//!   `training_manifest_id` and the lock hash `lock_file_sha256`; this
//!   module's [`load_spec`] accepts those names (falling back to `shape`,
//!   `manifest_id`, `lock_hash` for this module's own pre-trainer test
//!   fixtures). The same shape [`meta_json`] emits into `meta.nn`, under
//!   this module's own key names, because the trainer's claim about itself
//!   and the file's claim about the trainer are the same claim.
//! - `<layer_index>.weight.f32` / `<layer_index>.bias.f32`: one pair of raw
//!   little-endian `f32` files per layer with parameters, where
//!   `layer_index` is that layer's position in `spec.json`'s `layers` array
//!   (0-based, counting every layer, not just the weighted ones). A layer
//!   with no parameters (`relu`, `maxpool2`, `flatten`, `concat_features`)
//!   has no files.
//!
//! # Validation
//!
//! `build` refuses a trainer directory whose `charset_sha256` or
//! `feature_extractor` disagrees with the model being written, or whose
//! tensor files disagree with `spec.json`'s declared shapes, rather than
//! writing a table that would silently mismatch the recognisers beside it.
//! An unparseable `spec.json` or a missing tensor file is refused the same
//! way. A malformed *file on disk* is a build-time error, never something
//! [`ocrcer_core::ocrw::Model::load`] has to cope with later -- that runtime
//! leniency (`ocrcer_core::nn`'s module doc) is for a file that already
//! shipped, not for one still being built.
//!
//! # Quantisation
//!
//! Each weighted layer's weight matrix is quantised `int8` **per output
//! channel** (`ocrw::quantise_per_row`), not per feature dimension as the
//! prototype bank is: a dense or conv layer's rows are its learned output
//! channels, and channels can have very different learned magnitudes
//! (`crates/ocrcer-build/src/ocrw.rs`'s doc comment on `quantise_per_row`
//! gives the full reasoning). Biases stay `f32`.

use std::path::Path;

use ocrcer_core::nn::LayerKind;

use crate::ocrw::{self, json_string, Kind, Table};

pub const T_NN: &str = "nn";

/// One layer as `spec.json` declares it -- kind and shape only. Weight and
/// bias, when the kind has parameters, are read separately from the tensor
/// files and are not kept on this struct: [`build`] streams them straight
/// into the quantised table rather than holding two live copies of a weight
/// matrix that can run into the megabytes.
#[derive(Debug, Clone)]
struct LayerSpec {
    kind_text: String,
    kind: LayerKind,
    shape: Vec<u32>,
}

/// A trainer output directory's `spec.json`, parsed and validated for
/// internal consistency (not yet checked against the model it will be
/// written beside -- see [`build`]).
#[derive(Debug, Clone)]
pub struct TrainerSpec {
    nn_version: u32,
    junk_index: u32,
    n_outputs: u32,
    layers: Vec<LayerSpec>,
    pub charset_sha256: String,
    pub feature_extractor: u32,
    pub manifest_id: String,
    pub seed: i64,
    pub torch_version: String,
    pub python_version: String,
    pub lock_hash: String,
}

fn read_string_field(obj: &ocrcer_core::json::Json, key: &str) -> Result<String, String> {
    obj.get(key).and_then(ocrcer_core::json::Json::as_str).map(str::to_string).ok_or_else(|| format!("spec.json missing string field {key:?}"))
}

/// Reads a string field trying each key in order, so a trainer that names a
/// provenance field differently than this module's own doc comment guessed
/// (`training_manifest_id` rather than `manifest_id`, `lock_file_sha256`
/// rather than `lock_hash`) still parses -- the architecture's contract
/// (`ARCHITECTURE.md` section 11) only fixes what the field *means*, not its
/// literal JSON key, and `tools/nn/train.py` is the trainer that actually
/// shipped.
fn read_string_field_any(obj: &ocrcer_core::json::Json, keys: &[&str]) -> Result<String, String> {
    for key in keys {
        if let Some(s) = obj.get(key).and_then(ocrcer_core::json::Json::as_str) {
            return Ok(s.to_string());
        }
    }
    Err(format!("spec.json missing string field (tried {keys:?})"))
}

/// Loads and self-validates `dir/spec.json`. Does not touch the tensor
/// files, and does not check `charset_sha256` / `feature_extractor` against
/// anything -- that is [`build`]'s job, once it knows what model it is
/// writing.
pub fn load_spec(dir: &Path) -> Result<TrainerSpec, String> {
    use ocrcer_core::json::Json;

    let path = dir.join("spec.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v = Json::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;

    let nn_version = v.get("nn_version").and_then(Json::as_u32).ok_or("spec.json missing nn_version")?;
    let junk_index = v.get("junk_index").and_then(Json::as_u32).ok_or("spec.json missing junk_index")?;
    let n_outputs = v.get("n_outputs").and_then(Json::as_u32).ok_or("spec.json missing n_outputs")?;
    if n_outputs != junk_index + 1 {
        return Err(format!(
            "spec.json n_outputs ({n_outputs}) must be junk_index + 1 ({})",
            junk_index + 1
        ));
    }

    let layers_v = v.get("layers").and_then(Json::as_array).ok_or("spec.json missing layers")?;
    if layers_v.is_empty() {
        return Err("spec.json layers is empty".into());
    }
    let mut layers = Vec::with_capacity(layers_v.len());
    for (i, l) in layers_v.iter().enumerate() {
        let kind_text = l.get("kind").and_then(Json::as_str).ok_or_else(|| format!("spec.json layer {i} missing kind"))?.to_string();
        let kind = LayerKind::parse(&kind_text).ok_or_else(|| format!("spec.json layer {i} has unknown kind {kind_text:?}"))?;
        // `weight_shape` is what `tools/nn/train.py` actually emits per
        // weighted layer (full tensor shape, e.g. `[out,in,3,3]`); `shape`
        // is kept as a fallback for the synthetic fixtures in this module's
        // own tests, which predate that trainer.
        let shape: Vec<u32> = l
            .get("weight_shape")
            .or_else(|| l.get("shape"))
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(Json::as_u32).collect())
            .unwrap_or_default();
        if kind.has_params() && shape.is_empty() {
            return Err(format!("spec.json layer {i} ({kind_text}) has parameters but no shape"));
        }
        layers.push(LayerSpec { kind_text, kind, shape });
    }

    let charset_sha256 = read_string_field(&v, "charset_sha256")?;
    let feature_extractor = v.get("feature_extractor").and_then(Json::as_u32).ok_or("spec.json missing feature_extractor")?;
    let manifest_id = read_string_field_any(&v, &["manifest_id", "training_manifest_id"])?;
    let seed = v.get("seed").and_then(Json::as_i64).ok_or("spec.json missing seed")?;
    let torch_version = read_string_field(&v, "torch_version")?;
    let python_version = read_string_field(&v, "python_version")?;
    let lock_hash = read_string_field_any(&v, &["lock_hash", "lock_file_sha256"])?;

    Ok(TrainerSpec {
        nn_version,
        junk_index,
        n_outputs,
        layers,
        charset_sha256,
        feature_extractor,
        manifest_id,
        seed,
        torch_version,
        python_version,
        lock_hash,
    })
}

/// Refuses a trainer directory whose claims about the model it was trained
/// against disagree with the model actually being written. This is the
/// check that keeps a network and a prototype bank from silently disagreeing
/// about what class index 137 means, or from being scored with two different
/// feature extractors.
pub fn validate(spec: &TrainerSpec, want_charset_sha256: &str, want_feature_extractor: u32) -> Result<(), String> {
    if spec.charset_sha256 != want_charset_sha256 {
        return Err(format!(
            "spec.json charset_sha256 ({}) does not match this build's charset ({want_charset_sha256}); \
             the trainer was run against a different charset than the one being written",
            spec.charset_sha256
        ));
    }
    if spec.feature_extractor != want_feature_extractor {
        return Err(format!(
            "spec.json feature_extractor ({}) does not match this build's feature extractor ({want_feature_extractor})",
            spec.feature_extractor
        ));
    }
    if spec.nn_version != ocrcer_core::nn::SUPPORTED_NN_VERSION {
        return Err(format!(
            "spec.json nn_version ({}) is not one this build knows how to write (expects {}); \
             a runtime that cannot read this version would fall back to prototypes silently, \
             which is the wrong failure mode for a build you are about to ship",
            spec.nn_version,
            ocrcer_core::nn::SUPPORTED_NN_VERSION
        ));
    }
    Ok(())
}

fn read_f32_file(path: &Path, expect_len: usize) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() != expect_len * 4 {
        return Err(format!(
            "{}: {} bytes, expected {} (={expect_len} f32 values)",
            path.display(),
            bytes.len(),
            expect_len * 4
        ));
    }
    Ok(bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

/// One weighted layer's tensors, dequantised weight and untouched bias, kept
/// only long enough to (a) quantise into the table and (b) optionally dump
/// to `--nn-dequant-out`.
struct WeightedLayer {
    layer_index: usize,
    out_dim: usize,
    in_dim: usize,
    weight: Vec<f32>,
    bias: Vec<f32>,
    quantised: Vec<i8>,
    scales: Vec<f32>,
}

fn layer_dims(kind: LayerKind, shape: &[u32]) -> (usize, usize) {
    let out_dim = *shape.first().unwrap_or(&0) as usize;
    let in_dim = match kind {
        LayerKind::Conv3x3 => shape.get(1..4).map(|s| s.iter().product::<u32>() as usize).unwrap_or(0),
        LayerKind::Dense => shape.get(1).copied().unwrap_or(0) as usize,
        _ => 0,
    };
    (out_dim, in_dim)
}

/// Reads every weighted layer's tensor files, quantising each as it is read.
fn read_weighted_layers(dir: &Path, spec: &TrainerSpec) -> Result<Vec<WeightedLayer>, String> {
    let mut out = Vec::new();
    for (idx, l) in spec.layers.iter().enumerate() {
        if !l.kind.has_params() {
            continue;
        }
        let (out_dim, in_dim) = layer_dims(l.kind, &l.shape);
        if out_dim == 0 || in_dim == 0 {
            return Err(format!("layer {idx} ({}) has a degenerate shape {:?}", l.kind_text, l.shape));
        }
        let weight = read_f32_file(&dir.join(format!("{idx}.weight.f32")), out_dim * in_dim)?;
        let bias = read_f32_file(&dir.join(format!("{idx}.bias.f32")), out_dim)?;
        let (quantised, scales) = ocrw::quantise_per_row(&weight, out_dim, in_dim);
        out.push(WeightedLayer { layer_index: idx, out_dim, in_dim, weight, bias, quantised, scales });
    }
    Ok(out)
}

/// The largest quantisation round-trip error found on one weight tensor,
/// alongside which layer it came from. Deterministic -- the same trainer
/// output always names the same layer and the same error -- rather than
/// drawn from an RNG, per `CLAUDE.md` rule 1's requirement that a build's
/// numbers be reproducible.
pub struct QuantSample {
    pub layer_index: usize,
    pub max_abs_error: f32,
    pub bound: f32,
}

fn sample_quant_error(layers: &[WeightedLayer]) -> Option<QuantSample> {
    // The layer with the most weights: the tensor most likely to expose a
    // quantisation problem, and a deterministic choice given the same input.
    let l = layers.iter().max_by_key(|l| l.out_dim * l.in_dim)?;
    let back = ocrw::dequantise_per_row(&l.quantised, &l.scales, l.out_dim, l.in_dim);
    let max_abs_error = l.weight.iter().zip(&back).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    let bound = l.scales.iter().fold(0.0f32, |m, s| m.max(*s)) / 2.0;
    Some(QuantSample { layer_index: l.layer_index, max_abs_error, bound })
}

/// What [`build`] produces: the `nn` table, its `meta.nn` JSON object text
/// (the complete `{...}`, ready for [`crate::emit::meta_with_nn`]), and the
/// quantisation error sample [`CLAUDE.md`'s rule 8 asks be reported for
/// every export.
pub struct Built {
    pub table: Table,
    pub meta_json: String,
    pub weighted_layers: usize,
    pub quant_sample: Option<QuantSample>,
    spec: TrainerSpec,
    dequantised: Vec<WeightedLayer>,
}

/// Reads `dir`, validates it against the model being written, quantises
/// every weighted layer, and returns the `nn` table plus its `meta.nn` text.
pub fn build(dir: &Path, want_charset_sha256: &str, want_feature_extractor: u32) -> Result<Built, String> {
    let spec = load_spec(dir)?;
    validate(&spec, want_charset_sha256, want_feature_extractor)?;
    let weighted = read_weighted_layers(dir, &spec)?;

    let mut data = Vec::new();
    data.extend_from_slice(b"NNET");
    data.extend_from_slice(&(spec.nn_version as u16).to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes()); // reserved
    data.extend_from_slice(&(weighted.len() as u32).to_le_bytes());
    let mut scales = Vec::new();
    for l in &weighted {
        data.extend_from_slice(&(l.layer_index as u32).to_le_bytes());
        data.extend_from_slice(&(l.out_dim as u32).to_le_bytes());
        data.extend_from_slice(&(l.in_dim as u32).to_le_bytes());
        data.extend(l.quantised.iter().map(|&v| v as u8));
        for b in &l.bias {
            data.extend_from_slice(&b.to_le_bytes());
        }
        scales.extend_from_slice(&l.scales);
    }

    let table = Table { name: T_NN.into(), kind: Kind::Opaque, dims: vec![data.len() as u32], scales, data };
    let quant_sample = sample_quant_error(&weighted);
    let meta_json = meta_json(&spec);
    let weighted_layers = weighted.len();

    Ok(Built { table, meta_json, weighted_layers, quant_sample, spec, dequantised: weighted })
}

impl Built {
    /// Writes the dequantised `f32` tensors this table carries, in the same
    /// `<layer_index>.weight.f32` / `<layer_index>.bias.f32` layout the
    /// trainer output directory used -- what `--nn-dequant-out` is for. A
    /// parity check against this must compare the network the runtime will
    /// actually run (post-quantisation), not the pristine trainer output, the
    /// same reasoning `crate::emit::quantise_in_place` gives for the
    /// prototype bank.
    pub fn write_dequantised(&self, out_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
        for l in &self.dequantised {
            let back = ocrw::dequantise_per_row(&l.quantised, &l.scales, l.out_dim, l.in_dim);
            write_f32_file(&out_dir.join(format!("{}.weight.f32", l.layer_index)), &back)?;
            write_f32_file(&out_dir.join(format!("{}.bias.f32", l.layer_index)), &l.bias)?;
        }
        Ok(())
    }

    /// The trainer's provenance, if a caller wants to print or check it.
    pub fn spec(&self) -> &TrainerSpec {
        &self.spec
    }
}

fn write_f32_file(path: &Path, values: &[f32]) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for v in values {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Builds `meta.nn`'s JSON object text: layer spec, `nn_version`,
/// `junk_index`, `n_outputs`, and the trainer's provenance fields, copied
/// from `spec.json` rather than assumed, per `CLAUDE.md` rule 1 -- every
/// value here has a named source, and the source is this trainer run.
fn meta_json(spec: &TrainerSpec) -> String {
    let mut s = String::new();
    s.push('{');
    s.push_str(&format!("\"nn_version\":{},", spec.nn_version));
    s.push_str(&format!("\"junk_index\":{},", spec.junk_index));
    s.push_str(&format!("\"n_outputs\":{},", spec.n_outputs));
    s.push_str("\"layers\":[");
    for (i, l) in spec.layers.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!("{{\"kind\":{}", json_string(&l.kind_text)));
        if !l.shape.is_empty() {
            s.push_str(",\"shape\":[");
            for (j, d) in l.shape.iter().enumerate() {
                if j > 0 {
                    s.push(',');
                }
                s.push_str(&d.to_string());
            }
            s.push(']');
        }
        s.push('}');
    }
    s.push_str("],");
    s.push_str(&format!("\"charset_sha256\":{},", json_string(&spec.charset_sha256)));
    s.push_str(&format!("\"feature_extractor\":{},", spec.feature_extractor));
    s.push_str(&format!("\"manifest_id\":{},", json_string(&spec.manifest_id)));
    s.push_str(&format!("\"seed\":{},", spec.seed));
    s.push_str(&format!("\"torch_version\":{},", json_string(&spec.torch_version)));
    s.push_str(&format!("\"python_version\":{},", json_string(&spec.python_version)));
    s.push_str(&format!("\"lock_hash\":{}", json_string(&spec.lock_hash)));
    s.push('}');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a minimal but complete trainer output directory: one conv
    /// layer, one relu, one dense layer, matching the shapes
    /// `ocrcer-core`'s `nn` parser tests build by hand -- so this crate's
    /// writer and that crate's reader are checked against the same numbers.
    /// A directory name unique to this call, not just this process: `cargo
    /// test` runs a module's tests concurrently on separate threads of the
    /// same process, so a name keyed only on the process id collides between
    /// tests that both build a fixture directory at once.
    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("ocrcer-build-nn-{label}-{}-{n}", std::process::id()))
    }

    fn write_fixture_dir() -> std::path::PathBuf {
        let dir = unique_temp_dir("test");
        std::fs::create_dir_all(&dir).unwrap();

        let spec = r#"{
            "nn_version":1,"junk_index":3,"n_outputs":4,
            "layers":[
                {"kind":"conv3x3","shape":[2,1,3,3]},
                {"kind":"relu"},
                {"kind":"dense","shape":[3,2]}
            ],
            "charset_sha256":"deadbeef","feature_extractor":7,
            "manifest_id":"m1","seed":42,
            "torch_version":"2.4.0","python_version":"3.11.9","lock_hash":"abc123"
        }"#;
        std::fs::write(dir.join("spec.json"), spec).unwrap();

        write_f32_file(&dir.join("0.weight.f32"), &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, -1.0, -2.0, -3.0, -4.0, -5.0, -6.0, -7.0, -8.0, -9.0]).unwrap();
        write_f32_file(&dir.join("0.bias.f32"), &[0.5, -0.5]).unwrap();
        write_f32_file(&dir.join("2.weight.f32"), &[1.0, -2.0, 3.0, -4.0, 5.0, -6.0]).unwrap();
        write_f32_file(&dir.join("2.bias.f32"), &[1.0, 2.0, 3.0]).unwrap();
        dir
    }

    // The actual container round-trip (build here, parse in ocrcer-core's
    // `Model::load`) lives in `crates/ocrcer-build/tests/nn_roundtrip.rs`,
    // beside the rest of the chunk 3 round-trip gate: it needs a full
    // writable model (prototypes, feature_norm, ...), which is integration-
    // test machinery, not a unit-test fixture. This module's tests cover
    // what is local to reading and quantising a trainer directory.
    #[test]
    fn a_well_formed_trainer_directory_builds() {
        let dir = write_fixture_dir();
        let built = build(&dir, "deadbeef", 7).unwrap();
        assert_eq!(built.weighted_layers, 2);
        assert_eq!(built.table.name, T_NN);
        assert_eq!(built.table.kind, Kind::Opaque);
        assert!(built.meta_json.contains("\"nn_version\":1"));
        assert!(built.meta_json.contains("\"charset_sha256\":\"deadbeef\""));
        assert!(built.meta_json.contains("\"lock_hash\":\"abc123\""));
        let sample = built.quant_sample.as_ref().unwrap();
        assert!(sample.max_abs_error <= sample.bound + 1e-6, "{} vs {}", sample.max_abs_error, sample.bound);
        assert_eq!(built.spec().manifest_id, "m1");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `Built` holds a `Table`, which does not implement `Debug`, so
    /// `Result::unwrap_err` (which requires the `Ok` side to be `Debug` too)
    /// does not typecheck here. This is that unwrap, without the bound.
    fn expect_err(r: Result<Built, String>) -> String {
        match r {
            Ok(_) => panic!("expected an Err"),
            Err(e) => e,
        }
    }

    #[test]
    fn a_charset_mismatch_is_refused_with_a_clear_reason() {
        let dir = write_fixture_dir();
        let err = expect_err(build(&dir, "not-the-right-hash", 7));
        assert!(err.contains("charset_sha256"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_feature_extractor_mismatch_is_refused_with_a_clear_reason() {
        let dir = write_fixture_dir();
        let err = expect_err(build(&dir, "deadbeef", 99));
        assert!(err.contains("feature_extractor"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_wrong_size_tensor_file_is_refused() {
        let dir = write_fixture_dir();
        write_f32_file(&dir.join("0.weight.f32"), &[1.0, 2.0]).unwrap();
        let err = expect_err(build(&dir, "deadbeef", 7));
        assert!(err.contains("0.weight.f32"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_dequantised_reproduces_the_same_layout() {
        let dir = write_fixture_dir();
        let built = build(&dir, "deadbeef", 7).unwrap();
        let out = unique_temp_dir("dequant");
        built.write_dequantised(&out).unwrap();
        let w = read_f32_file(&out.join("0.weight.f32"), 18).unwrap();
        assert_eq!(w.len(), 18);
        let b = read_f32_file(&out.join("0.bias.f32"), 2).unwrap();
        assert_eq!(b, vec![0.5, -0.5]);
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&out).ok();
    }
}
