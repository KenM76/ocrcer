//! Exact, diagnostic comparison. No tolerance anywhere — see
//! `ARCHITECTURE.md` section 3.1 and 8.2: the extractor has no
//! transcendental function, so its output is bit-identical on x86 and
//! wasm32, and a fixture is allowed to demand bit equality.
//!
//! Equality is `f32::to_bits()`, not `==`, so `-0.0` and `0.0` — equal
//! under `==`, distinct bit patterns — are correctly told apart.

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail { message: String },
}

impl Outcome {
    pub fn is_pass(&self) -> bool {
        matches!(self, Outcome::Pass)
    }
}

/// Compare an actual feature vector against its checked-in expectation.
/// On any disagreement, the returned message names the fixture, the
/// stage, which dimension disagreed (or the length, if they differ), and
/// both values — never just "fixtures differ".
pub fn compare_feature_vectors(
    fixture: &str,
    stage: &str,
    expected: &[f32],
    actual: &[f32],
) -> Outcome {
    if expected.len() != actual.len() {
        return Outcome::Fail {
            message: format!(
                "FAIL {fixture} (stage={stage}): dims mismatch — expected {} values, actual {}",
                expected.len(),
                actual.len()
            ),
        };
    }
    for (i, (e, a)) in expected.iter().zip(actual.iter()).enumerate() {
        if e.to_bits() != a.to_bits() {
            return Outcome::Fail {
                message: format!(
                    "FAIL {fixture} (stage={stage}): value mismatch at index {i} of {} — \
                     expected {e} (bits=0x{:08x}), actual {a} (bits=0x{:08x})",
                    expected.len(),
                    e.to_bits(),
                    a.to_bits()
                ),
            };
        }
    }
    Outcome::Pass
}

/// All indices where two same-length vectors' bit patterns differ. Used by
/// `bless` to report a diff stat ("N/107 values differ") without
/// duplicating the exact-comparison logic above. Returns `None` if the
/// lengths differ (that is a dims mismatch, reported separately).
pub fn diff_positions(expected: &[f32], actual: &[f32]) -> Option<Vec<usize>> {
    if expected.len() != actual.len() {
        return None;
    }
    Some(
        expected
            .iter()
            .zip(actual.iter())
            .enumerate()
            .filter(|(_, (e, a))| e.to_bits() != a.to_bits())
            .map(|(i, _)| i)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_vectors_pass() {
        let v = vec![1.0_f32, 2.0, 3.0];
        assert_eq!(compare_feature_vectors("f", "feature", &v, &v), Outcome::Pass);
    }

    #[test]
    fn length_mismatch_names_both_lengths() {
        let expected = vec![1.0_f32, 2.0, 3.0];
        let actual = vec![1.0_f32, 2.0];
        let Outcome::Fail { message } = compare_feature_vectors("f", "feature", &expected, &actual) else {
            panic!("expected Fail");
        };
        assert!(message.contains("dims mismatch"));
        assert!(message.contains("expected 3"));
        assert!(message.contains("actual 2"));
        assert!(message.contains('f'));
    }

    #[test]
    fn first_differing_index_is_reported() {
        let expected = vec![1.0_f32, 2.0, 3.0, 4.0];
        let actual = vec![1.0_f32, 2.0, 3.5, 4.0];
        let Outcome::Fail { message } = compare_feature_vectors("g", "feature", &expected, &actual) else {
            panic!("expected Fail");
        };
        assert!(message.contains("index 2"));
        assert!(message.contains('3'));
    }

    #[test]
    fn negative_zero_and_zero_are_distinct_bit_patterns() {
        let expected = vec![0.0_f32];
        let actual = vec![-0.0_f32];
        assert!(!compare_feature_vectors("z", "feature", &expected, &actual).is_pass());
    }

    #[test]
    fn one_ulp_difference_fails() {
        let base = 1.0_f32;
        let bumped = f32::from_bits(base.to_bits() + 1);
        let expected = vec![base];
        let actual = vec![bumped];
        assert!(!compare_feature_vectors("u", "feature", &expected, &actual).is_pass());
    }
}
