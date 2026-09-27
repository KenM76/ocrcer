//! `w1-gap-probe`: Fix W1's step-1 measurement (`ARCHITECTURE.md` section 11,
//! "Fix W1 (over-split short cells) pre-registered"). Purely geometric, no
//! model or ground truth needed -- the same style as L1's own measurement:
//! reconstructs `words::band_space_rules`/`split_band_with`'s pre-fix output
//! (`Params::DEFAULT`, `words.short_split_x_heights` still 0) across every
//! page, and for every `ThresholdSource::Valley` split that leaves a sub-word
//! of three members or fewer on either side, records that split's own gap as
//! a fraction of its fragment's x-height.
//!
//! Does not read or score against `finfilings`, `finfilings-val`,
//! `pages-cov`, or any fixture.
//!
//! # Usage
//!
//! ```text
//! w1-gap-probe <pages-dir> [n-pages]
//! ```

use ocrcer_build::page;
use ocrcer_core::image::{binarize, components, deskew};
use ocrcer_core::layout::words::ThresholdSource;
use ocrcer_core::layout::{lines, words};
use ocrcer_core::params::Params;
use ocrcer_core::Gray;

use std::path::PathBuf;
use std::process::ExitCode;

const SHORT_MEMBERS: usize = 3;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: w1-gap-probe <pages-dir> [n-pages]");
        return ExitCode::FAILURE;
    }
    let pages_dir = PathBuf::from(&args[0]);
    let n_pages: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);

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
    eprintln!("w1-gap-probe: {} pages", stems.len());

    let p = Params::DEFAULT;
    let line_p = p.lines();
    let word_p = p.words();
    if word_p.short_split_x_heights != 0.0 {
        eprintln!("w1-gap-probe: expected the rule disabled (0.0) to measure pre-fix behaviour");
        return ExitCode::FAILURE;
    }

    let mut short_ratios: Vec<f64> = Vec::new();
    let mut other_ratios: Vec<f64> = Vec::new();
    let mut fragments_seen = 0u64;
    let mut valley_fragments = 0u64;

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
        let mask0 = binarize::binarize_with(&Gray { width, height, data: &data }, &p.binarize());
        let slope = deskew::estimate_with(&mask0, width, height, f64::from(p.deskew.max_slope));
        let page_deskewed = deskew::correct_with(
            &Gray { width, height, data: &data },
            slope,
            f64::from(p.deskew.min_corrected_slope),
        );
        let gray2 = page_deskewed.gray();
        let mask = binarize::binarize_with(&gray2, &p.binarize());
        let mw = page_deskewed.width;
        let mh = page_deskewed.height;
        let (_labels, comps) = {
            let (labels, count) =
                components::label(&mask, mw, mh, components::Connectivity::Eight);
            let comps = components::components(&labels, mw, mh, count);
            (labels, comps)
        };
        if comps.is_empty() {
            continue;
        }

        let bands = lines::group_with_bands(&comps, mw, mh, &line_p);
        for group in &bands {
            let rules = words::band_space_rules(group, &comps, &word_p);
            let spans_by_fragment = words::split_band_with(group, &comps, &word_p);
            for ((fragment, rule), spans) in group.iter().zip(&rules).zip(&spans_by_fragment) {
                fragments_seen += 1;
                if rule.source != ThresholdSource::Valley || spans.len() < 2 {
                    continue;
                }
                valley_fragments += 1;
                if fragment.x_height <= 0.0 {
                    continue;
                }
                // "Compact run" (ARCHITECTURE.md section 11, Fix W1): the
                // whole fragment resolves to short sub-words, not merely one
                // side of one cut -- otherwise an ordinary sentence
                // containing one incidental short word ("of", "is") would be
                // counted, and its gap is a genuine space, not an over-split.
                let compact_run = spans.iter().all(|s| s.members.len() <= SHORT_MEMBERS);
                let g = words::gaps(fragment, &comps);
                let mut pos_end = 0usize;
                for wi in 0..spans.len() {
                    pos_end += spans[wi].members.len();
                    if wi + 1 == spans.len() {
                        break;
                    }
                    let gap_idx = pos_end - 1;
                    let gap_px = g[gap_idx];
                    let left_len = spans[wi].members.len();
                    let right_len = spans[wi + 1].members.len();
                    let ratio = f64::from(gap_px) / f64::from(fragment.x_height);
                    if compact_run {
                        short_ratios.push(ratio);
                    } else if left_len > SHORT_MEMBERS && right_len > SHORT_MEMBERS {
                        other_ratios.push(ratio);
                    }
                }
            }
        }
    }

    short_ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
    other_ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());

    println!("fragments_seen: {fragments_seen}");
    println!("valley_fragments: {valley_fragments}");
    println!("short_split samples: {}", short_ratios.len());
    print_histogram("short_split", &short_ratios);
    println!("other (both sides > {SHORT_MEMBERS} members) samples: {}", other_ratios.len());
    print_histogram("other", &other_ratios);

    ExitCode::SUCCESS
}

fn print_histogram(label: &str, v: &[f64]) {
    if v.is_empty() {
        println!("  {label}: no samples");
        return;
    }
    println!("  {label} min={:.4} max={:.4} n={}", v[0], v[v.len() - 1], v.len());
    // Fixed-width buckets of 0.05 x-heights from 0.0 to 2.0, plus an overflow
    // bucket, so a gap in the distribution shows up as an empty row.
    let mut buckets = [0u32; 41];
    for &x in v {
        let b = ((x / 0.05).floor() as isize).clamp(0, 40) as usize;
        buckets[b] += 1;
    }
    for (i, &c) in buckets.iter().enumerate() {
        if c == 0 {
            continue;
        }
        let lo = i as f64 * 0.05;
        let hi = lo + 0.05;
        println!("    [{lo:.2},{hi:.2}) {c}");
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("w1-gap-probe: {msg}");
    ExitCode::FAILURE
}
