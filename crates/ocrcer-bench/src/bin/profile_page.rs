//! `profile_page`: prints `ocrcer-core`'s stage-timing counters for one or
//! more pages, one row per page. Bench-only driver for the counters in
//! `ocrcer_core::prof` -- it does not exist to be run by pdfcer or by any
//! fixture, only by a human chasing a slow page.
//!
//! ```text
//! OCRCER_PROFILE=1 cargo run -p ocrcer-bench --release --bin profile_page \
//!     -- model/out/ocrcer.ocrw page1.pgm [page2.pgm ...]
//! ```
//!
//! `OCRCER_PROFILE` must be set before the process starts: `ocrcer_core::prof`
//! reads it once, on the first call to any stage in the run.

use std::process::ExitCode;
use std::time::Instant;

use ocrcer_bench::pages::load_page;
use ocrcer_core::pipeline::Engine;
use ocrcer_core::prof;
use ocrcer_core::Gray;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [model_path, page_paths @ ..] = args.as_slice() else {
        eprintln!("usage: profile_page <model.ocrw> <page.pgm> [more.pgm ...]");
        return ExitCode::FAILURE;
    };
    if page_paths.is_empty() {
        eprintln!("usage: profile_page <model.ocrw> <page.pgm> [more.pgm ...]");
        return ExitCode::FAILURE;
    }
    if !prof::enabled() {
        eprintln!("profile_page: OCRCER_PROFILE=1 is not set; counters will all read zero");
    }
    let bytes = match std::fs::read(model_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("profile_page: {model_path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let engine = match Engine::from_bytes(&bytes) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("profile_page: {model_path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    for page_path in page_paths {
        let (w, h, grey) = match load_page(std::path::Path::new(page_path)) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("profile_page: {page_path}: {e}");
                continue;
            }
        };
        prof::reset();
        let t0 = Instant::now();
        let lines = engine.recognize_lines(Gray { width: w, height: h, data: &grey });
        let wall = t0.elapsed();
        let n_words: usize = lines.as_ref().map(|l| l.iter().map(|l| l.words.len()).sum()).unwrap_or(0);

        let c = &prof::COUNTERS;
        let binarize_layout_ms = prof::get(&c.binarize_layout_ns) as f64 / 1e6;
        let segment_ms = prof::get(&c.segment_ns) as f64 / 1e6;
        let extract_ms = prof::get(&c.extract_ns) as f64 / 1e6;
        let match_ms = prof::get(&c.match_ns) as f64 / 1e6;
        let decode_ms = prof::get(&c.decode_ns) as f64 / 1e6;
        let accounted_ms = binarize_layout_ms + segment_ms + extract_ms + match_ms + decode_ms;
        let words = prof::get(&c.words);
        let edges = prof::get(&c.edges);
        let match_calls = prof::get(&c.match_calls);
        let visited = prof::get(&c.prototypes_visited);
        let abandoned = prof::get(&c.prototypes_abandoned);

        println!("== {page_path} ==");
        println!("wall: {:.1} ms  ({} words recognised)", wall.as_secs_f64() * 1e3, n_words);
        println!(
            "  binarize/layout: {binarize_layout_ms:8.1} ms ({:5.1}%)",
            pct(binarize_layout_ms, accounted_ms)
        );
        println!("  segment (lattice): {segment_ms:8.1} ms ({:5.1}%)", pct(segment_ms, accounted_ms));
        println!("  extract (features): {extract_ms:8.1} ms ({:5.1}%)", pct(extract_ms, accounted_ms));
        println!("  match (nearest):  {match_ms:8.1} ms ({:5.1}%)", pct(match_ms, accounted_ms));
        println!("  decode (viterbi): {decode_ms:8.1} ms ({:5.1}%)", pct(decode_ms, accounted_ms));
        println!("  accounted total:  {accounted_ms:8.1} ms");
        println!(
            "  words: {words}  edges: {edges}  edges/word: {:.2}",
            edges as f64 / words.max(1) as f64
        );
        println!(
            "  nearest() calls: {match_calls}  prototypes visited: {visited}  abandoned: {abandoned} ({:.1}%)",
            pct(abandoned as f64, visited as f64)
        );
        println!(
            "  prototypes visited / nearest() call: {:.1}",
            visited as f64 / match_calls.max(1) as f64
        );
    }
    ExitCode::SUCCESS
}

fn pct(part: f64, whole: f64) -> f64 {
    if whole > 0.0 {
        100.0 * part / whole
    } else {
        0.0
    }
}
