//! `ocrcer-bench`: runs every fixture's stage against its checked-in
//! expectation and reports. Exits non-zero on any mismatch (including a
//! missing expectation file — an unverified fixture is a failure state,
//! not a skip). See `fixtures/README.md`.
//!
//! Usage: `cargo run -p ocrcer-bench [-- --fixtures-dir PATH]`

use std::path::PathBuf;
use std::process::ExitCode;

fn parse_fixtures_dir(args: &[String]) -> PathBuf {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--fixtures-dir" {
            if let Some(v) = it.next() {
                return PathBuf::from(v);
            }
        }
    }
    ocrcer_bench::default_fixtures_root()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let fixtures_root = parse_fixtures_dir(&args);

    println!("ocrcer-bench: running fixtures under {}", fixtures_root.display());
    println!("--- stages: feature (fixtures/glyphs), decode (fixtures/decode) ---");
    let mut report = ocrcer_bench::runner::run_glyph_stage(&fixtures_root);
    report.results.extend(ocrcer_bench::runner::run_decode_stage(&fixtures_root).results);
    report.print();

    if report.all_passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
