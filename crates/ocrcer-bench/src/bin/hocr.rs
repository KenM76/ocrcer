//! `hocr`: write what the engine read as hOCR, so somebody else's harness can
//! score it.
//!
//! ```text
//! hocr <model.ocrw> <pages-dir> <out-dir> [--only SUBSTRING] [--set name=value]
//! ```
//!
//! # Why this exists
//!
//! Every accuracy figure in this crate is computed by this crate, against
//! truth this project converted, by a metric this project chose. That is
//! enough to compare OCRcer to itself across a change, and it is *not* enough
//! to compare OCRcer to a third-party engine: a metric of one's own devising
//! flatters whoever devised it, and two engines can swap places between two
//! defensible definitions of "word accuracy".
//!
//! Published third-party benchmarks score `.hocr` files. Emitting hOCR lets
//! OCRcer be scored by **their** harness, on **their** metric, against
//! **their** hand-checked truth, next to numbers they published for engines
//! nobody here tuned. That is a materially stronger claim than any number this
//! crate computes, and it is the only way to make one.
//!
//! The output coordinate space is the page as handed to the engine. A corpus
//! converted with a `--scale` other than 1.0 has moved out of the truth's
//! coordinate space and cannot be scored by a box-matching harness; convert at
//! scale 1.0 for this purpose.
//!
//! # What is deliberately not in the output
//!
//! Only what the engine actually produces: page box, line boxes, word boxes,
//! word text, a word confidence that is the calibrated match margin
//! (`CLAUDE.md` rule 5), and the two line metrics the engine genuinely
//! measures: the baseline and the x-height. No `x_size`, `x_ascenders` or
//! `x_descenders` — those hOCR fields have meanings this writer has not
//! checked against, and a plausible number written into an interchange file
//! is the failure mode rule 1 exists to prevent. If a consumer turns out to
//! need one it gets added once its meaning is verified, not guessed.
//!
//! No font family, no language, no paragraph or column structure: OCRcer does
//! not identify any of those, and claiming them in the output would be an
//! assertion rather than a reading.

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use ocrcer_bench::pages::{list_pages, load_page};
use ocrcer_core::pipeline::{Engine, Line};
use ocrcer_core::Gray;

fn usage(msg: &str) -> ExitCode {
    eprintln!("hocr: {msg}");
    eprintln!("usage: hocr <model.ocrw> <pages-dir> <out-dir> [--only SUB] [--set name=value]");
    ExitCode::FAILURE
}

/// The five XML predefined entities. A word containing `&` or `<` would
/// otherwise produce a file the consumer's parser rejects, and an OCR engine
/// reading a page of HTML source is not a hypothetical.
fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
}

fn page_hocr(stem: &str, w: u32, h: u32, lines: &[Line]) -> String {
    let mut s = String::with_capacity(4096);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\"\n");
    s.push_str("    \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">\n");
    s.push_str("<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"en\" lang=\"en\"><head>\n");
    s.push_str("\t<title></title>\n");
    s.push_str("\t<meta http-equiv=\"Content-Type\" content=\"text/html;charset=utf-8\" />\n");
    s.push_str("\t<meta name='ocr-system' content='ocrcer' />\n");
    s.push_str("\t<meta name='ocr-capabilities' content='ocr_page ocr_line ocrx_word ocrp_wconf' />\n");
    s.push_str("</head>\n<body>\n");
    let _ = writeln!(s, "\t<div class='ocr_page' id='page_1' title='image \"{stem}\"; bbox 0 0 {w} {h}; ppageno 0'>");
    let mut id = 0usize;
    for line in lines {
        let r = line.rect;
        // `baseline <slope> <offset>`: the offset is relative to the BOTTOM of
        // the line box, which is the convention consumers of this field assume.
        // The slope is zero because the page is deskewed before lines are
        // found, so a residual per-line slope is not something this engine
        // estimates — zero is a statement about the pipeline rather than a
        // placeholder. Some importers drop any line that omits this field
        // entirely, which reads a whole page as empty.
        let offset = f64::from(line.baseline) - f64::from(r.y + r.height);
        let _ = writeln!(
            s,
            "\t\t<span class='ocr_line' title=\"bbox {} {} {} {}; baseline 0 {offset:.0}; x_x_height {:.2}\">",
            r.x,
            r.y,
            r.x + r.width,
            r.y + r.height,
            line.x_height
        );
        for word in &line.words {
            let b = word.rect;
            // hOCR's confidence field is an integer percent. Rounding is the
            // field's own precision, not a loss of information this writer
            // chose to take.
            let conf = (f64::from(word.confidence) * 100.0).round().clamp(0.0, 100.0) as u32;
            let _ = write!(
                s,
                "\t\t\t<span class='ocrx_word' id='word_1_{id}' title='bbox {} {} {} {};x_wconf {conf}'>",
                b.x,
                b.y,
                b.x + b.width,
                b.y + b.height
            );
            escape(&word.text, &mut s);
            s.push_str("</span>\n");
            id += 1;
        }
        s.push_str("\t\t</span>\n");
    }
    s.push_str("\t</div>\n</body>\n</html>\n");
    s
}

fn run(model: &str, dir: &str, out: &str, only: Option<&str>, set: &[(String, f32)]) -> Result<(), String> {
    let bytes = std::fs::read(model).map_err(|e| format!("{model}: {e}"))?;
    let mut engine = Engine::from_bytes(&bytes).map_err(|e| format!("{model}: {e:?}"))?;
    for (name, v) in set {
        if !engine.set_param(name, *v) {
            return Err(format!("no such parameter: {name}"));
        }
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{out}: {e}"))?;
    let pages = list_pages(dir)?;
    let mut n = 0usize;
    for pgm in pages {
        let stem = pgm.file_stem().unwrap_or_default().to_string_lossy().to_string();
        if only.is_some_and(|s| !stem.contains(s)) {
            continue;
        }
        let (w, h, pix) = load_page(&pgm)?;
        let lines = engine
            .recognize_lines(Gray { width: w, height: h, data: &pix })
            .map_err(|e| format!("{}: {e:?}", pgm.display()))?;
        let words: usize = lines.iter().map(|l| l.words.len()).sum();
        let text = page_hocr(&format!("{stem}.png"), w, h, &lines);
        let path = Path::new(out).join(format!("{stem}.hocr"));
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("{stem:28} {w}x{h}  {:4} lines  {words:5} words", lines.len());
        n += 1;
    }
    println!("\n{n} pages written to {out}");
    println!("scored by the harness that owns the truth, not by this crate");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut positional: Vec<String> = Vec::new();
    let mut only: Option<String> = None;
    let mut set: Vec<(String, f32)> = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--only" => {
                i += 1;
                match args.get(i) {
                    Some(v) if !v.is_empty() => only = Some(v.clone()),
                    _ => return usage("--only needs a page-name substring"),
                }
            }
            "--set" => {
                i += 1;
                match args
                    .get(i)
                    .and_then(|v| v.split_once('='))
                    .and_then(|(n, v)| v.trim().parse::<f32>().ok().map(|v| (n.to_string(), v)))
                {
                    Some(nv) => set.push(nv),
                    None => return usage("--set needs <name>=<value>"),
                }
            }
            other => positional.push(other.to_string()),
        }
        i += 1;
    }
    let [model, dir, out] = match <[String; 3]>::try_from(positional) {
        Ok(v) => v,
        Err(_) => return usage("need a model, a pages directory and an output directory"),
    };
    match run(&model, &dir, &out, only.as_deref(), &set) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hocr: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocrcer_core::pipeline::{Rect, Word};

    fn word(text: &str, x: u32) -> Word {
        Word {
            text: text.to_string(),
            rect: Rect { x, y: 10, width: 20, height: 12 },
            confidence: 0.5,
            chars: Vec::new(),
        }
    }

    #[test]
    fn markup_special_characters_are_escaped() {
        let line = Line {
            words: vec![word("A&B", 0), word("<i>", 30)],
            rect: Rect { x: 0, y: 10, width: 50, height: 12 },
            baseline: 20.0,
            x_height: 8.0,
            confidence: 0.5,
            band: 0,
        };
        let s = page_hocr("p.png", 100, 200, &[line]);
        assert!(s.contains(">A&amp;B</span>"), "{s}");
        assert!(s.contains(">&lt;i&gt;</span>"), "{s}");
        // And the raw forms must not survive anywhere in the body.
        assert!(!s.contains(">A&B<"), "{s}");
    }

    #[test]
    fn boxes_are_emitted_as_x0_y0_x1_y1_not_x_y_w_h() {
        let line = Line {
            words: vec![word("hi", 5)],
            rect: Rect { x: 5, y: 10, width: 20, height: 12 },
            baseline: 20.0,
            x_height: 8.0,
            confidence: 0.5,
            band: 0,
        };
        let s = page_hocr("p.png", 100, 200, &[line]);
        assert!(s.contains("bbox 5 10 25 22;x_wconf 50"), "{s}");
        assert!(s.contains("bbox 0 0 100 200"), "{s}");
    }

    #[test]
    fn a_page_with_no_text_is_still_a_well_formed_page() {
        let s = page_hocr("p.png", 10, 20, &[]);
        assert!(s.contains("class='ocr_page'"), "{s}");
        assert!(s.trim_end().ends_with("</html>"), "{s}");
    }
}
