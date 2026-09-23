//! `ocr`: the whole engine, on whole pages, against known text.
//!
//! ```text
//! ocr <model.ocrw> <pages-dir> [--limit N] [--stride N] [--offset N]
//!     [--only SUBSTRING]
//!     [--oracle] [--worst N] [--line-pages]
//! ```
//!
//! # What makes this different from every other figure in this crate
//!
//! `charset-cost` and the bank sweeps read a page with its glyph boxes handed
//! to them. That is an oracle: it measures the matcher alone, and every number
//! from it is a ceiling rather than a result. This binary hands the engine
//! nothing but pixels. Binarisation, deskew, line grouping, word splitting,
//! the segmentation lattice and the decoder all have to work, and every one of
//! them can lose characters the matcher would have got right.
//!
//! So the two columns answer different questions and both are worth having:
//!
//! - **end-to-end** — what the engine actually does with a page. This is the
//!   number that is comparable to `ocrs`, because `ocrs` is also handed only
//!   pixels.
//! - **oracle** (`--oracle`) — the same pages read through
//!   `pages::read_with_bank`, boxes supplied. The gap between the two columns
//!   is what the layout stages and the decoder cost or win, and attributing it
//!   needs both measured on the same pages in the same run.
//!
//! `--oracle` rebuilds a prototype bank in-process from `model/` rather than
//! reading the `.ocrw`, so it costs a minute or two. Without it only the
//! end-to-end column is produced and no bank is built.
//!
//! # Reading order, and why two metrics
//!
//! CER/WER charge for displacement: a page read correctly but in the wrong
//! order scores near-total failure. Token recall/precision is layout-free.
//! Neither alone is the answer (`cer::TokenScore` says why at length). Both
//! are printed; report both.
//!
//! A third number, `line-matched CER`, sits between them: it pairs each
//! truth line to its best-matching read line (`cer::line_matched_score`)
//! before charging edit distance, so a table read in the wrong row order is
//! priced as recognition error, not as displacement. It is a second CER, not
//! a replacement for the one above -- both are printed. `--line-pages`
//! prints the same per-page listing this binary already keeps for the
//! ordinary CER, scored under the line-matched metric instead, so the worst
//! pages under each metric can be found and compared.
//!
//! # What this corpus is and is not
//!
//! These pages are rendered, not scanned: clean two-valued ink, no skew, no
//! noise, no compression. An accuracy from them is an upper bound on scanned
//! input, and it is stated as one. Degradation is a separate corpus.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::Instant;

use ocrcer_bench::cer::{line_matched_score, score, token_score, LineScore, Score, TokenScore};
use ocrcer_bench::pages::{list_pages, load_page, load_truth_beside, page_text};
use ocrcer_core::pipeline::Engine;
use ocrcer_core::Gray;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut model = String::new();
    let mut dir = String::new();
    let mut limit = usize::MAX;
    // A subsample of a corpus written by nested loops must not alias against
    // the loops: `bench/pages` cycles 5 render sizes inside 6 text blocks, so
    // a stride sharing a factor with either samples one stratum and reports it
    // as the corpus. 13 is coprime with both. `--offset` shifts the sample so
    // a value chosen on one subset can be re-measured on a disjoint one.
    let mut stride = 1usize;
    let mut offset = 0usize;
    let mut oracle = false;
    let mut worst = 12usize;
    let mut show = 0usize;
    let mut distances = false;
    let mut layout = false;
    let mut raw = false;
    // Diagnosis-only speed-up for `--layout` classification sweeps (2026-09-22
    // amendment ablation, docs/measurements/2026-09-22_fixed_pitch_spaces.txt):
    // skips the second `Engine::recognize_lines` call and the decoded-word
    // printing it feeds. A classification-only count (fragment rule source,
    // mono/proportional recall) never reads the `decoded {...}` line, and the
    // second recognition pass is the majority of `--layout`'s per-page cost.
    let mut no_decode = false;
    // Per-page listing under the line-matched metric, `worst`-limited like
    // the existing (ordinary-CER) "worst pages" section. Off by default
    // because it duplicates that section's shape and is a diagnostic, not
    // part of the headline two lines.
    let mut line_pages = false;
    // Parameter overrides, applied to the loaded engine before any page is
    // read. For running the full diagnostic at a candidate point of a `tune`
    // sweep — which is how a winning row gets explained rather than just
    // reported.
    // Restricts the corpus to page names containing this substring. A single
    // face or size reported alone is a diagnostic, never a corpus figure:
    // the header says how many pages survived the filter for exactly that
    // reason.
    let mut only: Option<String> = None;
    let mut set: Vec<(String, f32)> = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--stride" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()).filter(|n| *n >= 1) {
                    Some(n) => stride = n,
                    None => return usage("--stride needs a positive count"),
                }
            }
            "--offset" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(n) => offset = n,
                    None => return usage("--offset needs a count"),
                }
            }
            "--limit" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(n) => limit = n,
                    None => return usage("--limit needs a count"),
                }
            }
            "--worst" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(n) => worst = n,
                    None => return usage("--worst needs a count"),
                }
            }
            "--show" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse().ok()) {
                    Some(n) => show = n,
                    None => return usage("--show needs a count"),
                }
            }
            "--set" => {
                i += 1;
                match args.get(i).and_then(|v| v.split_once('=')).and_then(|(n, v)| {
                    v.trim().parse::<f32>().ok().map(|v| (n.to_string(), v))
                }) {
                    Some(nv) => set.push(nv),
                    None => return usage("--set needs <name>=<value>"),
                }
            }
            "--only" => {
                i += 1;
                match args.get(i) {
                    Some(v) if !v.is_empty() => only = Some(v.clone()),
                    _ => return usage("--only needs a page-name substring"),
                }
            }
            "--oracle" => oracle = true,
            "--distances" => distances = true,
            "--layout" => layout = true,
            "--raw" => raw = true,
            "--no-decode" => no_decode = true,
            "--line-pages" => line_pages = true,
            other if model.is_empty() => model = other.to_string(),
            other if dir.is_empty() => dir = other.to_string(),
            other => return usage(&format!("unexpected argument {other:?}")),
        }
        i += 1;
    }
    if model.is_empty() || dir.is_empty() {
        return usage("need a model and a pages directory");
    }
    if layout {
        return match run_layout(&model, &dir, limit, stride, offset, only.as_deref(), &set, no_decode) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("ocr: {e}");
                ExitCode::FAILURE
            }
        };
    }
    if distances {
        return match run_distances(&model, &dir, limit, stride, offset, only.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("ocr: {e}");
                ExitCode::FAILURE
            }
        };
    }
    if raw {
        return match run_raw(&model, &dir, limit, stride, offset, only.as_deref(), &set) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("ocr: {e}");
                ExitCode::FAILURE
            }
        };
    }
    match run(
        &model,
        &dir,
        limit,
        stride,
        offset,
        only.as_deref(),
        oracle,
        worst,
        show,
        &set,
        line_pages,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ocr: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage(why: &str) -> ExitCode {
    eprintln!("ocr: {why}");
    eprintln!(
        "usage: ocr <model.ocrw> <pages-dir> [--limit N] [--stride N] [--offset N]
       [--only SUBSTRING] [--oracle] [--worst N] [--show N] [--line-pages]
       [--set <name>=<value>] [--distances] [--layout] [--raw] [--no-decode]"
    );
    ExitCode::FAILURE
}

/// One column's running totals.
#[derive(Default)]
struct Column {
    seq: Score,
    tok: TokenScore,
    lm: LineScore,
    /// Pages whose CER was above this column's own page average, worst first.
    pages: Vec<(f64, String)>,
    /// Same idea, scored under the line-matched metric instead. Kept
    /// separate from `pages` because the two metrics can and do disagree on
    /// which page is worst -- that disagreement is the point of having both.
    line_pages: Vec<(f64, String)>,
}

impl Column {
    fn add(&mut self, stem: &str, reference: &str, read: &str) {
        let s = score(reference, read);
        let t = token_score(reference, read);
        let l = line_matched_score(reference, read);
        if let Some(c) = s.cer() {
            self.pages.push((c, stem.to_string()));
        }
        if let Some(c) = l.cer() {
            self.line_pages.push((c, stem.to_string()));
        }
        self.seq.add(&s);
        self.tok.add(&t);
        self.lm.add(&l);
    }

    fn line(&self, name: &str) -> String {
        let pct = |v: Option<f64>| v.map_or("--".to_string(), |x| format!("{:6.3}%", x * 100.0));
        format!(
            "{name:<12} CER {}  WER {}  recall {}  precision {}  F1 {}",
            pct(self.seq.cer()),
            pct(self.seq.wer()),
            pct(self.tok.recall()),
            pct(self.tok.precision()),
            pct(self.tok.f1()),
        )
    }

    /// The second, reading-order-independent CER, printed as its own line
    /// beside `line()`'s -- see `cer::line_matched_score`.
    fn line_matched_line(&self, name: &str) -> String {
        let pct = |v: Option<f64>| v.map_or("--".to_string(), |x| format!("{:6.3}%", x * 100.0));
        format!("{name:<12} CER {}  (reading-order-independent)", pct(self.lm.cer()))
    }
}

/// The corpus the flags asked for, and the header that says what survived.
///
/// Every mode selects pages through here, because a mode that does its own
/// selection is a mode that can quietly ignore a flag the parser accepted.
/// `--layout` and `--distances` did exactly that: both took
/// `(model, dir, limit)` and read the first N files in the directory whatever
/// `--only`, `--stride` and `--offset` said.
fn select_pages(
    dir: &str,
    limit: usize,
    stride: usize,
    offset: usize,
    only: Option<&str>,
) -> Result<Vec<std::path::PathBuf>, String> {
    let pgms = list_pages(dir)?;
    let kept: Vec<_> = match only {
        Some(sub) => pgms
            .iter()
            .filter(|p| p.file_name().is_some_and(|f| f.to_string_lossy().contains(sub)))
            .cloned()
            .collect(),
        None => pgms.clone(),
    };
    let chosen: Vec<_> = kept.into_iter().skip(offset).step_by(stride).take(limit).collect();
    println!(
        "pages   {} of {} in {dir} (stride {stride}, offset {offset}{})",
        chosen.len(),
        pgms.len(),
        match only {
            Some(sub) => format!(", only {sub:?}"),
            None => String::new(),
        }
    );
    println!();
    Ok(chosen)
}

/// A loaded engine with the `--set` overrides applied, or an error naming the
/// knob it refused. Shared for the same reason as [`select_pages`]: a
/// diagnostic that silently reports the shipped value while the caller asked
/// for another one manufactures false evidence.
fn engine_with(model: &str, set: &[(String, f32)]) -> Result<Engine, String> {
    let bytes = std::fs::read(model).map_err(|e| format!("{model}: {e}"))?;
    let mut engine = Engine::from_bytes(&bytes).map_err(|e| format!("{model}: {e:?}"))?;
    for (name, v) in set {
        if !ocrcer_bench::knobs::set(&mut engine, name, *v) {
            return Err(format!("{name} = {v}: not a knob this engine takes"));
        }
        println!("set     {name} = {v}  (an override, not what the file carries)");
    }
    Ok(engine)
}

fn run(
    model: &str,
    dir: &str,
    limit: usize,
    stride: usize,
    offset: usize,
    only: Option<&str>,
    oracle: bool,
    worst: usize,
    show: usize,
    set: &[(String, f32)],
    line_pages: bool,
) -> Result<(), String> {
    let engine = engine_with(model, set)?;
    let m = engine.model();
    println!(
        "model   {} — {} classes, {} faces, {} prototypes, lexicon {}, bigrams {}, confusions {}",
        model,
        m.classes.len(),
        m.faces.len(),
        m.n_prototypes(),
        m.lexicon.as_ref().map_or("absent".into(), |l| format!("{} nodes", l.nodes())),
        m.bigrams.as_ref().map_or("absent".into(), |b| format!("{} rows", b.rows())),
        m.confusions.as_ref().map_or("absent".into(), |c| format!("{}", c.len())),
    );
    println!("{}", ocrcer_bench::provenance::line(std::path::Path::new(model)));

    let oracle_reader = if oracle { Some(OracleBank::build(m)?) } else { None };

    let chosen = select_pages(dir, limit, stride, offset, only)?;
    let n = chosen.len();

    let mut e2e = Column::default();
    let mut orc = Column::default();
    let mut by_size: BTreeMap<u32, Score> = BTreeMap::new();
    let mut by_family: BTreeMap<String, Score> = BTreeMap::new();
    let mut subs: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut ink_pixels = 0u64;
    let mut read_nanos = 0u128;

    for pgm in &chosen {
        let stem = pgm.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let truth = load_truth_beside(pgm)?;
        let (w, h, grey) = load_page(pgm)?;
        let reference = truth.lines.join("\n");

        let t0 = Instant::now();
        let lines = engine
            .recognize_lines(Gray { width: w, height: h, data: &grey })
            .map_err(|e| format!("{stem}: {e:?}"))?;
        read_nanos += t0.elapsed().as_nanos();
        ink_pixels += u64::from(w) * u64::from(h);

        let read = page_text(&lines);

        if e2e.pages.len() < show {
            println!("--- {stem}");
            for (r, g) in reference.lines().zip(read.lines().chain(std::iter::repeat(""))) {
                println!("  want {r}");
                println!("  read {g}");
            }
            println!();
        }
        e2e.add(&stem, &reference, &read);
        let s = score(&reference, &read);
        by_size.entry(truth.px_per_em.round() as u32).or_default().add(&s);
        by_family.entry(truth.family.clone()).or_default().add(&s);
        align_substitutions(&reference, &read, &mut subs);

        if let Some(o) = &oracle_reader {
            orc.add(&stem, &reference, &o.read(&truth, &grey, w, h));
        }
    }

    println!("{}", e2e.line("end-to-end"));
    println!("{}", e2e.line_matched_line("line-matched"));
    if oracle_reader.is_some() {
        println!("{}", orc.line("oracle-seg"));
        println!("{}", orc.line_matched_line("line-matched"));
        let (ce, co) = (e2e.seq.cer().unwrap_or(0.0), orc.seq.cer().unwrap_or(0.0));
        if co > ce {
            // Checked, not asserted in prose. The oracle is handed every glyph
            // box, so it cannot score worse unless the two columns stopped
            // being the same recogniser.
            println!(
                "
BROKEN      the oracle column scored BELOW end to end (CER {:.3}% vs {:.3}%).
                             It is handed every glyph box, so it cannot be worse unless the
                             two columns are not the same recogniser. Quote neither until
                             that is found.",
                co * 100.0,
                ce * 100.0
            );
        } else {
            println!(
                "
gap         the layout stages and decoder move CER by {:+.3} points
                             (oracle is a ceiling: it is handed every glyph box)",
                (ce - co) * 100.0
            );
        }
    }

    let secs = read_nanos as f64 / 1e9;
    println!(
        "\nspeed       {:.1}s for {n} pages, {:.0} ms/page, {:.1} Mpixel/s \
         (recognition only; loading, scoring and rendering excluded)",
        secs,
        secs * 1000.0 / n.max(1) as f64,
        ink_pixels as f64 / 1e6 / secs.max(1e-9),
    );

    println!("\nby px/em");
    for (px, s) in &by_size {
        println!(
            "  {px:>3}px   CER {:6.3}%  WER {:6.3}%  ({} chars)",
            s.cer().unwrap_or(0.0) * 100.0,
            s.wer().unwrap_or(0.0) * 100.0,
            s.chars
        );
    }

    println!("\nby face");
    let mut fam: Vec<(&String, &Score)> = by_family.iter().collect();
    fam.sort_by(|a, b| {
        b.1.cer().unwrap_or(0.0).partial_cmp(&a.1.cer().unwrap_or(0.0)).unwrap()
    });
    for (name, s) in fam {
        println!(
            "  {:6.3}%  {name} ({} chars)",
            s.cer().unwrap_or(0.0) * 100.0,
            s.chars
        );
    }

    if worst > 0 {
        let mut top: Vec<((String, String), usize)> = subs.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        println!("\ntop confusions (aligned, end-to-end)");
        for ((want, got), n) in top.into_iter().take(worst) {
            println!("  {n:>6}  {want:?} -> {got:?}");
        }

        e2e.pages.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)));
        println!("\nworst pages");
        for (c, stem) in e2e.pages.into_iter().take(worst) {
            println!("  {:6.2}%  {stem}", c * 100.0);
        }
    }

    if line_pages {
        e2e.line_pages.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)));
        println!("\nworst pages (line-matched)");
        for (c, stem) in e2e.line_pages.into_iter().take(worst) {
            println!("  {:6.2}%  {stem}", c * 100.0);
        }
    }
    Ok(())
}

/// Counts aligned substitutions, insertions and deletions between two strings.
///
/// A plain per-position diff would be useless once one character is dropped:
/// everything after the drop reads as wrong. This walks the edit path the
/// Levenshtein distance implies, so a deletion is charged once as a deletion
/// rather than as a page of substitutions. Insertions and deletions are
/// reported with `""` on the side that has nothing, which is what makes a
/// split or merged glyph visible in the list at all.
fn align_substitutions(
    reference: &str,
    read: &str,
    into: &mut BTreeMap<(String, String), usize>,
) {
    let a: Vec<char> = ocrcer_bench::cer::normalise(reference).chars().collect();
    let b: Vec<char> = ocrcer_bench::cer::normalise(read).chars().collect();
    // Full matrix: these are page-sized strings, a few thousand characters,
    // so the quadratic table is a few megabytes and is gone at the end of the
    // call. Correct alignment is worth more here than the row-at-a-time trick
    // `cer::levenshtein` uses, because the path itself is the output.
    let (n, m) = (a.len(), b.len());
    if n * m > 16_000_000 {
        return;
    }
    let mut d = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in 0..=n {
        d[at(i, 0)] = i as u32;
    }
    for j in 0..=m {
        d[at(0, j)] = j as u32;
    }
    for i in 1..=n {
        for j in 1..=m {
            let sub = d[at(i - 1, j - 1)] + u32::from(a[i - 1] != b[j - 1]);
            d[at(i, j)] = sub.min(d[at(i - 1, j)] + 1).min(d[at(i, j - 1)] + 1);
        }
    }
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && d[at(i, j)] == d[at(i - 1, j - 1)] + u32::from(a[i - 1] != b[j - 1]) {
            if a[i - 1] != b[j - 1] {
                *into.entry((a[i - 1].to_string(), b[j - 1].to_string())).or_default() += 1;
            }
            i -= 1;
            j -= 1;
        } else if i > 0 && d[at(i, j)] == d[at(i - 1, j)] + 1 {
            *into.entry((a[i - 1].to_string(), String::new())).or_default() += 1;
            i -= 1;
        } else {
            *into.entry((String::new(), b[j - 1].to_string())).or_default() += 1;
            j -= 1;
        }
    }
}

/// The oracle-segmented comparison column: a bank built in-process from
/// `model/`, read through `pages::read_with_bank`.
/// The oracle column's recogniser: the same bank the model file carries,
/// rebuilt in process so glyph boxes can be handed to it directly.
///
/// # Why this takes everything from the model file
///
/// The column's whole claim is to bound the end-to-end column, which holds
/// only if the two are the same recogniser differing in segmentation alone.
/// Both the render ladder and the geometry weight used to be literals here,
/// and both had gone stale: the bank was built at `[12, 16, 24, 32]` while
/// the shipped file carried `[16, 20, 24, 32, 48]`, and the weight was left
/// at 1 while the file carried 6. The result was a "ceiling" that scored
/// **below** the column it bounds and a printed gap line asserting the
/// opposite. Nothing here restates a default any more.
struct OracleBank {
    bank: ocrcer_build::bank::Bank,
    index_to_char: BTreeMap<u16, char>,
    probe: Option<ocrcer_bench::pages::Restricted>,
}

impl OracleBank {
    fn build(m: &ocrcer_core::ocrw::Model) -> Result<OracleBank, String> {
        use ocrcer_build::{bank, tables};
        let dir = tables::model_dir();
        let classes = tables::load_charset(&dir)?;
        let entries = tables::load_fonts(&dir)?;
        let fonts = bank::Fonts::load(&entries, true);
        let (faces, renderers, failed) = fonts.renderers();
        for line in &failed {
            eprintln!("face failed to parse: {line}");
        }
        let px: Vec<f32> = m.sizes.clone();
        let bank = bank::build(&classes, &faces, &renderers, &px);
        let index_to_char: BTreeMap<u16, char> =
            classes.iter().map(|c| (c.index, c.codepoint)).collect();

        // Mirror the file's geometry weight. One group scalar can only do
        // that while the four dimensions agree; when they do not, say so and
        // fall back to the unweighted probe rather than claim a ceiling.
        let w = m.weights;
        let g = w[103];
        let probe = if w[103..107].iter().any(|x| *x != g) {
            eprintln!(
                "oracle matcher: the file weights dims 103..107 unequally and one group scalar cannot mirror that; the oracle column is NOT a ceiling in this run"
            );
            None
        } else if g != 1.0 {
            eprintln!("oracle matcher: dims 103..107 weighted x{g}, taken from the engine file");
            Some(
                ocrcer_bench::pages::Restricted::new(&index_to_char, |_| true)
                    .with_group_weight(103..107, g),
            )
        } else {
            None
        };

        eprintln!(
            "oracle bank: {} prototypes from {} faces at {px:?} px/em, taken from the engine file",
            bank.prototypes.len(),
            faces.len()
        );
        Ok(OracleBank { bank, index_to_char, probe })
    }

    fn read(
        &self,
        truth: &ocrcer_bench::pages::Truth,
        grey: &[u8],
        width: u32,
        height: u32,
    ) -> String {
        // The page is binarized once, with the shipped defaults, because the
        // oracle column's claim is that layout and segmentation are removed
        // -- not that thresholding is.
        let mask = ocrcer_bench::pages::binarize_page(
            grey,
            width,
            height,
            &ocrcer_core::params::Params::DEFAULT,
        );
        ocrcer_bench::pages::read_with_bank(
            &self.bank,
            &self.index_to_char,
            truth,
            &mask,
            width,
            ocrcer_build::bank::Gate::None,
            self.probe.as_ref(),
        )
        .text
    }
}

/// What a correct character's match distance actually looks like.
///
/// # Why this mode exists
///
/// The decoder's score is a sum over characters of a negative match term, so
/// a path that explains the same ink with *fewer* characters starts ahead
/// before any evidence is weighed — every extra character it declines to
/// emit is a cost it does not pay. Offsetting that needs a per-character
/// credit, and a credit is only principled if it is the distance a correct
/// character actually costs. That number is not guessable, so it is measured
/// here: every glyph on the corpus is cropped at its known box, matched
/// against the shipped model, and its top-1 distance recorded.
///
/// Printed separately for glyphs the matcher got right and glyphs it got
/// wrong, because a credit set from the combined distribution would be
/// inflated by the failures it is meant to be neutral about.
///
/// This is oracle-segmented on purpose. The question is what a *correct*
/// single character costs, and only the truth boxes know which crops those
/// are.
fn run_distances(
    model: &str,
    dir: &str,
    limit: usize,
    stride: usize,
    offset: usize,
    only: Option<&str>,
) -> Result<(), String> {
    use ocrcer_core::feature::{extract, GlyphInput};

    let bytes = std::fs::read(model).map_err(|e| format!("{model}: {e}"))?;
    let m = ocrcer_core::ocrw::Model::load(&bytes).map_err(|e| format!("{model}: {e:?}"))?;
    let pgms = select_pages(dir, limit, stride, offset, only)?;

    let mut right: Vec<f32> = Vec::new();
    let mut wrong: Vec<f32> = Vec::new();
    let mut by_size: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
    let mut confused: BTreeMap<(char, char), usize> = BTreeMap::new();
    for pgm in pgms.iter() {
        let truth = load_truth_beside(pgm)?;
        let (w, _h, grey) = load_page(pgm)?;
        for g in &truth.glyphs {
            if g.w == 0 || g.h == 0 {
                continue;
            }
            let mut ink = vec![0u8; (g.w * g.h) as usize];
            for r in 0..g.h {
                for c in 0..g.w {
                    ink[(r * g.w + c) as usize] =
                        u8::from(grey[((g.y + r) * w + g.x + c) as usize] < 128);
                }
            }
            let f = extract(&GlyphInput {
                ink: &ink,
                width: g.w,
                height: g.h,
                baseline_dy: g.baseline as f32 - g.y as f32,
                x_height: g.x_height,
            });
            let Some(hit) = ocrcer_core::r#match::nearest(&m, &f, 1) else { continue };
            let Some(top) = hit.best.first() else { continue };
            let px = truth.px_per_em.round() as u32;
            let e = by_size.entry(px).or_default();
            e.1 += 1;
            if m.char_of(top.class) == Some(g.ch) {
                right.push(top.distance);
                e.0 += 1;
            } else {
                wrong.push(top.distance);
                let got = m.char_of(top.class).unwrap_or('?');
                *confused.entry((g.ch, got)).or_default() += 1;
            }
        }
    }

    let report = |name: &str, v: &mut Vec<f32>| {
        if v.is_empty() {
            println!("{name:<10} none");
            return;
        }
        v.sort_by(|a, b| a.partial_cmp(b).expect("distances are finite"));
        let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
        let mean = v.iter().map(|x| f64::from(*x)).sum::<f64>() / v.len() as f64;
        println!(
            "{name:<10} n {:>7}  mean {:6.3}  p10 {:6.3}  p50 {:6.3}  p75 {:6.3}  p90 {:6.3}  p99 {:6.3}",
            v.len(),
            mean,
            at(0.10),
            at(0.50),
            at(0.75),
            at(0.90),
            at(0.99),
        );
    };
    let total = right.len() + wrong.len();
    println!(
        "glyphs {total} from {} pages; top-1 correct {:.3}%
",
        pgms.len().min(limit),
        right.len() as f64 * 100.0 / total.max(1) as f64
    );
    report("correct", &mut right);
    report("wrong", &mut wrong);

    println!("
top-1 by px/em");
    for (px, (ok, n)) in &by_size {
        println!("  {px:>3}px   {:6.3}%  ({n} glyphs)", *ok as f64 * 100.0 / *n as f64);
    }

    let mut top: Vec<((char, char), usize)> = confused.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    println!("
top matcher confusions (oracle boxes, no decoder)");
    for ((want, got), n) in top.into_iter().take(25) {
        println!("  {n:>6}  {want:?} -> {got:?}");
    }
    Ok(())
}

/// The whole-page reference and read strings, one pair per page, undivided by
/// any per-line zip.
///
/// # Why this exists alongside `--show`
///
/// `--show` prints `reference.lines().zip(read.lines())`, which is exactly
/// what `run`'s own scoring does -- but that zip silently misaligns the
/// instant the two sides disagree on line *count*, which a column-cut band
/// serialized across several output lines does routinely (`ARCHITECTURE.md`
/// section 11's recorded, not-yet-acted-on `" " -> "\n"` finding). A diagnosis
/// that needs to see which characters actually differ has to align the two
/// whole-page strings the way `cer::score` does, not re-pair truth lines with
/// output lines by position. This prints both strings each as one line, `\n`
/// escaped literally so a caller can align them itself without that
/// confound. Diagnostic only, added for the 2026-09-22 fixed-pitch detector
/// diagnosis; not part of any scored figure.
fn run_raw(
    model: &str,
    dir: &str,
    limit: usize,
    stride: usize,
    offset: usize,
    only: Option<&str>,
    set: &[(String, f32)],
) -> Result<(), String> {
    let engine = engine_with(model, set)?;
    for pgm in select_pages(dir, limit, stride, offset, only)?.iter() {
        let stem = pgm.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let truth = load_truth_beside(pgm)?;
        let (w, h, grey) = load_page(pgm)?;
        let lines = engine
            .recognize_lines(Gray { width: w, height: h, data: &grey })
            .map_err(|e| format!("{stem}: {e:?}"))?;
        let reference = truth.lines.join("\n");
        let read = page_text(&lines);
        println!("@@@ {stem}");
        println!("REF {}", reference.replace('\n', "\\n"));
        println!("GOT {}", read.replace('\n', "\\n"));
    }
    Ok(())
}

/// What the layout stages decided, per line, before anything was recognised.
///
/// Prints the line's measured x-height and where it came from, the gap
/// multiset, and the space threshold with its separability — the four things
/// that decide whether two words come back as two words. Calls the same public
/// stage functions `Engine::recognize_lines` calls, in the same order; it is a
/// second *caller*, not a second implementation.
///
/// Also prints each fragment's decoded words (`Engine::recognize_lines`
/// called a second time, full page), so a caller attributing a spurious or
/// missing space can see what the fragment actually read instead of
/// inferring it from whole-page text.
fn run_layout(
    model: &str,
    dir: &str,
    limit: usize,
    stride: usize,
    offset: usize,
    only: Option<&str>,
    set: &[(String, f32)],
    no_decode: bool,
) -> Result<(), String> {
    use ocrcer_core::image::{binarize, components, deskew};
    use ocrcer_core::layout::{lines, words};

    let engine = engine_with(model, set)?;
    let p = engine.params();

    for pgm in select_pages(dir, limit, stride, offset, only)?.iter() {
        let (w, h, grey) = load_page(pgm)?;
        let truth = load_truth_beside(pgm)?;
        let img = Gray { width: w, height: h, data: &grey };
        let mask = binarize::binarize_with(&img, &p.binarize());
        let slope = deskew::estimate_with(&mask, w, h, f64::from(p.deskew.max_slope));
        let page = deskew::correct_with(&img, slope, f64::from(p.deskew.min_corrected_slope));
        let gray = page.gray();
        let mask = binarize::binarize_with(&gray, &p.binarize());
        let (labels, count) =
            components::label(&mask, page.width, page.height, components::Connectivity::Eight);
        let comps = components::components(&labels, page.width, page.height, count);

        // Decoded words per fragment, so a caller attributing a spurious or
        // missing space can see what the fragment actually read rather than
        // inferring it from whole-page text (`ARCHITECTURE.md` section 11's
        // amendment: "`--layout` should print each fragment's decoded words
        // so the remainder can be attributed rather than inferred"). Calls
        // `Engine::recognize_lines` a second time -- the same public entry
        // point every scored run in this binary uses -- rather than
        // reimplementing matching or decoding here. Keyed by each
        // fragment's own `(x0, x1, y0)`: `y0` is required, not merely
        // belt-and-braces, because a column-cut invoice table reuses the
        // same `(x0, x1)` for every row of one column, and `x0, x1` alone
        // collided across rows -- confirmed empirically (2026-09-22): a
        // "Total 875.75" row's `want` line paired with a `decoded` array
        // that was actually an earlier row's ("Dowel pin ...") content,
        // because `recognize_lines` drops a line/fragment outright when
        // every one of its spans decodes to empty text, desynchronising a
        // same-key FIFO queue permanently for the rest of the page from
        // that point on. `recognize_lines`' `Rect.y` is the *unsheared*
        // `y0` (`pipeline.rs`'s own `unshear(line.y0, line.x0, slope, ...)`,
        // duplicated below rather than exposed -- four lines of coordinate
        // arithmetic, not a pipeline stage), so the key replicates that
        // same transform on this loop's own `line.y0`; skipping it would
        // silently miss every fragment on the ~8% of corpus pages this
        // pipeline measures a nonzero residual slope on.
        let mut decoded_words: std::collections::HashMap<
            (u32, u32, u32),
            std::collections::VecDeque<Vec<String>>,
        > = std::collections::HashMap::new();
        if !no_decode {
            let decoded = engine
                .recognize_lines(Gray { width: w, height: h, data: &grey })
                .map_err(|e| format!("{}: {e:?}", pgm.display()))?;
            for dl in &decoded {
                let key = (dl.rect.x, dl.rect.x + dl.rect.width, dl.rect.y);
                decoded_words
                    .entry(key)
                    .or_default()
                    .push_back(dl.words.iter().map(|w| w.text.clone()).collect());
            }
        }

        println!(
            "=== {} — {}x{}, {} components, slope {slope:+.4}",
            pgm.file_stem().unwrap_or_default().to_string_lossy(),
            w,
            h,
            comps.len()
        );
        let mut i = 0usize;
        for group in lines::group_with_bands(&comps, page.width, page.height, &p.lines()) {
            let rules = words::band_space_rules(&group, &comps, &p.words());
            let spans_by_line = words::split_band_with(&group, &comps, &p.words());
            let n_fragments = group.len();
            for (j, ((line, rule), spans)) in
                group.iter().zip(&rules).zip(spans_by_line).enumerate()
            {
                let g = words::gaps(line, &comps);
                let band = if n_fragments > 1 {
                    format!(", band fragment {} of {n_fragments}", j + 1)
                } else {
                    String::new()
                };
                println!(
                    "  line {i}: x {}..{}, {} members, x-height {:.2} ({:?}), cap {:.2}, median {}, baseline {:.1}{band}",
                    line.x0,
                    line.x1,
                    line.members.len(),
                    line.x_height,
                    line.x_height_source,
                    line.cap_height,
                    line.median_height,
                    line.baseline,
                );
                println!(
                    "    want {:?}",
                    truth.lines.get(i).map(String::as_str).unwrap_or("<no such line>")
                );
                println!("    gaps {g:?}");
                println!(
                    "    threshold {} ({:?}, eta {:.3}) -> {} words",
                    rule.threshold,
                    rule.source,
                    rule.separability,
                    spans.len()
                );
                // Diagnostic only (2026-09-22 fixed-pitch detector diagnosis,
                // amended 2026-09-22 for the cell-merge amendment, extended
                // 2026-09-22 for the amendment ablation): centre-to-centre
                // distances and the median pitch they imply, printed only
                // for fragments the fixed-pitch test actually fired on, plus
                // the raw per-member component boxes. Recomputed here rather
                // than exposed from `words::cells`/`cell_distances`
                // (private) -- plain arithmetic over `Component::x0`/`x1`,
                // not a second implementation of a pipeline stage. Honours
                // `words.pitch_cell_merge` so the printed `d`/`p` matches
                // whichever ablation arm actually ran the classifier.
                if rule.source == words::ThresholdSource::FixedPitch {
                    let merge_cells = p.words.pitch_cell_merge != 0;
                    let mut cell_boxes: Vec<(u32, u32)> = Vec::new();
                    for &idx in &line.members {
                        let c = &comps[idx];
                        match cell_boxes.last_mut() {
                            Some((_, x1)) if merge_cells && c.x0 < *x1 => {
                                *x1 = (*x1).max(c.x1);
                            }
                            _ => cell_boxes.push((c.x0, c.x1)),
                        }
                    }
                    let centres: Vec<f64> =
                        cell_boxes.iter().map(|&(x0, x1)| (f64::from(x0) + f64::from(x1)) / 2.0).collect();
                    let d: Vec<f64> = centres.windows(2).map(|w| w[1] - w[0]).collect();
                    let mut sorted = d.clone();
                    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let pitch = if sorted.is_empty() {
                        0.0
                    } else if sorted.len() % 2 == 1 {
                        sorted[sorted.len() / 2]
                    } else {
                        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
                    };
                    let boxes: Vec<(u32, u32, u32, u32)> = line
                        .members
                        .iter()
                        .map(|&idx| (comps[idx].x0, comps[idx].y0, comps[idx].x1, comps[idx].y1))
                        .collect();
                    println!(
                        "    pitch p={pitch:.3}  d={d:.2?}  ({} cells from {} members)  boxes(x0,y0,x1,y1)={boxes:?}",
                        cell_boxes.len(),
                        line.members.len()
                    );
                }
                let unsheared_y0 = if slope == 0.0 {
                    line.y0
                } else {
                    let dy = f64::from(line.x0) * slope;
                    (f64::from(line.y0) + dy)
                        .clamp(0.0, f64::from(page.height.saturating_sub(1)))
                        as u32
                };
                if no_decode {
                    println!("    decoded <skipped, --no-decode>");
                } else {
                    let words = decoded_words
                        .get_mut(&(line.x0, line.x1, unsheared_y0))
                        .and_then(std::collections::VecDeque::pop_front)
                        .unwrap_or_default();
                    println!("    decoded {words:?}");
                }
                i += 1;
            }
        }
    }
    Ok(())
}
