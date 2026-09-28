//! `route-probe`: chunk 15c's whole-page router grid fit (step 1) and
//! diagnosis (step 0) (`docs/ARCHITECTURE.md` §11, 2026-09-28, "chunk 15c
//! pre-registered").
//!
//! # Why this is not `route_fit.rs` again
//!
//! `route_fit.rs` (chunk 15b) sweeps `(matcher_margin, net_prob)` against
//! **crop-level, truth-aligned accuracy** — exactly the biased objective the
//! 15c brief calls out: it only ever asks about glyphs the alignment gate
//! could match to ground truth, so a change that shifts segmentation itself
//! (extra or dropped characters) is invisible to it. Chunk 15c is required
//! to fit against **end-to-end page CER**, which is not decomposable from
//! per-crop accuracy — it has to be recomputed as a whole page's edit
//! distance against truth. Re-running `Engine::recognize_lines` once per
//! grid point (250 points x 2 weight sets x 54 fold-A pages) was measured
//! infeasible (`tune`, ~9.76s/page in debug, would need days).
//!
//! Instead this reuses chunk 15c's `pipeline::RouteProbe`/`NetProbe`
//! capture: `Engine::recognize_lines_route_probe` is called **once per
//! page**, at `route.matcher_margin` fixed to `0.95` (the grid's own
//! maximum, so every glyph any grid point could ever query is captured —
//! `matcher_conf < margin` is monotonic in `margin`) and `route.max_junk`
//! left at its off default (`1.0`, never vetoes) so the net is still asked
//! for its top candidate regardless of what a smaller `max_junk` grid value
//! would later veto. Every other grid point is then replayed **in memory**
//! against those cached per-glyph numbers, calling `read_word`'s own
//! [`ocrcer_core::pipeline::category_flip_vetoed`] (now `pub` for this one
//! caller, chunk 15c) rather than a second implementation of the veto rule
//! -- `CLAUDE.md` rule 4. Word/line text assembly reuses
//! `ocrcer_bench::pages::page_text` unchanged: this file only overwrites
//! each captured word's `text` field before handing the whole `Vec<Line>`
//! to that one shared joiner, so the two things that could disagree about
//! how a page is flattened -- `ocr`'s scored corpora and this fit -- cannot.
//!
//! # Usage
//!
//! ```text
//! route-probe fit  <weights-label> <model.ocrw> <pages-train-dir> <split-file>
//! route-probe diag <weights-label> <model.ocrw> <pages-train-dir> <split-file>
//! ```
//!
//! `fit` (step 1): fold-A pages only, sweeps
//! `matcher_margin x net_prob x max_junk x same_category`
//! (`docs/ARCHITECTURE.md` §11, 2026-09-28's grid), maximising end-to-end
//! page CER, ties broken by fewest relabels.
//!
//! `diag` (step 0): fold-B pages only, at the fixed 15b point
//! (`matcher_margin = 0.95, net_prob = 0.50`, `max_junk`/`same_category`
//! off -- chunk 15c's own two new gates did not exist at the point being
//! diagnosed). Splits every relabel into "on a truth-aligned word" (further
//! split right/wrong) and "on a word this run could not align to truth",
//! and reports each split's share of the total page-level CER delta.
//!
//! Both refuse a `pages-dir` not named `*-train` (same firewall
//! `nn15_dump.rs::assert_train_dir_name` and `route_fit.rs` enforce) and
//! read only the fold their split-file column names -- fold A for `fit`,
//! fold B for `diag`. Neither ever opens `finfilings-val`, `pages-cov`, a
//! fixture, `bench/ident`, or `finfilings-train-unseen`.

use ocrcer_bench::cer::score;
use ocrcer_bench::pages::{list_pages, load_page, load_truth_beside, page_text};
use ocrcer_core::decode::viterbi::ClassInfo;
use ocrcer_core::pipeline::{category_flip_vetoed, Engine, Line, RouteProbe, Word};
use ocrcer_core::Gray;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Step 1's grid, exactly `ARCHITECTURE.md` §11's 2026-09-28 entry.
const MATCHER_MARGIN_GRID: &[f32] = &[0.3, 0.5, 0.7, 0.85, 0.95];
const NET_PROB_GRID: &[f32] = &[0.5, 0.7, 0.85, 0.95, 0.99];
const MAX_JUNK_GRID: &[f32] = &[0.02, 0.05, 0.1, 0.2, 1.0];
const SAME_CATEGORY_GRID: &[u32] = &[0, 1];

/// The margin probes are captured at: the grid's own maximum, so
/// `matcher_conf < CAPTURE_MARGIN` is true for every glyph any grid point
/// could ever query (`matcher_conf < margin` is monotonic in `margin`).
const CAPTURE_MARGIN: f32 = 0.95;

/// One point in the 4-parameter grid.
#[derive(Clone, Copy, Debug)]
struct Point {
    matcher_margin: f32,
    net_prob: f32,
    max_junk: f32,
    same_category: u32,
}

struct PageProbe {
    /// Kept for `eprintln!` diagnostics on a skipped page; not read once a
    /// page is in the scored set (every score below is corpus-aggregate).
    #[allow(dead_code)]
    stem: String,
    reference: String,
    /// Original `Line`s from the capture run -- band/rect/baseline kept
    /// as-is; `words[].text` is never read during replay (every glyph, not
    /// only the ones a particular grid point relabels, gets a `RouteProbe`
    /// pushed in `read_word`, so `probes[l][w][i].matcher_class` alone is
    /// enough to rebuild the pre-relabel text for every point).
    lines: Vec<Line>,
    probes: Vec<Vec<Vec<RouteProbe>>>,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 5 {
        eprintln!(
            "usage: route-probe <fit|diag> <weights-label> <model.ocrw> <pages-train-dir> <split-file>"
        );
        return ExitCode::FAILURE;
    }
    let mode = args[0].as_str();
    let label = args[1].clone();
    let model_path = PathBuf::from(&args[2]);
    let pages_dir = args[3].clone();
    let split_file = args[4].clone();

    if let Err(e) = assert_train_dir_name(Path::new(&pages_dir)) {
        return fail(&e);
    }

    let fold = match mode {
        "fit" => "A",
        "diag" => "B",
        other => return fail(&format!("unknown mode {other:?}, want fit or diag")),
    };

    let model_bytes = match std::fs::read(&model_path) {
        Ok(b) => b,
        Err(e) => return fail(&format!("reading {}: {e}", model_path.display())),
    };
    let mut engine = match Engine::from_bytes(&model_bytes) {
        Ok(e) => e,
        Err(e) => return fail(&format!("loading engine: {e}")),
    };
    if engine.model().nn.is_none() {
        return fail("model has no nn table; build with --nn first");
    }
    // classifier=3 turns on `route_on` (and therefore probe capture) in
    // `recognize_lines_route_probe`; `route.matcher_margin` is fixed at
    // capture time to the grid's max so every grid point's queries are a
    // subset of what gets captured. `route.net_prob`/`max_junk`/
    // `same_category` do not gate capture itself (only the relabel
    // decision, per `pipeline.rs`), so their capture-time values do not
    // matter, but they are set to "off" defaults for cleanliness.
    for (name, v) in [
        ("match.classifier", 3.0f32),
        ("route.matcher_margin", CAPTURE_MARGIN),
        ("route.net_prob", 0.0),
        ("route.max_junk", 1.0),
        ("route.same_category", 0.0),
    ] {
        if !engine.set_param(name, v) {
            return fail(&format!("{name}: not a knob this engine takes"));
        }
    }
    let class_info = engine.model().class_info.clone();

    let stems = match load_fold_stems(&split_file, fold) {
        Ok(s) => s,
        Err(e) => return fail(&e),
    };
    eprintln!("route-probe [{label}/{mode}]: {} fold-{fold} stems", stems.len());

    let all_pgms = match list_pages(&pages_dir) {
        Ok(p) => p,
        Err(e) => return fail(&e),
    };
    let chosen: Vec<PathBuf> = all_pgms
        .into_iter()
        .filter(|p| p.file_stem().map(|s| stems.contains(&s.to_string_lossy().into_owned())).unwrap_or(false))
        .collect();
    eprintln!("route-probe [{label}/{mode}]: {} pages chosen (fold {fold} only)", chosen.len());

    let mut pages: Vec<PageProbe> = Vec::new();
    for pgm in &chosen {
        match gather(&engine, pgm) {
            Ok(p) => pages.push(p),
            Err(e) => eprintln!("route-probe [{label}/{mode}]: skipping {}: {e}", pgm.display()),
        }
    }
    eprintln!("route-probe [{label}/{mode}]: {} pages captured", pages.len());

    match mode {
        "fit" => run_fit(&label, &pages, &engine, &class_info),
        "diag" => run_diag(&label, &pages, &engine, &class_info),
        _ => unreachable!(),
    }
    ExitCode::SUCCESS
}

fn gather(engine: &Engine, pgm_path: &Path) -> Result<PageProbe, String> {
    let stem = pgm_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let truth = load_truth_beside(pgm_path)?;
    let (w, h, grey) = load_page(pgm_path)?;
    let reference = truth.lines.join("\n");
    let result = engine
        .recognize_lines_route_probe(Gray { width: w, height: h, data: &grey })
        .map_err(|e| format!("{e:?}"))?;
    let (lines, probes): (Vec<Line>, Vec<Vec<Vec<RouteProbe>>>) = result.into_iter().unzip();
    Ok(PageProbe { stem, reference, lines, probes })
}

/// Replays one grid point's relabel decision over a page's captured probes
/// and returns the reconstructed page text, via `page_text` unchanged.
///
/// This is `read_word`'s relabel loop and nothing else — no confidence, no
/// word-agreement re-scoring, because CER reads only `Word.text`.
fn replay(engine: &Engine, class_info: &[ClassInfo], page: &PageProbe, pt: Point) -> String {
    let model = engine.model();
    let mut out_lines: Vec<Line> = Vec::with_capacity(page.lines.len());
    for (line, word_probes) in page.lines.iter().zip(page.probes.iter()) {
        let mut words: Vec<Word> = Vec::with_capacity(line.words.len());
        for (word, probes) in line.words.iter().zip(word_probes.iter()) {
            let orig_classes: Vec<u16> = probes.iter().map(|p| p.matcher_class).collect();
            let mut text = String::with_capacity(probes.len());
            for (i, p) in probes.iter().enumerate() {
                let mut class = p.matcher_class;
                if p.matcher_conf < pt.matcher_margin {
                    if let Some(net) = p.net {
                        if net.junk_prob <= pt.max_junk && net.prob >= pt.net_prob && net.class != p.matcher_class
                        {
                            let vetoed = pt.same_category == 1
                                && category_flip_vetoed(class_info, &orig_classes, i, net.class);
                            if !vetoed {
                                class = net.class;
                            }
                        }
                    }
                }
                if let Some(ch) = model.char_of(class) {
                    text.push(ch);
                }
            }
            let mut w2 = word.clone();
            w2.text = text;
            words.push(w2);
        }
        let mut l2 = line.clone();
        l2.words = words;
        out_lines.push(l2);
    }
    page_text(&out_lines)
}

/// How many characters differ between `read_word`'s relabelled text and the
/// same word replayed with every glyph forced back to its matcher class —
/// i.e. how many glyphs in this word this grid point actually relabels.
fn relabel_count(engine: &Engine, class_info: &[ClassInfo], page: &PageProbe, pt: Point) -> u64 {
    let model = engine.model();
    let mut n = 0u64;
    for word_probes in &page.probes {
        for probes in word_probes {
            let orig_classes: Vec<u16> = probes.iter().map(|p| p.matcher_class).collect();
            for (i, p) in probes.iter().enumerate() {
                let mut class = p.matcher_class;
                if p.matcher_conf < pt.matcher_margin {
                    if let Some(net) = p.net {
                        if net.junk_prob <= pt.max_junk && net.prob >= pt.net_prob && net.class != p.matcher_class
                        {
                            let vetoed = pt.same_category == 1
                                && category_flip_vetoed(class_info, &orig_classes, i, net.class);
                            if !vetoed {
                                class = net.class;
                            }
                        }
                    }
                }
                if class != p.matcher_class {
                    n += 1;
                }
            }
            let _ = model;
        }
    }
    n
}

fn page_cer_sum(engine: &Engine, class_info: &[ClassInfo], pages: &[PageProbe], pt: Point) -> (u64, u64, u64) {
    // (char_errors, chars, relabels) summed over pages -- a corpus-level
    // CER, not a mean of per-page CERs, matching `ocr.rs`'s `Column`/`Score`
    // aggregation (`cer.rs`'s `Score::add`).
    let mut char_errors = 0u64;
    let mut chars = 0u64;
    let mut relabels = 0u64;
    for page in pages {
        let read = replay(engine, class_info, page, pt);
        let s = score(&page.reference, &read);
        char_errors += s.char_errors as u64;
        chars += s.chars as u64;
        relabels += relabel_count(engine, class_info, page, pt);
    }
    (char_errors, chars, relabels)
}

fn run_fit(label: &str, pages: &[PageProbe], engine: &Engine, class_info: &[ClassInfo]) {
    let baseline = Point { matcher_margin: 0.0, net_prob: 1.0, max_junk: 0.0, same_category: 0 };
    let (b_err, b_chars, _) = page_cer_sum(engine, class_info, pages, baseline);
    let baseline_cer = b_err as f64 / b_chars.max(1) as f64;

    let mut best: Option<(Point, f64, u64)> = None;
    let mut n_points = 0usize;
    println!("{{");
    println!("  \"label\": {label:?},");
    println!("  \"n_pages\": {},", pages.len());
    println!("  \"baseline_cer\": {baseline_cer:.6},");
    println!("  \"baseline_char_errors\": {b_err},");
    println!("  \"baseline_chars\": {b_chars},");
    println!("  \"grid\": [");
    let mut first = true;
    for &mm in MATCHER_MARGIN_GRID {
        for &np in NET_PROB_GRID {
            for &mj in MAX_JUNK_GRID {
                for &sc in SAME_CATEGORY_GRID {
                    let pt = Point { matcher_margin: mm, net_prob: np, max_junk: mj, same_category: sc };
                    let (err, chars, relabels) = page_cer_sum(engine, class_info, pages, pt);
                    let cer = err as f64 / chars.max(1) as f64;
                    n_points += 1;
                    if !first {
                        println!(",");
                    }
                    first = false;
                    print!(
                        "    {{\"matcher_margin\": {mm}, \"net_prob\": {np}, \"max_junk\": {mj}, \"same_category\": {sc}, \"cer\": {cer:.6}, \"char_errors\": {err}, \"chars\": {chars}, \"relabels\": {relabels}}}"
                    );
                    let better = match best {
                        None => true,
                        // Lower CER wins; ties broken by fewest relabels
                        // (brief's own tie-break rule).
                        Some((_, bcer, brelab)) => cer < bcer || (cer == bcer && relabels < brelab),
                    };
                    if better {
                        best = Some((pt, cer, relabels));
                    }
                }
            }
        }
    }
    println!();
    println!("  ],");
    println!("  \"n_points\": {n_points},");
    if let Some((pt, cer, relabels)) = best {
        let on_boundary = pt.matcher_margin == *MATCHER_MARGIN_GRID.first().unwrap()
            || pt.matcher_margin == *MATCHER_MARGIN_GRID.last().unwrap()
            || pt.net_prob == *NET_PROB_GRID.first().unwrap()
            || pt.net_prob == *NET_PROB_GRID.last().unwrap()
            || pt.max_junk == *MAX_JUNK_GRID.first().unwrap()
            || pt.max_junk == *MAX_JUNK_GRID.last().unwrap();
        println!("  \"best_matcher_margin\": {},", pt.matcher_margin);
        println!("  \"best_net_prob\": {},", pt.net_prob);
        println!("  \"best_max_junk\": {},", pt.max_junk);
        println!("  \"best_same_category\": {},", pt.same_category);
        println!("  \"best_cer\": {cer:.6},");
        println!("  \"best_relabels\": {relabels},");
        println!("  \"best_on_grid_boundary\": {on_boundary}");
    }
    println!("}}");
}

fn run_diag(label: &str, pages: &[PageProbe], engine: &Engine, class_info: &[ClassInfo]) {
    // The fixed 15b point being diagnosed: chunk 15c's own gates did not
    // exist when this point was fitted, so they are off here.
    let pt = Point { matcher_margin: 0.95, net_prob: 0.50, max_junk: 1.0, same_category: 0 };
    let baseline = Point { matcher_margin: 0.0, net_prob: 1.0, max_junk: 0.0, same_category: 0 };

    let model = engine.model();
    let mut base_errors = 0u64;
    let mut base_chars = 0u64;
    let mut routed_errors = 0u64;
    let mut routed_chars = 0u64;

    // Aligned-relabel / unaligned-relabel split: a page is scored three
    // ways -- mode 0 (baseline), mode 3 at the diagnosed point (routed),
    // and "routed but every relabel on a word this page's own decoded
    // line/word count could align to truth is reverted" (aligned-off) --
    // so the CER delta attributable to aligned relabels is
    // routed-vs-aligned-off, and the rest of the routed-vs-baseline delta
    // is attributable to relabels on words this alignment could not check.
    let mut aligned_off_errors = 0u64;
    let mut aligned_off_chars = 0u64;
    let mut aligned_relabels_right = 0u64;
    let mut aligned_relabels_wrong = 0u64;
    let mut unaligned_relabels = 0u64;
    let mut total_relabels = 0u64;

    for page in pages {
        let base_read = replay(engine, class_info, page, baseline);
        let bs = score(&page.reference, &base_read);
        base_errors += bs.char_errors as u64;
        base_chars += bs.chars as u64;

        let routed_read = replay(engine, class_info, page, pt);
        let rs = score(&page.reference, &routed_read);
        routed_errors += rs.char_errors as u64;
        routed_chars += rs.chars as u64;

        // Truth alignment, word-for-word, the same shape `route_fit.rs`
        // uses (equal line count assumed by truth-line order, equal
        // per-line word count, equal per-word char count) -- reused as a
        // decision only, not a second scoring implementation: CER itself
        // still comes from `cer::score` above.
        let truth_lines: Vec<&str> = page.reference.lines().collect();
        let mut aligned_word_ptrs: BTreeSet<(usize, usize)> = BTreeSet::new();
        for (li, (line, word_probes)) in page.lines.iter().zip(page.probes.iter()).enumerate() {
            let Some(truth_line) = truth_lines.get(li) else { continue };
            let truth_tokens: Vec<&str> = truth_line.split_whitespace().collect();
            if line.words.len() != truth_tokens.len() {
                continue;
            }
            for (wi, (word, probes)) in line.words.iter().zip(word_probes.iter()).enumerate() {
                let Some(ttoken) = truth_tokens.get(wi) else { continue };
                let truth_chars: Vec<char> = ttoken.chars().collect();
                if probes.len() != truth_chars.len() {
                    continue;
                }
                aligned_word_ptrs.insert((li, wi));
                let orig_classes: Vec<u16> = probes.iter().map(|p| p.matcher_class).collect();
                for (i, p) in probes.iter().enumerate() {
                    let mut class = p.matcher_class;
                    if p.matcher_conf < pt.matcher_margin {
                        if let Some(net) = p.net {
                            if net.junk_prob <= pt.max_junk
                                && net.prob >= pt.net_prob
                                && net.class != p.matcher_class
                            {
                                let vetoed = pt.same_category == 1
                                    && category_flip_vetoed(class_info, &orig_classes, i, net.class);
                                if !vetoed {
                                    class = net.class;
                                }
                            }
                        }
                    }
                    if class != p.matcher_class {
                        total_relabels += 1;
                        if let (Some(&want), Some(got)) = (truth_chars.get(i), model.char_of(class)) {
                            if got == want {
                                aligned_relabels_right += 1;
                            } else {
                                aligned_relabels_wrong += 1;
                            }
                        }
                    }
                }
                let _ = word;
            }
        }
        // Every relabel outside an aligned word is counted as unaligned.
        for (li, word_probes) in page.probes.iter().enumerate() {
            for (wi, probes) in word_probes.iter().enumerate() {
                if aligned_word_ptrs.contains(&(li, wi)) {
                    continue;
                }
                let orig_classes: Vec<u16> = probes.iter().map(|p| p.matcher_class).collect();
                for (i, p) in probes.iter().enumerate() {
                    let mut class = p.matcher_class;
                    if p.matcher_conf < pt.matcher_margin {
                        if let Some(net) = p.net {
                            if net.junk_prob <= pt.max_junk
                                && net.prob >= pt.net_prob
                                && net.class != p.matcher_class
                            {
                                let vetoed = pt.same_category == 1
                                    && category_flip_vetoed(class_info, &orig_classes, i, net.class);
                                if !vetoed {
                                    class = net.class;
                                }
                            }
                        }
                    }
                    if class != p.matcher_class {
                        total_relabels += 1;
                        unaligned_relabels += 1;
                    }
                }
            }
        }

        // Aligned-off: replay with every aligned word's relabels reverted
        // to the matcher class (unaligned words keep the routed decision).
        let mut out_lines: Vec<Line> = Vec::with_capacity(page.lines.len());
        for (li, (line, word_probes)) in page.lines.iter().zip(page.probes.iter()).enumerate() {
            let mut words: Vec<Word> = Vec::with_capacity(line.words.len());
            for (wi, (word, probes)) in line.words.iter().zip(word_probes.iter()).enumerate() {
                let revert = aligned_word_ptrs.contains(&(li, wi));
                let orig_classes: Vec<u16> = probes.iter().map(|p| p.matcher_class).collect();
                let mut text = String::with_capacity(probes.len());
                for (i, p) in probes.iter().enumerate() {
                    let mut class = p.matcher_class;
                    if !revert && p.matcher_conf < pt.matcher_margin {
                        if let Some(net) = p.net {
                            if net.junk_prob <= pt.max_junk
                                && net.prob >= pt.net_prob
                                && net.class != p.matcher_class
                            {
                                let vetoed = pt.same_category == 1
                                    && category_flip_vetoed(class_info, &orig_classes, i, net.class);
                                if !vetoed {
                                    class = net.class;
                                }
                            }
                        }
                    }
                    if let Some(ch) = model.char_of(class) {
                        text.push(ch);
                    }
                }
                let mut w2 = word.clone();
                w2.text = text;
                words.push(w2);
            }
            let mut l2 = line.clone();
            l2.words = words;
            out_lines.push(l2);
        }
        let aligned_off_read = page_text(&out_lines);
        let aos = score(&page.reference, &aligned_off_read);
        aligned_off_errors += aos.char_errors as u64;
        aligned_off_chars += aos.chars as u64;
    }

    let base_cer = base_errors as f64 / base_chars.max(1) as f64;
    let routed_cer = routed_errors as f64 / routed_chars.max(1) as f64;
    let aligned_off_cer = aligned_off_errors as f64 / aligned_off_chars.max(1) as f64;

    println!("{{");
    println!("  \"label\": {label:?},");
    println!("  \"n_pages\": {},", pages.len());
    println!("  \"point\": {{\"matcher_margin\": {}, \"net_prob\": {}}},", pt.matcher_margin, pt.net_prob);
    println!("  \"baseline_cer\": {base_cer:.6},");
    println!("  \"routed_cer\": {routed_cer:.6},");
    println!("  \"routed_minus_baseline_pp\": {:.6},", (routed_cer - base_cer) * 100.0);
    println!("  \"aligned_off_cer\": {aligned_off_cer:.6},");
    println!(
        "  \"cer_delta_from_aligned_relabels_pp\": {:.6},",
        (routed_cer - aligned_off_cer) * 100.0
    );
    println!(
        "  \"cer_delta_from_unaligned_relabels_pp\": {:.6},",
        (aligned_off_cer - base_cer) * 100.0
    );
    println!("  \"total_relabels\": {total_relabels},");
    println!("  \"aligned_relabels_right\": {aligned_relabels_right},");
    println!("  \"aligned_relabels_wrong\": {aligned_relabels_wrong},");
    println!("  \"unaligned_relabels\": {unaligned_relabels}");
    println!("}}");
}

/// Same firewall `nn15_dump.rs::assert_train_dir_name` / `route_fit.rs`
/// enforce, copied (one-line guard, not a pipeline stage).
fn assert_train_dir_name(dir: &Path) -> Result<(), String> {
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: cannot read a directory name", dir.display()))?;
    if name.ends_with("-train") {
        Ok(())
    } else {
        Err(format!("{}: refusing to run against a directory not named `*-train`", dir.display()))
    }
}

/// Stems in the named fold (`bench/splits/nn15_page_split.tsv`'s
/// `#stem\tfold\tcluster_id`).
fn load_fold_stems(path: &str, fold: &str) -> Result<BTreeSet<String>, String> {
    use std::io::BufRead;
    let f = std::fs::File::open(path).map_err(|e| format!("reading {path}: {e}"))?;
    let mut out = BTreeSet::new();
    for line in std::io::BufReader::new(f).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            continue;
        }
        if cols[1] == fold {
            out.insert(cols[0].to_string());
        }
    }
    Ok(out)
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("route-probe: {msg}");
    ExitCode::FAILURE
}
