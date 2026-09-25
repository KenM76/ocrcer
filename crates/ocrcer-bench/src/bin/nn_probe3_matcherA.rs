//! `nn-probe3-matcherA`: Round 3 of the throwaway neural probe
//! (`docs/measurements/2026-09-25_nn_probe.md` "Round 3", task item 1 --
//! "the comparison is not yet fair" -- and item 2, the cluster-disjoint
//! re-split). Builds the "matcher+realA" side of the fair head-to-head:
//! chunk 13's real-scan-prototypes idea (§11, 2026-09-24 item (b)), applied
//! in-memory only, never written to `model/out`.
//!
//! No feature is recomputed and no distance kernel is reimplemented
//! (`CLAUDE.md` rule 4): every feature vector consumed here was already
//! extracted by `crates/ocrcer-bench/src/bin/nn_dump.rs` calling
//! `ocrcer_core::feature::extract_with_grid`, and every match is
//! `ocrcer_core::r#match::nearest` -- the runtime's own matcher, run against
//! an in-memory [`Model`] whose `prototypes`/`prototype_class`/
//! `prototype_italic`/`class_holes` have been extended with one extra row
//! per training-fold real crop.
//!
//! # What "appended as an extra prototype" means here
//!
//! `Model::prototypes` (`ocrcer-core/src/ocrw.rs`) is already standardised
//! and already dequantised at load -- exactly the units `nn_dump.rs`'s
//! `real_X.f32` is written in (`model.standardise(&raw)`), so a training-fold
//! crop's `X` row can be appended to `Model::prototypes` directly, no
//! re-standardisation needed. `class_holes` and `prototype_italic` are the
//! two other per-prototype tables the matcher reads (`match.rs`): a crop's
//! hole count is recovered by inverting `standardise` (`raw = X*sd + mean`,
//! exact since `mean`/`sd` are this same model's own constants) and calling
//! `ocrcer_core::feature::holes_of`; `prototype_italic` is set `false` for
//! every appended row (real filing crops are overwhelmingly upright; this is
//! an assumption, stated here, not a measurement -- Round 3's matcher+realA
//! call uses `italic_ok = true` throughout regardless, the same convention
//! `nn_dump.rs` already uses for real crops, so this flag has no effect on
//! this run's outcome and is set only so the table stays well-formed).
//!
//! # Quantisation
//!
//! The shipped bank's prototypes were quantised once, at build, to `i8` with
//! one `max|v|/127` scale per column over the *original* bank
//! (`ocrcer_build::ocrw::quantise`) and dequantised once at load
//! (`Model::load`) -- so what's in memory here is already a lossy copy, not
//! the pre-quantisation floats. This tool cannot recover those, so it
//! re-quantises+dequantises the *combined* set (original dequantised rows +
//! the new real-A rows) through the same `ocrcer_build::ocrw::{quantise,
//! dequantise}` functions the real build uses, with one column scale over
//! all rows together -- a double-quantisation of the original rows, and an
//! approximation of what a real chunk-13 build (which would quantise the
//! true pre-quantisation floats plus the new rows together) would produce.
//! Reported as a measured caveat, not smoothed over.
//!
//! # Usage
//!
//! ```text
//! cargo run -p ocrcer-bench --release --bin nn-probe3-matcherA -- \
//!     <model.ocrw> <dump-dir> <fold-assignment.tsv> <out-dir>
//! ```
//!
//! `dump-dir` is a `nn-dump` output directory carrying `real_X.f32`,
//! `real_y.u16`, `real_meta.tsv` (column 2 = stem) -- Round 2's dump is
//! reused as-is; this tool never re-extracts a feature. `fold-assignment.tsv`
//! is `#stem\tfold\tcluster_id` with `fold` in `{A, B}`
//! (`tools/nnprobe/cluster_pages.py`'s output).
//!
//! Writes, for both directions (`train=A,test=B` and `train=B,test=A`):
//! `fold1_test_matcherA_top1.u16` / `fold2_test_matcherA_top1.u16` -- one
//! predicted class per **test-fold row, in `real_meta.tsv`'s original row
//! order** (so it aligns element-for-element with a numpy boolean mask
//! `stem_in_test_fold` applied to the same `real_meta.tsv` order, which is
//! how `tools/nnprobe/train3.py` reads it back -- no row-index file needed).
//! `NONE_CLASS` (`0xFFFF`) marks a query `nearest` returned nothing for
//! (never observed in Round 1/2, kept as a sentinel for correctness). Also
//! writes `report3_matcherA.json` with row counts, bank sizes, and wall time.

use ocrcer_build::ocrw::{dequantise, quantise};
use ocrcer_core::feature::{holes_of, FEATURE_DIMS};
use ocrcer_core::ocrw::Model;
use ocrcer_core::r#match;

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

const NONE_CLASS: u16 = 0xFFFF;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let model_path = match args.next() {
        Some(a) => PathBuf::from(a),
        None => return fail("usage: nn-probe3-matcherA <model.ocrw> <dump-dir> <fold-assignment.tsv> <out-dir>"),
    };
    let dump_dir = match args.next() {
        Some(a) => PathBuf::from(a),
        None => return fail("missing <dump-dir>"),
    };
    let fold_path = match args.next() {
        Some(a) => PathBuf::from(a),
        None => return fail("missing <fold-assignment.tsv>"),
    };
    let out_dir = match args.next() {
        Some(a) => PathBuf::from(a),
        None => return fail("missing <out-dir>"),
    };
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        return fail(&format!("creating {}: {e}", out_dir.display()));
    }

    let model_bytes = match std::fs::read(&model_path) {
        Ok(b) => b,
        Err(e) => return fail(&format!("reading {}: {e}", model_path.display())),
    };
    let model = match Model::load(&model_bytes) {
        Ok(m) => m,
        Err(e) => return fail(&format!("loading {}: {e}", model_path.display())),
    };

    let stems = match read_stems(&dump_dir.join("real_meta.tsv")) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let n = stems.len();
    let x = match read_f32(&dump_dir.join("real_X.f32"), n * FEATURE_DIMS) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };
    let y = match read_u16(&dump_dir.join("real_y.u16"), n) {
        Ok(v) => v,
        Err(e) => return fail(&e),
    };

    let fold = match read_fold_assignment(&fold_path) {
        Ok(m) => m,
        Err(e) => return fail(&e),
    };
    for s in &stems {
        if !fold.contains_key(s) {
            return fail(&format!("{}: stem {:?} has no fold assignment", fold_path.display(), s));
        }
    }

    let diag_only = std::env::var("NN_PROBE_DIAG_ONLY").as_deref() == Ok("1");

    let mut report = String::from("{\n");
    for (tag, train_fold, test_fold) in [("fold1", 'A', 'B'), ("fold2", 'B', 'A')] {
        let t0 = Instant::now();
        let augmented = build_augmented_model(&model, &stems, &x, &y, &fold, train_fold);
        let build_s = t0.elapsed().as_secs_f64();
        if diag_only {
            continue;
        }

        let t1 = Instant::now();
        let mut preds: Vec<u16> = Vec::new();
        let mut n_test = 0usize;
        let mut n_train_extra = 0usize;
        for i in 0..n {
            if fold[&stems[i]] == train_fold {
                n_train_extra += 1;
            }
            if fold[&stems[i]] != test_fold {
                continue;
            }
            n_test += 1;
            let x_row = &x[i * FEATURE_DIMS..(i + 1) * FEATURE_DIMS];
            let raw = invert_standardise(x_row, &model);
            let pred = r#match::nearest(&augmented, &raw, 1, true)
                .and_then(|m| m.top())
                .map_or(NONE_CLASS, |c| c.class);
            preds.push(pred);
        }
        let match_s = t1.elapsed().as_secs_f64();

        let out_path = out_dir.join(format!("{tag}_test_matcherA_top1.u16"));
        if let Err(e) = write_u16(&out_path, &preds) {
            return fail(&e);
        }
        report.push_str(&format!(
            "  \"{tag}\": {{\"train_fold\": \"{train_fold}\", \"test_fold\": \"{test_fold}\", \
             \"n_train_extra\": {n_train_extra}, \"n_test\": {n_test}, \
             \"n_prototypes_base\": {}, \"n_prototypes_augmented\": {}, \
             \"build_s\": {build_s:.2}, \"match_s\": {match_s:.2}}},\n",
            model.n_prototypes(),
            augmented.n_prototypes(),
        ));
        eprintln!(
            "nn-probe3-matcherA: {tag} train={train_fold} test={test_fold} \
             n_train_extra={n_train_extra} n_test={n_test} base_protos={} \
             augmented_protos={} build={build_s:.2}s match={match_s:.2}s",
            model.n_prototypes(),
            augmented.n_prototypes(),
        );
    }
    report.push_str("  \"note\": \"predictions are in real_meta.tsv row order restricted to the test fold; see module doc\"\n}\n");
    if let Err(e) = std::fs::write(out_dir.join("report3_matcherA.json"), &report) {
        return fail(&format!("writing report3_matcherA.json: {e}"));
    }
    print!("{report}");

    ExitCode::SUCCESS
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("nn-probe3-matcherA: {msg}");
    ExitCode::FAILURE
}

/// `raw = X*sd + mean`, the exact inverse of `Model::standardise`, using
/// this same model's own constants -- lossless up to `f32` rounding.
fn invert_standardise(x_row: &[f32], model: &Model) -> [f32; FEATURE_DIMS] {
    let mut raw = [0.0f32; FEATURE_DIMS];
    for i in 0..FEATURE_DIMS {
        raw[i] = x_row[i] * model.sd[i] + model.mean[i];
    }
    raw
}

/// Builds an in-memory model whose bank is the original plus every real crop
/// whose stem falls in `train_fold`, re-quantised+dequantised together (see
/// the module doc's "Quantisation" section).
fn build_augmented_model(
    model: &Model,
    stems: &[String],
    x: &[f32],
    y: &[u16],
    fold: &std::collections::HashMap<String, char>,
    train_fold: char,
) -> Model {
    let base_n = model.n_prototypes();
    let mut prototypes = model.prototypes.clone();
    let mut prototype_class = model.prototype_class.clone();
    let mut prototype_italic = model.prototype_italic.clone();
    if prototype_italic.len() < base_n {
        prototype_italic.resize(base_n, false);
    }
    let mut class_holes = model.class_holes.clone();

    for i in 0..stems.len() {
        if fold[&stems[i]] != train_fold {
            continue;
        }
        let x_row = &x[i * FEATURE_DIMS..(i + 1) * FEATURE_DIMS];
        prototypes.extend_from_slice(x_row);
        prototype_class.push(y[i]);
        prototype_italic.push(false);

        let raw = invert_standardise(x_row, model);
        let holes = holes_of(&raw);
        let class = y[i] as usize;
        if class >= class_holes.len() {
            class_holes.resize(class + 1, 0);
        }
        class_holes[class] |= 1u8 << holes.min(7);
    }

    let n_rows = prototype_class.len();
    let (q, scales) = quantise(&prototypes, n_rows, FEATURE_DIMS);
    let requantised = dequantise(&q, &scales, n_rows, FEATURE_DIMS);

    // Round 3 diagnostic (NN_PROBE_DIAG=1): how much does re-quantising the
    // combined (base + real-A) set over one shared per-column scale disturb
    // the *original* base rows versus the base model's own one-shot
    // quantisation? Compares this re-quantisation's error against the
    // original prototypes (already dequantised once at model load) only
    // over the base_n rows, so it isolates the scale-widening effect of
    // adding real-A rows from the (unavoidable, already-baked-in) original
    // build's own quantisation error.
    if std::env::var("NN_PROBE_DIAG").as_deref() == Ok("1") {
        let mut max_scale_before = 0.0f32;
        let mut max_scale_after = 0.0f32;
        let (_q0, scales0) = quantise(&model.prototypes, base_n, FEATURE_DIMS);
        for c in 0..FEATURE_DIMS {
            max_scale_before = max_scale_before.max(scales0[c]);
            max_scale_after = max_scale_after.max(scales[c]);
        }
        let mut sum_abs_err_base_only = 0.0f64;
        let mut sum_abs_err_after = 0.0f64;
        let base0 = dequantise(&_q0, &scales0, base_n, FEATURE_DIMS);
        for i in 0..base_n * FEATURE_DIMS {
            sum_abs_err_base_only += (base0[i] - model.prototypes[i]).abs() as f64;
            sum_abs_err_after += (requantised[i] - model.prototypes[i]).abs() as f64;
        }
        let n_vals = (base_n * FEATURE_DIMS) as f64;
        eprintln!(
            "NN_PROBE_DIAG: base_n={base_n} n_rows={n_rows} \
             max_col_scale base-only-requant={max_scale_before:.6} \
             combined-requant={max_scale_after:.6} \
             mean_abs_err base-only-requant={:.6} combined-requant={:.6} \
             (both vs the original once-dequantised model.prototypes)",
            sum_abs_err_base_only / n_vals,
            sum_abs_err_after / n_vals,
        );
    }

    let prototypes = requantised;

    Model {
        build_id: model.build_id.clone(),
        feature_version: model.feature_version,
        classes: model.classes.clone(),
        faces: model.faces.clone(),
        sizes: model.sizes.clone(),
        prototypes,
        prototype_class,
        prototype_italic,
        mean: model.mean,
        sd: model.sd,
        class_holes,
        weights: model.weights,
        class_info: model.class_info.clone(),
        params: model.params.clone(),
        lexicon: None,
        bigrams: None,
        confusions: None,
        nn: None,
        nn_status: ocrcer_core::nn::NnStatus::Absent,
    }
}

fn read_stems(path: &std::path::Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        let stem = cols.get(1).ok_or_else(|| format!("{}: row missing stem column", path.display()))?;
        out.push((*stem).to_string());
    }
    Ok(out)
}

fn read_f32(path: &std::path::Path, expect_len: usize) -> Result<Vec<f32>, String> {
    let mut f = File::open(path).map_err(|e| format!("opening {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(|e| format!("reading {}: {e}", path.display()))?;
    if bytes.len() != expect_len * 4 {
        return Err(format!(
            "{}: {} bytes, expected {} ({} f32 values)",
            path.display(),
            bytes.len(),
            expect_len * 4,
            expect_len
        ));
    }
    Ok(bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

fn read_u16(path: &std::path::Path, expect_len: usize) -> Result<Vec<u16>, String> {
    let mut f = File::open(path).map_err(|e| format!("opening {}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(|e| format!("reading {}: {e}", path.display()))?;
    if bytes.len() != expect_len * 2 {
        return Err(format!(
            "{}: {} bytes, expected {} ({} u16 values)",
            path.display(),
            bytes.len(),
            expect_len * 2,
            expect_len
        ));
    }
    Ok(bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
}

fn write_u16(path: &std::path::Path, values: &[u16]) -> Result<(), String> {
    let mut f = BufWriter::new(File::create(path).map_err(|e| format!("creating {}: {e}", path.display()))?);
    for &v in values {
        f.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn read_fold_assignment(path: &std::path::Path) -> Result<std::collections::HashMap<String, char>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        let stem = cols.first().ok_or_else(|| format!("{}: bad row {line:?}", path.display()))?;
        let fold = cols.get(1).ok_or_else(|| format!("{}: bad row {line:?}", path.display()))?;
        let c = fold
            .chars()
            .next()
            .ok_or_else(|| format!("{}: empty fold value on row {line:?}", path.display()))?;
        if c != 'A' && c != 'B' {
            return Err(format!("{}: fold value must be A or B, got {fold:?}", path.display()));
        }
        out.insert((*stem).to_string(), c);
    }
    Ok(out)
}
