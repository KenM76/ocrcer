//! The `nn` table's round trip: what `ocrcer-build` writes from a trainer
//! output directory, `ocrcer-core`'s `Model::load` reads back, within the
//! per-output-channel quantisation this crate applies. Beside
//! `roundtrip.rs`, which is the same gate for every other table.

use ocrcer_build::bank::{self, BankFace, Renderer};
use ocrcer_build::emit;
use ocrcer_build::ocrw;
use ocrcer_build::tables::{load_charset, model_dir, Class, Distribution};
use ocrcer_core::feature::FEATURE_VERSION;
use ocrcer_core::ocrw::Model;

fn tiny_bank() -> (bank::Bank, Vec<Class>, Vec<f32>) {
    let classes = load_charset(&model_dir()).unwrap();
    let subset: Vec<Class> = classes.iter().filter(|c| "oi8H".contains(c.codepoint)).cloned().collect();
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

/// A trainer output directory matching `docs/ARCHITECTURE.md` section 11's
/// chunk 15 interfaces entry, built against the given charset hash: one conv
/// layer, one relu, one dense layer, ending at `n_outputs = classes.len() +
/// 1` for the junk class.
fn write_trainer_dir(charset_sha256: &str, n_classes: usize) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);

    let junk_index = n_classes as u32;
    let n_outputs = junk_index + 1;
    let dir = std::env::temp_dir().join(format!("ocrcer-nn-roundtrip-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let spec = format!(
        r#"{{
            "nn_version":1,"junk_index":{junk_index},"n_outputs":{n_outputs},
            "layers":[
                {{"kind":"conv3x3","shape":[2,1,3,3]}},
                {{"kind":"relu"}},
                {{"kind":"dense","shape":[3,2]}}
            ],
            "charset_sha256":"{charset_sha256}","feature_extractor":{FEATURE_VERSION},
            "manifest_id":"nn-roundtrip-test","seed":7,
            "torch_version":"2.4.0","python_version":"3.11.9","lock_hash":"testlock"
        }}"#
    );
    std::fs::write(dir.join("spec.json"), spec).unwrap();

    let f32s = |path: &std::path::Path, values: &[f32]| {
        let mut bytes = Vec::new();
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(path, bytes).unwrap();
    };
    f32s(&dir.join("0.weight.f32"), &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, -1.0, -2.0, -3.0, -4.0, -5.0, -6.0, -7.0, -8.0, -9.0]);
    f32s(&dir.join("0.bias.f32"), &[0.5, -0.5]);
    f32s(&dir.join("2.weight.f32"), &[1.0, -2.0, 3.0, -4.0, 5.0, -6.0]);
    f32s(&dir.join("2.bias.f32"), &[1.0, 2.0, 3.0]);
    dir
}

#[test]
fn the_nn_table_round_trips_within_its_own_quantisation() {
    let (b, classes, sizes) = tiny_bank();
    let charset_hash = emit::charset_sha256(&classes);
    let trainer_dir = write_trainer_dir(&charset_hash, classes.len());

    let built = ocrcer_build::nn::build(&trainer_dir, &charset_hash, FEATURE_VERSION).unwrap();

    let mut tables = emit::tables(&b);
    tables.push(built.table);
    let meta = emit::meta_with_nn(&b, &classes, &sizes, &built.meta_json);

    let dir = std::env::temp_dir().join("ocrcer-nn-roundtrip-out");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("with_nn.ocrw");
    ocrw::write(&path, 1, 1, &meta, &tables).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    let m = Model::load(&bytes).expect("a file with a well-formed nn table must load");
    assert_eq!(m.nn_status, ocrcer_core::nn::NnStatus::Loaded);
    let nn = m.nn.expect("nn must be Some when nn_status is Loaded");
    assert_eq!(nn.junk_index, classes.len() as u32);
    assert_eq!(nn.n_outputs, classes.len() as u32 + 1);
    assert_eq!(nn.layers.len(), 3);

    // Layer 0 (conv3x3): row 0 used weights 1..9, row 1 used -1..-9. Check
    // the dequantised values land within that row's quantisation step of the
    // trainer's original f32 values -- the same bound
    // `crates/ocrcer-build/src/ocrw.rs`'s `quantise_per_row` test checks.
    let expect_row0: [f32; 9] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
    let expect_row1: [f32; 9] = [-1.0, -2.0, -3.0, -4.0, -5.0, -6.0, -7.0, -8.0, -9.0];
    let bound_row0 = expect_row0.iter().fold(0.0f32, |m, v| m.max(v.abs())) / 127.0 / 2.0;
    let bound_row1 = expect_row1.iter().fold(0.0f32, |m, v| m.max(v.abs())) / 127.0 / 2.0;
    for (i, want) in expect_row0.iter().enumerate() {
        assert!((nn.layers[0].weight[i] - want).abs() <= bound_row0 + 1e-6, "row0[{i}]: {} vs {want}", nn.layers[0].weight[i]);
    }
    for (i, want) in expect_row1.iter().enumerate() {
        let got = nn.layers[0].weight[9 + i];
        assert!((got - want).abs() <= bound_row1 + 1e-6, "row1[{i}]: {got} vs {want}");
    }
    assert_eq!(nn.layers[0].bias, vec![0.5, -0.5]);
    assert_eq!(nn.layers[2].bias, vec![1.0, 2.0, 3.0]);

    std::fs::remove_dir_all(&trainer_dir).ok();
    std::fs::remove_file(&path).ok();
}

/// A trainer directory built against a different charset must be refused at
/// build time, not shipped and discovered wrong later.
#[test]
fn a_trainer_directory_for_a_different_charset_is_refused() {
    let (_b, classes, _sizes) = tiny_bank();
    let real_hash = emit::charset_sha256(&classes);
    let wrong_dir = write_trainer_dir("0000000000000000000000000000000000000000000000000000000000000000", classes.len());
    let err = match ocrcer_build::nn::build(&wrong_dir, &real_hash, FEATURE_VERSION) {
        Ok(_) => panic!("expected an Err"),
        Err(e) => e,
    };
    assert!(err.contains("charset_sha256"), "{err}");
    std::fs::remove_dir_all(&wrong_dir).ok();
}

/// Without `--nn`, `ocrcer-build write`'s output is unaffected: `meta`
/// (chunk 3 `emit::meta`) never gains an `nn` key just because the crate now
/// knows how to write one.
#[test]
fn a_build_without_nn_carries_no_nn_key() {
    let (b, classes, sizes) = tiny_bank();
    let meta = emit::meta(&b, &classes, &sizes);
    let parsed = ocrcer_core::json::Json::parse(&meta).unwrap();
    assert!(parsed.get("nn").is_none());
}
