//! Loads every fixture, runs its stage, compares against the checked-in
//! expectation, reports. `run_glyph_stage` is what both the `ocrcer-bench`
//! binary and the perturbation self-tests call — the binary just adds
//! stdout formatting and a process exit code.

use crate::{compare, featurefile, fixture, meta, pbm, stage};
use std::path::Path;

pub struct FixtureResult {
    pub name: String,
    pub stage: &'static str,
    pub outcome: compare::Outcome,
}

pub struct RunReport {
    pub results: Vec<FixtureResult>,
}

impl RunReport {
    pub fn all_passed(&self) -> bool {
        !self.results.is_empty() && self.results.iter().all(|r| r.outcome.is_pass())
    }

    /// The `Fail` messages, in fixture order — what a test asserts against.
    pub fn failure_messages(&self) -> Vec<&str> {
        self.results
            .iter()
            .filter_map(|r| match &r.outcome {
                compare::Outcome::Fail { message } => Some(message.as_str()),
                compare::Outcome::Pass => None,
            })
            .collect()
    }

    pub fn print(&self) {
        for r in &self.results {
            match &r.outcome {
                compare::Outcome::Pass => println!("PASS {} (stage={})", r.name, r.stage),
                compare::Outcome::Fail { message } => println!("{message}"),
            }
        }
        let passed = self.results.iter().filter(|r| r.outcome.is_pass()).count();
        let total = self.results.len();
        println!("---");
        println!("{passed}/{total} fixtures passed");
    }
}

fn run_one_glyph_fixture(fx: &fixture::GlyphFixture) -> compare::Outcome {
    let bmp = match pbm::read(&fx.pbm_path) {
        Ok(b) => b,
        Err(e) => {
            return compare::Outcome::Fail {
                message: format!("FAIL {} (stage=feature): {e}", fx.name),
            }
        }
    };
    let m = match meta::read(&fx.meta_path) {
        Ok(m) => m,
        Err(e) => {
            return compare::Outcome::Fail {
                message: format!("FAIL {} (stage=feature): {e}", fx.name),
            }
        }
    };

    let input = stage::GlyphStageInput {
        ink: bmp.ink,
        width: bmp.width,
        height: bmp.height,
        baseline_dy: m.baseline_dy,
        x_height: m.x_height,
    };
    let actual = match stage::run_feature_stage(&input) {
        Ok(v) => v,
        Err(e) => {
            return compare::Outcome::Fail {
                message: format!("FAIL {} (stage=feature): {e}", fx.name),
            }
        }
    };

    if !fx.expected_path.exists() {
        return compare::Outcome::Fail {
            message: format!(
                "FAIL {} (stage=feature): no expectation file at {} — run `bless` \
                 (deliberately, with --yes) to generate one, then read it before committing",
                fx.name,
                fx.expected_path.display()
            ),
        };
    }
    let expected = match featurefile::read(&fx.expected_path) {
        Ok(f) => f,
        Err(e) => {
            return compare::Outcome::Fail {
                message: format!("FAIL {} (stage=feature): {e}", fx.name),
            }
        }
    };

    compare::compare_feature_vectors(&fx.name, "feature", &expected.values, &actual)
}

pub fn run_glyph_stage(fixtures_root: &Path) -> RunReport {
    let fixtures = match fixture::discover_glyph_fixtures(fixtures_root) {
        Ok(f) => f,
        Err(e) => {
            return RunReport {
                results: vec![FixtureResult {
                    name: "<discovery>".to_string(),
                    stage: "feature",
                    outcome: compare::Outcome::Fail {
                        message: format!("FAIL <discovery> (stage=feature): {e}"),
                    },
                }],
            }
        }
    };

    let results = fixtures
        .iter()
        .map(|fx| FixtureResult {
            name: fx.name.clone(),
            stage: "feature",
            outcome: run_one_glyph_fixture(fx),
        })
        .collect();

    RunReport { results }
}

/// Runs every decode fixture: authored lattice in, decoder's reading compared
/// against the checked-in expectation.
///
/// A missing input and a missing expectation are both named failures rather
/// than absences, for the reason `fixtures/README.md` gives at the glyph
/// stage: a fixture that vanishes from the run when a file is deleted is a
/// fixture that can be disabled by accident.
pub fn run_decode_stage(fixtures_root: &Path) -> RunReport {
    let names = match crate::decodefile::discover(fixtures_root) {
        Ok(n) => n,
        Err(e) => {
            return RunReport {
                results: vec![FixtureResult {
                    name: "<discovery>".to_string(),
                    stage: "decode",
                    outcome: compare::Outcome::Fail {
                        message: format!("FAIL <discovery> (stage=decode): {e}"),
                    },
                }],
            }
        }
    };

    let results = names
        .iter()
        .map(|name| FixtureResult {
            name: name.clone(),
            stage: "decode",
            outcome: run_one_decode_fixture(fixtures_root, name),
        })
        .collect();

    RunReport { results }
}

fn run_one_decode_fixture(root: &Path, name: &str) -> compare::Outcome {
    let fail = |m: String| compare::Outcome::Fail { message: format!("FAIL {name} (stage=decode): {m}") };

    let in_path = root.join("decode").join(format!("{name}.lattice.json"));
    let exp_path = root.join("expected").join("decode").join(format!("{name}.decode.json"));

    let input = match std::fs::read_to_string(&in_path) {
        Ok(t) => match crate::decodefile::parse_input(&t) {
            Ok(i) => i,
            Err(e) => return fail(format!("{}: {e}", in_path.display())),
        },
        Err(e) => return fail(format!("{}: {e}", in_path.display())),
    };
    let expected = match std::fs::read_to_string(&exp_path) {
        Ok(t) => match crate::decodefile::parse_expectation(&t) {
            Ok(x) => x,
            Err(e) => return fail(format!("{}: {e}", exp_path.display())),
        },
        Err(e) => return fail(format!("{}: {e}", exp_path.display())),
    };
    let actual = match crate::decodefile::run(&input, name) {
        Ok(a) => a,
        Err(e) => return fail(e),
    };

    if actual.text != expected.text {
        return fail(format!(
            "text: expected {:?}, got {:?}\n  the fixture exists because: {}",
            expected.text, actual.text, input.why
        ));
    }
    if actual.identifier != expected.identifier {
        return fail(format!(
            "identifier: expected {}, got {}\n  the fixture exists because: {}",
            expected.identifier, actual.identifier, input.why
        ));
    }
    // By bits, not by `==`: the same reason `fixtures/README.md` gives for the
    // feature vectors. There is no tolerance here because the decoder
    // accumulates in `f64` over a fixed visit order, so a changed bit is a
    // changed decision path and not rounding.
    let want = expected.score.parse::<f64>().map(f64::to_bits);
    let got = actual.score.parse::<f64>().map(f64::to_bits);
    if want != got {
        return fail(format!(
            "score: expected {} ({:?}), got {} ({:?})\n  the fixture exists because: {}",
            expected.score, want, actual.score, got, input.why
        ));
    }
    compare::Outcome::Pass
}
