//! Character and word error rate, and the whitespace normalisation both
//! engines' output is scored under.
//!
//! # The scoring contract
//!
//! CER is Levenshtein distance between the read text and the reference,
//! divided by the reference length in characters. WER is the same over
//! whitespace-separated tokens. Both are error rates: lower is better, and
//! both can exceed 1.0, because an engine that inserts text can be more than
//! completely wrong.
//!
//! [`line_matched_score`] is a second, reading-order-independent CER: it
//! pairs truth lines to read lines before charging any edit distance, so a
//! table read in the wrong row order is scored on recognition, not on order.
//! It is printed beside [`score`], never in place of it. Forgiving order can
//! only ever remove error, so by design its CER should never read *higher*
//! than [`score`]'s on the same page. Two independent implementation bugs
//! broke that bound unconditionally — not as an approximation error but as a
//! flat miscount on every page they touched — and both have been found and
//! fixed: a greedy line-pairing step that had a fold pass for
//! many-truth-lines-to-one-read-line but not the mirror
//! one-truth-line-to-many-read-lines direction, and a denominator that
//! silently excluded the line-separator characters [`score`]'s own
//! denominator counts. See [`line_matched_score`]'s doc comment for both.
//!
//! What is *not* fixed, and is not a bug of the same kind: the line-pairing
//! itself is a greedy nearest-first match, not a proven-optimal one-to-one
//! assignment, so on a corpus with heavy multi-way splitting a suboptimal
//! pairing can still leave this CER a little above [`score`]'s — observed on
//! `finfilings`, where the gap narrowed sharply after the two fixes above but
//! did not fully close. That is the documented cost of "deliberately simple"
//! in [`line_matched_score`]'s doc comment, not a defect to chase inside this
//! pass.
//!
//! # Why the text is normalised first, and what that costs
//!
//! The corpus sets columns with runs of spaces. No recogniser reproduces a
//! run of spaces — there is nothing there to recognise — so scoring against
//! one measures layout reconstruction, not character recognition.
//! [`normalise`] therefore collapses every whitespace run to one space and
//! trims each line.
//!
//! This is applied identically to both engines and to the reference, so it
//! does not favour either. It does mean the comparison says nothing about
//! whether an engine preserved a table's shape. That is a real property of a
//! document engine and it is not being measured here; a later page-layout
//! measure is where it belongs.

use std::collections::BTreeMap;

/// Collapses whitespace runs to single spaces, trims each line, and drops
/// lines that are empty afterwards.
pub fn normalise(s: &str) -> String {
    s.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Levenshtein distance over any comparable sequence.
///
/// Two rows rather than a full matrix: the corpus has pages of a few hundred
/// characters and the full matrix would be fine, but the same function is
/// wanted for whole-document scoring later.
pub fn levenshtein<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ai) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, bj) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ai != bj);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        core::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// One page's score.
#[derive(Clone, Copy, Debug, Default)]
pub struct Score {
    pub char_errors: usize,
    pub chars: usize,
    pub word_errors: usize,
    pub words: usize,
}

impl Score {
    /// Character error rate. `None` when the reference is empty, which is
    /// not a rate of zero — it is a page there was nothing to get right on.
    pub fn cer(&self) -> Option<f64> {
        (self.chars > 0).then(|| self.char_errors as f64 / self.chars as f64)
    }

    pub fn wer(&self) -> Option<f64> {
        (self.words > 0).then(|| self.word_errors as f64 / self.words as f64)
    }

    pub fn add(&mut self, other: &Score) {
        self.char_errors += other.char_errors;
        self.chars += other.chars;
        self.word_errors += other.word_errors;
        self.words += other.words;
    }
}

/// Scores `read` against `reference`, normalising both.
pub fn score(reference: &str, read: &str) -> Score {
    let r = normalise(reference);
    let g = normalise(read);
    let rc: Vec<char> = r.chars().collect();
    let gc: Vec<char> = g.chars().collect();
    let rw: Vec<&str> = r.split_whitespace().collect();
    let gw: Vec<&str> = g.split_whitespace().collect();
    Score {
        char_errors: levenshtein(&rc, &gc),
        chars: rc.len(),
        word_errors: levenshtein(&rw, &gw),
        words: rw.len(),
    }
}

/// How many of the reference's words came back at all, ignoring where.
///
/// # Why a second metric, and why this one
///
/// A page of columns — an invoice with a description column and an amount
/// column — can be read correctly word for word and still come back
/// column-first rather than row-first. [`score`] rates that as a near-total
/// failure, because every line after the first is displaced. That is a true
/// statement about reading order and a badly misleading one about
/// recognition, and the two questions need separate answers.
///
/// A first attempt at this matched each reference line to its best-matching
/// read line and scored those pairs. It was measured and discarded: an
/// engine that *splits* one reference line into three, which is exactly what
/// a column-first read of a table does, is charged for two whole extra lines
/// and scores worse than it does in order. Line matching measures a muddle
/// of order and grouping, so it is not reported.
///
/// What is reported instead is the multiset of whitespace-separated tokens:
/// recall is the share of reference words that appear somewhere in the read,
/// precision the share of read words that belong. Layout is entirely absent
/// from it, by construction. A word is a match only if every character of it
/// is right, so this forgives nothing about recognition — `1,284.50` for
/// `1,204.50` is still a miss.
///
/// **Neither number alone is the answer.** [`score`] is the harsher and more
/// document-faithful one; this is the layout-free one. An engine handed the
/// line structure in advance — which is what the oracle-segmentation OCRcer
/// path is — loses nothing under either and so gains nothing here, while an
/// engine that had to find the structure itself gains whatever finding it
/// cost. Reporting both brackets the answer; reporting one picks a winner by
/// choosing a metric.
#[derive(Clone, Copy, Debug, Default)]
pub struct TokenScore {
    pub hits: usize,
    pub reference_words: usize,
    pub read_words: usize,
}

impl TokenScore {
    pub fn recall(&self) -> Option<f64> {
        (self.reference_words > 0).then(|| self.hits as f64 / self.reference_words as f64)
    }

    pub fn precision(&self) -> Option<f64> {
        (self.read_words > 0).then(|| self.hits as f64 / self.read_words as f64)
    }

    /// Harmonic mean of the two, so an engine cannot win by emitting
    /// everything or by emitting almost nothing.
    pub fn f1(&self) -> Option<f64> {
        let (r, p) = (self.recall()?, self.precision()?);
        (r + p > 0.0).then(|| 2.0 * r * p / (r + p))
    }

    pub fn add(&mut self, other: &TokenScore) {
        self.hits += other.hits;
        self.reference_words += other.reference_words;
        self.read_words += other.read_words;
    }
}

/// One page's line-matched score. See [`line_matched_score`].
#[derive(Clone, Copy, Debug, Default)]
pub struct LineScore {
    pub char_errors: usize,
    pub chars: usize,
}

impl LineScore {
    /// Same convention as [`Score::cer`]: `None` on an empty reference, not
    /// a rate of zero.
    pub fn cer(&self) -> Option<f64> {
        (self.chars > 0).then(|| self.char_errors as f64 / self.chars as f64)
    }

    pub fn add(&mut self, other: &LineScore) {
        self.char_errors += other.char_errors;
        self.chars += other.chars;
    }
}

/// Reading-order-independent character error rate: matches each truth line to
/// its closest read line before charging any edit distance, so a table read
/// in a different row order is no longer indistinguishable from one read
/// wrong. After Clausner, Pletschacher & Antonacopoulos, "Flexible character
/// accuracy measure for reading-order-independent evaluation" (Pattern
/// Recognition Letters 131, 2020) -- the idea only; nothing here is copied
/// from the paper or from any implementation of it.
///
/// # Matching
///
/// Every (truth line, read line) pair is scored by Levenshtein distance
/// normalised by the longer line's length, purely to rank candidate pairs --
/// the amount actually charged is the raw, unnormalised distance. Pairs are
/// claimed greedily, lowest normalised distance first, both sides removed
/// from the pool once claimed; a tie goes to the lowest truth index, then the
/// lowest read index, so the result does not depend on iteration order. A
/// truth line nothing claims is charged as a deletion of its own length; a
/// read line nothing claims is charged as an insertion of its own length --
/// unless one of the two folding passes below claims it first.
///
/// # One-to-many: split and merged lines
///
/// A truth source that is not the render layout -- `finfilings`'s HTML-
/// derived lines are the motivating case -- can carry a line the OCR places
/// on the same row as its neighbour, e.g. a `"(1)"` marker column merged onto
/// the row after it. Charging that as an unrelated deletion plus a prefixed
/// substitution overstates a single merge. Handled minimally: after the
/// greedy pass, each still-unmatched truth line is offered to whichever
/// adjacent, already-matched truth line's read partner it could extend --
/// only a direct neighbour at the current edge of that match, never a general
/// search over all lines -- and the merge is taken only if it is cheaper than
/// leaving the line as a deletion.
///
/// # Many-to-one the other way: a truth line the segmenter split
///
/// The mirror case is the one a page-layout segmenter actually produces: one
/// truth line emitted as two adjacent read lines (a wide line wrapped by the
/// line-grouping band height, not a real content boundary). Left unhandled
/// this was worse than not having the metric at all -- confirmed against
/// this crate's own `bench/pages-cov` and `finfilings` corpora, where
/// line-matched CER read *higher* than the order-dependent whole-document
/// CER it is supposed to lower-bound, exactly backwards for a metric whose
/// entire point is forgiving reading order. The greedy pass above claims at
/// most one read line per truth line; the truth line's other half then went
/// unclaimed and was charged as a full insertion, while the claimed half was
/// charged a large substitution against the *whole* truth line it only
/// partly matches -- often costing more than the couple of characters the
/// whole-document metric charges for a stray newline in what is otherwise a
/// flat character stream. Handled the same way as the truth-side case, mirror
/// image: each still-unclaimed read line is offered to whichever adjacent,
/// already-claimed read line's truth partner it could extend, taken only if
/// cheaper than leaving it as an insertion. Deliberately simple, same as the
/// truth-side pass; a real many-to-many alignment solver is out of scope for
/// a diagnostic metric.
pub fn line_matched_score(reference: &str, read: &str) -> LineScore {
    let r = normalise(reference);
    let g = normalise(read);
    let t: Vec<Vec<char>> = r.lines().map(|l| l.chars().collect()).collect();
    let o: Vec<Vec<char>> = g.lines().map(|l| l.chars().collect()).collect();

    if t.is_empty() {
        return LineScore { char_errors: o.iter().map(Vec::len).sum(), chars: 0 };
    }

    // Raw (unnormalised) distance is what gets charged; the normalised value
    // is only for ranking which pairs are claimed first.
    let mut raw = vec![vec![0usize; o.len()]; t.len()];
    let mut pairs: Vec<(f64, usize, usize)> = Vec::with_capacity(t.len() * o.len());
    for (i, ti) in t.iter().enumerate() {
        for (j, oj) in o.iter().enumerate() {
            let d = levenshtein(ti, oj);
            raw[i][j] = d;
            let denom = ti.len().max(oj.len()).max(1) as f64;
            pairs.push((d as f64 / denom, i, j));
        }
    }
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    let mut match_of_t: Vec<Option<usize>> = vec![None; t.len()];
    let mut used_o = vec![false; o.len()];
    for (_, i, j) in &pairs {
        if match_of_t[*i].is_none() && !used_o[*j] {
            match_of_t[*i] = Some(*j);
            used_o[*j] = true;
        }
    }

    // Per read-line group: which read index owns each truth line, the
    // truth-index span it currently covers, its accumulated merged text, and
    // the distance of that text against the read line. Seeded from the
    // 1:1 matches above; the loop below only ever grows a group by one truth
    // line at a time, at whichever edge is adjacent.
    let mut owner: Vec<Option<usize>> = vec![None; t.len()];
    let mut span: Vec<Option<(usize, usize)>> = vec![None; o.len()];
    let mut text: Vec<Vec<char>> = vec![Vec::new(); o.len()];
    let mut dist: Vec<usize> = vec![0; o.len()];
    for (i, m) in match_of_t.iter().enumerate() {
        if let Some(j) = *m {
            owner[i] = Some(j);
            span[j] = Some((i, i));
            text[j] = t[i].clone();
            dist[j] = raw[i][j];
        }
    }

    for i in 0..t.len() {
        if owner[i].is_some() {
            continue;
        }
        let deletion_cost = t[i].len() as isize;
        let mut best: Option<(isize, usize, Vec<char>, usize, (usize, usize))> = None;

        if i > 0 {
            if let Some(j) = owner[i - 1] {
                let (lo, hi) = span[j].expect("an owned group has a span");
                if hi == i - 1 {
                    let mut candidate = text[j].clone();
                    candidate.push(' ');
                    candidate.extend(t[i].iter().copied());
                    let d = levenshtein(&candidate, &o[j]);
                    best = Some((d as isize - dist[j] as isize, j, candidate, d, (lo, i)));
                }
            }
        }
        if i + 1 < t.len() {
            if let Some(j) = owner[i + 1] {
                let (lo, hi) = span[j].expect("an owned group has a span");
                if lo == i + 1 {
                    let mut candidate = t[i].clone();
                    candidate.push(' ');
                    candidate.extend(text[j].iter().copied());
                    let d = levenshtein(&candidate, &o[j]);
                    let marginal = d as isize - dist[j] as isize;
                    let better = match &best {
                        Some((bm, bj, ..)) => marginal < *bm || (marginal == *bm && j < *bj),
                        None => true,
                    };
                    if better {
                        best = Some((marginal, j, candidate, d, (i, hi)));
                    }
                }
            }
        }

        if let Some((marginal, j, candidate, d, new_span)) = best {
            if marginal < deletion_cost {
                owner[i] = Some(j);
                text[j] = candidate;
                dist[j] = d;
                span[j] = Some(new_span);
            }
        }
    }

    // Mirror image of the loop above: a read line the greedy pass left
    // unclaimed is offered to whichever adjacent, already-claimed read
    // line's *truth* partner it could extend -- the case of one truth line
    // the segmenter emitted as two read lines. `read_home[j]` names the
    // anchor read index whose group `j` currently belongs to (itself, once
    // claimed): a home that is updated as groups grow, exactly as `owner`
    // is above. `rspan`/`rtext` are the read-side counterparts of
    // `span`/`text`, which stay truth-indexed throughout and so already
    // reflect any truth-side merging finished above -- this pass runs after
    // that one so the truth text it compares against is final.
    //
    // A single ascending sweep only ever grows a group towards higher
    // indices, because it can only see an anchor that a lower-indexed
    // neighbour already resolved earlier in the same sweep -- an anchor to
    // the *right* of two or more consecutive unclaimed lines is invisible to
    // the leftmost of them until the line between has already folded in. So
    // the sweep repeats until nothing new folds, which converges in at most
    // `o.len()` passes since each successful fold strictly shrinks the
    // unclaimed count; a three-way split (rare, but a wide CAD dimension
    // line wrapped twice is not impossible) folds fully rather than only its
    // rightmost two-thirds.
    let mut read_home: Vec<Option<usize>> = (0..o.len()).map(|j| used_o[j].then_some(j)).collect();
    let mut rspan: Vec<Option<(usize, usize)>> =
        (0..o.len()).map(|j| used_o[j].then_some((j, j))).collect();
    let mut rtext: Vec<Vec<char>> = o.clone();

    loop {
        let mut changed = false;
        for j in 0..o.len() {
            if read_home[j].is_some() {
                continue;
            }
            let insertion_cost = o[j].len() as isize;
            let mut best: Option<(isize, usize, Vec<char>, usize, (usize, usize))> = None;

            if j > 0 {
                if let Some(anchor) = read_home[j - 1] {
                    let (lo, hi) = rspan[anchor].expect("a claimed read line has a span");
                    if hi == j - 1 {
                        let mut candidate = rtext[anchor].clone();
                        candidate.push(' ');
                        candidate.extend(o[j].iter().copied());
                        let d = levenshtein(&text[anchor], &candidate);
                        best =
                            Some((d as isize - dist[anchor] as isize, anchor, candidate, d, (lo, j)));
                    }
                }
            }
            if j + 1 < o.len() {
                if let Some(anchor) = read_home[j + 1] {
                    let (lo, hi) = rspan[anchor].expect("a claimed read line has a span");
                    if lo == j + 1 {
                        let mut candidate = o[j].clone();
                        candidate.push(' ');
                        candidate.extend(rtext[anchor].iter().copied());
                        let d = levenshtein(&text[anchor], &candidate);
                        let marginal = d as isize - dist[anchor] as isize;
                        let better = match &best {
                            Some((bm, ba, ..)) => marginal < *bm || (marginal == *bm && anchor < *ba),
                            None => true,
                        };
                        if better {
                            best = Some((marginal, anchor, candidate, d, (j, hi)));
                        }
                    }
                }
            }

            if let Some((marginal, anchor, candidate, d, new_span)) = best {
                if marginal < insertion_cost {
                    rtext[anchor] = candidate;
                    dist[anchor] = d;
                    rspan[anchor] = Some(new_span);
                    read_home[j] = Some(anchor);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let mut char_errors = 0usize;
    for (j, oj) in o.iter().enumerate() {
        char_errors += match read_home[j] {
            Some(anchor) if anchor == j => dist[anchor], // an anchor's group is charged once
            Some(_) => 0,                                // folded into another anchor's charge
            None => oj.len(),                            // nothing claimed it: an insertion
        };
    }
    for (i, ti) in t.iter().enumerate() {
        if owner[i].is_none() {
            char_errors += ti.len();
        }
    }
    // The denominator has to be the same quantity [`score`] divides by, or
    // the two printed CERs are not answers to the same question. `score`'s
    // reference length is `normalise(reference).chars().count()` -- which
    // counts the line-separator between every pair of lines as a character,
    // because it is one flat string. Summing bare line lengths here instead
    // silently dropped one character per truth-line boundary from the
    // denominator on every single multi-line page, inflating this metric's
    // percentage relative to `score`'s on every page with more than one
    // line, independent of anything a fold pass could fix -- unlike the
    // fold passes above, this was not occasional, it was universal. `r` is
    // exactly the same normalised string [`score`] would compute from this
    // same `reference` argument, so reusing its count rather than
    // recomputing it is what keeps the two guaranteed to agree.
    let chars = r.chars().count();

    LineScore { char_errors, chars }
}

/// Multiset token overlap between `reference` and `read`.
pub fn token_score(reference: &str, read: &str) -> TokenScore {
    let mut want: BTreeMap<&str, usize> = BTreeMap::new();
    let mut reference_words = 0usize;
    for w in reference.split_whitespace() {
        *want.entry(w).or_default() += 1;
        reference_words += 1;
    }
    let mut hits = 0usize;
    let mut read_words = 0usize;
    for w in read.split_whitespace() {
        read_words += 1;
        if let Some(n) = want.get_mut(w) {
            if *n > 0 {
                *n -= 1;
                hits += 1;
            }
        }
    }
    TokenScore {
        hits,
        reference_words,
        read_words,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_perfect_read_scores_zero() {
        let s = score("Total 875.75", "Total 875.75");
        assert_eq!(s.char_errors, 0);
        assert_eq!(s.word_errors, 0);
        assert_eq!(s.cer(), Some(0.0));
    }

    #[test]
    fn column_spacing_is_not_scored() {
        let s = score("Qty   Amount", "Qty Amount");
        assert_eq!(s.char_errors, 0);
    }

    /// One wrong digit in an amount is one character error, and the same
    /// substitution is one whole word error — the asymmetry that makes WER
    /// the harsher number on this corpus and worth reporting beside CER.
    #[test]
    fn one_wrong_digit_is_one_char_error_and_one_word_error() {
        let s = score("Charges 1,204.50", "Charges 1,284.50");
        assert_eq!(s.char_errors, 1);
        assert_eq!(s.word_errors, 1);
        assert_eq!(s.chars, 16);
        assert_eq!(s.words, 2);
    }

    /// An engine that invents text is worse than one that reads nothing, and
    /// the score has to be able to say so.
    #[test]
    fn inserted_text_can_push_the_rate_above_one() {
        let s = score("ab", "abcdefgh");
        assert!(s.cer().unwrap() > 1.0);
    }

    /// The whole reason the second metric exists: a table read column-first
    /// has every word right and every line in the wrong place, and one
    /// number cannot say both of those things.
    #[test]
    fn a_column_first_read_is_bad_in_order_and_perfect_word_for_word() {
        let reference = "Qty Amount
4 450.00
12 45.00";
        let read = "Qty
4
12
Amount
450.00
45.00";
        assert!(score(reference, read).cer().unwrap() > 0.2);
        let t = token_score(reference, read);
        assert_eq!(t.f1(), Some(1.0));
    }

    /// A word counts only if every character of it is right, so the
    /// layout-free metric forgives nothing about recognition.
    #[test]
    fn a_single_wrong_digit_loses_the_whole_word() {
        let t = token_score("Charges 1,204.50", "Charges 1,284.50");
        assert_eq!(t.hits, 1);
        assert_eq!(t.reference_words, 2);
    }

    /// Repeated words are a multiset, not a set: reading one `280.00` when
    /// the page has two is a half score, not a full one.
    #[test]
    fn a_repeated_word_has_to_be_read_as_many_times_as_it_appears() {
        let t = token_score("280.00 280.00", "280.00");
        assert_eq!(t.hits, 1);
        assert_eq!(t.recall(), Some(0.5));
        assert_eq!(t.precision(), Some(1.0));
    }

    /// Invented words cost precision, so an engine cannot buy recall by
    /// emitting everything it can think of.
    #[test]
    fn invented_words_cost_precision() {
        let t = token_score("one", "one fabricated");
        assert_eq!(t.recall(), Some(1.0));
        assert_eq!(t.precision(), Some(0.5));
    }

    #[test]
    fn an_empty_reference_has_no_rate_rather_than_a_perfect_one() {
        assert_eq!(score("", "spurious").cer(), None);
    }

    #[test]
    fn line_matched_identical_text_is_zero() {
        let s = line_matched_score("Alpha\nBeta gadget", "Alpha\nBeta gadget");
        assert_eq!(s.char_errors, 0);
        assert_eq!(s.cer(), Some(0.0));
    }

    /// The whole point of the metric: two truth lines read in swapped order
    /// cost nothing here, once each is paired with its actual match, while
    /// the ordinary whole-document CER still charges for the displacement.
    #[test]
    fn swapped_line_order_is_zero_here_but_not_on_the_ordinary_cer() {
        let reference = "Alpha widget\nBeta gadget";
        let read = "Beta gadget\nAlpha widget";
        assert_eq!(line_matched_score(reference, read).char_errors, 0);
        assert!(score(reference, read).cer().unwrap() > 0.0);
    }

    #[test]
    fn one_substituted_character_counts_as_one() {
        let s = line_matched_score("Total 875.75", "Total 875.85");
        assert_eq!(s.char_errors, 1);
    }

    /// The one-to-many case the doc comment names: a truth line the OCR
    /// merges onto the row after it. Left as two independent lines this
    /// would charge a 3-character deletion plus a 4-character substitution
    /// (7 errors); folding the marker into its neighbour's match finds the
    /// exact text the merged row actually contains, at zero cost.
    #[test]
    fn a_marker_line_merged_onto_the_next_row_is_folded_not_double_charged() {
        let s = line_matched_score("(1)\nSome description", "(1) Some description");
        assert_eq!(s.char_errors, 0);
    }

    #[test]
    fn an_unmatched_read_line_is_charged_as_an_insertion() {
        let s = line_matched_score("Alpha", "Alpha\nspurious");
        assert_eq!(s.char_errors, "spurious".len());
    }

    #[test]
    fn an_unmatched_truth_line_is_charged_as_a_deletion() {
        let s = line_matched_score("Alpha\nmissing", "Alpha");
        assert_eq!(s.char_errors, "missing".len());
        // +1: the separator between the two truth lines, which `score`'s own
        // denominator on the same reference also counts -- see
        // `the_denominator_matches_scores_reference_length` for why that +1
        // has to be there.
        assert_eq!(s.chars, "Alpha".len() + "missing".len() + 1);
    }

    /// The second, universal cause of the reading-order-independent metric
    /// reading higher than the ordinary one on real corpora: unlike the fold
    /// passes above, which only fire on an actual split or merge, this
    /// applies to every multi-line page whether or not anything went wrong.
    /// `score`'s reference length counts the newline between every pair of
    /// lines as a character, because it treats the normalised reference as
    /// one flat string; a denominator built by summing bare line lengths
    /// silently drops one character per truth-line boundary, so the two
    /// metrics must be dividing by the same number for the same reference or
    /// they are not comparable at all.
    #[test]
    fn the_denominator_matches_scores_reference_length() {
        let reference = "Alpha widget\nBeta gadget\nGamma";
        let read = "Alpha widget\nBeta gadget\nGamma";
        let expected = score(reference, read).chars;
        assert_eq!(line_matched_score(reference, read).chars, expected);
    }

    /// The mirror of `a_marker_line_merged_onto_the_next_row_is_folded_not_
    /// double_charged`: one truth line the OCR emitted as two read lines --
    /// a wide line wrapped by the line-grouping band height, which is what a
    /// real segmenter does, not a table-column artefact. Before the read-side
    /// fold pass existed, the greedy match claimed the first read fragment
    /// against the *whole* truth line (a large substitution), then charged
    /// the second fragment as a full, unrelated insertion on top -- so this
    /// case cost more here than reassembling the same two lines cost the
    /// whole-document CER, which is exactly backwards for a metric whose
    /// point is forgiving reading order rather than penalising it further.
    #[test]
    fn a_line_the_ocr_splits_across_two_read_lines_is_folded_not_double_charged() {
        let s = line_matched_score(
            "Some description continues",
            "Some description\ncontinues",
        );
        assert_eq!(s.char_errors, 0);
    }

    /// The regression this pass exists to close, run through both metrics at
    /// once: a truth line split across two read lines used to score *worse*
    /// under the reading-order-independent metric than under the ordinary,
    /// order-dependent one, on `bench/pages-cov` and `finfilings` alike. A
    /// metric that forgives reading order must never read higher than one
    /// that charges for it.
    #[test]
    fn a_split_line_no_longer_scores_worse_here_than_end_to_end() {
        let reference = "ABCDEFGH widget assembly complete";
        let read = "ABCDEFGH widget\nassembly complete";
        let e2e = score(reference, read).cer().unwrap();
        let lm = line_matched_score(reference, read).cer().unwrap();
        assert!(lm <= e2e, "line-matched ({lm}) exceeded end-to-end ({e2e})");
        assert_eq!(lm, 0.0);
    }

    /// A longer chain -- one truth line split into three read fragments --
    /// needs the sweep-to-convergence behaviour: the leftmost fragment's
    /// only neighbour is another unclaimed fragment, not the anchor itself,
    /// so it cannot fold until a second pass sees that the middle fragment
    /// joined on the first one.
    #[test]
    fn a_line_split_into_three_read_fragments_still_folds_to_zero() {
        let s = line_matched_score(
            "Alpha beta gamma delta",
            "Alpha\nbeta\ngamma delta",
        );
        assert_eq!(s.char_errors, 0);
    }
}
