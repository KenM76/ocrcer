//! `skew-probe`: chunk 15 diagnosis only (`docs/ARCHITECTURE.md` §11, "Chunk
//! 15 step 2 fails" -- hypothesis (1), train/serve skew). No parameter,
//! weight or default is written or changed here; this reads an already-built
//! model and already-dumped trainer data and reports a comparison.
//!
//! For a sample of real, on-path character crops the trainer actually saw
//! (`nn15b/real_{train,val}_*`, written by `nn15_dump.rs`'s (b) real-crop
//! path), this rebuilds the *runtime* candidate for the same character --
//! `segment::crop`'s label-filtered ink and `TextLine::baseline_dy`'s
//! unrounded float baseline, exactly what `pipeline.rs::read_word` feeds
//! every live edge via `Glyph::input` -- and diffs it against what
//! `nn15_dump.rs` wrote to disk for that character.
//!
//! `nn15_dump.rs`'s own (b) path does not call `Glyph::input`/`segment::crop`
//! for its *positive* rows: it re-derives `ink` with a flat rectangular copy
//! of the recomputed mask at the decoded `CharBox.rect` (no label filter),
//! and re-derives `baseline_dy` by undoing `pipeline.rs`'s `unshear` on the
//! already-rounded, already-unsheared `Line.baseline` it decoded rather than
//! reading the internal `TextLine`'s own unrounded value (which it has in
//! scope for its own (c)(ii) negatives, and uses correctly there). Both are
//! candidate skew sources this binary measures independently: ink-filtering
//! changes `G` (and everything `X` derives from `G`); baseline reconstruction
//! changes only `X`'s two baseline-relative geometry dims (105, 106).
//!
//! # Usage
//!
//! ```text
//! skew-probe <model.ocrw> <pages-train-dir> <nn15b-dir> <split-file> [per-stem-cap]
//! ```
//!
//! `pages-train-dir` is `finfilings-train` (the full 427-page set nn15b's
//! stems are drawn from), `nn15b-dir` is the trainer dump directory,
//! `split-file` is `bench/splits/nn15_page_split.tsv` (read only, to name
//! which 107 stems are "training set" pages, matching `nn15_dump.rs`'s own
//! contract). `per-stem-cap` (default 4) bounds how many of a stem's rows are
//! sampled -- the per-page decode cost is paid once regardless, so this only
//! bounds output size.
//!
//! Never touches `finfilings`, `finfilings-val`, `pages-cov`, or any fixture.

use ocrcer_build::page;
use ocrcer_core::feature::{extract_with_grid, FEATURE_DIMS};
use ocrcer_core::image::{binarize, components, deskew};
use ocrcer_core::layout::{lines, segment, underline, words};
use ocrcer_core::ocrw::Model;
use ocrcer_core::{Engine, Gray};

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const GRID: usize = 32;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!(
            "usage: skew-probe <model.ocrw> <pages-train-dir> <nn15b-dir> <split-file> [per-stem-cap]"
        );
        return ExitCode::FAILURE;
    }
    let model_path = PathBuf::from(&args[0]);
    let pages_dir = PathBuf::from(&args[1]);
    let dump_dir = PathBuf::from(&args[2]);
    let split_file = PathBuf::from(&args[3]);
    let cap: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(4);

    let model_bytes = match std::fs::read(&model_path) {
        Ok(b) => b,
        Err(e) => return fail(&format!("reading {}: {e}", model_path.display())),
    };
    let model = match Model::load(&model_bytes) {
        Ok(m) => m,
        Err(e) => return fail(&format!("loading model: {e}")),
    };
    let engine = match Engine::from_bytes(&model_bytes) {
        Ok(e) => e,
        Err(e) => return fail(&format!("loading engine: {e}")),
    };
    let Some(net) = model.nn.as_ref() else {
        return fail("model has no nn table; build with --nn first");
    };

    let split_stems: Vec<String> = match load_split_stems(&split_file) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    eprintln!("skew-probe: {} split stems", split_stems.len());

    let mut rows_by_stem: BTreeMap<String, (Vec<MetaRow>, Vec<MetaRow>)> = BTreeMap::new();
    for stem in &split_stems {
        rows_by_stem.insert(stem.clone(), (Vec::new(), Vec::new()));
    }
    for (fold_name, is_val) in [("real_train", false), ("real_val", true)] {
        let meta_path = dump_dir.join(format!("{fold_name}_meta.tsv"));
        let rows = match load_meta(&meta_path) {
            Ok(r) => r,
            Err(e) => return fail(&e),
        };
        for (idx, row) in rows.into_iter().enumerate() {
            if let Some(entry) = rows_by_stem.get_mut(&row.stem) {
                let mr = MetaRow { row_index: idx, is_val, ..row };
                if is_val {
                    entry.1.push(mr);
                } else {
                    entry.0.push(mr);
                }
            }
        }
    }

    let g_bytes = GRID * GRID * 4;
    let x_bytes = FEATURE_DIMS * 4;
    let mut g_train = match File::open(dump_dir.join("real_train_G.f32")) {
        Ok(f) => f,
        Err(e) => return fail(&format!("opening real_train_G.f32: {e}")),
    };
    let mut x_train = match File::open(dump_dir.join("real_train_X.f32")) {
        Ok(f) => f,
        Err(e) => return fail(&format!("opening real_train_X.f32: {e}")),
    };
    let mut y_train = match File::open(dump_dir.join("real_train_y.u16")) {
        Ok(f) => f,
        Err(e) => return fail(&format!("opening real_train_y.u16: {e}")),
    };
    let mut g_val = match File::open(dump_dir.join("real_val_G.f32")) {
        Ok(f) => f,
        Err(e) => return fail(&format!("opening real_val_G.f32: {e}")),
    };
    let mut x_val = match File::open(dump_dir.join("real_val_X.f32")) {
        Ok(f) => f,
        Err(e) => return fail(&format!("opening real_val_X.f32: {e}")),
    };
    let mut y_val = match File::open(dump_dir.join("real_val_y.u16")) {
        Ok(f) => f,
        Err(e) => return fail(&format!("opening real_val_y.u16: {e}")),
    };

    let mut stats = Stats::default();
    let p = engine.params();

    for stem in &split_stems {
        let (train_rows, val_rows) = &rows_by_stem[stem];
        let sample: Vec<&MetaRow> =
            sample_rows(train_rows, cap.div_ceil(2)).chain(sample_rows(val_rows, cap / 2)).collect();
        if sample.is_empty() {
            continue;
        }

        let pgm_path = pages_dir.join(format!("{stem}.pgm"));
        let truth_path = pages_dir.join(format!("{stem}.truth.json"));
        let pgm_bytes = match std::fs::read(&pgm_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skew-probe: skipping {stem}: {e}");
                continue;
            }
        };
        let (width, height, data) = match page::from_pgm(&pgm_bytes) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("skew-probe: {stem}: bad pgm: {e}");
                continue;
            }
        };
        let _truth_bytes = match std::fs::read(&truth_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skew-probe: {stem}: {e}");
                continue;
            }
        };

        let gray = Gray { width, height, data: &data };
        let decoded = match engine.recognize_lines(gray) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("skew-probe: {stem}: recognize_lines failed: {e}");
                continue;
            }
        };

        // Same reconstruction `nn15_dump.rs` uses -- public calls only, same
        // order, never a second segmentation implementation.
        let mask0 = binarize::binarize_with(&Gray { width, height, data: &data }, &p.binarize());
        let slope = deskew::estimate_with(&mask0, width, height, f64::from(p.deskew.max_slope));
        let page_deskewed = deskew::correct_with(
            &Gray { width, height, data: &data },
            slope,
            f64::from(p.deskew.min_corrected_slope),
        );
        let gray2 = page_deskewed.gray();
        let mut mask = binarize::binarize_with(&gray2, &p.binarize());
        let line_p = p.lines();
        let (labels, comps) = if line_p.underline_strip {
            let stripped =
                underline::strip_underlines(&mut mask, page_deskewed.width, page_deskewed.height, &line_p);
            (stripped.labels, stripped.components)
        } else {
            let (labels, count) = components::label(
                &mask,
                page_deskewed.width,
                page_deskewed.height,
                components::Connectivity::Eight,
            );
            let comps = components::components(&labels, page_deskewed.width, page_deskewed.height, count);
            (labels, comps)
        };
        let mw = page_deskewed.width;
        let mh = page_deskewed.height;

        let word_p = p.words();
        let seg_p = p.segment();
        let bands = if comps.is_empty() { Vec::new() } else { lines::group_with_bands(&comps, mw, mh, &line_p) };
        let mut rebuilt: Vec<(lines::TextLine, Vec<words::WordSpan>)> = Vec::new();
        for group in &bands {
            let spans_by_line = words::split_band_with(group, &comps, &word_p);
            for (tl, spans) in group.iter().zip(spans_by_line) {
                rebuilt.push((tl.clone(), spans));
            }
        }

        for mr in sample {
            stats.sampled += 1;
            let Some(dline) = decoded.get(mr.line_idx) else {
                stats.oob += 1;
                continue;
            };
            let Some(dword) = dline.words.get(mr.word_idx) else {
                stats.oob += 1;
                continue;
            };
            let Some(cbox) = dword.chars.get(mr.char_idx) else {
                stats.oob += 1;
                continue;
            };
            if cbox.ch as u32 != mr.codepoint {
                // The re-decode disagrees with what nn15b recorded for this
                // exact (stem, line, word, char) -- a config drift the
                // build_id check should have already ruled out; skip and
                // count rather than compare mismatched characters.
                stats.codepoint_mismatch += 1;
                continue;
            }

            let wx0 = dword.rect.x;
            let wx1 = dword.rect.x + dword.rect.width;
            let wy_mid = dword.rect.y + dword.rect.height / 2;
            let candidate = rebuilt.iter().find(|(tl, _)| wy_mid >= tl.y0 && wy_mid < tl.y1).and_then(
                |(tl, spans)| {
                    spans
                        .iter()
                        .filter(|sp| sp.x1 > wx0 && sp.x0 < wx1)
                        .max_by_key(|sp| overlap_len(sp.x0, sp.x1, wx0, wx1))
                        .map(|sp| (tl.clone(), sp.clone()))
                },
            );
            let Some((tl, span)) = candidate else {
                stats.no_word_span += 1;
                continue;
            };
            let lat = segment::build_with(&span, &comps, &labels, mw, &tl, &seg_p);
            let want = (cbox.rect.x, cbox.rect.y, cbox.rect.width, cbox.rect.height);
            let mut found: Option<segment::Glyph> = None;
            for e in &lat.edges {
                if let Some(g) = segment::crop(&lat, &labels, mw, e) {
                    if (g.x, g.y, g.width, g.height) == want {
                        found = Some(g);
                        break;
                    }
                }
            }
            let Some(glyph) = found else {
                stats.no_edge_match += 1;
                continue;
            };

            let input = glyph.input(&tl);
            let (raw_true, grid_true) = extract_with_grid(&input);
            let x_true = model.standardise(&raw_true);

            let (g_file, x_file, y_file) =
                if mr.is_val { (&mut g_val, &mut x_val, &mut y_val) } else { (&mut g_train, &mut x_train, &mut y_train) };
            let mut g_buf = vec![0u8; g_bytes];
            if let Err(e) = read_at(g_file, mr.row_index * g_bytes, &mut g_buf) {
                eprintln!("skew-probe: reading dumped G row {}: {e}", mr.row_index);
                continue;
            }
            let mut x_buf = vec![0u8; x_bytes];
            if let Err(e) = read_at(x_file, mr.row_index * x_bytes, &mut x_buf) {
                eprintln!("skew-probe: reading dumped X row {}: {e}", mr.row_index);
                continue;
            }
            let mut y_buf = [0u8; 2];
            if let Err(e) = read_at(y_file, mr.row_index * 2, &mut y_buf) {
                eprintln!("skew-probe: reading dumped y row {}: {e}", mr.row_index);
                continue;
            }
            let y_dump = u16::from_le_bytes(y_buf);

            let g_dump = bytes_to_grid(&g_buf);
            let x_dump = bytes_to_x(&x_buf);

            stats.matched += 1;
            let mut g_max = 0f32;
            let mut g_sum = 0f64;
            for r in 0..GRID {
                for c in 0..GRID {
                    let d = (grid_true[r][c] - g_dump[r][c]).abs();
                    g_max = g_max.max(d);
                    g_sum += f64::from(d);
                }
            }
            let g_mean = g_sum / (GRID * GRID) as f64;
            stats.g_max = stats.g_max.max(g_max);
            stats.g_mean_sum += g_mean;
            if g_max > 1e-9 {
                stats.g_nonzero += 1;
            }

            let mut x_max = 0f32;
            let mut x_sum = 0f64;
            let mut x_geom_max = 0f32;
            let mut x_nongeom_max = 0f32;
            for i in 0..FEATURE_DIMS {
                let d = (x_true[i] - x_dump[i]).abs();
                x_max = x_max.max(d);
                x_sum += f64::from(d);
                if i == 105 || i == 106 {
                    x_geom_max = x_geom_max.max(d);
                } else {
                    x_nongeom_max = x_nongeom_max.max(d);
                }
            }
            stats.x_max = stats.x_max.max(x_max);
            stats.x_mean_sum += x_sum / FEATURE_DIMS as f64;
            if x_geom_max > 1e-6 {
                stats.x_baseline_dims_nonzero += 1;
            }
            if x_nongeom_max > 1e-6 {
                stats.x_other_dims_nonzero += 1;
            }

            // Net accuracy: dump-fed (what training saw) vs runtime-fed
            // (what the pipeline would actually serve), same trained
            // weights, excluding the junk class exactly as
            // `pipeline::nn_candidates` does.
            if let Ok(lp_dump) = net.forward(&g_dump, &x_dump) {
                if top1_excluding_junk(&lp_dump, net.junk_index as usize) == y_dump {
                    stats.correct_dump += 1;
                }
            }
            if let Ok(lp_true) = net.forward(&grid_true, &x_true) {
                if top1_excluding_junk(&lp_true, net.junk_index as usize) == y_dump {
                    stats.correct_true += 1;
                }
            }
        }
    }

    println!("{}", stats.report());
    ExitCode::SUCCESS
}

#[derive(Default)]
struct Stats {
    sampled: u64,
    oob: u64,
    codepoint_mismatch: u64,
    no_word_span: u64,
    no_edge_match: u64,
    matched: u64,
    g_max: f32,
    g_mean_sum: f64,
    g_nonzero: u64,
    x_max: f32,
    x_mean_sum: f64,
    x_baseline_dims_nonzero: u64,
    x_other_dims_nonzero: u64,
    correct_dump: u64,
    correct_true: u64,
}

impl Stats {
    fn report(&self) -> String {
        let m = self.matched.max(1) as f64;
        format!(
            "{{\n  \"sampled\": {},\n  \"out_of_bounds\": {},\n  \"codepoint_mismatch\": {},\n  \
             \"no_word_span\": {},\n  \"no_edge_match\": {},\n  \"matched\": {},\n  \
             \"delta_G_max\": {:.6},\n  \"delta_G_mean\": {:.6},\n  \"delta_G_nonzero_rows\": {},\n  \
             \"delta_X_max\": {:.6},\n  \"delta_X_mean\": {:.6},\n  \
             \"delta_X_baseline_dims_nonzero_rows\": {},\n  \"delta_X_other_dims_nonzero_rows\": {},\n  \
             \"accuracy_dump_fed\": {:.4},\n  \"accuracy_runtime_fed\": {:.4}\n}}",
            self.sampled,
            self.oob,
            self.codepoint_mismatch,
            self.no_word_span,
            self.no_edge_match,
            self.matched,
            self.g_max,
            self.g_mean_sum / m,
            self.g_nonzero,
            self.x_max,
            self.x_mean_sum / m,
            self.x_baseline_dims_nonzero,
            self.x_other_dims_nonzero,
            self.correct_dump as f64 / m,
            self.correct_true as f64 / m,
        )
    }
}

fn top1_excluding_junk(log_probs: &[f32], junk_index: usize) -> u16 {
    let mut best_i = 0usize;
    let mut best_v = f32::NEG_INFINITY;
    for (i, &v) in log_probs.iter().enumerate() {
        if i == junk_index {
            continue;
        }
        if v > best_v {
            best_v = v;
            best_i = i;
        }
    }
    best_i as u16
}

fn overlap_len(a0: u32, a1: u32, b0: u32, b1: u32) -> u32 {
    let lo = a0.max(b0);
    let hi = a1.min(b1);
    hi.saturating_sub(lo)
}

fn read_at(f: &mut File, offset: usize, buf: &mut [u8]) -> std::io::Result<()> {
    f.seek(SeekFrom::Start(offset as u64))?;
    f.read_exact(buf)
}

fn bytes_to_grid(buf: &[u8]) -> [[f32; GRID]; GRID] {
    let mut out = [[0f32; GRID]; GRID];
    let mut i = 0;
    for row in out.iter_mut() {
        for v in row.iter_mut() {
            *v = f32::from_le_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]);
            i += 4;
        }
    }
    out
}

fn bytes_to_x(buf: &[u8]) -> [f32; FEATURE_DIMS] {
    let mut out = [0f32; FEATURE_DIMS];
    for (i, v) in out.iter_mut().enumerate() {
        let o = i * 4;
        *v = f32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    }
    out
}

#[derive(Clone)]
struct MetaRow {
    stem: String,
    line_idx: usize,
    word_idx: usize,
    char_idx: usize,
    codepoint: u32,
    row_index: usize,
    is_val: bool,
}

fn load_meta(path: &Path) -> Result<Vec<MetaRow>, String> {
    let f = File::open(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line.map_err(|e| e.to_string())?;
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 6 {
            continue;
        }
        out.push(MetaRow {
            stem: cols[1].to_string(),
            line_idx: cols[2].parse().map_err(|_| format!("bad line_idx in {line:?}"))?,
            word_idx: cols[3].parse().map_err(|_| format!("bad word_idx in {line:?}"))?,
            char_idx: cols[4].parse().map_err(|_| format!("bad char_idx in {line:?}"))?,
            codepoint: cols[5].parse().map_err(|_| format!("bad codepoint in {line:?}"))?,
            row_index: 0,
            is_val: false,
        });
    }
    Ok(out)
}

fn load_split_stems(path: &Path) -> Result<Vec<String>, String> {
    let f = File::open(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(stem) = line.split('\t').next() {
            out.push(stem.to_string());
        }
    }
    Ok(out)
}

/// Evenly spaced indices into `rows`, up to `cap`, so a stem's sample spans
/// its page rather than clustering on its first characters.
fn sample_rows(rows: &[MetaRow], cap: usize) -> std::vec::IntoIter<&MetaRow> {
    if cap == 0 || rows.is_empty() {
        return Vec::new().into_iter();
    }
    let n = rows.len();
    let take = cap.min(n);
    let mut out = Vec::with_capacity(take);
    for k in 0..take {
        let idx = if take == 1 { 0 } else { k * (n - 1) / (take - 1) };
        out.push(&rows[idx]);
    }
    out.into_iter()
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("skew-probe: {msg}");
    ExitCode::FAILURE
}
