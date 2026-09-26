//! Turns a built `Bank` into the tables and `meta` block of a `.ocrw` file.
//!
//! # Contract
//!
//! Deterministic: the same bank gives the same bytes. The build identifier is
//! a digest of the inputs that produced the bank, not a timestamp, for the
//! reason `ocrw`'s module doc gives.
//!
//! The charset is written into `meta` rather than left in Rust source, so a
//! model file and a runtime cannot disagree about what a class index means
//! (`ARCHITECTURE.md` section 7).

use ocrcer_core::feature::{FEATURE_DIMS, FEATURE_VERSION};

use crate::bank::Bank;
use crate::ocrw::{self, crc32, json_string, Table};
use crate::tables::{Class, Distribution};

/// Table names, so the writer here and the reader in `ocrcer-core` name the
/// same things.
pub const T_PROTOTYPES: &str = "prototypes";
pub const T_PROTOTYPE_CLASS: &str = "prototype_class";
pub const T_FEATURE_NORM: &str = "feature_norm";
pub const T_CLASS_HOLES: &str = "class_holes";
pub const T_PROTOTYPE_FACE: &str = "prototype_face";
pub const T_LEXICON: &str = "lexicon";
pub const T_BIGRAMS: &str = "bigrams";
pub const T_CONFUSIONS: &str = "confusions";
pub const T_PARAMS: &str = "params";

/// The tables a recogniser model carries, in the order they are written.
///
/// `prototype_class` and `class_holes` are `Kind::Opaque` because the format
/// has no integer table kind. Their encodings are stated in `meta` rather
/// than assumed: little-endian `u16` per prototype, and one `u8` bitmask per
/// class where bit *n* means "a prototype of this class was measured with *n*
/// holes".
pub fn tables(bank: &Bank) -> Vec<Table> {
    let n = bank.prototypes.len();
    let mut flat = Vec::with_capacity(n * FEATURE_DIMS);
    for s in &bank.standardised {
        flat.extend_from_slice(s);
    }
    let (q, scales) = ocrw::quantise(&flat, n, FEATURE_DIMS);

    let mut class_bytes = Vec::with_capacity(n * 2);
    let mut face_bytes = Vec::with_capacity(n * 2);
    for p in &bank.prototypes {
        class_bytes.extend_from_slice(&p.class.to_le_bytes());
        face_bytes.extend_from_slice(&p.face.to_le_bytes());
    }

    let slots = bank.gates.len();
    let holes: Vec<u8> = bank.gates.iter().map(|g| g.holes).collect();

    let mut norm = Vec::with_capacity(FEATURE_DIMS * 2);
    norm.extend_from_slice(&bank.mean);
    norm.extend_from_slice(&bank.sd);

    vec![
        Table::i8s(T_PROTOTYPES, n as u32, FEATURE_DIMS as u32, q, scales),
        Table::opaque(T_PROTOTYPE_CLASS, vec![n as u32], class_bytes),
        Table::f32s(T_FEATURE_NORM, vec![2, FEATURE_DIMS as u32], &norm),
        Table::opaque(T_CLASS_HOLES, vec![slots as u32], holes),
        // Face index per prototype, little-endian u16, so the runtime can
        // derive `prototype_italic` from `meta.faces[i].style` without a
        // second copy of the style string in every row (`ARCHITECTURE.md`
        // section 11, 2026-09-24 decision). Optional in the format, but
        // written by every build from here on.
        Table::opaque(T_PROTOTYPE_FACE, vec![n as u32], face_bytes),
    ]
}

/// The authored language tables, compiled from `model/`.
///
/// Separate from [`tables`] because they are a different kind of thing: those
/// are measured from rendered glyphs and these are authored knowledge. They
/// are also all optional in the format, so a build that cannot compile one of
/// them can still write a usable recogniser — a decode without a lexicon is a
/// weaker reading, never a wrong one.
///
/// They are appended after the bank tables, so a reader walking the table
/// list in order sees the recogniser first and the decoder's knowledge after.
/// `feature_weights` rides along here rather than with the bank because it is
/// authored too: the bank says what a glyph looks like, the weights say which
/// parts of that description the matcher should believe.
pub fn language_tables(dir: &std::path::Path, classes: &[Class]) -> Result<Vec<Table>, String> {
    let words = crate::lexicon::expand(&crate::lexicon::load(dir)?);
    let ps = crate::params::load(dir)?;
    // An entry the identifier gate would suppress can never earn its bonus,
    // so it is not a harmless extra word: it is coverage that reads as real.
    // Checked against the shipped parameter values rather than the defaults,
    // because it is the shipped gate the entry will meet.
    let pval = |name: &str, fallback: f64| {
        ps.iter().find(|p| p.name == name).map_or(fallback, |p| p.value)
    };
    let dead = crate::lexicon::identifier_shaped(
        &words,
        pval("decode.identifier_min_length", 2.0) as u32,
        pval("decode.identifier_digit_fraction", 0.2) as f32,
    );
    if !dead.is_empty() {
        let shown: Vec<&str> = dead.iter().take(8).map(String::as_str).collect();
        return Err(format!(
            concat!(
                "lexicon.txt holds {} identifier-shaped word(s), e.g. {}: rule 6 suppresses ",
                "the lexicon inside identifier-shaped context, so these can never earn their ",
                "bonus and their presence makes the vocabulary look covered when it is not. ",
                "Remove them; do not soften the gate to make them work."
            ),
            dead.len(),
            shown.join(", ")
        ));
    }
    let dawg = crate::lexicon::build(&words, classes)?;
    let priors = crate::bigrams::load_priors(dir)?;
    let bigrams = crate::bigrams::build(&words, &priors, classes)?;
    let confusions = crate::confusions::from_model_dir(dir, classes)?;
    let params = crate::params::build(&ps)?;

    let mut out = vec![
        Table::opaque(T_LEXICON, vec![dawg.bytes.len() as u32], dawg.bytes),
        Table::opaque(T_BIGRAMS, vec![bigrams.bytes.len() as u32], bigrams.bytes),
        Table::opaque(T_CONFUSIONS, vec![confusions.bytes.len() as u32], confusions.bytes),
        Table::opaque(T_PARAMS, vec![params.bytes.len() as u32], params.bytes),
    ];
    // Optional on purpose: absent, every dimension weighs 1.0, which is what
    // a reader that predates the table already assumes. Present only when the
    // authored weights are not all 1.0, so the table's existence in a file is
    // itself the statement that the matcher has an opinion.
    if let Some(rows) = crate::weights::load(dir)? {
        if let Some(t) = crate::weights::build(&rows) {
            out.push(t);
        }
    }
    Ok(out)
}

/// Replaces a bank's standardised vectors with what a runtime would hold
/// after loading them from `int8` storage.
///
/// Int8 is a storage format only (`ARCHITECTURE.md` section 7): tables are
/// dequantised once at load and no kernel ever sees an integer. So the only
/// place quantisation error can show up is a difference in the answer, and
/// the only honest way to state it is to run the same queries against a bank
/// that has been through the round trip and count the disagreements.
pub fn quantise_in_place(bank: &mut Bank) {
    let n = bank.prototypes.len();
    let mut flat = Vec::with_capacity(n * FEATURE_DIMS);
    for s in &bank.standardised {
        flat.extend_from_slice(s);
    }
    let (q, scales) = ocrw::quantise(&flat, n, FEATURE_DIMS);
    let back = ocrw::dequantise(&q, &scales, n, FEATURE_DIMS);
    for (i, row) in bank.standardised.iter_mut().enumerate() {
        row.copy_from_slice(&back[i * FEATURE_DIMS..(i + 1) * FEATURE_DIMS]);
    }
}

/// The `meta` JSON block.
///
/// `sizes` is the px/em list the bank was built at, carried so a rebuild can
/// be reproduced from the file rather than from a memory of the command line.
///
/// # Panics
/// Panics unless `classes` is the **whole** charset in index order. A bank
/// may be built over a subset — a size sweep across four classes is a useful
/// measurement — but the emitted charset may not be, because class identity
/// in every table is the *position* in this list. A sparse charset beside
/// global class indices produces a file that loads and answers wrongly.
pub fn meta(bank: &Bank, classes: &[Class], sizes: &[f32]) -> String {
    for (i, c) in classes.iter().enumerate() {
        assert_eq!(
            usize::from(c.index),
            i,
            "meta must be written from the whole charset in index order"
        );
    }
    let mut s = String::new();
    s.push('{');
    s.push_str(&format!("\"feature_version\":{FEATURE_VERSION},"));
    s.push_str(&format!("\"feature_dims\":{FEATURE_DIMS},"));
    s.push_str("\"prototype_class_encoding\":\"u16le\",");
    s.push_str("\"class_holes_encoding\":\"u8 bitmask, bit n = n holes observed\",");
    s.push_str("\"prototype_face_encoding\":\"u16le, index into faces\",");
    s.push_str(&format!("\"prototypes\":{},", bank.prototypes.len()));

    s.push_str("\"sizes\":[");
    for (i, px) in sizes.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!("{px}"));
    }
    s.push_str("],");

    s.push_str("\"faces\":[");
    for (i, f) in bank.faces.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let d = match f.distribution {
            Distribution::Shippable => "shippable",
            Distribution::LocalOnly => "local-only",
            Distribution::Excluded => "excluded",
        };
        s.push_str(&format!(
            "{{\"family\":{},\"style\":{},\"distribution\":\"{d}\",\"licence\":{},\"licence_source\":{}}}",
            json_string(&f.family),
            json_string(&f.style),
            json_string(&f.licence),
            json_string(&f.licence_source)
        ));
    }
    s.push_str("],");

    s.push_str("\"charset\":");
    s.push_str(&charset_array_json(classes));

    // The identifier last, over everything above it, so it changes when
    // anything the file claims about itself changes.
    let id = crc32(s.as_bytes());
    s.push_str(&format!(",\"build_id\":\"{id:08x}\"}}"));
    s
}

/// The `meta.charset` array's JSON text, exactly as [`meta`] writes it: `[` +
/// one object per class in index order + `]`.
///
/// Pulled out of [`meta`] rather than reimplemented so this text and what a
/// file's `charset` field actually holds can never drift (`CLAUDE.md` rule
/// 4). This is also the byte string [`charset_sha256`] hashes: a trainer
/// building the `nn` table's weights (`ARCHITECTURE.md` section 11, the
/// 2026-09-25 chunk 15 interfaces entry) hashes the same text independently,
/// so the two sides can compare a hash rather than ship a charset twice.
pub fn charset_array_json(classes: &[Class]) -> String {
    let mut s = String::new();
    s.push('[');
    for (i, c) in classes.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"index\":{},\"cp\":{},\"category\":{},\"twin\":{}}}",
            c.index,
            c.codepoint as u32,
            json_string(&c.category),
            c.case_twin.map_or(-1i32, i32::from)
        ));
    }
    s.push(']');
    s
}

/// The digest an `nn`-table trainer's `spec.json` must carry as
/// `charset_sha256` for this build's charset. `ocrcer-build write --nn`
/// refuses the trainer output if the two disagree, rather than writing a
/// model whose network and whose prototype bank silently disagree about what
/// class index 137 means (`CLAUDE.md` rule 1's "a plausible-looking number is
/// not a number", applied to a whole table rather than one field of it).
pub fn charset_sha256(classes: &[Class]) -> String {
    crate::sha256::hex(charset_array_json(classes).as_bytes())
}

/// As [`meta`], but with an `"nn":{...}` block spliced in before `build_id`,
/// so the identifier changes when the network changes too. `nn_json` is the
/// complete `{...}` object [`crate::nn::meta_json`] builds; this function
/// does not interpret it.
///
/// # Panics
/// Same as [`meta`].
pub fn meta_with_nn(bank: &Bank, classes: &[Class], sizes: &[f32], nn_json: &str) -> String {
    let base = meta(bank, classes, sizes);
    // `meta` ends with `,"build_id":"xxxxxxxx"}`; splice the nn block in
    // just before that suffix, then recompute the identifier over the whole
    // thing so it still covers everything the file claims about itself.
    let cut = base.rfind(",\"build_id\":").expect("meta always ends with build_id");
    let mut s = base[..cut].to_string();
    s.push_str(",\"nn\":");
    s.push_str(nn_json);
    let id = crc32(s.as_bytes());
    s.push_str(&format!(",\"build_id\":\"{id:08x}\"}}"));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bank::{self, BankFace, Renderer};
    use crate::tables::{load_charset, model_dir};

    /// A bank over four classes, with the whole charset beside it: the bank
    /// may be a subset, the emitted charset may not be.
    fn tiny() -> (Bank, Vec<Class>) {
        let classes = load_charset(&model_dir()).unwrap();
        let subset: Vec<Class> =
            classes.iter().filter(|c| "oi8H".contains(c.codepoint)).cloned().collect();
        let faces = vec![BankFace {
            family: bank::AUTHORED_FAMILY.into(),
            style: "Regular".into(),
            distribution: Distribution::Shippable,
            licence: bank::AUTHORED_LICENCE.into(),
            licence_source: bank::AUTHORED_LICENCE_SOURCE.into(),
        }];
        let renderers = vec![Renderer::Authored(crate::face::glyphs::glyphs())];
        let b = bank::build(&subset, &faces, &renderers, &[16.0, 32.0]);
        (b, classes)
    }

    /// The identifier must be a function of the file's own claims, so that a
    /// rebuild of the same inputs reproduces it and a change to any input
    /// does not.
    #[test]
    fn the_build_id_follows_the_inputs_and_nothing_else() {
        let (b, classes) = tiny();
        assert_eq!(meta(&b, &classes, &[16.0, 32.0]), meta(&b, &classes, &[16.0, 32.0]));
        assert_ne!(meta(&b, &classes, &[16.0, 32.0]), meta(&b, &classes, &[16.0, 48.0]));
    }

    /// `meta` is parsed by the runtime at load; a stray control character or
    /// an unescaped quote in a font family name would make the file
    /// unreadable, and font names are not under this project's control.
    #[test]
    fn the_meta_block_is_balanced_and_carries_the_feature_version() {
        let (b, classes) = tiny();
        let m = meta(&b, &classes, &[16.0, 32.0]);
        assert!(m.starts_with('{') && m.ends_with('}'));
        assert!(m.contains(&format!("\"feature_version\":{FEATURE_VERSION}")));
        assert!(m.contains("\"build_id\":"));
        let braces = m.chars().filter(|&c| c == '{').count();
        assert_eq!(braces, m.chars().filter(|&c| c == '}').count());
        assert!(!m.chars().any(|c| (c as u32) < 0x20));
    }

    #[test]
    fn the_tables_are_shaped_as_the_reader_expects() {
        let (b, _classes) = tiny();
        let ts = tables(&b);
        let n = b.prototypes.len() as u32;
        assert!(n > 0);
        let p = ts.iter().find(|t| t.name == T_PROTOTYPES).unwrap();
        assert_eq!(p.dims, vec![n, FEATURE_DIMS as u32]);
        assert_eq!(p.scales.len(), FEATURE_DIMS);
        assert_eq!(p.data.len(), n as usize * FEATURE_DIMS);
        let c = ts.iter().find(|t| t.name == T_PROTOTYPE_CLASS).unwrap();
        assert_eq!(c.data.len(), n as usize * 2);
        let f = ts.iter().find(|t| t.name == T_FEATURE_NORM).unwrap();
        assert_eq!(f.data.len(), 2 * FEATURE_DIMS * 4);
        let pf = ts.iter().find(|t| t.name == T_PROTOTYPE_FACE).unwrap();
        assert_eq!(pf.data.len(), n as usize * 2);
    }
}
