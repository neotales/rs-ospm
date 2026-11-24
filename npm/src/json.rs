//! Minimal JSON reader for npm command output.
//!
//! npm reports its global package tree and search results as JSON. This crate
//! keeps the dependency-free build of the other process providers, so this
//! module parses the JSON itself. It accepts only valid JSON, rejects input
//! above [`MAX_INPUT`], and limits nesting depth. Objects keep their source
//! order, which is enough for the fields the provider reads.

/// Largest npm output this parser accepts.
pub(crate) const MAX_INPUT: usize = 16 * 1024 * 1024;

/// Deepest JSON nesting this parser accepts.
const MAX_DEPTH: usize = 64;

/// A parsed JSON value.
///
/// Numbers keep no value because npm's package entries only need strings.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool,
    /// Any valid JSON number.
    Number,
    /// A string with escapes decoded.
    String(String),
    /// An array in source order.
    Array(Vec<Json>),
    /// An object in source order. Duplicate keys are kept; [`Json::get`]
    /// returns the last value for a key.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Parses one JSON value and rejects trailing non-whitespace.
    pub(crate) fn parse(input: &[u8]) -> Result<Self, JsonError> {
        if input.len() > MAX_INPUT {
            return Err(JsonError);
        }
        let text = std::str::from_utf8(input).map_err(|_| JsonError)?;
        let mut parser = Parser {
            text,
            bytes: text.as_bytes(),
            position: 0,
        };
        parser.skip_whitespace();
        let value = parser.value(0)?;
        parser.skip_whitespace();
        if parser.position != parser.bytes.len() {
            return Err(JsonError);
        }
        Ok(value)
    }

    /// Returns the last value for `key` when this is an object.
    pub(crate) fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .rev()
                .find_map(|(name, value)| (name == key).then_some(value)),
            _ => None,
        }
    }

    /// Returns the string when this is a JSON string.
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the elements when this is a JSON array.
    pub(crate) fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    /// Returns the entries when this is a JSON object.
    pub(crate) fn as_object(&self) -> Option<&[(String, Self)]> {
        match self {
            Self::Object(entries) => Some(entries),
            _ => None,
        }
    }

    /// Returns whether this is `null`.
    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

/// JSON input did not match the accepted grammar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct JsonError;

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        Some(byte)
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.position += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return Err(JsonError);
        }
        self.skip_whitespace();
        match self.peek().ok_or(JsonError)? {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => Ok(Json::String(self.string()?)),
            b't' => {
                self.literal(b"true")?;
                Ok(Json::Bool)
            }
            b'f' => {
                self.literal(b"false")?;
                Ok(Json::Bool)
            }
            b'n' => {
                self.literal(b"null")?;
                Ok(Json::Null)
            }
            b'-' | b'0'..=b'9' => {
                self.number()?;
                Ok(Json::Number)
            }
            _ => Err(JsonError),
        }
    }

    fn literal(&mut self, literal: &[u8]) -> Result<(), JsonError> {
        if self.bytes[self.position..].starts_with(literal) {
            self.position += literal.len();
            Ok(())
        } else {
            Err(JsonError)
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.position += 1;
        self.skip_whitespace();
        let mut entries = Vec::new();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return Err(JsonError);
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.bump() != Some(b':') {
                return Err(JsonError);
            }
            let value = self.value(depth + 1)?;
            entries.push((key, value));
            self.skip_whitespace();
            match self.bump() {
                Some(b',') => continue,
                Some(b'}') => return Ok(Json::Object(entries)),
                _ => return Err(JsonError),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.position += 1;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(Json::Array(values));
        }
        loop {
            values.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.bump() {
                Some(b',') => self.skip_whitespace(),
                Some(b']') => return Ok(Json::Array(values)),
                _ => return Err(JsonError),
            }
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.position += 1;
        let mut output = String::new();
        loop {
            let byte = self.bump().ok_or(JsonError)?;
            match byte {
                b'"' => return Ok(output),
                b'\\' => self.escape(&mut output)?,
                0x00..=0x1f => return Err(JsonError),
                0x20..=0x7e => output.push(char::from(byte)),
                _ => {
                    self.position -= 1;
                    let character = self.text[self.position..].chars().next().ok_or(JsonError)?;
                    output.push(character);
                    self.position += character.len_utf8();
                }
            }
        }
    }

    fn escape(&mut self, output: &mut String) -> Result<(), JsonError> {
        match self.bump().ok_or(JsonError)? {
            b'"' => output.push('"'),
            b'\\' => output.push('\\'),
            b'/' => output.push('/'),
            b'b' => output.push('\u{0008}'),
            b'f' => output.push('\u{000c}'),
            b'n' => output.push('\n'),
            b'r' => output.push('\r'),
            b't' => output.push('\t'),
            b'u' => output.push(self.unicode_escape()?),
            _ => return Err(JsonError),
        }
        Ok(())
    }

    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let first = self.hex4()?;
        match first {
            0xd800..=0xdbff => {
                if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                    return Err(JsonError);
                }
                let second = self.hex4()?;
                if !(0xdc00..=0xdfff).contains(&second) {
                    return Err(JsonError);
                }
                let code =
                    0x1_0000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(second) - 0xdc00);
                char::from_u32(code).ok_or(JsonError)
            }
            0xdc00..=0xdfff => Err(JsonError),
            _ => char::from_u32(u32::from(first)).ok_or(JsonError),
        }
    }

    fn hex4(&mut self) -> Result<u16, JsonError> {
        let mut value = 0u16;
        for _ in 0..4 {
            let byte = self.bump().ok_or(JsonError)?;
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err(JsonError),
            };
            value = value * 16 + u16::from(digit);
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<(), JsonError> {
        if self.peek() == Some(b'-') {
            self.position += 1;
        }
        match self.bump() {
            Some(b'0') => {
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(JsonError);
                }
            }
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.position += 1;
                }
            }
            _ => return Err(JsonError),
        }
        if self.peek() == Some(b'.') {
            self.position += 1;
            if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                return Err(JsonError);
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.position += 1;
            }
            if !self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                return Err(JsonError);
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Json, JsonError};

    #[test]
    fn parses_nested_values() {
        let value = Json::parse(
            br#"{"name":"lib","dependencies":{"eslint":{"version":"9.0.0"},"@types/node":{"version":"22.0.0"}},"list":[1,true,null,-1.5e2]}"#,
        )
        .unwrap();

        let dependencies = value.get("dependencies").unwrap().as_object().unwrap();
        assert_eq!(dependencies.len(), 2);
        assert_eq!(
            value
                .get("dependencies")
                .unwrap()
                .get("@types/node")
                .unwrap()
                .get("version")
                .unwrap()
                .as_str(),
            Some("22.0.0")
        );
        assert_eq!(value.get("list").unwrap().as_array().unwrap().len(), 4);
        assert!(value.get("missing").is_none());
        assert!(value.get("list").unwrap().get("x").is_none());
    }

    #[test]
    fn decodes_escapes_and_surrogate_pairs() {
        let value = Json::parse(br#""a\"b\\c\/d\b\f\n\r\t\u00e9\ud83d\ude00""#).unwrap();
        assert_eq!(
            value.as_str(),
            Some("a\"b\\c/d\u{0008}\u{000c}\n\r\t\u{00e9}\u{1f600}")
        );
    }

    #[test]
    fn duplicate_keys_return_the_last_value() {
        let value = Json::parse(br#"{"a":"first","a":"second"}"#).unwrap();
        assert_eq!(value.get("a").unwrap().as_str(), Some("second"));
    }

    #[test]
    fn accepts_top_level_scalars() {
        assert_eq!(Json::parse(b"null").unwrap(), Json::Null);
        assert_eq!(Json::parse(b"true").unwrap(), Json::Bool);
        assert_eq!(Json::parse(b"42").unwrap(), Json::Number);
        assert_eq!(
            Json::parse(b"\"text\"").unwrap(),
            Json::String("text".to_owned())
        );
    }

    #[test]
    fn rejects_invalid_documents() {
        for input in [
            &b""[..],
            b"{}trailing",
            b"{",
            b"[1,]",
            b"{\"a\":}",
            b"\"unterminated",
            b"\"line\nbreak\"",
            b"\"\\x\"",
            b"01",
            b"1.",
            b"+1",
            b"tru",
            b"\xff",
        ] {
            assert!(
                Json::parse(input).is_err(),
                "input should be rejected: {input:?}"
            );
        }
    }

    #[test]
    fn rejects_lone_surrogates() {
        assert_eq!(Json::parse(br#""\ud800""#).unwrap_err(), JsonError);
        assert_eq!(Json::parse(br#""\udc00""#).unwrap_err(), JsonError);
    }

    #[test]
    fn rejects_deep_nesting() {
        let mut input = String::new();
        for _ in 0..(super::MAX_DEPTH + 2) {
            input.push('[');
        }
        for _ in 0..(super::MAX_DEPTH + 2) {
            input.push(']');
        }
        assert!(Json::parse(input.as_bytes()).is_err());
    }
}
