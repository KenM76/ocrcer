//! `ident`: the corpus-level identifier-preservation gate.
//!
//! ```text
//! ident generate <out-dir> <sizes-csv> [--local]
//! ident score <model.ocrw> <pages-dir> [--set <name>=<value>]... [--threshold N]
//! ```
//!
//! `CLAUDE.md` rule 6 and `PLAN.md` chunk 8 require a test that fails loudly
//! when an identifier-shaped string is silently rewritten into a different
//! identifier or a dictionary word. Two unit tests existed before this
//! binary (`docs/measurements/2026-09-25_score_12b.md` section 4); neither
//! ran an image through the engine. `score` does: real segmentation, real
//! binarization, the whole pipeline `bin/ocr.rs` exercises — not the oracle
//! reader other harnesses use.
//!
//! `generate` renders `ocrcer_bench::ident_corpus`'s text with the same
//! rasteriser `bench/pages-cov` uses (`ocrcer_build::page::render`); `score`
//! reads those pages exactly the way `ocr` does
//! (`ocrcer_bench::pages::{list_pages, load_truth_beside, load_page,
//! page_text}`). Classification calls `ocrcer_core::params::identifier_shape`
//! and `ocrcer_core::decode::lexicon::lookup` directly — see `src/ident.rs`'s
//! header for why nothing here reimplements either.
//!
//! `score` exits non-zero and lists every offender when the REWRITTEN count
//! (identifier-shaped ground truth read back as a *different* identifier-
//! shaped string, or as a lexicon word) exceeds `--threshold` (default 0 —
//! rule 6 says "never"; a nonzero default would be setting the gate to
//! whatever passes, which is the one thing this binary is required not to
//! do). If the measured count is above zero on the shipped model, that is
//! reported as a real result, not tuned away here.

use std::process::ExitCode;

use ocrcer_bench::ident::{self, Outcome};
use ocrcer_core::pipeline::Engine;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("generate") => run_generate(&args[1..]),
        Some("score") => run_score(&args[1..]),
        _ => usage("need a subcommand: generate | score"),
    }
}

fn usage(why: &str) -> ExitCode {
    eprintln!("ident: {why}");
    eprintln!(
        "usage: ident generate <out-dir> <sizes-csv> [--local]
       ident score <model.ocrw> <pages-dir> [--set <name>=<value>]... [--threshold N]"
    );
    ExitCode::FAILURE
}

fn sizes(csv: &str) -> Result<Vec<f32>, String> {
    csv.split(',')
        .map(|s| s.trim().parse::<f32>().map_err(|_| format!("{s:?}: not a size")))
        .collect()
}

fn run_generate(args: &[String]) -> ExitCode {
    let mut out_dir = String::new();
    let mut px_csv = String::new();
    let mut local = false;
    for a in args {
        match a.as_str() {
            "--local" => local = true,
            other if out_dir.is_empty() => out_dir = other.to_string(),
            other if px_csv.is_empty() => px_csv = other.to_string(),
            other => return usage(&format!("unexpected argument {other:?}")),
        }
    }
    if out_dir.is_empty() || px_csv.is_empty() {
        return usage("need an out-dir and a sizes csv");
    }
    let px_list = match sizes(&px_csv) {
        Ok(v) => v,
        Err(e) => return usage(&e),
    };
    match ident::generate(&out_dir, &px_list, local) {
        Ok(r) => {
            println!(
                "{} pages from {} faces into {out_dir}{}",
                r.written,
                r.faces_used,
                if r.skipped_missing > 0 {
                    format!(" ({} blocks skipped: face lacked a glyph)", r.skipped_missing)
                } else {
                    String::new()
                }
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("ident generate: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_score(args: &[String]) -> ExitCode {
    let mut model = String::new();
    let mut dir = String::new();
    let mut set: Vec<(String, f32)> = Vec::new();
    let mut threshold = 0usize;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--set" => {
                i += 1;
                match args.get(i).and_then(|v| v.split_once('=')).and_then(|(n, v)| {
                    v.trim().parse::<f32>().ok().map(|v| (n.to_string(), v))
                }) {
                    Some(nv) => set.push(nv),
                    None => return usage("--set needs <name>=<value>"),
                }
            }
            "--threshold" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(n) => threshold = n,
                    None => return usage("--threshold needs a count"),
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

    let bytes = match std::fs::read(&model) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("ident score: {model}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut engine = match Engine::from_bytes(&bytes) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("ident score: {model}: {e:?}");
            return ExitCode::FAILURE;
        }
    };
    for (name, v) in &set {
        if !ocrcer_bench::knobs::set(&mut engine, name, *v) {
            eprintln!("ident score: {name} = {v}: not a knob this engine takes");
            return ExitCode::FAILURE;
        }
        println!("set     {name} = {v}  (an override, not what the file carries)");
    }

    let (tally, offenders) = match ident::score_dir(&engine, &dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ident score: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!(
        "identifiers  {} tested: {} exact, {} dropped-or-garbled, {} REWRITTEN \
         ({} to a different identifier, {} to a lexicon word)",
        tally.total(),
        tally.exact,
        tally.dropped,
        tally.rewritten(),
        tally.rewritten_identifier,
        tally.rewritten_lexicon,
    );
    if tally.total() > 0 {
        println!(
            "rate         exact {:.3}%  dropped-or-garbled {:.3}%  REWRITTEN {:.3}%",
            100.0 * tally.exact as f64 / tally.total() as f64,
            100.0 * tally.dropped as f64 / tally.total() as f64,
            100.0 * tally.rewritten() as f64 / tally.total() as f64,
        );
    }
    if !offenders.is_empty() {
        println!();
        println!("REWRITTEN offenders (truth -> read, page, line, kind):");
        for o in &offenders {
            let kind = match o.outcome {
                Outcome::RewrittenIdentifier => "different identifier",
                Outcome::RewrittenLexicon => "lexicon word",
                _ => unreachable!("offenders is filtered to the two REWRITTEN outcomes"),
            };
            println!(
                "  {:?} -> {}   {} line {}   [{kind}]",
                o.truth_word,
                o.hyp_word.as_deref().unwrap_or("<none>"),
                o.stem,
                o.line
            );
        }
    }

    println!();
    if tally.rewritten() > threshold {
        println!(
            "FAIL         REWRITTEN {} > threshold {threshold}",
            tally.rewritten()
        );
        ExitCode::FAILURE
    } else {
        println!("PASS         REWRITTEN {} <= threshold {threshold}", tally.rewritten());
        ExitCode::SUCCESS
    }
}
