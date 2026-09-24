//! The GPT-2 / Qwen byte-level BPE alphabet: a fixed bijection between the
//! 256 raw byte values and 256 chosen Unicode codepoints, used so that every
//! byte string is representable as printable text for a string-keyed BPE
//! vocabulary. This is the same table `tokenizers`/`transformers` build with
//! their `bytes_to_unicode()` (every printable Latin-1 byte maps to itself;
//! every other byte, including whitespace and control bytes, maps to a
//! codepoint starting at U+0100).
//!
//! This table exists **only at conversion time**: `ocrcer-llm`'s runtime
//! never sees vocabulary strings, only raw bytes and token ids, because the
//! converter does this decoding once and writes the resulting bytes into the
//! `.ocrl` tables directly (`tok.vocab_bytes`). A byte string round-trips
//! through this map and back to the same bytes for any valid vocabulary
//! entry, which is what lets the converter translate `tokenizer.json`'s
//! string-keyed vocabulary and merges into id-keyed tables with no
//! ambiguity.

use std::collections::HashMap;

pub struct ByteMap {
    byte_to_char: [char; 256],
    char_to_byte: HashMap<char, u8>,
}

impl ByteMap {
    pub fn new() -> ByteMap {
        let mut bytes: Vec<u16> = Vec::with_capacity(256);
        bytes.extend(b'!' as u16..=b'~' as u16);
        bytes.extend(0xA1u16..=0xACu16);
        bytes.extend(0xAEu16..=0xFFu16);
        let mut codepoints: Vec<u32> = bytes.iter().map(|&b| u32::from(b)).collect();

        let mut n = 0u32;
        for b in 0u16..256 {
            if !bytes.contains(&b) {
                bytes.push(b);
                codepoints.push(256 + n);
                n += 1;
            }
        }

        let mut byte_to_char = ['\0'; 256];
        let mut char_to_byte = HashMap::with_capacity(256);
        for (b, cp) in bytes.iter().zip(codepoints.iter()) {
            let c = char::from_u32(*cp).expect("byte-map codepoints are all valid scalar values");
            byte_to_char[*b as usize] = c;
            char_to_byte.insert(c, *b as u8);
        }
        ByteMap { byte_to_char, char_to_byte }
    }

    pub fn byte_to_char(&self, b: u8) -> char {
        self.byte_to_char[b as usize]
    }

    /// Decodes a byte-level BPE vocabulary string back to the raw bytes it
    /// represents. `None` if `s` contains a character outside this map's
    /// image, which means it was never produced by this byte-level encoding
    /// (a malformed or foreign vocabulary file).
    pub fn decode(&self, s: &str) -> Option<Vec<u8>> {
        s.chars().map(|c| self.char_to_byte.get(&c).copied()).collect()
    }
}

impl Default for ByteMap {
    fn default() -> ByteMap {
        ByteMap::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_byte_round_trips() {
        let m = ByteMap::new();
        for b in 0u16..256 {
            let b = b as u8;
            let c = m.byte_to_char(b);
            assert_eq!(m.decode(&c.to_string()), Some(vec![b]), "byte {b}");
        }
    }

    #[test]
    fn printable_ascii_is_the_identity() {
        let m = ByteMap::new();
        for b in b'!'..=b'~' {
            assert_eq!(m.byte_to_char(b), b as char, "byte {b}");
        }
    }

    #[test]
    fn a_multi_char_string_decodes_to_its_bytes_in_order() {
        let m = ByteMap::new();
        // 'Ġ' is the mapped form of the space byte (0x20) in this table.
        let space_char = m.byte_to_char(0x20);
        let s = format!("{space_char}the");
        assert_eq!(m.decode(&s), Some(b" the".to_vec()));
    }
}
