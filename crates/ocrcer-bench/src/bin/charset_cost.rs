//! `charset-cost`: what does carrying classes the text cannot contain cost
//! the bare matcher?
//!
//! ```text
//! charset-cost <pages-dir> <bank-sizes> [--limit N]
//! ```
//!
//! # The question, and why it needed measuring rather than asserting
//!
//! The charset is deliberately wide — accented Latin, typographic
//! punctuation, engineering symbols — because a prototype is a script run
//! here rather than a training run, so coverage is cheap to add. On ASCII
//! business and drawing text, the bare nearest-neighbour matcher's error
//! list was then observed to be dominated by substitutions into classes the
//! text *could not contain*: `.` read as `•`, `:` as `;`, `i` as `í`, `-` as
//! `–`.
//!
//! That observation is not a measurement. "A wide charset costs accuracy" is
//! folklore until there is a number beside it, and `CLAUDE.md` rule 1 does
//! not accept a plausible-looking number. This tool produces the number.
//!
//! # What it actually does, and the one thing it does not
//!
//! Each page is read twice from the same bank and the same pixels: once with
//! every class allowed, once with the matcher restricted to classes whose
//! character is ASCII. Both reads are **ungated** — no hole-count pruning on
//! either side — so the only difference between them is which classes may
//! win.
//!
//! It does **not** rebuild the bank from a narrower charset. A genuinely
//! ASCII-only bank would also standardise against its own mean and standard
//! deviation, and changing that at the same time would confound two effects
//! into one number. So this measures *the cost of the extra classes*, which
//! is the question, and not *the full difference against an ASCII-only
//! build*, which is a different and less useful one. Report it as the
//! former.
//!
//! The restriction is an oracle in its own right — real input does not come
//! with a promise that it is ASCII — so the restricted column is not an
//! achievable accuracy. It is the headroom a language model has to win back.
//!
//! Like every figure from this corpus: the read is oracle-segmented, so both
//! columns are ceilings rather than results.

use std::collections::BTreeMap;
use std::process::ExitCode;

use ocrcer_bench::cer::{score, Score};
use ocrcer_bench::pages::{list_pages, load_page, load_truth_beside, read_with_bank, Restricted};
use ocrcer_build::{bank, tables};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let r = match argv.as_slice() {
        [pages, sizes] => run(pages, sizes, usize::MAX),
        [pages, sizes, "--limit", n] => n
            .parse()
            .map_err(|_| format!("bad limit {n:?}"))
            .and_then(|n| run(pages, sizes, n)),
        _ => Err("usage: charset-cost <pages-dir> <bank-sizes> [--limit N]".into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("charset-cost: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(pages_dir: &str, bank_sizes: &str, limit: usize) -> Result<(), String> {
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
    let ascii = Restricted::new(&index_to_char, |c| c.is_ascii());
    let all = Restricted::new(&index_to_char, |_| true);
    println!(
        "bank: {} prototypes from {} faces at {:?} px/em\n\
         classes: {} in charset, {} of them ASCII",
        b.prototypes.len(),
        faces.len(),
        px_list,
        all.len(),
        ascii.len()
    );

    let pgms = list_pages(pages_dir)?;
    let total = pgms.len().min(limit);

    let mut full = Score::default();
    let mut restricted = Score::default();
    let mut by_size: BTreeMap<String, (Score, Score)> = BTreeMap::new();
    let mut lost: BTreeMap<(char, char), usize> = BTreeMap::new();
    let mut fixed: BTreeMap<(char, char), usize> = BTreeMap::new();
    let mut still_wrong: BTreeMap<(char, char), usize> = BTreeMap::new();
    let mut skipped = 0usize;

    for (i, pgm) in pgms.iter().take(limit).enumerate() {
        let stem = pgm.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let truth = load_truth_beside(pgm)?;
        let reference = truth.lines.join("\n");
        // A page whose text contains a non-ASCII character is out of scope by
        // construction: restricting the matcher's answers to ASCII makes that
        // character unreachable, so the restricted column would be charged for
        // an error the restriction itself caused. The question is what the
        // extra classes cost on text that *cannot* contain them.
        if !reference.is_ascii() {
            skipped += 1;
            continue;
        }
        let (w, h, grey) = load_page(pgm)?;
        // One binarization for both columns: this ablation changes the
        // allowed class set and nothing else.
        let mask = ocrcer_bench::pages::binarize_page(
            &grey,
            w,
            h,
            &ocrcer_core::params::Params::DEFAULT,
        );

        let a = read_with_bank(&b, &index_to_char, &truth, &mask, w, bank::Gate::None, Some(&all));
        let r = read_with_bank(&b, &index_to_char, &truth, &mask, w, bank::Gate::None, Some(&ascii));

        let sa = score(&reference, &a.text);
        let sr = score(&reference, &r.text);
        full.add(&sa);
        restricted.add(&sr);
        let key = format!("{:>5.0}", truth.px_per_em);
        let e = by_size.entry(key).or_default();
        e.0.add(&sa);
        e.1.add(&sr);

        // Compared position by position, which only the oracle makes sound:
        // both reads have exactly the reference's length and its layout,
        // because segmentation was handed to them. A substitution the full
        // bank made and the restricted one did not is one the extra classes
        // caused; counting by value instead would miscount a pair that
        // occurs twice on a page.
        for ((want, ga), gr) in reference.chars().zip(a.text.chars()).zip(r.text.chars()) {
            if ga != want {
                *lost.entry((want, ga)).or_default() += 1;
                if gr == want {
                    *fixed.entry((want, ga)).or_default() += 1;
                }
            }
            if gr != want {
                *still_wrong.entry((want, gr)).or_default() += 1;
            }
        }

        eprint!("\r{}/{total} {stem}                    ", i + 1);
    }
    eprintln!();

    let acc = |s: &Score| match s.cer() {
        Some(c) => format!("{:.2}%", 100.0 * (1.0 - c)),
        None => "n/a".into(),
    };
    println!(
        "\n{} pages scored, {} characters\n\
         ({skipped} pages skipped: their text is not ASCII, so an\n\
         ASCII-restricted matcher could not reach it and the restriction\n\
         would be scored for an error it caused itself)\n",
        total - skipped,
        full.chars
    );
    println!("                          full charset   ASCII-restricted   delta");
    println!(
        "  character accuracy      {:>10}         {:>10}   {:>+6.2}",
        acc(&full),
        acc(&restricted),
        100.0 * (full.cer().unwrap_or(0.0) - restricted.cer().unwrap_or(0.0))
    );
    println!(
        "  word accuracy           {:>10}         {:>10}   {:>+6.2}",
        match full.wer() {
            Some(c) => format!("{:.2}%", 100.0 * (1.0 - c)),
            None => "n/a".into(),
        },
        match restricted.wer() {
            Some(c) => format!("{:.2}%", 100.0 * (1.0 - c)),
            None => "n/a".into(),
        },
        100.0 * (full.wer().unwrap_or(0.0) - restricted.wer().unwrap_or(0.0))
    );

    println!("\nby render size            full charset   ASCII-restricted");
    for (k, (a, r)) in &by_size {
        println!("  {k} px/em               {:>10}         {:>10}", acc(a), acc(r));
    }

    let mut top: Vec<_> = fixed.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    println!("\nsubstitutions the extra classes caused (fixed by restricting), top 25");
    for ((want, got), n) in top.iter().take(25) {
        println!("  {n:>6}  {want:?} -> {got:?}");
    }

    let mut rest: Vec<_> = still_wrong.into_iter().collect();
    rest.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    println!("\nsubstitutions that survive the restriction — the matcher's own, top 25");
    for ((want, got), n) in rest.iter().take(25) {
        println!("  {n:>6}  {want:?} -> {got:?}");
    }

    let total_errors: usize = lost.values().sum();
    println!(
        "\n{total_errors} substitutions with the full charset. Both columns are\n\
         oracle-segmented ceilings, and the restricted column is itself an\n\
         oracle — real input carries no promise that it is ASCII. Read it as\n\
         the headroom a language model has to win back, not as an accuracy."
    );
    Ok(())
}
