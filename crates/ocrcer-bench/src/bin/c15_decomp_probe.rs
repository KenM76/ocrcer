//! `c15-decomp-probe`: chunk 15 loss decomposition (`docs/ARCHITECTURE.md`
//! §11, "Chunk 15 trace" -- the five-item decomposition that entry
//! pre-registers before the margin explanation is revisited).
//!
//! Diagnosis only. No parameter, weight, or default is written or changed;
//! `match.classifier`/`nn.scale` are set only via `ocrcer_bench::knobs::set`
//! at probe run time, on an in-memory `Engine`, never on `model/params.tsv`.
//! Reuses `ocrcer_bench::cer::{score, line_matched_score, align,
//! greedy_line_pairs}` -- the same alignment `ocr`'s CER and confusion table
//! use (`CLAUDE.md` rule 4) -- rather than a second implementation.
//!
//! Never touches `finfilings`, `finfilings-val`, `pages-cov`, any fixture,
//! or `bench/ident`.
//!
//! # Usage
//!
//! ```text
//! c15-decomp-probe <model.ocrw> <pages-dir> [--stride N] [--offset N]
//!     [--limit N] [--nn-scale F] [--json PATH] [--dump-dir PATH]
//! ```
//!
//! `--dump-dir`, if given, writes one `<stem>.json` per page under it with
//! the raw reference/read0/read1 text -- for the task's own item 5
//! (categorising the ten worst-delta pages by hand against the page image).
//! That directory is scratch: never under the repo, never committed, and
//! this probe does not write inside `docs/` or `bench/`.

use ocrcer_bench::cer::{align, greedy_line_pairs, line_matched_score, normalise, score, Edit};
use ocrcer_bench::pages::{list_pages, load_page, load_truth_beside, page_text};
use ocrcer_core::pipeline::Engine;
use ocrcer_core::Gray;

use std::collections::BTreeMap;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut model = String::new();
    let mut dir = String::new();
    let mut stride = 1usize;
    let mut offset = 0usize;
    let mut limit = usize::MAX;
    let mut nn_scale = std::f32::consts::SQRT_2;
    let mut json: Option<String> = None;
    let mut dump_dir: Option<String> = None;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--stride" => {
                i += 1;
                stride = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(stride);
            }
            "--offset" => {
                i += 1;
                offset = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(offset);
            }
            "--limit" => {
                i += 1;
                limit = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(limit);
            }
            "--nn-scale" => {
                i += 1;
                nn_scale = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(nn_scale);
            }
            "--json" => {
                i += 1;
                json = args.get(i).cloned();
            }
            "--dump-dir" => {
                i += 1;
                dump_dir = args.get(i).cloned();
            }
            other if model.is_empty() => model = other.to_string(),
            other if dir.is_empty() => dir = other.to_string(),
            other => {
                eprintln!("c15-decomp-probe: unexpected argument {other:?}");
                return ExitCode::FAILURE;
            }
        }
        i += 1;
    }
    if model.is_empty() || dir.is_empty() {
        eprintln!("usage: c15-decomp-probe <model.ocrw> <pages-dir> [--stride N] [--offset N] [--limit N] [--nn-scale F] [--json PATH] [--dump-dir PATH]");
        return ExitCode::FAILURE;
    }
    if let Some(d) = &dump_dir {
        if let Err(e) = std::fs::create_dir_all(d) {
            eprintln!("c15-decomp-probe: {d}: {e}");
            return ExitCode::FAILURE;
        }
    }

    match run(&model, &dir, stride, offset, limit, nn_scale, json.as_deref(), dump_dir.as_deref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("c15-decomp-probe: {e}");
            ExitCode::FAILURE
        }
    }
}

/// One page's measurements under both modes. Every field is either a direct
/// count from `cer::score`/`cer::line_matched_score`/`cer::align`, or a sum
/// over the two edit paths those produce -- nothing here is estimated.
#[derive(Default, Clone)]
struct PageRow {
    stem: String,
    chars: usize,
    e2e_errors: [usize; 2],
    lm_chars: usize,
    lm_errors: [usize; 2],
    // From `cer::align` on the whole normalised page: substitutions,
    // insertions, deletions, matches.
    sub: [usize; 2],
    ins: [usize; 2],
    del: [usize; 2],
    mat: [usize; 2],
    // From the per-truth-line paired alignment (task 4): matches and
    // substitutions among characters that got a same-line counterpart at
    // all -- insertions/deletions inside a paired line, and any line that
    // paired with nothing, are excluded from this specific rate by
    // definition (see the probe's doc comment and the report's task 4).
    line_mat: [usize; 2],
    line_sub: [usize; 2],
    // Lines paired at all (denominator context, not part of the rate).
    lines_total: usize,
    lines_paired: [usize; 2],
    align_skipped: [bool; 2],
}

fn run(
    model: &str,
    dir: &str,
    stride: usize,
    offset: usize,
    limit: usize,
    nn_scale: f32,
    json: Option<&str>,
    dump_dir: Option<&str>,
) -> Result<(), String> {
    let bytes = std::fs::read(model).map_err(|e| format!("{model}: {e}"))?;
    let engine0 = Engine::from_bytes(&bytes).map_err(|e| format!("{model}: {e:?}"))?;
    let mut engine1 = Engine::from_bytes(&bytes).map_err(|e| format!("{model}: {e:?}"))?;
    if !ocrcer_bench::knobs::set(&mut engine1, "match.classifier", 1.0) {
        return Err("match.classifier: not a knob this engine takes".into());
    }
    if !ocrcer_bench::knobs::set(&mut engine1, "nn.scale", nn_scale) {
        return Err("nn.scale: not a knob this engine takes".into());
    }
    println!("mode0   match.classifier=0 (defaults)");
    println!("mode1   match.classifier=1, nn.scale={nn_scale}");

    let pgms = list_pages(dir)?;
    let chosen: Vec<_> = pgms.iter().skip(offset).step_by(stride).take(limit).cloned().collect();
    println!("pages   {} of {} in {dir} (stride {stride}, offset {offset})", chosen.len(), pgms.len());

    let mut rows = Vec::with_capacity(chosen.len());
    // Global edit-pair tallies (task 2/3), from the same whole-page alignment
    // `PageRow::sub/ins/del` sum from -- one pass, both uses.
    let mut edit_counts: [BTreeMap<(String, String), u64>; 2] = [BTreeMap::new(), BTreeMap::new()];

    for pgm in &chosen {
        let stem = pgm.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let truth = load_truth_beside(pgm)?;
        let (w, h, grey) = load_page(pgm)?;
        let reference = truth.lines.join("\n");

        let read0 = page_text(
            &engine0
                .recognize_lines(Gray { width: w, height: h, data: &grey })
                .map_err(|e| format!("{stem}: {e:?}"))?,
        );
        let read1 = page_text(
            &engine1
                .recognize_lines(Gray { width: w, height: h, data: &grey })
                .map_err(|e| format!("{stem}: {e:?}"))?,
        );

        if let Some(d) = dump_dir {
            let path = std::path::Path::new(d).join(format!("{stem}.json"));
            let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
            let body = format!(
                "{{\"reference\":\"{}\",\"read0\":\"{}\",\"read1\":\"{}\"}}",
                esc(&reference),
                esc(&read0),
                esc(&read1)
            );
            std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
        }

        let s0 = score(&reference, &read0);
        let s1 = score(&reference, &read1);
        let lm0 = line_matched_score(&reference, &read0);
        let lm1 = line_matched_score(&reference, &read1);

        let mut row = PageRow {
            stem: stem.clone(),
            chars: s0.chars,
            e2e_errors: [s0.char_errors, s1.char_errors],
            lm_chars: lm0.chars,
            lm_errors: [lm0.char_errors, lm1.char_errors],
            ..Default::default()
        };
        debug_assert_eq!(s0.chars, s1.chars, "same reference must give the same denominator");
        debug_assert_eq!(lm0.chars, lm1.chars, "same reference must give the same denominator");

        let a: Vec<char> = normalise(&reference).chars().collect();
        for (m, read) in [read0.as_str(), read1.as_str()].into_iter().enumerate() {
            let b: Vec<char> = normalise(read).chars().collect();
            match align(&a, &b) {
                Some(path) => {
                    for e in path {
                        match e {
                            Edit::Match => row.mat[m] += 1,
                            Edit::Sub(x, y) => {
                                row.sub[m] += 1;
                                *edit_counts[m].entry((x.to_string(), y.to_string())).or_default() += 1;
                            }
                            Edit::Del(x) => {
                                row.del[m] += 1;
                                *edit_counts[m].entry((x.to_string(), String::new())).or_default() += 1;
                            }
                            Edit::Ins(y) => {
                                row.ins[m] += 1;
                                *edit_counts[m].entry((String::new(), y.to_string())).or_default() += 1;
                            }
                        }
                    }
                }
                None => row.align_skipped[m] = true,
            }
        }

        // Task 4: per-truth-line paired alignment. `t`/`o0`/`o1` are the
        // same per-line splitting `line_matched_score` uses internally
        // (normalise, then split on '\n').
        let rn = normalise(&reference);
        let t: Vec<Vec<char>> = rn.lines().map(|l| l.chars().collect()).collect();
        row.lines_total = t.len();
        for (m, read) in [read0.as_str(), read1.as_str()].into_iter().enumerate() {
            let on = normalise(read);
            let o: Vec<Vec<char>> = on.lines().map(|l| l.chars().collect()).collect();
            let (match_of_t, _used_o) = greedy_line_pairs(&t, &o);
            let mut paired = 0usize;
            for (ti, mj) in match_of_t.iter().enumerate() {
                if let Some(j) = mj {
                    paired += 1;
                    if let Some(path) = align(&t[ti], &o[*j]) {
                        for e in path {
                            match e {
                                Edit::Match => row.line_mat[m] += 1,
                                Edit::Sub(_, _) => row.line_sub[m] += 1,
                                Edit::Del(_) | Edit::Ins(_) => {}
                            }
                        }
                    }
                }
            }
            row.lines_paired[m] = paired;
        }

        rows.push(row);
    }

    report(&rows, &edit_counts, json)
}

fn cer_of(errors: usize, chars: usize) -> Option<f64> {
    (chars > 0).then(|| errors as f64 / chars as f64)
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = q * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let frac = pos - lo as f64;
        sorted[lo] * (1.0 - frac) + sorted[hi] * frac
    }
}

fn report(
    rows: &[PageRow],
    edit_counts: &[BTreeMap<(String, String), u64>; 2],
    json: Option<&str>,
) -> Result<(), String> {
    let n = rows.len();
    println!("\n== task 1: per-page CER delta (mode1 - mode0) ==");

    let mut deltas: Vec<f64> = Vec::with_capacity(n);
    let mut per_page: Vec<(f64, String)> = Vec::with_capacity(n);
    for r in rows {
        let c0 = cer_of(r.e2e_errors[0], r.chars).unwrap_or(0.0);
        let c1 = cer_of(r.e2e_errors[1], r.chars).unwrap_or(0.0);
        let d = c1 - c0;
        deltas.push(d);
        per_page.push((d, r.stem.clone()));
    }
    let mut sorted = deltas.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = deltas.iter().sum::<f64>() / n.max(1) as f64;
    println!(
        "quantiles (mode1-mode0 CER, pp): min {:.3} p10 {:.3} p25 {:.3} median {:.3} p75 {:.3} p90 {:.3} max {:.3} mean {:.3}",
        sorted.first().copied().unwrap_or(0.0) * 100.0,
        quantile(&sorted, 0.10) * 100.0,
        quantile(&sorted, 0.25) * 100.0,
        quantile(&sorted, 0.50) * 100.0,
        quantile(&sorted, 0.75) * 100.0,
        quantile(&sorted, 0.90) * 100.0,
        sorted.last().copied().unwrap_or(0.0) * 100.0,
        mean * 100.0,
    );

    let improved = deltas.iter().filter(|d| **d < 0.0).count();
    let worsened = deltas.iter().filter(|d| **d > 0.0).count();
    let flat = n - improved - worsened;
    println!("pages worse under mode1: {worsened}   improved: {improved}   unchanged: {flat}   (of {n})");

    let mut desc = per_page.clone();
    desc.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let total_pos: f64 = desc.iter().map(|(d, _)| d.max(0.0)).sum();
    let mut cum = 0.0;
    let mut n50 = 0usize;
    let mut n80 = 0usize;
    for (i, (d, _)) in desc.iter().enumerate() {
        if *d <= 0.0 {
            break;
        }
        cum += d;
        if n50 == 0 && cum >= 0.5 * total_pos {
            n50 = i + 1;
        }
        if n80 == 0 && cum >= 0.8 * total_pos {
            n80 = i + 1;
        }
    }
    println!(
        "total positive delta (sum of mode1-worse pages, pp): {:.3}; {} pages carry 50% of it, {} pages carry 80%",
        total_pos * 100.0,
        n50,
        n80
    );
    println!("top 10 worst-delta pages:");
    for (d, stem) in desc.iter().take(10) {
        println!("  {:+.3} pp  {stem}", d * 100.0);
    }

    println!("\n== task 2: edit totals (whole-page alignment) ==");
    let (mut s0, mut i0, mut d0, mut m0) = (0usize, 0usize, 0usize, 0usize);
    let (mut s1, mut i1, mut d1, mut m1) = (0usize, 0usize, 0usize, 0usize);
    let mut skipped = [0usize; 2];
    for r in rows {
        s0 += r.sub[0];
        i0 += r.ins[0];
        d0 += r.del[0];
        m0 += r.mat[0];
        s1 += r.sub[1];
        i1 += r.ins[1];
        d1 += r.del[1];
        m1 += r.mat[1];
        skipped[0] += usize::from(r.align_skipped[0]);
        skipped[1] += usize::from(r.align_skipped[1]);
    }
    println!("mode0 (matcher)  sub {s0}  ins {i0}  del {d0}  match {m0}");
    println!("mode1 (net)      sub {s1}  ins {i1}  del {d1}  match {m1}");
    println!("delta (1-0)      sub {:+} ins {:+} del {:+}", s1 as i64 - s0 as i64, i1 as i64 - i0 as i64, d1 as i64 - d0 as i64);
    if skipped[0] + skipped[1] > 0 {
        println!("align skipped (page too large for full matrix): mode0={} mode1={}", skipped[0], skipped[1]);
    }

    println!("\n== task 4: ground-truth-line-paired character correctness ==");
    let (mut lm0, mut ls0, mut lm1, mut ls1) = (0usize, 0usize, 0usize, 0usize);
    let (mut lines_total, mut paired0, mut paired1) = (0usize, 0usize, 0usize);
    for r in rows {
        lm0 += r.line_mat[0];
        ls0 += r.line_sub[0];
        lm1 += r.line_mat[1];
        ls1 += r.line_sub[1];
        lines_total += r.lines_total;
        paired0 += r.lines_paired[0];
        paired1 += r.lines_paired[1];
    }
    let rate0 = lm0 as f64 / (lm0 + ls0).max(1) as f64;
    let rate1 = lm1 as f64 / (lm1 + ls1).max(1) as f64;
    println!(
        "mode0 (matcher)  correct {:.4}%  ({} match / {} sub, over {} paired lines of {})",
        rate0 * 100.0,
        lm0,
        ls0,
        paired0,
        lines_total
    );
    println!(
        "mode1 (net)      correct {:.4}%  ({} match / {} sub, over {} paired lines of {})",
        rate1 * 100.0,
        lm1,
        ls1,
        paired1,
        lines_total
    );

    println!("\n== task 3: top 40 edit rows by count difference (mode1 - mode0) ==");
    let mut keys: std::collections::BTreeSet<(String, String)> = std::collections::BTreeSet::new();
    keys.extend(edit_counts[0].keys().cloned());
    keys.extend(edit_counts[1].keys().cloned());
    let mut rows_diff: Vec<(i64, u64, u64, (String, String))> = keys
        .into_iter()
        .map(|k| {
            let c0 = *edit_counts[0].get(&k).unwrap_or(&0);
            let c1 = *edit_counts[1].get(&k).unwrap_or(&0);
            (c1 as i64 - c0 as i64, c0, c1, k)
        })
        .collect();
    rows_diff.sort_by(|a, b| b.0.cmp(&a.0).then(a.3.cmp(&b.3)));
    println!("top 40 (mode1 has more):");
    for (diff, c0, c1, (want, got)) in rows_diff.iter().take(40) {
        println!("  diff {diff:+6}  mode0 {c0:>6}  mode1 {c1:>6}  {want:?} -> {got:?}");
    }
    println!("top 20 the other way (mode0 has more):");
    for (diff, c0, c1, (want, got)) in rows_diff.iter().rev().take(20) {
        println!("  diff {diff:+6}  mode0 {c0:>6}  mode1 {c1:>6}  {want:?} -> {got:?}");
    }

    if let Some(path) = json {
        write_json(path, rows, &deltas, &rows_diff)?;
    }
    Ok(())
}

fn write_json(
    path: &str,
    rows: &[PageRow],
    deltas: &[f64],
    rows_diff: &[(i64, u64, u64, (String, String))],
) -> Result<(), String> {
    use std::fmt::Write as _;
    let mut s = String::from("{\"pages\":[");
    for (i, (r, d)) in rows.iter().zip(deltas).enumerate() {
        if i > 0 {
            s.push(',');
        }
        write!(
            s,
            "{{\"stem\":\"{}\",\"chars\":{},\"delta_cer\":{:.8},\"e2e_errors\":[{},{}],\"lm_errors\":[{},{}],\"lm_chars\":{},\"sub\":[{},{}],\"ins\":[{},{}],\"del\":[{},{}],\"mat\":[{},{}],\"line_mat\":[{},{}],\"line_sub\":[{},{}],\"lines_total\":{},\"lines_paired\":[{},{}]}}",
            r.stem, r.chars, d,
            r.e2e_errors[0], r.e2e_errors[1],
            r.lm_errors[0], r.lm_errors[1], r.lm_chars,
            r.sub[0], r.sub[1], r.ins[0], r.ins[1], r.del[0], r.del[1], r.mat[0], r.mat[1],
            r.line_mat[0], r.line_mat[1], r.line_sub[0], r.line_sub[1],
            r.lines_total, r.lines_paired[0], r.lines_paired[1],
        ).ok();
    }
    s.push_str("],\"edit_rows\":[");
    for (i, (diff, c0, c1, (want, got))) in rows_diff.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let esc = |x: &str| x.replace('\\', "\\\\").replace('"', "\\\"");
        write!(s, "{{\"diff\":{diff},\"mode0\":{c0},\"mode1\":{c1},\"want\":\"{}\",\"got\":\"{}\"}}", esc(want), esc(got)).ok();
    }
    s.push_str("]}");
    std::fs::write(path, s).map_err(|e| format!("{path}: {e}"))
}
