//! Compiles `model/lexicon.txt` into the `lexicon` table: authored base
//! forms, expanded by deterministic English spelling rules, minimised into a
//! DAWG keyed on case-folded class indices.
//!
//! # Contract
//!
//! [`load`] parses the authored file and fails loudly on a malformed header,
//! an unknown inflection class or a tier outside `1..=5`. [`expand`] applies
//! the spelling rules. [`build`] minimises the result into the byte layout
//! documented on [`Dawg`], which `ocrcer_core::decode::lexicon` reads.
//!
//! # Why the symbols are class indices and not characters
//!
//! The decoder walks the lattice in class indices, so a lexicon keyed on
//! `char` would need a conversion at every edge of every path — the hot
//! inner loop of the whole engine. Keying the DAWG the same way costs one
//! lookup at build time instead.
//!
//! Symbols are **case-folded** through the charset's `case_twin` column, so
//! `INVOICE`, `Invoice` and `invoice` traverse the same path. That column is
//! already the authority on which classes are case pairs; a second mapping
//! typed in here would be the one that drifts.

use crate::tables::Class;

use std::collections::BTreeMap;
use std::path::Path;

/// The strongest tier. Tier 1 is the closed-class core.
pub const MAX_TIER: u8 = 5;

/// What a base form generates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Inflect {
    None,
    Noun,
    Verb,
    Adjective,
    NounVerb,
    NounAdjective,
    AdjectiveVerb,
}

impl Inflect {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "-" => Inflect::None,
            "n" => Inflect::Noun,
            "v" => Inflect::Verb,
            "a" => Inflect::Adjective,
            "nv" => Inflect::NounVerb,
            "na" => Inflect::NounAdjective,
            "av" => Inflect::AdjectiveVerb,
            _ => return None,
        })
    }

    fn noun(self) -> bool {
        matches!(self, Inflect::Noun | Inflect::NounVerb | Inflect::NounAdjective)
    }
    fn verb(self) -> bool {
        matches!(self, Inflect::Verb | Inflect::NounVerb | Inflect::AdjectiveVerb)
    }
    fn adjective(self) -> bool {
        matches!(self, Inflect::Adjective | Inflect::NounAdjective | Inflect::AdjectiveVerb)
    }
}

/// One authored base form.
#[derive(Clone, Debug)]
pub struct Entry {
    pub word: String,
    pub tier: u8,
    pub class: Inflect,
    pub domain: String,
}

/// Reads the authored file.
pub fn load(dir: &Path) -> Result<Vec<Entry>, String> {
    let path = dir.join("lexicon.txt");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    let mut out = Vec::new();
    let (mut tier, mut class, mut domain) = (0u8, Inflect::None, String::new());
    let mut have_header = false;

    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(body) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let (mut t, mut c, mut d) = (None, None, None);
            for field in body.split('|') {
                let mut it = field.split_whitespace();
                match (it.next(), it.next(), it.next()) {
                    (Some("tier"), Some(v), None) => t = v.parse::<u8>().ok(),
                    (Some("class"), Some(v), None) => c = Inflect::parse(v),
                    (Some("domain"), Some(v), None) => d = Some(v.to_string()),
                    _ => return Err(format!("lexicon.txt line {}: bad header field", n + 1)),
                }
            }
            match (t, c, d) {
                (Some(t), Some(c), Some(d)) if (1..=MAX_TIER).contains(&t) => {
                    tier = t;
                    class = c;
                    domain = d;
                    have_header = true;
                }
                _ => return Err(format!("lexicon.txt line {}: incomplete header", n + 1)),
            }
            continue;
        }
        if !have_header {
            return Err(format!("lexicon.txt line {}: word before any header", n + 1));
        }
        if line.split_whitespace().count() != 1 {
            return Err(format!("lexicon.txt line {}: one word per line", n + 1));
        }
        out.push(Entry { word: line.to_string(), tier, class, domain: domain.clone() });
    }
    if out.is_empty() {
        return Err("lexicon.txt holds no words".into());
    }
    Ok(out)
}

/// The words a lexicon must not contain, because the decoder's identifier
/// gate would suppress the lexicon wherever they appear.
///
/// `CLAUDE.md` rule 6 suppresses the lexicon term inside identifier-shaped
/// context so that `M8x1.25` can never be rewritten into a dictionary word.
/// The cost is that a lexicon entry of that same shape can never earn its
/// bonus: `T4`, `RC59` and `GST34` are exactly as identifier-shaped as `M8`
/// is, and no heuristic can separate a form number from a thread spec. Such
/// an entry is not merely wasted bytes — it makes "is the domain vocabulary
/// covered?" answer yes when the answer is no.
///
/// The verdict comes from `ocrcer_core::params::identifier_shape`, the same
/// rule the decoder applies, so this check cannot drift away from the gate it
/// is predicting.
pub fn identifier_shaped(
    words: &[(String, u8)],
    min_length: u32,
    digit_fraction: f32,
) -> Vec<String> {
    let mut d = ocrcer_core::params::Params::default().decode;
    d.identifier_min_length = min_length;
    d.identifier_digit_fraction = digit_fraction;
    words
        .iter()
        .filter(|(w, _)| {
            let len = w.chars().count();
            let digits = w.chars().filter(|c| c.is_ascii_digit()).count();
            let letters = w.chars().filter(|c| c.is_alphabetic()).count();
            ocrcer_core::params::identifier_shape(len, digits, letters, &d)
        })
        .map(|(w, _)| w.clone())
        .collect()
}

/// Expands the authored base forms into every surface form, keeping the
/// strongest tier when two entries agree on a word.
///
/// **A generated form is banded one tier weaker than the form it came from.**
/// The rules below are the regular English ones and English is not entirely
/// regular, so a handful of the generated strings are not words. Under rule 6
/// that costs a misapplied bonus and never a rewrite, and weakening the band
/// makes the misapplication smaller than the coverage is worth.
pub fn expand(entries: &[Entry]) -> Vec<(String, u8)> {
    let mut best: BTreeMap<String, u8> = BTreeMap::new();
    let mut put = |w: String, tier: u8| {
        if w.is_empty() {
            return;
        }
        let e = best.entry(w).or_insert(MAX_TIER);
        if tier < *e {
            *e = tier;
        }
    };

    for e in entries {
        put(e.word.clone(), e.tier);
        let derived = e.tier.saturating_add(1).min(MAX_TIER);
        let w = &e.word;
        // Inflection applies to lowercase-shaped words only. An all-caps
        // abbreviation that happened to be filed under a noun class would
        // otherwise generate `DWGs`, which is not a string anyone writes.
        if !w.chars().all(|c| c.is_lowercase() || c == '-') {
            continue;
        }
        if e.class.noun() || e.class.verb() {
            put(plural_s(w), derived);
        }
        if e.class.verb() {
            put(past(w), derived);
            put(gerund(w), derived);
        }
        if e.class.adjective() {
            put(comparative(w, "er"), derived);
            put(comparative(w, "est"), derived);
            put(adverb(w), derived);
        }
    }
    best.into_iter().collect()
}

fn ends_any(w: &str, suffixes: &[&str]) -> bool {
    suffixes.iter().any(|s| w.ends_with(s))
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

/// One vowel cluster, so roughly one syllable — the condition under which
/// English doubles a final consonant. `fit` doubles, `offer` does not.
fn monosyllabic(w: &str) -> bool {
    let mut clusters = 0;
    let mut prev_vowel = false;
    for c in w.chars() {
        let v = is_vowel(c);
        if v && !prev_vowel {
            clusters += 1;
        }
        prev_vowel = v;
    }
    clusters == 1
}

/// `stop` -> `stopp`, for the suffixes that need it. Returns `None` when the
/// word does not take the doubling.
fn doubled(w: &str) -> Option<String> {
    let mut it = w.chars().rev();
    let last = it.next()?;
    let mid = it.next()?;
    let first = it.next()?;
    if !monosyllabic(w) {
        return None;
    }
    if is_vowel(last) || matches!(last, 'w' | 'x' | 'y') {
        return None;
    }
    if !is_vowel(mid) || is_vowel(first) {
        return None;
    }
    let mut s = w.to_string();
    s.push(last);
    Some(s)
}

fn plural_s(w: &str) -> String {
    if ends_any(w, &["s", "x", "z", "ch", "sh"]) {
        format!("{w}es")
    } else if let Some(stem) = consonant_y_stem(w) {
        format!("{stem}ies")
    } else {
        format!("{w}s")
    }
}

/// `carry` -> `carr`, but `play` -> `None`: the rule is consonant-then-y.
fn consonant_y_stem(w: &str) -> Option<String> {
    let mut it = w.chars().rev();
    if it.next()? != 'y' {
        return None;
    }
    if is_vowel(it.next()?) {
        return None;
    }
    Some(w[..w.len() - 1].to_string())
}

fn past(w: &str) -> String {
    if w.ends_with('e') {
        format!("{w}d")
    } else if let Some(stem) = consonant_y_stem(w) {
        format!("{stem}ied")
    } else if let Some(d) = doubled(w) {
        format!("{d}ed")
    } else {
        format!("{w}ed")
    }
}

fn gerund(w: &str) -> String {
    if w.ends_with("ee") || w.ends_with("oe") || w.ends_with("ye") {
        format!("{w}ing")
    } else if w.ends_with('e') {
        format!("{}ing", &w[..w.len() - 1])
    } else if let Some(d) = doubled(w) {
        format!("{d}ing")
    } else {
        format!("{w}ing")
    }
}

fn comparative(w: &str, suffix: &str) -> String {
    if w.ends_with('e') {
        format!("{w}{}", &suffix[1..])
    } else if let Some(stem) = consonant_y_stem(w) {
        format!("{stem}i{suffix}")
    } else if let Some(d) = doubled(w) {
        format!("{d}{suffix}")
    } else {
        format!("{w}{suffix}")
    }
}

/// A consonant followed by `le` at the end, which is the shape that drops its
/// `e` before the adverbial `y`.
fn consonant_le(w: &str) -> bool {
    let mut it = w.chars().rev();
    it.next() == Some('e') && it.next() == Some('l') && it.next().is_some_and(|c| !is_vowel(c))
}

fn adverb(w: &str) -> String {
    if let Some(stem) = consonant_y_stem(w) {
        format!("{stem}ily")
    } else if w.ends_with("ic") {
        format!("{w}ally")
    } else if consonant_le(w) {
        // `simple` -> `simply`, `possible` -> `possibly`.
        format!("{}y", &w[..w.len() - 1])
    } else {
        format!("{w}ly")
    }
}

/// The compiled word graph, in the byte layout the runtime reads.
///
/// ```text
/// 0   magic        [u8; 4]   "LXDW"
/// 4   version      u16       1
/// 6   reserved     u16       0
/// 8   n_nodes      u32       node 0 is the root
/// 12  n_edges      u32
/// 16  node_off     [u32; n_nodes + 1]   edge range per node, ascending
///     edge_target  [u32; n_edges]
///     edge_symbol  [u16; n_edges]       class index, ascending within a node
///     node_flags   [u8;  n_nodes]       bit 0 terminal, bits 1-3 tier - 1
/// ```
///
/// Edges are sorted by symbol inside each node so the runtime can binary
/// search, and every multi-byte field is little-endian and naturally aligned
/// at its own offset.
pub struct Dawg {
    pub bytes: Vec<u8>,
    pub words: usize,
    pub nodes: usize,
    pub edges: usize,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Node {
    terminal: bool,
    tier: u8,
    edges: Vec<(u16, u32)>,
}

impl Node {
    fn new() -> Self {
        Node { terminal: false, tier: 0, edges: Vec::new() }
    }
}

/// Minimises the word list into a DAWG.
///
/// Incremental construction over sorted input: at each word the common prefix
/// with the previous word is kept, everything past it is finished and either
/// found in the register of already-minimised nodes or added to it. That is
/// the standard construction, and its property here is the one that matters —
/// the output depends only on the sorted word list, so two runs of this
/// produce identical bytes.
pub fn build(words: &[(String, u8)], classes: &[Class]) -> Result<Dawg, String> {
    let fold = fold_map(classes);

    // Symbol sequences, sorted. The sort is on symbols and not on the
    // strings, because the register's equivalence is on symbols.
    let mut keys: Vec<(Vec<u16>, u8)> = Vec::with_capacity(words.len());
    for (w, tier) in words {
        let mut syms = Vec::with_capacity(w.chars().count());
        let mut ok = true;
        for c in w.chars() {
            match fold.get(&c) {
                Some(&s) => syms.push(s),
                // A word using a character the charset cannot recognise is
                // dropped rather than refused: it is unreachable by the
                // decoder anyway, so keeping it would only cost bytes.
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok && !syms.is_empty() {
            keys.push((syms, *tier));
        }
    }
    keys.sort();
    keys.dedup_by(|a, b| {
        if a.0 == b.0 {
            b.1 = b.1.min(a.1);
            true
        } else {
            false
        }
    });

    // Uncompressed path of the word being added; `register` maps a finished
    // node to its assigned index.
    let mut register: BTreeMap<Node, u32> = BTreeMap::new();
    let mut pool: Vec<Node> = vec![Node::new()]; // slot 0 reserved for the root
    let mut path: Vec<Node> = vec![Node::new()];
    let mut prev: Vec<u16> = Vec::new();

    let flush = |path: &mut Vec<Node>,
                     pool: &mut Vec<Node>,
                     register: &mut BTreeMap<Node, u32>,
                     down_to: usize,
                     prev: &[u16]| {
        while path.len() > down_to + 1 {
            let node = path.pop().expect("path deeper than the root");
            let sym = prev[path.len() - 1];
            let id = match register.get(&node) {
                Some(&id) => id,
                None => {
                    let id = pool.len() as u32;
                    pool.push(node.clone());
                    register.insert(node, id);
                    id
                }
            };
            path.last_mut().expect("root is never popped").edges.push((sym, id));
        }
    };

    for (syms, tier) in &keys {
        let common = syms.iter().zip(prev.iter()).take_while(|(a, b)| a == b).count();
        flush(&mut path, &mut pool, &mut register, common, &prev);
        for _ in common..syms.len() {
            path.push(Node::new());
        }
        let leaf = path.last_mut().expect("a word is at least one symbol");
        leaf.terminal = true;
        leaf.tier = *tier;
        prev = syms.clone();
    }
    flush(&mut path, &mut pool, &mut register, 0, &prev);
    pool[0] = path.pop().expect("the root survives");

    Ok(encode(&pool, keys.len()))
}

fn encode(pool: &[Node], words: usize) -> Dawg {
    let n_nodes = pool.len();
    let n_edges: usize = pool.iter().map(|n| n.edges.len()).sum();

    let mut bytes = Vec::with_capacity(16 + 4 * (n_nodes + 1) + 6 * n_edges + n_nodes);
    bytes.extend_from_slice(b"LXDW");
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&(n_nodes as u32).to_le_bytes());
    bytes.extend_from_slice(&(n_edges as u32).to_le_bytes());

    let mut sorted: Vec<Vec<(u16, u32)>> = Vec::with_capacity(n_nodes);
    for n in pool {
        let mut e = n.edges.clone();
        e.sort();
        sorted.push(e);
    }

    let mut off = 0u32;
    for e in &sorted {
        bytes.extend_from_slice(&off.to_le_bytes());
        off += e.len() as u32;
    }
    bytes.extend_from_slice(&off.to_le_bytes());

    for e in &sorted {
        for &(_, target) in e {
            bytes.extend_from_slice(&target.to_le_bytes());
        }
    }
    for e in &sorted {
        for &(sym, _) in e {
            bytes.extend_from_slice(&sym.to_le_bytes());
        }
    }
    for n in pool {
        let tier = n.tier.clamp(0, MAX_TIER);
        let flags = u8::from(n.terminal) | (tier.saturating_sub(1) << 1);
        bytes.push(flags);
    }

    Dawg { bytes, words, nodes: n_nodes, edges: n_edges }
}

/// Character to case-folded class index, from the charset's `case_twin`
/// column.
fn fold_map(classes: &[Class]) -> BTreeMap<char, u16> {
    let lower: BTreeMap<u16, u16> = classes
        .iter()
        .filter(|c| c.case_twin.is_some() && c.codepoint.is_uppercase())
        .map(|c| (c.index, c.case_twin.expect("filtered on Some")))
        .collect();
    classes
        .iter()
        .map(|c| (c.codepoint, *lower.get(&c.index).unwrap_or(&c.index)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(word: &str, class: Inflect) -> Entry {
        Entry { word: word.into(), tier: 2, class, domain: "test".into() }
    }

    fn forms(word: &str, class: Inflect) -> Vec<String> {
        expand(&[entry(word, class)]).into_iter().map(|(w, _)| w).collect()
    }

    #[test]
    fn the_gate_catches_a_form_number_and_spares_a_word() {
        let probe: Vec<(String, u8)> = ["T4", "RC59", "M8x1.25", "total", "HST", "2026"]
            .iter()
            .map(|w| ((*w).to_string(), 3u8))
            .collect();
        let dead = identifier_shaped(&probe, 2, 0.2);
        assert!(dead.contains(&"T4".to_string()));
        assert!(dead.contains(&"RC59".to_string()));
        assert!(dead.contains(&"M8x1.25".to_string()));
        assert!(!dead.contains(&"total".to_string()));
        assert!(!dead.contains(&"HST".to_string()));
        // Pure digits are a number, not an identifier, and the lexicon holds
        // no digit strings anyway.
        assert!(!dead.contains(&"2026".to_string()));
    }

    #[test]
    fn the_shipped_lexicon_holds_nothing_the_identifier_gate_would_suppress() {
        let dir = crate::tables::model_dir();
        let words = expand(&load(&dir).unwrap());
        let ps = crate::params::load(&dir).unwrap();
        let v = |n: &str, f: f64| ps.iter().find(|p| p.name == n).map_or(f, |p| p.value);
        let dead = identifier_shaped(
            &words,
            v("decode.identifier_min_length", 2.0) as u32,
            v("decode.identifier_digit_fraction", 0.2) as f32,
        );
        assert!(
            dead.is_empty(),
            "{} entries can never earn their bonus, e.g. {:?}",
            dead.len(),
            &dead[..dead.len().min(8)]
        );
    }

    #[test]
    fn the_authored_file_parses_and_is_not_trivially_small() {
        let dir = crate::tables::model_dir();
        let entries = load(&dir).expect("lexicon.txt parses");
        assert!(entries.len() > 1000, "only {} base forms", entries.len());
        let words = expand(&entries);
        assert!(words.len() > entries.len(), "expansion generated nothing");
    }

    #[test]
    fn plurals_follow_the_authored_spelling_rules() {
        assert!(forms("box", Inflect::Noun).contains(&"boxes".to_string()));
        assert!(forms("company", Inflect::Noun).contains(&"companies".to_string()));
        assert!(forms("day", Inflect::Noun).contains(&"days".to_string()));
        assert!(forms("account", Inflect::Noun).contains(&"accounts".to_string()));
        assert!(forms("class", Inflect::Noun).contains(&"classes".to_string()));
    }

    #[test]
    fn a_final_consonant_doubles_only_on_a_single_syllable() {
        assert!(forms("stop", Inflect::Verb).contains(&"stopped".to_string()));
        assert!(forms("fit", Inflect::Verb).contains(&"fitting".to_string()));
        // Two vowel clusters, so no doubling: `offered`, not `offerred`.
        assert!(forms("offer", Inflect::Verb).contains(&"offered".to_string()));
        assert!(!forms("offer", Inflect::Verb).contains(&"offerred".to_string()));
        // A final vowel, w, x or y never doubles.
        assert!(forms("play", Inflect::Verb).contains(&"played".to_string()));
        assert!(forms("fix", Inflect::Verb).contains(&"fixed".to_string()));
    }

    #[test]
    fn a_silent_e_is_dropped_before_ing_and_kept_before_d() {
        assert!(forms("close", Inflect::Verb).contains(&"closing".to_string()));
        assert!(forms("close", Inflect::Verb).contains(&"closed".to_string()));
        assert!(forms("agree", Inflect::Verb).contains(&"agreeing".to_string()));
    }

    #[test]
    fn a_generated_form_is_banded_one_tier_weaker_than_its_base() {
        let out = expand(&[entry("account", Inflect::Noun)]);
        let base = out.iter().find(|(w, _)| w == "account").expect("base form");
        let plural = out.iter().find(|(w, _)| w == "accounts").expect("plural");
        assert_eq!(base.1, 2);
        assert_eq!(plural.1, 3);
    }

    /// The whole authored list, compiled. This is the one that would catch a
    /// charset change breaking the fold map, and it is where the reported
    /// word and node counts come from.
    #[test]
    fn the_authored_lexicon_compiles_into_a_graph() {
        let dir = crate::tables::model_dir();
        let classes = crate::tables::load_charset(&dir).expect("charset");
        let words = expand(&load(&dir).expect("lexicon"));
        let dawg = build(&words, &classes).expect("dawg builds");
        assert!(dawg.words > 3000, "only {} words", dawg.words);
        assert!(dawg.nodes < dawg.words * 8, "minimisation did nothing");
        assert_eq!(&dawg.bytes[..4], b"LXDW");
    }

    /// Two runs must produce the same bytes, or the `.ocrw` stops being
    /// reproducible and rule 1's "anyone can re-run the script and get the
    /// same bytes" stops being true.
    #[test]
    fn the_graph_is_byte_identical_across_runs() {
        let dir = crate::tables::model_dir();
        let classes = crate::tables::load_charset(&dir).expect("charset");
        let words = expand(&load(&dir).expect("lexicon"));
        let a = build(&words, &classes).expect("dawg");
        let b = build(&words, &classes).expect("dawg");
        assert_eq!(a.bytes, b.bytes);
    }

    #[test]
    fn an_abbreviation_generates_nothing() {
        let out = forms("DWG", Inflect::Noun);
        assert_eq!(out, vec!["DWG".to_string()]);
    }
}
