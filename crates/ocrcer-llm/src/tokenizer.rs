//! Byte-level BPE tokenizer matching the Qwen/GPT-2 pre-tokenizer and merge
//! algorithm exactly, operating purely on bytes and token ids: the GPT-2
//! byte<->unicode remapping table that `tokenizer.json`'s string-keyed
//! vocabulary uses exists only at conversion time, in `ocrcer-build`. By the
//! time a `.ocrl` file is loaded, every vocabulary entry has already been
//! resolved to the id its raw bytes decode to, and every merge to
//! `(left_id, right_id) -> (rank, result_id)`, so this module never decodes
//! a vocabulary string.
//!
//! # Pre-tokenizer, reimplemented without a regex engine
//!
//! Qwen (like GPT-2) splits text before BPE with:
//!
//! ```text
//! (?i:'s|'t|'re|'ve|'m|'ll|'d)|[^\r\n\p{L}\p{N}]?\p{L}+|\p{N}
//!   | ?[^\s\p{L}\p{N}]+[\r\n]*|\s*[\r\n]+|\s+(?!\S)|\s+
//! ```
//!
//! `\s+(?!\S)` needs lookahead, which the standard library has no engine
//! for, so `next_token_end` hand-traces what each alternative's backtracking
//! does instead of running the pattern. On a whitespace run `[i, k)`
//! (`k` = first non-whitespace position or end of input):
//!
//! - if the run contains a CR/LF, the match is `[i, p+1)` where `p` is the
//!   *rightmost* CR/LF in the run — greedy `\s*` gives back only as many
//!   trailing characters as `[\r\n]+` needs, and since it needs just one,
//!   giving back to the last CR/LF is the first (and only) backtrack that
//!   succeeds;
//! - otherwise, at end of input the whole run matches (`(?!\S)` is
//!   vacuously true at end of input); otherwise `\s+(?!\S)` matches all but
//!   the last character (leaving it for the next word's leading-space
//!   branch), unless that would go below the `+` minimum of one character,
//!   in which case the lookahead-free `\s+` fallback takes the single
//!   character instead.

use crate::container::{Container, Error};
use std::collections::HashMap;

pub struct Tokenizer {
    byte_id: [u32; 256],
    merges: HashMap<(u32, u32), (u32, u32)>, // (left, right) -> (rank, result)
    specials: Vec<(String, u32)>,
    pub vocab_size: usize,
}

impl Tokenizer {
    pub fn from_container(c: &Container) -> Result<Tokenizer, Error> {
        let byte_id_vec = c.need("tok.byte_id")?.u32s()?;
        if byte_id_vec.len() != 256 {
            return Err(Error::BadTable { name: "tok.byte_id".into(), why: "must have exactly 256 entries" });
        }
        let mut byte_id = [0u32; 256];
        byte_id.copy_from_slice(&byte_id_vec);

        let left = c.need("tok.merge_left")?.u32s()?;
        let right = c.need("tok.merge_right")?.u32s()?;
        let result = c.need("tok.merge_result")?.u32s()?;
        if left.len() != right.len() || left.len() != result.len() {
            return Err(Error::BadTable { name: "tok.merge_left".into(), why: "merge tables have mismatched lengths" });
        }
        let mut merges = HashMap::with_capacity(left.len());
        for (rank, ((&l, &r), &res)) in left.iter().zip(right.iter()).zip(result.iter()).enumerate() {
            merges.insert((l, r), (rank as u32, res));
        }

        let specials = read_specials(c)?;
        let vocab_size = c
            .meta
            .get("config")
            .and_then(|cfg| cfg.get("vocab_size"))
            .and_then(crate::json::Json::as_u32)
            .ok_or(Error::BadTable { name: "meta.config".into(), why: "missing config.vocab_size" })? as usize;

        Ok(Tokenizer { byte_id, merges, specials, vocab_size })
    }

    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut out = Vec::new();
        let mut pos = 0usize;
        while pos < text.len() {
            match self.match_special(&text[pos..]) {
                Some((id, len)) => {
                    out.push(id);
                    pos += len;
                }
                None => {
                    let end = self.next_special_at_or_after(text, pos);
                    for word in pretokenize(&text[pos..end]) {
                        self.bpe_encode(word, &mut out);
                    }
                    pos = end;
                }
            }
        }
        out
    }

    fn match_special(&self, s: &str) -> Option<(u32, usize)> {
        self.specials
            .iter()
            .filter(|(tok, _)| s.starts_with(tok.as_str()))
            .map(|(tok, id)| (*id, tok.len()))
            .max_by_key(|&(_, len)| len)
    }

    /// The end of the plain-text run starting at `from`: the next byte
    /// offset (after `from`) at which a special token begins, or the end of
    /// the string if none occurs again.
    fn next_special_at_or_after(&self, text: &str, from: usize) -> usize {
        if self.specials.is_empty() {
            return text.len();
        }
        for (off, _) in text[from..].char_indices().skip(1) {
            if self.match_special(&text[from + off..]).is_some() {
                return from + off;
            }
        }
        text.len()
    }

    /// Encodes one pre-tokenizer word (no special tokens, no further
    /// splitting) by repeatedly merging the lowest-rank adjacent pair present
    /// anywhere in it, merging every occurrence of that exact pair per pass —
    /// the standard BPE encode loop.
    fn bpe_encode(&self, word: &str, out: &mut Vec<u32>) {
        let mut ids: Vec<u32> = word.bytes().map(|b| self.byte_id[b as usize]).collect();
        while ids.len() > 1 {
            let mut best: Option<(u32, u32, u32)> = None; // (rank, left, right)
            for pair in ids.windows(2) {
                if let Some(&(rank, _)) = self.merges.get(&(pair[0], pair[1])) {
                    if best.is_none_or(|(best_rank, ..)| rank < best_rank) {
                        best = Some((rank, pair[0], pair[1]));
                    }
                }
            }
            let Some((_, left, right)) = best else { break };
            let (_, result) = self.merges[&(left, right)];
            let mut next = Vec::with_capacity(ids.len());
            let mut i = 0;
            while i < ids.len() {
                if i + 1 < ids.len() && ids[i] == left && ids[i + 1] == right {
                    next.push(result);
                    i += 2;
                } else {
                    next.push(ids[i]);
                    i += 1;
                }
            }
            ids = next;
        }
        out.extend(ids);
    }
}

fn read_specials(c: &Container) -> Result<Vec<(String, u32)>, Error> {
    let mut specials = Vec::new();
    if let Some(list) = c.meta.get("special_tokens").and_then(crate::json::Json::as_array) {
        for entry in list {
            let text = entry
                .get("text")
                .and_then(crate::json::Json::as_str)
                .ok_or(Error::BadTable { name: "meta.special_tokens".into(), why: "entry missing text" })?;
            let id = entry
                .get("id")
                .and_then(crate::json::Json::as_u32)
                .ok_or(Error::BadTable { name: "meta.special_tokens".into(), why: "entry missing id" })?;
            specials.push((text.to_string(), id));
        }
    }
    Ok(specials)
}

fn is_cr_lf(c: char) -> bool {
    c == '\r' || c == '\n'
}

fn match_contraction(chars: &[(usize, char)], i: usize) -> Option<usize> {
    let get = |k: usize| chars.get(k).map(|&(_, c)| c);
    if let Some(c1) = get(i + 1) {
        if matches!(c1.to_ascii_lowercase(), 's' | 't' | 'm' | 'd') {
            return Some(i + 2);
        }
    }
    if let (Some(c1), Some(c2)) = (get(i + 1), get(i + 2)) {
        let pair = (c1.to_ascii_lowercase(), c2.to_ascii_lowercase());
        if pair == ('r', 'e') || pair == ('v', 'e') || pair == ('l', 'l') {
            return Some(i + 3);
        }
    }
    None
}

/// Index (into `chars`) one past the token that starts at `chars[i]`.
fn next_token_end(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    let c = chars[i].1;

    if c == '\'' {
        if let Some(end) = match_contraction(chars, i) {
            return end;
        }
    }

    // `[^\r\n\p{L}\p{N}]? \p{L}+`
    {
        let mut j = i;
        if !is_cr_lf(c) && !c.is_alphabetic() && !c.is_numeric() && i + 1 < n && chars[i + 1].1.is_alphabetic() {
            j = i + 1;
        }
        if chars[j].1.is_alphabetic() {
            let mut k = j;
            while k < n && chars[k].1.is_alphabetic() {
                k += 1;
            }
            return k;
        }
    }

    // `\p{N}` — exactly one.
    if c.is_numeric() {
        return i + 1;
    }

    // `" ?[^\s\p{L}\p{N}]+[\r\n]*"`
    {
        let mut j = i;
        if c == ' ' && i + 1 < n {
            let d = chars[i + 1].1;
            if !d.is_whitespace() && !d.is_alphabetic() && !d.is_numeric() {
                j = i + 1;
            }
        }
        let start = chars[j].1;
        if !start.is_whitespace() && !start.is_alphabetic() && !start.is_numeric() {
            let mut k = j;
            while k < n {
                let d = chars[k].1;
                if d.is_whitespace() || d.is_alphabetic() || d.is_numeric() {
                    break;
                }
                k += 1;
            }
            while k < n && is_cr_lf(chars[k].1) {
                k += 1;
            }
            return k;
        }
    }

    // Everything reaching here is whitespace (branches 1-4 above cover every
    // letter, digit, and punctuation/symbol start; see the module doc).
    debug_assert!(c.is_whitespace());
    let mut k = i;
    while k < n && chars[k].1.is_whitespace() {
        k += 1;
    }
    if let Some(p) = (i..k).rev().find(|&p| is_cr_lf(chars[p].1)) {
        return p + 1;
    }
    if k == n || k - i < 2 {
        k
    } else {
        k - 1
    }
}

fn pretokenize(s: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < n {
        let end = next_token_end(&chars, i);
        let start_byte = chars[i].0;
        let end_byte = if end < n { chars[end].0 } else { s.len() };
        out.push(&s[start_byte..end_byte]);
        i = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn splits(s: &str) -> Vec<&str> {
        pretokenize(s)
    }

    #[test]
    fn a_plain_sentence_keeps_the_leading_space_with_each_word() {
        assert_eq!(splits("the cat sat"), vec!["the", " cat", " sat"]);
    }

    #[test]
    fn a_space_before_a_digit_run_is_its_own_token() {
        assert_eq!(splits("the 5"), vec!["the", " ", "5"]);
    }

    #[test]
    fn digits_split_one_at_a_time_before_merges() {
        assert_eq!(splits("12"), vec!["1", "2"]);
    }

    #[test]
    fn a_contraction_is_its_own_token() {
        assert_eq!(splits("don't"), vec!["don", "'t"]);
    }

    #[test]
    fn punctuation_run_attaches_a_leading_space() {
        assert_eq!(splits("a (b"), vec!["a", " (", "b"]);
    }

    #[test]
    fn a_run_of_newlines_is_grouped_and_kept_from_the_next_word() {
        assert_eq!(splits("a\n\nb"), vec!["a", "\n\n", "b"]);
    }

    #[test]
    fn trailing_whitespace_at_end_of_string_is_one_token() {
        assert_eq!(splits("a   "), vec!["a", "   "]);
    }

    #[test]
    fn a_synthetic_tiny_vocabulary_encodes_by_merge_rank() {
        // Vocabulary: bytes 0..256 as ids 0..256, plus "th" -> 256 (rank 0),
        // then "th"+"e" -> "the" -> 257 (rank 1).
        let mut byte_id = [0u32; 256];
        for (b, id) in byte_id.iter_mut().enumerate() {
            *id = b as u32;
        }
        let t_id = u32::from(b't');
        let h_id = u32::from(b'h');
        let e_id = u32::from(b'e');
        let mut merges = HashMap::new();
        merges.insert((t_id, h_id), (0u32, 256u32));
        merges.insert((256u32, e_id), (1u32, 257u32));
        let tok = Tokenizer { byte_id, merges, specials: Vec::new(), vocab_size: 258 };
        let mut out = Vec::new();
        tok.bpe_encode("the", &mut out);
        assert_eq!(out, vec![257]);
    }

    #[test]
    fn special_tokens_are_pulled_out_before_pretokenization() {
        let mut byte_id = [0u32; 256];
        for (b, id) in byte_id.iter_mut().enumerate() {
            *id = b as u32;
        }
        let tok = Tokenizer {
            byte_id,
            merges: HashMap::new(),
            specials: vec![("<|endoftext|>".to_string(), 999)],
            vocab_size: 1000,
        };
        let ids = tok.encode("a<|endoftext|>b");
        assert_eq!(ids, vec![u32::from(b'a'), 999, u32::from(b'b')]);
    }
}
