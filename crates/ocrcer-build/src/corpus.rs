//! The authored text the comparison corpus is set from.
//!
//! Original text, written for this project. Nothing here is sampled,
//! scraped, or adapted from an existing document or corpus — `CLAUDE.md`
//! rule 2 applies to evaluation material for the same reason it applies to
//! the model: a benchmark whose provenance cannot be stated is a benchmark
//! that cannot be published.
//!
//! # What it is chosen to contain
//!
//! `FEASIBILITY.md` section 6 names the domain: printed documents and CAD
//! drawing text. So the lines here are the things those actually say —
//! amounts with thousands separators and two decimals, dates in several
//! formats, account and invoice codes, tax lines, thread and tolerance
//! callouts, material specs, drawing numbers and revisions. Text where the
//! *characters* carry the meaning and a plausible-looking substitution is
//! expensive: `1,204.50` misread as `1,284.50` is a wrong number that looks
//! like a right one.
//!
//! Deliberately included and deliberately hard:
//!
//! - **Identifier-shaped strings** (`M8x1.25`, `4100-02`, `Ø12.7`). These
//!   are what `CLAUDE.md` rule 6 exists to protect. An engine with a
//!   language model behind it may "correct" them into words.
//! - **Digit and letter twins** (`0`/`O`, `1`/`l`/`I`, `5`/`S`, `8`/`B`) in
//!   contexts where both readings are plausible.
//! - **The three dash lengths** and the punctuation an accounting document
//!   actually uses, including bracketed negatives.
//! - **Dot-leader lines**, where punctuation outnumbers text. These are
//!   ordinary in this domain and they are the case a per-line metric taken
//!   as a count over components gets wrong, so their absence would leave the
//!   gate blind to a whole-line failure rather than a per-glyph one.
//!
//! Deliberately excluded: handwriting-like text, scene text, and non-Latin
//! scripts, per `FEASIBILITY.md` section 6 condition 1.
//!
//! # A bias this corpus has, stated rather than hidden
//!
//! Every character here is in `charset.tsv`. That is necessary — scoring an
//! engine on characters it has no class for measures a scope decision, not a
//! recogniser — but it is not neutral: it gives OCRcer a charset chosen to
//! fit the test, and any other engine a charset that was not. A comparison
//! using these pages has to say so, and the right reading of a result on
//! them is "on text OCRcer is designed for", not "in general".

/// One named block of lines. The name goes into the fixture filename, so a
/// disagreement can be reported as "invoice line 3" rather than "page 7".
pub struct Block {
    pub name: &'static str,
    pub lines: &'static [&'static str],
}

/// Everything that stays inside ASCII. Usable with any face in the
/// inventory, so the whole corpus renders in every family and no page is
/// dropped for missing glyphs.
pub const ASCII_BLOCKS: &[Block] = &[
    Block {
        name: "invoice",
        lines: &[
            "INVOICE 2026-0417",
            "Date: 14 September 2026",
            "Bill to: Northgate Fabrication Ltd.",
            "Terms: Net 30    Due: 14 October 2026",
            "",
            "Qty  Description                 Unit      Amount",
            "  4  Bracket, 6mm plate        112.50      450.00",
            " 12  Dowel pin 8 x 40 mm         3.75       45.00",
            "  1  Setup and programming     280.00      280.00",
            "                          Subtotal        775.00",
            "                          HST 13%         100.75",
            "                          Total          875.75",
        ],
    },
    Block {
        name: "statement",
        lines: &[
            "ACCOUNT STATEMENT",
            "Account 4100-02   Period 01/09/2026 to 30/09/2026",
            "Opening balance              12,480.15",
            "Payments received           (3,250.00)",
            "Charges this period           1,204.50",
            "Adjustments                      -0.05",
            "Closing balance              10,434.60",
            "Amounts shown in CAD. E&OE.",
        ],
    },
    Block {
        name: "drawing",
        lines: &[
            "PART NO. 71-4820-B   REV C",
            "MATERIAL: A36 HRS 6mm",
            "FINISH: HOT DIP GALV",
            "TOLERANCE UNLESS NOTED: +/-0.5",
            "4X M8x1.25 THRU",
            "2X 12.7 DIA C'BORE 20 DEEP",
            "BEND R3.0 TYP",
            "SCALE 1:2   SHEET 1 OF 3",
            "DO NOT SCALE DRAWING",
        ],
    },
    Block {
        name: "twins",
        lines: &[
            "0O 1lI 5S 8B 2Z 6G rn/m",
            "Order 10O1 shipped 2026-01-08",
            "Serial B8085 vs BB085 vs 88085",
            "Lot Il1 . Lot 1I1 . Lot l11",
            "Rate 0.50 vs O.5O vs 0.5O",
        ],
    },
    Block {
        name: "leaders",
        lines: &[
            // A dot-leader line is the single most common shape in this
            // domain -- contents pages, tax forms, statements, menus, indexes
            // -- and it is the shape that breaks a line metric taken as a
            // count over components: the periods outnumber the letters, so a
            // count-based x-height elects the height of a period and every
            // letter on the line is then measured against it. The block is
            // here because a regression gate that omits it cannot see that
            // failure at all.
            "CONTENTS",
            "Summary of operations ............ 2",
            "Notes to the accounts ........... 14",
            "Auditor's report ................ 21",
            "",
            "Total income .............. 1,234.56",
            "Less: deductions ............ 987.65",
            "Net amount payable .......... 246.91",
            "........................................",
            "Enter this amount on line 23600.",
        ],
    },
    Block {
        name: "prose",
        lines: &[
            "The engine reports a confidence for every word it returns,",
            "and a word it is unsure of is marked rather than guessed at.",
            "A reviewer can then look only where looking is warranted --",
            "which is the whole point of reporting a number at all.",
        ],
    },
];

/// Lines that need characters outside ASCII. Rendered only by faces that
/// have the glyphs; a face missing any of them skips the block rather than
/// being scored against text it never drew.
pub const EXTENDED_BLOCKS: &[Block] = &[
    Block {
        name: "currency",
        lines: &[
            "Invoice total: €1.284,50",
            "Paid in £ 980.00 on 2026-03-11",
            "Exchange ¥152.40 per unit",
            "Deposit $2,500.00 — balance due 30 days",
            "Reference no. 44 § 2(b)",
        ],
    },
    Block {
        name: "technical",
        lines: &[
            "Ø12.7 ±0.05 THRU",
            "SURFACE AREA 0.42 SQ M",
            "TEMP RANGE -40°C to +85°C",
            "ANGLE 45° ±0°30'",
            "CTE 1.2 × 10-6 per °C",
        ],
    },
];
