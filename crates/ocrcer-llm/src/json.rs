//! A minimal JSON reader for `.ocrl`'s `meta` block.
//!
//! Deliberately a near-duplicate of `ocrcer-core::json`, not a shared
//! dependency on it: `ocrcer-llm` is specified (`ARCHITECTURE.md` section 11,
//! 2026-09-24) as depending on nothing but `std`, including not on
//! `ocrcer-core`, so that pdfcer can enable the LLM add-on without pulling in
//! the OCR engine's own dependency graph. A hand-rolled JSON *value* reader is
//! infrastructure, not one of the pipeline stages `CLAUDE.md` rule 4 guards
//! against duplicating (feature extraction, matching, decoding) — there is no
//! risk of the two silently drifting apart on what a class or a feature means,
//! because neither reads the other's model file.
//!
//! Accepts RFC 8259 text. Numbers are parsed as `f64`, sufficient for every
//! field `meta` carries here (layer counts, dimensions, RoPE theta, epsilon).

pub const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub at: usize,
    pub what: &'static str,
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at byte {}", self.what, self.at)
    }
}

impl Json {
    pub fn parse(text: &str) -> Result<Json, JsonError> {
        let mut p = Parser { b: text.as_bytes(), at: 0, depth: 0 };
        p.ws();
        let v = p.value()?;
        p.ws();
        if p.at != p.b.len() {
            return Err(p.err("trailing data after value"));
        }
        Ok(v)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        let n = self.as_f64()?;
        if n.fract() == 0.0 && n >= 0.0 && n <= f64::from(u32::MAX) {
            Some(n as u32)
        } else {
            None
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Obj(v) => Some(v),
            _ => None,
        }
    }
}

struct Parser<'a> {
    b: &'a [u8],
    at: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, what: &'static str) -> JsonError {
        JsonError { at: self.at, what }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.at).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn lit(&mut self, word: &[u8]) -> bool {
        if self.b[self.at..].starts_with(word) {
            self.at += word.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Json, JsonError> {
        if self.depth >= MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        match self.peek() {
            None => Err(self.err("unexpected end of input")),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') if self.lit(b"true") => Ok(Json::Bool(true)),
            Some(b'f') if self.lit(b"false") => Ok(Json::Bool(false)),
            Some(b'n') if self.lit(b"null") => Ok(Json::Null),
            _ => self.number(),
        }
    }

    fn object(&mut self) -> Result<Json, JsonError> {
        self.at += 1;
        self.depth += 1;
        let mut fields = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.at += 1;
            self.depth -= 1;
            return Ok(Json::Obj(fields));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a key"));
            }
            let k = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'"));
            }
            self.at += 1;
            self.ws();
            fields.push((k, self.value()?));
            self.ws();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(Json::Obj(fields));
                }
                _ => return Err(self.err("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self) -> Result<Json, JsonError> {
        self.at += 1;
        self.depth += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.at += 1;
            self.depth -= 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    self.depth -= 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(self.err("expected ',' or ']'")),
            }
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or_else(|| self.err("unterminated string"))?;
            match c {
                b'"' => {
                    self.at += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.at += 1;
                    let e = self.peek().ok_or_else(|| self.err("unterminated escape"))?;
                    self.at += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return Err(self.err("unknown escape")),
                    }
                }
                c if c < 0x20 => return Err(self.err("raw control character in string")),
                _ => {
                    let len = utf8_len(c);
                    let end = self.at + len;
                    if end > self.b.len() {
                        return Err(self.err("truncated UTF-8 sequence"));
                    }
                    match std::str::from_utf8(&self.b[self.at..end]) {
                        Ok(s) => out.push_str(s),
                        Err(_) => return Err(self.err("invalid UTF-8 in string")),
                    }
                    self.at = end;
                }
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let hi = self.hex4()?;
        if (0xD800..0xDC00).contains(&hi) {
            if self.b[self.at..].starts_with(b"\\u") {
                let save = self.at;
                self.at += 2;
                let lo = self.hex4()?;
                if (0xDC00..0xE000).contains(&lo) {
                    let cp = 0x1_0000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                    return Ok(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                }
                self.at = save;
            }
            return Ok('\u{FFFD}');
        }
        Ok(char::from_u32(hi).unwrap_or('\u{FFFD}'))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        if self.at + 4 > self.b.len() {
            return Err(self.err("truncated \\u escape"));
        }
        let mut v = 0u32;
        for i in 0..4 {
            let d = self.b[self.at + i];
            let n = match d {
                b'0'..=b'9' => u32::from(d - b'0'),
                b'a'..=b'f' => u32::from(d - b'a') + 10,
                b'A'..=b'F' => u32::from(d - b'A') + 10,
                _ => return Err(self.err("bad hex digit in \\u escape")),
            };
            v = v * 16 + n;
        }
        self.at += 4;
        Ok(v)
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        if self.at == start {
            return Err(self.err("expected a value"));
        }
        let text = std::str::from_utf8(&self.b[start..self.at]).map_err(|_| self.err("bad number"))?;
        text.parse::<f64>().map(Json::Num).map_err(|_| JsonError { at: start, what: "bad number" })
    }
}

fn utf8_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_config_shaped_object() {
        let v = Json::parse(r#"{"n_layers":28,"theta":1000000.0,"tie":true,"name":"Qwen3-0.6B"}"#).unwrap();
        assert_eq!(v.get("n_layers").unwrap().as_u32(), Some(28));
        assert_eq!(v.get("theta").unwrap().as_f64(), Some(1_000_000.0));
        assert_eq!(v.get("tie").unwrap().as_bool(), Some(true));
        assert_eq!(v.get("name").unwrap().as_str(), Some("Qwen3-0.6B"));
    }

    #[test]
    fn malformed_input_is_refused() {
        for bad in ["{", "{\"a\"}", "[1,]", "tru", "\"x"] {
            assert!(Json::parse(bad).is_err());
        }
    }
}
