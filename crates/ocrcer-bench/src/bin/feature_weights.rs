//! `feature-weights`: does weighting the baseline-relative group fix the
//! same-shape-different-size confusions, or is something deeper wrong?
//!
//! ```text
//! feature-weights <pages-dir> <bank-sizes> [--stride N] [--weights a,b,c]
//!                 [--all-classes]
//! ```
//!
//! # The question
//!
//! The charset-cost ablation measured that ~58% of the bare matcher's errors
//! are pairs that are the same shape at a different size — `0`/`o`/`O`/`Q`,
//! `l`/`I`/`1`, `s`/`S`, `c`/`C`, `u`/`U`, `v`/`V`, `w`/`W`. Section 3's
//! feature table already carries four dimensions (`103..107`: aspect, ink
//! fraction, height above baseline, depth below) whose stated purpose is
//! exactly this pair, so the capability is present and not working.
//!
//! Section 4.1 step 3 names the likely reason without claiming it: the
//! matching weights "are not yet authored", so all 107 standardised
//! dimensions vote equally. For `o` against `O` the ~80 shape dimensions are
//! near-identical and carry no signal while 4 carry all of it, and 4 lose to
//! 80. If that is the cause, multiplying the group's contribution should
//! recover most of the family. If it is not, the multiplier will move the
//! count a little and then stop, and the fault is in the *inputs* to those
//! four dimensions — the line's `x_height`, which is chunk 2's geometry —
//! rather than in their share of the distance.
//!
//! **Both outcomes are useful and the tool must be able to report either.**
//! A sweep that only reported its best point would answer a question nobody
//! asked.
//!
//! # What it does
//!
//! One pass per candidate weight, over the same pages, from the same bank and
//! the same pixels, ungated on every pass so the multiplier is the only thing
//! that differs. `1.0` is the control and reproduces today's matcher exactly.
//!
//! Non-ASCII pages are skipped for the same reason `charset-cost` skips them:
//! this measures a confusion family that lives in ASCII, and mixing in
//! characters the families do not contain only dilutes the signal.
//!
//! `--all-classes` drops both the skip and the restriction, and asks the
//! separate question the ablation raised: the charset-caused substitutions it
//! named — `i` read as `ì`, `-` as `–`, `:` as `÷` — differ from
//! their targets in height above the baseline or in width, which is exactly
//! what these four dimensions measure. A weight authored for the case pairs
//! may therefore reduce the charset’s 1.39-point cost as a side effect, and
//! that deserves a number rather than an assumption.
//!
//! Reads are oracle-segmented, so every number here is a ceiling.

use std::collections::BTreeMap;
use std::process::ExitCode;

use ocrcer_bench::cer::{score, Score};
use ocrcer_bench::pages::{list_pages, load_page, load_truth_beside, read_with_bank, Restricted};
use ocrcer_build::{bank, tables};

/// `ARCHITECTURE.md` section 3.1: aspect, ink fraction, `baseline_dy/x_height`,
/// `(height - baseline_dy)/x_height`.
const BASELINE_GEOMETRY: std::ops::Range<usize> = 103..107;

/// The families the charset-cost ablation named, so the sweep reports what it
/// was aimed at rather than only an aggregate that could move for any reason.
const FAMILIES: &[(&str, &str)] = &[
    ("0oOQ", "0oOQ"),
    ("lI1|", "lI1|"),
    ("sS$5", "sS$5"),
    ("cC", "cC"),
    ("uU", "uU"),
    ("vV", "vV"),
    ("wW", "wW"),
];

fn family_of(want: char, got: char) -> Option<&'static str> {
    FAMILIES
        .iter()
        .find(|(_, set)| set.contains(want) && set.contains(got))
        .map(|(name, _)| *name)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let (pages, sizes, rest) = match argv.as_slice() {
        [p, s, rest @ ..] => (*p, *s, rest),
        _ => {
            eprintln!(
                "usage: feature-weights <pages-dir> <bank-sizes>                  [--stride N] [--weights a,b,c] [--all-classes]"
            );
            return ExitCode::FAILURE;
        }
    };
    let mut stride = 1usize;
    let mut all_classes = false;
    let mut weights: Vec<f32> = vec![1.0, 2.0, 4.0, 8.0, 16.0, 32.0];
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match *a {
            "--all-classes" => all_classes = true,
            "--stride" => match it.next().and_then(|n| n.parse().ok()) {
                Some(n) if n >= 1 => stride = n,
                _ => {
                    eprintln!("feature-weights: --stride wants a positive integer");
                    return ExitCode::FAILURE;
                }
            },
            "--weights" => match it.next().map(|w| {
                w.split(',')
                    .map(|v| v.trim().parse::<f32>())
                    .collect::<Result<Vec<_>, _>>()
            }) {
                Some(Ok(w)) if !w.is_empty() => weights = w,
                _ => {
                    eprintln!("feature-weights: --weights wants a comma-separated float list");
                    return ExitCode::FAILURE;
                }
            },
            other => {
                eprintln!("feature-weights: unexpected argument {other:?}");
                return ExitCode::FAILURE;
            }
        }
    }
    if !weights.contains(&1.0) {
        // The control is not optional: without it the sweep reports movement
        // against nothing and cannot say whether it moved at all.
        weights.insert(0, 1.0);
    }
    match run(pages, sizes, stride, &weights, all_classes) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("feature-weights: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(
    pages_dir: &str,
    bank_sizes: &str,
    stride: usize,
    weights: &[f32],
    all_classes: bool,
) -> Result<(), String> {
    let px_list: Vec<f32> = bank_sizes
        .split(',')
        .map(|p| p.trim().parse::<f32>().map_err(|_| format!("bad px/em {p:?}")))
        .collect::<Result<_, _>>()?;

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
    let b = bank::build(&classes, &faces, &renderers, &px_list);

    // Every ASCII page, taken at a stride so the subset spans every face
    // rather than the alphabetically first few. Loaded once and held: the
    // sweep reads the same pixels on every pass, and re-decoding the PGMs per
    // pass would be most of the runtime.
    let mut corpus: Vec<(String, String, f32, ocrcer_bench::pages::Truth, u32, Vec<u8>)> =
        Vec::new();
    for pgm in list_pages(pages_dir)?.iter().step_by(stride) {
        let truth = load_truth_beside(pgm)?;
        let reference = truth.lines.join("\n");
        if !all_classes && !reference.is_ascii() {
            continue;
        }
        let (w, h, grey) = load_page(pgm)?;
        // Binarized once here rather than per sweep point: the sweep varies
        // a feature weight and must vary nothing else.
        let grey = ocrcer_bench::pages::binarize_page(
            &grey,
            w,
            h,
            &ocrcer_core::params::Params::DEFAULT,
        );
        let stem = pgm.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let px = truth.px_per_em;
        corpus.push((stem, reference, px, truth, w, grey));
    }
    if corpus.is_empty() {
        return Err("no pages in the corpus at that stride".into());
    }
    println!(
        "bank: {} prototypes from {} faces at {:?} px/em\n\
         corpus: {} {} pages (stride {stride})\n",
        b.prototypes.len(),
        faces.len(),
        px_list,
        corpus.len(),
        if all_classes { "corpus" } else { "ASCII" }
    );

    println!("weight on dims 103..107 (baseline-relative geometry)");
    println!(
        "  {:>7}  {:>9}  {:>9}  {:>10}  {:>12}",
        "weight", "char acc", "word acc", "size-pair", "other errors"
    );

    let mut best: Option<(f32, f64)> = None;
    let mut family_at: BTreeMap<String, BTreeMap<&'static str, usize>> = BTreeMap::new();
    // Every substitution, per weight, so the run can say what the winning
    // weight *failed* to fix. A sweep that reports only the aggregate cannot
    // distinguish "the intervention worked" from "the intervention worked on
    // half the family and the other half is a different problem".
    let mut pairs_at: BTreeMap<String, BTreeMap<(char, char), usize>> = BTreeMap::new();

    for &w in weights {
        let probe = Restricted::new(&index_to_char, |c| all_classes || c.is_ascii())
            .with_group_weight(BASELINE_GEOMETRY, w);
        let mut total = Score::default();
        let mut size_pair = 0usize;
        let mut other = 0usize;
        let mut families: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut pairs: BTreeMap<(char, char), usize> = BTreeMap::new();

        for (_stem, reference, _px, truth, width, mask) in &corpus {
            let r = read_with_bank(
                &b,
                &index_to_char,
                truth,
                mask,
                *width,
                bank::Gate::None,
                Some(&probe),
            );
            total.add(&score(reference, &r.text));
            for (want, got) in reference.chars().zip(r.text.chars()) {
                if want == got {
                    continue;
                }
                *pairs.entry((want, got)).or_default() += 1;
                match family_of(want, got) {
                    Some(f) => {
                        size_pair += 1;
                        *families.entry(f).or_default() += 1;
                    }
                    None => other += 1,
                }
            }
        }

        let acc = 1.0 - total.cer().unwrap_or(0.0);
        println!(
            "  {w:>7.1}  {:>8.2}%  {:>8.2}%  {size_pair:>10}  {other:>12}",
            100.0 * acc,
            100.0 * (1.0 - total.wer().unwrap_or(0.0)),
        );
        if best.is_none_or(|(_, ba)| acc > ba) {
            best = Some((w, acc));
        }
        family_at.insert(format!("{w:.1}"), families);
        pairs_at.insert(format!("{w:.1}"), pairs);
    }

    println!("\nby confusion family, count of substitutions");
    let cols: Vec<&String> = family_at.keys().collect();
    print!("  {:<10}", "family");
    for c in &cols {
        print!("{:>10}", format!("w={c}"));
    }
    println!();
    for (name, _) in FAMILIES {
        print!("  {name:<10}");
        for c in &cols {
            print!("{:>10}", family_at[*c].get(name).copied().unwrap_or(0));
        }
        println!();
    }

    // Every weight's column for the pairs that matter most, because the
    // aggregate hides the shape of the effect. A pair that falls monotonically
    // is one this intervention is the right tool for; a pair that *rises* is
    // one the intervention is buying its aggregate win from, and the aggregate
    // alone would never say so.
    {
        let cols: Vec<String> = weights.iter().map(|w| format!("{w:.1}")).collect();
        let mut union: BTreeMap<(char, char), usize> = BTreeMap::new();
        for c in &cols {
            for (k, v) in &pairs_at[c] {
                let e = union.entry(*k).or_default();
                *e = (*e).max(*v);
            }
        }
        let mut rows: Vec<_> = union.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        println!("
substitution counts per weight, top 24 pairs by worst-case count");
        print!("  {:<14}", "pair");
        for c in &cols {
            print!("{:>9}", format!("w={c}"));
        }
        println!();
        for ((want, got), _) in rows.iter().take(24) {
            print!("  {:<14}", format!("{want:?} -> {got:?}"));
            for c in &cols {
                print!("{:>9}", pairs_at[c].get(&(*want, *got)).copied().unwrap_or(0));
            }
            println!();
        }
    }

    match best {
        // `w <= 1.0` rather than `== 1.0`: the control is the only weight at or
        // below 1.0 the default sweep offers, and a caller passing a fractional
        // weight that won would be saying the same thing — the group's share of
        // the distance is not what is wrong.
        Some((w, acc)) if w <= 1.0 => println!(
            "\nNo weight beat the control ({:.2}%). The four dimensions are not\n\
             merely outvoted, so the next question is whether their inputs are\n\
             sound — the line's x-height, which is chunk 2's geometry — rather\n\
             than their share of the distance.",
            100.0 * acc
        ),
        Some((w, acc)) => println!(
            "\nBest at weight {w:.1}: {:.2}% character accuracy. A weight is a\n\
             number that needs a sentence (CLAUDE.md rule 1), so this is a\n\
             measured candidate and not yet an authored parameter — the swept\n\
             optimum of a one-parameter family on this corpus, which is not the\n\
             same thing as the right value on unseen input.",
            100.0 * acc
        ),
        None => {}
    }
    println!(
        "\nOracle-segmented and ASCII-restricted on every pass, so every figure\n\
         is a ceiling. Only the differences between rows are the measurement."
    );
    Ok(())
}
