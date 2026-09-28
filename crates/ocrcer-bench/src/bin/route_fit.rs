//! `route-fit`: chunk 15b's train-fold-only grid fit for the router
//! (`docs/ARCHITECTURE.md` §11, 2026-09-27, "chunk 15b pre-registered").
//!
//! Reads real character crops through the exact runtime crop path
//! (`segment::build_with`/`segment::crop`/`Glyph::input`, the same
//! alignment gate `gen15_probe.rs` and `nn15_dump.rs` use — never a second
//! segmentation implementation, `CLAUDE.md` rule 4), computes the matcher's
//! `Match` (`d1`/`d2`/`ratio`) and the network's log-probabilities on the
//! identical extracted `G`/`X` for every qualifying crop, then sweeps a
//! grid of `(route.matcher_margin, route.net_prob)` replaying
//! `pipeline.rs::read_word`'s exact router formula from those two cached
//! per-crop numbers — never a second implementation of the relabel
//! decision, only `nn_candidates` (now `pub`, chunk 15b) called the same
//! way the runtime calls it.
//!
//! **Train fold only.** `--split-file` is `bench/splits/nn15_page_split.tsv`;
//! this binary keeps only stems marked fold `A` and refuses (hard error) a
//! `pages-dir` whose name does not end in `-train` — the same firewall
//! `nn15_dump.rs::assert_train_dir_name` enforces, copied rather than
//! imported since it is a one-line guard, not a pipeline stage. Fold `B` is
//! the net's own internal validation and is never read here. This binary
//! never opens `finfilings`, `finfilings-val`, `pages-cov`, any fixture, or
//! `bench/ident`.
//!
//! # Usage
//!
//! ```text
//! route-fit <label> <model.ocrw> <pages-train-dir> <split-file>
//! ```
//!
//! Prints one JSON object: the full grid (train-fold accuracy per
//! `(matcher_margin, net_prob)` point) plus the best point and a reliability
//! table for the relabelled glyphs at that point (empirical accuracy per
//! calibrated-confidence decile, checking whether the existing
//! `confidence::AUTHORED` curve — authored for the matcher's ratio, reused
//! unchanged for the network's own margin-derived ratio — still holds for
//! glyphs the router actually relabels).

use ocrcer_bench::pages;
use ocrcer_build::page;
use ocrcer_core::confidence;
use ocrcer_core::feature::extract_with_grid;
use ocrcer_core::image::{binarize, components, deskew};
use ocrcer_core::layout::{lines, segment, underline, words};
use ocrcer_core::ocrw::Model;
use ocrcer_core::pipeline::nn_candidates;
use ocrcer_core::{r#match, Engine, Gray};

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The grid this fit sweeps, both dimensions. Not measured or derived —
/// chosen as a coarse-then-visible-enough scan of the confidence unit
/// interval; the winning point and its immediate neighbours are what
/// matters, not the grid's own resolution.
const MATCHER_MARGIN_GRID: &[f32] = &[
    0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95,
];
const NET_PROB_GRID: &[f32] = &[
    0.50, 0.60, 0.70, 0.80, 0.85, 0.90, 0.93, 0.95, 0.97, 0.99,
];

struct Crop {
    truth_class: u16,
    matcher_class: Option<u16>,
    matcher_ratio: f32,
    net_class: Option<u16>,
    /// `exp(-distance)` at `scale = 1.0`: the network's own top-charset
    /// probability, exactly what `pipeline.rs`'s relabel pass thresholds.
    net_prob: f32,
    /// The network's own margin-derived ratio (`nn_candidates`'s `.ratio`),
    /// for the reliability check only — never used in the relabel decision
    /// itself.
    net_ratio: f32,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!("usage: route-fit <label> <model.ocrw> <pages-train-dir> <split-file>");
        return ExitCode::FAILURE;
    }
    let label = &args[0];
    let model_path = PathBuf::from(&args[1]);
    let pages_dir = args[2].clone();
    let split_file = &args[3];

    if let Err(e) = assert_train_dir_name(Path::new(&pages_dir)) {
        return fail(&e);
    }

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
    let cal = confidence::Calibration {
        lm_floor: model.params.confidence.lm_floor,
        ..confidence::AUTHORED
    };

    let fold_a_stems = match load_fold_a_stems(split_file) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    eprintln!("route-fit [{label}]: {} fold-A stems", fold_a_stems.len());

    let mut class_of_char: BTreeMap<char, u16> = BTreeMap::new();
    for c in &model.classes {
        class_of_char.insert(c.codepoint, c.index);
    }

    let all_pgms = match pages::list_pages(&pages_dir) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let chosen: Vec<PathBuf> = all_pgms
        .into_iter()
        .filter(|p| {
            p.file_stem().map(|s| fold_a_stems.contains(&s.to_string_lossy().into_owned())).unwrap_or(false)
        })
        .collect();
    eprintln!("route-fit [{label}]: {} pages chosen (fold A only)", chosen.len());

    let p = engine.params();
    let mut crops: Vec<Crop> = Vec::new();
    let mut pages_read = 0u64;

    for pgm_path in &chosen {
        let stem = pgm_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let truth_path = pgm_path.with_file_name(format!("{stem}.truth.json"));
        let Ok(pgm_bytes) = std::fs::read(pgm_path) else { continue };
        let Ok((width, height, data)) = page::from_pgm(&pgm_bytes) else { continue };
        let Ok(truth_bytes) = std::fs::read(&truth_path) else { continue };
        let Ok(truth) = serde_json::from_slice::<serde_json::Value>(&truth_bytes) else { continue };
        let Some(truth_lines) = truth.get("lines").and_then(|v| v.as_array()) else { continue };

        let gray = Gray { width, height, data: &data };
        let Ok(decoded) = engine.recognize_lines(gray) else { continue };
        pages_read += 1;

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
        let _ = mh;

        for (dline, tline) in decoded.iter().zip(truth_lines.iter()) {
            let Some(truth_text) = tline.as_str() else { continue };
            let truth_tokens: Vec<&str> = truth_text.split_whitespace().collect();
            if dline.words.len() != truth_tokens.len() {
                continue;
            }
            for (dword, ttoken) in dline.words.iter().zip(truth_tokens.iter()) {
                let truth_chars: Vec<char> = ttoken.chars().collect();
                if dword.chars.len() != truth_chars.len() {
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
                let Some((tl, span)) = candidate else { continue };
                let lat = segment::build_with(&span, &comps, &labels, mw, &tl, &seg_p);

                for (cbox, &tch) in dword.chars.iter().zip(truth_chars.iter()) {
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
                    let Some(glyph) = found else { continue };
                    let Some(&truth_class) = class_of_char.get(&tch) else { continue };

                    let input = glyph.input(&tl);
                    let (raw, grid) = extract_with_grid(&input);
                    let xv = model.standardise(&raw);

                    let mtch = r#match::nearest(&model, &raw, 1, true);
                    let matcher_class = mtch.as_ref().and_then(|m| m.top()).map(|c| c.class);
                    let matcher_ratio = mtch.as_ref().map(|m| m.ratio()).unwrap_or(1.0);

                    let mut net_class = None;
                    let mut net_prob = 0.0f32;
                    let mut net_ratio = 1.0f32;
                    if let Ok(lp) = net.forward(&grid, &xv) {
                        if let Some(top) = nn_candidates(net, &lp, 1, 1.0).into_iter().next() {
                            net_class = Some(top.class);
                            net_prob = (-top.distance).exp();
                            net_ratio = top.ratio;
                        }
                    }

                    crops.push(Crop {
                        truth_class,
                        matcher_class,
                        matcher_ratio,
                        net_class,
                        net_prob,
                        net_ratio,
                    });
                }
            }
        }
    }
    eprintln!("route-fit [{label}]: {pages_read} pages read, {} crops scored", crops.len());

    // Baseline: classifier 0 (no router), the number every grid point is
    // measured against.
    let baseline_correct = crops.iter().filter(|c| c.matcher_class == Some(c.truth_class)).count();
    let baseline_acc = baseline_correct as f64 / crops.len().max(1) as f64;

    let mut best: Option<(f32, f32, f64, u64, u64)> = None;
    let mut grid_rows: Vec<(f32, f32, f64, u64, u64)> = Vec::new();
    for &mm in MATCHER_MARGIN_GRID {
        for &np in NET_PROB_GRID {
            let mut correct = 0u64;
            let mut relabels = 0u64;
            for c in &crops {
                let final_class = route_decide(c, &cal, mm, np, &mut relabels);
                if final_class == Some(c.truth_class) {
                    correct += 1;
                }
            }
            let acc = correct as f64 / crops.len().max(1) as f64;
            grid_rows.push((mm, np, acc, correct, relabels));
            let better = match best {
                None => true,
                // Ties: prefer the higher net_prob threshold (more
                // conservative — fewer relabels for the same accuracy), then
                // the higher matcher_margin (routes more candidates through
                // the check rather than fewer), matching the "no invented
                // aggressiveness" spirit of the rest of this project's
                // tie-breaks (deterministic, stated, not a coin flip).
                Some((bmm, bnp, bacc, _, _)) => {
                    acc > bacc || (acc == bacc && (np > bnp || (np == bnp && mm > bmm)))
                }
            };
            if better {
                best = Some((mm, np, acc, correct, relabels));
            }
        }
    }
    let (best_mm, best_np, best_acc, best_correct, best_relabels) = best.unwrap_or((0.8, 0.9, baseline_acc, 0, 0));

    // Reliability check at the winning point: bucket relabelled glyphs by
    // their (post-relabel) calibrated confidence decile, report empirical
    // accuracy per bucket against the AUTHORED curve reused unchanged.
    let mut buckets: BTreeMap<u32, (u64, u64)> = BTreeMap::new();
    let mut dummy_relabels = 0u64;
    for c in &crops {
        let Some(nc) = c.net_class else { continue };
        let matcher_conf = confidence::character(&cal, c.matcher_ratio);
        if matcher_conf >= best_mm || c.net_prob < best_np || Some(nc) == c.matcher_class {
            continue;
        }
        // This crop is a relabel at the winning point.
        let _ = route_decide(c, &cal, best_mm, best_np, &mut dummy_relabels);
        let conf = confidence::character(&cal, c.net_ratio);
        let decile = ((conf * 10.0).floor() as u32).min(9);
        let e = buckets.entry(decile).or_insert((0, 0));
        e.0 += 1;
        if nc == c.truth_class {
            e.1 += 1;
        }
    }

    print_report(
        label,
        &crops,
        baseline_acc,
        baseline_correct as u64,
        &grid_rows,
        best_mm,
        best_np,
        best_acc,
        best_correct,
        best_relabels,
        &buckets,
    );
    ExitCode::SUCCESS
}

/// Replays `pipeline.rs::read_word`'s router formula from one crop's cached
/// matcher/net numbers. `relabels` is incremented, not returned, so the
/// grid-sweep caller can also tally the relabel rate per point without a
/// second pass over `crops`.
fn route_decide(
    c: &Crop,
    cal: &confidence::Calibration,
    matcher_margin: f32,
    net_prob_threshold: f32,
    relabels: &mut u64,
) -> Option<u16> {
    let matcher_conf = confidence::character(cal, c.matcher_ratio);
    if matcher_conf < matcher_margin {
        if let Some(nc) = c.net_class {
            if c.net_prob >= net_prob_threshold && Some(nc) != c.matcher_class {
                *relabels += 1;
                return Some(nc);
            }
        }
    }
    c.matcher_class
}

#[allow(clippy::too_many_arguments)]
fn print_report(
    label: &str,
    crops: &[Crop],
    baseline_acc: f64,
    baseline_correct: u64,
    grid_rows: &[(f32, f32, f64, u64, u64)],
    best_mm: f32,
    best_np: f32,
    best_acc: f64,
    best_correct: u64,
    best_relabels: u64,
    buckets: &BTreeMap<u32, (u64, u64)>,
) {
    println!("{{");
    println!("  \"label\": {:?},", label);
    println!("  \"n_crops\": {},", crops.len());
    println!("  \"baseline_matcher_accuracy\": {:.6},", baseline_acc);
    println!("  \"baseline_correct\": {},", baseline_correct);
    println!("  \"grid\": [");
    for (i, (mm, np, acc, correct, relabels)) in grid_rows.iter().enumerate() {
        if i > 0 {
            println!(",");
        }
        print!(
            "    {{\"matcher_margin\": {mm}, \"net_prob\": {np}, \"accuracy\": {acc:.6}, \"correct\": {correct}, \"relabels\": {relabels}}}"
        );
    }
    println!();
    println!("  ],");
    println!("  \"best_matcher_margin\": {best_mm},");
    println!("  \"best_net_prob\": {best_np},");
    println!("  \"best_accuracy\": {best_acc:.6},");
    println!("  \"best_correct\": {best_correct},");
    println!("  \"best_relabels\": {best_relabels},");
    println!("  \"reliability\": [");
    let mut first = true;
    for (decile, (n, correct)) in buckets {
        if !first {
            println!(",");
        }
        first = false;
        let lo = *decile as f32 / 10.0;
        let hi = lo + 0.1;
        print!(
            "    {{\"decile\": [{lo:.1}, {hi:.1}], \"n\": {n}, \"empirical_accuracy\": {:.4}}}",
            *correct as f64 / (*n).max(1) as f64
        );
    }
    println!();
    println!("  ]");
    println!("}}");
}

fn overlap_len(a0: u32, a1: u32, b0: u32, b1: u32) -> u32 {
    let lo = a0.max(b0);
    let hi = a1.min(b1);
    hi.saturating_sub(lo)
}

/// Same firewall `nn15_dump.rs::assert_train_dir_name` enforces, copied
/// (one-line guard, not a pipeline stage) so this binary refuses a
/// non-train directory on its own rather than trusting the caller.
fn assert_train_dir_name(dir: &Path) -> Result<(), String> {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: cannot read a directory name", dir.display()))?;
    if name.ends_with("-train") {
        Ok(())
    } else {
        Err(format!("{}: refusing to run against a directory not named `*-train`", dir.display()))
    }
}

/// Fold-`A` stems only (`bench/splits/nn15_page_split.tsv`'s `#stem\tfold\t
/// cluster_id`) — fold `B` is the net's own internal validation and stays
/// unread here, per this binary's module doc.
fn load_fold_a_stems(path: &str) -> Result<BTreeSet<String>, String> {
    use std::io::BufRead;
    let f = std::fs::File::open(path).map_err(|e| format!("reading {path}: {e}"))?;
    let mut out = BTreeSet::new();
    for line in std::io::BufReader::new(f).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            continue;
        }
        if cols[1] == "A" {
            out.insert(cols[0].to_string());
        }
    }
    Ok(out)
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("route-fit: {msg}");
    ExitCode::FAILURE
}
