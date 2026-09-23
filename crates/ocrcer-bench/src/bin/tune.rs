//! `tune`: sweeps one named parameter end to end and reports what each value
//! measured, so a threshold `model/params.tsv` labels a guess can be replaced
//! by a number with a run behind it.
//!
//! ```text
//! tune <model.ocrw> <pages-dir> <name>=<v1,v2,...> [<name>=... ...]
//!      [--limit N] [--stride N] [--offset N]
//! ```
//!
//! `<name>` is any key in `Params::NAMES`, or the pseudo-key
//! `match.geometry_weight`, which weights `ARCHITECTURE.md` section 3.1's
//! dims 103..107 — the baseline-relative geometry block — in the matcher's
//! distance. That block is where a case pair like `o`/`O` lives: on a
//! normalised 32x32 grid the two are the same shape, so 103 of the 107
//! dimensions cannot tell them apart and only these four can.
//!
//! Several `<name>=<values>` arguments sweep the **cross product**, which is
//! how an interaction shows up — a bigram weight that helps at one geometry
//! weight and hurts at another is a thing a pair of one-parameter sweeps
//! cannot see.
//!
//! # What a winning row is and is not
//!
//! It is the best value of a small family **on this corpus**, measured. It is
//! not the right value on unseen input, and a sweep run on the pages it will
//! be reported against is fitting in the only sense this project admits —
//! `CLAUDE.md` rule 1 asks for a number that can be explained by a sentence,
//! and "it won a sweep over pages of this kind" is such a sentence only when
//! the pages are named and the margin is stated. Report the margin between
//! the winner and the control, not the winner alone.
//!
//! Every figure here is **end to end**: the engine is handed the page and
//! nothing else, so a value that helps the matcher but costs the decoder
//! shows up as the loss it is.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::Instant;

use ocrcer_bench::cer::{score, token_score, Score, TokenScore};
use ocrcer_bench::pages::{list_pages, load_page, load_truth_beside, page_text};
use ocrcer_core::pipeline::Engine;
use ocrcer_core::Gray;

use ocrcer_bench::knobs::{self, GEOMETRY_WEIGHT};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let (model, pages, rest) = match argv.as_slice() {
        [m, p, rest @ ..] if !rest.is_empty() => (*m, *p, rest),
        _ => {
            eprintln!(
                "usage: tune <model.ocrw> <pages-dir> <name>=<v1,v2,...> [more...] \
                 [--limit N] [--stride N] [--offset N]"
            );
            return ExitCode::FAILURE;
        }
    };

    let mut limit = usize::MAX;
    let mut stride = 1usize;
    // Where the strided sample starts. The point of it is a held-out check:
    // a value chosen on `--stride 13` and re-measured on `--stride 13
    // --offset 1` was fitted on one set of pages and tested on another, and a
    // winner that does not survive that is a winner of a sweep and not a
    // parameter.
    let mut offset = 0usize;
    let mut axes: Vec<(String, Vec<f32>)> = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match *a {
            "--limit" => match it.next().and_then(|n| n.parse().ok()) {
                Some(n) => limit = n,
                None => {
                    eprintln!("tune: --limit wants an integer");
                    return ExitCode::FAILURE;
                }
            },
            "--stride" => match it.next().and_then(|n| n.parse::<usize>().ok()) {
                Some(n) if n > 0 => stride = n,
                _ => {
                    eprintln!("tune: --stride wants a positive integer");
                    return ExitCode::FAILURE;
                }
            },
            "--offset" => match it.next().and_then(|n| n.parse::<usize>().ok()) {
                Some(n) => offset = n,
                None => {
                    eprintln!("tune: --offset wants a non-negative integer");
                    return ExitCode::FAILURE;
                }
            },
            spec => {
                let Some((name, vals)) = spec.split_once('=') else {
                    eprintln!("tune: {spec:?} is not <name>=<v1,v2,...>");
                    return ExitCode::FAILURE;
                };
                let parsed: Result<Vec<f32>, _> =
                    vals.split(',').map(|v| v.trim().parse::<f32>()).collect();
                match parsed {
                    Ok(v) if !v.is_empty() => axes.push((name.to_string(), v)),
                    _ => {
                        eprintln!("tune: {name}: values must be a comma-separated float list");
                        return ExitCode::FAILURE;
                    }
                }
            }
        }
    }
    if axes.is_empty() {
        eprintln!("tune: nothing to sweep");
        return ExitCode::FAILURE;
    }

    match run(model, pages, &axes, limit, stride, offset) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tune: {e}");
            ExitCode::FAILURE
        }
    }
}

/// One page, loaded once and reused across every point of the sweep, so a
/// difference between rows cannot be a difference in what was read from disk.
struct Page {
    stem: String,
    width: u32,
    height: u32,
    grey: Vec<u8>,
    reference: String,
    px: u32,
}

/// Every combination of the axes, in a fixed order: last axis varies fastest,
/// which puts a one-axis sweep in the order it was written.
fn grid(axes: &[(String, Vec<f32>)]) -> Vec<Vec<f32>> {
    let mut out: Vec<Vec<f32>> = vec![Vec::new()];
    for (_, vals) in axes {
        let mut next = Vec::with_capacity(out.len() * vals.len());
        for prefix in &out {
            for v in vals {
                let mut row = prefix.clone();
                row.push(*v);
                next.push(row);
            }
        }
        out = next;
    }
    out
}

fn run(
    model: &str,
    pages_dir: &str,
    axes: &[(String, Vec<f32>)],
    limit: usize,
    stride: usize,
    offset: usize,
) -> Result<(), String> {
    let bytes = std::fs::read(model).map_err(|e| format!("{model}: {e}"))?;
    // One load, then a fresh engine per point: `Engine` is cheap to clone
    // from bytes and an override must not leak from one point into the next.
    let probe = Engine::from_bytes(&bytes).map_err(|e| format!("{model}: {e:?}"))?;
    for (name, _) in axes {
        if !knobs::known(&probe, name) {
            return Err(format!("{name}: not a parameter name, and not {GEOMETRY_WEIGHT}"));
        }
    }

    let pgms = list_pages(pages_dir)?;
    let mut pages: Vec<Page> = Vec::new();
    for pgm in pgms.iter().skip(offset).step_by(stride).take(limit) {
        let truth = load_truth_beside(pgm)?;
        let (w, h, grey) = load_page(pgm)?;
        pages.push(Page {
            stem: pgm.file_stem().unwrap_or_default().to_string_lossy().to_string(),
            width: w,
            height: h,
            grey,
            reference: truth.lines.join("\n"),
            px: truth.px_per_em.round() as u32,
        });
    }
    if pages.is_empty() {
        return Err(format!("no pages in {pages_dir}"));
    }

    println!("model   {model}");
    println!("{}", ocrcer_bench::provenance::line(std::path::Path::new(model)));
    println!("pages   {} of {} in {pages_dir} (stride {stride}, offset {offset})", pages.len(), pgms.len());
    for (name, vals) in axes {
        let at = match probe.params().get(name) {
            Some(v) => format!("{v}"),
            None => "1 (no feature_weights table in the file)".into(),
        };
        println!("sweep   {name}  now {at}  over {vals:?}");
    }
    let points = grid(axes);
    println!("        {} points x {} pages\n", points.len(), pages.len());

    // Each column is as wide as its own heading needs, never a fixed 14: a
    // name longer than the column silently overruns into the next heading and
    // the table stops being readable exactly when the sweep gets interesting.
    let widths: Vec<usize> = axes.iter().map(|(n, _)| short(n).len().max(12) + 2).collect();
    let mut head = String::new();
    for ((name, _), w) in axes.iter().zip(&widths) {
        head.push_str(&format!("{:>w$}", short(name), w = *w));
    }
    println!("{head}      CER      WER       F1    sec");

    let mut best: Option<(Vec<f32>, f64)> = None;
    let mut control: Option<f64> = None;
    let mut rows: Vec<(Vec<f32>, Score, TokenScore)> = Vec::new();

    for point in &points {
        let mut engine = Engine::from_bytes(&bytes).map_err(|e| format!("{model}: {e:?}"))?;
        for ((name, _), v) in axes.iter().zip(point) {
            if !knobs::set(&mut engine, name, *v) {
                return Err(format!("{name}: {v} was refused"));
            }
        }

        let mut seq = Score::default();
        let mut tok = TokenScore::default();
        let t0 = Instant::now();
        for p in &pages {
            let lines = engine
                .recognize_lines(Gray { width: p.width, height: p.height, data: &p.grey })
                .map_err(|e| format!("{}: {e:?}", p.stem))?;
            let read = page_text(&lines);
            seq.add(&score(&p.reference, &read));
            tok.add(&token_score(&p.reference, &read));
        }
        let secs = t0.elapsed().as_secs_f64();

        let mut line = String::new();
        for (v, w) in point.iter().zip(&widths) {
            line.push_str(&format!("{v:>w$}", w = *w));
        }
        println!(
            "{line}  {:>7}  {:>7}  {:>7}  {secs:>5.1}",
            opt_pct(seq.cer()),
            opt_pct(seq.wer()),
            opt_pct(tok.f1()),
        );

        let cer = seq.cer().unwrap_or(1.0);
        if is_control(axes, point, &probe) {
            control = Some(cer);
        }
        if best.as_ref().is_none_or(|(_, b)| cer < *b) {
            best = Some((point.clone(), cer));
        }
        rows.push((point.clone(), seq, tok));
    }

    // A sweep whose rows all measure the same thing has no winner, and
    // printing the first row as one is how "3 is the measured optimum" gets
    // written down about a parameter that never bound on this corpus. Say
    // inert, and say it before the best line.
    let inert = rows.len() > 1
        && rows.iter().all(|(_, seq, tok)| {
            let (s0, t0) = (&rows[0].1, &rows[0].2);
            seq.cer() == s0.cer() && seq.wer() == s0.wer() && tok.f1() == t0.f1()
        });
    if inert {
        let names: Vec<&str> = axes.iter().map(|(n, _)| n.as_str()).collect();
        println!(
            "
inert   every value of {} measured identical CER, WER and F1 on these pages.
        There is no argmax here: the parameter did not bind, and a value this
        sweep cannot choose stays authored rather than becoming measured.",
            names.join(" and ")
        );
    }
    if let Some((point, cer)) = &best {
        let named: Vec<String> =
            axes.iter().zip(point).map(|((n, _), v)| format!("{n} = {v}")).collect();
        println!("\nbest    {}", named.join(", "));
        println!("        CER {:.3}%", 100.0 * cer);
        match control {
            Some(c) => println!(
                "        the file's own values measured CER {:.3}% on the same pages, so the\n        \
                 margin is {:.3} points",
                100.0 * c,
                100.0 * (c - cer)
            ),
            None => println!(
                "        the file's own values were not in the swept set, so there is no\n        \
                 control row and the margin is unstated"
            ),
        }
    }

    // A per-size breakdown of the winner only: a value that wins overall by
    // helping large text while hurting 14px is a different proposition from
    // one that helps everywhere, and the aggregate hides which it is.
    if let Some((point, _)) = &best {
        let mut engine = Engine::from_bytes(&bytes).map_err(|e| format!("{model}: {e:?}"))?;
        for ((name, _), v) in axes.iter().zip(point) {
            knobs::set(&mut engine, name, *v);
        }
        let mut by_size: BTreeMap<u32, Score> = BTreeMap::new();
        for p in &pages {
            let lines = engine
                .recognize_lines(Gray { width: p.width, height: p.height, data: &p.grey })
                .map_err(|e| format!("{}: {e:?}", p.stem))?;
            let read = page_text(&lines);
            by_size.entry(p.px).or_default().add(&score(&p.reference, &read));
        }
        println!("\nthe winning row, by px/em");
        for (px, s) in &by_size {
            println!("  {px:>4}px   CER {:>7}   ({} chars)", opt_pct(s.cer()), s.chars);
        }
    }

    println!(
        "\nEnd to end: the engine was handed the page and nothing else. A winning\n\
         row is the best of a small family on these pages, not a measurement of\n\
         the right value on unseen input — quote it with its margin over the\n\
         control row, never alone."
    );
    Ok(())
}

/// Whether a point is the model file's own values on every axis.
fn is_control(axes: &[(String, Vec<f32>)], point: &[f32], probe: &Engine) -> bool {
    axes.iter().zip(point).all(|((name, _), v)| match probe.params().get(name) {
        Some(cur) => (cur - v).abs() < 1e-6,
        // No `feature_weights` table means every dimension weighs one.
        None => (v - 1.0).abs() < 1e-6,
    })
}

fn opt_pct(v: Option<f64>) -> String {
    match v {
        Some(x) => format!("{:.3}%", 100.0 * x),
        None => "n/a".into(),
    }
}

/// The part of a dotted parameter name that fits a column header.
fn short(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::grid;

    #[test]
    fn one_axis_sweeps_in_the_order_it_was_written() {
        let axes = vec![("a".to_string(), vec![1.0, 2.0, 3.0])];
        assert_eq!(grid(&axes), vec![vec![1.0], vec![2.0], vec![3.0]]);
    }

    #[test]
    fn two_axes_sweep_the_cross_product_with_the_last_varying_fastest() {
        let axes =
            vec![("a".to_string(), vec![1.0, 2.0]), ("b".to_string(), vec![10.0, 20.0])];
        assert_eq!(
            grid(&axes),
            vec![vec![1.0, 10.0], vec![1.0, 20.0], vec![2.0, 10.0], vec![2.0, 20.0]]
        );
    }
}
