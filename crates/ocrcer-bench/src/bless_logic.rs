//! Planning and writing for the `bless` binary. Kept separate from
//! `bin/bless.rs` so the planning logic is unit-testable as a library and
//! the binary is a thin CLI shell.
//!
//! Blessing is deliberately awkward: a dry run is the default, writing
//! requires an explicit `--yes`, and a plan touching more than one stage
//! boundary refuses outright and names `ocrcer-architect`, per
//! `ARCHITECTURE.md` section 8.2 and `PLAN.md` section 4's "fixtures get
//! blessed to make a test pass" risk. `cargo test` never calls anything in
//! this module — only `bin/bless.rs` does.

use crate::{compare, featurefile, fixture, meta, pbm, stage};
use std::collections::BTreeSet;
use std::path::Path;

pub enum ChangeKind {
    /// No expectation file exists yet for this fixture.
    New,
    /// An expectation exists and disagrees with the current stage output.
    Changed { differing: usize, total: usize, first_index: usize },
    /// The same, for a stage whose output is not a vector of floats: the
    /// difference is described in the stage's own terms rather than as an
    /// index, because "expected 'payments', got 'payMents'" is a thing a
    /// reviewer can check against the fixture and "3 values differ" is not.
    ChangedDescribed { what: String },
    /// Expectation already matches; nothing to do.
    Unchanged,
    /// The stage itself could not be run (e.g. bad fixture input, or the
    /// extractor unavailable) — blessing cannot proceed for this fixture.
    Error(String),
}

pub struct FixtureChange {
    pub name: String,
    pub stage: &'static str,
    pub kind: ChangeKind,
    pub expected_path: std::path::PathBuf,
    pub payload: Payload,
}

/// What a blessing would write. One variant per stage boundary, so adding a
/// stage cannot accidentally reuse another stage's file format.
pub enum Payload {
    Feature(Vec<f32>),
    Decode(crate::decodefile::DecodeExpectation),
    /// Nothing to write — the fixture errored, or is unchanged.
    None,
}

pub struct BlessPlan {
    pub changes: Vec<FixtureChange>,
}

impl BlessPlan {
    pub fn stages_touched(&self) -> BTreeSet<&'static str> {
        self.changes
            .iter()
            .filter(|c| {
                matches!(
                    c.kind,
                    ChangeKind::New | ChangeKind::Changed { .. } | ChangeKind::ChangedDescribed { .. }
                )
            })
            .map(|c| c.stage)
            .collect()
    }

    pub fn to_write(&self) -> Vec<&FixtureChange> {
        self.changes
            .iter()
            .filter(|c| {
                matches!(
                    c.kind,
                    ChangeKind::New | ChangeKind::Changed { .. } | ChangeKind::ChangedDescribed { .. }
                )
            })
            .collect()
    }

    pub fn errors(&self) -> Vec<&FixtureChange> {
        self.changes
            .iter()
            .filter(|c| matches!(c.kind, ChangeKind::Error(_)))
            .collect()
    }

    /// Human-readable diff, printed both for a dry run and before a write.
    pub fn print_diff(&self) {
        for c in &self.changes {
            match &c.kind {
                // A NEW fixture prints what would be written, not just that
                // something would be. A blessing is only reviewable if the
                // reviewer can see the answer next to the fixture that
                // produced it, and for a new one there is no "before" to
                // diff against.
                ChangeKind::New => match &c.payload {
                    Payload::Decode(e) => println!(
                        "  NEW      {} (stage={}) — would write {:?} (identifier {}, score {})",
                        c.name, c.stage, e.text, e.identifier, e.score
                    ),
                    _ => println!(
                        "  NEW      {} (stage={}) — no expectation yet, would write {}",
                        c.name,
                        c.stage,
                        c.expected_path.display()
                    ),
                },
                ChangeKind::Changed { differing, total, first_index } => println!(
                    "  CHANGED  {} (stage={}) — {differing}/{total} values differ, first at index {first_index}",
                    c.name, c.stage
                ),
                ChangeKind::ChangedDescribed { what } => {
                    println!("  CHANGED  {} (stage={}) — {what}", c.name, c.stage)
                }
                ChangeKind::Unchanged => {}
                ChangeKind::Error(e) => println!("  ERROR    {} (stage={}) — {e}", c.name, c.stage),
            }
        }
        let to_write = self.to_write().len();
        let stages = self.stages_touched().len();
        println!("---");
        println!("{to_write} fixture(s) would change, across {stages} stage(s)");
    }

    pub fn write_all(&self) -> Result<usize, String> {
        let mut n = 0;
        for c in self.to_write() {
            match &c.payload {
                Payload::Feature(values) => {
                    featurefile::write(&c.expected_path, &c.name, c.stage, values)
                        .map_err(|e| e.to_string())?;
                }
                Payload::Decode(e) => {
                    if let Some(d) = c.expected_path.parent() {
                        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&c.expected_path, crate::decodefile::write_expectation(e))
                        .map_err(|e| e.to_string())?;
                }
                Payload::None => continue,
            }
            n += 1;
        }
        Ok(n)
    }
}

pub fn plan_glyph_stage(fixtures_root: &Path) -> Result<BlessPlan, String> {
    let fixtures = fixture::discover_glyph_fixtures(fixtures_root)?;
    let mut changes = Vec::new();

    for fx in fixtures {
        let bmp = match pbm::read(&fx.pbm_path) {
            Ok(b) => b,
            Err(e) => {
                changes.push(FixtureChange {
                    name: fx.name,
                    stage: "feature",
                    kind: ChangeKind::Error(e.to_string()),
                    expected_path: fx.expected_path,
                    payload: Payload::None,
                });
                continue;
            }
        };
        let m = match meta::read(&fx.meta_path) {
            Ok(m) => m,
            Err(e) => {
                changes.push(FixtureChange {
                    name: fx.name,
                    stage: "feature",
                    kind: ChangeKind::Error(e),
                    expected_path: fx.expected_path,
                    payload: Payload::None,
                });
                continue;
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
                changes.push(FixtureChange {
                    name: fx.name,
                    stage: "feature",
                    kind: ChangeKind::Error(e),
                    expected_path: fx.expected_path,
                    payload: Payload::None,
                });
                continue;
            }
        };

        let kind = if !fx.expected_path.exists() {
            ChangeKind::New
        } else {
            match featurefile::read(&fx.expected_path) {
                Ok(existing) => match compare::diff_positions(&existing.values, &actual) {
                    Some(positions) if positions.is_empty() => ChangeKind::Unchanged,
                    Some(positions) => ChangeKind::Changed {
                        differing: positions.len(),
                        total: actual.len(),
                        first_index: positions[0],
                    },
                    None => ChangeKind::Changed {
                        differing: actual.len().max(existing.values.len()),
                        total: actual.len(),
                        first_index: 0,
                    },
                },
                Err(e) => ChangeKind::Error(format!(
                    "existing expectation is unreadable ({e}); refusing to silently overwrite — \
                     inspect or delete it manually first"
                )),
            }
        };

        changes.push(FixtureChange {
            name: fx.name,
            stage: "feature",
            kind,
            expected_path: fx.expected_path,
            payload: Payload::Feature(actual),
        });
    }

    Ok(BlessPlan { changes })
}

/// Plans the decode stage: runs every authored lattice and compares its
/// reading against the checked-in expectation.
///
/// A decode expectation is three small fields, not a vector of floats, so the
/// diff is printed as the reading itself. That matters more here than at the
/// feature stage: a reviewer can decide whether `payments` or `payMents` is
/// right by reading the lattice's distances and the sentence in its `why`,
/// which is exactly the check `ARCHITECTURE.md` section 8.2 asks for and an
/// index into a float array cannot support.
pub fn plan_decode_stage(fixtures_root: &Path) -> Result<BlessPlan, String> {
    let names = crate::decodefile::discover(fixtures_root)?;
    let mut changes = Vec::new();
    for name in names {
        let in_path = fixtures_root.join("decode").join(format!("{name}.lattice.json"));
        let expected_path = fixtures_root
            .join("expected")
            .join("decode")
            .join(format!("{name}.decode.json"));

        let err = |e: String| FixtureChange {
            name: name.clone(),
            stage: "decode",
            kind: ChangeKind::Error(e),
            expected_path: expected_path.clone(),
            payload: Payload::None,
        };

        let input = match std::fs::read_to_string(&in_path)
            .map_err(|e| format!("{}: {e}", in_path.display()))
            .and_then(|t| crate::decodefile::parse_input(&t).map_err(|e| format!("{}: {e}", in_path.display())))
        {
            Ok(i) => i,
            Err(e) => {
                changes.push(err(e));
                continue;
            }
        };
        let actual = match crate::decodefile::run(&input, &name) {
            Ok(a) => a,
            Err(e) => {
                changes.push(err(e));
                continue;
            }
        };

        let kind = match std::fs::read_to_string(&expected_path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ChangeKind::New,
            Err(e) => ChangeKind::Error(format!("{}: {e}", expected_path.display())),
            Ok(t) => match crate::decodefile::parse_expectation(&t) {
                Err(e) => ChangeKind::Error(format!("{}: {e}", expected_path.display())),
                Ok(want) => {
                    let same = want.text == actual.text
                        && want.identifier == actual.identifier
                        && want.score.parse::<f64>().map(f64::to_bits)
                            == actual.score.parse::<f64>().map(f64::to_bits);
                    if same {
                        ChangeKind::Unchanged
                    } else {
                        ChangeKind::ChangedDescribed {
                            what: format!(
                                "expected {:?} (identifier {}, score {}), got {:?} (identifier {}, score {})",
                                want.text, want.identifier, want.score,
                                actual.text, actual.identifier, actual.score
                            ),
                        }
                    }
                }
            },
        };

        changes.push(FixtureChange {
            name,
            stage: "decode",
            kind,
            expected_path,
            payload: Payload::Decode(actual),
        });
    }
    Ok(BlessPlan { changes })
}
