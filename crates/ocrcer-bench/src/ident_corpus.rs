//! Text for the identifier-preservation corpus (`bin/ident.rs`).
//!
//! Separate from `ocrcer_build::corpus` deliberately: that module is the
//! shipped comparison corpus (`bench/pages-cov`) and belongs to
//! `ocrcer-build`. This one exists only to probe one failure mode —
//! `CLAUDE.md` rule 6, an identifier-shaped token silently rewritten into a
//! dictionary word or a different identifier — so it is denser in
//! identifier-shaped tokens than a representative document would be, and it
//! says so rather than presenting itself as a general-purpose corpus.
//!
//! Every character used here is in `model/charset.tsv` (checked by hand
//! against the same file `ocrcer_build::corpus` is held to). No text is
//! copied from any source; it is authored for this purpose, same as
//! `ocrcer_build::corpus`'s own disclosure requires.
//!
//! The first line of `THREADS` is deliberately identical to
//! `ocrcer_build::corpus::ASCII_BLOCKS`'s `"drawing"` block, line 5: it is
//! the known failure named in `docs/measurements/2026-09-25_score_12b.md`
//! section 3 (Noto Sans Regular, `drawing`, 18px: "4X M8x1.25 THRU" read
//! back as "4X IV18x1 .25 THRU" under the fitted config) and this corpus
//! exists to catch it deliberately rather than by accident.

/// One named block of lines, same shape as `ocrcer_build::corpus::Block`.
pub struct Block {
    pub name: &'static str,
    pub lines: &'static [&'static str],
}

/// ASCII-only: safe with every face in the inventory.
pub const ASCII_BLOCKS: &[Block] = &[
    Block {
        name: "threads",
        lines: &[
            "4X M8x1.25 THRU",
            "2X M12x1.75 TAPPED 20 DEEP",
            "8X 1/4-20 UNC THRU ALL",
            "FASTENER TORQUE 25 Nm TYP ALL",
        ],
    },
    Block {
        name: "parts",
        lines: &[
            "PART NO. 71-4820-B REV C",
            "MATERIAL A36 HRS 6mm QTY 12",
            "STOCK CODE 4100-02 BIN A14",
            "ALLOY 6061-T6 FINISH ANODIZE",
        ],
    },
    Block {
        name: "revisions",
        lines: &[
            "REV C ECR 2026-0417 APPROVED",
            "REVISION HISTORY R1.0 R2.1 R3.0",
            "DRAWING NO. 71-4820-B SHEET 2 OF 3",
        ],
    },
    Block {
        name: "dims",
        lines: &[
            "BEND R3.0 TYP RADIUS R0.5 MIN",
            "SCALE 1:2 SHEET 1 OF 3",
            "HOLE PATTERN 4X 90 DEG APART",
        ],
    },
    Block {
        name: "accounts",
        lines: &[
            "INVOICE INV-2026-0042 DUE 30 DAYS",
            "ACCOUNT 4100-02 PO NO. PO-88213",
            "REFERENCE RMA-0091-A DATE 2026-09-25",
            "CUSTOMER CODE NG-4471 TERMS NET 30",
        ],
    },
    Block {
        name: "dates",
        lines: &[
            "ORDER DATE 2026-09-25 SHIP BY 2026-10-02",
            "LOT NO. L2026-0913-A EXP 2028-09",
            "BATCH 20260925-07 QTY 500 UNITS",
        ],
    },
];

/// Needs characters outside ASCII (`Ø`, `±`, `°`), same split as
/// `ocrcer_build::corpus::EXTENDED_BLOCKS` and for the same reason: a face
/// missing one of these glyphs skips the block rather than being scored
/// against text it never drew.
pub const EXTENDED_BLOCKS: &[Block] = &[Block {
    name: "dims-ext",
    lines: &[
        "Ø12.5 ±0.1 THRU ALL",
        "25.4±0.1 TYP UNLESS NOTED",
        "CHAMFER 2X45 DEG ANGLE 45° ±0°30'",
    ],
}];
