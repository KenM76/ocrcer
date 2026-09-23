//! `vs-ocrs`: OCRcer against the engine `pdfcer` ships today, on the same
//! pixels.
//!
//! ```text
//! vs-ocrs <pages-dir> <ocrs-model-dir> <bank-sizes> [--engine <model.ocrw>]
//!         [--limit N] [--show] [--geometry-weight W] [--no-oracle]
//! ```
//!
//! `pages-dir` holds what `ocrcer-build pages` wrote: a `.pgm` and a
//! `.truth.json` per page. `ocrs-model-dir` holds `text-detection.rten` and
//! `text-rec-checkpoint.rten` — files already on the machine; nothing is
//! downloaded and nothing is vendored. `bank-sizes` is the px/em list the
//! oracle column's prototype bank is built at.
//!
//! # The three columns, and which of them is a result
//!
//! **OCRcer (end to end)** — `--engine` — is the shipped `.ocrw` model driven
//! by [`ocrcer_core::pipeline::Engine`], handed the page and nothing else.
//! It binarises, deskews, finds components, groups lines, splits words,
//! builds a segmentation lattice, matches and decodes. Everything it gets
//! wrong counts against it, exactly as for `ocrs`. **This is the only column
//! that is a like-for-like comparison, and the only one to quote against
//! `ocrs`.**
//!
//! **ocrs** is given a page and nothing else, the same way.
//!
//! **OCRcer (oracle seg)** is a bank and a nearest-neighbour search handed
//! the **exact** bounding box of every mark of ink from the page's own
//! ground truth, and the exact positions of the spaces. It cannot lose a
//! line, split a glyph, or join two. That makes it a **ceiling**, not a
//! result: the gap between it and the end-to-end column is precisely what
//! the layout and segmentation stages are costing, which is why it is worth
//! printing beside them rather than deleting.
//!
//! That reading holds only while the two OCRcer columns run the **same
//! matcher**. With `--engine` and no explicit `--geometry-weight`, the oracle
//! column takes its geometry weight from the engine file for exactly that
//! reason; pass the flag only to measure a candidate weight the file does not
//! carry, and read the oracle column as a different matcher when you do.
//!
//! Three biases in the corpus, all stated:
//!
//! - **Favours OCRcer.** The corpus is confined to `charset.tsv` (see
//!   `corpus.rs`), a charset chosen to fit the test. `ocrs` had no such say.
//! - **Favours OCRcer.** The pages have no noise, skew or scanner blur.
//!   `ocrs` is a neural recogniser trained on real scans and photographs;
//!   this is not the input it was built for. A scanned corpus is where
//!   `ocrs` should be expected to look better than it does here.
//! - **Favours neither, and damages both.** The pages are rasterised
//!   two-level with no dropout control, so hairlines are lost rather than
//!   thinned: measured over the corpus, 9.52% of single-contour glyphs at
//!   14 px/em arrive as two or more disjoint pieces (Open Sans Condensed
//!   Light: 47.9%), and 202 truth glyphs at that size have ink boxes two
//!   pixels tall or less. No real rasteriser or scanner does this — see
//!   `ARCHITECTURE.md` section 11's 2026-09-22 renderer entry. Relative
//!   figures from this corpus stand; absolute ones are a lower bound until
//!   the pages are re-rendered with coverage output.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use ocrcer_bench::cer::{score, token_score, Score, TokenScore};
use ocrcer_bench::pages::{
    list_pages, load_page, load_truth_beside, page_text, read_with_bank, Restricted,
};
use ocrcer_build::{bank, tables};
use ocrcer_core::pipeline::Engine;
use ocrcer_core::Gray;
use ocrs::{ImageSource, OcrEngineParams};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let (pages, models, sizes, rest) = match argv.as_slice() {
        [p, m, s, rest @ ..] => (*p, *m, *s, rest),
        _ => {
            eprintln!(
                "usage: vs-ocrs <pages-dir> <ocrs-model-dir> <bank-sizes> \
                 [--engine <model.ocrw>] [--limit N] [--show] \
                 [--geometry-weight W] [--no-oracle]"
            );
            return ExitCode::FAILURE;
        }
    };
    let mut limit = usize::MAX;
    let mut show = false;
    let mut engine_path: Option<String> = None;
    let mut oracle = true;
    // A candidate weight on `ARCHITECTURE.md` section 3.1's dims 103..107,
    // applied to the oracle column so the head-to-head can be re-run without
    // waiting for a weight to be authored into the model file.
    //
    // `None` means "take it from the engine file", not "1.0". The oracle
    // column is a ceiling on the end-to-end column only if the two are the
    // same matcher; hard-wired to 1.0 it silently became a *different*
    // matcher the moment `feature_weights` shipped, and the unweighted
    // oracle then scored BELOW end to end -- a ceiling that is not one is
    // worse than no ceiling, because it reads as a measurement of what
    // layout and segmentation cost.
    let mut geometry_weight: Option<f32> = None;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match *a {
            "--show" => show = true,
            "--no-oracle" => oracle = false,
            "--engine" => match it.next() {
                Some(p) => engine_path = Some((*p).to_string()),
                None => {
                    eprintln!("vs-ocrs: --engine wants a path to a .ocrw file");
                    return ExitCode::FAILURE;
                }
            },
            "--limit" => match it.next().and_then(|n| n.parse().ok()) {
                Some(n) => limit = n,
                None => {
                    eprintln!("vs-ocrs: --limit wants an integer");
                    return ExitCode::FAILURE;
                }
            },
            "--geometry-weight" => match it.next().and_then(|w| w.parse::<f32>().ok()) {
                Some(w) if w > 0.0 => geometry_weight = Some(w),
                _ => {
                    eprintln!("vs-ocrs: --geometry-weight wants a positive float");
                    return ExitCode::FAILURE;
                }
            },
            other => {
                eprintln!("vs-ocrs: unexpected argument {other:?}");
                return ExitCode::FAILURE;
            }
        }
    }
    if !oracle && engine_path.is_none() {
        eprintln!("vs-ocrs: --no-oracle without --engine leaves no OCRcer column");
        return ExitCode::FAILURE;
    }
    match run(pages, models, sizes, limit, show, geometry_weight, engine_path.as_deref(), oracle) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vs-ocrs: {e}");
            ExitCode::FAILURE
        }
    }
}

fn ocrs_read(
    engine: &ocrs::OcrEngine,
    grey: &[u8],
    width: u32,
    height: u32,
) -> Result<String, String> {
    let source = ImageSource::from_bytes(grey, (width, height)).map_err(|e| e.to_string())?;
    let input = engine.prepare_input(source).map_err(|e| e.to_string())?;
    let rects = engine.detect_words(&input).map_err(|e| e.to_string())?;
    let lines = engine.find_text_lines(&input, &rects);
    let read = engine.recognize_text(&input, &lines).map_err(|e| e.to_string())?;
    Ok(read.into_iter().flatten().map(|l| l.to_string()).collect::<Vec<_>>().join("\n"))
}

/// Accumulated scores for one engine, overall and split by the two axes the
/// corpus varies: the block of text and the render size.
#[derive(Default)]
struct Totals {
    overall: Score,
    tokens: TokenScore,
    by_block: BTreeMap<String, Score>,
    by_block_tokens: BTreeMap<String, TokenScore>,
    by_size: BTreeMap<String, Score>,
    by_size_tokens: BTreeMap<String, TokenScore>,
    by_family: BTreeMap<String, Score>,
    by_family_tokens: BTreeMap<String, TokenScore>,
    seconds: f64,
}

impl Totals {
    /// Scores one page's reading against its reference and files it under
    /// every axis the report breaks out.
    fn add(&mut self, block: &str, family: &str, px: f32, reference: &str, read: &str) {
        let s = score(reference, read);
        let t = token_score(reference, read);
        self.overall.add(&s);
        self.tokens.add(&t);
        self.by_block.entry(block.to_string()).or_default().add(&s);
        self.by_block_tokens.entry(block.to_string()).or_default().add(&t);
        self.by_size.entry(format!("{px}")).or_default().add(&s);
        self.by_size_tokens.entry(format!("{px}")).or_default().add(&t);
        self.by_family.entry(family.to_string()).or_default().add(&s);
        self.by_family_tokens.entry(family.to_string()).or_default().add(&t);
    }
}

/// One named column of the report.
struct Col<'a> {
    name: &'a str,
    t: &'a Totals,
}

#[allow(clippy::too_many_arguments)]
fn run(
    pages_dir: &str,
    models_dir: &str,
    bank_sizes: &str,
    limit: usize,
    show: bool,
    geometry_weight: Option<f32>,
    engine_path: Option<&str>,
    want_oracle: bool,
) -> Result<(), String> {
    // --- OCRcer end-to-end side: the shipped file, driven as a caller would.
    let engine = match engine_path {
        Some(p) => {
            let bytes = std::fs::read(p).map_err(|e| format!("{p}: {e}"))?;
            let e = Engine::from_bytes(&bytes).map_err(|e| format!("{p}: {e:?}"))?;
            let m = e.model();
            println!(
                "engine: {p} — {} classes, {} prototypes, {} lexicon nodes",
                m.classes.len(),
                m.n_prototypes(),
                m.lexicon.as_ref().map_or(0, |l| l.nodes())
            );
            println!("{}", ocrcer_bench::provenance::line(std::path::Path::new(p)));
            Some(e)
        }
        None => None,
    };

    // Resolve the oracle column's matcher to the engine file's, unless the
    // caller named a candidate weight explicitly. One group scalar can mirror
    // the file only when the four geometry dimensions agree; when they do not,
    // the oracle column is dropped rather than quoted as a ceiling it is not.
    let mut want_oracle = want_oracle;
    let geometry_weight = match geometry_weight {
        Some(w) => w,
        None => match engine.as_ref() {
            Some(e) => {
                let w = e.model().weights;
                let g = w[103];
                if w[103..107].iter().any(|x| *x != g) {
                    println!("oracle column dropped: the engine file weights the four geometry dimensions unequally, and one group scalar cannot mirror that");
                    want_oracle = false;
                }
                if g != 1.0 {
                    println!("oracle matcher: dims 103..107 weighted x{g}, taken from the engine file, so the two OCRcer columns differ only in segmentation");
                }
                g
            }
            None => 1.0,
        },
    };

    // --- OCRcer oracle side: the bank the shipped file would carry. ---
    let px_list: Vec<f32> = bank_sizes
        .split(',')
        .map(|p| p.trim().parse::<f32>().map_err(|_| format!("bad px/em {p:?}")))
        .collect::<Result<_, _>>()?;
    let oracle = if want_oracle {
        let dir = tables::model_dir();
        let classes = tables::load_charset(&dir)?;
        let entries = tables::load_fonts(&dir)?;
        let fonts = bank::Fonts::load(&entries, false);
        let (faces, renderers, failed) = fonts.renderers();
        for line in &failed {
            eprintln!("face failed to parse: {line}");
        }
        let index_to_char: BTreeMap<u16, char> =
            classes.iter().map(|c| (c.index, c.codepoint)).collect();
        let started = std::time::Instant::now();
        let b = bank::build(&classes, &faces, &renderers, &px_list);
        println!(
            "oracle bank: {} prototypes from {} faces at {:?} px/em in {:.1}s",
            b.prototypes.len(),
            faces.len(),
            px_list,
            started.elapsed().as_secs_f64()
        );
        // `None` at weight 1.0 rather than an all-ones probe, deliberately:
        // it keeps an unweighted run on byte-for-byte the same code path as
        // the measurements already recorded, so the two remain comparable.
        let probe = (geometry_weight != 1.0).then(|| {
            Restricted::new(&index_to_char, |_| true).with_group_weight(103..107, geometry_weight)
        });
        Some((b, index_to_char, probe))
    } else {
        None
    };

    // --- ocrs side: the exact weight files pdfcer ships. ---
    let md = Path::new(models_dir);
    let load = |name: &str| -> Result<rten::Model, String> {
        let p = md.join(name);
        if !p.is_file() {
            return Err(format!("missing {}", p.display()));
        }
        rten::Model::load_file(&p).map_err(|e| format!("{}: {e}", p.display()))
    };
    let theirs_engine = ocrs::OcrEngine::new(OcrEngineParams {
        detection_model: Some(load("text-detection.rten")?),
        recognition_model: Some(load("text-rec-checkpoint.rten")?),
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;

    let pgms = list_pages(pages_dir)?;
    let total = pgms.len().min(limit);

    let mut e2e = Totals::default();
    let mut orc = Totals::default();
    let mut ocrs_t = Totals::default();

    for (i, pgm) in pgms.iter().take(limit).enumerate() {
        let stem = pgm.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let block = stem.split("__").nth(2).unwrap_or("?").to_string();
        let truth = load_truth_beside(pgm)?;
        let (w, h, grey) = load_page(pgm)?;
        let reference = truth.lines.join("\n");

        let mut mine_e2e = String::new();
        if let Some(e) = &engine {
            let t0 = std::time::Instant::now();
            let lines = e
                .recognize_lines(Gray { width: w, height: h, data: &grey })
                .map_err(|err| format!("{stem}: {err:?}"))?;
            e2e.seconds += t0.elapsed().as_secs_f64();
            mine_e2e = page_text(&lines);
            e2e.add(&block, &truth.family, truth.px_per_em, &reference, &mine_e2e);
        }

        let mut mine_oracle = String::new();
        if let Some((b, index_to_char, probe)) = &oracle {
            let t0 = std::time::Instant::now();
            // The same binarizer, with the same parameters, that the
            // end-to-end column runs: the two columns are meant to differ
            // only in segmentation, and a column that thresholds the page
            // differently is not a ceiling on one that does not.
            let params = match &engine {
                Some(e) => e.params().clone(),
                None => ocrcer_core::params::Params::DEFAULT,
            };
            let mask = ocrcer_bench::pages::binarize_page(&grey, w, h, &params);
            mine_oracle = read_with_bank(
                b,
                index_to_char,
                &truth,
                &mask,
                w,
                bank::Gate::Holes,
                probe.as_ref(),
            )
            .text;
            orc.seconds += t0.elapsed().as_secs_f64();
            orc.add(&block, &truth.family, truth.px_per_em, &reference, &mine_oracle);
        }

        let t1 = std::time::Instant::now();
        let theirs = ocrs_read(&theirs_engine, &grey, w, h)?;
        ocrs_t.seconds += t1.elapsed().as_secs_f64();
        ocrs_t.add(&block, &truth.family, truth.px_per_em, &reference, &theirs);

        if show {
            println!("\n=== {stem}");
            let mut rows: Vec<(&str, &String)> = vec![("ref    ", &reference)];
            if engine.is_some() {
                rows.push(("ocrcer ", &mine_e2e));
            }
            if oracle.is_some() {
                rows.push(("oracle ", &mine_oracle));
            }
            rows.push(("ocrs   ", &theirs));
            for (tag, text) in rows {
                for l in ocrcer_bench::cer::normalise(text).lines() {
                    println!("  {tag} | {l}");
                }
            }
        } else {
            eprint!("\r{}/{total} {stem}                    ", i + 1);
        }
    }
    eprintln!();

    let mut cols: Vec<Col<'_>> = Vec::new();
    if engine.is_some() {
        cols.push(Col { name: "OCRcer e2e", t: &e2e });
    }
    cols.push(Col { name: "ocrs 0.12.2", t: &ocrs_t });
    if oracle.is_some() {
        cols.push(Col { name: "OCRcer oracle", t: &orc });
    }
    report(&cols, total, engine.is_some());

    // The oracle column is *defined* as an upper bound on the end-to-end
    // column, so check it rather than trusting the definition. This has
    // already failed twice for two different reasons -- a matcher weight the
    // oracle hard-coded, and a fixed threshold that stopped matching the
    // engine's binarizer once the corpus stopped being two-valued -- and both
    // times the impossible number read as a plausible finding about
    // segmentation. A ceiling below the thing it bounds is a harness bug
    // until proven otherwise.
    if let (Some(o), Some(e)) = (orc.overall.cer(), e2e.overall.cer()) {
        if o > e {
            println!(
                "
BROKEN  the oracle column scored BELOW end to end ({:.2}% vs {:.2}% character
        accuracy). The oracle is handed every glyph box, so it cannot be worse
        unless the two columns are not the same recogniser. Do not quote either
        column until that is found.",
                100.0 * (1.0 - o),
                100.0 * (1.0 - e)
            );
        }
    }
    Ok(())
}

fn pct(s: &Score) -> String {
    match s.cer() {
        Some(c) => format!("{:.2}%", 100.0 * (1.0 - c)),
        None => "n/a".into(),
    }
}

fn wpct(s: &Score) -> String {
    match s.wer() {
        Some(c) => format!("{:.2}%", 100.0 * (1.0 - c)),
        None => "n/a".into(),
    }
}

fn f1pct(t: &TokenScore) -> String {
    match t.f1() {
        Some(v) => format!("{:.2}%", 100.0 * v),
        None => "n/a".into(),
    }
}

const W: usize = 15;

/// One report row: a label, then one cell per column.
fn row(label: &str, cells: impl Iterator<Item = String>) {
    let mut s = format!("  {label:<22}");
    for c in cells {
        s.push_str(&format!("{c:>W$}"));
    }
    println!("{s}");
}

fn report(cols: &[Col<'_>], pages: usize, has_e2e: bool) {
    println!("\n{pages} pages\n");
    let mut head = format!("  {:<22}", "");
    for c in cols {
        head.push_str(&format!("{:>W$}", c.name));
    }
    println!("{head}");

    row("character accuracy", cols.iter().map(|c| pct(&c.t.overall)));
    row("word accuracy", cols.iter().map(|c| wpct(&c.t.overall)));
    row("characters scored", cols.iter().map(|c| c.t.overall.chars.to_string()));
    row("seconds", cols.iter().map(|c| format!("{:.1}", c.t.seconds)));

    println!("\n  layout-free, word for word (order and grouping ignored)");
    row(
        "word recall",
        cols.iter().map(|c| {
            c.t.tokens.recall().map_or("n/a".into(), |v| format!("{:.2}%", 100.0 * v))
        }),
    );
    row(
        "word precision",
        cols.iter().map(|c| {
            c.t.tokens.precision().map_or("n/a".into(), |v| format!("{:.2}%", 100.0 * v))
        }),
    );
    row("word F1", cols.iter().map(|c| f1pct(&c.t.tokens)));

    for (title, keys, get, gett) in [
        (
            "by render size",
            cols[0].t.by_size.keys().cloned().collect::<Vec<_>>(),
            &Totals::by_size as &dyn Fn(&Totals) -> &BTreeMap<String, Score>,
            &Totals::by_size_tokens as &dyn Fn(&Totals) -> &BTreeMap<String, TokenScore>,
        ),
        (
            "by text block",
            cols[0].t.by_block.keys().cloned().collect::<Vec<_>>(),
            &Totals::by_block,
            &Totals::by_block_tokens,
        ),
        (
            "by face",
            cols[0].t.by_family.keys().cloned().collect::<Vec<_>>(),
            &Totals::by_family,
            &Totals::by_family_tokens,
        ),
    ] {
        println!("\n{title} — character accuracy");
        for k in &keys {
            row(k, cols.iter().map(|c| get(c.t).get(k).map_or("n/a".into(), pct)));
        }
        println!("{title} — word F1");
        for k in &keys {
            row(k, cols.iter().map(|c| gett(c.t).get(k).map_or("n/a".into(), f1pct)));
        }
    }

    if has_e2e {
        println!(
            "\nThe OCRcer e2e and ocrs columns are like for like: both were given\n\
             the page and nothing else. The oracle column, where present, was\n\
             handed every glyph box and every space from the page's own ground\n\
             truth — it is a ceiling, and the gap to it is what layout and\n\
             segmentation cost. See this binary's module doc."
        );
    } else {
        println!(
            "\nThe OCRcer column is a ceiling, not a result: it was given every\n\
             glyph box and every space from the page's own ground truth. The ocrs\n\
             column is end to end from the page. Pass --engine for a like-for-like\n\
             row. See this binary's module doc."
        );
    }
}

impl Totals {
    fn by_size(&self) -> &BTreeMap<String, Score> {
        &self.by_size
    }
    fn by_size_tokens(&self) -> &BTreeMap<String, TokenScore> {
        &self.by_size_tokens
    }
    fn by_block(&self) -> &BTreeMap<String, Score> {
        &self.by_block
    }
    fn by_block_tokens(&self) -> &BTreeMap<String, TokenScore> {
        &self.by_block_tokens
    }
    fn by_family(&self) -> &BTreeMap<String, Score> {
        &self.by_family
    }
    fn by_family_tokens(&self) -> &BTreeMap<String, TokenScore> {
        &self.by_family_tokens
    }
}
