//! `nn-dump`: emits the training data for the throwaway neural probe defined
//! in `docs/ARCHITECTURE.md` section 11 ("Candidate: a throwaway neural
//! probe before chunk 15").
//!
//! This binary computes every feature vector and grid it writes by calling
//! `ocrcer_core::feature::extract_with_grid` -- the same extractor
//! `ocrcer-build`'s bank and the runtime matcher use (`CLAUDE.md` rule 4).
//! Python, downstream, never computes a grid or a feature vector; it only
//! trains on what this binary already extracted.
//!
//! # Two dumps
//!
//! **(a) Bank renders.** Every class in `model/charset.tsv`, rendered from
//! every *shippable* face in `model/fonts.tsv` (`ocrcer-build`'s own
//! `bank::Fonts`/`Renderer`, not a second rasteriser) at the model's own
//! build-size ladder, plus three seeded, deterministic damage variants on
//! top of the clean render (gaussian-ish noise, partial blur, threshold
//! jitter, sub-pixel shift, and, for one variant, stroke erosion/dilation).
//! Split 80/20 into `bank_train_*` / `bank_val_*` by a seeded hash of
//! `(class, face, size, variant)`.
//!
//! **(b) Real crops.** `multifinben-englishocr` **train**-split pages only
//! (`bench/splits/manifest.tsv`, checked the same way
//! `count_text.rs` checks it -- dataset/split filter, then the redundant
//! `splits::assert_fittable` gate, then a `*-train` directory-name gate).
//! `Engine::recognize_lines` is called once per page to get decoded
//! `Line`/`Word`/`CharBox`es; a truth line is paired with a decoded line by
//! index only when their word counts match, and a truth word is paired with
//! a decoded word only when their character counts match (both are
//! selection-bias gates against misalignment, not an oracle: see the
//! reported qualify/total counts). The engine's own final binary mask is
//! recomputed with the same public calls `recognize_lines` itself makes
//! (binarize -> deskew-estimate -> deskew-correct -> re-binarize ->
//! underline-strip when `params.lines().underline_strip`), and each
//! qualifying `CharBox`'s ink is cropped from that mask; its `baseline_dy`
//! is recovered by inverting `pipeline.rs`'s own `unshear` (`Line.rect.x`
//! is never sheared, so `deskewed_baseline = Line.baseline - Line.rect.x *
//! slope`). This is a reconstruction of what the engine's own matcher saw,
//! not a second segmentation implementation: segmentation, lattice-building
//! and decoding are never re-run here, only the engine's own already-decoded
//! output is read back against a recomputed mask.
//!
//! For every qualifying real crop this also records the raw prototype
//! matcher's top-1 (`ocrcer_core::match::nearest` against the same model,
//! the same extracted vector) so the report can compare matcher vs. network
//! on identical inputs.
//!
//! # Output
//!
//! Flat binary files under `<out-dir>` (never committed; see the module
//! doc's "NEVER read the score set" rule -- this binary never opens
//! `finfilings`, `finfilings-val`, `bench/pages-cov` or any fixture):
//!
//! - `classes.tsv` -- a copy of the authored charset, for Python's grouping.
//! - `bank_train_{G,X,y,meta}.*`, `bank_val_{G,X,y,meta}.*`
//! - `real_{G,X,y,top1,meta}.*`
//! - `summary.json` -- every count this run measured.
//!
//! `G` is 32*32 `f32` (row-major), `X` is 107 `f32` (standardised with the
//! loaded model's own mean/sd -- the file's constants, never a compiled-in
//! copy, per `match.rs`'s own contract), `y`/`top1` are `u16` class indices
//! (`0xFFFF` sentinel for "no match").
//!
//! # Usage
//!
//! ```text
//! cargo run -p ocrcer-bench --bin nn-dump -- [model.ocrw] [pages-train-dir] [out-dir]
//! ```
//! Defaults: `model/out/ocrcer.ocrw`, `D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-train`,
//! `D:/Dev/ExcludedPrivate/ocrcer/nnprobe`.

use ocrcer_bench::splits;
use ocrcer_build::face::raster::Raster;
use ocrcer_build::{bank, page, tables};
use ocrcer_core::feature::{extract_with_grid, GlyphInput, FEATURE_DIMS};
use ocrcer_core::image::{binarize, deskew};
use ocrcer_core::layout::underline;
use ocrcer_core::ocrw::Model;
use ocrcer_core::{r#match, Engine, Gray};

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const NONE_CLASS: u16 = 0xFFFF;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let model_path =
        args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("model/out/ocrcer.ocrw"));
    let pages_dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("D:/Dev/ExcludedPrivate/ocrcer/pages/finfilings-train"));
    let out_dir = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("D:/Dev/ExcludedPrivate/ocrcer/nnprobe"));
    // Optional 5th arg: process every Nth train-split page (manifest order,
    // which is grouped by shard) instead of all of them. This is a wall-time
    // knob only -- it does not change what's licence-clean or what's
    // train-vs-score, and it is reported verbatim in summary.json so the
    // report can state exactly how much of the train split was sampled and
    // that the stride spans all 8 shards rather than reading a shard-biased
    // prefix. Default 1 = every page.
    let stride: usize = args
        .next()
        .map(|s| s.parse().unwrap_or(1))
        .filter(|&n| n >= 1)
        .unwrap_or(1);

    if let Err(e) = assert_train_dir_name(&pages_dir) {
        return fail(&e);
    }
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
    let engine = match Engine::from_bytes(&model_bytes) {
        Ok(e) => e,
        Err(e) => return fail(&format!("loading engine from {}: {e}", model_path.display())),
    };

    let model_dir = tables::model_dir();
    let classes = match tables::load_charset(&model_dir) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    if let Err(e) = write_classes_tsv(&out_dir.join("classes.tsv"), &classes) {
        return fail(&e);
    }
    let mut class_of_char: BTreeMap<char, u16> = BTreeMap::new();
    for c in &classes {
        class_of_char.insert(c.codepoint, c.index);
    }

    let bank_summary = match dump_bank(&model, &classes, &out_dir) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let real_summary =
        match dump_real(&engine, &model, &class_of_char, &pages_dir, &out_dir, stride) {
            Ok(s) => s,
            Err(e) => return fail(&e),
        };

    let summary = format!(
        "{{\n  \"bank_train_rows\": {},\n  \"bank_val_rows\": {},\n  \"bank_faces\": {},\n  \
         \"bank_sizes\": {:?},\n  \"page_stride\": {},\n  \"pages_read\": {},\n  \
         \"lines_total\": {},\n  \
         \"lines_qualifying\": {},\n  \"words_total\": {},\n  \"words_qualifying\": {},\n  \
         \"chars_dumped\": {},\n  \"chars_out_of_charset\": {},\n  \"chars_bad_crop\": {}\n}}\n",
        bank_summary.train_rows,
        bank_summary.val_rows,
        bank_summary.faces,
        bank_summary.sizes,
        stride,
        real_summary.pages_read,
        real_summary.lines_total,
        real_summary.lines_qualifying,
        real_summary.words_total,
        real_summary.words_qualifying,
        real_summary.chars_dumped,
        real_summary.chars_out_of_charset,
        real_summary.chars_bad_crop,
    );
    if let Err(e) = std::fs::write(out_dir.join("summary.json"), &summary) {
        return fail(&format!("writing summary.json: {e}"));
    }
    print!("{summary}");

    ExitCode::SUCCESS
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("nn-dump: {msg}");
    ExitCode::FAILURE
}

/// Same firewall `count_text.rs` enforces: refuses to run against anything
/// not named `*-train`.
fn assert_train_dir_name(dir: &Path) -> Result<(), String> {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: cannot read a directory name", dir.display()))?;
    if name.ends_with("-train") {
        Ok(())
    } else {
        Err(format!(
            "{}: refusing to dump from a directory not named `*-train` -- this tool must \
             never read a scoring or validation directory",
            dir.display()
        ))
    }
}

fn write_classes_tsv(path: &Path, classes: &[tables::Class]) -> Result<(), String> {
    let mut f = BufWriter::new(
        File::create(path).map_err(|e| format!("creating {}: {e}", path.display()))?,
    );
    writeln!(f, "index\tcodepoint_u32\tcategory").map_err(|e| e.to_string())?;
    for c in classes {
        writeln!(f, "{}\t{}\t{}", c.index, c.codepoint as u32, c.category)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------
// (a) Bank renders
// ---------------------------------------------------------------------

struct BankSummary {
    train_rows: u64,
    val_rows: u64,
    faces: usize,
    sizes: Vec<f32>,
}

struct Sink3 {
    g: BufWriter<File>,
    x: BufWriter<File>,
    y: BufWriter<File>,
    meta: BufWriter<File>,
}

impl Sink3 {
    fn open(dir: &Path, prefix: &str) -> Result<Sink3, String> {
        let open = |name: &str| -> Result<BufWriter<File>, String> {
            let p = dir.join(format!("{prefix}_{name}"));
            File::create(&p).map(BufWriter::new).map_err(|e| format!("creating {}: {e}", p.display()))
        };
        Ok(Sink3 { g: open("G.f32")?, x: open("X.f32")?, y: open("y.u16")?, meta: open("meta.tsv")? })
    }

    fn write(
        &mut self,
        grid: &[[f32; 32]; 32],
        xv: &[f32; FEATURE_DIMS],
        y: u16,
        meta_row: &str,
    ) -> Result<(), String> {
        for row in grid {
            for v in row {
                self.g.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
            }
        }
        for v in xv {
            self.x.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
        }
        self.y.write_all(&y.to_le_bytes()).map_err(|e| e.to_string())?;
        writeln!(self.meta, "{meta_row}").map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn dump_bank(model: &Model, classes: &[tables::Class], out_dir: &Path) -> Result<BankSummary, String> {
    let entries = tables::load_fonts(&tables::model_dir())?;
    // `include_local_only = false`: the render dump uses the same face set a
    // shipping bank does, not the operator's local-only faces.
    let fonts = bank::Fonts::load(&entries, false);
    let (faces, renderers, failed) = fonts.renderers();
    for line in &failed {
        eprintln!("nn-dump: face failed to parse: {line}");
    }

    let sizes: Vec<f32> = if model.sizes.is_empty() { vec![16.0, 20.0, 24.0, 32.0, 48.0] } else { model.sizes.clone() };
    // Variant 0 is clean; 1..=3 are seeded damage recipes (see `augment`).
    let variants: [u32; 4] = [0, 1, 2, 3];

    let mut train = Sink3::open(out_dir, "bank_train")?;
    let mut val = Sink3::open(out_dir, "bank_val")?;
    let mut train_rows = 0u64;
    let mut val_rows = 0u64;

    for (fi, renderer) in renderers.iter().enumerate() {
        for class in classes {
            for &px in &sizes {
                let Some(r) = renderer.render(class.codepoint, px) else { continue };
                let Some(x_height) = renderer.x_height_px(px).filter(|x| *x > 0.0) else { continue };
                for &variant in &variants {
                    let seed = sample_seed(class.index, fi as u32, px, variant);
                    let (ink, width, height, baseline_dy) = augment(&r, variant, seed);
                    if width == 0 || height == 0 {
                        continue;
                    }
                    let input =
                        GlyphInput { ink: &ink, width, height, baseline_dy, x_height };
                    let (raw, grid) = extract_with_grid(&input);
                    let xv = model.standardise(&raw);
                    let meta = format!(
                        "{}\t{}\t{}\t{}\t{}\t{}",
                        class.index, fi, faces[fi].family, faces[fi].style, px as u32, variant
                    );
                    let is_val = split_fraction(seed) < 0.2;
                    if is_val {
                        val.write(&grid, &raw_or_std(&xv), class.index, &meta)?;
                        val_rows += 1;
                    } else {
                        train.write(&grid, &raw_or_std(&xv), class.index, &meta)?;
                        train_rows += 1;
                    }
                }
            }
        }
    }

    Ok(BankSummary { train_rows, val_rows, faces: faces.len(), sizes })
}

fn raw_or_std(v: &[f32; FEATURE_DIMS]) -> [f32; FEATURE_DIMS] {
    *v
}

/// A row's position in `[0, 1)`, deterministic and RNG-free -- same
/// technique as `ocrcer_bench::splits::sample_fraction`
/// (FNV-1a folds a shared-prefix key too weakly on its own; `fmix64` spreads
/// it across every output bit before use).
fn split_fraction(seed: u64) -> f64 {
    fmix64(seed) as f64 / u64::MAX as f64
}

fn sample_seed(class: u16, face: u32, px: f32, variant: u32) -> u64 {
    let key = format!("{class}\u{1f}{face}\u{1f}{}\u{1f}{variant}", px.to_bits());
    fmix64(fnv1a64(key.as_bytes()))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51afd7ed558ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ceb9fe1a85ec53);
    k ^= k >> 33;
    k
}

/// Deterministic scan-damage augmentation, applied to the rendered raster
/// before extraction (never after -- the extractor must see exactly the
/// pixels a scanned page would present). Variant 0 is a no-op (clean).
///
/// Recipe, seeded by `(class, face, size, variant)` so a rebuild reproduces
/// the same bytes: pad by 2px, sub-pixel shift (bilinear), partial box blur,
/// additive noise (sum-of-uniforms, roughly gaussian), threshold jitter
/// around 0.5, and -- with a per-seed roll, guaranteed for variant 3 -- one
/// pass of 3x3 erosion or dilation. JPEG-style blockiness is the one
/// documented-optional damage type and is not implemented here.
fn augment(r: &Raster, variant: u32, seed: u64) -> (Vec<u8>, u32, u32, f32) {
    if variant == 0 {
        return (r.ink.clone(), r.width, r.height, r.baseline_dy);
    }
    let pad: u32 = 2;
    let w2 = r.width + 2 * pad;
    let h2 = r.height + 2 * pad;
    let mut canvas = vec![0f64; (w2 * h2) as usize];
    for y in 0..r.height {
        for x in 0..r.width {
            let v = r.ink[(y * r.width + x) as usize];
            canvas[((y + pad) * w2 + (x + pad)) as usize] = if v != 0 { 1.0 } else { 0.0 };
        }
    }

    let mut st = seed | 1;
    let mut next = move || -> f64 { (xorshift64star(&mut st) as f64) / (u64::MAX as f64) };

    let shift_amt = if variant == 1 { 0.3 } else { 0.4 };
    let dx = (next() - 0.5) * shift_amt;
    let dy = (next() - 0.5) * shift_amt;
    let shifted = bilinear_shift(&canvas, w2, h2, dx, dy);

    let blur_amt = match variant {
        1 => 0.5,
        2 => 0.65,
        _ => 0.25,
    };
    let blurred = box_blur3(&shifted, w2, h2);
    let mut mixed: Vec<f64> =
        shifted.iter().zip(blurred.iter()).map(|(&a, &b)| a * (1.0 - blur_amt) + b * blur_amt).collect();

    let sigma = match variant {
        1 => 0.18,
        2 => 0.12,
        _ => 0.15,
    };
    for v in mixed.iter_mut() {
        let n = (next() + next() + next() + next() - 2.0) * sigma;
        *v = (*v + n).clamp(0.0, 1.0);
    }

    let thresh = 0.5 + (next() - 0.5) * 0.24;
    let mut binmask: Vec<u8> = mixed.iter().map(|&v| u8::from(v >= thresh)).collect();

    let morph_roll = next();
    if variant == 3 || morph_roll < 0.35 {
        binmask =
            if next() < 0.5 { erode3(&binmask, w2, h2) } else { dilate3(&binmask, w2, h2) };
    }

    (binmask, w2, h2, r.baseline_dy + pad as f32)
}

fn xorshift64star(state: &mut u64) -> u64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

fn bilinear_shift(canvas: &[f64], w: u32, h: u32, dx: f64, dy: f64) -> Vec<f64> {
    let mut out = vec![0f64; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let sx = x as f64 - dx;
            let sy = y as f64 - dy;
            out[(y * w + x) as usize] = bilinear_sample(canvas, w, h, sx, sy);
        }
    }
    out
}

fn bilinear_sample(canvas: &[f64], w: u32, h: u32, x: f64, y: f64) -> f64 {
    let x0 = x.floor();
    let y0 = y.floor();
    let tx = x - x0;
    let ty = y - y0;
    let get = |xi: i64, yi: i64| -> f64 {
        if xi < 0 || yi < 0 || xi as u32 >= w || yi as u32 >= h {
            0.0
        } else {
            canvas[(yi as u32 * w + xi as u32) as usize]
        }
    };
    let x0i = x0 as i64;
    let y0i = y0 as i64;
    let a = get(x0i, y0i);
    let b = get(x0i + 1, y0i);
    let c = get(x0i, y0i + 1);
    let d = get(x0i + 1, y0i + 1);
    let top = a * (1.0 - tx) + b * tx;
    let bot = c * (1.0 - tx) + d * tx;
    top * (1.0 - ty) + bot * ty
}

fn box_blur3(canvas: &[f64], w: u32, h: u32) -> Vec<f64> {
    let get = |x: i64, y: i64| -> f64 {
        if x < 0 || y < 0 || x as u32 >= w || y as u32 >= h {
            0.0
        } else {
            canvas[(y as u32 * w + x as u32) as usize]
        }
    };
    let mut out = vec![0f64; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            for oy in -1i64..=1 {
                for ox in -1i64..=1 {
                    sum += get(x as i64 + ox, y as i64 + oy);
                }
            }
            out[(y * w + x) as usize] = sum / 9.0;
        }
    }
    out
}

fn erode3(mask: &[u8], w: u32, h: u32) -> Vec<u8> {
    morph3(mask, w, h, true)
}

fn dilate3(mask: &[u8], w: u32, h: u32) -> Vec<u8> {
    morph3(mask, w, h, false)
}

fn morph3(mask: &[u8], w: u32, h: u32, erode: bool) -> Vec<u8> {
    let get = |x: i64, y: i64| -> u8 {
        if x < 0 || y < 0 || x as u32 >= w || y as u32 >= h {
            0
        } else {
            mask[(y as u32 * w + x as u32) as usize]
        }
    };
    let mut out = vec![0u8; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut all1 = true;
            let mut any1 = false;
            for oy in -1i64..=1 {
                for ox in -1i64..=1 {
                    let v = get(x as i64 + ox, y as i64 + oy);
                    all1 &= v != 0;
                    any1 |= v != 0;
                }
            }
            out[(y * w + x) as usize] = u8::from(if erode { all1 } else { any1 });
        }
    }
    out
}

// ---------------------------------------------------------------------
// (b) Real crops
// ---------------------------------------------------------------------

struct RealSummary {
    pages_read: u64,
    lines_total: u64,
    lines_qualifying: u64,
    words_total: u64,
    words_qualifying: u64,
    chars_dumped: u64,
    chars_out_of_charset: u64,
    chars_bad_crop: u64,
}

fn dump_real(
    engine: &Engine,
    model: &Model,
    class_of_char: &BTreeMap<char, u16>,
    pages_dir: &Path,
    out_dir: &Path,
    stride: usize,
) -> Result<RealSummary, String> {
    let manifest_path = ocrcer_bench::default_splits_root().join("manifest.tsv");
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("reading {}: {e}", manifest_path.display()))?;
    let manifest = splits::parse_manifest_tsv(&manifest_text)?;

    let all_train_rows: Vec<&splits::ManifestRow> = manifest
        .iter()
        .filter(|r| r.dataset == splits::MULTIFINBEN_DATASET && r.split == splits::Split::Train)
        .collect();
    if all_train_rows.is_empty() {
        return Err("no multifinben-englishocr train rows in the manifest".into());
    }
    // step_by over the manifest-order list (grouped by shard) rather than
    // reading a prefix: with stride > 1 this spans all shards instead of
    // biasing toward the first one, which has only 17 of 427 rows.
    let train_rows: Vec<&splits::ManifestRow> =
        all_train_rows.into_iter().step_by(stride).collect();

    let mut sink = Sink4::open(out_dir, "real")?;

    let mut s = RealSummary {
        pages_read: 0,
        lines_total: 0,
        lines_qualifying: 0,
        words_total: 0,
        words_qualifying: 0,
        chars_dumped: 0,
        chars_out_of_charset: 0,
        chars_bad_crop: 0,
    };

    let p = engine.params();

    for row in &train_rows {
        splits::assert_fittable(&manifest, splits::MULTIFINBEN_DATASET, &row.row_id)?;

        let (shard, row_num) = split_row_id(&row.row_id)?;
        let shard_idx = shard_index(&shard)?;
        let stem = format!("filing__s{shard_idx}__r{row_num:06}");
        let pgm_path = pages_dir.join(format!("{stem}.pgm"));
        let truth_path = pages_dir.join(format!("{stem}.truth.json"));

        let pgm_bytes = match std::fs::read(&pgm_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("nn-dump: skipping {}: {e}", pgm_path.display());
                continue;
            }
        };
        let (width, height, data) = page::from_pgm(&pgm_bytes)?;
        let truth_bytes = std::fs::read(&truth_path)
            .map_err(|e| format!("reading {}: {e}", truth_path.display()))?;
        let truth: serde_json::Value = serde_json::from_slice(&truth_bytes)
            .map_err(|e| format!("parsing {}: {e}", truth_path.display()))?;
        let truth_lines = match truth.get("lines").and_then(|v| v.as_array()) {
            Some(a) => a,
            None => {
                eprintln!("nn-dump: {}: no `lines` array, skipping", truth_path.display());
                continue;
            }
        };

        let gray = Gray { width, height, data: &data };
        let decoded = match engine.recognize_lines(gray) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("nn-dump: {}: recognize_lines failed: {e}", pgm_path.display());
                continue;
            }
        };
        s.pages_read += 1;

        // Recompute the mask the engine's own `read_word` cropped ink from,
        // using only the public calls `recognize_lines` itself makes.
        let mask0 = binarize::binarize_with(&Gray { width, height, data: &data }, &p.binarize());
        let slope = deskew::estimate_with(&mask0, width, height, f64::from(p.deskew.max_slope));
        let page_deskewed =
            deskew::correct_with(&Gray { width, height, data: &data }, slope, f64::from(p.deskew.min_corrected_slope));
        let gray2 = page_deskewed.gray();
        let mut mask = binarize::binarize_with(&gray2, &p.binarize());
        let line_p = p.lines();
        if line_p.underline_strip {
            underline::strip_underlines(&mut mask, page_deskewed.width, page_deskewed.height, &line_p);
        }
        let mw = page_deskewed.width;
        let mh = page_deskewed.height;

        s.lines_total += decoded.len().min(truth_lines.len()) as u64;
        for (line_idx, (dline, tline)) in decoded.iter().zip(truth_lines.iter()).enumerate() {
            let Some(truth_text) = tline.as_str() else { continue };
            let truth_tokens: Vec<&str> = truth_text.split_whitespace().collect();
            if dline.words.len() != truth_tokens.len() {
                continue;
            }
            s.lines_qualifying += 1;

            let deskewed_baseline = f64::from(dline.baseline) - f64::from(dline.rect.x) * slope;

            for (word_idx, (dword, ttoken)) in dline.words.iter().zip(truth_tokens.iter()).enumerate() {
                s.words_total += 1;
                let truth_chars: Vec<char> = ttoken.chars().collect();
                if dword.chars.len() != truth_chars.len() {
                    continue;
                }
                s.words_qualifying += 1;

                for (char_idx, (cbox, &tch)) in dword.chars.iter().zip(truth_chars.iter()).enumerate() {
                    let Some(&class_idx) = class_of_char.get(&tch) else {
                        s.chars_out_of_charset += 1;
                        continue;
                    };
                    let rx = cbox.rect.x;
                    let ry = cbox.rect.y;
                    let rw = cbox.rect.width;
                    let rh = cbox.rect.height;
                    if rw == 0
                        || rh == 0
                        || rx.saturating_add(rw) > mw
                        || ry.saturating_add(rh) > mh
                    {
                        s.chars_bad_crop += 1;
                        continue;
                    }
                    let mut ink = vec![0u8; (rw * rh) as usize];
                    for yy in 0..rh {
                        for xx in 0..rw {
                            let src = ((ry + yy) * mw + (rx + xx)) as usize;
                            ink[(yy * rw + xx) as usize] = mask[src];
                        }
                    }
                    let baseline_dy = (deskewed_baseline - f64::from(ry)) as f32;
                    let input =
                        GlyphInput { ink: &ink, width: rw, height: rh, baseline_dy, x_height: dline.x_height };
                    let (raw, grid) = extract_with_grid(&input);
                    let xv = model.standardise(&raw);
                    let top1 = r#match::nearest(model, &raw, 1, true)
                        .and_then(|m| m.top())
                        .map_or(NONE_CLASS, |c| c.class);
                    let meta = format!(
                        "{}\t{}\t{}\t{}\t{}\t{}",
                        row.row_id, stem, line_idx, word_idx, char_idx, tch as u32
                    );
                    sink.write(&grid, &xv, class_idx, top1, &meta)?;
                    s.chars_dumped += 1;
                }
            }
        }
    }

    Ok(s)
}

struct Sink4 {
    g: BufWriter<File>,
    x: BufWriter<File>,
    y: BufWriter<File>,
    top1: BufWriter<File>,
    meta: BufWriter<File>,
}

impl Sink4 {
    fn open(dir: &Path, prefix: &str) -> Result<Sink4, String> {
        let open = |name: &str| -> Result<BufWriter<File>, String> {
            let p = dir.join(format!("{prefix}_{name}"));
            File::create(&p).map(BufWriter::new).map_err(|e| format!("creating {}: {e}", p.display()))
        };
        Ok(Sink4 {
            g: open("G.f32")?,
            x: open("X.f32")?,
            y: open("y.u16")?,
            top1: open("top1.u16")?,
            meta: open("meta.tsv")?,
        })
    }

    fn write(
        &mut self,
        grid: &[[f32; 32]; 32],
        xv: &[f32; FEATURE_DIMS],
        y: u16,
        top1: u16,
        meta_row: &str,
    ) -> Result<(), String> {
        for row in grid {
            for v in row {
                self.g.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
            }
        }
        for v in xv {
            self.x.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
        }
        self.y.write_all(&y.to_le_bytes()).map_err(|e| e.to_string())?;
        self.top1.write_all(&top1.to_le_bytes()).map_err(|e| e.to_string())?;
        writeln!(self.meta, "{meta_row}").map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn split_row_id(row_id: &str) -> Result<(String, u32), String> {
    let (shard, row) =
        row_id.split_once('#').ok_or_else(|| format!("row id {row_id:?} has no '#'"))?;
    let row_num: u32 = row.parse().map_err(|_| format!("bad row number in {row_id:?}"))?;
    Ok((shard.to_string(), row_num))
}

fn shard_index(shard: &str) -> Result<u32, String> {
    let after_dash = shard
        .split('-')
        .nth(1)
        .ok_or_else(|| format!("shard name {shard:?} has no '-NNNNN-' segment"))?;
    after_dash.parse().map_err(|_| format!("bad shard index in {shard:?}"))
}
