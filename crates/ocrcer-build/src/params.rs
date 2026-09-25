//! Compiles `model/params.tsv` into the `params` table, and reports which of
//! the engine's thresholds are guesses.
//!
//! # Contract
//!
//! [`load`] fails loudly on an unknown type, an unknown provenance word or a
//! malformed row. [`build`] produces the byte layout `ocrcer_core::params`
//! reads. [`check_against_defaults`] is the anti-drift gate: it fails when a
//! row's value differs from the compiled-in default, which is the one way the
//! duplication between this file and `ocrcer_core::params::Params::DEFAULT`
//! can be made safe.

use std::path::Path;

/// Where a value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Read off a named run.
    Measured,
    /// Follows from a stated principle rather than a sweep.
    Authored,
    /// A plausible starting value and nothing more.
    Guess,
    /// Chosen by a committed, deterministic script on a named training
    /// split and confirmed on a disjoint validation split; the row's
    /// description names both.
    Fitted,
}

impl Provenance {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "measured" => Provenance::Measured,
            "authored" => Provenance::Authored,
            "guess" => Provenance::Guess,
            "fitted" => Provenance::Fitted,
            _ => return None,
        })
    }

    pub fn word(self) -> &'static str {
        match self {
            Provenance::Measured => "measured",
            Provenance::Authored => "authored",
            Provenance::Guess => "guess",
            Provenance::Fitted => "fitted",
        }
    }
}

/// One threshold.
#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub value: f64,
    /// `false` for `f32`, `true` for an integer or a bool.
    pub integral: bool,
    pub provenance: Provenance,
    pub tune: bool,
    pub note: String,
}

/// Reads `model/params.tsv`.
pub fn load(dir: &Path) -> Result<Vec<Param>, String> {
    let path = dir.join("params.tsv");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut out: Vec<Param> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        if raw.trim().is_empty() || raw.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = raw.split('\t').collect();
        let bad = |why: &str| format!("params.tsv line {}: {why}", n + 1);
        if f.len() < 6 {
            return Err(bad("expected six tab-separated columns"));
        }
        let integral = match f[2] {
            "f32" => false,
            "u32" | "bool" => true,
            _ => return Err(bad("type must be f32, u32 or bool")),
        };
        let param = Param {
            name: f[0].to_string(),
            value: f[1].trim().parse().map_err(|_| bad("bad value"))?,
            integral,
            provenance: Provenance::parse(f[3]).ok_or_else(|| bad("unknown provenance"))?,
            tune: match f[4] {
                "yes" => true,
                "no" => false,
                _ => return Err(bad("tune must be yes or no")),
            },
            note: f[5].to_string(),
        };
        if param.note.trim().is_empty() {
            return Err(bad("every row needs a note saying where the number came from"));
        }
        if out.iter().any(|p| p.name == param.name) {
            return Err(bad("duplicate name"));
        }
        out.push(param);
    }
    if out.is_empty() {
        return Err("params.tsv holds no rows".into());
    }
    Ok(out)
}

/// The compiled table.
///
/// ```text
/// 0   magic    [u8; 4]   "PARM"
/// 4   version  u16       1
/// 6   reserved u16       0
/// 8   count    u32
///     per row: name_len u8, tag u8 (0 = f32, 1 = u32), name [u8], value [u8; 4]
/// ```
///
/// Names rather than positions, because a positional parameter block has the
/// same failure mode as a positional charset: inserting one shifts every
/// threshold after it, and nothing in the file reports that it happened.
pub struct Table {
    pub bytes: Vec<u8>,
    pub count: usize,
}

/// Encodes the parameter block.
pub fn build(params: &[Param]) -> Result<Table, String> {
    let mut b = Vec::new();
    b.extend_from_slice(b"PARM");
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    b.extend_from_slice(&(params.len() as u32).to_le_bytes());
    for p in params {
        if p.name.len() > 255 {
            return Err(format!("`{}` is too long a parameter name", p.name));
        }
        b.push(p.name.len() as u8);
        b.push(u8::from(p.integral));
        b.extend_from_slice(p.name.as_bytes());
        if p.integral {
            if p.value < 0.0 || p.value > f64::from(u32::MAX) || p.value.fract() != 0.0 {
                return Err(format!("`{}` is declared integral but its value is not", p.name));
            }
            b.extend_from_slice(&(p.value as u32).to_le_bytes());
        } else {
            b.extend_from_slice(&(p.value as f32).to_le_bytes());
        }
    }
    Ok(Table { bytes: b, count: params.len() })
}

/// How many of the engine's thresholds are of each provenance.
///
/// The number a report quotes when it says what fraction of the pipeline is
/// still guessed. Reporting it is the point of the provenance column.
pub fn census(params: &[Param]) -> (usize, usize, usize, usize) {
    let count = |p: Provenance| params.iter().filter(|q| q.provenance == p).count();
    (
        count(Provenance::Measured),
        count(Provenance::Authored),
        count(Provenance::Guess),
        count(Provenance::Fitted),
    )
}

/// Fails when the file and the compiled-in defaults disagree.
///
/// This is what makes it safe for a threshold to exist both in
/// `model/params.tsv` and in `ocrcer_core::params::Params::DEFAULT`. Without
/// it the two would drift, the engine would run one set of numbers, and the
/// file recording their provenance would describe a different set — which is
/// exactly the silent divergence `CLAUDE.md` rule 4 exists to prevent.
pub fn check_against_defaults(params: &[Param]) -> Result<(), String> {
    use ocrcer_core::params::Params;
    let d = Params::DEFAULT;
    let mut problems = Vec::new();

    for p in params {
        match d.get(&p.name) {
            None => problems.push(format!("`{}` is in params.tsv and not in Params", p.name)),
            Some(v) => {
                // Compared at `f32` precision because that is the precision
                // the table stores and the engine uses.
                let want = p.value as f32;
                if v.to_bits() != want.to_bits() {
                    problems.push(format!("`{}`: params.tsv says {want}, Params says {v}", p.name));
                }
            }
        }
    }
    for name in Params::NAMES {
        if !params.iter().any(|p| p.name == name) {
            problems.push(format!("`{name}` is in Params and not in params.tsv"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables;

    #[test]
    fn the_authored_file_parses() {
        let p = load(&tables::model_dir()).expect("params.tsv parses");
        assert!(p.len() >= 30, "only {} rows", p.len());
        let (measured, authored, guess, fitted) = census(&p);
        assert_eq!(measured + authored + guess + fitted, p.len());
        // Not an assertion about the right proportion — only that the column
        // is being used, so a report of it means something.
        assert!(guess > 0 && authored > 0);
    }

    /// The gate. If this fails, one of the two copies moved without the
    /// other, and the fix is to decide which is right rather than to relax
    /// the test.
    #[test]
    fn the_file_and_the_compiled_defaults_agree() {
        let p = load(&tables::model_dir()).expect("params.tsv parses");
        if let Err(e) = check_against_defaults(&p) {
            panic!("params.tsv and ocrcer_core::params::Params::DEFAULT disagree:\n{e}");
        }
    }

    #[test]
    fn the_table_round_trips_through_the_runtime_reader() {
        let p = load(&tables::model_dir()).expect("params.tsv parses");
        let t = build(&p).expect("table builds");
        let mut loaded = ocrcer_core::params::Params::DEFAULT;
        // Start from something different so a no-op would show.
        loaded.decode.w_bigram = -1.0;
        let applied = loaded.apply(&t.bytes).expect("the table loads");
        assert_eq!(applied, ocrcer_core::params::Params::NAMES.len());
        assert_eq!(loaded, ocrcer_core::params::Params::DEFAULT);
    }
}
