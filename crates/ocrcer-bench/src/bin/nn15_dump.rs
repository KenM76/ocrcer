//! `nn15-dump`: the chunk 15 trainer data (`docs/ARCHITECTURE.md` section 11,
//! "Chunk 15 contract amended: the network learns to reject non-characters",
//! "Chunk 15's trainer may be Python", "Chunk 15 interfaces").
//!
//! Written fresh for chunk 15, not a copy of `bin/nn_dump.rs` (the throwaway
//! probe's dump, kept as evidence only). Every feature vector and grid this
//! binary writes still comes from `ocrcer_core::feature::extract_with_grid`
//! -- the same extractor the bank and the runtime matcher use (`CLAUDE.md`
//! rule 4) -- and every vector is standardised with the *loaded model's own*
//! `mean`/`sd` (`Model::standardise`, the `feature_norm` table) before it
//! reaches disk, so the Python trainer never normalises anything itself.
//!
//! # Subcommands
//!
//! - `list-stems <pages-train-dir> <stride>` -- prints `row_id\tstem` for
//!   every `stride`-th `multifinben-englishocr` **train**-split manifest row
//!   (manifest order, so a stride spans all 8 shards). Feeds
//!   `tools/nn/cluster_split.py`, which turns this list into the committed
//!   `bench/splits/nn15_page_split.tsv` fold file *before* `dump` runs.
//! - `dump <model.ocrw> <pages-train-dir> <out-dir> <split-file> <stride>`
//!   -- the actual data dump, three sources:
//! - `charset-hash <model.ocrw> <out-file>` -- writes the hex SHA-256
//!   `spec.json`'s `charset_sha256` field needs (`ARCHITECTURE.md` section 11,
//!   "Chunk 15 interfaces", item 1). Hashed over a canonical reconstruction of
//!   `meta.charset` -- `[{"index":I,"cp":C}, ...]` compact JSON, ascending by
//!   index, built from `Model::load`'s already-parsed `classes` field so this
//!   can never disagree with what the loader itself reads. **This exact
//!   canonicalisation is this dumper's own choice**, not read from the
//!   `c15-nn-table` writer (a parallel, unmerged branch this session cannot
//!   see) -- it must be checked against that writer's own hash at merge time,
//!   flagged in `tools/nn/README.md`.
//!
//! ## (a) Bank renders + damage
//!
//! Every class in `model/charset.tsv`, every shippable face
//! (`model/fonts.tsv`), the model's own size ladder, clean plus three seeded
//! damage variants (`augment`, unchanged from the probe's recipe -- see its
//! doc comment below for exactly what it does). An 80/20 split by a seeded
//! hash of `(class, face, size, variant)` -- `bank_train_*` / `bank_val_*`.
//!
//! ## (b) Real crops, cluster-disjoint internal split
//!
//! `multifinben-englishocr` **train**-split pages only, `stride`-th row of
//! manifest order, the same reconstruction `nn_dump.rs` established: rebuild
//! the engine's own binarize/deskew/re-binarize/underline-strip mask from
//! public calls, read `Engine::recognize_lines`'s already-decoded
//! `Line`/`Word`/`CharBox`es back against it, pair a truth line with a
//! decoded line by index only when word counts match and a truth word with a
//! decoded word only when character counts match (a selection-bias gate, not
//! an oracle -- see the reported qualify/total counts). Every qualifying
//! character is a positive; `split-file` (`stem -> 'A'|'B'`, built by
//! `tools/nn/cluster_split.py` from the *same* stem list `list-stems`
//! printed, clustered by near-duplicate word-5-gram shingle containment --
//! the method `tools/nnprobe/cluster_pages.py` used, reimplemented rather
//! than imported since that script is probe evidence) routes each page's
//! rows to `real_train_*` (fold A) or `real_val_*` (fold B). This is an
//! *internal* held-out split of the train pages for the trainer's own use --
//! `finfilings-val` and every fixture stay unread, per `CLAUDE.md` rule 1.
//!
//! ## (c) Junk negatives
//!
//! Two sources, per the amendment's item 2:
//!
//! - **(i) rendered-line candidates.** `ocrcer_bench::ident_corpus`'s
//!   authored blocks, rendered with `ocrcer_build::page::render` (the bank's
//!   own rasteriser) across every shippable face at `RENDER_SIZES`, run
//!   through the identical binarize/lines/words/segment pipeline stages
//!   (`ocrcer_core`'s public functions, the same ones `pipeline.rs` calls --
//!   never a second segmentation implementation). Every lattice edge whose
//!   `(x0, x1)` does not match any truth `PageGlyph`'s `(x, x+width)` within
//!   1px at *both* edges, on the same rendered line, is a negative.
//! - **(ii) non-path candidates inside count-matched real words.** For every
//!   qualifying real word from (b), the word's lattice is rebuilt from the
//!   same recomputed mask (`components::label/components`,
//!   `lines::group_with_bands`, `words::split_band_with`, located by
//!   x-extent overlap against the decoded `Word.rect`, then verified by
//!   exact tight-crop-box match against every `CharBox.rect` the engine
//!   actually decoded for that word -- a mismatch is skipped and counted,
//!   never guessed). Every edge whose cropped box is not one of those
//!   on-path boxes is a negative.
//!
//! Every negative also gets a `match::nearest(model, raw, 1, true)` call
//! (top-1 class, `Match::ratio()`), so the report can compute item 3's
//! "real-shape negatives" share: the fraction of negatives whose matcher
//! top-1 ratio sits *under* that class's real-positive median ratio (from
//! `real_train` only, classes with fewer than 5 positives excluded and
//! counted separately -- too few to name a stable median).
//!
//! # Output (`<out-dir>`, never committed -- `D:/Dev/ExcludedPrivate/...`)
//!
//! `classes.tsv`, `bank_{train,val}_{G,X,y,meta}.*`, `bank_val_top1.u16`,
//! `real_{train,val}_{G,X,y,top1,topk_class,topk_dist,meta}.*`,
//! `junk_render_{train,val}_{G,X,top1,ratio,meta}.*`,
//! `junk_nonpath_{train,val}_{G,X,top1,ratio,meta}.*`, `summary.json`.
//!
//! `G` is 32x32 `f32` row-major, `X` is 107 `f32` standardised, `y`/`top1`/
//! `topk_class` are `u16` class indices (`0xFFFF` sentinel), `topk_dist`/
//! `ratio` are `f32` (`+inf` sentinel where there was no match).
//!
//! This binary never opens `finfilings`, `finfilings-val`, `bench/pages-cov`
//! or any fixture (`CLAUDE.md` rule 1) -- `assert_train_dir_name` refuses to
//! run against a directory not named `*-train`.

use ocrcer_bench::{ident_corpus, splits};
use ocrcer_build::face::raster::Raster;
use ocrcer_build::{bank, page, tables};
use ocrcer_core::feature::{extract_with_grid, GlyphInput, FEATURE_DIMS};
use ocrcer_core::image::{binarize, components, deskew};
use ocrcer_core::layout::{lines, segment, underline, words};
use ocrcer_core::ocrw::Model;
use ocrcer_core::{r#match, Engine, Gray};

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const NONE_CLASS: u16 = 0xFFFF;
const TOPK: usize = 5;
/// Sizes rendered for junk source (i). A guess, not measured: bounded to
/// three points across the model's own range so the render-junk dump stays a
/// minutes-long step rather than repeating every bank size for every face.
const RENDER_SIZES: [f32; 3] = [16.0, 24.0, 32.0];
/// Item 3's median needs a population; fewer than this many `real_train`
/// positives for a class and its median is not reported.
const MIN_CLASS_POSITIVES: usize = 5;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("list-stems") => run_list_stems(&args.collect::<Vec<_>>()),
        Some("dump") => run_dump(&args.collect::<Vec<_>>()),
        Some("charset-hash") => run_charset_hash(&args.collect::<Vec<_>>()),
        _ => {
            eprintln!(
                "usage:\n  nn15-dump list-stems <pages-train-dir> <stride>\n  \
                 nn15-dump dump <model.ocrw> <pages-train-dir> <out-dir> <split-file> <stride>\n  \
                 nn15-dump charset-hash <model.ocrw> <out-file>"
            );
            ExitCode::FAILURE
        }
    }
}

fn run_charset_hash(args: &[String]) -> ExitCode {
    let Some(model_path) = args.first() else {
        return fail("need: <model.ocrw> <out-file>");
    };
    let Some(out_path) = args.get(1) else {
        return fail("need: <model.ocrw> <out-file>");
    };
    let bytes = match std::fs::read(model_path) {
        Ok(b) => b,
        Err(e) => return fail(&format!("reading {model_path}: {e}")),
    };
    let model = match Model::load(&bytes) {
        Ok(m) => m,
        Err(e) => return fail(&format!("loading {model_path}: {e}")),
    };
    let digest = charset_sha256_of(&model);
    if let Err(e) = std::fs::write(out_path, format!("{digest}\n")) {
        return fail(&format!("writing {out_path}: {e}"));
    }
    println!("{digest}");
    ExitCode::SUCCESS
}

/// `spec.json`'s `charset_sha256` (`ARCHITECTURE.md` section 11, "Chunk 15
/// interfaces", item 1) -- SHA-256 over a canonical reconstruction of
/// `meta.charset` as the loaded model actually stores it: `[{"index":I,
/// "cp":C}, ...]` compact JSON, ascending by index, built from `Model::load`'s
/// already-parsed `classes` field. See the module doc comment's caveat: this
/// canonicalisation is this dumper's own choice, not read from the
/// `c15-nn-table` writer, and must be cross-checked against that writer's own
/// hash at merge time.
fn charset_sha256_of(model: &Model) -> String {
    let mut classes: Vec<&ocrcer_core::ocrw::Class> = model.classes.iter().collect();
    classes.sort_by_key(|c| c.index);
    let mut canonical = String::from("[");
    for (i, c) in classes.iter().enumerate() {
        if i > 0 {
            canonical.push(',');
        }
        canonical.push_str(&format!("{{\"index\":{},\"cp\":{}}}", c.index, c.codepoint as u32));
    }
    canonical.push(']');
    sha256_hex(canonical.as_bytes())
}

/// SHA-256 (FIPS 180-4), written from scratch rather than adding a crate
/// dependency for one hash call -- consistent with `splits.rs`'s own
/// from-scratch FNV-1a/fmix64 rather than pulling in a hashing crate.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = String::with_capacity(64);
    for word in h {
        out.push_str(&format!("{word:08x}"));
    }
    out
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("nn15-dump: {msg}");
    ExitCode::FAILURE
}

// ---------------------------------------------------------------------
// list-stems
// ---------------------------------------------------------------------

fn run_list_stems(args: &[String]) -> ExitCode {
    let Some(pages_dir) = args.first().map(PathBuf::from) else {
        return fail("need a pages-train-dir");
    };
    let stride: usize = args.get(1).and_then(|s| s.parse().ok()).filter(|&n| n >= 1).unwrap_or(1);
    if let Err(e) = assert_train_dir_name(&pages_dir) {
        return fail(&e);
    }
    let stems = match train_stems(stride) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    let out = std::io::stdout();
    let mut w = BufWriter::new(out.lock());
    for (row_id, stem) in &stems {
        if writeln!(w, "{row_id}\t{stem}").is_err() {
            return fail("writing stdout");
        }
    }
    ExitCode::SUCCESS
}

/// The `stride`-th `multifinben-englishocr` train-split manifest rows, in
/// manifest order (grouped by shard, so a stride spans all 8 shards rather
/// than biasing toward the first). Shared by `list-stems` and `dump` so the
/// two can never sample a different page set.
fn train_stems(stride: usize) -> Result<Vec<(String, String)>, String> {
    let manifest_path = ocrcer_bench::default_splits_root().join("manifest.tsv");
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("reading {}: {e}", manifest_path.display()))?;
    let manifest = splits::parse_manifest_tsv(&manifest_text)?;
    let all_train_rows: Vec<&splits::ManifestRow> = manifest
        .iter()
        .filter(|r| r.dataset == splits::MULTIFINBEN_DATASET && r.split == splits::Split::Train)
        .collect();
    if all_train_rows.is_empty() {
        return Err("no multifinben-englishocr train rows in the manifest".into());
    }
    let mut out = Vec::new();
    for row in all_train_rows.into_iter().step_by(stride.max(1)) {
        splits::assert_fittable(&manifest, splits::MULTIFINBEN_DATASET, &row.row_id)?;
        let (shard, row_num) = split_row_id(&row.row_id)?;
        let shard_idx = shard_index(&shard)?;
        let stem = format!("filing__s{shard_idx}__r{row_num:06}");
        out.push((row.row_id.clone(), stem));
    }
    Ok(out)
}

/// Same firewall `count_text.rs`/`nn_dump.rs` enforce.
fn assert_train_dir_name(dir: &Path) -> Result<(), String> {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: cannot read a directory name", dir.display()))?;
    if name.ends_with("-train") {
        Ok(())
    } else {
        Err(format!(
            "{}: refusing to run against a directory not named `*-train`",
            dir.display()
        ))
    }
}

fn split_row_id(row_id: &str) -> Result<(String, u32), String> {
    let (shard, row) =
        row_id.split_once('#').ok_or_else(|| format!("row id {row_id:?} has no '#'"))?;
    let row_num: u32 = row.parse().map_err(|_| format!("bad row number in {row_id:?}"))?;
    Ok((shard.to_string(), row_num))
}

fn shard_index(shard: &str) -> Result<u32, String> {
    let after_dash =
        shard.split('-').nth(1).ok_or_else(|| format!("shard name {shard:?} has no '-NNNNN-' segment"))?;
    after_dash.parse().map_err(|_| format!("bad shard index in {shard:?}"))
}

fn load_fold_file(path: &Path) -> Result<BTreeMap<String, char>, String> {
    let f = File::open(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut out = BTreeMap::new();
    for line in BufReader::new(f).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            continue;
        }
        let fold = cols[1].chars().next().ok_or_else(|| format!("bad fold in {line:?}"))?;
        out.insert(cols[0].to_string(), fold);
    }
    Ok(out)
}

// ---------------------------------------------------------------------
// dump
// ---------------------------------------------------------------------

fn run_dump(args: &[String]) -> ExitCode {
    if args.len() < 5 {
        return fail("need: <model.ocrw> <pages-train-dir> <out-dir> <split-file> <stride>");
    }
    let model_path = PathBuf::from(&args[0]);
    let pages_dir = PathBuf::from(&args[1]);
    let out_dir = PathBuf::from(&args[2]);
    let split_file = PathBuf::from(&args[3]);
    let stride: usize = args[4].parse().unwrap_or(1).max(1);

    if let Err(e) = assert_train_dir_name(&pages_dir) {
        return fail(&e);
    }
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        return fail(&format!("creating {}: {e}", out_dir.display()));
    }

    let model_bytes = match std::fs::read(&model_path) {
        Ok(b) => b,
        Err(e) => return fail(&format!("reading {}: {e}", model_path.display())),
    };
    let model = match Model::load(&model_bytes) {
        Ok(m) => m,
        Err(e) => return fail(&format!("loading {}: {e}", model_path.display())),
    };
    let engine = match Engine::from_bytes(&model_bytes) {
        Ok(e) => e,
        Err(e) => return fail(&format!("loading engine from {}: {e}", model_path.display())),
    };

    let model_dir = tables::model_dir();
    let classes = match tables::load_charset(&model_dir) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    if let Err(e) = write_classes_tsv(&out_dir.join("classes.tsv"), &classes) {
        return fail(&e);
    }
    // `spec.json` needs `feature_extractor` (the model's own `feature_version`)
    // and `charset_sha256` (item 1); write both once here so `tools/nn/train.py`
    // reads one small file instead of re-deriving either from the `.ocrw` bytes
    // itself -- the model stays the single source both this dumper and the
    // trainer read from, never re-parsed twice with different logic.
    let model_meta = format!(
        "{{\n  \"feature_extractor\": {},\n  \"charset_sha256\": \"{}\",\n  \"build_id\": \"{}\",\n  \"n_classes\": {}\n}}\n",
        model.feature_version,
        charset_sha256_of(&model),
        model.build_id.replace('\\', "\\\\").replace('"', "\\\""),
        model.classes.len(),
    );
    if let Err(e) = std::fs::write(out_dir.join("model_meta.json"), &model_meta) {
        return fail(&format!("writing model_meta.json: {e}"));
    }
    let mut class_of_char: BTreeMap<char, u16> = BTreeMap::new();
    for c in &classes {
        class_of_char.insert(c.codepoint, c.index);
    }

    let fold_of_stem = match load_fold_file(&split_file) {
        Ok(m) => m,
        Err(e) => return fail(&e),
    };

    eprintln!("nn15-dump: (a) bank renders + damage");
    let bank_summary = match dump_bank(&model, &classes, &out_dir) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };

    eprintln!("nn15-dump: (b) real crops + (c)(ii) non-path negatives");
    let (real_summary, nonpath_summary, class_ratios) = match dump_real_and_nonpath(
        &engine,
        &model,
        &class_of_char,
        &pages_dir,
        &out_dir,
        stride,
        &fold_of_stem,
    ) {
        Ok(x) => x,
        Err(e) => return fail(&e),
    };

    eprintln!("nn15-dump: (c)(i) rendered-line negatives");
    let render_summary = match dump_render_junk(&engine, &model, &out_dir) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };

    let medians = class_medians(&class_ratios);
    let nonpath_share = real_shape_share(&nonpath_summary.rows, &medians);
    let render_share = real_shape_share(&render_summary.rows, &medians);

    let summary = format!(
        "{{\n  \"bank_train_rows\": {},\n  \"bank_val_rows\": {},\n  \"bank_faces\": {},\n  \
         \"bank_sizes\": {:?},\n  \"page_stride\": {},\n  \"pages_read\": {},\n  \
         \"lines_total\": {},\n  \"lines_qualifying\": {},\n  \"words_total\": {},\n  \
         \"words_qualifying\": {},\n  \"chars_dumped\": {},\n  \"chars_out_of_charset\": {},\n  \
         \"chars_bad_crop\": {},\n  \"real_train_rows\": {},\n  \"real_val_rows\": {},\n  \
         \"matcher_topk\": {},\n  \
         \"junk_nonpath_train\": {},\n  \"junk_nonpath_val\": {},\n  \
         \"junk_nonpath_lattice_rebuild_mismatches\": {},\n  \
         \"junk_nonpath_real_shape_share\": {:.4},\n  \
         \"junk_nonpath_real_shape_excluded_no_median\": {},\n  \
         \"junk_render_pages\": {},\n  \"junk_render_train\": {},\n  \"junk_render_val\": {},\n  \
         \"junk_render_real_shape_share\": {:.4},\n  \
         \"junk_render_real_shape_excluded_no_median\": {},\n  \
         \"classes_with_median\": {}\n}}\n",
        bank_summary.train_rows,
        bank_summary.val_rows,
        bank_summary.faces,
        bank_summary.sizes,
        stride,
        real_summary.pages_read,
        real_summary.lines_total,
        real_summary.lines_qualifying,
        real_summary.words_total,
        real_summary.words_qualifying,
        real_summary.chars_dumped,
        real_summary.chars_out_of_charset,
        real_summary.chars_bad_crop,
        real_summary.train_rows,
        real_summary.val_rows,
        TOPK,
        nonpath_summary.train_rows,
        nonpath_summary.val_rows,
        nonpath_summary.mismatches,
        nonpath_share.share,
        nonpath_share.excluded,
        render_summary.pages,
        render_summary.train_rows,
        render_summary.val_rows,
        render_share.share,
        render_share.excluded,
        medians.len(),
    );
    if let Err(e) = std::fs::write(out_dir.join("summary.json"), &summary) {
        return fail(&format!("writing summary.json: {e}"));
    }
    print!("{summary}");
    ExitCode::SUCCESS
}

fn write_classes_tsv(path: &Path, classes: &[tables::Class]) -> Result<(), String> {
    let mut f = BufWriter::new(
        File::create(path).map_err(|e| format!("creating {}: {e}", path.display()))?,
    );
    writeln!(f, "index\tcodepoint_u32\tcategory").map_err(|e| e.to_string())?;
    for c in classes {
        writeln!(f, "{}\t{}\t{}", c.index, c.codepoint as u32, c.category).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------------
// (a) Bank renders -- unchanged recipe from the probe (`nn_dump.rs`), which
// this binary's module doc credits rather than silently re-deriving. Only
// the italic-fix duplicate matcher column is dropped: chunk 15 has no use
// for the pre-fix column the probe needed to show its own regression.
// ---------------------------------------------------------------------

struct BankSummary {
    train_rows: u64,
    val_rows: u64,
    faces: usize,
    sizes: Vec<f32>,
}

struct Sink3 {
    g: BufWriter<File>,
    x: BufWriter<File>,
    y: BufWriter<File>,
    meta: BufWriter<File>,
}

impl Sink3 {
    fn open(dir: &Path, prefix: &str) -> Result<Sink3, String> {
        let open = |name: &str| -> Result<BufWriter<File>, String> {
            let p = dir.join(format!("{prefix}_{name}"));
            File::create(&p).map(BufWriter::new).map_err(|e| format!("creating {}: {e}", p.display()))
        };
        Ok(Sink3 { g: open("G.f32")?, x: open("X.f32")?, y: open("y.u16")?, meta: open("meta.tsv")? })
    }

    fn write(
        &mut self,
        grid: &[[f32; 32]; 32],
        xv: &[f32; FEATURE_DIMS],
        y: u16,
        meta_row: &str,
    ) -> Result<(), String> {
        for row in grid {
            for v in row {
                self.g.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
            }
        }
        for v in xv {
            self.x.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
        }
        self.y.write_all(&y.to_le_bytes()).map_err(|e| e.to_string())?;
        writeln!(self.meta, "{meta_row}").map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn dump_bank(model: &Model, classes: &[tables::Class], out_dir: &Path) -> Result<BankSummary, String> {
    let entries = tables::load_fonts(&tables::model_dir())?;
    let fonts = bank::Fonts::load(&entries, false);
    let (faces, renderers, failed) = fonts.renderers();
    for line in &failed {
        eprintln!("nn15-dump: face failed to parse: {line}");
    }

    let sizes: Vec<f32> =
        if model.sizes.is_empty() { vec![16.0, 20.0, 24.0, 32.0, 48.0] } else { model.sizes.clone() };
    let variants: [u32; 4] = [0, 1, 2, 3];

    let mut train = Sink3::open(out_dir, "bank_train")?;
    let mut val = Sink3::open(out_dir, "bank_val")?;
    let mut train_rows = 0u64;
    let mut val_rows = 0u64;
    let mut val_top1 = BufWriter::new(
        File::create(out_dir.join("bank_val_top1.u16"))
            .map_err(|e| format!("creating bank_val_top1.u16: {e}"))?,
    );

    for (fi, renderer) in renderers.iter().enumerate() {
        for class in classes {
            for &px in &sizes {
                let Some(r) = renderer.render(class.codepoint, px) else { continue };
                let Some(x_height) = renderer.x_height_px(px).filter(|x| *x > 0.0) else { continue };
                for &variant in &variants {
                    let seed = sample_seed(class.index, fi as u32, px, variant);
                    let (ink, width, height, baseline_dy) = augment(&r, variant, seed);
                    if width == 0 || height == 0 {
                        continue;
                    }
                    let input = GlyphInput { ink: &ink, width, height, baseline_dy, x_height };
                    let (raw, grid) = extract_with_grid(&input);
                    let xv = model.standardise(&raw);
                    let meta = format!(
                        "{}\t{}\t{}\t{}\t{}\t{}",
                        class.index, fi, faces[fi].family, faces[fi].style, px as u32, variant
                    );
                    let is_val = split_fraction(seed) < 0.2;
                    if is_val {
                        val.write(&grid, &xv, class.index, &meta)?;
                        let m1 = r#match::nearest(model, &raw, 1, true)
                            .and_then(|m| m.top())
                            .map_or(NONE_CLASS, |c| c.class);
                        val_top1
                            .write_all(&m1.to_le_bytes())
                            .map_err(|e| format!("writing bank_val_top1.u16: {e}"))?;
                        val_rows += 1;
                    } else {
                        train.write(&grid, &xv, class.index, &meta)?;
                        train_rows += 1;
                    }
                }
            }
        }
    }

    Ok(BankSummary { train_rows, val_rows, faces: faces.len(), sizes })
}

fn split_fraction(seed: u64) -> f64 {
    fmix64(seed) as f64 / u64::MAX as f64
}

fn sample_seed(class: u16, face: u32, px: f32, variant: u32) -> u64 {
    let key = format!("{class}\u{1f}{face}\u{1f}{}\u{1f}{variant}", px.to_bits());
    fmix64(fnv1a64(key.as_bytes()))
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51afd7ed558ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ceb9fe1a85ec53);
    k ^= k >> 33;
    k
}

/// Deterministic scan-damage augmentation. Reused unchanged from the probe's
/// dump (`nn_dump.rs`, "Deterministic scan-damage augmentation" doc comment)
/// -- the recipe is evidence-tested there and re-deriving it here would be
/// exactly the drift `CLAUDE.md` rule 4 warns against for anything that
/// stands in for a pipeline stage.
fn augment(r: &Raster, variant: u32, seed: u64) -> (Vec<u8>, u32, u32, f32) {
    if variant == 0 {
        return (r.ink.clone(), r.width, r.height, r.baseline_dy);
    }
    let pad: u32 = 2;
    let w2 = r.width + 2 * pad;
    let h2 = r.height + 2 * pad;
    let mut canvas = vec![0f64; (w2 * h2) as usize];
    for y in 0..r.height {
        for x in 0..r.width {
            let v = r.ink[(y * r.width + x) as usize];
            canvas[((y + pad) * w2 + (x + pad)) as usize] = if v != 0 { 1.0 } else { 0.0 };
        }
    }

    let mut st = seed | 1;
    let mut next = move || -> f64 { (xorshift64star(&mut st) as f64) / (u64::MAX as f64) };

    let shift_amt = if variant == 1 { 0.3 } else { 0.4 };
    let dx = (next() - 0.5) * shift_amt;
    let dy = (next() - 0.5) * shift_amt;
    let shifted = bilinear_shift(&canvas, w2, h2, dx, dy);

    let blur_amt = match variant {
        1 => 0.5,
        2 => 0.65,
        _ => 0.25,
    };
    let blurred = box_blur3(&shifted, w2, h2);
    let mut mixed: Vec<f64> =
        shifted.iter().zip(blurred.iter()).map(|(&a, &b)| a * (1.0 - blur_amt) + b * blur_amt).collect();

    let sigma = match variant {
        1 => 0.18,
        2 => 0.12,
        _ => 0.15,
    };
    for v in mixed.iter_mut() {
        let n = (next() + next() + next() + next() - 2.0) * sigma;
        *v = (*v + n).clamp(0.0, 1.0);
    }

    let thresh = 0.5 + (next() - 0.5) * 0.24;
    let mut binmask: Vec<u8> = mixed.iter().map(|&v| u8::from(v >= thresh)).collect();

    let morph_roll = next();
    if variant == 3 || morph_roll < 0.35 {
        binmask = if next() < 0.5 { erode3(&binmask, w2, h2) } else { dilate3(&binmask, w2, h2) };
    }

    (binmask, w2, h2, r.baseline_dy + pad as f32)
}

fn xorshift64star(state: &mut u64) -> u64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

fn bilinear_shift(canvas: &[f64], w: u32, h: u32, dx: f64, dy: f64) -> Vec<f64> {
    let mut out = vec![0f64; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let sx = x as f64 - dx;
            let sy = y as f64 - dy;
            out[(y * w + x) as usize] = bilinear_sample(canvas, w, h, sx, sy);
        }
    }
    out
}

fn bilinear_sample(canvas: &[f64], w: u32, h: u32, x: f64, y: f64) -> f64 {
    let x0 = x.floor();
    let y0 = y.floor();
    let tx = x - x0;
    let ty = y - y0;
    let get = |xi: i64, yi: i64| -> f64 {
        if xi < 0 || yi < 0 || xi as u32 >= w || yi as u32 >= h {
            0.0
        } else {
            canvas[(yi as u32 * w + xi as u32) as usize]
        }
    };
    let x0i = x0 as i64;
    let y0i = y0 as i64;
    let a = get(x0i, y0i);
    let b = get(x0i + 1, y0i);
    let c = get(x0i, y0i + 1);
    let d = get(x0i + 1, y0i + 1);
    let top = a * (1.0 - tx) + b * tx;
    let bot = c * (1.0 - tx) + d * tx;
    top * (1.0 - ty) + bot * ty
}

fn box_blur3(canvas: &[f64], w: u32, h: u32) -> Vec<f64> {
    let get = |x: i64, y: i64| -> f64 {
        if x < 0 || y < 0 || x as u32 >= w || y as u32 >= h {
            0.0
        } else {
            canvas[(y as u32 * w + x as u32) as usize]
        }
    };
    let mut out = vec![0f64; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            for oy in -1i64..=1 {
                for ox in -1i64..=1 {
                    sum += get(x as i64 + ox, y as i64 + oy);
                }
            }
            out[(y * w + x) as usize] = sum / 9.0;
        }
    }
    out
}

fn erode3(mask: &[u8], w: u32, h: u32) -> Vec<u8> {
    morph3(mask, w, h, true)
}

fn dilate3(mask: &[u8], w: u32, h: u32) -> Vec<u8> {
    morph3(mask, w, h, false)
}

fn morph3(mask: &[u8], w: u32, h: u32, erode: bool) -> Vec<u8> {
    let get = |x: i64, y: i64| -> u8 {
        if x < 0 || y < 0 || x as u32 >= w || y as u32 >= h {
            0
        } else {
            mask[(y as u32 * w + x as u32) as usize]
        }
    };
    let mut out = vec![0u8; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut all1 = true;
            let mut any1 = false;
            for oy in -1i64..=1 {
                for ox in -1i64..=1 {
                    let v = get(x as i64 + ox, y as i64 + oy);
                    all1 &= v != 0;
                    any1 |= v != 0;
                }
            }
            out[(y * w + x) as usize] = u8::from(if erode { all1 } else { any1 });
        }
    }
    out
}

// ---------------------------------------------------------------------
// (b) Real crops, internal cluster-disjoint split, and (c)(ii) non-path
// negatives -- one page pass, one recomputed mask, since both need it.
// ---------------------------------------------------------------------

struct RealSummary {
    pages_read: u64,
    lines_total: u64,
    lines_qualifying: u64,
    words_total: u64,
    words_qualifying: u64,
    chars_dumped: u64,
    chars_out_of_charset: u64,
    chars_bad_crop: u64,
    train_rows: u64,
    val_rows: u64,
}

/// One negative row's matcher verdict, kept in memory for the item-3 metric
/// (computed once all `real_train` per-class medians are known).
struct NegRow {
    top1_class: u16,
    ratio: f32,
}

struct NonpathSummary {
    train_rows: u64,
    val_rows: u64,
    /// Word rebuilds whose on-path box set didn't exactly match the
    /// engine's own decoded `CharBox.rect`s -- skipped, not guessed at.
    mismatches: u64,
    rows: Vec<NegRow>,
}

struct SinkReal {
    g: BufWriter<File>,
    x: BufWriter<File>,
    y: BufWriter<File>,
    top1: BufWriter<File>,
    topk_class: BufWriter<File>,
    topk_dist: BufWriter<File>,
    meta: BufWriter<File>,
}

impl SinkReal {
    fn open(dir: &Path, prefix: &str) -> Result<SinkReal, String> {
        let open = |name: &str| -> Result<BufWriter<File>, String> {
            let p = dir.join(format!("{prefix}_{name}"));
            File::create(&p).map(BufWriter::new).map_err(|e| format!("creating {}: {e}", p.display()))
        };
        Ok(SinkReal {
            g: open("G.f32")?,
            x: open("X.f32")?,
            y: open("y.u16")?,
            top1: open("top1.u16")?,
            topk_class: open("topk_class.u16")?,
            topk_dist: open("topk_dist.f32")?,
            meta: open("meta.tsv")?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn write(
        &mut self,
        grid: &[[f32; 32]; 32],
        xv: &[f32; FEATURE_DIMS],
        y: u16,
        topk_class: &[u16; TOPK],
        topk_dist: &[f32; TOPK],
        meta_row: &str,
    ) -> Result<(), String> {
        for row in grid {
            for v in row {
                self.g.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
            }
        }
        for v in xv {
            self.x.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
        }
        self.y.write_all(&y.to_le_bytes()).map_err(|e| e.to_string())?;
        self.top1.write_all(&topk_class[0].to_le_bytes()).map_err(|e| e.to_string())?;
        for &c in topk_class {
            self.topk_class.write_all(&c.to_le_bytes()).map_err(|e| e.to_string())?;
        }
        for &d in topk_dist {
            self.topk_dist.write_all(&d.to_le_bytes()).map_err(|e| e.to_string())?;
        }
        writeln!(self.meta, "{meta_row}").map_err(|e| e.to_string())?;
        Ok(())
    }
}

struct SinkJunk {
    g: BufWriter<File>,
    x: BufWriter<File>,
    top1: BufWriter<File>,
    ratio: BufWriter<File>,
    meta: BufWriter<File>,
}

impl SinkJunk {
    fn open(dir: &Path, prefix: &str) -> Result<SinkJunk, String> {
        let open = |name: &str| -> Result<BufWriter<File>, String> {
            let p = dir.join(format!("{prefix}_{name}"));
            File::create(&p).map(BufWriter::new).map_err(|e| format!("creating {}: {e}", p.display()))
        };
        Ok(SinkJunk {
            g: open("G.f32")?,
            x: open("X.f32")?,
            top1: open("top1.u16")?,
            ratio: open("ratio.f32")?,
            meta: open("meta.tsv")?,
        })
    }

    fn write(
        &mut self,
        grid: &[[f32; 32]; 32],
        xv: &[f32; FEATURE_DIMS],
        top1: u16,
        ratio: f32,
        meta_row: &str,
    ) -> Result<(), String> {
        for row in grid {
            for v in row {
                self.g.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
            }
        }
        for v in xv {
            self.x.write_all(&v.to_le_bytes()).map_err(|e| e.to_string())?;
        }
        self.top1.write_all(&top1.to_le_bytes()).map_err(|e| e.to_string())?;
        self.ratio.write_all(&ratio.to_le_bytes()).map_err(|e| e.to_string())?;
        writeln!(self.meta, "{meta_row}").map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn dump_real_and_nonpath(
    engine: &Engine,
    model: &Model,
    class_of_char: &BTreeMap<char, u16>,
    pages_dir: &Path,
    out_dir: &Path,
    stride: usize,
    fold_of_stem: &BTreeMap<String, char>,
) -> Result<(RealSummary, NonpathSummary, BTreeMap<u16, Vec<f32>>), String> {
    let train_rows_list = train_stems(stride)?;

    let mut real_train = SinkReal::open(out_dir, "real_train")?;
    let mut real_val = SinkReal::open(out_dir, "real_val")?;
    let mut nonpath_train = SinkJunk::open(out_dir, "junk_nonpath_train")?;
    let mut nonpath_val = SinkJunk::open(out_dir, "junk_nonpath_val")?;

    let mut s = RealSummary {
        pages_read: 0,
        lines_total: 0,
        lines_qualifying: 0,
        words_total: 0,
        words_qualifying: 0,
        chars_dumped: 0,
        chars_out_of_charset: 0,
        chars_bad_crop: 0,
        train_rows: 0,
        val_rows: 0,
    };
    let mut nonpath_train_rows = 0u64;
    let mut nonpath_val_rows = 0u64;
    let mut nonpath_mismatches = 0u64;
    let mut nonpath_negrows: Vec<NegRow> = Vec::new();
    let mut class_ratios: BTreeMap<u16, Vec<f32>> = BTreeMap::new();

    let p = engine.params();

    for (row_id, stem) in &train_rows_list {
        let Some(&fold) = fold_of_stem.get(stem) else {
            eprintln!("nn15-dump: {stem}: not in split file, skipping");
            continue;
        };
        let is_val_fold = fold == 'B';

        let pgm_path = pages_dir.join(format!("{stem}.pgm"));
        let truth_path = pages_dir.join(format!("{stem}.truth.json"));
        let pgm_bytes = match std::fs::read(&pgm_path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("nn15-dump: skipping {}: {e}", pgm_path.display());
                continue;
            }
        };
        let (width, height, data) = page::from_pgm(&pgm_bytes)?;
        let truth_bytes = std::fs::read(&truth_path)
            .map_err(|e| format!("reading {}: {e}", truth_path.display()))?;
        let truth: serde_json::Value = serde_json::from_slice(&truth_bytes)
            .map_err(|e| format!("parsing {}: {e}", truth_path.display()))?;
        let truth_lines = match truth.get("lines").and_then(|v| v.as_array()) {
            Some(a) => a,
            None => continue,
        };

        let gray = Gray { width, height, data: &data };
        let decoded = match engine.recognize_lines(gray) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("nn15-dump: {}: recognize_lines failed: {e}", pgm_path.display());
                continue;
            }
        };
        s.pages_read += 1;

        // Recompute the mask (and, for (ii), the full intermediate
        // segmentation state) the engine's own `read_word` used, from the
        // same public calls `recognize_lines` itself makes, in the same
        // order -- never a second implementation of any of these stages.
        let mask0 = binarize::binarize_with(&Gray { width, height, data: &data }, &p.binarize());
        let slope = deskew::estimate_with(&mask0, width, height, f64::from(p.deskew.max_slope));
        let page_deskewed = deskew::correct_with(
            &Gray { width, height, data: &data },
            slope,
            f64::from(p.deskew.min_corrected_slope),
        );
        let gray2 = page_deskewed.gray();
        let mut mask = binarize::binarize_with(&gray2, &p.binarize());
        let line_p = p.lines();
        let (labels, comps) = if line_p.underline_strip {
            let stripped =
                underline::strip_underlines(&mut mask, page_deskewed.width, page_deskewed.height, &line_p);
            (stripped.labels, stripped.components)
        } else {
            let (labels, count) = components::label(
                &mask,
                page_deskewed.width,
                page_deskewed.height,
                components::Connectivity::Eight,
            );
            let comps = components::components(&labels, page_deskewed.width, page_deskewed.height, count);
            (labels, comps)
        };
        let mw = page_deskewed.width;
        let mh = page_deskewed.height;

        // Rebuild every WordSpan on the page once, up front, for (ii)'s
        // x-extent lookup -- cheap relative to per-word segmentation.
        let word_p = p.words();
        let seg_p = p.segment();
        let bands = if comps.is_empty() { Vec::new() } else { lines::group_with_bands(&comps, mw, mh, &line_p) };
        let mut rebuilt: Vec<(lines::TextLine, Vec<words::WordSpan>)> = Vec::new();
        for group in &bands {
            let spans_by_line = words::split_band_with(group, &comps, &word_p);
            for (tl, spans) in group.iter().zip(spans_by_line) {
                rebuilt.push((tl.clone(), spans));
            }
        }

        s.lines_total += decoded.len().min(truth_lines.len()) as u64;
        for (line_idx, (dline, tline)) in decoded.iter().zip(truth_lines.iter()).enumerate() {
            let Some(truth_text) = tline.as_str() else { continue };
            let truth_tokens: Vec<&str> = truth_text.split_whitespace().collect();
            if dline.words.len() != truth_tokens.len() {
                continue;
            }
            s.lines_qualifying += 1;
            let deskewed_baseline = f64::from(dline.baseline) - f64::from(dline.rect.x) * slope;

            for (word_idx, (dword, ttoken)) in dline.words.iter().zip(truth_tokens.iter()).enumerate() {
                s.words_total += 1;
                let truth_chars: Vec<char> = ttoken.chars().collect();
                if dword.chars.len() != truth_chars.len() {
                    continue;
                }
                s.words_qualifying += 1;

                let mut on_path_boxes: Vec<(u32, u32, u32, u32)> = Vec::new();
                for (char_idx, (cbox, &tch)) in dword.chars.iter().zip(truth_chars.iter()).enumerate() {
                    on_path_boxes.push((cbox.rect.x, cbox.rect.y, cbox.rect.width, cbox.rect.height));
                    let Some(&class_idx) = class_of_char.get(&tch) else {
                        s.chars_out_of_charset += 1;
                        continue;
                    };
                    let rx = cbox.rect.x;
                    let ry = cbox.rect.y;
                    let rw = cbox.rect.width;
                    let rh = cbox.rect.height;
                    if rw == 0 || rh == 0 || rx.saturating_add(rw) > mw || ry.saturating_add(rh) > mh {
                        s.chars_bad_crop += 1;
                        continue;
                    }
                    let mut ink = vec![0u8; (rw * rh) as usize];
                    for yy in 0..rh {
                        for xx in 0..rw {
                            let src = ((ry + yy) * mw + (rx + xx)) as usize;
                            ink[(yy * rw + xx) as usize] = mask[src];
                        }
                    }
                    let baseline_dy = (deskewed_baseline - f64::from(ry)) as f32;
                    let input =
                        GlyphInput { ink: &ink, width: rw, height: rh, baseline_dy, x_height: dline.x_height };
                    let (raw, grid) = extract_with_grid(&input);
                    let xv = model.standardise(&raw);
                    let mtch = r#match::nearest(model, &raw, TOPK, true);
                    let mut topk_class = [NONE_CLASS; TOPK];
                    let mut topk_dist = [f32::INFINITY; TOPK];
                    if let Some(m) = &mtch {
                        for (i, c) in m.best.iter().take(TOPK).enumerate() {
                            topk_class[i] = c.class;
                            topk_dist[i] = c.distance;
                        }
                        if let Some(top) = m.top() {
                            if top.class == class_idx {
                                class_ratios.entry(class_idx).or_default().push(m.ratio());
                            }
                        }
                    }
                    let meta = format!("{row_id}\t{stem}\t{line_idx}\t{word_idx}\t{char_idx}\t{}", tch as u32);
                    let sink = if is_val_fold { &mut real_val } else { &mut real_train };
                    sink.write(&grid, &xv, class_idx, &topk_class, &topk_dist, &meta)?;
                    s.chars_dumped += 1;
                    if is_val_fold {
                        s.val_rows += 1;
                    } else {
                        s.train_rows += 1;
                    }
                }

                // (c)(ii): find this word's WordSpan by x-extent overlap
                // against `dword.rect` (same mask coordinate space as
                // `CharBox.rect` -- see the module doc), rebuild its
                // lattice, and verify by exact on-path box match before
                // trusting any of its non-path edges as negatives.
                let wx0 = dword.rect.x;
                let wx1 = dword.rect.x + dword.rect.width;
                let wy_mid = dword.rect.y + dword.rect.height / 2;
                let candidate = rebuilt.iter().find(|(tl, _)| wy_mid >= tl.y0 && wy_mid < tl.y1).and_then(
                    |(tl, spans)| {
                        spans
                            .iter()
                            .filter(|sp| sp.x1 > wx0 && sp.x0 < wx1)
                            .max_by_key(|sp| overlap_len(sp.x0, sp.x1, wx0, wx1))
                            .map(|sp| (tl.clone(), sp.clone()))
                    },
                );
                let Some((tl, span)) = candidate else {
                    nonpath_mismatches += 1;
                    continue;
                };
                let lat = segment::build_with(&span, &comps, &labels, mw, &tl, &seg_p);
                let mut all_boxes: Vec<(u32, u32, u32, u32, &segment::Edge)> = Vec::new();
                for e in &lat.edges {
                    let Some(g) = segment::crop(&lat, &labels, mw, e) else { continue };
                    if g.width == 0 || g.height == 0 {
                        continue;
                    }
                    all_boxes.push((g.x, g.y, g.width, g.height, e));
                }
                let on_path_found: usize = on_path_boxes
                    .iter()
                    .filter(|ob| all_boxes.iter().any(|(x, y, w, h, _)| (*x, *y, *w, *h) == **ob))
                    .count();
                if on_path_found != on_path_boxes.len() {
                    nonpath_mismatches += 1;
                    continue;
                }
                for (x, y, w, h, e) in &all_boxes {
                    let bx = (*x, *y, *w, *h);
                    if on_path_boxes.contains(&bx) {
                        continue;
                    }
                    let Some(g) = segment::crop(&lat, &labels, mw, e) else { continue };
                    let input = g.input(&tl);
                    let (raw, grid) = extract_with_grid(&input);
                    let xv = model.standardise(&raw);
                    let mtch = r#match::nearest(model, &raw, 1, true);
                    let (top1, ratio) = mtch
                        .as_ref()
                        .and_then(|m| m.top().map(|c| (c.class, m.ratio())))
                        .unwrap_or((NONE_CLASS, f32::INFINITY));
                    let meta = format!("{row_id}\t{stem}\t{line_idx}\t{word_idx}\t{}\t{}", e.x0, e.x1);
                    let sink = if is_val_fold { &mut nonpath_val } else { &mut nonpath_train };
                    sink.write(&grid, &xv, top1, ratio, &meta)?;
                    nonpath_negrows.push(NegRow { top1_class: top1, ratio });
                    if is_val_fold {
                        nonpath_val_rows += 1;
                    } else {
                        nonpath_train_rows += 1;
                    }
                }
            }
        }
    }

    Ok((
        s,
        NonpathSummary {
            train_rows: nonpath_train_rows,
            val_rows: nonpath_val_rows,
            mismatches: nonpath_mismatches,
            rows: nonpath_negrows,
        },
        class_ratios,
    ))
}

fn overlap_len(a0: u32, a1: u32, b0: u32, b1: u32) -> u32 {
    let lo = a0.max(b0);
    let hi = a1.min(b1);
    hi.saturating_sub(lo)
}

// ---------------------------------------------------------------------
// (c)(i) rendered-line negatives
// ---------------------------------------------------------------------

struct RenderSummary {
    pages: u64,
    train_rows: u64,
    val_rows: u64,
    rows: Vec<NegRow>,
}

fn dump_render_junk(engine: &Engine, model: &Model, out_dir: &Path) -> Result<RenderSummary, String> {
    let dir = tables::model_dir();
    let entries = tables::load_fonts(&dir)?;
    let mut train = SinkJunk::open(out_dir, "junk_render_train")?;
    let mut val = SinkJunk::open(out_dir, "junk_render_val")?;

    let blocks: Vec<(&str, Vec<String>)> = ident_corpus::ASCII_BLOCKS
        .iter()
        .map(|b| (b.name, b.lines.iter().map(|s| (*s).to_string()).collect()))
        .collect();

    let mut pages = 0u64;
    let mut train_rows = 0u64;
    let mut val_rows = 0u64;
    let mut rows: Vec<NegRow> = Vec::new();

    // These pages have no ground-truth text to decode against, only truth
    // glyph boxes to compare lattice edges to, so segmentation is rebuilt
    // directly from `engine.params()` rather than through `recognize_lines`
    // -- the same public calls (b) uses, same order, no `Engine` decode step
    // needed since there is nothing here for the decoder to read.
    let seg_params = engine.params();

    for fe in &entries {
        if !fe.distribution.usable(false) {
            continue;
        }
        let Some(path) = fe.file() else { continue };
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(face) = ocrcer_build::ttf_load::Face::parse(&bytes, 0) else { continue };

        for (block_name, lines_text) in &blocks {
            for &px in &RENDER_SIZES {
                let Some(pg) = page::render(&face, lines_text, px) else { continue };
                if !pg.missing.is_empty() || !pg.dropped.is_empty() {
                    continue;
                }
                pages += 1;
                let is_val = split_fraction(fmix64(fnv1a64(
                    format!("{}\u{1f}{}\u{1f}{}", fe.family, block_name, px.to_bits()).as_bytes(),
                ))) < 0.2;

                let width = pg.width;
                let height = pg.height;
                let grey = pg.grey.clone();
                let mask0 = binarize::binarize_with(
                    &Gray { width, height, data: &grey },
                    &seg_params.binarize(),
                );
                let slope =
                    deskew::estimate_with(&mask0, width, height, f64::from(seg_params.deskew.max_slope));
                let page_deskewed = deskew::correct_with(
                    &Gray { width, height, data: &grey },
                    slope,
                    f64::from(seg_params.deskew.min_corrected_slope),
                );
                let gray2 = page_deskewed.gray();
                let mask = binarize::binarize_with(&gray2, &seg_params.binarize());
                let mw = page_deskewed.width;
                let mh = page_deskewed.height;
                let line_p = seg_params.lines();
                let (labels, count) =
                    components::label(&mask, mw, mh, components::Connectivity::Eight);
                let comps = components::components(&labels, mw, mh, count);
                if comps.is_empty() {
                    continue;
                }
                let bands = lines::group_with_bands(&comps, mw, mh, &line_p);
                let word_p = seg_params.words();
                let seg_p = seg_params.segment();

                // Truth lines by `.line` index, sorted by baseline -- same
                // top-to-bottom order the renderer laid the text out in.
                let mut truth_by_line: BTreeMap<usize, Vec<&page::PageGlyph>> = BTreeMap::new();
                for g in &pg.glyphs {
                    truth_by_line.entry(g.line).or_default().push(g);
                }
                let mut truth_lines: Vec<Vec<&page::PageGlyph>> = truth_by_line.into_values().collect();
                truth_lines.sort_by_key(|gs| gs.first().map(|g| g.baseline).unwrap_or(0));

                let mut my_lines: Vec<&lines::TextLine> = Vec::new();
                for group in &bands {
                    for tl in group {
                        my_lines.push(tl);
                    }
                }
                my_lines.sort_by(|a, b| a.baseline.partial_cmp(&b.baseline).unwrap());

                if my_lines.len() != truth_lines.len() {
                    continue;
                }

                for (tl, tglyphs) in my_lines.iter().zip(truth_lines.iter()) {
                    let spans = words::split_with(tl, &comps, &word_p);
                    for span in &spans {
                        let lat = segment::build_with(span, &comps, &labels, mw, tl, &seg_p);
                        for e in &lat.edges {
                            let matches_truth = tglyphs.iter().any(|g| {
                                let gx0 = g.x;
                                let gx1 = g.x + g.width;
                                (gx0 as i64 - e.x0 as i64).unsigned_abs() <= 1
                                    && (gx1 as i64 - e.x1 as i64).unsigned_abs() <= 1
                            });
                            if matches_truth {
                                continue;
                            }
                            let Some(g) = segment::crop(&lat, &labels, mw, e) else { continue };
                            if g.width == 0 || g.height == 0 {
                                continue;
                            }
                            let input = g.input(tl);
                            let (raw, grid) = extract_with_grid(&input);
                            let xv = model.standardise(&raw);
                            let mtch = r#match::nearest(model, &raw, 1, true);
                            let (top1, ratio) = mtch
                                .as_ref()
                                .and_then(|m| m.top().map(|c| (c.class, m.ratio())))
                                .unwrap_or((NONE_CLASS, f32::INFINITY));
                            let meta =
                                format!("{}\t{}\t{}\t{}\t{}", fe.family, block_name, px as u32, e.x0, e.x1);
                            let sink = if is_val { &mut val } else { &mut train };
                            sink.write(&grid, &xv, top1, ratio, &meta)?;
                            rows.push(NegRow { top1_class: top1, ratio });
                            if is_val {
                                val_rows += 1;
                            } else {
                                train_rows += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(RenderSummary { pages, train_rows, val_rows, rows })
}

// ---------------------------------------------------------------------
// Item 3: real-shape negatives share
// ---------------------------------------------------------------------

fn class_medians(class_ratios: &BTreeMap<u16, Vec<f32>>) -> BTreeMap<u16, f32> {
    let mut out = BTreeMap::new();
    for (&class, vals) in class_ratios {
        if vals.len() < MIN_CLASS_POSITIVES {
            continue;
        }
        let mut v = vals.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mid = v.len() / 2;
        let median = if v.len() % 2 == 0 { (v[mid - 1] + v[mid]) / 2.0 } else { v[mid] };
        out.insert(class, median);
    }
    out
}

struct ShareResult {
    share: f64,
    excluded: u64,
}

fn real_shape_share(rows: &[NegRow], medians: &BTreeMap<u16, f32>) -> ShareResult {
    let mut under = 0u64;
    let mut counted = 0u64;
    let mut excluded = 0u64;
    for r in rows {
        match medians.get(&r.top1_class) {
            Some(&m) => {
                counted += 1;
                if r.ratio < m {
                    under += 1;
                }
            }
            None => excluded += 1,
        }
    }
    let share = if counted > 0 { under as f64 / counted as f64 } else { 0.0 };
    ShareResult { share, excluded }
}
