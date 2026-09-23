//! Compiles `model/confusions.tsv` into the `confusions` table.
//!
//! # What this table is for
//!
//! The matcher separates most classes on shape. The pairs in
//! `confusion_candidates.tsv` are the ones it measurably cannot, and for those
//! the separating evidence is not in the glyph — it is in what sits beside it.
//! This table carries that evidence: a named context, the member of the pair
//! it argues for, and what the argument is worth.
//!
//! # Contract
//!
//! [`load`] fails loudly on an unknown context word, a glyph the charset does
//! not hold, a `favour` column naming neither member, or a non-positive
//! weight. [`build`] compiles pair rows into per-class adjustments and emits
//! the layout `ocrcer_core::decode::confusion` reads.
//!
//! # The zero-sum property, enforced here rather than trusted
//!
//! Every authored row is split evenly: `+adjust/2` to the favoured class and
//! `-adjust/2` to the other, in that context only. [`build`] performs that
//! split itself, so the file cannot express a one-sided prior even by
//! accident. A one-sided prior would read as a shape preference the matcher
//! never measured, and it would be invisible in any test that only looks at
//! the pair.

use std::collections::BTreeMap;
use std::path::Path;

use crate::tables::Class;

/// The contexts the decoder can evaluate, in bit order.
///
/// This order is a format constant. It is written into the table as names so
/// that a runtime compiled against a different order refuses the file rather
/// than reading `lexicon_word` where `identifier` was meant.
pub const CONTEXTS: [&str; 7] = [
    "digit_neighbour",
    "letter_neighbour",
    "upper_run",
    "word_start",
    "word_end",
    "identifier",
    "lexicon_word",
];

/// One authored row.
#[derive(Debug, Clone)]
pub struct Rule {
    pub a: char,
    pub b: char,
    /// Index into [`CONTEXTS`].
    pub context: usize,
    /// `true` when the context argues for `a`.
    pub favour_a: bool,
    /// Log2 units, before the even split.
    pub adjust: f64,
    pub note: String,
}

/// Reads `model/confusions.tsv`.
pub fn load(dir: &Path) -> Result<Vec<Rule>, String> {
    let path = dir.join("confusions.tsv");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut out: Vec<Rule> = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        if raw.trim().is_empty() || raw.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = raw.split('\t').collect();
        let bad = |why: &str| format!("confusions.tsv line {}: {why}", n + 1);
        if f.len() < 6 {
            return Err(bad("expected six tab-separated columns"));
        }
        let one = |s: &str, which: &str| -> Result<char, String> {
            let mut it = s.chars();
            match (it.next(), it.next()) {
                (Some(c), None) => Ok(c),
                _ => Err(bad(&format!("column `{which}` must be exactly one character"))),
            }
        };
        let a = one(f[0], "a")?;
        let b = one(f[1], "b")?;
        if a == b {
            return Err(bad("a pair of one glyph is not a confusion"));
        }
        let context = CONTEXTS
            .iter()
            .position(|c| *c == f[2])
            .ok_or_else(|| bad(&format!("unknown context {:?}", f[2])))?;
        let favour_a = match f[3] {
            "a" => true,
            "b" => false,
            _ => return Err(bad("favour must be `a` or `b`")),
        };
        let adjust: f64 = f[4].trim().parse().map_err(|_| bad("bad adjust"))?;
        if !(adjust > 0.0) || adjust > 8.0 {
            return Err(bad("adjust must be positive and no more than 8 log2 units"));
        }
        if f[5].trim().is_empty() {
            return Err(bad("every rule needs a note saying what the test is"));
        }
        if out.iter().any(|r| r.a == a && r.b == b && r.context == context) {
            return Err(bad("this pair already has a rule for this context"));
        }
        out.push(Rule { a, b, context, favour_a, adjust, note: f[5].to_string() });
    }
    if out.is_empty() {
        return Err("confusions.tsv holds no rules".into());
    }
    Ok(out)
}

/// The compiled table.
///
/// ```text
/// 0   magic       [u8; 4]  "CNFS"
/// 4   version     u16      1
/// 6   n_contexts  u16
/// 8   count       u32      number of (class, context) adjustments
///     per context, in bit order: name_len u8, name [u8]
///     per adjustment, sorted by (class, context):
///         class u16, context u8, pad u8, adjust f32
/// ```
///
/// The context names are in the file so that a runtime whose compiled list has
/// moved refuses the table. The adjustments are sorted so a reader can find a
/// class by bisection and then walk its short run.
pub struct Table {
    pub bytes: Vec<u8>,
    /// Authored rows consumed.
    pub rules: usize,
    /// Distinct (class, context) adjustments emitted.
    pub entries: usize,
}

/// Compiles the rules against a charset.
pub fn build(rules: &[Rule], classes: &[Class]) -> Result<Table, String> {
    let index: BTreeMap<char, u16> = classes.iter().map(|c| (c.codepoint, c.index)).collect();
    let mut acc: BTreeMap<(u16, usize), f64> = BTreeMap::new();

    for r in rules {
        let ia = *index
            .get(&r.a)
            .ok_or_else(|| format!("confusions.tsv: U+{:04X} is not in the charset", r.a as u32))?;
        let ib = *index
            .get(&r.b)
            .ok_or_else(|| format!("confusions.tsv: U+{:04X} is not in the charset", r.b as u32))?;
        let half = r.adjust / 2.0;
        let (up, down) = if r.favour_a { (ia, ib) } else { (ib, ia) };
        *acc.entry((up, r.context)).or_insert(0.0) += half;
        *acc.entry((down, r.context)).or_insert(0.0) -= half;
    }

    // A rule can cancel exactly — `x`/`×` favouring one way in one context and
    // the other way in another does not, but two rules on the same class and
    // context can. A zero carries no information and the runtime would add it
    // for nothing, so it is dropped here rather than stored.
    acc.retain(|_, v| *v != 0.0);

    let mut b = Vec::new();
    b.extend_from_slice(b"CNFS");
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&(CONTEXTS.len() as u16).to_le_bytes());
    b.extend_from_slice(&(acc.len() as u32).to_le_bytes());
    for name in CONTEXTS {
        b.push(name.len() as u8);
        b.extend_from_slice(name.as_bytes());
    }
    for ((class, context), adjust) in &acc {
        b.extend_from_slice(&class.to_le_bytes());
        b.push(*context as u8);
        b.push(0);
        b.extend_from_slice(&(*adjust as f32).to_le_bytes());
    }
    Ok(Table { bytes: b, rules: rules.len(), entries: acc.len() })
}

/// Loads and compiles in one step.
pub fn from_model_dir(dir: &Path, classes: &[Class]) -> Result<Table, String> {
    build(&load(dir)?, classes)
}

/// How many distinct pairs the table separates, for reporting.
pub fn pairs(rules: &[Rule]) -> usize {
    let set: std::collections::BTreeSet<(char, char)> = rules.iter().map(|r| (r.a, r.b)).collect();
    set.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables;
    use ocrcer_core::decode::confusion::Confusions;

    fn charset() -> Vec<Class> {
        tables::load_charset(&tables::model_dir()).expect("charset loads")
    }

    #[test]
    fn the_authored_file_parses_and_covers_the_classic_pairs() {
        let r = load(&tables::model_dir()).expect("confusions.tsv parses");
        assert!(r.len() >= 30, "only {} rules", r.len());
        let has = |a: char, b: char| r.iter().any(|x| x.a == a && x.b == b);
        // The families `ARCHITECTURE.md` section 2 names by hand.
        assert!(has('1', 'l') && has('0', 'O') && has('1', 'I'));
        // The domain pair the decision log settled.
        assert!(has('Ø', '⌀'));
    }

    /// The property that makes a confusion rule safe: it can never make a
    /// class more likely in general, only more likely than its rival.
    #[test]
    fn every_context_sums_to_zero_across_classes() {
        let rules = load(&tables::model_dir()).expect("parses");
        let t = build(&rules, &charset()).expect("builds");
        let c = Confusions::parse(&t.bytes).expect("parses back");
        for ctx in 0..CONTEXTS.len() {
            let mut sum = 0.0f64;
            for class in 0..charset().len() as u16 {
                sum += f64::from(c.adjust(class, 1u8 << ctx));
            }
            assert!(sum.abs() < 1e-4, "context {} sums to {sum}", CONTEXTS[ctx]);
        }
    }

    #[test]
    fn a_context_that_is_not_present_costs_nothing() {
        let rules = load(&tables::model_dir()).expect("parses");
        let t = build(&rules, &charset()).expect("builds");
        let c = Confusions::parse(&t.bytes).expect("parses back");
        for class in 0..charset().len() as u16 {
            assert_eq!(c.adjust(class, 0), 0.0);
        }
    }

    /// A digit between digits should be argued for, and the letter it is
    /// confused with argued against, by the same amount.
    #[test]
    fn a_digit_context_favours_the_digit_over_its_letter_twin() {
        let rules = load(&tables::model_dir()).expect("parses");
        let cs = charset();
        let t = build(&rules, &cs).expect("builds");
        let c = Confusions::parse(&t.bytes).expect("parses back");
        let idx = |ch: char| cs.iter().find(|k| k.codepoint == ch).expect("in charset").index;
        let digit = 1u8 << CONTEXTS.iter().position(|c| *c == "digit_neighbour").unwrap();
        assert!(c.adjust(idx('0'), digit) > 0.0);
        assert!(c.adjust(idx('O'), digit) < 0.0);
        assert!(c.adjust(idx('1'), digit) > 0.0);
        assert!(c.adjust(idx('l'), digit) < 0.0);
    }

    #[test]
    fn the_table_is_byte_identical_across_runs() {
        let rules = load(&tables::model_dir()).expect("parses");
        let cs = charset();
        let a = build(&rules, &cs).expect("builds");
        let b = build(&rules, &cs).expect("builds");
        assert_eq!(a.bytes, b.bytes);
    }
}
