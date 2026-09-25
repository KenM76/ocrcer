//! `ocrcer-build`: renders glyphs, builds the prototype bank, compiles the
//! authored tables, and writes `.ocrw`. This crate never ships. It links
//! `ocrcer-core` and calls its `ocrcer_core::feature::extract` for glyph
//! features rather than owning a second implementation of the extractor
//! (see `CLAUDE.md` rule 4).
//!
//! # Commands
//!
//! ```text
//! ocrcer-build render <font-file> <char> <px-per-em> [face-index]
//! ```
//!
//! Rasterises one character from one font file and prints it as ASCII, with
//! its pixel size and baseline. An operator's eyeball check on a face before
//! it goes into the bank: a face that parses but renders as a rectangle, or
//! upside down, or blank at the sizes the engine cares about, is visible here
//! in a second and invisible in an accuracy number three chunks later.
//!
//! ```text
//! ocrcer-build bank <build-sizes> <eval-sizes> <gate> [--local]
//! ```
//!
//! Builds a prototype bank at the comma-separated `build-sizes` (px/em) and
//! measures 1-nearest-neighbour accuracy on glyphs rendered at `eval-sizes`,
//! reporting build time, prototype count, coverage gaps and the confusions
//! that actually occurred.
//!
//! Its purpose is to settle by measurement what `ARCHITECTURE.md` section 4
//! leaves open — the canonical render size. Features normalise onto a 32x32
//! grid and are largely scale-invariant, but hole count and crossings are
//! taken from the *source* bitmap, so a bank rendered only large may hold
//! clean counters where a runtime glyph near the resolution floor has fused
//! ones. Give `eval-sizes` that are not in `build-sizes` and the number
//! printed is the held-out answer to that question rather than a restatement
//! of the inputs.
//!
//! `gate` is `none`, `holes` or `measured` — which of section
//! 4.1's pruning steps to apply. Pruning exists to save work, and whether a
//! given step also costs accuracy is a question this command answers rather
//! than assumes.
//!
//! ```text
//! ocrcer-build style <build-sizes> <eval-sizes> <field-lengths> [--local]
//! ```
//!
//! Diagnostic, not a build step: measures whether Sarkar & Nagy's (IEEE PAMI
//! 27(1) 2005) style-consistent classification beats plain 1-NN when a
//! field's true face is not itself in the bank, simulated by leave-one-
//! face-out. `field-lengths` is a comma-separated list of field sizes (e.g.
//! `1,2,4,8`). See `crates/ocrcer-build/src/style.rs` for the classifiers and
//! `docs/measurements/` for the write-up this command's output feeds.
//!
//! ```text
//! ocrcer-build write <build-sizes> <eval-sizes> <out.ocrw> [--local]
//! ```
//!
//! Builds the bank, writes it as a `.ocrw` file, and reports the one number
//! quantisation is allowed to be judged by: top-1 agreement between the `f32`
//! bank and the same bank round-tripped through `int8` storage, over the
//! glyphs rendered at `eval-sizes`.
//!
//! `--local` admits `local-only` faces. Without it the bank is the one that
//! could actually be shipped.
//!
//! Exit status is `0` on success and `1` on any failure, with the reason on
//! stderr.

use ocrcer_build::{bank, corpus, emit, llm_pack, ocrw, page, style, tables, ttf_load};

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    match argv.as_slice() {
        ["render", path, ch, px] => report(render(path, ch, px, "0")),
        ["render", path, ch, px, index] => report(render(path, ch, px, index)),
        ["bank", build, eval, gate] => report(run_bank(build, eval, gate, false)),
        ["bank", build, eval, gate, "--local"] => report(run_bank(build, eval, gate, true)),
        ["style", build, eval, fields] => report(run_style(build, eval, fields, false)),
        ["style", build, eval, fields, "--local"] => report(run_style(build, eval, fields, true)),
        ["write", build, eval, out] => report(run_write(build, eval, out, false)),
        ["write", build, eval, out, "--local"] => report(run_write(build, eval, out, true)),
        ["inspect", path] => report(run_inspect(path)),
        ["aspect"] => report(run_aspect(false)),
        ["aspect", "--local"] => report(run_aspect(true)),
        ["metrics"] => report(run_metrics(false)),
        ["metrics", "--local"] => report(run_metrics(true)),
        ["pages", out, px] => report(run_pages(out, px, false)),
        ["pages", out, px, "--local"] => report(run_pages(out, px, true)),
        ["llm-pack", hf_dir, out, "--quant", quant, "--model-id", model_id, "--revision", revision] => {
            report(run_llm_pack(hf_dir, out, quant, model_id, revision))
        }
        _ => {
            eprintln!("usage: ocrcer-build render <font-file> <char> <px-per-em> [face-index]");
            eprintln!(
                "       ocrcer-build bank <build-sizes> <eval-sizes> <none|holes|measured> [--local]"
            );
            eprintln!(
                "       ocrcer-build style <build-sizes> <eval-sizes> <field-lengths> [--local]"
            );
            eprintln!("       ocrcer-build write <build-sizes> <eval-sizes> <out.ocrw> [--local]");
            eprintln!("       ocrcer-build inspect <model.ocrw>");
            eprintln!("       ocrcer-build pages <out-dir> <sizes> [--local]");
            eprintln!("       ocrcer-build metrics [--local]");
            eprintln!("       ocrcer-build aspect [--local]");
            eprintln!(
                "       ocrcer-build llm-pack <hf-model-dir> <out.ocrl> --quant <f32|q8> --model-id <id> --revision <sha>"
            );
            ExitCode::FAILURE
        }
    }
}

/// Converts a Hugging Face Qwen model directory into a `.ocrl` file
/// (`ARCHITECTURE.md` section 11, 2026-09-24). `model_id` and `revision` are
/// recorded in `meta` for attribution; they are supplied on the command line
/// rather than read from the directory because a local clone's path is not
/// the upstream identity, and the revision is what the operator pinned when
/// they downloaded it, not something the directory's contents can attest to.
fn run_llm_pack(hf_dir: &str, out: &str, quant: &str, model_id: &str, revision: &str) -> Result<(), String> {
    let quant = llm_pack::Quant::parse(quant)?;
    llm_pack::convert(std::path::Path::new(hf_dir), std::path::Path::new(out), quant, model_id, revision)
}

/// Reports what a `.ocrw` carries, as the runtime sees it.
///
/// Loads through `ocrcer_core::ocrw::Model::load` rather than re-parsing the
/// container here, so what it prints is what the engine would actually get.
/// A table the runtime silently skipped is therefore reported as absent,
/// which is the honest thing to say about it.
fn run_inspect(path: &str) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let m = ocrcer_core::ocrw::Model::load(&bytes).map_err(|e| format!("{path}: {e:?}"))?;
    println!("build_id            {}", m.build_id);
    println!("feature_version     {}", m.feature_version);
    println!("classes             {}", m.classes.len());
    println!("faces               {}", m.faces.len());
    println!("prototypes          {}", m.n_prototypes());
    // `meta.sizes` is the build size ladder `ocrcer-build write` was given
    // (ARCHITECTURE.md section 11, 2026-09-23): a bank's ladder is read from
    // the file, not remembered from the command line that built it. A file
    // written before the field existed still parses -- `sizes` is optional --
    // and reports as not recorded rather than as an empty ladder, which would
    // read as a bank built at no size at all.
    println!("ladder              {}", format_ladder(&m.sizes));
    println!(
        "lexicon             {}",
        m.lexicon.as_ref().map_or("absent".into(), |l| format!("{} nodes", l.nodes()))
    );
    println!(
        "bigrams             {}",
        m.bigrams.as_ref().map_or("absent".into(), |b| format!("{} rows", b.rows()))
    );
    println!(
        "confusions          {}",
        m.confusions.as_ref().map_or("absent".into(), |c| format!("{} adjustments", c.len()))
    );
    let p = m.params;
    println!("params              w_match {} w_bigram {} w_lex {} w_seg {}",
        p.decode.w_match, p.decode.w_bigram, p.decode.w_lex, p.decode.w_seg);
    // Read back out of the file, because the runtime takes these from the file
    // and not from the compiled defaults: a tuned value that failed to reach
    // the table would otherwise only show up as accuracy that did not move.
    println!("params              words min_gaps {} min_separability {} lone_gap_x_heights {}",
        p.words.min_gaps, p.words.min_separability, p.words.lone_gap_x_heights);
    println!(
        "params              lines column_gap_heights {} column_lone_guard {}",
        p.lines.column_gap_heights, p.lines.column_lone_guard
    );
    // Phase 2, ARCHITECTURE.md section 11, 2026-09-23 ("Merged lines: split
    // on two baselines"): both new controls read back from the file, same
    // reasoning as above -- the floor and the split toggle only protect a
    // page if they actually reached the table.
    println!(
        "params              lines x_height_floor_per_cap {} baseline_split {} baseline_split_sep {} baseline_split_support {}",
        p.lines.x_height_floor_per_cap, p.lines.baseline_split, p.lines.baseline_split_sep, p.lines.baseline_split_support
    );
    // Underline-strip, `ARCHITECTURE.md` section 11, 2026-09-23
    // ("Underlines are stripped from the pixels of over-wide components",
    // and the same day's "second rule" for debris_heights): same reasoning
    // as the pair above -- the derived floors only protect a page if they
    // actually reached the table, and the toggle they gate is a decision
    // the reader must be able to confirm from the file it loaded, not from
    // the source that built it.
    println!(
        "params              lines rule_run_heights {} debris_heights {} underline_strip {}",
        p.lines.rule_run_heights, p.lines.debris_heights, p.lines.underline_strip
    );
    // Whether the optional table actually reached the runtime. Section 7 lets
    // an unknown table name be skipped in silence, so "I wrote it" and "the
    // engine reads it" are different claims and only this one is the second.
    let w = m.weights;
    if w.iter().all(|x| *x == 1.0) {
        println!("weights             absent or all 1.0; every dimension weighs the same");
    } else {
        let mut seen: Vec<String> = Vec::new();
        for (i, x) in w.iter().enumerate() {
            let s = format!("{x}");
            if !seen.contains(&s) {
                seen.push(s.clone());
                println!("weights             dim {i} = {s} (first of its value)");
            }
        }
    }
    // Every table's on-disk data length, because ARCHITECTURE.md section 2's
    // Size column claims to have been read out of the file rather than
    // estimated. A claim like that needs a command behind it, or the next
    // rebuild quietly makes it false.
    let c = ocrcer_core::ocrw::Container::load(&bytes).map_err(|e| format!("{path}: {e:?}"))?;
    println!("meta                {} B", c.meta_text.len());
    for t in &c.tables {
        println!("table {:<15} {} B", t.name, t.data.len());
    }
    println!("file                {} B", bytes.len());
    if let Some(l) = m.lexicon.as_ref() {
        // Proves the fold map reached the graph: a word typed in three cases
        // must land on the same node, and a part number must not be a word.
        let idx = |c: char| m.classes.iter().find(|k| k.codepoint == c).map(|k| k.index);
        let walk = |w: &str| -> Option<Option<u8>> {
            let cls: Option<Vec<u16>> = w.chars().map(idx).collect();
            Some(ocrcer_core::decode::lexicon::lookup(l, &cls?))
        };
        for w in ["invoice", "Invoice", "INVOICE", "M8x1", "zzqx"] {
            match walk(w) {
                Some(Some(t)) => println!("lexicon  {w:<10} tier {t}"),
                Some(None) => println!("lexicon  {w:<10} not a word"),
                None => println!("lexicon  {w:<10} not spellable in this charset"),
            }
        }
    }
    Ok(())
}

fn report(r: Result<(), String>) -> ExitCode {
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ocrcer-build: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Formats `Model::sizes` for `inspect`. Empty means the field predates the
/// file (`meta.sizes` is optional and additive), never a bank built at no
/// size, so the two cases must not print the same way.
fn format_ladder(sizes: &[f32]) -> String {
    if sizes.is_empty() {
        "(not recorded)".to_string()
    } else {
        sizes.iter().map(|px| format!("{px}")).collect::<Vec<_>>().join(",")
    }
}

fn sizes(csv: &str) -> Result<Vec<f32>, String> {
    let mut out = Vec::new();
    for part in csv.split(',') {
        let px: f32 = part
            .trim()
            .parse()
            .map_err(|_| format!("bad px-per-em {part:?}"))?;
        if !px.is_finite() || px <= 0.0 {
            return Err(format!("px-per-em must be positive, got {px}"));
        }
        out.push(px);
    }
    Ok(out)
}

fn run_bank(build_sizes: &str, eval_sizes: &str, gate: &str, local: bool) -> Result<(), String> {
    use ocrcer_core::feature::{extract, GlyphInput};
    use std::time::Instant;

    let gate = bank::Gate::parse(gate).ok_or_else(|| format!("unknown gate {gate:?}"))?;
    let build_px = sizes(build_sizes)?;
    let eval_px = sizes(eval_sizes)?;
    let dir = tables::model_dir();
    let classes = tables::load_charset(&dir)?;
    let entries = tables::load_fonts(&dir)?;

    let fonts = bank::Fonts::load(&entries, local);
    let (faces, renderers, failed) = fonts.renderers();
    for line in &failed {
        eprintln!("face failed to parse: {line}");
    }

    let started = Instant::now();
    let b = bank::build(&classes, &faces, &renderers, &build_px);
    let build_secs = started.elapsed().as_secs_f64();

    println!(
        "{} faces, {} classes, gate {gate:?}, build sizes {:?}",
        faces.len(),
        classes.len(),
        build_px
    );
    println!(
        "{} prototypes in {build_secs:.1}s ({} absent rows)",
        b.prototypes.len(),
        fonts.absent.len()
    );
    let glyph_of = |i: &u16| classes[usize::from(*i)].codepoint;
    if !b.uncovered.is_empty() {
        let list: String = b.uncovered.iter().map(glyph_of).collect();
        println!("uncovered by every face ({}): {list}", b.uncovered.len());
    }
    if !b.uncovered_in_base.is_empty() {
        let list: String = b.uncovered_in_base.iter().map(glyph_of).collect();
        println!(
            "uncovered by shippable faces ({}): {list}",
            b.uncovered_in_base.len()
        );
    }

    let mut wrong: Vec<(char, char, f32)> = Vec::new();
    let mut by_category: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
    // The engine's target domain is printed documents and CAD drawing text
    // (rule 7). Accented Latin-1 is in the charset and must work, but an
    // accuracy number that mixes it with the ASCII core hides which of the
    // two is costing what.
    let mut by_band: std::collections::BTreeMap<&str, (usize, usize)> = Default::default();
    let mut total = 0usize;
    let mut hits = 0usize;
    // A twin counts as correct here as well as in `hits`. Case is resolved by
    // line geometry in the decoder (section 5), not by the matcher, so a
    // matcher that answers `V` for a `v` has not made the kind of mistake the
    // engine's output will contain -- and reporting only the raw number would
    // read as an accuracy problem where there is a division of labour.
    let mut folded = 0usize;
    for &px in &eval_px {
        let (mut t, mut h) = (0usize, 0usize);
        for r in &renderers {
            for class in &classes {
                let Some(g) = r.render(class.codepoint, px) else {
                    continue;
                };
                let Some(x_height) = r.x_height_px(px).filter(|x| *x > 0.0) else {
                    continue;
                };
                let f = extract(&GlyphInput {
                    ink: &g.ink,
                    width: g.width,
                    height: g.height,
                    baseline_dy: g.baseline_dy,
                    x_height,
                });
                let Some((got, d1, d2)) = b.nearest(&f, gate) else {
                    continue;
                };
                t += 1;
                let band = if (class.codepoint as u32) < 128 {
                    "ascii"
                } else {
                    "non-ascii"
                };
                let b = by_band.entry(band).or_insert((0usize, 0usize));
                b.0 += 1;
                if got != class.index {
                    b.1 += 1;
                }
                let entry = by_category.entry(class.category.as_str()).or_default();
                entry.0 += 1;
                if got == class.index || class.case_twin == Some(got) {
                    folded += 1;
                }
                if got == class.index {
                    h += 1;
                } else {
                    entry.1 += 1;
                    let margin = if d2.is_finite() { d2 - d1 } else { f32::INFINITY };
                    wrong.push((class.codepoint, classes[usize::from(got)].codepoint, margin));
                }
            }
        }
        total += t;
        hits += h;
        let pct = if t == 0 { 0.0 } else { 100.0 * h as f64 / t as f64 };
        println!("  {px:>5} px/em: {h}/{t} = {pct:.2}%");
    }
    let pct = if total == 0 {
        0.0
    } else {
        100.0 * hits as f64 / total as f64
    };
    let fpct = if total == 0 {
        0.0
    } else {
        100.0 * folded as f64 / total as f64
    };
    println!("overall: {hits}/{total} = {pct:.2}%  (case-folded {fpct:.2}%)");
    for (band, (n, bad)) in &by_band {
        let p = if *n == 0 { 0.0 } else { 100.0 * (n - bad) as f64 / *n as f64 };
        println!("  {band:>9}: {}/{n} = {p:.2}%", n - bad);
    }
    for (cat, (n, bad)) in &by_category {
        let p = if *n == 0 { 0.0 } else { 100.0 * (n - bad) as f64 / *n as f64 };
        println!("  {cat:>8}: {}/{n} = {p:.2}%", n - bad);
    }

    wrong.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    let mut counts: std::collections::BTreeMap<(char, char), usize> = Default::default();
    for (want, got, _) in &wrong {
        *counts.entry((*want, *got)).or_default() += 1;
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for ((want, got), n) in ranked.iter().take(20) {
        println!("  confusion {want:?} -> {got:?} x{n}");
    }
    Ok(())
}

/// Per-condition, per-field-length running tally: `run_style`'s report is
/// eight of these (2 conditions x 4 field lengths by default) plus the
/// case-folded twin of each.
#[derive(Default)]
struct Tally {
    total: usize,
    singlet_wrong: usize,
    ls_wrong: usize,
    singlet_wrong_folded: usize,
    ls_wrong_folded: usize,
}

/// Sarkar & Nagy 2005 style-field diagnostic (chunk `style-probe`). Builds
/// the bank exactly as [`run_bank`] does, then asks whether their cheap
/// label-style approximation beats plain 1-NN on fields of eval glyphs --
/// especially when the field's own face is withheld from the bank
/// (leave-one-face-out), which is the condition that decides anything.
///
/// Distance is `bank.rs`'s standardised, unweighted squared-L2 (see
/// `style.rs`'s module doc for why), fixed to `Gate::Holes` -- the shipped
/// runtime's only enabled pruning step -- rather than exposed as a
/// parameter, since this command is asking about style, not about gates.
fn run_style(build_sizes: &str, eval_sizes: &str, field_lengths: &str, local: bool) -> Result<(), String> {
    use ocrcer_core::feature::{extract, GlyphInput};
    use std::time::Instant;

    let build_px = sizes(build_sizes)?;
    let eval_px = sizes(eval_sizes)?;
    for &e in &eval_px {
        if build_px.iter().any(|&b| b == e) {
            return Err(format!(
                "eval size {e} is in the build ladder; give sizes the bank was not built at"
            ));
        }
    }
    let field_lens: Vec<usize> = field_lengths
        .split(',')
        .map(|s| {
            s.trim()
                .parse::<usize>()
                .map_err(|_| format!("bad field length {s:?}"))
                .and_then(|l| if l == 0 { Err("field length must be positive".to_string()) } else { Ok(l) })
        })
        .collect::<Result<_, _>>()?;

    let dir = tables::model_dir();
    let classes = tables::load_charset(&dir)?;
    let entries = tables::load_fonts(&dir)?;
    let fonts = bank::Fonts::load(&entries, local);
    let (faces, renderers, failed) = fonts.renderers();
    for line in &failed {
        eprintln!("face failed to parse: {line}");
    }
    let n_faces = faces.len();
    let gate = bank::Gate::Holes;

    let started = Instant::now();
    let b = bank::build(&classes, &faces, &renderers, &build_px);
    println!(
        "{} faces, {} classes, gate {gate:?}, build sizes {} ({:.1}s)",
        n_faces,
        classes.len(),
        format_ladder(&build_px),
        started.elapsed().as_secs_f64()
    );
    println!(
        "eval sizes {} (held out of the build ladder), field lengths {field_lens:?}",
        format_ladder(&eval_px)
    );
    println!(
        "distance: bank.rs's standardised, UNWEIGHTED squared-L2 (matches run_bank/Bank::nearest) \
         -- not the shipped runtime's ocrcer_core::r#match weighted matcher"
    );

    // Step 2/3: score every eval glyph against every face once, grouped by
    // (face, eval size) so a field never crosses either boundary.
    let score_started = Instant::now();
    let mut groups: std::collections::BTreeMap<(u16, u32), Vec<style::Glyph>> = Default::default();
    let mut scored = 0usize;
    for &px in &eval_px {
        for (fi, r) in renderers.iter().enumerate() {
            for class in &classes {
                let Some(g) = r.render(class.codepoint, px) else { continue };
                let Some(x_height) = r.x_height_px(px).filter(|x| *x > 0.0) else { continue };
                let f = extract(&GlyphInput {
                    ink: &g.ink,
                    width: g.width,
                    height: g.height,
                    baseline_dy: g.baseline_dy,
                    x_height,
                });
                let per_face = style::per_face_best(&b, &f, gate);
                groups.entry((fi as u16, px.to_bits())).or_default().push(style::Glyph {
                    truth: class.index,
                    per_face,
                });
                scored += 1;
            }
        }
    }
    println!("scored {scored} glyphs across {} (face, size) groups in {:.1}s", groups.len(), score_started.elapsed().as_secs_f64());

    // Step 3: one deterministic order per group, reused at every field
    // length, per `style.rs`'s shuffle contract.
    for glyphs in groups.values_mut() {
        style::shuffle(glyphs, style::SHUFFLE_SEED);
    }

    // Sanity check: at L=1 the two classifiers must agree, in both
    // conditions, on every glyph. A violation stops the run rather than
    // silently reporting numbers built on a broken classifier.
    for (&(face, _), glyphs) in &groups {
        for g in glyphs {
            let (s, l) = style::l1_agreement(&g.per_face, None);
            if s != l {
                return Err(format!(
                    "L=1 sanity check failed (in-bank): class {} face {face}: singlet {s:?} != LS {l:?}",
                    classes[usize::from(g.truth)].codepoint
                ));
            }
            let (s, l) = style::l1_agreement(&g.per_face, Some(face));
            if s != l {
                return Err(format!(
                    "L=1 sanity check failed (leave-one-out): class {} face {face}: singlet {s:?} != LS {l:?}",
                    classes[usize::from(g.truth)].codepoint
                ));
            }
        }
    }
    println!("L=1 sanity check passed: LS matches singlet on every glyph, both conditions");

    let case_ok = |truth: u16, got: Option<u16>| {
        got == Some(truth) || got.is_some_and(|c| classes[usize::from(truth)].case_twin == Some(c))
    };

    // Step 6/7: both conditions, every field length, from the same scored
    // groups -- no further distance computation past this point.
    let mut fixed: std::collections::BTreeMap<(char, char), usize> = Default::default();
    let mut introduced: std::collections::BTreeMap<(char, char), usize> = Default::default();
    println!("condition            L  glyphs  singlet_err%  ls_err%  rel_change%  singlet_err%(folded)  ls_err%(folded)");
    for (cond_name, leave_out) in [("in-bank", false), ("leave-one-out", true)] {
        for &l in &field_lens {
            let mut t = Tally::default();
            let mut k_star_hits = 0usize;
            let mut k_star_total = 0usize;
            for (&(face, _), glyphs) in &groups {
                let excluded = if leave_out { Some(face) } else { None };
                for field in glyphs.chunks(l) {
                    if field.len() != l {
                        continue; // drop the tail remainder, per the task spec
                    }
                    let (k_star, labels) = style::label_field(field, n_faces, excluded);
                    if !leave_out {
                        k_star_total += 1;
                        if k_star == Some(usize::from(face)) {
                            k_star_hits += 1;
                        }
                    }
                    for (g, &label) in field.iter().zip(&labels) {
                        let faces_iter = (0..n_faces).filter(|&k| Some(k as u16) != excluded);
                        let single = style::singlet(&g.per_face, faces_iter);
                        t.total += 1;
                        let s_ok = single == Some(g.truth);
                        let l_ok = label == Some(g.truth);
                        if !s_ok {
                            t.singlet_wrong += 1;
                        }
                        if !l_ok {
                            t.ls_wrong += 1;
                        }
                        if !case_ok(g.truth, single) {
                            t.singlet_wrong_folded += 1;
                        }
                        if !case_ok(g.truth, label) {
                            t.ls_wrong_folded += 1;
                        }
                        if leave_out && l == 4 {
                            let want = classes[usize::from(g.truth)].codepoint;
                            // `single`/`label` are `None` only when no face
                            // admits any class for this glyph at all -- too
                            // rare to have a `(want, got)` pair, so it is
                            // skipped rather than forced into one.
                            if !s_ok && l_ok {
                                if let Some(sc) = single {
                                    let got = classes[usize::from(sc)].codepoint;
                                    *fixed.entry((want, got)).or_default() += 1;
                                }
                            } else if s_ok && !l_ok {
                                if let Some(lc) = label {
                                    let got = classes[usize::from(lc)].codepoint;
                                    *introduced.entry((want, got)).or_default() += 1;
                                }
                            }
                        }
                    }
                }
            }
            let pct = |n: usize| if t.total == 0 { 0.0 } else { 100.0 * n as f64 / t.total as f64 };
            let s_err = pct(t.singlet_wrong);
            let l_err = pct(t.ls_wrong);
            let rel = if s_err == 0.0 { 0.0 } else { 100.0 * (l_err - s_err) / s_err };
            println!(
                "{cond_name:<13} {l:>2}  {:>6}  {s_err:>11.2}  {l_err:>7.2}  {rel:>10.2}  {:>20.2}  {:>15.2}",
                t.total,
                pct(t.singlet_wrong_folded),
                pct(t.ls_wrong_folded)
            );
            if !leave_out {
                let hit_pct = if k_star_total == 0 { 0.0 } else { 100.0 * k_star_hits as f64 / k_star_total as f64 };
                println!("  k* == true face at L={l}: {k_star_hits}/{k_star_total} = {hit_pct:.2}%");
            }
        }
    }

    let mut fixed_ranked: Vec<_> = fixed.into_iter().collect();
    fixed_ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    println!("top confusions LS fixed at L=4 (leave-one-out), want -> got x count:");
    for ((want, got), n) in fixed_ranked.iter().take(10) {
        println!("  {want:?} -> {got:?} x{n}");
    }

    let mut introduced_ranked: Vec<_> = introduced.into_iter().collect();
    introduced_ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    println!("top confusions LS introduced at L=4 (leave-one-out), want -> got x count:");
    for ((want, got), n) in introduced_ranked.iter().take(10) {
        println!("  {want:?} -> {got:?} x{n}");
    }

    Ok(())
}

fn render(path: &str, ch: &str, px: &str, index: &str) -> Result<(), String> {
    let mut chars = ch.chars();
    let (c, rest) = (chars.next(), chars.next());
    let c = match (c, rest) {
        (Some(c), None) => c,
        _ => return Err(format!("expected a single character, got {ch:?}")),
    };
    let px_per_em: f32 = px.parse().map_err(|_| format!("bad px-per-em {px:?}"))?;
    let index: u32 = index.parse().map_err(|_| format!("bad face index {index:?}"))?;

    let data = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let font = ttf_load::Face::parse(&data, index).map_err(|e| format!("{path}: {e}"))?;

    if !font.has_char(c) {
        return Err(format!("{path} has no glyph for {c:?}"));
    }
    let r = font
        .render(c, px_per_em)
        .ok_or_else(|| format!("{c:?} renders no ink at {px_per_em} px/em"))?;

    let x_height = font
        .x_height_px(px_per_em)
        .map_or("unknown".to_string(), |x| format!("{x:.2} px"));
    println!(
        "{c:?}  {w}x{h} px  baseline {b:.2} px below top  x-height {x_height}  ({upm} units/em)",
        w = r.width,
        h = r.height,
        b = r.baseline_dy,
        upm = font.units_per_em()
    );
    for row in r.ink.chunks(r.width as usize) {
        let line: String = row.iter().map(|&b| if b != 0 { '#' } else { '.' }).collect();
        println!("{line}");
    }
    Ok(())
}

/// Reports the extreme long-over-short ink extent of every charset class,
/// over every face in the bank, at one render size.
///
/// The line stage has to decide whether a component is a piece of a glyph or
/// a piece of the page's furniture. A table rule inside one cell of a form is
/// far too small to trip the whole-page furniture fraction, so it survives as
/// a component and is matched against the charset like anything else --
/// usually landing on a dash. The gate that rejects it is an aspect ratio,
/// and the ratio has to be a bound the bank can vouch for rather than a
/// typographic guess: this is the measurement that supplies it. The widest
/// class here is the flattest thing the engine must still accept, so any
/// gate has to sit above it.
///
/// One size is enough because the ratio is scale-free up to rasterisation:
/// the size is large so a one-pixel rounding moves it by well under a
/// percent, the same reason `metrics` renders at 256 px/em.
fn run_aspect(local: bool) -> Result<(), String> {
    const PX: f32 = 256.0;

    let dir = tables::model_dir();
    let classes = tables::load_charset(&dir)?;
    let entries = tables::load_fonts(&dir)?;
    let fonts = bank::Fonts::load(&entries, local);
    let (faces, renderers, failed) = fonts.renderers();
    for line in &failed {
        eprintln!("face failed to parse: {line}");
    }

    // Per class: the widest ratio any face produced, and which face that was.
    let mut rows: Vec<(f64, char, String, u32, u32)> = Vec::new();
    // Longest horizontal ink run of any glyph, divided by that face's
    // x-height, over every class and face. This is `lines.rule_run_heights`:
    // the underline-strip rule only ever touches a component whose width is
    // at least this many x-heights, so the bound has to sit above the
    // longest run any real letterform produces at its own x-height -- an "m"
    // or an underlined descender's connecting stroke, not just a rule.
    let mut worst_run: Option<(f64, char, String, u32, f32)> = None;
    // Tallest ink height of any glyph, divided by that face's x-height, over
    // every class and face. This is `lines.debris_heights`: a strip-produced
    // piece (`ARCHITECTURE.md` section 11, 2026-09-23, "Drop strip debris")
    // is only ever a rule remnant, never real glyph ink, so the bound has to
    // sit above the tallest letterform this engine's own bank can produce at
    // its own x-height -- an ascender-plus-descender pileup, not just a
    // remnant.
    let mut worst_tall: Option<(f64, char, String, u32, f32)> = None;
    // Tallest ink height, in x-heights, among glyphs whose own ink width is
    // <= 0.5 x-height, over every class and face. This is
    // `lines.thin_debris_heights` (`ARCHITECTURE.md` section 11, 2026-09-23,
    // "Rule 2, part 3b"): a strip-produced sliver is narrow and tall (the
    // 2026-09-23 r000055 sliver measured ~3.9 x-heights at 51px/13px), so the
    // debris floor for a *narrow* piece has to sit above the tallest narrow
    // letterform this bank can produce, not above the tallest letterform of
    // any width the way `debris_heights` does.
    let mut worst_thin_tall: Option<(f64, char, String, u32, f32)> = None;
    let mut rendered = 0usize;
    for class in &classes {
        let mut worst: Option<(f64, String, u32, u32)> = None;
        for (f, r) in faces.iter().zip(renderers.iter()) {
            let Some(g) = r.render(class.codepoint, PX) else { continue };
            if g.width == 0 || g.height == 0 {
                continue;
            }
            rendered += 1;
            // Symmetric: the gate this feeds has to reject a vertical
            // cell rule as well as a horizontal one, so the bound that
            // matters is the longer extent over the shorter, whichever
            // way round the glyph is.
            let (lo, hi) = if g.width < g.height { (g.width, g.height) } else { (g.height, g.width) };
            let ratio = f64::from(hi) / f64::from(lo);
            if worst.as_ref().is_none_or(|(w, _, _, _)| ratio > *w) {
                worst = Some((ratio, format!("{} {}", f.family, f.style), g.width, g.height));
            }

            if let Some(x_height) = r.x_height_px(PX).filter(|&h| h > 0.0) {
                let mut longest_run = 0u32;
                for row in g.ink.chunks(g.width as usize) {
                    let mut run = 0u32;
                    for &px in row {
                        if px != 0 {
                            run += 1;
                            if run > longest_run {
                                longest_run = run;
                            }
                        } else {
                            run = 0;
                        }
                    }
                }
                if longest_run > 0 {
                    let run_ratio = f64::from(longest_run) / f64::from(x_height);
                    if worst_run.as_ref().is_none_or(|(w, _, _, _, _)| run_ratio > *w) {
                        worst_run = Some((
                            run_ratio,
                            class.codepoint,
                            format!("{} {}", f.family, f.style),
                            longest_run,
                            x_height,
                        ));
                    }
                }

                let tall_ratio = f64::from(g.height) / f64::from(x_height);
                if worst_tall.as_ref().is_none_or(|(w, _, _, _, _)| tall_ratio > *w) {
                    worst_tall = Some((
                        tall_ratio,
                        class.codepoint,
                        format!("{} {}", f.family, f.style),
                        g.height,
                        x_height,
                    ));
                }

                if f64::from(g.width) <= 0.5 * f64::from(x_height)
                    && worst_thin_tall.as_ref().is_none_or(|(w, _, _, _, _)| tall_ratio > *w)
                {
                    worst_thin_tall = Some((
                        tall_ratio,
                        class.codepoint,
                        format!("{} {}", f.family, f.style),
                        g.height,
                        x_height,
                    ));
                }
            }
        }
        if let Some((ratio, face, w, h)) = worst {
            rows.push((ratio, class.codepoint, face, w, h));
        }
    }
    if rows.is_empty() {
        return Err("no class rendered ink on any face".into());
    }

    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).expect("no NaN: both extents are positive"));
    println!("ratio	char	px	face");
    for (ratio, c, face, w, h) in rows.iter().take(15) {
        println!("{ratio:8.2}	{c:?}	{w}x{h}	{face}");
    }
    println!();
    println!(
        "{} classes, {} face renders at {PX} px/em",
        rows.len(),
        rendered
    );
    println!(
        "flattest or thinnest class {:?} at {:.2}:1 -- an aspect gate must sit above this",
        rows[0].1, rows[0].0
    );

    if let Some((run_ratio, c, face, run_px, x_height)) = worst_run {
        println!();
        println!(
            "longest horizontal ink run {:?} on {face}: {run_px}px run / {x_height:.2}px x-height = {run_ratio:.4} x-heights",
            c
        );
        println!(
            "lines.rule_run_heights = 2 x {run_ratio:.4} = {:.4}",
            run_ratio * 2.0
        );
    }

    if let Some((tall_ratio, c, face, h_px, x_height)) = worst_tall {
        println!();
        println!(
            "tallest ink height {:?} on {face}: {h_px}px height / {x_height:.2}px x-height = {tall_ratio:.4} x-heights",
            c
        );
        println!(
            "lines.debris_heights = 2 x {tall_ratio:.4} = {:.4}",
            tall_ratio * 2.0
        );
    }

    if let Some((tall_ratio, c, face, h_px, x_height)) = worst_thin_tall {
        println!();
        println!(
            "tallest thin-class (ink width <= 0.5 x-height) ink height {:?} on {face}: {h_px}px height / {x_height:.2}px x-height = {tall_ratio:.4} x-heights",
            c
        );
        println!(
            "lines.thin_debris_heights = 1.5 x {tall_ratio:.4} = {:.4}",
            tall_ratio * 1.5
        );
    }
    Ok(())
}

/// Reports each face's x-height and cap-height as measured ink, plus the
/// ratio between them.
///
/// The runtime's line stage can read an x-height off a page directly only
/// when the line has lowercase on it. An all-caps CAD annotation or a row of
/// dimension figures has no x-height band to find, and the four
/// baseline-relative features are normalised by x-height, so the runtime
/// needs a ratio to convert the cap band it *can* see into the x-height the
/// prototypes were measured against. That ratio is this measurement, not a
/// typographic rule of thumb.
///
/// Cap height is the ink height of `H` rather than the OS/2 declaration, for
/// the same reason `x_height_px` prefers to render `x` when the face
/// declares nothing: the prototypes come from ink, so the ratio must too.
fn run_metrics(local: bool) -> Result<(), String> {
    const PX: f32 = 256.0; // large, so a one-pixel rounding is ~0.4%

    let dir = tables::model_dir();
    let entries = tables::load_fonts(&dir)?;
    let fonts = bank::Fonts::load(&entries, local);
    let (faces, renderers, failed) = fonts.renderers();
    for line in &failed {
        eprintln!("face failed to parse: {line}");
    }

    println!("family	style	x_px	cap_px	ratio");
    let mut ratios: Vec<f64> = Vec::new();
    for (f, r) in faces.iter().zip(renderers.iter()) {
        let x = r.x_height_px(PX).filter(|v| *v > 0.0);
        let cap = r.render('H', PX).map(|g| g.height as f32).filter(|v| *v > 0.0);
        match (x, cap) {
            (Some(x), Some(cap)) => {
                let ratio = f64::from(x) / f64::from(cap);
                ratios.push(ratio);
                println!("{}	{}	{x:.1}	{cap:.1}	{ratio:.4}", f.family, f.style);
            }
            _ => println!("{}	{}	-	-	-", f.family, f.style),
        }
    }

    if ratios.is_empty() {
        return Err("no face yielded both an x-height and a cap height".into());
    }
    ratios.sort_by(|a, b| a.partial_cmp(b).expect("no NaN: both heights are positive"));
    let n = ratios.len();
    let median =
        if n % 2 == 1 { ratios[n / 2] } else { (ratios[n / 2 - 1] + ratios[n / 2]) / 2.0 };
    println!(
        "
{n} faces measured  median {median:.4}  min {:.4}  max {:.4}",
        ratios[0],
        ratios[n - 1]
    );
    Ok(())
}

fn run_write(build_sizes: &str, eval_sizes: &str, out: &str, local: bool) -> Result<(), String> {
    use ocrcer_core::feature::{extract, GlyphInput};

    let build_px = sizes(build_sizes)?;
    let eval_px = sizes(eval_sizes)?;
    let dir = tables::model_dir();
    let classes = tables::load_charset(&dir)?;
    let entries = tables::load_fonts(&dir)?;

    let fonts = bank::Fonts::load(&entries, local);
    let (faces, renderers, failed) = fonts.renderers();
    for line in &failed {
        eprintln!("face failed to parse: {line}");
    }

    let b = bank::build(&classes, &faces, &renderers, &build_px);
    if !b.uncovered_in_base.is_empty() {
        let list: String = b
            .uncovered_in_base
            .iter()
            .map(|i| classes[usize::from(*i)].codepoint)
            .collect();
        return Err(format!(
            "{} classes have no prototype in any shippable face: {list}",
            b.uncovered_in_base.len()
        ));
    }

    let path = std::path::Path::new(out);
    let meta = emit::meta(&b, &classes, &build_px);
    let mut all = emit::tables(&b);
    // A failure to compile the authored tables is reported and the file is
    // still written. The alternative -- refusing to emit a recogniser because
    // the lexicon has a bad row -- would make a language-table edit able to
    // block the image pipeline, which is exactly the coupling the chunk plan
    // keeps out.
    match emit::language_tables(&dir, &classes) {
        Ok(mut t) => all.append(&mut t),
        Err(e) => eprintln!("authored tables not written: {e}"),
    }
    // What the file says about its own thresholds and weights, printed at
    // write time rather than left to be discovered. CLAUDE.md rule 1 asks for
    // a guess to be labelled a guess; a build that never says how many
    // guesses it shipped has labelled them nowhere anyone reads.
    match ocrcer_build::params::load(&dir) {
        Ok(ps) => {
            let (m, a, g) = ocrcer_build::params::census(&ps);
            println!("params  {} rows: {a} authored, {m} measured, {g} guess", ps.len());
        }
        Err(e) => eprintln!("params census unavailable: {e}"),
    }
    match ocrcer_build::weights::load(&dir) {
        Ok(Some(ws)) => {
            let (a, m, g) = ocrcer_build::weights::census(&ws);
            let flat = ocrcer_build::weights::build(&ws).is_none();
            println!(
                "weights {} blocks: {a} authored, {m} measured, {g} guess{}",
                ws.len(),
                if flat { "; all 1.0, so no table was written" } else { "" }
            );
        }
        Ok(None) => println!("weights no feature_weights.tsv; every dimension weighs 1.0"),
        Err(e) => eprintln!("feature weights not written: {e}"),
    }

    ocrw::write(path, 1, 1, &meta, &all).map_err(|e| format!("{out}: {e}"))?;
    let size = std::fs::metadata(path).map_err(|e| format!("{out}: {e}"))?.len();
    println!(
        "wrote {out}: {} prototypes, {} faces, {} tables, {:.2} MB",
        b.prototypes.len(),
        faces.len(),
        all.len(),
        size as f64 / (1024.0 * 1024.0)
    );

    // Int8 is storage only. What it may cost is a changed answer, so that is
    // what gets counted -- not a reconstruction error, which would be a
    // number about the numbers rather than about the engine.
    let mut q = bank::build(&classes, &faces, &renderers, &build_px);
    emit::quantise_in_place(&mut q);
    let (mut n, mut agree, mut both_right) = (0usize, 0usize, 0usize);
    for &px in &eval_px {
        for r in &renderers {
            for class in &classes {
                let Some(g) = r.render(class.codepoint, px) else {
                    continue;
                };
                let Some(x_height) = r.x_height_px(px).filter(|x| *x > 0.0) else {
                    continue;
                };
                let f = extract(&GlyphInput {
                    ink: &g.ink,
                    width: g.width,
                    height: g.height,
                    baseline_dy: g.baseline_dy,
                    x_height,
                });
                let (Some((a, _, _)), Some((c, _, _))) = (
                    b.nearest(&f, bank::Gate::Holes),
                    q.nearest(&f, bank::Gate::Holes),
                ) else {
                    continue;
                };
                n += 1;
                if a == c {
                    agree += 1;
                }
                if c == class.index {
                    both_right += 1;
                }
            }
        }
    }
    let pct = |k: usize| if n == 0 { 0.0 } else { 100.0 * k as f64 / n as f64 };
    println!(
        "int8 top-1 agreement with f32: {agree}/{n} = {:.3}%",
        pct(agree)
    );
    println!("int8 top-1 accuracy: {both_right}/{n} = {:.2}%", pct(both_right));
    Ok(())
}

/// Renders the authored corpus with every eligible face at every requested
/// size, writing one `.pgm` page and one `.truth.json` beside it.
///
/// Files on disk rather than an in-process handoff because the whole point
/// is that two different engines read the *same bytes*: an engine that got
/// its pixels from a different code path is not being compared, it is being
/// described. `ARCHITECTURE.md` section 8.2's fixtures have the same shape
/// and the same reason.
///
/// The authored ISO 3098 face is skipped: it is a set of pen centrelines
/// with no advance-width table, so there is no defensible way to set a line
/// of text in it. Its prototypes stay in the bank; it just does not appear
/// in this corpus.
fn run_pages(out_dir: &str, px_list: &str, local: bool) -> Result<(), String> {
    use std::path::Path;

    let px_list = sizes(px_list)?;
    let dir = tables::model_dir();
    let entries = tables::load_fonts(&dir)?;
    let out = Path::new(out_dir);
    std::fs::create_dir_all(out).map_err(|e| format!("cannot create {out_dir}: {e}"))?;

    let slug = |s: &str| -> String {
        s.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect()
    };

    let mut written = 0usize;
    let mut skipped_missing = 0usize;
    let mut faces_used = 0usize;
    // Glyphs the binarizer broke into more than one mark. Not a defect and
    // not refused -- a serif arm that separates at 16 px/em is difficulty a
    // real scan has -- but a corpus that does not say how much of it there is
    // invites reading a per-size accuracy figure as a matcher result.
    let mut shattered = 0usize;
    let mut total_glyphs = 0usize;

    for e in &entries {
        if !e.distribution.usable(local) {
            continue;
        }
        let Some(path) = e.file() else { continue };
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(face) = ttf_load::Face::parse(&bytes, 0) else { continue };
        faces_used += 1;

        for block in corpus::ASCII_BLOCKS.iter().chain(corpus::EXTENDED_BLOCKS) {
            let lines: Vec<String> = block.lines.iter().map(|s| (*s).to_string()).collect();
            for &px in &px_list {
                let Some(pg) = page::render(&face, &lines, px) else { continue };
                if !pg.missing.is_empty() {
                    // Ground truth that was never drawn is not ground truth.
                    skipped_missing += 1;
                    continue;
                }
                let stem = format!(
                    "{}__{}__{}__{}px",
                    slug(&e.family),
                    slug(&e.style),
                    block.name,
                    px
                );
                // Named, because "the stroke broke" without a page is a report
                // nobody can act on.
                page::check(&pg, &face).map_err(|e| format!("{stem}: {e}"))?;
                total_glyphs += pg.glyphs.len();
                shattered += pg.glyphs.iter().filter(|g| g.ink_components > 1).count();
                std::fs::write(out.join(format!("{stem}.pgm")), page::to_pgm(&pg))
                    .map_err(|err| format!("writing {stem}.pgm: {err}"))?;
                std::fs::write(out.join(format!("{stem}.truth.json")), truth_json(&pg, e, px))
                    .map_err(|err| format!("writing {stem}.truth.json: {err}"))?;
                written += 1;
            }
        }
    }

    println!("{written} pages from {faces_used} faces into {out_dir}");
    if total_glyphs > 0 {
        let pct = 100.0 * shattered as f64 / total_glyphs as f64;
        println!(
            "{shattered} of {total_glyphs} glyphs ({pct:.2}%) binarize into more than one mark"
        );
    }
    if skipped_missing > 0 {
        println!("{skipped_missing} blocks skipped: face lacked a glyph the text needs");
    }
    if written == 0 {
        return Err("no pages rendered".into());
    }
    Ok(())
}

/// The ground truth beside each page: the text, and the box, baseline and
/// x-height of every mark of ink on it.
fn truth_json(pg: &page::Page, e: &tables::FontEntry, px: f32) -> String {
    let esc = ocrw::json_string;
    let mut s = String::new();
    s.push_str(&format!(
        "{{\"family\":{},\"style\":{},\"px_per_em\":{px},\"width\":{},\"height\":{},\"lines\":[",
        esc(&e.family),
        esc(&e.style),
        pg.width,
        pg.height
    ));
    for (i, l) in pg.lines.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&esc(l));
    }
    s.push_str("],\"glyphs\":[");
    for (i, g) in pg.glyphs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"ch\":{},\"line\":{},\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"baseline\":{},\"x_height\":{}}}",
            esc(&g.ch.to_string()),
            g.line,
            g.x,
            g.y,
            g.width,
            g.height,
            g.baseline,
            g.x_height
        ));
    }
    s.push_str("]}");
    s
}

#[cfg(test)]
mod tests {
    use super::format_ladder;

    /// The ladder prints as the writer's own CSV, echoing `sizes()`'s input
    /// format back so `inspect`'s output can be pasted straight into another
    /// `write --sizes`.
    #[test]
    fn a_recorded_ladder_prints_as_csv() {
        assert_eq!(format_ladder(&[16.0, 20.0, 24.0, 32.0, 48.0]), "16,20,24,32,48");
    }

    /// A file predating the field must not read as a bank built at no size:
    /// the two are different claims, and only one of them is true.
    #[test]
    fn a_missing_ladder_is_not_recorded_not_empty() {
        assert_eq!(format_ladder(&[]), "(not recorded)");
    }
}
