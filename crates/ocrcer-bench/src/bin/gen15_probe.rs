//! `gen15-probe`: chunk 15 diagnosis only (`docs/ARCHITECTURE.md` §11,
//! "Chunk 15 mode 2 fails" -- the one diagnostic that entry pre-registers
//! before the net continues or is paused).
//!
//! Per-crop top-1 accuracy of the network and of the prototype matcher, on
//! real character crops aligned to ground truth, built through the runtime
//! crop path (`segment::crop` + `Glyph::input` -- the same construction
//! `pipeline.rs::read_word` serves and `nn15_dump.rs`'s rule-4 fix
//! (`c15-dumpfix`, merged) now uses for its real-positive rows). This binary
//! writes no dump; it reads a page's own ground truth directly and computes
//! both classifiers' top-1 on the identical extracted `G`/`X` per crop, so
//! the two accuracies are never computed on different inputs.
//!
//! # Alignment (identical gate to `nn15_dump.rs`'s (b) real-positive path)
//!
//! A truth line pairs with a decoded line by index only when their word
//! counts match. A truth word pairs with a decoded word only when their
//! character counts match. Every qualifying character's crop is rebuilt via
//! `segment::build_with`/`segment::crop`, verified against the decoded
//! `CharBox.rect` by exact box match -- never a second crop implementation,
//! never a guess at alignment beyond that verified match.
//!
//! One decode per page, at the shipped default (`match.classifier=0`, the
//! matcher) -- `match.classifier` does not gate which characters qualify
//! here (that is `recognize_lines`'s own segmentation, which per
//! `2026-09-26_c15_mode2.md` is largely classifier-invariant for `d_seg`),
//! only which class each crop is *labelled* by, which this binary reads
//! independently for both classifiers off the same crop.
//!
//! Never touches `finfilings`, `finfilings-val`, `pages-cov`, any fixture, or
//! `bench/ident`. No parameter, weight or default is written or changed.
//!
//! # Usage
//!
//! ```text
//! gen15-probe <label> <model.ocrw> <pages-dir> <split-file-or-dash> <stride> <max-pages>
//! ```
//!
//! `split-file-or-dash`: a fold-split TSV (`bench/splits/nn15_page_split.tsv`)
//! to restrict `pages-dir` to only the stems it lists (the "trainfold" run),
//! or `-` to use every stem in `pages-dir` (the "unseen" run, since that
//! directory already excludes split stems by construction).
//! `stride`/`max-pages`: `skip(0).step_by(stride).take(max_pages)` over the
//! sorted stem list, same convention as `ocr.rs`'s `select_pages`.

use ocrcer_bench::pages;
use ocrcer_build::{ocrw::json_string, page};
use ocrcer_core::feature::extract_with_grid;
use ocrcer_core::image::{binarize, components, deskew};
use ocrcer_core::layout::{lines, segment, underline, words};
use ocrcer_core::ocrw::Model;
use ocrcer_core::{r#match, Engine, Gray};

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 6 {
        eprintln!(
            "usage: gen15-probe <label> <model.ocrw> <pages-dir> <split-file-or-dash> <stride> <max-pages>"
        );
        return ExitCode::FAILURE;
    }
    let label = &args[0];
    let model_path = PathBuf::from(&args[1]);
    let pages_dir = args[2].clone();
    let split_arg = &args[3];
    let stride: usize = args[4].parse().unwrap_or(1).max(1);
    let max_pages: usize = args[5].parse().unwrap_or(usize::MAX);

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

    let mut class_of_char: BTreeMap<char, u16> = BTreeMap::new();
    let mut char_of_class: BTreeMap<u16, char> = BTreeMap::new();
    for c in &model.classes {
        class_of_char.insert(c.codepoint, c.index);
        char_of_class.insert(c.index, c.codepoint);
    }

    let all_pgms = match pages::list_pages(&pages_dir) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let kept: Vec<PathBuf> = if split_arg == "-" {
        all_pgms
    } else {
        let split_stems = match load_split_stems(split_arg) {
            Ok(s) => s,
            Err(e) => return fail(&e),
        };
        all_pgms
            .into_iter()
            .filter(|p| {
                p.file_stem()
                    .map(|s| split_stems.contains(&s.to_string_lossy().into_owned()))
                    .unwrap_or(false)
            })
            .collect()
    };
    let chosen: Vec<PathBuf> = kept.into_iter().step_by(stride).take(max_pages).collect();
    eprintln!("gen15-probe [{label}]: {} pages chosen from {pages_dir}", chosen.len());

    let p = engine.params();
    let mut s = Stats::default();

    for pgm_path in &chosen {
        let stem = pgm_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let truth_path = pgm_path.with_file_name(format!("{stem}.truth.json"));
        let pgm_bytes = match std::fs::read(pgm_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("gen15-probe: skipping {stem}: {e}");
                continue;
            }
        };
        let (width, height, data) = match page::from_pgm(&pgm_bytes) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("gen15-probe: {stem}: bad pgm: {e}");
                continue;
            }
        };
        let truth_bytes = match std::fs::read(&truth_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("gen15-probe: {stem}: {e}");
                continue;
            }
        };
        let truth: serde_json::Value = match serde_json::from_slice(&truth_bytes) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("gen15-probe: {stem}: bad truth json: {e}");
                continue;
            }
        };
        let Some(truth_lines) = truth.get("lines").and_then(|v| v.as_array()) else {
            continue;
        };

        let gray = Gray { width, height, data: &data };
        let decoded = match engine.recognize_lines(gray) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("gen15-probe: {stem}: recognize_lines failed: {e}");
                continue;
            }
        };
        s.pages_read += 1;

        // Same reconstruction `nn15_dump.rs` and `skew_probe.rs` use --
        // public calls only, same order, never a second segmentation
        // implementation.
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

        s.lines_total += decoded.len().min(truth_lines.len()) as u64;
        for (dline, tline) in decoded.iter().zip(truth_lines.iter()) {
            let Some(truth_text) = tline.as_str() else { continue };
            let truth_tokens: Vec<&str> = truth_text.split_whitespace().collect();
            if dline.words.len() != truth_tokens.len() {
                continue;
            }
            s.lines_qualifying += 1;

            for (dword, ttoken) in dline.words.iter().zip(truth_tokens.iter()) {
                s.words_total += 1;
                let truth_chars: Vec<char> = ttoken.chars().collect();
                if dword.chars.len() != truth_chars.len() {
                    continue;
                }
                s.words_qualifying += 1;

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
                    s.no_word_span += truth_chars.len() as u64;
                    continue;
                };
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
                    let Some(glyph) = found else {
                        s.no_edge_match += 1;
                        continue;
                    };
                    let Some(&class_idx) = class_of_char.get(&tch) else {
                        s.out_of_charset += 1;
                        continue;
                    };

                    let input = glyph.input(&tl);
                    let (raw, grid) = extract_with_grid(&input);
                    let xv = model.standardise(&raw);

                    s.scored += 1;
                    let mut net_class: Option<u16> = None;
                    let mut net_p: f32 = 0.0;
                    if let Ok(lp) = net.forward(&grid, &xv) {
                        let (c, prob) = top1_excluding_junk(&lp, net.junk_index as usize);
                        net_class = Some(c);
                        net_p = prob;
                    } else {
                        s.net_forward_err += 1;
                    }
                    let mtch = r#match::nearest(&model, &raw, 1, true);
                    let matcher_class = mtch.as_ref().and_then(|m| m.top()).map(|c| c.class);
                    if matcher_class.is_none() {
                        s.matcher_none += 1;
                    }

                    let net_ok = net_class == Some(class_idx);
                    let matcher_ok = matcher_class == Some(class_idx);
                    if net_class.is_some() {
                        if net_ok {
                            s.net_correct += 1;
                        } else {
                            s.net_wrong += 1;
                            if net_p >= 0.9 {
                                s.net_wrong_high_conf += 1;
                            }
                            if let Some(nc) = net_class {
                                *s.net_confusions
                                    .entry((tch, char_of_class.get(&nc).copied().unwrap_or('?')))
                                    .or_insert(0) += 1;
                            }
                        }
                    }
                    if matcher_class.is_some() && matcher_ok {
                        s.matcher_correct += 1;
                    }

                    let cls = class_group(tch);
                    let e = s.per_class.entry(tch).or_insert((0, 0, 0));
                    e.0 += 1;
                    if net_ok {
                        e.1 += 1;
                    }
                    if matcher_ok {
                        e.2 += 1;
                    }
                    let g = s.per_group.entry(cls).or_insert((0, 0, 0));
                    g.0 += 1;
                    if net_ok {
                        g.1 += 1;
                    }
                    if matcher_ok {
                        g.2 += 1;
                    }
                }
            }
        }
    }

    print_report(label, &s);
    ExitCode::SUCCESS
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    Digit,
    Upper,
    Other,
}

fn class_group(ch: char) -> Group {
    if ch.is_ascii_digit() {
        Group::Digit
    } else if ch.is_ascii_uppercase() {
        Group::Upper
    } else {
        Group::Other
    }
}

#[derive(Default)]
struct Stats {
    pages_read: u64,
    lines_total: u64,
    lines_qualifying: u64,
    words_total: u64,
    words_qualifying: u64,
    no_word_span: u64,
    no_edge_match: u64,
    out_of_charset: u64,
    scored: u64,
    net_forward_err: u64,
    matcher_none: u64,
    net_correct: u64,
    net_wrong: u64,
    net_wrong_high_conf: u64,
    matcher_correct: u64,
    per_class: BTreeMap<char, (u64, u64, u64)>,
    per_group: BTreeMap<Group, (u64, u64, u64)>,
    net_confusions: BTreeMap<(char, char), u64>,
}

fn print_report(label: &str, s: &Stats) {
    let denom = s.scored.max(1) as f64;
    println!("{{");
    println!("  \"label\": {},", json_string(label));
    println!("  \"pages_read\": {},", s.pages_read);
    println!("  \"lines_total\": {},", s.lines_total);
    println!("  \"lines_qualifying\": {},", s.lines_qualifying);
    println!("  \"words_total\": {},", s.words_total);
    println!("  \"words_qualifying\": {},", s.words_qualifying);
    println!("  \"no_word_span\": {},", s.no_word_span);
    println!("  \"no_edge_match\": {},", s.no_edge_match);
    println!("  \"out_of_charset\": {},", s.out_of_charset);
    println!("  \"scored\": {},", s.scored);
    println!("  \"net_forward_err\": {},", s.net_forward_err);
    println!("  \"matcher_none\": {},", s.matcher_none);
    println!("  \"net_correct\": {},", s.net_correct);
    println!("  \"net_accuracy\": {:.4},", s.net_correct as f64 / denom);
    println!("  \"matcher_correct\": {},", s.matcher_correct);
    println!("  \"matcher_accuracy\": {:.4},", s.matcher_correct as f64 / denom);
    println!("  \"net_errors_total\": {},", s.net_wrong);
    println!("  \"net_errors_high_conf_ge_0.9\": {},", s.net_wrong_high_conf);
    println!(
        "  \"net_errors_high_conf_frac\": {:.4},",
        s.net_wrong_high_conf as f64 / (s.net_wrong.max(1) as f64)
    );

    println!("  \"per_class\": {{");
    let mut first = true;
    for (ch, (n, net_c, matcher_c)) in &s.per_class {
        if !matches!(class_group(*ch), Group::Digit | Group::Upper) {
            continue;
        }
        if !first {
            println!(",");
        }
        first = false;
        print!(
            "    {}: {{\"n\": {n}, \"net_acc\": {:.4}, \"matcher_acc\": {:.4}}}",
            json_string(&ch.to_string()),
            *net_c as f64 / (*n).max(1) as f64,
            *matcher_c as f64 / (*n).max(1) as f64
        );
    }
    println!();
    println!("  }},");

    println!("  \"per_group\": {{");
    let mut gfirst = true;
    for (g, (n, net_c, matcher_c)) in &s.per_group {
        if !gfirst {
            println!(",");
        }
        gfirst = false;
        let name = match g {
            Group::Digit => "digit",
            Group::Upper => "upper",
            Group::Other => "other",
        };
        print!(
            "    \"{name}\": {{\"n\": {n}, \"net_acc\": {:.4}, \"matcher_acc\": {:.4}}}",
            *net_c as f64 / (*n).max(1) as f64,
            *matcher_c as f64 / (*n).max(1) as f64
        );
    }
    println!();
    println!("  }},");

    let mut confusions: Vec<(&(char, char), &u64)> = s.net_confusions.iter().collect();
    confusions.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("  \"net_top_confusions\": [");
    for (i, ((from, to), count)) in confusions.iter().take(20).enumerate() {
        if i > 0 {
            println!(",");
        }
        print!(
            "    {{\"from\": {}, \"to\": {}, \"count\": {count}}}",
            json_string(&from.to_string()),
            json_string(&to.to_string())
        );
    }
    println!();
    println!("  ]");
    println!("}}");
}

/// `(class, softmax probability)` of the highest-scoring non-junk class,
/// ties broken low-index -- mirrors `pipeline.rs::nn_candidates`.
fn top1_excluding_junk(log_probs: &[f32], junk_index: usize) -> (u16, f32) {
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
    (best_i as u16, best_v.exp())
}

fn overlap_len(a0: u32, a1: u32, b0: u32, b1: u32) -> u32 {
    let lo = a0.max(b0);
    let hi = a1.min(b1);
    hi.saturating_sub(lo)
}

fn load_split_stems(path: &str) -> Result<std::collections::BTreeSet<String>, String> {
    use std::io::BufRead;
    let f = std::fs::File::open(path).map_err(|e| format!("reading {path}: {e}"))?;
    let mut out = std::collections::BTreeSet::new();
    for line in std::io::BufReader::new(f).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(stem) = line.split('\t').next() {
            out.insert(stem.to_string());
        }
    }
    Ok(out)
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("gen15-probe: {msg}");
    ExitCode::FAILURE
}
