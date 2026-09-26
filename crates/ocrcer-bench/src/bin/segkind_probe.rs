//! `segkind-probe`: chunk 15 diagnosis only (`docs/ARCHITECTURE.md` §11,
//! "Chunk 15 step 2 fails" -- hypothesis (2), candidates the net never saw
//! trained on). Cheap check, per the assigned task's step 4: on unseen
//! pages, what fraction of the decoder's winning lattice candidates are
//! mis-segments (`EdgeKind::Merge`/`Split` rather than `Single`), and does
//! that fraction skew towards high confidence more under `match.classifier=1`
//! than under `match.classifier=0`.
//!
//! This is a self-referential check against the decoder's own edge-kind
//! bookkeeping, not against ground truth -- it costs one page decode per
//! page, per classifier arm, with no alignment/scoring logic. It does not
//! read or score against `finfilings`, `finfilings-val`, `pages-cov`, or any
//! fixture; `finfilings-train-unseen` is a distinct, allowed directory.
//!
//! # Usage
//!
//! ```text
//! segkind-probe <model.ocrw> <pages-dir> [n-pages]
//! ```

use ocrcer_build::page;
use ocrcer_core::image::{binarize, components, deskew};
use ocrcer_core::layout::segment::EdgeKind;
use ocrcer_core::layout::{lines, segment, underline, words};
use ocrcer_core::pipeline::Engine;
use ocrcer_core::Gray;

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: segkind-probe <model.ocrw> <pages-dir> [n-pages]");
        return ExitCode::FAILURE;
    }
    let model_path = PathBuf::from(&args[0]);
    let pages_dir = PathBuf::from(&args[1]);
    let n_pages: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(24);

    let bytes = match std::fs::read(&model_path) {
        Ok(b) => b,
        Err(e) => return fail(&format!("reading {}: {e}", model_path.display())),
    };

    let mut stems: Vec<String> = match std::fs::read_dir(&pages_dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect(),
        Err(e) => return fail(&format!("reading {}: {e}", pages_dir.display())),
    };
    stems.sort();
    stems.dedup();
    stems.truncate(n_pages);
    eprintln!("segkind-probe: {} pages", stems.len());

    for (arm_name, classifier) in [("classifier=0 (matcher)", 0.0f32), ("classifier=1 (net)", 1.0f32)] {
        let mut engine = match Engine::from_bytes(&bytes) {
            Ok(e) => e,
            Err(e) => return fail(&format!("loading engine: {e}")),
        };
        if classifier > 0.5 {
            if engine.model().nn.is_none() {
                return fail("model has no nn table; build with --nn first");
            }
            if !ocrcer_bench::knobs::set(&mut engine, "match.classifier", classifier) {
                return fail("engine refused match.classifier override");
            }
        }
        let p = engine.params();

        let mut total_chars = 0u64;
        let mut merge_or_split = 0u64;
        let mut high_conf = 0u64;
        let mut high_conf_merge_or_split = 0u64;
        let mut no_edge_match = 0u64;
        let mut sum_conf_single = 0f64;
        let mut n_single = 0u64;
        let mut sum_conf_ms = 0f64;
        let mut n_ms = 0u64;

        for stem in &stems {
            let pgm_path = pages_dir.join(format!("{stem}.pgm"));
            let pgm_bytes = match std::fs::read(&pgm_path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let (width, height, data) = match page::from_pgm(&pgm_bytes) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let gray = Gray { width, height, data: &data };
            let decoded = match engine.recognize_lines(gray) {
                Ok(l) => l,
                Err(_) => continue,
            };
            if decoded.is_empty() {
                continue;
            }

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
                let stripped = underline::strip_underlines(
                    &mut mask,
                    page_deskewed.width,
                    page_deskewed.height,
                    &line_p,
                );
                (stripped.labels, stripped.components)
            } else {
                let (labels, count) = components::label(
                    &mask,
                    page_deskewed.width,
                    page_deskewed.height,
                    components::Connectivity::Eight,
                );
                let comps =
                    components::components(&labels, page_deskewed.width, page_deskewed.height, count);
                (labels, comps)
            };
            let mw = page_deskewed.width;
            let mh = page_deskewed.height;

            let word_p = p.words();
            let seg_p = p.segment();
            let bands =
                if comps.is_empty() { Vec::new() } else { lines::group_with_bands(&comps, mw, mh, &line_p) };
            let mut rebuilt: Vec<(lines::TextLine, Vec<words::WordSpan>)> = Vec::new();
            for group in &bands {
                let spans_by_line = words::split_band_with(group, &comps, &word_p);
                for (tl, spans) in group.iter().zip(spans_by_line) {
                    rebuilt.push((tl.clone(), spans));
                }
            }

            for dline in &decoded {
                for dword in &dline.words {
                    let wx0 = dword.rect.x;
                    let wx1 = dword.rect.x + dword.rect.width;
                    let wy_mid = dword.rect.y + dword.rect.height / 2;
                    let Some((tl, span)) = rebuilt.iter().find(|(tl, _)| wy_mid >= tl.y0 && wy_mid < tl.y1).and_then(
                        |(tl, spans)| {
                            spans
                                .iter()
                                .filter(|sp| sp.x1 > wx0 && sp.x0 < wx1)
                                .max_by_key(|sp| overlap_len(sp.x0, sp.x1, wx0, wx1))
                                .map(|sp| (tl.clone(), sp.clone()))
                        },
                    ) else {
                        no_edge_match += u64::try_from(dword.chars.len()).unwrap_or(0);
                        continue;
                    };
                    let lat = segment::build_with(&span, &comps, &labels, mw, &tl, &seg_p);
                    let _ = mh;

                    for cbox in &dword.chars {
                        total_chars += 1;
                        let want = (cbox.rect.x, cbox.rect.y, cbox.rect.width, cbox.rect.height);
                        let edge = lat.edges.iter().find(|e| {
                            segment::crop(&lat, &labels, mw, e)
                                .map(|g| (g.x, g.y, g.width, g.height) == want)
                                .unwrap_or(false)
                        });
                        let Some(edge) = edge else {
                            no_edge_match += 1;
                            continue;
                        };
                        let is_ms = edge.kind != EdgeKind::Single;
                        if is_ms {
                            merge_or_split += 1;
                            sum_conf_ms += f64::from(cbox.confidence);
                            n_ms += 1;
                        } else {
                            sum_conf_single += f64::from(cbox.confidence);
                            n_single += 1;
                        }
                        if cbox.confidence > 0.8 {
                            high_conf += 1;
                            if is_ms {
                                high_conf_merge_or_split += 1;
                            }
                        }
                    }
                }
            }
        }

        println!(
            "{{\n  \"arm\": \"{arm_name}\",\n  \"total_chars\": {total_chars},\n  \
             \"no_edge_match\": {no_edge_match},\n  \"merge_or_split\": {merge_or_split},\n  \
             \"merge_or_split_frac\": {:.4},\n  \"high_conf\": {high_conf},\n  \
             \"high_conf_merge_or_split\": {high_conf_merge_or_split},\n  \
             \"high_conf_merge_or_split_frac_of_high_conf\": {:.4},\n  \
             \"mean_conf_single\": {:.4},\n  \"mean_conf_merge_or_split\": {:.4}\n}}",
            merge_or_split as f64 / total_chars.max(1) as f64,
            high_conf_merge_or_split as f64 / high_conf.max(1) as f64,
            sum_conf_single / n_single.max(1) as f64,
            sum_conf_ms / n_ms.max(1) as f64,
        );
    }

    ExitCode::SUCCESS
}

fn overlap_len(a0: u32, a1: u32, b0: u32, b1: u32) -> u32 {
    let lo = a0.max(b0);
    let hi = a1.min(b1);
    hi.saturating_sub(lo)
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("segkind-probe: {msg}");
    ExitCode::FAILURE
}
