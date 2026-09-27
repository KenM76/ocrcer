//! `w1_workcounts`: deterministic per-page work counts (lattice edges,
//! matcher/classifier calls) summed over a fixed page list, for a control
//! vs. candidate comparison under an explicit `--set` override.
//!
//! One-off probe for the W1 paired-timing measurement
//! (`docs/measurements/2026-09-27_W1.md` addendum). Reuses the existing
//! `ocrcer_core::prof` counters (`edges`, `match_calls`) that
//! `profile_page.rs` already reads -- no new counter, no change to
//! `ocrcer-core`'s hot paths. `profile_page` prints a per-page timing
//! breakdown one page at a time; this sums the two work counts across a
//! whole page list under one `--set` configuration, which is what a
//! control-vs-candidate comparison needs and `profile_page` does not do.
//!
//! ```text
//! OCRCER_PROFILE=1 cargo run --release -p ocrcer-bench --bin w1_workcounts -- \
//!     model/out/ocrcer.ocrw --set words.short_split_x_heights=0 page1.pgm page2.pgm ...
//! ```
//!
//! `OCRCER_PROFILE` must be set before the process starts (`ocrcer_core::prof`
//! reads it once). `--set <name>=<value>` is optional and may repeat, same
//! syntax as `ocr --set`.

use std::process::ExitCode;

use ocrcer_bench::pages::load_page;
use ocrcer_core::pipeline::Engine;
use ocrcer_core::prof;
use ocrcer_core::Gray;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut model: Option<String> = None;
    let mut set: Vec<(String, f32)> = Vec::new();
    let mut pages: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--set" => {
                i += 1;
                match args.get(i).and_then(|v| v.split_once('=')).and_then(|(n, v)| {
                    v.trim().parse::<f32>().ok().map(|v| (n.to_string(), v))
                }) {
                    Some(nv) => set.push(nv),
                    None => {
                        eprintln!("w1_workcounts: --set needs <name>=<value>");
                        return ExitCode::FAILURE;
                    }
                }
            }
            other if model.is_none() => model = Some(other.to_string()),
            other => pages.push(other.to_string()),
        }
        i += 1;
    }
    let Some(model) = model else {
        eprintln!(
            "usage: w1_workcounts <model.ocrw> [--set <name>=<value> ...] <page.pgm> [more.pgm ...]"
        );
        return ExitCode::FAILURE;
    };
    if pages.is_empty() {
        eprintln!("w1_workcounts: no pages given");
        return ExitCode::FAILURE;
    }
    if !prof::enabled() {
        eprintln!("w1_workcounts: OCRCER_PROFILE=1 is not set; counters will all read zero");
    }

    let bytes = match std::fs::read(&model) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("w1_workcounts: {model}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut engine = match Engine::from_bytes(&bytes) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("w1_workcounts: {model}: {e}");
            return ExitCode::FAILURE;
        }
    };
    for (name, v) in &set {
        if !ocrcer_bench::knobs::set(&mut engine, name, *v) {
            eprintln!("w1_workcounts: unknown or rejected override {name}={v}");
            return ExitCode::FAILURE;
        }
        println!("set     {name} = {v}  (an override, not what the file carries)");
    }

    let mut total_edges = 0u64;
    let mut total_match_calls = 0u64;
    let mut total_words = 0u64;
    let mut n_pages = 0usize;

    for page_path in &pages {
        let (w, h, grey) = match load_page(std::path::Path::new(page_path)) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("w1_workcounts: {page_path}: {e}");
                continue;
            }
        };
        prof::reset();
        let _ = engine.recognize_lines(Gray { width: w, height: h, data: &grey });
        let c = &prof::COUNTERS;
        total_edges += prof::get(&c.edges);
        total_match_calls += prof::get(&c.match_calls);
        total_words += prof::get(&c.words);
        n_pages += 1;
    }

    println!(
        "pages {n_pages}  edges {total_edges}  match_calls {total_match_calls}  words {total_words}"
    );
    println!(
        "per-page mean: edges {:.1}  match_calls {:.1}  words {:.1}",
        total_edges as f64 / n_pages.max(1) as f64,
        total_match_calls as f64 / n_pages.max(1) as f64,
        total_words as f64 / n_pages.max(1) as f64,
    );
    ExitCode::SUCCESS
}
