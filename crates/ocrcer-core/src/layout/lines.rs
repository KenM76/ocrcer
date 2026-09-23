//! Grouping page components into text lines, and measuring each line's
//! baseline and x-height.
//!
//! # Contract
//!
//! [`group`] takes the page's connected components and returns [`TextLine`]s
//! in top-to-bottom reading order, each holding its members left to right
//! plus the baseline and x-height the feature extractor needs
//! (`ARCHITECTURE.md` section 6). Deterministic: every tie has a stated rule
//! and no step depends on hash or float ordering.
//!
//! **Why tallest-first rather than top-down.** Section 6 asks for grouping by
//! vertical overlap against a *running* median component height, not a fixed
//! threshold, so that mixed type sizes on one page survive. Seeding in
//! descending height order is what makes that work: the components that
//! define a line's band arrive before the marks that decorate it, so an
//! `i`-dot or a comma finds a band to join instead of seeding a line of its
//! own, and a smaller type size elsewhere on the page seeds its own band with
//! its own median rather than being measured against the larger one.

use crate::image::components::Component;

/// x-height as a fraction of cap height: the median over the 19 shippable
/// faces in the prototype bank, each measured as the ink height of `x` (or
/// the face's declared x-height) over the ink height of `H`, both at 256
/// px/em.
///
/// Measured, not a typographic rule of thumb: run `ocrcer-build metrics` to
/// reproduce it. The spread across those faces is 0.6335 (the authored
/// technical face, which is deliberately large-on-the-body) to 0.8051
/// (Liberation Mono).
///
/// It exists because an all-caps CAD annotation or a row of dimension
/// figures has no x-height band on the page to measure, while the four
/// baseline-relative features are normalised by x-height. Converting the cap
/// band that *is* visible is the only way to put such a line on the same
/// scale as the prototypes. Every line that uses it is marked
/// [`XHeightSource::FromCapHeight`], so a caller can tell a measurement from
/// a conversion.
pub const X_HEIGHT_PER_CAP: f32 = 0.7431;

/// Line-grouping parameters.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// Components with fewer ink pixels than this are dropped as speckle.
    /// One isolated pixel cannot carry a shape; the feature extractor
    /// normalises to a 32x32 grid and would be reading noise.
    pub min_area: u32,
    /// Vertical overlap a component must share with a line, as a fraction of
    /// the shorter of the component's height and the line's running median
    /// height, to join it.
    ///
    /// Half: an x-height letter beside a capital on the same baseline
    /// overlaps by its whole height, while an ascender reaching up into the
    /// line above overlaps that line by a small part of itself.
    pub overlap_fraction: f32,
    /// A component wider than this fraction of the page is page furniture —
    /// a table rule, a title-block border — not a glyph. No character is a
    /// fifth of a page wide.
    pub furniture_fraction: f32,
    /// A component whose longer ink extent exceeds this multiple of its
    /// shorter one is a rule, not a glyph: 0 disables the gate.
    ///
    /// `furniture_fraction` only catches furniture measured against the
    /// whole page, so a rule drawn inside one cell of a form is far too
    /// small to trip it and survives to be matched against the charset,
    /// where it lands on a dash. This gate is scale-free instead. The bound
    /// it must clear is measured rather than assumed: `ocrcer-build aspect`
    /// renders every class on every shippable face and reports the extreme.
    pub rule_aspect: f32,
    /// A band whose tallest member is no more than this fraction of another
    /// band's running median is a *mark* — an `i`-dot, an accent — rather
    /// than a line of its own, and is folded into the band it decorates.
    ///
    /// The limitation this rate admits: text set smaller than a third of the
    /// text beside it would be read as marks on it. A 4pt footnote against
    /// 12pt body text is outside this engine's domain; an `i`-dot against its
    /// own stem, at a fifth of the height, is the case that has to work.
    pub mark_height_fraction: f32,
    /// How far a mark may sit from the band it decorates, as a fraction of
    /// that band's running median. An `i`-dot clears the x-height band by
    /// roughly the gap between x-height and ascender height.
    pub mark_reach_fraction: f32,
    /// How far below the baseline a member must reach, as a fraction of the
    /// line's single top band, to prove the line carries lowercase. A
    /// descender belongs to `g p q y j`, whose bodies sit at x-height, so its
    /// presence settles what an ambiguous single band is.
    pub descender_fraction: f32,
    /// How far a component may hang below a band's current bottom edge, as a
    /// fraction of that band's running median, and still be that band's
    /// descender rather than a line of its own.
    ///
    /// The first descender on a line is the case this exists for. A band's
    /// box stops at its lowest ink, so before any descender has joined, the
    /// box bottom *is* the baseline — and a comma, whose ink is mostly below
    /// the baseline, then shares too little of itself with the box to pass
    /// [`Params::overlap_fraction`] and seeds a line of its own. It is too
    /// tall to be folded back as a mark, so the page comes back with a line
    /// of commas in it and every line after it displaced.
    ///
    /// The test that replaces the ratio test for such a component is: its top
    /// edge lies inside the band, and it hangs below the band by no more than
    /// this. A glyph on the *next* line whose ascender pokes one pixel into
    /// this band also has its top edge inside it, but hangs below by close to
    /// a whole median, so the reach is what separates the two.
    pub descender_reach_fraction: f32,
    /// x-height as a fraction of cap height, used when a line shows only one
    /// ink band and the band has to be read as one or the other.
    ///
    /// Measured rather than authored: the median over the shippable faces, by
    /// `ocrcer-build metrics`. [`X_HEIGHT_PER_CAP`] is the same number as a
    /// compile-time constant for callers that have no parameter block.
    pub x_height_per_cap: f32,
    /// The plausibility floor on [`XHeightSource::Observed`]: `main` must be
    /// at least this fraction of `cap`, or the branch that would call it
    /// `Observed` falls through as if `upper` were empty.
    ///
    /// A band formed by two lines merging (`ARCHITECTURE.md` section 11,
    /// 2026-09-23, "Merged lines: split on two baselines") produces a `tops`
    /// histogram piled at zero -- every member of the wrongly-absorbed second
    /// line clamps to `top == 0` against the first line's baseline -- so
    /// `main` comes back as the clamp floor of `1` rather than a real
    /// measurement, while `upper` still holds the first line's genuine
    /// x-height and cap-height values and so is non-empty. `!upper.is_empty()`
    /// alone cannot tell that pathology from an ordinary condensed face, so
    /// this checks the one thing that does: whether `main` is a plausible
    /// x-height for the `cap` beside it.
    ///
    /// Measured, not authored: half of the smallest per-face x-height/cap-
    /// height ratio across the shippable font set, `ocrcer-build metrics`
    /// (2026-09-23, 32 faces): the extreme is 0.6335 (`OCRcer Technical`,
    /// deliberately large-on-the-body), so the floor is 0.3168, with a full
    /// factor of two of headroom below the most condensed real face this
    /// engine ships. The traced defect measured about 0.08 -- clear of even
    /// a floor this conservative. See
    /// `docs/measurements/2026-09-23_line_merge_phase2.txt`.
    pub x_height_floor_per_cap: f32,
    /// A line whose x-height came out below this fraction of the page's
    /// typical x-height did not measure one; it inherits instead.
    ///
    /// The case is a line with no x-height evidence *of its own*: a row of
    /// leader dots, a row of underscores, a rule, a line of full stops in a
    /// table of contents. Its members are all the same small height, so the
    /// modal band is the punctuation and the converted x-height comes out at
    /// a fraction of the real one. Nothing about the line is ambiguous to a
    /// reader -- the page around it says plainly how big its text is -- and
    /// nothing in a per-line measurement can see that.
    ///
    /// Half is deliberately far from the cases it must not catch. Real
    /// same-page size variation is footnotes against body text, which is
    /// about 0.8, and a table's fine print, about 0.7. A leader line
    /// measures 0.2 to 0.3. See section 3 of `ARCHITECTURE.md`.
    pub inherit_x_height_below: f32,

    /// A horizontal gap between consecutive members of a band wider than
    /// this many median component heights is a column or box boundary, and
    /// the band is cut there.
    ///
    /// Banding is purely horizontal: anything sharing a row joins the same
    /// line, which on a ruled form means a 6 pt field label and a 14 pt
    /// display heading three boxes away end up in one line with one set of
    /// metrics. The line is then measured for neither of them, and the word
    /// splitter over-splits the large text and under-splits the small text
    /// at the same time.
    ///
    /// Zero disables the cut. See [`Params::column_lone_guard`] for whether a
    /// candidate between two lone glyphs survives: see
    /// [`split_at_column_gaps`].
    pub column_gap_heights: f32,

    /// Whether a candidate boundary between two single-component fragments --
    /// a leader dot beside another leader dot -- is dropped rather than cut.
    ///
    /// `true` is the guard reasoned from the leader fixture: a run of dots is
    /// not a column boundary, so a candidate both of whose sides are lone
    /// glyphs is merged back in. `false` is the plain greedy cut, which keeps
    /// every candidate [`split_at_column_gaps`] finds and must be byte-for-
    /// byte the same as cutting at every one of them.
    ///
    /// Both readings of the guard have now cost real pages on `finfilings`
    /// with nothing narrower than a whole corpus able to say why
    /// (`ARCHITECTURE.md` section 11, 2026-09-23, "The narrowed lone-glyph
    /// rule also fails the real-filings gate"). This toggle exists so both
    /// behaviours run from one binary while that is investigated; it ships
    /// `false`, the configuration both corpora measured best.
    pub column_lone_guard: bool,

    /// Whether the two-baseline split post-pass runs: `false` leaves band
    /// growth's output untouched, `true` runs [`split_baselines`] on every
    /// band before [`measure`].
    ///
    /// A real text line has exactly one baseline. A band two lines were
    /// merged into (`ARCHITECTURE.md` section 11, 2026-09-23, "Merged lines:
    /// split on two baselines") has two, well separated and each with its
    /// own support -- the same asymmetry [`Params::x_height_floor_per_cap`]
    /// exploits from the other side. Off by default until measured against
    /// both corpora; a guess, on the chunk-8 list.
    pub baseline_split: bool,
    /// How far apart two baseline peaks must sit, as a fraction of the
    /// band's median body height, before the fainter one is treated as a
    /// second line rather than baseline wobble on one line.
    ///
    /// A guess, on the chunk-8 list.
    pub baseline_split_sep: f32,
    /// The least support the fainter baseline peak may carry, as a fraction
    /// of the dominant peak's support, and the most weight the valley
    /// between them may carry, as a fraction of the fainter peak's support.
    /// Both read off the same number: below it, the second peak is either
    /// too faint to trust or not really separated from the first.
    ///
    /// A guess, on the chunk-8 list.
    pub baseline_split_support: f32,

    /// The run floor for [`strip_underlines`], as a multiple of the page's
    /// median glyphish-component height. Only a component whose width is at
    /// least this many median heights is examined; inside it, only a
    /// horizontal ink run at least this long is erased.
    ///
    /// Derived, not guessed: `ocrcer-build aspect` renders every class on
    /// every shippable face and reports the longest horizontal ink run
    /// divided by that face's x-height; this is twice the maximum over every
    /// class and face, the doubling being authored headroom so that the
    /// widest real letterform or dash this engine's own bank can produce
    /// never qualifies. See
    /// `docs/measurements/2026-09-23_underline_strip.txt`.
    pub rule_run_heights: f32,
    /// The height, as a multiple of the page's median glyphish-component
    /// height `h`, above which a strip-produced piece is discarded as rule
    /// debris rather than kept as a component.
    ///
    /// `ARCHITECTURE.md` section 11, 2026-09-23 ("Drop strip debris"):
    /// erasing a band out of an over-wide component can leave a tall/narrow
    /// remnant behind -- a box's vertical sides, once its top and bottom
    /// rules are gone -- that never existed as a component before the strip
    /// and is not glyph-shaped. This never applies to an ordinary component,
    /// only to a piece produced by re-labelling a component
    /// [`strip_underlines`] actually erased ink from.
    ///
    /// Derived, not guessed: `ocrcer-build aspect` renders every class on
    /// every shippable face and reports the tallest ink height divided by
    /// that face's x-height; this is twice the maximum over every class and
    /// face, the doubling being the same authored headroom
    /// [`Params::rule_run_heights`] uses, so the tallest real letterform
    /// this engine's own bank can produce never qualifies as debris. See
    /// `docs/measurements/2026-09-23_underline_strip.txt`.
    pub debris_heights: f32,
    /// The height, as a multiple of `h`, above which a strip-produced piece
    /// **narrower than `0.5 * h`** is discarded as debris even though it
    /// falls under [`Params::debris_heights`].
    ///
    /// `ARCHITECTURE.md` section 11, 2026-09-23 ("Two mechanisms named"):
    /// erasing a bordered numeric cell's top rule can leave the box's own
    /// left/right side behind as a narrow, tall sliver (observed 3x51px on
    /// `filing__r000055`/`r000044`) that is too narrow for
    /// [`Params::furniture_fraction`] and too short for `debris_heights`
    /// (51px under a 65px floor), so it survives as an ordinary component
    /// and seeds a wrong line-grouping band, fusing two real prose lines
    /// into one. This is a second, narrow-piece-only floor alongside
    /// `debris_heights`, not a replacement for it.
    ///
    /// The ink height is derived: `ocrcer-build aspect` reports the tallest
    /// ink height, in x-heights, among classes whose own ink width is
    /// `<= 0.5` x-height, over all 32 shippable faces. The `x1.5` headroom
    /// over that measured height is a **guess**, on chunk 8's tuning list --
    /// unlike `rule_run_heights`/`debris_heights`'s `x2`, it is deliberately
    /// smaller, because `x2` would sit too close to the observed sliver
    /// (about 3.6-3.9 x `h`). See
    /// `docs/measurements/2026-09-23_split_gate_and_strip_3b.txt`.
    pub thin_debris_heights: f32,
    /// Whether [`strip_underlines`] runs before line grouping. `false`
    /// leaves component labelling untouched.
    ///
    /// `ARCHITECTURE.md` section 11, 2026-09-23 ("Underlines are stripped
    /// from the pixels of over-wide components, not judged as whole
    /// components"): a whole-component gate cannot separate an underlined
    /// section heading from a genuine hyphen or minus sign without
    /// collateral damage, because both are wide relative to their own
    /// height. Stripping the rule's own ink out of an over-wide component
    /// and re-labelling what remains keeps ordinary dashes untouched -- they
    /// are never wide enough to be examined at all -- while recovering the
    /// letters an underline would otherwise fuse into one wrong component.
    /// Guess: ships `false` pending the readings in the measurement file
    /// above; not this crate's call to flip.
    pub underline_strip: bool,
}

/// The shipped values, taken from the parameter block rather than restated.
///
/// The block in [`crate::params`] is the single definition of every knob, and
/// it is what `model/params.tsv` is asserted against at build time. A second
/// copy here would be a second definition that nothing compares.
impl Default for Params {
    fn default() -> Self {
        crate::params::Params::DEFAULT.lines()
    }
}

/// Where a line's x-height came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XHeightSource {
    /// Read off the page: the line has a distinct x-height band, either
    /// because a taller band sits above it or because a descender proves
    /// lowercase is present.
    Observed,
    /// Converted from the line's only band through [`X_HEIGHT_PER_CAP`],
    /// because the line is all caps, all digits, or otherwise carries
    /// nothing that reaches above or below the band.
    FromCapHeight,
    /// Taken from the page rather than from the line, because the line's own
    /// members carry no x-height evidence: they are all punctuation-sized,
    /// so the band they form is not an x-height band at all. See
    /// [`Params::inherit_x_height_below`].
    Inherited,
}

/// One text line: its members and its vertical metrics.
///
/// `members` indexes the slice passed to [`group`], left to right. `baseline`
/// is the page y coordinate of the row *just below* flat-bottomed ink, which
/// is the convention [`crate::feature::GlyphInput::baseline_dy`] expects: a
/// glyph whose bottom row is the last ink row has `baseline_dy == height`.
#[derive(Debug, Clone)]
pub struct TextLine {
    pub members: Vec<usize>,
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
    pub baseline: f32,
    pub x_height: f32,
    pub cap_height: f32,
    pub x_height_source: XHeightSource,
    /// Median height of the members that defined the band, the scale the
    /// word splitter and the segmentation prior work in.
    pub median_height: u32,
}

impl TextLine {
    pub fn width(&self) -> u32 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> u32 {
        self.y1 - self.y0
    }
    /// This line's baseline measured downward from a bitmap whose top edge is
    /// at page row `top` — the form [`crate::feature::extract`] takes.
    pub fn baseline_dy(&self, top: u32) -> f32 {
        self.baseline - top as f32
    }
}

/// Groups components into lines with the default parameters.
pub fn group(components: &[Component], page_width: u32, page_height: u32) -> Vec<TextLine> {
    group_with(components, page_width, page_height, &Params::default())
}

/// Groups components into lines.
///
/// Components that are speckle or page furniture are left out of every line;
/// no line is ever empty.
pub fn group_with(
    components: &[Component],
    page_width: u32,
    page_height: u32,
    p: &Params,
) -> Vec<TextLine> {
    group_with_bands(components, page_width, page_height, p).into_iter().flatten().collect()
}

/// Groups components into lines, keeping each line grouped with the other
/// fragments the column cut split its band into.
///
/// [`group_with`] is this, flattened: every caller that does not care where
/// a line came from can keep using it. A caller that does -- the word
/// splitter's band-pooled space rule, `ARCHITECTURE.md` section 11 ("The
/// column cut's precision collapse...") -- needs the grouping, because a
/// fragment's own gaps are too few to show a reliable valley on their own.
/// A band the cut left whole, or the cut is off, comes back as a
/// single-fragment group, so this is a strict refinement of the same
/// ordering [`group_with`] already returns, not a different one.
pub fn group_with_bands(
    components: &[Component],
    page_width: u32,
    page_height: u32,
    p: &Params,
) -> Vec<Vec<TextLine>> {
    let mut order: Vec<usize> = (0..components.len())
        .filter(|&i| is_glyphish(&components[i], page_width, page_height, p))
        .collect();

    // Tallest first; equal heights in reading order, so the visit order is
    // fully determined by the component set.
    order.sort_by(|&a, &b| {
        let (ca, cb) = (&components[a], &components[b]);
        cb.height()
            .cmp(&ca.height())
            .then(ca.y0.cmp(&cb.y0))
            .then(ca.x0.cmp(&cb.x0))
            .then(ca.label.cmp(&cb.label))
    });

    let mut bands: Vec<Band> = Vec::new();
    for i in order {
        let c = &components[i];
        match best_band(&bands, c, p) {
            Some(b) => bands[b].absorb(i, c),
            None => bands.push(Band::new(i, c)),
        }
    }

    fold_marks(&mut bands, p);

    // Two-baseline split (`ARCHITECTURE.md` section 11, 2026-09-23, "Merged
    // lines: split on two baselines"): after marks have been folded in, so a
    // dot or accent already absorbed into its host band cannot be mistaken
    // for a second baseline's population, and before the reading-order sort
    // below, so a split band's pieces sort into the page order their own
    // content puts them in rather than inheriting the merged band's position.
    if p.baseline_split {
        bands = bands.into_iter().flat_map(|b| split_baselines(b, components, p)).collect();
    }

    // Reading order: down the page, then across, so a two-column page still
    // comes back in a stable order even though column detection is not this
    // stage's job.
    //
    // Settled here, on whole rows, because it cannot be settled after the
    // cut. Two fragments of one banded row do not share a top edge -- the
    // fragment holding the taller glyph starts higher -- so ordering the
    // pieces on `y0` returns a row right-hand-fragment first whenever their
    // tops differ, which on a label-and-value row is most of the time. The
    // words are all still there, so a bag-of-words score cannot see it; CER
    // reads the row backwards. Ordering the bands while a row is still one
    // object and cutting each left to right leaves the fragments adjacent and
    // in order, so there is nothing left to sort. The sort is stable, so
    // bands sharing a bounding box keep the order they were built in.
    bands.sort_by(|a, b| a.y0.cmp(&b.y0).then(a.x0.cmp(&b.x0)).then(a.x1.cmp(&b.x1)));

    let groups = split_at_column_gaps(bands, components, p);
    let sizes: Vec<usize> = groups.iter().map(Vec::len).collect();

    // Measured flat so `inherit_x_heights`' page-wide vote sees every line at
    // once, then re-nested by the sizes above -- which preserves the flat
    // order, so this costs nothing next to measuring group by group.
    let mut flat: Vec<TextLine> =
        groups.into_iter().flatten().map(|b| measure(b, components, p)).collect();
    inherit_x_heights(&mut flat, p);

    let mut out = Vec::with_capacity(sizes.len());
    let mut rest = flat.into_iter();
    for n in sizes {
        out.push((&mut rest).take(n).collect());
    }
    out
}

/// Gives a line with no x-height evidence of its own the page's.
///
/// A per-line measurement is the right default: a page mixes a heading, body
/// text and fine print, and a page-wide x-height would be wrong for all
/// three. It has one failure mode, and it is total rather than partial. A
/// line whose members are *all* punctuation-sized -- leader dots, a row of
/// underscores, a rule -- has a modal band that is not an x-height band,
/// and every one of its glyphs is then measured against a ruler a fifth of
/// the right length. The feature vector's last two dimensions are height
/// above and depth below the baseline in x-heights, so a full stop measured
/// this way reports as tall as an ascender and matches whatever class sits
/// at that height: on this project's charset, a middle dot or a bullet.
///
/// The repair is only available above the line, which is why it is a second
/// pass rather than part of [`measure`]. The page sets a typical x-height and
/// a line that came out below [`Params::inherit_x_height_below`] of it takes
/// it.
///
/// The page's value is a median of the lines' x-heights weighted by how much
/// of the page each line spans, for the same reason [`measure`] weights by
/// component width: a page's typical x-height is its body text's, and body
/// text is what covers the page.
///
/// **Who votes is two-tier, and the tiers exist for two different failures.**
/// Lines that observed a distinct x-height band vote when there are any: they
/// are the only lines that measured the quantity rather than converting it,
/// and letting the converted ones in would let a page of leaders elect the
/// leader height and confirm its own mistake. But a page can have no observed
/// line at all and still be perfectly ordinary -- small type, where the
/// ascender and x-height bands round into each other, or an all-caps form --
/// and refusing to act there leaves the failure this whole pass exists to
/// fix. So where no line observed a band, every line votes. A page that is
/// *nothing but* leaders then elects the leader height, no line falls below
/// the floor, and nothing changes, which is the correct no-op.
///
/// Deliberately conservative in two further ways: a line that observed a band
/// is never overridden, however small it is, because it has evidence and a
/// page average does not outrank evidence; and the baseline is never touched,
/// because a line of full stops sits *on* its baseline and measured it
/// correctly.
fn inherit_x_heights(lines: &mut [TextLine], p: &Params) {
    if lines.len() < 2 {
        return;
    }
    let vote = |l: &TextLine| (l.x_height.round().max(0.0) as u32, (l.x1 - l.x0).max(1));
    let observed: Vec<(u32, u32)> = lines
        .iter()
        .filter(|l| l.x_height_source == XHeightSource::Observed)
        .map(&vote)
        .filter(|&(h, _)| h > 0)
        .collect();
    let votes: Vec<(u32, u32)> = if observed.is_empty() {
        lines.iter().map(&vote).filter(|&(h, _)| h > 0).collect()
    } else {
        observed
    };
    if votes.is_empty() {
        return;
    }
    let page = weighted_median(votes.into_iter());
    let floor = f64::from(page) * f64::from(p.inherit_x_height_below);
    for l in lines.iter_mut() {
        if l.x_height_source == XHeightSource::Observed {
            continue;
        }
        if f64::from(l.x_height) >= floor {
            continue;
        }
        l.x_height = page as f32;
        l.cap_height = (page as f32 / p.x_height_per_cap).max(page as f32);
        l.x_height_source = XHeightSource::Inherited;
    }
}

/// Whether a component is a plausible piece of a glyph, as opposed to speckle
/// or page furniture.
fn is_glyphish(c: &Component, page_width: u32, page_height: u32, p: &Params) -> bool {
    if c.area < p.min_area || c.width() == 0 || c.height() == 0 {
        return false;
    }
    let wide = f64::from(c.width()) > f64::from(page_width) * f64::from(p.furniture_fraction);
    let tall = f64::from(c.height()) > f64::from(page_height) * f64::from(p.furniture_fraction);
    if wide || tall {
        return false;
    }
    if p.rule_aspect > 0.0 {
        let (lo, hi) = (c.width().min(c.height()), c.width().max(c.height()));
        if f64::from(hi) > f64::from(p.rule_aspect) * f64::from(lo) {
            return false;
        }
    }
    true
}

/// One erased rule or underline band, in the page's own pixel coordinates
/// (`x1`/`y1` exclusive, the same convention [`Component`] uses).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleSegment {
    pub x0: u32,
    pub x1: u32,
    pub y0: u32,
    pub y1: u32,
}

/// The result of [`strip_underlines`].
pub struct StripOutput {
    /// The modified mask's component labels.
    pub labels: Vec<u32>,
    /// The modified mask's components.
    pub components: Vec<Component>,
    /// Every rule or underline band this pass erased, for an output stage
    /// that wants to record underline formatting (`ARCHITECTURE.md` section
    /// 11, 2026-09-23, "Keep what was stripped"). Nothing consumes this yet.
    pub rule_segments: Vec<RuleSegment>,
}

/// Strips a table rule or an underline out of over-wide components, in
/// place, and re-labels what remains.
///
/// `ARCHITECTURE.md` section 11, 2026-09-23 ("Underline strip, second rule:
/// straight bands only, keep crossing strokes, drop strip debris, and keep
/// what was stripped"):
///
/// 1. Take the page's median height of glyphish components, `h`, and the run
///    floor `L = p.rule_run_heights * h`. Only a component whose width is at
///    least `L` is examined, same as before -- this is what keeps every
///    hyphen, minus sign, em dash and ordinary word out of this pass
///    entirely (see [`Params::rule_run_heights`]).
/// 2. **Bands, not rows.** Within an examined component, only a band of at
///    least 2 consecutive rows is erased, each row holding exactly one run
///    of the component's own ink at least `L` long, with the run's `x0`/`x1`
///    agreeing within 2px of the previous row in the band. A single
///    qualifying row, or a row holding more than one qualifying run, breaks
///    the band instead of joining it and is never erased.
/// 3. **Keep crossing strokes.** Within a band's x-range, a column's ink is
///    erased only where that column's own vertical run of ink -- the run
///    overlapping the band's rows, which may reach above or below them --
///    is no taller than the band's row count plus 1. A stroke that crosses
///    the rule (a `)`'s bowl, a box's vertical side) keeps every pixel.
/// 4. **Drop strip debris.** After the remaining pixels of every examined
///    component are re-labelled into sub-components (horizontal only: there
///    is no evidence yet of vertical rules touching text), a piece produced
///    by re-labelling a component this pass actually erased ink from --
///    entirely inside that component's original box -- is discarded if its
///    height exceeds `p.debris_heights * h`. A piece is also discarded, even
///    under that floor, when its width is `<= 0.5 * h` **and** its height
///    exceeds `p.thin_debris_heights * h` -- a box's left/right side left
///    behind by an erased top or bottom rule is exactly this shape
///    (`ARCHITECTURE.md` section 11, 2026-09-23, "Two mechanisms named").
///    This never touches an ordinary component that was never stripped.
///
/// Each accepted band from step 2 is recorded as a [`RuleSegment`] whether
/// or not step 3 kept some of its columns.
///
/// `mask` is modified in place; the returned labels and components describe
/// the modified mask, replacing what a caller would otherwise have labelled
/// directly. A no-op (returns the labels/components of the untouched mask,
/// no rule segments) when `p.rule_run_heights <= 0.0` or no component is
/// glyphish.
pub fn strip_underlines(mask: &mut [u8], width: u32, height: u32, p: &Params) -> StripOutput {
    let (labels0, count0) =
        crate::image::components::label(mask, width, height, crate::image::components::Connectivity::Eight);
    let comps0 = crate::image::components::components(&labels0, width, height, count0);

    let mut rule_segments: Vec<RuleSegment> = Vec::new();
    let mut stripped_boxes: Vec<(u32, u32, u32, u32)> = Vec::new();
    let mut h = 0.0f64;

    if p.rule_run_heights > 0.0 {
        let mut heights: Vec<u32> =
            comps0.iter().filter(|c| is_glyphish(c, width, height, p)).map(Component::height).collect();
        if !heights.is_empty() {
            heights.sort_unstable();
            h = f64::from(heights[heights.len() / 2]);
            let run_floor = f64::from(p.rule_run_heights) * h;
            if run_floor > 0.0 {
                for c in &comps0 {
                    if f64::from(c.width()) >= run_floor {
                        let erased =
                            strip_component_bands(mask, &labels0, width, c, run_floor, &mut rule_segments);
                        if erased {
                            stripped_boxes.push((c.x0, c.y0, c.x1, c.y1));
                        }
                    }
                }
            }
        }
    }

    let (mut labels, mut comps) = {
        let (l, count) =
            crate::image::components::label(mask, width, height, crate::image::components::Connectivity::Eight);
        let c = crate::image::components::components(&l, width, height, count);
        (l, c)
    };

    if (p.debris_heights > 0.0 || p.thin_debris_heights > 0.0) && h > 0.0 && !stripped_boxes.is_empty() {
        let debris_floor = f64::from(p.debris_heights) * h;
        let thin_debris_floor = f64::from(p.thin_debris_heights) * h;
        let thin_width_ceiling = 0.5 * h;
        let mut any_dropped = false;
        for c in &comps {
            let from_strip = stripped_boxes
                .iter()
                .any(|&(sx0, sy0, sx1, sy1)| c.x0 >= sx0 && c.x1 <= sx1 && c.y0 >= sy0 && c.y1 <= sy1);
            if !from_strip {
                continue;
            }
            let height = f64::from(c.height());
            let is_debris = (p.debris_heights > 0.0 && height > debris_floor)
                || (p.thin_debris_heights > 0.0
                    && f64::from(c.width()) <= thin_width_ceiling
                    && height > thin_debris_floor);
            if is_debris {
                erase_component(mask, &labels, width, c);
                any_dropped = true;
            }
        }
        if any_dropped {
            let (l, count) =
                crate::image::components::label(mask, width, height, crate::image::components::Connectivity::Eight);
            comps = crate::image::components::components(&l, width, height, count);
            labels = l;
        }
    }

    StripOutput { labels, components: comps, rule_segments }
}

/// Sets every pixel `labels` attributes to `c` back to background.
fn erase_component(mask: &mut [u8], labels: &[u32], width: u32, c: &Component) {
    let w = width as usize;
    for y in c.y0..c.y1 {
        let row = y as usize * w;
        for x in c.x0..c.x1 {
            if labels[row + x as usize] == c.label {
                mask[row + x as usize] = 0;
            }
        }
    }
}

/// Erases the straight rule/underline bands within `c`, keeping any column
/// whose ink reaches past the band. Returns whether any pixel was erased.
///
/// Checked against `labels`, not `mask`, so a neighbouring component that
/// happens to share a row or an x-range inside this box is left alone: only
/// pixels this component's own label owns are candidates.
fn strip_component_bands(
    mask: &mut [u8],
    labels: &[u32],
    width: u32,
    c: &Component,
    run_floor: f64,
    rule_segments: &mut Vec<RuleSegment>,
) -> bool {
    let w = width as usize;

    // Group each row's single qualifying run (if any) into bands of >= 2
    // consecutive rows whose runs agree within 2px of the row before them.
    let mut bands: Vec<Vec<(u32, u32, u32)>> = Vec::new();
    let mut current: Vec<(u32, u32, u32)> = Vec::new();
    let flush = |current: &mut Vec<(u32, u32, u32)>, bands: &mut Vec<Vec<(u32, u32, u32)>>| {
        if current.len() >= 2 {
            bands.push(std::mem::take(current));
        } else {
            current.clear();
        }
    };
    for y in c.y0..c.y1 {
        match single_qualifying_run(labels, width, c, y, run_floor) {
            Some((x0, x1)) => {
                let joins = current
                    .last()
                    .is_some_and(|&(_, px0, px1)| (x0 as i64 - px0 as i64).abs() <= 2 && (x1 as i64 - px1 as i64).abs() <= 2);
                if !joins {
                    flush(&mut current, &mut bands);
                }
                current.push((y, x0, x1));
            }
            None => flush(&mut current, &mut bands),
        }
    }
    flush(&mut current, &mut bands);

    let mut erased = false;
    for band in &bands {
        let y0 = band.first().expect("flush only keeps bands with >= 2 rows").0;
        let y1 = band.last().expect("flush only keeps bands with >= 2 rows").0 + 1;
        let x0 = band.iter().map(|&(_, bx0, _)| bx0).min().expect("band is non-empty");
        let x1 = band.iter().map(|&(_, _, bx1)| bx1).max().expect("band is non-empty");
        rule_segments.push(RuleSegment { x0, x1, y0, y1 });

        let thickness = band.len() as u32;
        for x in x0..x1 {
            if let Some((ry0, ry1)) = column_ink_run(labels, width, c, x, y0, y1 - 1) {
                if ry1 - ry0 <= thickness + 1 {
                    for ry in ry0..ry1 {
                        mask[ry as usize * w + x as usize] = 0;
                    }
                    erased = true;
                }
            }
        }
    }
    erased
}

/// The one run of `c`'s own ink on row `y` that reaches `run_floor` pixels,
/// or `None` if there is no such run or more than one.
fn single_qualifying_run(labels: &[u32], width: u32, c: &Component, y: u32, run_floor: f64) -> Option<(u32, u32)> {
    let w = width as usize;
    let row = y as usize * w;
    let mut found = None;
    let mut x = c.x0;
    while x < c.x1 {
        if labels[row + x as usize] == c.label {
            let start = x;
            while x < c.x1 && labels[row + x as usize] == c.label {
                x += 1;
            }
            if f64::from(x - start) >= run_floor {
                if found.is_some() {
                    return None;
                }
                found = Some((start, x));
            }
        } else {
            x += 1;
        }
    }
    found
}

/// The run of `c`'s own ink in column `x`, within `c`'s own bounding box,
/// that overlaps rows `band_y0..=band_y1_incl` -- `None` if column `x` has
/// no such ink.
fn column_ink_run(
    labels: &[u32],
    width: u32,
    c: &Component,
    x: u32,
    band_y0: u32,
    band_y1_incl: u32,
) -> Option<(u32, u32)> {
    let w = width as usize;
    let mut y = c.y0;
    while y < c.y1 {
        if labels[y as usize * w + x as usize] == c.label {
            let start = y;
            while y < c.y1 && labels[y as usize * w + x as usize] == c.label {
                y += 1;
            }
            if start <= band_y1_incl && y > band_y0 {
                return Some((start, y));
            }
        } else {
            y += 1;
        }
    }
    None
}

/// A line under construction: its span, its members, and the sorted member
/// heights the running median is taken from.
struct Band {
    members: Vec<usize>,
    heights: Vec<u32>,
    x0: u32,
    x1: u32,
    y0: u32,
    y1: u32,
}

impl Band {
    fn new(i: usize, c: &Component) -> Band {
        Band {
            members: vec![i],
            heights: vec![c.height()],
            x0: c.x0,
            x1: c.x1,
            y0: c.y0,
            y1: c.y1,
        }
    }

    fn absorb(&mut self, i: usize, c: &Component) {
        self.members.push(i);
        let at = self.heights.partition_point(|&h| h < c.height());
        self.heights.insert(at, c.height());
        self.x0 = self.x0.min(c.x0);
        self.x1 = self.x1.max(c.x1);
        self.y0 = self.y0.min(c.y0);
        self.y1 = self.y1.max(c.y1);
    }

    fn take(&mut self, other: Band) {
        self.members.extend(other.members);
        self.heights.extend(other.heights);
        self.heights.sort_unstable();
        self.x0 = self.x0.min(other.x0);
        self.x1 = self.x1.max(other.x1);
        self.y0 = self.y0.min(other.y0);
        self.y1 = self.y1.max(other.y1);
    }

    /// Tallest member: what decides whether the whole band is marks.
    fn max_height(&self) -> u32 {
        *self.heights.last().expect("a band always has a member")
    }

    /// Vertical gap between two bands, zero when their spans touch or
    /// overlap.
    fn gap(&self, other: &Band) -> u32 {
        if self.y1 <= other.y0 {
            other.y0 - self.y1
        } else if other.y1 <= self.y0 {
            self.y0 - other.y1
        } else {
            0
        }
    }

    fn overlaps_x(&self, other: &Band) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1
    }

    /// Running median height. Even counts take the upper middle, so the value
    /// is always one of the observed heights and never a half-pixel.
    fn median(&self) -> u32 {
        self.heights[self.heights.len() / 2]
    }

    fn overlap(&self, c: &Component) -> u32 {
        self.y1.min(c.y1).saturating_sub(self.y0.max(c.y0))
    }
}

/// Folds mark bands — rows of `i`-dots, accents, anything that sits clear of
/// the band it belongs to — into the text band they decorate.
///
/// The seeding sweep cannot do this on its own: on a line with no capital and
/// no ascender the dots sit entirely above the x-height band, share no
/// vertical overlap with it, and so seed a band of their own. That band is
/// recognisable after the fact by being far shorter than its neighbour and
/// sitting within reach of it.
///
/// Runs to a fixed point, which terminates because every round that changes
/// anything removes a band.
fn fold_marks(bands: &mut Vec<Band>, p: &Params) {
    loop {
        let mut chosen: Option<(usize, usize)> = None; // mark, host
        'outer: for m in 0..bands.len() {
            let mark = &bands[m];
            let mut best: Option<(usize, u32)> = None;
            for (h, host) in bands.iter().enumerate() {
                if h == m || !mark.overlaps_x(host) {
                    continue;
                }
                let median = host.median();
                if f64::from(mark.max_height()) > f64::from(p.mark_height_fraction) * f64::from(median)
                {
                    continue;
                }
                let gap = mark.gap(host);
                if f64::from(gap) > f64::from(p.mark_reach_fraction) * f64::from(median) {
                    continue;
                }
                // Nearest wins; on a tie the band *below* the mark takes it,
                // because a mark that is equally close to the line above and
                // the line below is an ascender-zone mark far more often than
                // a descender-zone one.
                let better = match best {
                    None => true,
                    Some((bh, bg)) => {
                        gap < bg || (gap == bg && host.y0 >= mark.y1 && bands[bh].y0 < mark.y1)
                    }
                };
                if better {
                    best = Some((h, gap));
                }
            }
            if let Some((h, _)) = best {
                chosen = Some((m, h));
                break 'outer;
            }
        }
        let Some((m, h)) = chosen else { return };
        let mark = bands.remove(m);
        let h = if h > m { h - 1 } else { h };
        bands[h].take(mark);
    }
}

/// Cuts every band wherever a horizontal gap is wide enough to be a column or
/// a box boundary rather than a word space, keeping each band's own pieces
/// grouped together in the outer `Vec`.
///
/// Runs before [`measure`] on purpose. A band that spans two boxes set at
/// different sizes has no single correct x-height, so measuring it first and
/// cutting afterwards would leave both halves carrying metrics taken from
/// the other. See [`Params::column_gap_heights`] for what that costs.
///
/// The threshold is in median component heights rather than x-heights
/// because no x-height exists yet, and the median height of the band is the
/// only scale available before [`measure`] runs.
///
/// A band the cut does not fire on -- the cut is off, or the band has no gap
/// wide enough -- comes back as its own one-element group, so the grouping
/// this returns is exactly "which fragments came from the same original
/// band" and nothing more: [`group_with_bands`] is what a caller wanting the
/// band-pooled space rule of `ARCHITECTURE.md` section 11 uses this for.
///
/// Every gap wider than the cut is a candidate. With [`Params::column_lone_guard`]
/// on, a candidate survives only unless the raw fragments it separates -- as
/// formed by all candidates together -- are *both* single components: that
/// is a run of leader dots, not a column boundary, and a lone glyph beside a
/// multi-glyph fragment is still cut off (`ARCHITECTURE.md` section 6/11,
/// 2026-09-23). With the guard off, every candidate survives -- the plain
/// greedy cut, byte-identical to cutting at every candidate. A dropped
/// candidate leaves its fragments joined. One pass builds the raw fragments,
/// a second filters and merges, so the result is a function of the component
/// set alone.
fn split_at_column_gaps(bands: Vec<Band>, components: &[Component], p: &Params) -> Vec<Vec<Band>> {
    if !(p.column_gap_heights > 0.0) {
        return bands.into_iter().map(|b| vec![b]).collect();
    }
    let mut out: Vec<Vec<Band>> = Vec::with_capacity(bands.len());
    for band in bands {
        // Width-weighted, for the reason `measure` is: on a leader line the
        // unweighted median height is the height of a period, which would put
        // the cut threshold below an ordinary word space and dice the line.
        let scale = weighted_median(band.members.iter().map(|&i| {
            let c = &components[i];
            (c.height(), c.width().max(1))
        }));
        let cut = f64::from(p.column_gap_heights) * f64::from(scale);
        let mut ms = band.members;
        // Left to right, with the component label breaking a tie, so the cut
        // points are a function of the component set alone.
        ms.sort_unstable_by_key(|&i| {
            let c = &components[i];
            (c.x0, c.x1, c.label)
        });
        let mut raw: Vec<Band> = Vec::new();
        let mut cur: Option<Band> = None;
        let mut reach = 0u32;
        for i in ms {
            let c = &components[i];
            let split = cur.is_some() && f64::from(c.x0.saturating_sub(reach)) > cut;
            if split {
                raw.push(cur.take().expect("split implies a band in hand"));
            }
            match cur.as_mut() {
                Some(b) => {
                    b.absorb(i, c);
                    reach = reach.max(c.x1);
                }
                None => {
                    cur = Some(Band::new(i, c));
                    reach = c.x1;
                }
            }
        }
        if let Some(b) = cur {
            raw.push(b);
        }
        // A candidate boundary between raw[k-1] and raw[k] is dropped only
        // when the guard is on and both sides, measured on the raw
        // (all-candidates) partition, are exactly one component -- two lone
        // glyphs, the leader-dot case. Sizes are read from `raw` before any
        // merge, so merging a dropped boundary never changes a neighbour's
        // verdict. With the guard off this keeps every boundary, so `pieces`
        // is `raw` unchanged -- the plain greedy cut.
        let sizes: Vec<usize> = raw.iter().map(|b| b.members.len()).collect();
        let mut pieces: Vec<Band> = Vec::with_capacity(raw.len());
        for (k, piece) in raw.into_iter().enumerate() {
            let keep_boundary =
                k > 0 && (!p.column_lone_guard || !(sizes[k - 1] == 1 && sizes[k] == 1));
            if keep_boundary {
                pieces.push(piece);
            } else if let Some(prev) = pieces.last_mut() {
                prev.take(piece);
            } else {
                pieces.push(piece);
            }
        }
        out.push(pieces);
    }
    out
}

/// The band this component joins, or `None` to start a new one.
///
/// Among bands that clear the overlap test, the greatest overlap *ratio*
/// wins; ties go to the greater overlap in pixels, then to the band created
/// first. Comparing ratios as cross-multiplied integers keeps the choice
/// exact.
fn best_band(bands: &[Band], c: &Component, p: &Params) -> Option<usize> {
    let mut best: Option<(usize, u64, u64, u32)> = None; // index, num, den, overlap
    for (b, band) in bands.iter().enumerate() {
        let ov = band.overlap(c);
        if ov == 0 {
            continue;
        }
        let den = c.height().min(band.median()).max(1);
        if f64::from(ov) < f64::from(p.overlap_fraction) * f64::from(den)
            && !hangs_below(band, c, p)
        {
            continue;
        }
        let (num, den) = (u64::from(ov), u64::from(den));
        let better = match best {
            None => true,
            Some((_, bn, bd, bov)) => num * bd > bn * den || (num * bd == bn * den && ov > bov),
        };
        if better {
            best = Some((b, num, den, ov));
        }
    }
    best.map(|(i, _, _, _)| i)
}

/// Whether `c` is a descender of `band`: top edge inside the band, bottom
/// edge no further below it than a descender reaches.
///
/// Admitted on its own terms rather than by loosening
/// [`Params::overlap_fraction`], because the two tests are about different
/// things. The ratio test asks how much of a component the band already
/// explains; this asks whether the part the band does not explain is a
/// descender's worth of ink or a whole other line's.
fn hangs_below(band: &Band, c: &Component, p: &Params) -> bool {
    if c.y0 < band.y0 || c.y0 >= band.y1 {
        return false;
    }
    let below = c.y1.saturating_sub(band.y1);
    f64::from(below) <= f64::from(p.descender_reach_fraction) * f64::from(band.median())
}

/// Turns a finished band into a measured line.
fn measure(band: Band, components: &[Component], p: &Params) -> TextLine {
    let mut members = band.members;
    members.sort_by(|&a, &b| {
        let (ca, cb) = (&components[a], &components[b]);
        ca.x0.cmp(&cb.x0).then(ca.y0.cmp(&cb.y0)).then(ca.label.cmp(&cb.label))
    });

    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for &i in &members {
        let c = &components[i];
        x0 = x0.min(c.x0);
        y0 = y0.min(c.y0);
        x1 = x1.max(c.x1);
        y1 = y1.max(c.y1);
    }

    let median = band.heights[band.heights.len() / 2];

    // The reference height the body filter is taken against, and the body
    // itself: see [`reference_height`] and [`body_of`], shared with
    // [`split_baselines`] so both stages read "body" the same way.
    let reference = reference_height(&members, components);
    let body = body_of(&members, components, reference);

    let baseline =
        mode_of_weighted(body.iter().map(|c| (c.y1, c.width().max(1))), y0, y1 + 2) as f32;

    // Heights above the baseline, one per body member, each weighted by its
    // component's width for the same reason the reference height is. Clamped
    // at zero: a member sitting entirely below the baseline contributes no
    // top band.
    let tops: Vec<(u32, u32)> = body
        .iter()
        .map(|c| ((baseline - c.y0 as f32).max(0.0).round() as u32, c.width().max(1)))
        .collect();
    let top_max = tops.iter().map(|&(h, _)| h).max().unwrap_or(1).max(1);
    let main = mode_of_weighted(tops.iter().copied(), 0, top_max + 2).max(1);

    // A band clearly above the modal one means the modal one is the x-height
    // band: the line has both capitals (or ascenders) and lowercase on it.
    let upper: Vec<(u32, u32)> =
        tops.iter().copied().filter(|&(h, _)| f64::from(h) > f64::from(main) * 1.15).collect();

    let deepest = body
        .iter()
        .map(|c| (c.y1 as f32 - baseline).max(0.0))
        .fold(0.0f32, f32::max);
    let descends = f64::from(deepest) > f64::from(p.descender_fraction) * f64::from(main);

    // The plausibility floor: a band clearly above the modal one is only
    // read as a genuine cap band when the modal one is itself a plausible
    // x-height for it. Without this, a band whose `main` is a clamped
    // artifact (every member of a wrongly-absorbed second line hitting the
    // `top == 0` floor, see [`Params::x_height_floor_per_cap`]) reports
    // `Observed` and defeats `inherit_x_heights`, the mechanism that exists
    // to repair exactly this symptom. Failing the floor falls through
    // exactly as if `upper` were empty.
    let cap_candidate = if upper.is_empty() {
        None
    } else {
        let cap = mode_of_weighted(upper.iter().copied(), main, top_max + 2).max(main + 1);
        let plausible = f64::from(main) >= f64::from(p.x_height_floor_per_cap) * f64::from(cap);
        plausible.then_some(cap)
    };

    // `upper` present but rejected by the floor is a different case from
    // `upper` genuinely empty, even though both fall through to the same
    // `cap_candidate == None`: a real single-band line with a descender
    // (`upper` empty from the start) has an untainted `main`, but a band
    // whose cap candidate just failed the floor has already shown `main`
    // itself is the clamped artifact -- the same corrupted value `descends`
    // would otherwise read as a genuine x-height. Letting `descends` fire
    // there reproduces the defect the floor exists to close, one branch
    // over: still `Observed`, still built on `main == 1`. So this case
    // skips `descends` too and falls all the way through, exactly where the
    // floor's own doc comment says the line should land -- at
    // `inherit_x_heights`, which does not look past the label to ask why.
    let main_is_suspect = !upper.is_empty() && cap_candidate.is_none();

    let (x_height, cap_height, source) = if let Some(cap) = cap_candidate {
        (main as f32, cap as f32, XHeightSource::Observed)
    } else if descends && !main_is_suspect {
        // One band, but something hangs below the baseline, and only
        // lowercase does that — so the band is the x-height band.
        (main as f32, main as f32 / p.x_height_per_cap, XHeightSource::Observed)
    } else {
        // One band and nothing below it: all caps, all digits, or a line too
        // short to say. Read it as the cap band and convert.
        (main as f32 * p.x_height_per_cap, main as f32, XHeightSource::FromCapHeight)
    };

    TextLine {
        members,
        x0,
        y0,
        x1,
        y1,
        baseline,
        x_height,
        cap_height,
        x_height_source: source,
        median_height: median,
    }
}

/// The mode of a weighted set of `u32` values in `lo..hi`, over a histogram
/// smoothed with a `1 2 1` kernel.
///
/// Smoothed because round letters overshoot the baseline by a pixel and a
/// rasteriser rounds edges inconsistently, so the raw histogram of a real
/// line has its mass spread over two or three adjacent bins rather than piled
/// on one. Ties take the smaller value: the lower of two equally supported
/// bands is the more conservative reading in both the places this is used.
///
/// The weight [`measure`] passes is the component's width in pixels, so the
/// histogram counts how much of the line's horizontal extent supports a band
/// rather than how many components do. Weight zero is treated as one by the
/// caller; a zero-width component cannot exist and would silently vanish.
fn mode_of_weighted(values: impl Iterator<Item = (u32, u32)>, lo: u32, hi: u32) -> u32 {
    if hi <= lo {
        return lo;
    }
    let hist = weighted_hist(values, lo, hi);
    match argmax_hist(&hist) {
        Some(i) => lo + i as u32,
        None => lo,
    }
}

/// The `1 2 1`-smoothed histogram of a weighted set of `u32` values in
/// `lo..hi`, bin `i` holding the value `lo + i`.
///
/// Factored out of [`mode_of_weighted`] so [`split_point`] can look for a
/// second peak away from the first over the same histogram, rather than
/// reimplementing the smoothing this stage already gets right (`CLAUDE.md`
/// rule 4). See [`mode_of_weighted`] for why the kernel exists.
fn weighted_hist(values: impl Iterator<Item = (u32, u32)>, lo: u32, hi: u32) -> Vec<u64> {
    let n = (hi.saturating_sub(lo)) as usize;
    // u64 because the weights are pixel widths summed over a page-wide line;
    // u32 would need a 2-billion-pixel line to overflow, but the cast is
    // free and the reader does not have to check.
    let mut hist = vec![0u64; n];
    for (v, w) in values {
        if v < lo || v >= hi {
            continue;
        }
        let w = u64::from(w);
        let i = (v - lo) as usize;
        hist[i] += 2 * w;
        if i > 0 {
            hist[i - 1] += w;
        }
        if i + 1 < n {
            hist[i + 1] += w;
        }
    }
    hist
}

/// The index of the tallest bin in a histogram from [`weighted_hist`], or
/// `None` when every bin is empty. Ties take the smaller index, the same
/// convention [`mode_of_weighted`] documents.
fn argmax_hist(hist: &[u64]) -> Option<usize> {
    if hist.iter().all(|&h| h == 0) {
        return None;
    }
    let mut best = 0usize;
    for i in 1..hist.len() {
        if hist[i] > hist[best] {
            best = i;
        }
    }
    Some(best)
}

/// The weighted median of `(value, weight)` pairs, or `1` when there are
/// none.
///
/// With every weight one this is the plain median taking the upper middle of
/// an even count, which is the convention [`Band::median`] uses, so a line
/// whose members are all the same width measures exactly as it did before
/// weighting existed.
fn weighted_median(values: impl Iterator<Item = (u32, u32)>) -> u32 {
    let mut v: Vec<(u32, u32)> = values.collect();
    if v.is_empty() {
        return 1;
    }
    v.sort_unstable();
    let total: u64 = v.iter().map(|&(_, w)| u64::from(w)).sum();
    let half = total / 2;
    let mut cum = 0u64;
    for &(value, w) in &v {
        cum += u64::from(w);
        if cum > half {
            return value;
        }
    }
    v[v.len() - 1].0
}

/// The reference height a body filter is taken against: the median member
/// height, weighted by member width.
///
/// Weighted and unweighted agree on a line of ordinary text. They stop
/// agreeing on a line of dot leaders, where punctuation outnumbers text and a
/// count-based median returns the height of a period -- every measurement
/// taken against that ruler calls every letter an implausible ascender and
/// loses the whole line, not just the leaders. Weighting by width asks which
/// population owns the line's horizontal extent rather than which is more
/// numerous, and text owns the extent even where leaders outnumber it.
///
/// Shared by [`measure`] and [`split_baselines`], which need the same
/// "body" population for the same reason: marks and leaders carry no
/// baseline evidence and would only blur either stage's histogram.
fn reference_height(members: &[usize], components: &[Component]) -> u32 {
    weighted_median(members.iter().map(|&i| {
        let c = &components[i];
        (c.height(), c.width().max(1))
    }))
}

/// The body-sized members of a band: everything at least half the reference
/// height, or every member when that filter would leave nothing. See
/// [`reference_height`].
fn body_of<'a>(
    members: &[usize],
    components: &'a [Component],
    reference: u32,
) -> Vec<&'a Component> {
    let body: Vec<&Component> = members
        .iter()
        .map(|&i| &components[i])
        .filter(|c| f64::from(c.height()) >= 0.5 * f64::from(reference))
        .collect();
    if body.is_empty() {
        members.iter().map(|&i| &components[i]).collect()
    } else {
        body
    }
}

/// Splits a band whose `body` members' baselines are bimodal into two,
/// recursing on each half so a three-line pile-up separates into three. The
/// upper half is returned first, in reading order.
///
/// A real text line has exactly one baseline; a band two lines were merged
/// into (`best_band`'s admission running against a shrinking median, not a
/// blank row ever separating the ink -- `ARCHITECTURE.md` section 11,
/// 2026-09-23, "Merged lines: split on two baselines") has two,
/// well-separated and each carrying its own share of the band's width. This
/// is keyed off the *baseline* (`body` members' `y1`) population rather than
/// the *top* population on purpose: an ordinary mixed-case line has one
/// baseline and a bimodal top histogram by design -- that bimodality is
/// exactly what lets [`measure`] tell x-height from cap-height in the first
/// place -- so a rule keyed off tops would flag every such line. Baseline
/// bimodality has no equivalent false-positive path: mixed case, ascenders
/// and descenders all still land on one baseline, which is what the
/// must-not-split fixtures below assert.
///
/// A no-op (returns `vec![band]`) unless [`split_point`] finds a genuine
/// second peak; see there for the exact test.
fn split_baselines(band: Band, components: &[Component], p: &Params) -> Vec<Band> {
    let reference = reference_height(&band.members, components);
    let body = body_of(&band.members, components, reference);
    // The band's median body height, unweighted: the scale
    // [`Params::baseline_split_sep`] reads distance between peaks against.
    let mut heights: Vec<u32> = body.iter().map(|c| c.height()).collect();
    heights.sort_unstable();
    let median_body = if heights.is_empty() { 1 } else { heights[heights.len() / 2] };

    let Some(cut) = split_point(&body, median_body, p) else {
        return vec![band];
    };

    let (mut above, mut below) = (Vec::new(), Vec::new());
    for &i in &band.members {
        if components[i].y1 as f32 <= cut {
            above.push(i);
        } else {
            below.push(i);
        }
    }
    // The peak/support tests already guarantee both halves are non-empty in
    // practice -- each peak has positive support from a real member -- but a
    // band is never allowed to lose a member to a degenerate split.
    if above.is_empty() || below.is_empty() {
        return vec![band];
    }

    let mut out = split_baselines(band_from(above, components), components, p);
    out.extend(split_baselines(band_from(below, components), components, p));
    out
}

/// Builds a [`Band`] from a member list, recomputing its bounding box and
/// height histogram. What [`split_baselines`] uses to turn each half of a
/// split back into a band [`measure`] can read.
fn band_from(members: Vec<usize>, components: &[Component]) -> Band {
    let mut heights: Vec<u32> = members.iter().map(|&i| components[i].height()).collect();
    heights.sort_unstable();
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for &i in &members {
        let c = &components[i];
        x0 = x0.min(c.x0);
        y0 = y0.min(c.y0);
        x1 = x1.max(c.x1);
        y1 = y1.max(c.y1);
    }
    Band { members, heights, x0, x1, y0, y1 }
}

/// The `y1` cut between two baseline peaks, or `None` when the `body`
/// population does not clear the bar for calling it two lines.
///
/// The primary peak is the mode of the width-weighted `y1` histogram. The
/// secondary peak is the best-supported bin at least
/// [`Params::baseline_split_sep`] times `median_body` away from it -- close
/// enough and it is baseline wobble on one line, not a second one. A split
/// fires only when the secondary peak's support is at least
/// [`Params::baseline_split_support`] of the primary's, *and* the weight
/// strictly between the two peaks' immediate (±1 bin) neighbourhoods is no
/// more than that same fraction of the secondary peak's support -- a real
/// valley between them, not a populated slope. The cut is the midpoint
/// between the two peaks.
fn split_point(body: &[&Component], median_body: u32, p: &Params) -> Option<f32> {
    if body.len() < 2 || median_body == 0 {
        return None;
    }
    let lo = body.iter().map(|c| c.y1).min()?;
    let hi = body.iter().map(|c| c.y1).max()?;
    if hi <= lo {
        return None;
    }
    let hist = weighted_hist(body.iter().map(|c| (c.y1, c.width().max(1))), lo, hi + 1);
    let i1 = argmax_hist(&hist)?;

    let sep = f64::from(p.baseline_split_sep) * f64::from(median_body);
    let i2 = (0..hist.len())
        .filter(|&i| (i as f64 - i1 as f64).abs() >= sep)
        .fold(None, |best: Option<usize>, i| match best {
            Some(b) if hist[b] >= hist[i] => Some(b),
            _ if hist[i] > 0 => Some(i),
            _ => best,
        })?;

    let support1 = hist[i1];
    let support2 = hist[i2];
    if support1 == 0 || support2 == 0 {
        return None;
    }
    if (support2 as f64) < f64::from(p.baseline_split_support) * (support1 as f64) {
        return None;
    }

    let (a, b) = (i1.min(i2), i1.max(i2));
    let valley: u64 = if b >= a + 4 { hist[a + 2..=b - 2].iter().sum() } else { 0 };
    if (valley as f64) > f64::from(p.baseline_split_support) * (support2 as f64) {
        return None;
    }

    Some(lo as f32 + (a as f32 + b as f32) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(label: u32, x0: u32, y0: u32, w: u32, h: u32) -> Component {
        Component { label, x0, y0, x1: x0 + w, y1: y0 + h, area: w * h }
    }

    /// Two lines of the same size, set one above the other, must come back as
    /// two lines in top-to-bottom order.
    #[test]
    fn two_rows_of_text_are_two_lines() {
        let comps = vec![
            c(1, 10, 10, 8, 10),
            c(2, 20, 10, 8, 10),
            c(3, 30, 10, 8, 10),
            c(4, 10, 30, 8, 10),
            c(5, 20, 30, 8, 10),
        ];
        let lines = group(&comps, 200, 100);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].members.len(), 3);
        assert_eq!(lines[1].members.len(), 2);
        assert!(lines[0].y0 < lines[1].y0);
    }

    /// The case a fixed threshold gets wrong: a heading and body text on one
    /// page. Each line's own running median has to govern its own grouping.
    #[test]
    fn a_heading_and_body_text_do_not_merge_or_split() {
        let mut comps = vec![c(1, 10, 10, 20, 30), c(2, 40, 10, 20, 30), c(3, 70, 10, 20, 30)];
        for k in 0..6u32 {
            comps.push(c(10 + k, 10 + k * 12, 60, 8, 10));
        }
        for k in 0..6u32 {
            comps.push(c(20 + k, 10 + k * 12, 80, 8, 10));
        }
        let lines = group(&comps, 300, 200);
        assert_eq!(lines.len(), 3, "expected heading plus two body lines");
        assert_eq!(lines[0].members.len(), 3);
        assert_eq!(lines[0].median_height, 30);
        assert_eq!(lines[1].median_height, 10);
    }

    /// An `i`-dot sits above its line's x-height band and must still land on
    /// that line, not seed one of its own.
    #[test]
    fn dots_and_accents_join_the_line_below_them() {
        let mut comps = vec![];
        for k in 0..4u32 {
            comps.push(c(1 + k, 10 + k * 12, 20, 8, 10)); // x-height bodies
        }
        comps.push(c(9, 12, 14, 2, 2)); // the dot of an i, above the band
        comps.push(c(10, 60, 28, 3, 3)); // a comma, at the baseline
        let lines = group(&comps, 200, 100);
        assert_eq!(lines.len(), 1, "a dot must not become a line");
        assert_eq!(lines[0].members.len(), 6);
    }

    /// The baseline is the row below flat-bottomed ink, and the x-height band
    /// is the modal top edge when a taller band sits above it.
    #[test]
    fn a_mixed_case_line_reads_its_own_x_height() {
        // Baseline at y = 40. Caps 20 tall (top 20), lowercase 14 (top 26).
        let mut comps = vec![c(1, 10, 20, 12, 20)];
        for k in 0..5u32 {
            comps.push(c(2 + k, 30 + k * 14, 26, 10, 14));
        }
        let lines = group(&comps, 200, 100);
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        assert_eq!(l.baseline, 40.0);
        assert_eq!(l.x_height_source, XHeightSource::Observed);
        assert_eq!(l.x_height, 14.0);
        assert_eq!(l.cap_height, 20.0);
    }

    /// An all-caps line has no x-height band to read, so it converts its cap
    /// band and says so.
    #[test]
    fn an_all_caps_line_converts_its_cap_band_and_labels_it() {
        let mut comps = vec![];
        for k in 0..6u32 {
            comps.push(c(1 + k, 10 + k * 16, 20, 12, 20));
        }
        let lines = group(&comps, 200, 100);
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        assert_eq!(l.x_height_source, XHeightSource::FromCapHeight);
        assert_eq!(l.cap_height, 20.0);
        assert!((l.x_height - 20.0 * X_HEIGHT_PER_CAP).abs() < 1e-4);
    }

    /// One band plus a descender is lowercase, not capitals — the descender
    /// settles it, so the band is read as the x-height it is.
    #[test]
    fn a_descender_proves_a_single_band_is_lowercase() {
        // Baseline 40, bodies 14 tall, one of them hanging 6 below.
        let mut comps = vec![];
        for k in 0..5u32 {
            comps.push(c(1 + k, 10 + k * 14, 26, 10, 14));
        }
        comps.push(c(9, 80, 26, 10, 20)); // a 'p': body at x-height, tail below
        let lines = group(&comps, 200, 100);
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        assert_eq!(l.x_height_source, XHeightSource::Observed);
        assert_eq!(l.x_height, 14.0);
    }

    /// A dot-leader run outnumbers the text it sits beside, so a count-based
    /// histogram calls the dots the x-height band and the line is lost --
    /// not just the leaders, the words as well. Weighting by width asks
    /// which population owns the line's extent instead.
    #[test]
    fn a_leader_run_does_not_take_the_x_height_band_from_the_text() {
        let mut comps = Vec::new();
        // Ten letters, 8 wide and 10 tall, sitting on a baseline at y=20.
        for k in 0..10u32 {
            comps.push(c(k + 1, 10 + k * 10, 10, 8, 10));
        }
        // Twenty leader dots, 2 by 2, on the same baseline. They outnumber
        // the letters two to one and are outweighed by width four to one.
        for k in 0..20u32 {
            comps.push(c(100 + k, 120 + k * 6, 18, 2, 2));
        }
        let p = Params { column_lone_guard: true, ..Params::default() };
        let lines = group_with(&comps, 400, 100, &p);
        assert_eq!(lines.len(), 1, "one line, not a line of text and a line of dots");
        let l = &lines[0];
        assert!(
            l.x_height >= 5.0,
            "x-height {} came from the dots, not the letters",
            l.x_height
        );
    }

    /// A line that is *only* leader dots has no x-height of its own at any
    /// weighting, and the page above it does. Its baseline is correct and
    /// must not move; only the x-height is inherited.
    #[test]
    fn a_line_of_only_leader_dots_inherits_the_pages_x_height() {
        let mut comps = Vec::new();
        // A line of text with an observed x-height: ascenders at 14 and
        // x-height letters at 10, all on a baseline at y=20.
        for k in 0..8u32 {
            comps.push(c(k + 1, 10 + k * 12, 10, 8, 10));
        }
        for k in 0..4u32 {
            comps.push(c(50 + k, 110 + k * 12, 6, 8, 14));
        }
        // A line of nothing but dots, far enough below not to be folded in.
        for k in 0..30u32 {
            comps.push(c(100 + k, 10 + k * 6, 48, 2, 2));
        }
        let p = Params { column_lone_guard: true, ..Params::default() };
        let lines = group_with(&comps, 400, 100, &p);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0].x_height_source, XHeightSource::Observed);
        let dots = &lines[1];
        assert_eq!(dots.x_height_source, XHeightSource::Inherited);
        assert_eq!(dots.x_height, lines[0].x_height);
        assert_eq!(dots.baseline, 50.0, "the baseline was right and must not move");
    }

    /// The guard on the inheritance rule: a page with no line that observed
    /// an x-height has nothing to lend, and must change nothing.
    #[test]
    fn a_page_with_no_observed_line_inherits_nothing() {
        let comps: Vec<Component> = (0..30u32).map(|k| c(k + 1, 10 + k * 6, 48, 2, 2)).collect();
        let p = Params { column_lone_guard: true, ..Params::default() };
        let lines = group_with(&comps, 400, 100, &p);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].x_height_source, XHeightSource::FromCapHeight);
    }

    /// The second tier: small type and all-caps setting produce pages where
    /// no line observes a distinct x-height band, and a leader line on such a
    /// page must still be repaired. Two converted text lines outvote one
    /// converted leader line.
    #[test]
    fn a_page_with_no_observed_line_votes_with_every_line_instead() {
        let mut comps = Vec::new();
        for (row, y) in [(0u32, 10u32), (1, 90)] {
            for k in 0..12u32 {
                comps.push(c(row * 100 + k + 1, 10 + k * 12, y, 8, 10));
            }
        }
        for k in 0..30u32 {
            comps.push(c(500 + k, 10 + k * 6, 58, 2, 2));
        }
        let p = Params { column_lone_guard: true, ..Params::default() };
        let lines = group_with(&comps, 400, 200, &p);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines.iter().all(|l| l.x_height_source != XHeightSource::Observed));
        assert_eq!(lines[1].x_height_source, XHeightSource::Inherited);
        // Within a pixel, not equal: the page value is a median of whole-pixel
        // x-heights, and the text lines' own value is a cap-height conversion
        // that lands between two pixels.
        assert!(
            (lines[1].x_height - lines[0].x_height).abs() <= 1.0,
            "{} vs {}",
            lines[1].x_height,
            lines[0].x_height
        );
    }

    /// The no-op that makes the second tier safe: a page that is nothing but
    /// leaders elects the leader height, so no line is below the floor and
    /// nothing is inherited. A wrong answer confirmed by its own page is the
    /// failure this guards.
    #[test]
    fn a_page_of_nothing_but_leaders_changes_nothing() {
        let mut comps = Vec::new();
        for (row, y) in [(0u32, 20u32), (1, 60)] {
            for k in 0..30u32 {
                comps.push(c(row * 100 + k + 1, 10 + k * 6, y, 2, 2));
            }
        }
        let p = Params { column_lone_guard: true, ..Params::default() };
        let lines = group_with(&comps, 400, 200, &p);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l.x_height_source == XHeightSource::FromCapHeight));
    }

    /// The failure this exists for: banding is horizontal, so two boxes on
    /// a form share a row and land in one line whose metrics fit neither.
    #[test]
    fn two_boxes_on_the_same_row_are_two_lines() {
        let mut comps = Vec::new();
        // A 6 pt field label on the left.
        for k in 0..8u32 {
            comps.push(c(k + 1, 10 + k * 7, 40, 5, 6));
        }
        // A 14 pt heading in the next box, far to the right.
        for k in 0..5u32 {
            comps.push(c(100 + k, 300 + k * 16, 36, 12, 14));
        }
        let lines = group(&comps, 600, 200);
        assert_eq!(lines.len(), 2, "{lines:?}");
        // Left to right, which is the order the row is read in. The two
        // halves do not share a top edge -- the heading is the taller type
        // and starts higher -- so ordering the halves on their own top edge
        // would return this row backwards, and does so on every
        // label-and-value row of a real form.
        assert_eq!(lines[0].members.len(), 8, "the field label");
        assert_eq!(lines[1].members.len(), 5, "the heading");
        assert!(
            lines[1].x_height > lines[0].x_height,
            "each half must be measured on its own: {} vs {}",
            lines[1].x_height,
            lines[0].x_height
        );
    }

    /// The guard: a tabbed column in ordinary set text is not a box
    /// boundary, and cutting there would turn one line into four.
    #[test]
    fn a_tabbed_column_is_not_a_column_gap() {
        let mut comps = Vec::new();
        for col in 0..4u32 {
            for k in 0..4u32 {
                // Columns 60 px apart, glyphs 8 px wide on a 12 px pitch and
                // 10 px tall: a gap of 16 px, 1.6 median heights, under the
                // 1.75 cut.
                comps.push(c(col * 10 + k + 1, col * 60 + k * 12, 20, 8, 10));
            }
        }
        let lines = group(&comps, 600, 100);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].members.len(), 16);
    }

    /// A leader line's median component height is the height of a period, so
    /// an unweighted cut threshold would fall below a word space and dice the
    /// line. The weighting is what stops that.
    #[test]
    fn a_leader_line_is_not_cut_at_every_dot() {
        let mut comps = Vec::new();
        for k in 0..10u32 {
            comps.push(c(k + 1, 10 + k * 12, 20, 8, 10));
        }
        for k in 0..30u32 {
            comps.push(c(100 + k, 140 + k * 9, 28, 2, 2));
        }
        let p = Params { column_lone_guard: true, ..Params::default() };
        let lines = group_with(&comps, 600, 100, &p);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].members.len(), 40);
    }

    /// A run of lone glyphs, each one flanked by gaps wide enough to cut,
    /// must not fracture: every candidate between them borders a singleton on
    /// both sides, so every candidate is dropped and the whole run stays one
    /// band (`ARCHITECTURE.md` section 6/11, 2026-09-23).
    #[test]
    fn a_run_of_lone_glyphs_across_wide_gaps_stays_one_band() {
        let mut comps = Vec::new();
        // Three singletons, 8x10, each 20 px from the last -- 2.0 median
        // heights, clear of the 1.75 cut -- with nothing beside any of them.
        for k in 0..3u32 {
            comps.push(c(k + 1, 10 + k * 28, 20, 8, 10));
        }
        let p = Params { column_lone_guard: true, ..Params::default() };
        let lines = group_with(&comps, 400, 100, &p);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0].members.len(), 3);
    }

    /// A lone glyph beside a multi-glyph fragment, across the same wide gap,
    /// is still cut off: only both sides being singletons drops the
    /// candidate, and a `$` in its own table cell is a column
    /// (`ARCHITECTURE.md` section 6/11, 2026-09-23).
    #[test]
    fn a_lone_glyph_beside_a_multi_glyph_fragment_is_still_cut() {
        let mut comps = Vec::new();
        // Three glyphs packed into one fragment, then a singleton 20 px
        // beyond it -- 2.0 median heights, clear of the 1.75 cut.
        for k in 0..3u32 {
            comps.push(c(k + 1, 10 + k * 12, 20, 8, 10));
        }
        comps.push(c(50, 62, 20, 8, 10));
        let p = Params { column_lone_guard: true, ..Params::default() };
        let lines = group_with(&comps, 400, 100, &p);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0].members.len(), 3, "the multi-glyph fragment");
        assert_eq!(lines[1].members.len(), 1, "the singleton, cut off");
    }

    /// With the guard off -- the shipped default -- a leader-only band is the
    /// plain greedy cut and fractures at every dot, exactly like cutting at
    /// every candidate (`ARCHITECTURE.md` section 11, 2026-09-23).
    #[test]
    fn with_the_guard_off_a_leader_only_band_is_cut_at_every_dot() {
        let mut comps = Vec::new();
        // Five dots, 2x2 on a 6 px pitch: a 4 px gap against a 3.5 px cut
        // (1.75 * the only height present, 2), so every gap is a candidate.
        for k in 0..5u32 {
            comps.push(c(k + 1, 10 + k * 6, 20, 2, 2));
        }
        let p = Params { column_lone_guard: false, ..Params::default() };
        let lines = group_with(&comps, 200, 100, &p);
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert!(lines.iter().all(|l| l.members.len() == 1));
    }

    /// A table rule spans the page and is not a glyph; a lone pixel is
    /// scanner noise. Neither may drag a line's metrics around.
    #[test]
    fn page_furniture_and_speckle_are_left_out() {
        let mut comps = vec![c(1, 0, 50, 280, 2), c(2, 150, 5, 1, 1)];
        for k in 0..4u32 {
            comps.push(c(10 + k, 10 + k * 12, 20, 8, 10));
        }
        let lines = group(&comps, 300, 200);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].members.len(), 4);
    }

    #[test]
    fn an_empty_page_has_no_lines() {
        assert!(group(&[], 100, 100).is_empty());
    }

    /// Nothing about the answer may depend on the order the labeller happened
    /// to emit components in.
    #[test]
    fn grouping_is_deterministic_under_input_order() {
        let mut comps = vec![];
        for k in 0..8u32 {
            comps.push(c(1 + k, 10 + k * 12, 20, 8, 10));
            comps.push(c(20 + k, 10 + k * 12, 45, 8, 14));
        }
        let reversed: Vec<Component> = comps.iter().rev().copied().collect();
        let a = group(&comps, 200, 100);
        let b = group(&reversed, 200, 100);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!((x.y0, x.y1, x.baseline, x.x_height), (y.y0, y.y1, y.baseline, y.x_height));
            let mut xs: Vec<u32> = x.members.iter().map(|&i| comps[i].label).collect();
            let mut ys: Vec<u32> = y.members.iter().map(|&i| reversed[i].label).collect();
            xs.sort_unstable();
            ys.sort_unstable();
            assert_eq!(xs, ys);
        }
    }

    /// Builds a [`Band`] holding every component in `comps`, in insertion
    /// order, member indices `0..comps.len()`.
    fn band_of(comps: &[Component]) -> Band {
        let mut band = Band::new(0, &comps[0]);
        for i in 1..comps.len() {
            band.absorb(i, &comps[i]);
        }
        band
    }

    // -- Rule A: the plausibility floor on `Observed` --------------------

    /// The traced shape from `docs/measurements/2026-09-23_line_merge_mechanism.md`:
    /// a band merging two lines, whose absorbed second line's tops pile up at
    /// zero and pull `main` to the floor of `1`, while `upper` still carries
    /// the first line's genuine tops at 13 and 17. Before the floor this
    /// reported `Observed` at x-height 1; after it, `main` (1) is nowhere
    /// near `x_height_floor_per_cap * cap` (0.3168 * 13 = 4.12), so the
    /// cap-band branch must not fire.
    #[test]
    fn the_traced_merged_line_shape_is_not_observed_as_a_cap_band() {
        let mut comps = Vec::new();
        // Line 1: baseline 370. Five lowercase members (top 13) and two caps
        // (top 17), giving `upper` its real, genuine population.
        for k in 0..5u32 {
            comps.push(c(1 + k, 10 + k * 15, 357, 10, 13)); // y1 = 370
        }
        for k in 0..2u32 {
            comps.push(c(10 + k, 200 + k * 15, 353, 10, 17)); // y1 = 370
        }
        // Line 2: wrongly absorbed, sitting entirely below line 1's baseline
        // so every member's top clamps to zero. Lighter total width than
        // line 1's 70, so the baseline mode still lands on 370.
        for k in 0..5u32 {
            comps.push(c(20 + k, 300 + k * 15, 375, 10, 18)); // y1 = 393
        }
        let band = band_of(&comps);
        let line = measure(band, &comps, &Params::default());
        assert_eq!(line.baseline, 370.0, "baseline must still land on line 1");
        assert_ne!(
            line.x_height_source,
            XHeightSource::Observed,
            "an implausible cap band (main={}, cap~13) must not be Observed",
            line.x_height
        );
    }

    /// The floor must not disturb any line that already had a plausible
    /// cap band: mixed-case, all-caps and a condensed face's tighter ratio
    /// all still read `Observed`.
    #[test]
    fn ordinary_lines_keep_observed_under_the_floor() {
        // Mixed-case, same shape as `a_mixed_case_line_reads_its_own_x_height`.
        let mut mixed = vec![c(1, 10, 20, 12, 20)];
        for k in 0..5u32 {
            mixed.push(c(2 + k, 30 + k * 14, 26, 10, 14));
        }
        let l = &group(&mixed, 200, 100)[0];
        assert_eq!(l.x_height_source, XHeightSource::Observed);

        // A condensed face's ratio: cap 20, x-height 13 (0.65, tighter than
        // the median 0.7431 and still well clear of the 0.3168 floor).
        let mut condensed = vec![c(1, 10, 20, 12, 20)];
        for k in 0..5u32 {
            condensed.push(c(2 + k, 30 + k * 14, 27, 10, 13));
        }
        let l = &group(&condensed, 200, 100)[0];
        assert_eq!(l.x_height_source, XHeightSource::Observed);
        assert_eq!(l.x_height, 13.0);
        assert_eq!(l.cap_height, 20.0);

        // All-caps keeps its own (unrelated) branch, `FromCapHeight`.
        let mut caps = Vec::new();
        for k in 0..6u32 {
            caps.push(c(1 + k, 10 + k * 16, 20, 12, 20));
        }
        let l = &group(&caps, 200, 100)[0];
        assert_eq!(l.x_height_source, XHeightSource::FromCapHeight);
    }

    // -- Rule B: the two-baseline split -----------------------------------

    /// Two tightly-leaded lines banded as one must split back into two, in
    /// top-to-bottom order.
    #[test]
    fn two_tight_lines_split_on_two_baselines() {
        let mut comps = Vec::new();
        for k in 0..5u32 {
            comps.push(c(1 + k, 10 + k * 15, 22, 10, 18)); // y1 = 40
        }
        for k in 0..5u32 {
            comps.push(c(10 + k, 10 + k * 15, 46, 10, 18)); // y1 = 64
        }
        let band = band_of(&comps);
        let out = split_baselines(band, &comps, &Params::default());
        assert_eq!(out.len(), 2, "expected a split into two bands");
        assert!(out[0].y1 < out[1].y1, "upper band must come first");
        assert_eq!(out[0].members.len(), 5);
        assert_eq!(out[1].members.len(), 5);
    }

    /// Three tightly-leaded lines banded as one must split into three,
    /// recursively, in top-to-bottom order.
    #[test]
    fn three_tight_lines_split_on_two_baselines_recursively() {
        let mut comps = Vec::new();
        for (row, y0) in [(0u32, 22u32), (1, 46), (2, 70)] {
            for k in 0..5u32 {
                comps.push(c(row * 10 + k + 1, 10 + k * 15, y0, 10, 18));
            }
        }
        let band = band_of(&comps);
        let out = split_baselines(band, &comps, &Params::default());
        assert_eq!(out.len(), 3, "expected a split into three bands");
        assert!(out[0].y1 < out[1].y1 && out[1].y1 < out[2].y1);
        for b in &out {
            assert_eq!(b.members.len(), 5);
        }
    }

    /// A mixed-case line with a descender must not split: the descender's
    /// baseline sits only 6px below the line's real baseline, well inside
    /// `baseline_split_sep`'s separation floor (0.6 * the 14px median body
    /// height = 8.4px).
    #[test]
    fn a_mixed_case_line_with_a_descender_does_not_split() {
        let mut comps = Vec::new();
        for k in 0..5u32 {
            comps.push(c(1 + k, 10 + k * 15, 26, 10, 14)); // lowercase, y1 = 40
        }
        for k in 0..2u32 {
            comps.push(c(10 + k, 200 + k * 15, 20, 10, 20)); // caps, y1 = 40
        }
        comps.push(c(20, 300, 26, 10, 20)); // a descender ('p'), y1 = 46
        let band = band_of(&comps);
        let out = split_baselines(band, &comps, &Params::default());
        assert_eq!(out.len(), 1, "a descender must not read as a second line");
    }

    /// A superscript or footnote marker sits far enough above the line to
    /// clear the separation gate, but must not split: its support (one
    /// narrow glyph) falls well short of `baseline_split_support` (0.25) of
    /// the main line's.
    #[test]
    fn a_superscript_marker_does_not_split() {
        let mut comps = Vec::new();
        for k in 0..8u32 {
            comps.push(c(1 + k, 10 + k * 15, 22, 10, 18)); // main line, y1 = 40
        }
        comps.push(c(20, 300, 10, 4, 10)); // a raised marker, y1 = 20
        let band = band_of(&comps);
        let out = split_baselines(band, &comps, &Params::default());
        assert_eq!(out.len(), 1, "a lone light marker must not read as a second line");
    }

    /// A line of digits beside tall punctuation (`$ ( ) |`) all sit on one
    /// baseline; there is only one `y1` value in the population, so there is
    /// nothing to split.
    #[test]
    fn digits_with_tall_punctuation_do_not_split() {
        let mut comps = Vec::new();
        for k in 0..10u32 {
            comps.push(c(1 + k, 10 + k * 12, 26, 8, 14)); // digits, y1 = 40
        }
        for k in 0..4u32 {
            comps.push(c(20 + k, 200 + k * 10, 16, 6, 24)); // tall punctuation, y1 = 40
        }
        let band = band_of(&comps);
        let out = split_baselines(band, &comps, &Params::default());
        assert_eq!(out.len(), 1, "one shared baseline must not split");
    }

    // -- `strip_underlines` -----------------------------------------------

    /// Turns `#`/`.` rows into a row-major mask, `(mask, width, height)`.
    fn mask_of(rows: &[&str]) -> (Vec<u8>, u32, u32) {
        let w = rows[0].len() as u32;
        let h = rows.len() as u32;
        let mut m = Vec::with_capacity((w * h) as usize);
        for r in rows {
            assert_eq!(r.len() as u32, w, "every row must be the same width");
            m.extend(r.chars().map(|ch| u8::from(ch == '#')));
        }
        (m, w, h)
    }

    fn strip_params(rule_run_heights: f32) -> Params {
        Params { min_area: 1, furniture_fraction: 1.0, rule_run_heights, underline_strip: true, ..Params::default() }
    }

    /// An underlined word: three one-pixel-wide strokes fused into one
    /// component by a full-width rule touching their feet. The rule is
    /// erased everywhere except where a stroke touches it directly.
    ///
    /// The rule is drawn 2 rows deep, not 1, per the 2026-09-23 second rule
    /// ("Bands, not rows"): a single-row run is never erased, so a 1-row
    /// rule fixture would no longer exercise this pass at all.
    ///
    /// Premise changed from the first rule's version of this test: item 3
    /// ("Keep crossing strokes") measures each touching column's own
    /// vertical ink run *through* the band, which may reach above or below
    /// it -- and a letter stroke that touches the rule with no vertical gap
    /// produces one continuous run for its whole height plus the rule's, far
    /// taller than the band's own thickness, so that column is preserved
    /// whole rather than split at the band boundary. The three letters no
    /// longer recover their original 8px height; each keeps its own 8 rows
    /// plus the 2 rule rows fused to its foot (10), and only the columns
    /// between the letters -- where the rule's ink stands alone -- are
    /// erased. This is the same mechanism, not a special case, that
    /// preserves a `)`'s bowl in
    /// `a_crossing_stroke_keeps_its_pixels_through_the_band` below.
    #[test]
    fn an_underlined_word_splits_into_letters_with_the_line_gone() {
        let (mut mask, w, h) = mask_of(&[
            "..............................",
            "..............................",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            ".....#.......#.......#........",
            "...####################.......",
            "...####################.......",
            "..............................",
        ]);
        // Single component on the page, so the median glyphish height this
        // measures against is that component's own height (10): run_floor =
        // 1.0 * 10 = 10, comfortably below the rule's 20px run and
        // comfortably above every 1px-wide letter stroke.
        let stripped = strip_underlines(&mut mask, w, h, &strip_params(1.0));
        let comps = stripped.components;
        assert_eq!(comps.len(), 3, "the rule must be gone and the three strokes separate");
        let mut x0s: Vec<u32> = comps.iter().map(|c| c.x0).collect();
        x0s.sort_unstable();
        assert_eq!(x0s, vec![5, 13, 21]);
        for c in &comps {
            assert_eq!(c.height(), 10, "a touching stroke keeps the rule rows fused to its own foot");
        }
        // Rows 10 and 11: erased everywhere except the three columns where a
        // letter's own stroke touches the rule directly.
        for y in [10u32, 11] {
            let row = &mask[(y * w) as usize..((y + 1) * w) as usize];
            for (x, &b) in row.iter().enumerate() {
                if [5usize, 13, 21].contains(&x) {
                    assert_ne!(b, 0, "a touching stroke's own column keeps its ink at row {y}, col {x}");
                } else {
                    assert_eq!(b, 0, "the rule alone must be gone at row {y}, col {x}");
                }
            }
        }
        assert_eq!(stripped.rule_segments.len(), 1, "one band erased");
        let seg = stripped.rule_segments[0];
        assert_eq!((seg.x0, seg.x1, seg.y0, seg.y1), (3, 23, 10, 12), "the band's own extent is recorded");
    }

    /// A long em dash and a separate row of hyphens, on a page whose other
    /// components establish a realistic median height. Both are narrower
    /// than the run floor and must never be examined at all.
    #[test]
    fn an_em_dash_and_a_row_of_hyphens_are_untouched() {
        let mut rows: Vec<String> = vec![".".repeat(80); 14];
        // Five 8px-tall letter blocks establish the page's median glyphish
        // height.
        for k in 0..5u32 {
            let x0 = (2 + k * 10) as usize;
            for y in 2..10usize {
                rows[y].replace_range(x0..x0 + 6, "######");
            }
        }
        // The em dash: one 20px-wide, 2px-tall component, well under
        // run_floor = 5.4209 * 8 = 43.4.
        for y in 12..14usize {
            rows[y].replace_range(2..22, &"#".repeat(20));
        }
        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (mut mask, w, h) = mask_of(&rows_ref);
        let before = mask.clone();
        let p = strip_params(Params::default().rule_run_heights.max(5.4209));
        let stripped = strip_underlines(&mut mask, w, h, &p);
        assert_eq!(mask, before, "nothing under the run floor may be touched");
        // 5 letter blocks + 1 dash, none merged or altered.
        assert_eq!(stripped.components.len(), 6);
        assert!(
            stripped.components.iter().any(|c| c.width() == 20 && c.height() == 2),
            "the dash must survive whole"
        );
        assert!(stripped.rule_segments.is_empty(), "nothing examined, nothing erased");
    }

    /// A table rule crossing a row of digit-like blocks: the rule is erased
    /// between the digits, and each digit still ends up its own component at
    /// its original x-position. Drawn 2 rows deep, same reason as the
    /// underlined-word fixture above.
    ///
    /// Premise changed from the first rule's version of this test, the same
    /// way and for the same reason as
    /// `an_underlined_word_splits_into_letters_with_the_line_gone` above:
    /// these digit blocks are solid rectangles touching the rule with no
    /// vertical gap, so every one of a digit's own columns reads as a
    /// crossing stroke under item 3 and keeps the two rule rows fused to its
    /// foot. Each digit comes back 10px tall, not 8; only the rule pixels
    /// strictly between digits, where no glyph column touches them, are
    /// erased.
    #[test]
    fn a_table_rule_touching_digits_recovers_them() {
        let mut rows: Vec<String> = vec![".".repeat(60); 12];
        for k in 0..4u32 {
            let x0 = (4 + k * 12) as usize;
            for y in 1..9usize {
                rows[y].replace_range(x0..x0 + 4, "####");
            }
        }
        // A rule spanning every digit, touching their feet at rows 9-10.
        rows[9] = format!(".{}.", "#".repeat(58));
        rows[10] = rows[9].clone();
        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (mut mask, w, h) = mask_of(&rows_ref);
        let stripped = strip_underlines(&mut mask, w, h, &strip_params(1.0));
        let comps = stripped.components;
        assert_eq!(comps.len(), 4, "all four digits recovered, the rule gone between them");
        let mut x0s: Vec<u32> = comps.iter().map(|c| c.x0).collect();
        x0s.sort_unstable();
        assert_eq!(x0s, vec![4, 16, 28, 40]);
        for c in &comps {
            assert_eq!((c.width(), c.height()), (4, 10), "each digit keeps its own columns fused to the rule rows beneath them");
        }
        assert_eq!(stripped.rule_segments.len(), 1, "one band erased");
        let seg = stripped.rule_segments[0];
        assert_eq!((seg.x0, seg.x1, seg.y0, seg.y1), (1, 59, 9, 11), "the band's own extent is recorded");
    }

    /// A lone row that matches a real rule's own x-range, but never gets a
    /// second consecutive qualifying row of its own, must not be erased --
    /// even though a genuine 2-row band right next to it is.
    /// `docs/measurements/2026-09-23_underline_strip_damage.md` section 3
    /// found exactly this shape: a single problematic row sitting between
    /// the two lines of a double-underline convention.
    #[test]
    fn a_single_row_between_a_double_rule_is_not_erased() {
        let mut rows: Vec<String> = vec![".".repeat(50); 5];
        // The real band: two matching rows.
        for y in [0usize, 1] {
            rows[y].replace_range(5..45, &"#".repeat(40));
        }
        // Row 2 stays blank, disconnecting the lone row below from the band
        // above -- the same effect the observed 5-disjoint-run row had:
        // whatever comes after it cannot extend the band above.
        // A lone row matching the band's own x-range, with nothing before
        // or after it to make a second row.
        rows[3].replace_range(5..45, &"#".repeat(40));
        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (mut mask, w, h) = mask_of(&rows_ref);
        let before_row3 = mask[(3 * w) as usize..(4 * w) as usize].to_vec();
        let stripped = strip_underlines(&mut mask, w, h, &strip_params(1.0));
        let after_row3 = mask[(3 * w) as usize..(4 * w) as usize].to_vec();
        assert_eq!(after_row3, before_row3, "a lone qualifying row is never erased");
        for y in [0u32, 1] {
            let row = &mask[(y * w) as usize..((y + 1) * w) as usize];
            assert!(row.iter().all(|&b| b == 0), "the genuine 2-row band must still be erased");
        }
        assert_eq!(stripped.rule_segments.len(), 1, "only the real band is recorded");
        let seg = stripped.rule_segments[0];
        assert_eq!((seg.x0, seg.x1, seg.y0, seg.y1), (5, 45, 0, 2));
    }

    /// A vertical stroke crossing a rule -- a `)`'s bowl running the whole
    /// height of the examined component -- must keep every pixel where it
    /// crosses the band, even though the band is erased everywhere else
    /// (`ARCHITECTURE.md` section 11, 2026-09-23, "Keep crossing strokes";
    /// the `$(83)`/`$(328)` finding in
    /// `docs/measurements/2026-09-23_underline_strip_damage.md` section 3).
    #[test]
    fn a_crossing_stroke_keeps_its_pixels_through_the_band() {
        let w = 30usize;
        let hgt = 9usize;
        let mut rows: Vec<String> = vec![".".repeat(w); hgt];
        // The rule: two full-width rows.
        for y in [3usize, 4] {
            rows[y] = "#".repeat(w);
        }
        // A stroke crossing the whole component, 3px wide, columns 10..13.
        for row in rows.iter_mut() {
            row.replace_range(10..13, "###");
        }
        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (mut mask, wu, hu) = mask_of(&rows_ref);
        let stripped = strip_underlines(&mut mask, wu, hu, &strip_params(1.0));
        for y in [3u32, 4] {
            let row = &mask[(y * wu) as usize..((y + 1) * wu) as usize];
            for x in 0..wu as usize {
                let ink = row[x] != 0;
                if (10..13).contains(&x) {
                    assert!(ink, "the crossing stroke keeps its pixels at row {y}, col {x}");
                } else {
                    assert!(!ink, "the rule itself must be erased at row {y}, col {x}");
                }
            }
        }
        assert_eq!(stripped.rule_segments.len(), 1, "the band is recorded even though a stroke crosses it");
    }

    /// A box whose top edge is a real 2-row band: erasing it leaves the
    /// box's own vertical sides behind as new tall/narrow pieces, because
    /// each side is a crossing stroke far taller than the band's own
    /// thickness. With `debris_heights` set, no such piece may survive
    /// (`ARCHITECTURE.md` section 11, 2026-09-23, "Drop strip debris";
    /// `docs/measurements/2026-09-23_underline_strip_damage.md` sections 3-4,
    /// the `POST-STRIP-LARGEST` finding).
    #[test]
    fn a_box_top_edge_leaves_no_debris_taller_than_the_bound() {
        let w = 140usize;
        let hgt = 95usize;
        let mut rows: Vec<String> = vec![".".repeat(w); hgt];

        // Five 8px-tall letter blocks set the page's median glyphish height.
        for k in 0..5u32 {
            let x0 = (2 + k * 10) as usize;
            for y in 2..10usize {
                rows[y].replace_range(x0..x0 + 6, "######");
            }
        }

        // A box: a 2-row top edge and two full-height vertical sides, far
        // taller than the letters.
        for y in [2usize, 3] {
            rows[y].replace_range(100..130, &"#".repeat(30));
        }
        for y in 2..90usize {
            rows[y].replace_range(100..101, "#");
            rows[y].replace_range(129..130, "#");
        }

        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (mut mask, wu, hu) = mask_of(&rows_ref);
        let p = Params {
            min_area: 1,
            furniture_fraction: 1.0,
            rule_run_heights: 1.0,
            debris_heights: 5.0,
            underline_strip: true,
            ..Params::default()
        };
        let stripped = strip_underlines(&mut mask, wu, hu, &p);

        let debris_floor = 5.0 * 8.0; // debris_heights * the median height h = 8
        assert!(
            stripped.components.iter().all(|c| f64::from(c.height()) <= debris_floor),
            "no post-strip piece may survive taller than the debris bound"
        );
        assert_eq!(stripped.components.len(), 5, "only the five letters survive; the box is fully consumed");
        assert!(stripped.components.iter().all(|c| c.x0 < 100), "nothing from the box's region survives");
        assert_eq!(stripped.rule_segments.len(), 1, "the box's top edge is recorded as one band");
        let seg = stripped.rule_segments[0];
        assert_eq!((seg.x0, seg.x1, seg.y0, seg.y1), (100, 130, 2, 4));
    }

    /// Every erased band is recorded as a rule segment on the strip's
    /// output, independent of whatever downstream consumes it
    /// (`ARCHITECTURE.md` section 11, 2026-09-23, "Keep what was stripped").
    /// Two independent bands on the same page must each produce their own
    /// segment.
    #[test]
    fn rule_segments_are_recorded() {
        let mut rows: Vec<String> = vec![".".repeat(60); 20];
        // Five 8px-tall letter blocks establish the page's median glyphish
        // height.
        for k in 0..5u32 {
            let x0 = (2 + k * 10) as usize;
            for y in 2..10usize {
                rows[y].replace_range(x0..x0 + 6, "######");
            }
        }
        // Two independent 2-row rule bands, in different components, at
        // different rows and x-ranges.
        for y in [12usize, 13] {
            rows[y].replace_range(0..40, &"#".repeat(40));
        }
        for y in [16usize, 17] {
            rows[y].replace_range(10..58, &"#".repeat(48));
        }
        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (mut mask, w, h) = mask_of(&rows_ref);
        let stripped = strip_underlines(&mut mask, w, h, &strip_params(1.0));
        let mut segs = stripped.rule_segments.clone();
        segs.sort_by_key(|s| s.y0);
        assert_eq!(segs.len(), 2, "two independent bands must each be recorded");
        assert_eq!((segs[0].x0, segs[0].x1, segs[0].y0, segs[0].y1), (0, 40, 12, 14));
        assert_eq!((segs[1].x0, segs[1].x1, segs[1].y0, segs[1].y1), (10, 58, 16, 18));
    }

    /// `underline_strip: false` must be byte-for-byte the pre-2026-09-23
    /// behaviour: `strip_underlines` is never called, so a rule that would
    /// otherwise be stripped survives whole.
    #[test]
    fn the_toggle_off_leaves_the_default_pipeline_path_alone() {
        // `Params::default()` ships `underline_strip: false`, so a caller
        // that never calls `strip_underlines` at all -- the pipeline's own
        // branch -- is what this asserts about, not a property of
        // `strip_underlines` itself, which always strips when called
        // directly. Documented here because the pipeline branch is what
        // `underline_strip` actually gates.
        assert!(!Params::default().underline_strip);
    }
}
