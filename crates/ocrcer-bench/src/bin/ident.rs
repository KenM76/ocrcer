//! `ident`: the corpus-level identifier-preservation gate.
//!
//! ```text
//! ident generate <out-dir> <sizes-csv> [--local]
//! ident score <model.ocrw> <pages-dir> [--set <name>=<value>]...
//!              [--threshold N] [--lexicon-threshold N]
//!              [--confident-threshold N] [--offenders-out <path>]
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
//! bonus actually changed from ones that only coincidentally read back as a
//! dictionary word (`src/ident.rs`'s "Causation, not just coincidence"
//! section explains why the plain output check undercounted this).
//!
//! Three independent gates (rule 6 says "never"; a nonzero default on the
//! hard gate would itself be setting the gate to whatever passes, which is
//! the one thing this binary must not do):
//!
//! - `--threshold` (default 0, ratchet) — REWRITTEN count (a different
//!   identifier, or a lexicon word). Lower it as fixes land, never raise it
//!   to make a build pass.
//! - `--lexicon-threshold` (default 0, hard) — LEXICON-HARM count
//!   (`ocrcer_bench::ident::LexCase::harm`), the architect's narrower
//!   condition (`ARCHITECTURE.md` §11, 2026-09-25, "Identifier gates"):
//!   among LEXICON-CHANGED cases, the ones where the lexicon-off run would
//!   have been correct or the lexicon-on word is itself a lexicon entry.
//!   The broader LEXICON-CHANGED count is always printed but never gated —
//!   see `src/ident.rs`'s "LEXICON-CHANGED vs. LEXICON-HARM" section.
//! - `--confident-threshold` (unset by default, report-only) — REWRITTEN
//!   count at word confidence >= 0.9, the population rule 5 calls the harm:
//!   a wrong identifier reported with a reviewer-trusted score.
//!
//! `--offenders-out <path>` writes the full REWRITTEN and LEXICON-CHANGED
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
                    [--threshold N] [--lexicon-threshold N]
                    [--confident-threshold N] [--offenders-out <path>]"
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
    let mut confident_threshold: Option<usize> = None;
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
            "--confident-threshold" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(n) => confident_threshold = Some(n),
                    None => return usage("--confident-threshold needs a count"),
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
    let ident::Report { tally, offenders, confidence, lexicon_changed } = report;
    let lexicon_harm: Vec<&ident::LexCase> = lexicon_changed.iter().filter(|c| c.harm).collect();
    let rewritten_hi_conf = confidence.rewritten_at_least(0.9);

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

    // Lexicon A/B. LEXICON-CHANGED is reported, never gated; LEXICON-HARM
    // (the architect's narrower subset, `ARCHITECTURE.md` §11 2026-09-25
    // "Identifier gates") is the hard gate, `--lexicon-threshold`, default
    // 0. See `src/ident.rs`'s "LEXICON-CHANGED vs. LEXICON-HARM" section.
    println!();
    println!(
        "lexicon A/B  re-scored every identifier token at decode.w_lex=0; \
         {} case(s) are LEXICON-CHANGED (on-run disagrees with off-run AND on-run is wrong), \
         {} of those are LEXICON-HARM (off-run would have been correct, or the on-run word is \
         itself a lexicon entry)",
        lexicon_changed.len(),
        lexicon_harm.len(),
    );

    let case_text = format_cases(&offenders, &lexicon_changed);
    match &offenders_out {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &case_text) {
                eprintln!("ident score: writing {path}: {e}");
                return ExitCode::FAILURE;
            }
            println!(
                "             {} REWRITTEN offender(s), {} LEXICON-CHANGED case(s) \
                 ({} LEXICON-HARM) written to {path}",
                offenders.len(),
                lexicon_changed.len(),
                lexicon_harm.len(),
            );
        }
        None => {
            print!("{case_text}");
        }
    }

    println!();
    let rewritten_fail = tally.rewritten() > threshold;
    let lexicon_fail = lexicon_harm.len() > lexicon_threshold;
    let confident_fail = confident_threshold.is_some_and(|n| rewritten_hi_conf > n);
    println!(
        "{}         REWRITTEN {} {} threshold {threshold}",
        if rewritten_fail { "FAIL" } else { "PASS" },
        tally.rewritten(),
        if rewritten_fail { ">" } else { "<=" },
    );
    println!(
        "{}         LEXICON-HARM {} {} lexicon-threshold {lexicon_threshold}  \
         (LEXICON-CHANGED {}, report-only, ungated)",
        if lexicon_fail { "FAIL" } else { "PASS" },
        lexicon_harm.len(),
        if lexicon_fail { ">" } else { "<=" },
        lexicon_changed.len(),
    );
    match confident_threshold {
        Some(n) => println!(
            "{}         REWRITTEN@>=0.9 {} {} confident-threshold {n}",
            if confident_fail { "FAIL" } else { "PASS" },
            rewritten_hi_conf,
            if confident_fail { ">" } else { "<=" },
        ),
        None => println!(
            "n/a          REWRITTEN@>=0.9 {rewritten_hi_conf}  (report-only; no --confident-threshold set)"
        ),
    }
    if rewritten_fail || lexicon_fail || confident_fail {
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

/// The REWRITTEN offender list and the LEXICON-CHANGED case list, as text —
/// shared by the stdout path and the `--offenders-out` file path so the two
/// can never drift into different formats. Each LEXICON-CHANGED line is
/// tagged `[HARM]` or `[benign]` per `LexCase::harm`.
fn format_cases(offenders: &[ident::IdentCase], lexicon_changed: &[ident::LexCase]) -> String {
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
    if !lexicon_changed.is_empty() {
        s.push_str("\nLEXICON-CHANGED cases (truth -> on-read | off-read, page, line, harm):\n");
        for c in lexicon_changed {
            s.push_str(&format!(
                "  {:?} -> {} | {}   {} line {}   [{}]\n",
                c.truth_word,
                c.hyp_on.as_deref().unwrap_or("<none>"),
                c.hyp_off.as_deref().unwrap_or("<none>"),
                c.stem,
                c.line,
                if c.harm { "HARM" } else { "benign" },
            ));
        }
    }
    s
}
