//! `ident`: the corpus-level identifier-preservation gate.
//!
//! ```text
//! ident generate <out-dir> <sizes-csv> [--local]
//! ident score <model.ocrw> <pages-dir> [--set <name>=<value>]...
//!              [--threshold N] [--lexicon-threshold N] [--offenders-out <path>]
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
//! (`ocrcer_bench::pages::{list_pages, load_truth_beside, load_page}`).
//! Classification calls `ocrcer_core::params::identifier_shape` and
//! `ocrcer_core::decode::lexicon::lookup` directly — see `src/ident.rs`'s
//! header for why nothing here reimplements either.
//!
//! `score` runs every page twice — once at the model's own `decode.w_lex`
//! (after any `--set` overrides), once with `decode.w_lex` forced to `0` and
//! every other parameter unchanged — to separate REWRITTEN cases the lexicon
//! bonus actually caused from ones that only coincidentally read back as a
//! dictionary word (`src/ident.rs`'s "Causation, not just coincidence"
//! section explains why the plain output check undercounted this).
//!
//! Two independent, hard gates, both default 0 (rule 6 says "never"; a
//! nonzero default would itself be setting the gate to whatever passes,
//! which is the one thing this binary must not do):
//!
//! - `--threshold` — REWRITTEN count (a different identifier, or a lexicon
//!   word, whether or not the lexicon caused it). A ratchet: lower it as
//!   fixes land, never raise it to make a build pass.
//! - `--lexicon-threshold` — LEXICON-CAUSED count (`ocrcer_bench::ident::
//!   Report::lexicon_caused`). Measured on the fitted config at introduction
//!   and left at 0; not required to pass yet (`docs/measurements/
//!   2026-09-25_ident_corpus.md`).
//!
//! `--offenders-out <path>` writes the full REWRITTEN and LEXICON-CAUSED
//! case lists to `path` (one text file; overwritten each run) instead of
//! stdout, so a report can cite a regenerable file under the gitignored
//! `bench/ident/` rather than pasting thousands of lines into committed
//! markdown.

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
       ident score <model.ocrw> <pages-dir> [--set <name>=<value>]...
                    [--threshold N] [--lexicon-threshold N] [--offenders-out <path>]"
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
    let mut lexicon_threshold = 0usize;
    let mut offenders_out: Option<String> = None;
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
            "--lexicon-threshold" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(n) => lexicon_threshold = n,
                    None => return usage("--lexicon-threshold needs a count"),
                }
            }
            "--offenders-out" => {
                i += 1;
                match args.get(i) {
                    Some(p) => offenders_out = Some(p.clone()),
                    None => return usage("--offenders-out needs a path"),
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
    // Two engines: `engine_on` reads at whatever `decode.w_lex` the model
    // (plus any `--set`) carries; `engine_off` is identical except
    // `decode.w_lex = 0`, forced *after* the same `--set` overrides so a
    // caller who explicitly sets `decode.w_lex` still gets a clean A/B
    // against "no lexicon at all" rather than against their own override.
    let mut engine_on = match Engine::from_bytes(&bytes) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("ident score: {model}: {e:?}");
            return ExitCode::FAILURE;
        }
    };
    let mut engine_off = match Engine::from_bytes(&bytes) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("ident score: {model}: {e:?}");
            return ExitCode::FAILURE;
        }
    };
    for (name, v) in &set {
        let ok_on = ocrcer_bench::knobs::set(&mut engine_on, name, *v);
        let ok_off = ocrcer_bench::knobs::set(&mut engine_off, name, *v);
        if !ok_on || !ok_off {
            eprintln!("ident score: {name} = {v}: not a knob this engine takes");
            return ExitCode::FAILURE;
        }
        println!("set     {name} = {v}  (an override, not what the file carries)");
    }
    if !ocrcer_bench::knobs::set(&mut engine_off, "decode.w_lex", 0.0) {
        eprintln!("ident score: decode.w_lex: not a knob this engine takes");
        return ExitCode::FAILURE;
    }

    let report = match ident::score_dir(&engine_on, &engine_off, &dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ident score: {e}");
            return ExitCode::FAILURE;
        }
    };
    let ident::Report { tally, offenders, confidence, lexicon_caused } = report;

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

    // Confidence split (rule 5: a confident wrong answer is the harm).
    // Measured only — no gate reads this yet.
    println!();
    println!(
        "confidence   REWRITTEN n={} median={}  >=0.5: {} ({:.1}%)  >=0.8: {} ({:.1}%)  >=0.9: {} ({:.1}%)",
        confidence.rewritten.len(),
        confidence.median_rewritten().map_or("n/a".to_string(), |m| format!("{m:.3}")),
        confidence.rewritten_at_least(0.5),
        pct(confidence.rewritten_at_least(0.5), confidence.rewritten.len()),
        confidence.rewritten_at_least(0.8),
        pct(confidence.rewritten_at_least(0.8), confidence.rewritten.len()),
        confidence.rewritten_at_least(0.9),
        pct(confidence.rewritten_at_least(0.9), confidence.rewritten.len()),
    );
    println!(
        "             exact     n={} median={}",
        confidence.exact.len(),
        confidence.median_exact().map_or("n/a".to_string(), |m| format!("{m:.3}")),
    );

    // Lexicon A/B (hard gate, `--lexicon-threshold`, default 0). See
    // `src/ident.rs`'s `score_dir` doc for the LEXICON-CAUSED definition.
    println!();
    println!(
        "lexicon A/B  re-scored every identifier token at decode.w_lex=0; \
         {} case(s) are LEXICON-CAUSED (on-run disagrees with off-run AND on-run is wrong)",
        lexicon_caused.len()
    );

    let case_text = format_cases(&offenders, &lexicon_caused);
    match &offenders_out {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &case_text) {
                eprintln!("ident score: writing {path}: {e}");
                return ExitCode::FAILURE;
            }
            println!(
                "             {} REWRITTEN offender(s), {} LEXICON-CAUSED case(s) written to {path}",
                offenders.len(),
                lexicon_caused.len()
            );
        }
        None => {
            print!("{case_text}");
        }
    }

    println!();
    let rewritten_fail = tally.rewritten() > threshold;
    let lexicon_fail = lexicon_caused.len() > lexicon_threshold;
    println!(
        "{}         REWRITTEN {} {} threshold {threshold}",
        if rewritten_fail { "FAIL" } else { "PASS" },
        tally.rewritten(),
        if rewritten_fail { ">" } else { "<=" },
    );
    println!(
        "{}         LEXICON-CAUSED {} {} lexicon-threshold {lexicon_threshold}",
        if lexicon_fail { "FAIL" } else { "PASS" },
        lexicon_caused.len(),
        if lexicon_fail { ">" } else { "<=" },
    );
    if rewritten_fail || lexicon_fail {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn pct(n: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        100.0 * n as f64 / total as f64
    }
}

/// The REWRITTEN offender list and the LEXICON-CAUSED case list, as text —
/// shared by the stdout path and the `--offenders-out` file path so the two
/// can never drift into different formats.
fn format_cases(offenders: &[ident::IdentCase], lexicon_caused: &[ident::LexCase]) -> String {
    let mut s = String::new();
    if !offenders.is_empty() {
        s.push_str("REWRITTEN offenders (truth -> read, page, line, kind, confidence):\n");
        for o in offenders {
            let kind = match o.outcome {
                Outcome::RewrittenIdentifier => "different identifier",
                Outcome::RewrittenLexicon => "lexicon word",
                _ => unreachable!("offenders is filtered to the two REWRITTEN outcomes"),
            };
            s.push_str(&format!(
                "  {:?} -> {}   {} line {}   [{kind}]   conf={:.3}\n",
                o.truth_word,
                o.hyp_word.as_deref().unwrap_or("<none>"),
                o.stem,
                o.line,
                o.confidence,
            ));
        }
    }
    if !lexicon_caused.is_empty() {
        s.push_str("\nLEXICON-CAUSED cases (truth -> on-read | off-read, page, line):\n");
        for c in lexicon_caused {
            s.push_str(&format!(
                "  {:?} -> {} | {}   {} line {}\n",
                c.truth_word,
                c.hyp_on.as_deref().unwrap_or("<none>"),
                c.hyp_off.as_deref().unwrap_or("<none>"),
                c.stem,
                c.line,
            ));
        }
    }
    s
}
