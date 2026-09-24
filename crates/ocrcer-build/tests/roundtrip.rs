//! The chunk 3 round-trip gate: what this crate writes, `ocrcer-core` reads,
//! and the two agree on every number.
//!
//! This is the only check that the writer and the reader — two halves of a
//! format spec, in two crates, with no shared code path — mean the same thing
//! by a table. Inspection cannot establish it; a round trip can.

use ocrcer_build::bank::{self, BankFace, Renderer};
use ocrcer_build::emit;
use ocrcer_build::ocrw::{self, Table};
use ocrcer_build::tables::{load_charset, model_dir, Class, Distribution};
use ocrcer_core::feature::{FEATURE_DIMS, FEATURE_VERSION};
use ocrcer_core::ocrw::{Container, Model};
use ocrcer_core::Error;

/// A bank small enough to build in a test, over classes chosen to span the
/// hole-count range: `o` has one hole, `8` has two, `i` has none, `H` has
/// none and a different aspect.
///
/// The bank is that subset; the returned charset is the whole thing, because
/// class identity in every table is a position in the emitted charset.
fn tiny_bank() -> (bank::Bank, Vec<Class>, Vec<f32>) {
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
    let renderers = vec![Renderer::Authored(ocrcer_build::face::glyphs::glyphs())];
    let sizes = vec![16.0f32, 32.0];
    let b = bank::build(&subset, &faces, &renderers, &sizes);
    (b, classes, sizes)
}

fn write_tiny(name: &str) -> (std::path::PathBuf, bank::Bank, Vec<Class>) {
    let (mut b, classes, sizes) = tiny_bank();
    let tables = emit::tables(&b);
    let meta = emit::meta(&b, &classes, &sizes);
    let dir = std::env::temp_dir().join("ocrcer-roundtrip");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    ocrw::write(&path, 1, 1, &meta, &tables).unwrap();
    // The reader dequantises; make the in-memory bank match what a loaded
    // file holds, so the comparison below is about the format and not about
    // int8.
    emit::quantise_in_place(&mut b);
    (path, b, classes)
}

#[test]
fn every_table_survives_the_trip_unchanged() {
    let (path, b, classes) = write_tiny("full.ocrw");
    let bytes = std::fs::read(&path).unwrap();
    let m = Model::load(&bytes).unwrap();

    assert_eq!(m.feature_version, FEATURE_VERSION);
    assert_eq!(m.n_prototypes(), b.prototypes.len());
    assert_eq!(m.classes.len(), classes.len());
    assert_eq!(m.sizes, vec![16.0, 32.0]);
    // Only four classes have prototypes, but every class index in the file
    // still names the charset entry at that position.
    for &c in &m.prototype_class {
        assert!("oi8H".contains(m.char_of(c).unwrap()));
    }

    for (i, c) in classes.iter().enumerate() {
        assert_eq!(m.classes[i].codepoint, c.codepoint);
        assert_eq!(m.classes[i].index, c.index);
        assert_eq!(m.classes[i].category, c.category);
        assert_eq!(m.classes[i].case_twin, c.case_twin);
    }

    assert_eq!(m.mean.as_slice(), b.mean.as_slice());
    for i in 0..FEATURE_DIMS {
        // The reader substitutes 1.0 for a non-positive sd; the builder is
        // supposed to have done the same, so they must already agree.
        assert_eq!(m.sd[i], b.sd[i], "sd disagrees at dimension {i}");
    }

    for (p, proto) in b.prototypes.iter().enumerate() {
        assert_eq!(m.prototype_class[p], proto.class);
        let row = &m.prototypes[p * FEATURE_DIMS..(p + 1) * FEATURE_DIMS];
        assert_eq!(row, b.standardised[p].as_slice(), "prototype {p} changed in the trip");
    }

    assert_eq!(m.class_holes.len(), b.gates.len());
    for (i, g) in b.gates.iter().enumerate() {
        assert_eq!(m.class_holes[i], g.holes);
    }

    // A standardised query computed from the file's constants must equal one
    // computed from the builder's. This is the guard against a runtime
    // measuring queries with a different ruler than the bank was measured
    // with, which is the silent wrong-answer mode section 7 exists to stop.
    let raw = b.prototypes[0].features;
    assert_eq!(m.standardise(&raw).as_slice(), b.standardise(&raw).as_slice());

    std::fs::remove_file(&path).ok();
}

/// Section 7's asymmetry: an unknown table name is dead bytes to an old
/// runtime, not a load failure.
#[test]
fn an_additive_table_costs_no_version_bump() {
    let (b, classes, sizes) = tiny_bank();
    let mut tables = emit::tables(&b);
    tables.push(Table::opaque("diagnostics_from_the_future", vec![5], vec![1, 2, 3, 4, 5]));
    let meta = emit::meta(&b, &classes, &sizes);
    let dir = std::env::temp_dir().join("ocrcer-roundtrip");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("additive.ocrw");
    ocrw::write(&path, 1, 1, &meta, &tables).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    let m = Model::load(&bytes).expect("an unknown table must not fail the load");
    assert_eq!(m.n_prototypes(), b.prototypes.len());

    let c = Container::load(&bytes).unwrap();
    assert_eq!(c.table("diagnostics_from_the_future").unwrap().data, &[1, 2, 3, 4, 5]);
    std::fs::remove_file(&path).ok();
}

/// The other half of the asymmetry: a version the reader does not know is
/// refused outright, because it means the tables it *does* know have changed
/// meaning.
#[test]
fn an_unknown_version_is_refused() {
    let (b, classes, sizes) = tiny_bank();
    let tables = emit::tables(&b);
    let meta = emit::meta(&b, &classes, &sizes);
    let dir = std::env::temp_dir().join("ocrcer-roundtrip");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("v2.ocrw");
    ocrw::write(&path, 2, 1, &meta, &tables).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(Model::load(&bytes).err(), Some(Error::UnsupportedVersion(2)));
    std::fs::remove_file(&path).ok();
}

/// A file built against a different feature-vector definition must be
/// refused, not read. Every prototype in it means something else.
#[test]
fn a_mismatched_feature_version_is_refused() {
    let (b, classes, sizes) = tiny_bank();
    let tables = emit::tables(&b);
    let meta = emit::meta(&b, &classes, &sizes)
        .replace(&format!("\"feature_version\":{FEATURE_VERSION}"), "\"feature_version\":99");
    let dir = std::env::temp_dir().join("ocrcer-roundtrip");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("featv.ocrw");
    ocrw::write(&path, 1, 1, &meta, &tables).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        Model::load(&bytes).err(),
        Some(Error::FeatureVersionMismatch { file: 99, runtime: FEATURE_VERSION })
    );
    std::fs::remove_file(&path).ok();
}

/// `meta.sizes` (the build size ladder `ocrcer-build write` was given,
/// ARCHITECTURE.md section 11, 2026-09-23) is optional and additive: a file
/// written before it existed must still load, and must report an unrecorded
/// ladder rather than an empty one -- those are different claims about the
/// bank, and only the reader's `sizes` field, not a version bump, is what
/// tells them apart.
#[test]
fn a_file_without_the_sizes_field_still_loads_with_an_empty_ladder() {
    let (b, classes, sizes) = tiny_bank();
    let tables = emit::tables(&b);
    let meta = emit::meta(&b, &classes, &sizes);
    let without_sizes = meta.replacen("\"sizes\":[16,32],", "", 1);
    assert_ne!(meta, without_sizes, "the sizes key must actually have been removed");

    let dir = std::env::temp_dir().join("ocrcer-roundtrip");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("no_sizes.ocrw");
    ocrw::write(&path, 1, 1, &without_sizes, &tables).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let m = Model::load(&bytes).expect("meta.sizes must be optional, not required to load");
    assert!(m.sizes.is_empty(), "a file that never wrote sizes must not fabricate a ladder");
    std::fs::remove_file(&path).ok();
}

/// A supplementary segment handed to the base-model loader is refused rather
/// than merged (section 7.1).
#[test]
fn a_segment_file_is_not_a_base_model() {
    let (b, classes, sizes) = tiny_bank();
    let tables = emit::tables(&b);
    let meta = emit::meta(&b, &classes, &sizes);
    let dir = std::env::temp_dir().join("ocrcer-roundtrip");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("segment.ocrw");
    ocrw::write(&path, 1, ocrcer_core::ocrw::KIND_SEGMENT, &meta, &tables).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(Model::load(&bytes).err(), Some(Error::UnsupportedModelKind(2)));
    std::fs::remove_file(&path).ok();
}

/// The emitted `meta` keys and table names are asserted against
/// `ARCHITECTURE.md` sections 2 and 7.1, which is the chunk 3 gate clause
/// that stops the file and the spec drifting apart unnoticed.
#[test]
fn the_emitted_keys_and_table_names_match_the_architecture() {
    let (b, classes, sizes) = tiny_bank();
    let meta_text = emit::meta(&b, &classes, &sizes);
    let tables = emit::tables(&b);

    let names: Vec<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            ocrcer_core::ocrw::T_PROTOTYPES,
            ocrcer_core::ocrw::T_PROTOTYPE_CLASS,
            ocrcer_core::ocrw::T_FEATURE_NORM,
            ocrcer_core::ocrw::T_CLASS_HOLES,
            ocrcer_core::ocrw::T_PROTOTYPE_FACE,
        ]
    );

    let meta = ocrcer_core::json::Json::parse(&meta_text).unwrap();
    for key in [
        "feature_version",
        "feature_dims",
        "prototype_class_encoding",
        "class_holes_encoding",
        "prototype_face_encoding",
        "prototypes",
        "sizes",
        "faces",
        "charset",
        "build_id",
    ] {
        assert!(meta.get(key).is_some(), "meta is missing the {key:?} key");
    }

    // Section 7.1: every face record carries family, style, distribution,
    // licence and licence source. The model file travels without this
    // repository, so an auditor holding only the file must be able to answer
    // what the prototypes were derived from and under what licence.
    let faces = meta.get("faces").unwrap().as_array().unwrap();
    assert!(!faces.is_empty());
    for f in faces {
        for key in ["family", "style", "distribution", "licence", "licence_source"] {
            assert!(f.get(key).is_some(), "a face record is missing {key:?}");
        }
    }
}
