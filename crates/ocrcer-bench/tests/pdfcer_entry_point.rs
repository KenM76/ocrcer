//! Proves `ocrcer_core::pipeline::Engine::recognize_bytes` — the entry point
//! shaped for `pdfcer`'s `OcrEngine::recognize(width, height, &[u8])` — is
//! exactly the existing `Engine::recognize(Gray)` path, on a real rendered
//! page through a real (if minimal) built model.
//!
//! Not a second implementation of anything (`CLAUDE.md` rule 4): it calls the
//! same `ocrcer_build::bank`/`emit`/`page` library code every other
//! `ocrcer-bench` binary calls, and `recognize_bytes` itself is a one-line
//! forward inside `ocrcer-core`. What this test adds is the proof that the
//! forward is exact and that a real page comes back non-empty with every
//! word carrying an in-range confidence — the shape `pdfcer`'s
//! `RecognizedWord` needs.
//!
//! One face, one size, one text block: this is a binding smoke test, not an
//! accuracy measurement. Chunks 5/6/8 own accuracy; this owns "the pdfcer
//! entry point is not a second, silently-diverging recogniser".

use ocrcer_build::{bank, corpus, emit, page, tables, ttf_load};
use ocrcer_core::pipeline::Engine;
use ocrcer_core::Gray;

#[test]
fn recognize_bytes_matches_recognize_on_a_rendered_page() {
    let dir = tables::model_dir();
    let classes = tables::load_charset(&dir).expect("charset");
    let entries = tables::load_fonts(&dir).expect("fonts.tsv");

    // The model: every shippable face, one render size. A single size is a
    // valid (if minimal) `.ocrw` — chunk 3's ladder is a tuning question, not
    // a loader requirement — and keeps this test's build cost to a fraction
    // of a real bank build.
    let px = [24.0f32];
    let fonts = bank::Fonts::load(&entries, false);
    let (faces, renderers, failed) = fonts.renderers();
    assert!(failed.is_empty(), "a shippable face failed to parse: {failed:?}");
    assert!(!renderers.is_empty(), "no shippable faces available to build from");
    let b = bank::build(&classes, &faces, &renderers, &px);

    let meta = emit::meta(&b, &classes, &px);
    let table_blobs = emit::tables(&b);
    let tmp = std::env::temp_dir()
        .join(format!("ocrcer-pdfcer-entry-point-smoke-{}.ocrw", std::process::id()));
    ocrcer_build::ocrw::write(&tmp, 1, 1, &meta, &table_blobs).expect("write model");
    let bytes = std::fs::read(&tmp).expect("read back the model just written");
    let _ = std::fs::remove_file(&tmp);

    let engine = Engine::from_bytes(&bytes).expect("a model this test just built must load");

    // One rendered page: the first eligible, parseable face in `fonts.tsv`,
    // the first ASCII corpus block, at the model's own render size — the
    // same size-collision consideration `PLAN.md` section 2 flags for
    // benchmark corpora, deliberately exploited here so the page is inside
    // this tiny bank's actual coverage rather than between its prototypes.
    let entry = entries
        .iter()
        .find(|e| e.distribution.usable(false) && e.file().is_some())
        .expect("at least one shippable, present face");
    let path = entry.file().expect("checked above");
    let face_bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let face = ttf_load::Face::parse(&face_bytes, 0).expect("face parses");
    let block = corpus::ASCII_BLOCKS.first().expect("at least one corpus block");
    let lines: Vec<String> = block.lines.iter().map(|s| (*s).to_string()).collect();
    let pg = page::render(&face, &lines, px[0]).expect("the page renders at the bank's own size");

    let via_gray = engine
        .recognize(Gray { width: pg.width, height: pg.height, data: &pg.grey })
        .expect("recognize");
    let via_bytes = engine
        .recognize_bytes(pg.width, pg.height, &pg.grey)
        .expect("recognize_bytes");

    assert_eq!(
        via_gray, via_bytes,
        "the pdfcer-shaped entry point must be exactly the existing pipeline path, not a second implementation"
    );
    assert!(
        !via_gray.is_empty(),
        "the page recognised nothing at all, which proves nothing about the entry point"
    );

    // The pdfcer hand-off contract (`CLAUDE.md` rule 5, ARCHITECTURE.md
    // §8.1): every word carries a calibrated confidence in range, never a
    // raw distance and never a placeholder constant.
    for w in &via_gray {
        assert!(
            (0.0..=1.0).contains(&w.confidence),
            "confidence out of range for {:?}: {}",
            w.text,
            w.confidence
        );
    }

    // Mismatched dimensions are refused, not read as garbage — the same
    // guard `ocrs`'s adapter applies before handing pdfcer's caller a
    // mismatched buffer.
    let bad = engine.recognize_bytes(pg.width, pg.height, &pg.grey[..pg.grey.len() - 1]);
    assert!(bad.is_err(), "a buffer shorter than width*height must be refused, not misread");
}
