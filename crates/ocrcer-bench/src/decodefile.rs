//! The decode-stage fixture format: a hand-authored word lattice in, the
//! decoder's chosen reading out.
//!
//! # Why this stage gets its own fixture
//!
//! `ARCHITECTURE.md` section 8.2 asks for an expectation at every stage
//! boundary, and until now the only one that had it was feature extraction.
//! The decoder is the stage whose parameters move most, and the one whose
//! determinism guarantees — fixed lattice visit order, ties broken by lowest
//! class index then earliest cut — have no other way of being checked: they
//! are invisible in an aggregate error rate, because a tie broken the other
//! way is wrong on about half of its occurrences and right on the rest.
//!
//! # Why the lattice is authored rather than captured
//!
//! A lattice captured from a real page would make this fixture a record of
//! what the matcher happened to return on the day it was captured, and every
//! bank rebuild would demand a re-bless of a file nobody could check by
//! reading. An authored lattice has known ground truth in the sense section
//! 8.2 requires: the distances say plainly which reading the image favoured,
//! so the right answer can be worked out from the fixture rather than from a
//! previous run.
//!
//! By the same reasoning the decoder is run with **no authored tables** —
//! no bigrams, no lexicon, no confusions — unless a fixture asks for them.
//! Those live in the model file and change when it is rebuilt; a fixture that
//! depended on them would fail for reasons that have nothing to do with the
//! decoder.
//!
//! # File format
//!
//! `fixtures/decode/<name>.lattice.json`, ordinary `serde_json`, authored by
//! hand:
//!
//! ```json
//! {
//!   "why": "payMents costs one case transition and payments costs none",
//!   "params": { "decode.case_shape_penalty": 3.44 },
//!   "positions": [
//!     [["p", 1.0], ["P", 1.0]],
//!     [["a", 1.0]]
//!   ]
//! }
//! ```
//!
//! `positions` is a linear chain: one lattice node per boundary, one edge per
//! position, and every candidate on that edge listed as `[character,
//! distance]`. Lower distance is a better match, exactly as the matcher
//! reports it. Two candidates at the *same* distance is the interesting case
//! and is how a tie-break expectation is written.
//!
//! A chain cannot express a merge or a split, which is deliberate: those
//! belong to the segmenter's own stage boundary, and mixing them in here
//! would make a decode fixture fail for a segmentation reason.
//!
//! `params` overrides `ocrcer_core::params::Decode::DEFAULT` by the same key
//! names `model/params.tsv` uses, so a fixture states the parameters it was
//! authored against instead of inheriting whatever the file happens to carry
//! this week.
//!
//! # Expectation format
//!
//! `fixtures/expected/decode/<name>.decode.json`:
//!
//! ```json
//! { "fixture": "...", "stage": "decode", "text": "payments",
//!   "identifier": false, "score": "12.5" }
//! ```
//!
//! `score` is a **string** holding Rust's `{}` formatting of the `f64`, for
//! the reason `fixtures/README.md` gives for the feature vectors: JSON's own
//! number path is not contractually pinned to round-trip a float bit for bit,
//! and a formatting drift there would be indistinguishable from a real
//! regression. Comparison is by `f64::to_bits`, with no tolerance.

use std::collections::BTreeMap;
use std::path::Path;

use ocrcer_core::decode::viterbi::{Cand, ClassInfo, Hyp, Tables, WordLattice};
use ocrcer_core::layout::segment::EdgeKind;
use ocrcer_core::params::Params;

/// One authored lattice fixture, as read off disk.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeInput {
    /// Why the fixture exists. Carried so a failure can print the sentence
    /// the author wrote, rather than leaving the next reader to guess what
    /// the numbers were chosen to prove.
    pub why: String,
    pub params: BTreeMap<String, f32>,
    /// One entry per character position; each is `(character, distance)`.
    pub positions: Vec<Vec<(char, f32)>>,
}

/// What the decoder produced, as written to disk.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodeExpectation {
    pub fixture: String,
    pub text: String,
    pub identifier: bool,
    /// Rust `{}` formatting of the path score.
    pub score: String,
}

/// Parses a `.lattice.json`.
pub fn parse_input(text: &str) -> Result<DecodeInput, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let obj = v.as_object().ok_or("lattice file is not a JSON object")?;

    let why = obj
        .get("why")
        .and_then(|w| w.as_str())
        .ok_or("lattice file needs a \"why\": a fixture nobody can read the point of is not a fixture")?
        .to_string();

    let mut params = BTreeMap::new();
    if let Some(p) = obj.get("params") {
        let p = p.as_object().ok_or("\"params\" is not an object")?;
        for (k, val) in p {
            let f = val.as_f64().ok_or_else(|| format!("params.{k} is not a number"))?;
            params.insert(k.clone(), f as f32);
        }
    }

    let positions_v = obj
        .get("positions")
        .and_then(|p| p.as_array())
        .ok_or("lattice file needs a \"positions\" array")?;
    if positions_v.is_empty() {
        return Err("\"positions\" is empty: there is nothing to decode".into());
    }
    let mut positions = Vec::with_capacity(positions_v.len());
    for (i, pos) in positions_v.iter().enumerate() {
        let cands = pos
            .as_array()
            .ok_or_else(|| format!("positions[{i}] is not an array of candidates"))?;
        if cands.is_empty() {
            return Err(format!("positions[{i}] offers no candidate"));
        }
        let mut out = Vec::with_capacity(cands.len());
        for (j, c) in cands.iter().enumerate() {
            let pair = c
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or_else(|| format!("positions[{i}][{j}] is not [character, distance]"))?;
            let s = pair[0]
                .as_str()
                .ok_or_else(|| format!("positions[{i}][{j}]: first element is not a string"))?;
            let mut it = s.chars();
            let (Some(ch), None) = (it.next(), it.next()) else {
                return Err(format!("positions[{i}][{j}]: {s:?} is not exactly one character"));
            };
            let d = pair[1]
                .as_f64()
                .ok_or_else(|| format!("positions[{i}][{j}]: distance is not a number"))?;
            if !d.is_finite() {
                return Err(format!("positions[{i}][{j}]: distance must be finite"));
            }
            out.push((ch, d as f32));
        }
        positions.push(out);
    }
    Ok(DecodeInput { why, params, positions })
}

/// Parses a `.decode.json`.
pub fn parse_expectation(text: &str) -> Result<DecodeExpectation, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let obj = v.as_object().ok_or("expectation file is not a JSON object")?;
    let s = |k: &str| -> Result<String, String> {
        obj.get(k)
            .and_then(|x| x.as_str())
            .map(str::to_string)
            .ok_or_else(|| format!("expectation needs a string \"{k}\""))
    };
    let stage = s("stage")?;
    if stage != "decode" {
        return Err(format!("expectation is for stage {stage:?}, not \"decode\""));
    }
    Ok(DecodeExpectation {
        fixture: s("fixture")?,
        text: s("text")?,
        identifier: obj
            .get("identifier")
            .and_then(|x| x.as_bool())
            .ok_or("expectation needs a boolean \"identifier\"")?,
        score: s("score")?,
    })
}

/// Writes a `.decode.json`, in the field order a reader expects.
///
/// Hand-formatted rather than serialised so the `score` string is never
/// routed through a float formatter: it is already a string by the time it
/// gets here, and the only risk left is JSON escaping, which `serde_json`
/// does for the two string fields that might need it.
pub fn write_expectation(e: &DecodeExpectation) -> String {
    format!(
        "{{\n  \"fixture\": {},\n  \"stage\": \"decode\",\n  \"text\": {},\n  \
         \"identifier\": {},\n  \"score\": {}\n}}\n",
        serde_json::Value::String(e.fixture.clone()),
        serde_json::Value::String(e.text.clone()),
        e.identifier,
        serde_json::Value::String(e.score.clone()),
    )
}

/// The charset a fixture implies: every distinct character it names, sorted,
/// so a class index is a property of the fixture's own text and not of the
/// shipped charset. A fixture that borrowed the model's class numbering would
/// change meaning every time a class was inserted.
pub fn charset(input: &DecodeInput) -> Vec<char> {
    let mut cs: Vec<char> = input.positions.iter().flatten().map(|(c, _)| *c).collect();
    cs.sort_unstable();
    cs.dedup();
    cs
}

/// Builds the lattice, the class table and the parameters, then runs the
/// decoder. `Err` is a fixture that could not be run at all — an unknown
/// parameter name — and is reported separately from a mismatch.
pub fn run(input: &DecodeInput, name: &str) -> Result<DecodeExpectation, String> {
    let cs = charset(input);
    let class_of = |c: char| cs.iter().position(|x| *x == c).unwrap() as u16;
    let class_info: Vec<ClassInfo> = cs.iter().map(|c| ClassInfo::of(*c)).collect();

    let mut edges = Vec::with_capacity(input.positions.len());
    for (i, pos) in input.positions.iter().enumerate() {
        // `ratio` is carried through the decoder untouched and only reaches
        // confidence, which this stage does not assert, so a constant keeps
        // the authored file about the thing it is testing.
        let cands: Vec<Cand> = pos
            .iter()
            .map(|(c, d)| Cand { class: class_of(*c), distance: *d, ratio: 0.5 })
            .collect();
        edges.push(Hyp {
            from: i,
            to: i + 1,
            x0: (i as u32) * 10,
            x1: (i as u32) * 10 + 10,
            kind: EdgeKind::Single,
            // One authored aspect for every edge, so the segmentation prior
            // contributes the same constant to every path and cannot be the
            // reason one reading beat another.
            aspect: 0.5,
            cands,
        });
    }
    let lat = WordLattice { nodes: input.positions.len() + 1, edges };

    let mut params = Params::DEFAULT;
    for (k, v) in &input.params {
        if !params.set_f32(k, *v) && !(*v >= 0.0 && params.set_u32(k, *v as u32)) {
            return Err(format!("{name}: {k:?} is not a parameter name"));
        }
    }

    let word = ocrcer_core::decode::viterbi::decode_word(
        &lat,
        &class_info,
        &Tables::default(),
        &params.decode,
    )
    .ok_or_else(|| format!("{name}: no path reached the end node"))?;

    Ok(DecodeExpectation {
        fixture: name.to_string(),
        text: word.chars.iter().map(|c| cs[c.class as usize]).collect(),
        identifier: word.identifier,
        score: format!("{}", word.score),
    })
}

/// Discovers decode fixtures by the union of names across the input and
/// expectation directories, for the reason `fixtures/README.md` gives: a
/// deleted file should produce a named failure, never a silent disappearance
/// from the run.
pub fn discover(fixtures_root: &Path) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = Vec::new();
    let mut scan = |dir: std::path::PathBuf, suffix: &str| -> Result<(), String> {
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        };
        for ent in rd {
            let ent = ent.map_err(|e| format!("{}: {e}", dir.display()))?;
            let f = ent.file_name().to_string_lossy().to_string();
            if let Some(stem) = f.strip_suffix(suffix) {
                names.push(stem.to_string());
            }
        }
        Ok(())
    };
    scan(fixtures_root.join("decode"), ".lattice.json")?;
    scan(fixtures_root.join("expected").join("decode"), ".decode.json")?;
    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "why": "a tie must break to the lowest class index",
      "params": { "decode.w_bigram": 0.0 },
      "positions": [ [["b", 1.0], ["a", 1.0]] ]
    }"#;

    #[test]
    fn a_lattice_file_parses_to_what_it_says() {
        let i = parse_input(SAMPLE).unwrap();
        assert_eq!(i.positions.len(), 1);
        assert_eq!(i.positions[0], vec![('b', 1.0), ('a', 1.0)]);
        assert_eq!(i.params.get("decode.w_bigram"), Some(&0.0));
    }

    #[test]
    fn the_charset_is_the_fixtures_own_and_is_sorted() {
        let i = parse_input(SAMPLE).unwrap();
        assert_eq!(charset(&i), vec!['a', 'b']);
    }

    #[test]
    fn a_lattice_without_a_why_is_refused() {
        let e = parse_input(r#"{"positions": [[["a", 1.0]]]}"#).unwrap_err();
        assert!(e.contains("why"), "{e}");
    }

    #[test]
    fn a_two_character_candidate_string_is_refused() {
        let e = parse_input(r#"{"why":"x","positions": [[["ab", 1.0]]]}"#).unwrap_err();
        assert!(e.contains("exactly one character"), "{e}");
    }

    #[test]
    fn an_expectation_round_trips_through_its_own_writer() {
        let e = DecodeExpectation {
            fixture: "tie".into(),
            text: "a".into(),
            identifier: false,
            score: format!("{}", 1.0f64 / 3.0),
        };
        let back = parse_expectation(&write_expectation(&e)).unwrap();
        assert_eq!(back, e);
        assert_eq!(back.score.parse::<f64>().unwrap().to_bits(), (1.0f64 / 3.0).to_bits());
    }

    #[test]
    fn an_expectation_for_another_stage_is_refused() {
        let e = parse_expectation(
            r#"{"fixture":"x","stage":"feature","text":"a","identifier":false,"score":"1"}"#,
        )
        .unwrap_err();
        assert!(e.contains("not \"decode\""), "{e}");
    }

    #[test]
    fn an_equal_distance_tie_breaks_to_the_lowest_class_index() {
        // `a` sorts before `b`, so `a` is class 0 and must win a tie even
        // though the file lists `b` first.
        let i = parse_input(SAMPLE).unwrap();
        assert_eq!(run(&i, "tie").unwrap().text, "a");
    }

    #[test]
    fn an_unknown_parameter_name_is_an_error_and_not_a_mismatch() {
        let i = parse_input(r#"{"why":"x","params":{"decode.nope":1.0},"positions":[[["a",1.0]]]}"#)
            .unwrap();
        assert!(run(&i, "x").unwrap_err().contains("not a parameter name"));
    }
}
