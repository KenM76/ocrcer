//! `timing_ab`: paired A/B timing protocol v2 — one process, one model load,
//! configurations alternated page by page, minimum-of-5 repeats per page.
//!
//! `ARCHITECTURE.md`'s 2026-09-27 W1 entry ("W1 fails the paired timing gate
//! as registered; that protocol was unfit; the timing gate must pass an A/A
//! test before it is used again") replaces the single-run wall-time gate and
//! the first paired-ABABAB attempt with this one, and requires an A/A pass
//! before any real candidate is timed. This binary is that protocol.
//!
//! ```text
//! timing_ab <model.ocrw> <pages-dir> \
//!     [--set-a name=value ...] [--set-b name=value ...] \
//!     [--stride N] [--offset N] [--limit N] [--reps N]
//! ```
//!
//! Defaults (`--stride 10 --offset 0 --limit 32`) reproduce the fixed
//! 32-page subset `docs/measurements/2026-09-27_W1.md`'s addendum used: over
//! the full `finfilings-train-unseen` directory (320 pages, deterministic
//! alphabetic order from `pages::list_pages`), stride 10 offset 0 lands on
//! the same 32 pages as taking every 5th page of the 160-page
//! stride-2-offset-0 cohort, because 2*5 = 10.
//!
//! # Protocol
//!
//! One `Engine` is loaded once from the model file. For each page, in order,
//! the loop runs A, B, A, B, ... for `--reps` repeats each (default 5),
//! re-applying the named override(s) via `Engine::set_param` before every
//! single call and timing only the `recognize_lines` call itself with
//! `std::time::Instant`. A page's minimum-of-5 for each side rejects a
//! transient burst (a Dropbox scan, an indexer sweep) that would otherwise
//! inflate exactly one repeat; taking the min is intentional, not a
//! best-case cherry-pick, because the question this protocol answers is
//! "how fast can this configuration go on this machine right now", which a
//! single stall should not be allowed to answer for it.
//!
//! `match.classifier` is forced to `0` after load, before either
//! configuration's overrides are applied and regardless of what `--set-a`/
//! `--set-b` name — this is a timing protocol comparing two parameter
//! points under one decode mode, not a mode sweep. Single-threaded: this
//! binary never touches the (unbuilt, opt-in) `parallel` feature.
//!
//! # What "loads the model once" means for A vs B
//!
//! `--set-a`/`--set-b` may each name a *different* set of parameters. A
//! parameter named in one list but not the other is not left at whatever
//! the previous call happened to set it to: this tool reads every named
//! parameter's value once at load (the file's own value, or `0` for
//! `match.classifier`, forced as above) and uses that as the value for
//! whichever side did not mention it, every time it switches configurations.
//! Skipping this would silently carry A's override into a B run that never
//! asked for it.
//!
//! # Process CPU time
//!
//! Not obtainable from `std` alone (no cross-platform process-CPU-time
//! call), and `ocrcer-bench` carries no dependency that provides one
//! (`Cargo.toml`: `ocrcer-core`, `ocrcer-build`, `ocrs`/`rten` behind
//! `vs-ocrs`, `serde`/`serde_json`) — adding one for a single measurement
//! session is not this tool's call to make. Reported as "not measured".

use std::process::ExitCode;
use std::time::Instant;

use ocrcer_bench::pages::{list_pages, load_page};
use ocrcer_core::pipeline::Engine;
use ocrcer_core::Gray;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut model = String::new();
    let mut dir = String::new();
    let mut stride = 10usize;
    let mut offset = 0usize;
    let mut limit = 32usize;
    let mut reps = 5usize;
    let mut set_a: Vec<(String, f32)> = Vec::new();
    let mut set_b: Vec<(String, f32)> = Vec::new();

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--stride" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()).filter(|n| *n >= 1) {
                    Some(n) => stride = n,
                    None => return usage("--stride needs a positive count"),
                }
            }
            "--offset" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(n) => offset = n,
                    None => return usage("--offset needs a count"),
                }
            }
            "--limit" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(n) => limit = n,
                    None => return usage("--limit needs a count"),
                }
            }
            "--reps" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()).filter(|n| *n >= 1) {
                    Some(n) => reps = n,
                    None => return usage("--reps needs a positive count"),
                }
            }
            "--set-a" => {
                i += 1;
                match parse_set(args.get(i)) {
                    Some(nv) => set_a.push(nv),
                    None => return usage("--set-a needs <name>=<value>"),
                }
            }
            "--set-b" => {
                i += 1;
                match parse_set(args.get(i)) {
                    Some(nv) => set_b.push(nv),
                    None => return usage("--set-b needs <name>=<value>"),
                }
            }
            other if model.is_empty() => model = other.to_string(),
            other if dir.is_empty() => dir = other.to_string(),
            other => return usage(&format!("unexpected argument {other:?}")),
        }
        i += 1;
    }
    if model.is_empty() || dir.is_empty() {
        return usage("need a model and a pages directory");
    }

    match run(&model, &dir, stride, offset, limit, reps, &set_a, &set_b) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("timing_ab: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse_set(v: Option<&String>) -> Option<(String, f32)> {
    v.and_then(|v| v.split_once('=')).and_then(|(n, v)| {
        v.trim().parse::<f32>().ok().map(|v| (n.to_string(), v))
    })
}

fn usage(why: &str) -> ExitCode {
    eprintln!("timing_ab: {why}");
    eprintln!(
        "usage: timing_ab <model.ocrw> <pages-dir> [--set-a name=value ...] \
         [--set-b name=value ...] [--stride N] [--offset N] [--limit N] [--reps N]"
    );
    ExitCode::FAILURE
}

fn run(
    model: &str,
    dir: &str,
    stride: usize,
    offset: usize,
    limit: usize,
    reps: usize,
    set_a: &[(String, f32)],
    set_b: &[(String, f32)],
) -> Result<(), String> {
    let bytes = std::fs::read(model).map_err(|e| format!("{model}: {e}"))?;
    let mut engine = Engine::from_bytes(&bytes).map_err(|e| format!("{model}: {e:?}"))?;

    // Mode 0, forced, ahead of reading any baseline value below, so a
    // baseline captured for `match.classifier` (if either side names it)
    // reads back the forced value, not whatever the file shipped.
    if !ocrcer_bench::knobs::set(&mut engine, "match.classifier", 0.0) {
        return Err("match.classifier: not a knob this engine takes".to_string());
    }

    // Union of every parameter either side names, in first-seen order, each
    // with the value to use when *its own* side is silent about it — read
    // back from the engine now, after the mode-0 force above and before
    // either A or B's overrides are applied.
    let mut names: Vec<String> = Vec::new();
    for (n, _) in set_a.iter().chain(set_b.iter()) {
        if !names.contains(n) {
            names.push(n.clone());
        }
    }
    let mut baseline: Vec<f32> = Vec::with_capacity(names.len());
    for n in &names {
        match engine.params().get(n) {
            Some(v) => baseline.push(v),
            None => {
                return Err(format!(
                    "{n}: not readable via Params::get; timing_ab needs the baseline value \
                     to restore whichever side leaves it unnamed"
                ))
            }
        }
    }

    let apply = |engine: &mut Engine, side: &[(String, f32)]| {
        for (idx, n) in names.iter().enumerate() {
            let v = side.iter().find(|(sn, _)| sn == n).map_or(baseline[idx], |(_, v)| *v);
            if !ocrcer_bench::knobs::set(engine, n, v) {
                panic!("{n} = {v}: engine refused a value it accepted at baseline read");
            }
        }
    };

    for (n, v) in set_a {
        println!("A   {n} = {v}");
    }
    for (n, v) in set_b {
        println!("B   {n} = {v}");
    }
    println!("mode.classifier forced to 0; reps {reps} per page, alternating A,B,A,B,...");

    let all_pages = list_pages(dir)?;
    let chosen: Vec<_> =
        all_pages.iter().skip(offset).step_by(stride).take(limit).cloned().collect();
    println!(
        "pages   {} of {} in {dir} (stride {stride}, offset {offset}, limit {limit})",
        chosen.len(),
        all_pages.len()
    );
    if chosen.is_empty() {
        return Err("no pages selected".to_string());
    }

    let mut sum_a = 0.0f64;
    let mut sum_b = 0.0f64;
    let mut ratios: Vec<f64> = Vec::with_capacity(chosen.len());
    println!("\n{:<28} {:>12} {:>12} {:>10}", "page", "A min (ms)", "B min (ms)", "B/A");

    for pgm in &chosen {
        let stem = pgm.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let (w, h, grey) = load_page(pgm)?;
        let img = || Gray { width: w, height: h, data: &grey };

        let mut a_times: Vec<f64> = Vec::with_capacity(reps);
        let mut b_times: Vec<f64> = Vec::with_capacity(reps);
        for _ in 0..reps {
            apply(&mut engine, set_a);
            let t0 = Instant::now();
            engine.recognize_lines(img()).map_err(|e| format!("{stem}: {e:?}"))?;
            a_times.push(t0.elapsed().as_secs_f64() * 1000.0);

            apply(&mut engine, set_b);
            let t0 = Instant::now();
            engine.recognize_lines(img()).map_err(|e| format!("{stem}: {e:?}"))?;
            b_times.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        let min_a = a_times.iter().cloned().fold(f64::INFINITY, f64::min);
        let min_b = b_times.iter().cloned().fold(f64::INFINITY, f64::min);
        sum_a += min_a;
        sum_b += min_b;
        let ratio = min_b / min_a;
        ratios.push(ratio);
        println!("{:<28} {:>12.1} {:>12.1} {:>10.4}", stem, min_a, min_b, ratio);
    }

    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let quartile = |q: f64| -> f64 {
        let idx = ((ratios.len() - 1) as f64 * q).round() as usize;
        ratios[idx]
    };
    let median = quartile(0.5);
    let total_ratio = sum_b / sum_a;

    println!("\nsum A (min-of-{reps}, {} pages): {:.1} ms", chosen.len(), sum_a);
    println!("sum B (min-of-{reps}, {} pages): {:.1} ms", chosen.len(), sum_b);
    println!("ratio sum(B)/sum(A): {total_ratio:.4}");
    println!(
        "per-page ratio quartiles: p25 {:.4}  p50 (median) {:.4}  p75 {:.4}  (range {:.4}-{:.4})",
        quartile(0.25),
        median,
        quartile(0.75),
        ratios.first().copied().unwrap_or(f64::NAN),
        ratios.last().copied().unwrap_or(f64::NAN),
    );
    println!(
        "process CPU time: not measured (no std cross-platform call, no CPU-time dependency in ocrcer-bench)"
    );

    Ok(())
}
