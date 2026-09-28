//! `paddle-score`: scores a third-party engine's already-produced text
//! against the same truth files `ocr` and `ident` score against, using the
//! same `cer`/`ident` functions — never a second implementation of either.
//!
//! PaddleOCR itself is Python and out of process; nothing about running it
//! belongs in this crate or in `ocrcer-core`/`ocrcer-build` (`CLAUDE.md`
//! rules 2 and 4 — no third-party code enters those crates, ever). A
//! separate script drives PaddleOCR and writes one `<stem>.txt` per page
//! into a directory; this binary reads that directory and the corpus's own
//! `pages-dir`, then scores exactly as `ocr` does.
//!
//! ```text
//! paddle-score cer <pages-dir> <text-dir> [--csv PATH] [--worst N]
//! paddle-score ident <pages-dir> <text-dir>
//! ```
//!
//! # `cer`
//!
//! Reference is `truth.lines.join("\n")`, read is the file's contents
//! verbatim (created by the driver script's own stated top-to-bottom,
//! left-to-right box-ordering rule — see that script's header). Scored with
//! `ocrcer_bench::cer::{score, token_score, line_matched_score}`, the same
//! three functions `bin/ocr.rs`'s `Column` calls, so every number here is
//! directly comparable to one in a `docs/measurements/*_score_*.md` report.
//! A page with no corresponding `<stem>.txt` scores as an empty read (total
//! miss), not skipped, and is counted and reported as such.
//!
//! # `ident`
//!
//! A weaker check than `ocrcer-bench`'s own `ident` binary, and stated as
//! such rather than implied to be equivalent. `ocrcer_bench::ident`'s own
//! harness aligns hypothesis words to truth words position-by-position
//! within a line (`align_words`) before deciding REWRITTEN vs. DROPPED vs.
//! exact. That alignment needs an engine's own word/line objects; Paddle's
//! output here is a flat, reconstructed page string with no such structure
//! preserved. This mode instead asks a looser question: does the truth's
//! identifier-shaped token appear, verbatim, as a whitespace-delimited
//! token anywhere on the page? That is easier to satisfy than exact
//! positional agreement (a token could be "preserved" here while having
//! moved to the wrong place on the page) and harder to satisfy than a
//! same-edit-distance fuzzy match (nothing partial counts). Report this
//! rate beside OCRcer's own published REWRITTEN/DROPPED/exact split, not as
//! the same measurement.
//!
//! Identifier-shape is decided by `ocrcer_bench::ident::is_identifier_shaped`
//! against `ocrcer_core::params::Params::DEFAULT.decode` — the same
//! predicate and the same (unfitted-override) decode thresholds the
//! shipped model itself carries for `identifier_min_length` and
//! `identifier_digit_fraction`, per that struct's own doc comment; not a
//! second definition.

use std::collections::{BTreeMap, HashSet};
use std::process::ExitCode;

use ocrcer_bench::cer::{line_matched_score, score, token_score, LineScore, Score, TokenScore};
use ocrcer_bench::ident::is_identifier_shaped;
use ocrcer_bench::pages::{list_pages, load_truth_beside};
use ocrcer_core::params::Params;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("cer") => run_cer(&args[1..]),
        Some("ident") => run_ident(&args[1..]),
        _ => usage("need a subcommand: cer | ident"),
    }
}

fn usage(why: &str) -> ExitCode {
    eprintln!("paddle-score: {why}");
    eprintln!(
        "usage: paddle-score cer <pages-dir> <text-dir> [--csv PATH] [--worst N]
       paddle-score ident <pages-dir> <text-dir> [--stride N] [--offset N]"
    );
    ExitCode::FAILURE
}

fn read_paddle_text(text_dir: &str, stem: &str) -> (String, bool) {
    let path = std::path::Path::new(text_dir).join(format!("{stem}.txt"));
    match std::fs::read_to_string(&path) {
        Ok(s) => (s, true),
        Err(_) => (String::new(), false),
    }
}

fn run_cer(args: &[String]) -> ExitCode {
    let mut pages_dir = String::new();
    let mut text_dir = String::new();
    let mut csv: Option<String> = None;
    let mut worst = 12usize;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--csv" => {
                i += 1;
                match args.get(i) {
                    Some(v) => csv = Some(v.clone()),
                    None => return usage("--csv needs a path"),
                }
            }
            "--worst" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(n) => worst = n,
                    None => return usage("--worst needs a count"),
                }
            }
            other if pages_dir.is_empty() => pages_dir = other.to_string(),
            other if text_dir.is_empty() => text_dir = other.to_string(),
            other => return usage(&format!("unexpected argument {other:?}")),
        }
        i += 1;
    }
    if pages_dir.is_empty() || text_dir.is_empty() {
        return usage("need a pages-dir and a text-dir");
    }

    let pgms = match list_pages(&pages_dir) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("paddle-score: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("pages   {} in {pages_dir}, text from {text_dir}", pgms.len());

    let mut seq = Score::default();
    let mut tok = TokenScore::default();
    let mut lm = LineScore::default();
    // page-mean per category: (sum of per-page CER, page count), matching
    // `docs/measurements/2026-09-25_score_12b.md` section 2's own stated
    // methodology ("page-mean, n=90 except currency n=85") rather than a
    // corpus-total char-weighted figure, so the two are directly
    // comparable, category by category.
    let mut by_category: BTreeMap<String, (f64, usize)> = BTreeMap::new();
    let mut missing = 0usize;
    let mut pages: Vec<(f64, String)> = Vec::new();
    let mut csv_rows: Vec<(String, Option<f64>, Option<f64>)> = Vec::new();

    for pgm in &pgms {
        let stem = pgm.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let truth = match load_truth_beside(pgm) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("paddle-score: {stem}: {e}");
                continue;
            }
        };
        let reference = truth.lines.join("\n");
        let (read, found) = read_paddle_text(&text_dir, &stem);
        if !found {
            missing += 1;
        }

        let s = score(&reference, &read);
        let t = token_score(&reference, &read);
        let l = line_matched_score(&reference, &read);
        if let Some(c) = s.cer() {
            pages.push((c, stem.clone()));
        }
        if csv.is_some() {
            csv_rows.push((stem.clone(), s.cer(), l.cer()));
        }
        seq.add(&s);
        tok.add(&t);
        lm.add(&l);
        // Third `__`-delimited field, matching `bin/ocr.rs`'s and
        // `score_12b.md`'s pages-cov category convention
        // (`<face>__<style>__<category>__<size>px`); absent on corpora that
        // do not use that stem shape (e.g. finfilings), where every page
        // falls into one bucket, `"?"`.
        let category = stem.split("__").nth(2).unwrap_or("?").to_string();
        if let Some(c) = s.cer() {
            let entry = by_category.entry(category).or_insert((0.0, 0));
            entry.0 += c;
            entry.1 += 1;
        }
    }

    let pct = |v: Option<f64>| v.map_or("--".to_string(), |x| format!("{:6.3}%", x * 100.0));
    println!(
        "end-to-end  CER {}  WER {}  recall {}  precision {}  F1 {}",
        pct(seq.cer()),
        pct(seq.wer()),
        pct(tok.recall()),
        pct(tok.precision()),
        pct(tok.f1()),
    );
    println!("line-matched CER {}", pct(lm.cer()));
    if missing > 0 {
        println!(
            "\nWARNING  {missing} of {} pages had no matching <stem>.txt in {text_dir} \
             and were scored as a total miss (empty read), not skipped.",
            pgms.len()
        );
    }

    if by_category.len() > 1 {
        println!("\nby category (page-mean CER, matching 2026-09-25_score_12b.md section 2)");
        for (name, (sum, n)) in &by_category {
            println!("  {:6.3}%  {name} (n={n})", 100.0 * sum / *n as f64);
        }
    }

    if worst > 0 {
        pages.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)));
        println!("\nworst pages");
        for (c, stem) in pages.into_iter().take(worst) {
            println!("  {:6.2}%  {stem}", c * 100.0);
        }
    }

    if let Some(path) = csv {
        use std::io::Write;
        let mut f = match std::fs::File::create(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("paddle-score: {path}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let _ = writeln!(f, "stem,cer,line_matched_cer");
        for (stem, cer, lm_cer) in &csv_rows {
            let fmt = |v: &Option<f64>| v.map_or(String::new(), |x| format!("{x:.8}"));
            let _ = writeln!(f, "{stem},{},{}", fmt(cer), fmt(lm_cer));
        }
        println!("\ncsv     {path} ({} rows)", csv_rows.len());
    }

    ExitCode::SUCCESS
}

fn run_ident(args: &[String]) -> ExitCode {
    let mut pages_dir = String::new();
    let mut text_dir = String::new();
    let mut stride = 1usize;
    let mut offset = 0usize;
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--stride" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(n) => stride = n,
                    None => return usage("--stride needs a count"),
                }
            }
            "--offset" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(n) => offset = n,
                    None => return usage("--offset needs a count"),
                }
            }
            other if pages_dir.is_empty() => pages_dir = other.to_string(),
            other if text_dir.is_empty() => text_dir = other.to_string(),
            other => return usage(&format!("unexpected argument {other:?}")),
        }
        i += 1;
    }
    if pages_dir.is_empty() || text_dir.is_empty() {
        return usage("ident needs <pages-dir> <text-dir>");
    }

    let all_pgms = match list_pages(&pages_dir) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("paddle-score: {e}");
            return ExitCode::FAILURE;
        }
    };
    // `--stride`/`--offset` mirror `paddle_run.py`'s own `pgms[offset::stride]`
    // slice over the same sorted listing, so a subsampled driver run (a
    // corpus this large was subsampled for wall-clock reasons; see the
    // report) scores exactly the pages that were actually sent to the
    // engine, never counting an untested page as a silent miss.
    let pgms: Vec<_> = all_pgms.into_iter().skip(offset).step_by(stride.max(1)).collect();
    let decode = Params::DEFAULT.decode;

    let mut total = 0usize;
    let mut preserved = 0usize;
    let mut missing_pages = 0usize;
    let mut examples: Vec<(String, String)> = Vec::new();

    for pgm in &pgms {
        let stem = pgm.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let truth = match load_truth_beside(pgm) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("paddle-score: {stem}: {e}");
                continue;
            }
        };
        let (read, found) = read_paddle_text(&text_dir, &stem);
        if !found {
            missing_pages += 1;
        }
        let hyp_tokens: HashSet<&str> = read.split_whitespace().collect();

        for line in &truth.lines {
            for word in line.split_whitespace() {
                let trimmed = word.trim_matches(|c: char| ",.;:()".contains(c));
                if trimmed.is_empty() || !is_identifier_shaped(trimmed, &decode) {
                    continue;
                }
                total += 1;
                if hyp_tokens.contains(trimmed) {
                    preserved += 1;
                } else if examples.len() < 20 {
                    examples.push((stem.clone(), trimmed.to_string()));
                }
            }
        }
    }

    println!(
        "pages   {} of pages-dir (stride {stride}, offset {offset}) in {pages_dir}, text from {text_dir}",
        pgms.len()
    );
    if missing_pages > 0 {
        println!("WARNING  {missing_pages} pages had no matching <stem>.txt (scored as empty read)");
    }
    println!(
        "identifier-shaped tokens in truth: {total}, verbatim-preserved (page-level, order-free): {preserved} ({:.3}%)",
        100.0 * preserved as f64 / total.max(1) as f64
    );
    println!("\nsample not-preserved (up to 20, stem + truth token; the read text may still be near-miss, not necessarily dropped):");
    for (stem, tok) in &examples {
        println!("  {stem}: {tok:?}");
    }

    ExitCode::SUCCESS
}
