//! Chunk 1's exit gate: the harness must fail loudly when a fixture is
//! deliberately altered, not silently pass or silently skip.
//!
//! Each test copies one known-good fixture (`glyph_l`, the simplest —
//! solid vertical stroke, no holes) into a fresh temp directory that
//! mirrors `fixtures/`'s layout, applies one perturbation, runs
//! `ocrcer_bench::runner::run_glyph_stage` against the copy, and asserts
//! both that it failed and that the failure message names the fixture and
//! the specific thing that went wrong. The checked-in fixtures under
//! `fixtures/` are never touched.

use std::fs;
use std::path::PathBuf;

const FIXTURE: &str = "glyph_l";

/// A private temp copy of `fixtures/` containing only `FIXTURE`'s three
/// files (bitmap, meta, expectation), so perturbing it can never affect
/// the checked-in corpus.
struct Sandbox {
    root: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let real_root = ocrcer_bench::default_fixtures_root();
        let root = tempfile::tempdir().expect("create temp dir");

        let glyphs_dir = root.path().join("glyphs");
        let expected_dir = root.path().join("expected").join("glyphs");
        fs::create_dir_all(&glyphs_dir).unwrap();
        fs::create_dir_all(&expected_dir).unwrap();

        fs::copy(
            real_root.join("glyphs").join(format!("{FIXTURE}.pbm")),
            glyphs_dir.join(format!("{FIXTURE}.pbm")),
        )
        .expect("copy .pbm fixture — run this test suite after fixtures/glyphs is seeded");
        fs::copy(
            real_root.join("glyphs").join(format!("{FIXTURE}.meta.json")),
            glyphs_dir.join(format!("{FIXTURE}.meta.json")),
        )
        .expect("copy .meta.json fixture");
        fs::copy(
            real_root
                .join("expected")
                .join("glyphs")
                .join(format!("{FIXTURE}.feature.json")),
            expected_dir.join(format!("{FIXTURE}.feature.json")),
        )
        .expect("copy .feature.json expectation — run `bless --yes` first if this is missing");

        Sandbox { root }
    }

    fn pbm_path(&self) -> PathBuf {
        self.root.path().join("glyphs").join(format!("{FIXTURE}.pbm"))
    }
    fn meta_path(&self) -> PathBuf {
        self.root
            .path()
            .join("glyphs")
            .join(format!("{FIXTURE}.meta.json"))
    }
    fn expected_path(&self) -> PathBuf {
        self.root
            .path()
            .join("expected")
            .join("glyphs")
            .join(format!("{FIXTURE}.feature.json"))
    }
    fn run(&self) -> ocrcer_bench::runner::RunReport {
        ocrcer_bench::runner::run_glyph_stage(self.root.path())
    }
}

/// The unperturbed sandbox must pass — otherwise every perturbation test
/// below would "pass" for the wrong reason (the harness always fails).
#[test]
fn baseline_sandbox_passes() {
    let sb = Sandbox::new();
    let report = sb.run();
    assert!(
        report.all_passed(),
        "unperturbed sandbox copy of {FIXTURE} did not pass: {:?}",
        report.failure_messages()
    );
}

#[test]
fn flipped_bitmap_bit_fails_loudly() {
    let sb = Sandbox::new();
    let bmp = ocrcer_bench::pbm::read(&sb.pbm_path()).unwrap();
    let mut ink = bmp.ink.clone();
    // Flip a pixel inside the stroke's bounding box, not a corner that a
    // degenerate extractor might ignore.
    let idx = (bmp.height as usize / 2) * bmp.width as usize + (bmp.width as usize / 2);
    ink[idx] = 1 - ink[idx];
    ocrcer_bench::pbm::write(
        &sb.pbm_path(),
        &ocrcer_bench::pbm::Bitmap { width: bmp.width, height: bmp.height, ink },
    )
    .unwrap();

    let report = sb.run();
    assert!(!report.all_passed(), "flipping a bitmap bit did not fail");
    let msgs = report.failure_messages();
    assert!(msgs.iter().any(|m| m.contains(FIXTURE)), "{msgs:?}");
    assert!(
        msgs.iter().any(|m| m.contains("value mismatch") || m.contains("dims mismatch")),
        "failure message did not name a specific field: {msgs:?}"
    );
}

#[test]
fn one_ulp_expectation_change_fails_loudly() {
    let sb = Sandbox::new();
    let existing = ocrcer_bench::featurefile::read(&sb.expected_path()).unwrap();
    let mut values = existing.values.clone();
    let bumped = f32::from_bits(values[0].to_bits() + 1);
    values[0] = bumped;
    ocrcer_bench::featurefile::write(&sb.expected_path(), &existing.fixture, &existing.stage, &values)
        .unwrap();

    let report = sb.run();
    assert!(!report.all_passed(), "a one-ulp expectation change did not fail");
    let msgs = report.failure_messages();
    assert!(msgs.iter().any(|m| m.contains(FIXTURE) && m.contains("index 0")), "{msgs:?}");
}

#[test]
fn truncated_vector_fails_loudly() {
    let sb = Sandbox::new();
    let existing = ocrcer_bench::featurefile::read(&sb.expected_path()).unwrap();
    let truncated = &existing.values[..existing.values.len() - 1];
    ocrcer_bench::featurefile::write(&sb.expected_path(), &existing.fixture, &existing.stage, truncated)
        .unwrap();

    let report = sb.run();
    assert!(!report.all_passed(), "a truncated expectation vector did not fail");
    let msgs = report.failure_messages();
    assert!(
        msgs.iter().any(|m| m.contains(FIXTURE) && m.contains("dims mismatch")),
        "{msgs:?}"
    );
}

#[test]
fn deleted_fixture_fails_loudly() {
    let sb = Sandbox::new();
    fs::remove_file(sb.expected_path()).unwrap();

    let report = sb.run();
    assert!(!report.all_passed(), "deleting the expectation file did not fail");
    let msgs = report.failure_messages();
    assert!(
        msgs.iter().any(|m| m.contains(FIXTURE) && m.contains("no expectation file")),
        "{msgs:?}"
    );
}

#[test]
fn deleted_bitmap_fails_loudly() {
    let sb = Sandbox::new();
    fs::remove_file(sb.pbm_path()).unwrap();

    let report = sb.run();
    assert!(!report.all_passed(), "deleting the bitmap file did not fail");
    let msgs = report.failure_messages();
    assert!(msgs.iter().any(|m| m.contains(FIXTURE)), "{msgs:?}");
}

#[test]
fn corrupted_expectation_json_fails_loudly() {
    let sb = Sandbox::new();
    fs::write(sb.expected_path(), "{ this is not valid json at all").unwrap();

    let report = sb.run();
    assert!(!report.all_passed(), "corrupt expectation JSON did not fail");
    let msgs = report.failure_messages();
    assert!(msgs.iter().any(|m| m.contains(FIXTURE)), "{msgs:?}");
}

#[test]
fn corrupted_meta_json_fails_loudly() {
    let sb = Sandbox::new();
    fs::write(sb.meta_path(), "{ not json").unwrap();

    let report = sb.run();
    assert!(!report.all_passed(), "corrupt meta JSON did not fail");
    let msgs = report.failure_messages();
    assert!(msgs.iter().any(|m| m.contains(FIXTURE)), "{msgs:?}");
}

/// Belt-and-braces: the checked-in `fixtures/` directory itself must never
/// be touched by any test above. If this fails, a test above has a bug.
#[test]
fn checked_in_fixtures_are_untouched_by_the_suite_above() {
    let real_root = ocrcer_bench::default_fixtures_root();
    let report = ocrcer_bench::runner::run_glyph_stage(&real_root);
    assert!(
        report.all_passed(),
        "checked-in fixtures under fixtures/ do not all pass — either they were mutated by a \
         test, or they were never blessed: {:?}",
        report.failure_messages()
    );
}

// ---------------------------------------------------------------------------
// The decode stage, held to the same gate.
//
// The point is the same one the glyph tests make: a harness that passes when
// the expectation has been altered is not checking anything. It matters more
// here, because a decode expectation is three short fields and a wrong one is
// easy to write by hand — which is exactly what the format forbids.
// ---------------------------------------------------------------------------

const DECODE_FIXTURE: &str = "case_anomaly_is_overturned_when_the_evidence_is_weak";

struct DecodeSandbox {
    root: tempfile::TempDir,
}

impl DecodeSandbox {
    fn new() -> Self {
        let real_root = ocrcer_bench::default_fixtures_root();
        let root = tempfile::tempdir().expect("create temp dir");
        let in_dir = root.path().join("decode");
        let exp_dir = root.path().join("expected").join("decode");
        fs::create_dir_all(&in_dir).unwrap();
        fs::create_dir_all(&exp_dir).unwrap();
        fs::copy(
            real_root.join("decode").join(format!("{DECODE_FIXTURE}.lattice.json")),
            in_dir.join(format!("{DECODE_FIXTURE}.lattice.json")),
        )
        .expect("copy .lattice.json fixture");
        fs::copy(
            real_root
                .join("expected")
                .join("decode")
                .join(format!("{DECODE_FIXTURE}.decode.json")),
            exp_dir.join(format!("{DECODE_FIXTURE}.decode.json")),
        )
        .expect("copy .decode.json expectation — run `bless --yes` first if this is missing");
        DecodeSandbox { root }
    }

    fn expected_path(&self) -> PathBuf {
        self.root
            .path()
            .join("expected")
            .join("decode")
            .join(format!("{DECODE_FIXTURE}.decode.json"))
    }
    fn lattice_path(&self) -> PathBuf {
        self.root.path().join("decode").join(format!("{DECODE_FIXTURE}.lattice.json"))
    }
    fn run(&self) -> ocrcer_bench::runner::RunReport {
        ocrcer_bench::runner::run_decode_stage(self.root.path())
    }
}

#[test]
fn baseline_decode_sandbox_passes() {
    let sb = DecodeSandbox::new();
    let report = sb.run();
    assert!(
        report.all_passed(),
        "unperturbed sandbox copy of {DECODE_FIXTURE} did not pass: {:?}",
        report.failure_messages()
    );
}

#[test]
fn a_changed_reading_fails_loudly() {
    let sb = DecodeSandbox::new();
    let t = fs::read_to_string(sb.expected_path()).unwrap();
    fs::write(sb.expected_path(), t.replace("\"payments\"", "\"payMents\"")).unwrap();
    let report = sb.run();
    assert!(!report.all_passed(), "a changed reading passed");
    let msgs = report.failure_messages();
    assert!(msgs.iter().any(|m| m.contains("text:")), "{msgs:?}");
    // The failure must carry the author's sentence, so the reviewer is told
    // what the fixture was for rather than left to reconstruct it.
    assert!(msgs.iter().any(|m| m.contains("the fixture exists because")), "{msgs:?}");
}

#[test]
fn a_one_ulp_score_change_fails_loudly() {
    let sb = DecodeSandbox::new();
    let t = fs::read_to_string(sb.expected_path()).unwrap();
    let want: f64 = t
        .split("\"score\": \"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .and_then(|v| v.parse().ok())
        .expect("expectation carries a parsable score");
    let nudged = f64::from_bits(want.to_bits() + 1);
    fs::write(
        sb.expected_path(),
        t.replace(&format!("{want}"), &format!("{nudged}")),
    )
    .unwrap();
    let report = sb.run();
    assert!(!report.all_passed(), "a one-ulp score change passed: there is no tolerance here");
    assert!(report.failure_messages().iter().any(|m| m.contains("score:")));
}

#[test]
fn a_deleted_expectation_is_a_named_failure_not_a_silent_skip() {
    let sb = DecodeSandbox::new();
    fs::remove_file(sb.expected_path()).unwrap();
    let report = sb.run();
    assert!(!report.all_passed(), "a deleted expectation passed");
    assert!(
        report.failure_messages().iter().any(|m| m.contains(DECODE_FIXTURE)),
        "the failure must name the fixture that lost its expectation"
    );
}

#[test]
fn a_lattice_without_its_why_fails_rather_than_running() {
    let sb = DecodeSandbox::new();
    let t = fs::read_to_string(sb.lattice_path()).unwrap();
    fs::write(sb.lattice_path(), t.replace("\"why\"", "\"note\"")).unwrap();
    let report = sb.run();
    assert!(!report.all_passed(), "a lattice with no stated purpose passed");
    assert!(report.failure_messages().iter().any(|m| m.contains("why")));
}
