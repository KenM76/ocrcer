//! `bless`: regenerates golden-fixture expectations. Deliberately awkward
//! — see `ARCHITECTURE.md` section 8.2 and `PLAN.md` section 4. Never
//! reachable from `cargo test`; only this binary calls
//! `ocrcer_bench::bless_logic`.
//!
//! Usage:
//!   cargo run -p ocrcer-bench --bin bless                    # dry run: prints the diff, writes nothing
//!   cargo run -p ocrcer-bench --bin bless -- --yes            # writes, if the plan touches exactly one stage
//!   cargo run -p ocrcer-bench --bin bless -- --yes --allow-multi-stage   # writes across stage boundaries too
//!
//! A plan spanning more than one stage boundary refuses even with `--yes`
//! unless `--allow-multi-stage` is also given, and prints a pointer at
//! `ocrcer-architect` — that span is exactly the shape of change
//! `ARCHITECTURE.md` 8.2 says must go through review as a deliberate act,
//! not get rubber-stamped by whoever happened to be blessing a typo fix.

use ocrcer_bench::bless_logic;
use std::path::PathBuf;
use std::process::ExitCode;

struct Args {
    fixtures_dir: PathBuf,
    yes: bool,
    allow_multi_stage: bool,
}

fn parse_args(raw: &[String]) -> Args {
    let mut fixtures_dir = ocrcer_bench::default_fixtures_root();
    let mut yes = false;
    let mut allow_multi_stage = false;
    let mut it = raw.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--yes" => yes = true,
            "--allow-multi-stage" => allow_multi_stage = true,
            "--fixtures-dir" => {
                if let Some(v) = it.next() {
                    fixtures_dir = PathBuf::from(v);
                }
            }
            other => eprintln!("bless: ignoring unrecognised argument {other:?}"),
        }
    }
    Args { fixtures_dir, yes, allow_multi_stage }
}

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&raw);

    println!("bless: planning against fixtures under {}", args.fixtures_dir.display());

    // Both stages are planned into one plan on purpose. The multi-stage gate
    // below is the whole point of this tool, and it can only count stages it
    // was shown — planning them separately would let a change that moves the
    // feature vector *and* the decoder's reading through as two single-stage
    // blessings.
    let mut plan = match bless_logic::plan_glyph_stage(&args.fixtures_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("bless: could not plan the glyph stage: {e}");
            return ExitCode::FAILURE;
        }
    };
    match bless_logic::plan_decode_stage(&args.fixtures_dir) {
        Ok(p) => plan.changes.extend(p.changes),
        Err(e) => {
            eprintln!("bless: could not plan the decode stage: {e}");
            return ExitCode::FAILURE;
        }
    }

    plan.print_diff();

    if !plan.errors().is_empty() {
        eprintln!(
            "bless: {} fixture(s) errored and were excluded from writing — fix them and re-run",
            plan.errors().len()
        );
    }

    let to_write = plan.to_write().len();
    if to_write == 0 {
        println!("bless: nothing to bless.");
        return ExitCode::SUCCESS;
    }

    let stages_touched = plan.stages_touched();
    if stages_touched.len() > 1 && !args.allow_multi_stage {
        eprintln!(
            "bless: this plan touches {} stage boundaries ({}), not one. Refusing by default — \
             a change this wide needs review by ocrcer-architect (per ARCHITECTURE.md section 8.2 \
             and PLAN.md section 4's \"fixtures get blessed to make a test pass\" risk), not a \
             single blessing command. Pass --allow-multi-stage only once that review has happened.",
            stages_touched.len(),
            stages_touched.into_iter().collect::<Vec<_>>().join(", ")
        );
        return ExitCode::FAILURE;
    }

    if !args.yes {
        println!(
            "bless: dry run only — {to_write} fixture(s) would be written. Re-run with --yes to \
             apply. Read the diff above before you do; a fixture blessed without being read is \
             exactly the failure mode this command's friction exists to prevent."
        );
        return ExitCode::SUCCESS;
    }

    match plan.write_all() {
        Ok(n) => {
            println!(
                "bless: wrote {n} fixture(s) across {} stage(s).",
                plan.stages_touched().len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("bless: write failed: {e}");
            ExitCode::FAILURE
        }
    }
}
