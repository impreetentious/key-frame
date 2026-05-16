//! A small, complete JSON reader and writer.
//!
//! The tools read vector files and write measurement receipts, and both have to
//! survive being edited by hand. Searching for a quoted key in a string finds
//! the first thing that looks like it, which is fine until a note happens to
//! contain the word it was looking for; this parses instead.
//!
//! It is deliberately small. There is no dependency here for the same reason
//! there is none anywhere else in the workspace, and JSON is a format you can
//! finish. What it does not do is silently accept malformed input: every
//! failure names a byte offset, because a receipt that parses differently on
//! two machines is worse than one that does not parse at all.
//!
//! Object members keep their file order. Iteration order is part of a receipt's
//! bytes, and a receipt whose bytes depend on a hash seed cannot be checked
//! against a published hash.

use core::fmt;

/// A parsed JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// JSON has one numeric type. `null` carries the unbounded case, so this is
    /// always finite.
    Number(f64),
    String(String),
    Array(Vec<Json>),
    /// Members in file order, duplicates preserved.
    Object(Vec<(String, Json)>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonError {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for JsonError {}

impl Json {
    /// Parses a complete document, refusing trailing content.
    pub fn parse(text: &str) -> Result<Self, JsonError> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            at: 0,
        };
        parser.skip_whitespace();
        let value = parser.value()?;
        parser.skip_whitespace();
        if parser.at != parser.bytes.len() {
            return Err(parser.error("trailing content after the document"));
        }
        Ok(value)
    }

    /// The member of an object, or `None` for any other shape.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Object(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Renders the value as compact JSON.
    ///
    /// Non-finite numbers are written as `null`: JSON has no infinity, and a
    /// large finite stand-in would claim a measurement that was not made.
    #[must_use]
    pub fn to_compact(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, None, 0);
        out
    }

    /// Renders the value indented by `spaces` per level, with a trailing
    /// newline, which is what the committed receipts hold.
    #[must_use]
    pub fn to_pretty(&self, spaces: usize) -> String {
        let mut out = String::new();
        self.write(&mut out, Some(spaces), 0);
        out.push('\n');
        out
    }

    fn write(&self, out: &mut String, indent: Option<usize>, depth: usize) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(true) => out.push_str("true"),
            Self::Bool(false) => out.push_str("false"),
            Self::Number(value) => out.push_str(&number_to_string(*value)),
            Self::String(text) => write_string(out, text),
            Self::Array(values) => {
                if values.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    newline(out, indent, depth + 1);
                    value.write(out, indent, depth + 1);
                }
                newline(out, indent, depth);
                out.push(']');
            }
            Self::Object(members) => {
                if members.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (index, (key, value)) in members.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    newline(out, indent, depth + 1);
                    write_string(out, key);
                    out.push(':');
                    if indent.is_some() {
                        out.push(' ');
                    }
                    value.write(out, indent, depth + 1);
                }
                newline(out, indent, depth);
                out.push('}');
            }
        }
    }
}

fn newline(out: &mut String, indent: Option<usize>, depth: usize) {
    if let Some(spaces) = indent {
        out.push('\n');
        for _ in 0..spaces * depth {
            out.push(' ');
        }
    }
}

/// Numbers round-trip: a value that is exactly an integer is written without a
/// fractional part, and everything else gets enough digits to parse back to the
/// same double.
fn number_to_string(value: f64) -> String {
    if !value.is_finite() {
        return "null".to_owned();
    }
    if value == value.trunc() && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    let short = format!("{value}");
    if short.parse::<f64>() == Ok(value) {
        return short;
    }
    format!("{value:.17e}")
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Everything below a space has to be escaped; the named forms above
            // are only the common ones.
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn error(&self, message: impl Into<String>) -> JsonError {
        JsonError {
            offset: self.at,
            message: message.into(),
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(byte) = self.bytes.get(self.at) {
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                self.at += 1;
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, literal: &str) -> Result<(), JsonError> {
        if self.bytes[self.at..].starts_with(literal.as_bytes()) {
            self.at += literal.len();
            Ok(())
        } else {
            Err(self.error(format!("expected {literal}")))
        }
    }

    fn value(&mut self) -> Result<Json, JsonError> {
        match self.bytes.get(self.at) {
            None => Err(self.error("the document ends where a value was expected")),
            Some(b'n') => self.expect("null").map(|()| Json::Null),
            Some(b't') => self.expect("true").map(|()| Json::Bool(true)),
            Some(b'f') => self.expect("false").map(|()| Json::Bool(false)),
            Some(b'"') => self.string().map(Json::String),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(byte) if byte.is_ascii_digit() || *byte == b'-' => self.number(),
            Some(byte) => Err(self.error(format!("{} does not start a value", *byte as char))),
        }
    }

    fn array(&mut self) -> Result<Json, JsonError> {
        self.at += 1;
        let mut values = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(Json::Array(values));
        }
        loop {
            self.skip_whitespace();
            values.push(self.value()?);
            self.skip_whitespace();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(values));
                }
                _ => return Err(self.error("expected , or ] in an array")),
            }
        }
    }

    fn object(&mut self) -> Result<Json, JsonError> {
        self.at += 1;
        let mut members = Vec::new();
        self.skip_whitespace();
        if self.bytes.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.skip_whitespace();
            if self.bytes.get(self.at) != Some(&b'"') {
                return Err(self.error("expected a quoted member name"));
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.bytes.get(self.at) != Some(&b':') {
                return Err(self.error("expected : after a member name"));
            }
            self.at += 1;
            self.skip_whitespace();
            members.push((key, self.value()?));
            self.skip_whitespace();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(members));
                }
                _ => return Err(self.error("expected , or } in an object")),
            }
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.at += 1;
        let mut text = String::new();
        loop {
            let byte = *self
                .bytes
                .get(self.at)
                .ok_or_else(|| self.error("the document ends inside a string"))?;
            match byte {
                b'"' => {
                    self.at += 1;
                    return Ok(text);
                }
                b'\\' => {
                    self.at += 1;
                    let escape = *self
                        .bytes
                        .get(self.at)
                        .ok_or_else(|| self.error("the document ends inside an escape"))?;
                    self.at += 1;
                    match escape {
                        b'"' => text.push('"'),
                        b'\\' => text.push('\\'),
                        b'/' => text.push('/'),
                        b'b' => text.push('\u{8}'),
                        b'f' => text.push('\u{c}'),
                        b'n' => text.push('\n'),
                        b'r' => text.push('\r'),
                        b't' => text.push('\t'),
                        b'u' => text.push(self.unicode_escape()?),
                        other => {
                            return Err(self.error(format!("\\{} is not an escape", other as char)));
                        }
                    }
                }
                _ => {
                    // Copy the whole UTF-8 sequence, not the leading byte: the
                    // input is a `&str`, so the boundary is already known good.
                    let rest = &self.bytes[self.at..];
                    let width = utf8_width(rest[0]);
                    if rest.len() < width {
                        return Err(self.error("truncated UTF-8 sequence"));
                    }
                    let slice = core::str::from_utf8(&rest[..width])
                        .map_err(|_| self.error("invalid UTF-8 in a string"))?;
                    text.push_str(slice);
                    self.at += width;
                }
            }
        }
    }

    /// Decodes `\uXXXX`, joining a surrogate pair when it finds one.
    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let high = self.hex4()?;
        if !(0xD800..0xDC00).contains(&high) {
            return char::from_u32(high).ok_or_else(|| self.error("not a Unicode scalar value"));
        }
        if !self.bytes[self.at..].starts_with(b"\\u") {
            return Err(self.error("a high surrogate with no low surrogate after it"));
        }
        self.at += 2;
        let low = self.hex4()?;
        if !(0xDC00..0xE000).contains(&low) {
            return Err(self.error("a high surrogate followed by a non-surrogate"));
        }
        let combined = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
        char::from_u32(combined).ok_or_else(|| self.error("not a Unicode scalar value"))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let slice = self
            .bytes
            .get(self.at..self.at + 4)
            .ok_or_else(|| self.error("a \\u escape needs four hex digits"))?;
        // Each byte is checked directly rather than handed to `from_str_radix`,
        // which accepts a leading `+` and would read `\u+123` as U+0123. This
        // reader exists to reject receipts that are not what they claim to be,
        // so it may not be the more permissive of the two.
        let mut value = 0_u32;
        for &byte in slice {
            let digit = (byte as char)
                .to_digit(16)
                .ok_or_else(|| self.error("a \\u escape is not four hex digits"))?;
            value = (value << 4) | digit;
        }
        self.at += 4;
        Ok(value)
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.at;
        if self.bytes.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        // JSON forbids a leading zero, and accepting one would be worse than
        // pedantry here: `012` would read as twelve, and a receipt written by
        // something that zero-pads its fields would parse into different
        // numbers than it wrote.
        if self.bytes.get(self.at) == Some(&b'0') {
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(byte) if byte.is_ascii_digit()) {
                return Err(self.error("a number may not have a leading zero"));
            }
        } else {
            self.digits()?;
        }
        if self.bytes.get(self.at) == Some(&b'.') {
            self.at += 1;
            self.digits()?;
        }
        if matches!(self.bytes.get(self.at), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.bytes.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            self.digits()?;
        }
        let text = core::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| self.error("a number is not UTF-8"))?;
        let value: f64 = text
            .parse()
            .map_err(|_| self.error(format!("{text} is not a number")))?;
        if !value.is_finite() {
            return Err(self.error(format!("{text} is out of range for a double")));
        }
        Ok(Json::Number(value))
    }

    fn digits(&mut self) -> Result<(), JsonError> {
        let start = self.at;
        while matches!(self.bytes.get(self.at), Some(byte) if byte.is_ascii_digit()) {
            self.at += 1;
        }
        if self.at == start {
            return Err(self.error("expected a digit"));
        }
        Ok(())
    }
}

const fn utf8_width(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// Builds an object from members in the order given.
#[must_use]
pub fn object(members: Vec<(&str, Json)>) -> Json {
    Json::Object(
        members
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

/// A number, or `null` when the value is not finite.
#[must_use]
pub fn number(value: f64) -> Json {
    if value.is_finite() {
        Json::Number(value)
    } else {
        Json::Null
    }
}

/// A string value.
#[must_use]
pub fn string(value: impl Into<String>) -> Json {
    Json::String(value.into())
}

#[cfg(test)]
mod tests {
    use super::{Json, JsonError, number, object, string};

    #[test]
    fn the_shapes_round_trip() {
        let text = r#"{"a":[1,2.5,-3e2],"b":{"c":null,"d":true},"e":"x"}"#;
        let value = Json::parse(text).unwrap();
        assert_eq!(
            value.to_compact(),
            r#"{"a":[1,2.5,-300],"b":{"c":null,"d":true},"e":"x"}"#
        );
        assert_eq!(Json::parse(&value.to_compact()).unwrap(), value);
    }

    #[test]
    fn a_key_inside_a_note_is_not_mistaken_for_the_key() {
        // The failure mode this parser exists to prevent: an ad-hoc search for
        // `"rate"` finds the one inside the note first and reads the wrong
        // number, or no number at all.
        let text = r#"{"note":"the \"rate\": here is prose","rate":4200}"#;
        let value = Json::parse(text).unwrap();
        assert_eq!(value.get("rate").and_then(Json::as_f64), Some(4200.0));
        assert_eq!(
            value.get("note").and_then(Json::as_str),
            Some(r#"the "rate": here is prose"#)
        );
    }

    #[test]
    fn member_order_survives_a_round_trip() {
        // Receipts are hashed as bytes, so a reordering parser would change a
        // published hash without changing a single measurement.
        let text = r#"{"z":1,"a":2,"m":3}"#;
        let value = Json::parse(text).unwrap();
        assert_eq!(value.to_compact(), text);
    }

    #[test]
    fn escapes_and_surrogate_pairs_decode() {
        let value = Json::parse(r#""aA\n\t\\\/😀""#).unwrap();
        assert_eq!(value.as_str(), Some("aA\n\t\\/\u{1f600}"));
        // And re-emit as something that parses back to the same string.
        let again = Json::parse(&value.to_compact()).unwrap();
        assert_eq!(again, value);
    }

    #[test]
    fn multibyte_text_is_copied_whole() {
        let value = Json::parse("\"café ✓ 東京\"").unwrap();
        assert_eq!(value.as_str(), Some("café ✓ 東京"));
    }

    #[test]
    fn malformed_documents_name_an_offset() {
        for (text, fragment) in [
            (r#"{"a":}"#, "does not start a value"),
            (r#"{"a" 1}"#, "expected : after a member name"),
            (r#"{a:1}"#, "expected a quoted member name"),
            (r#"[1,2"#, "expected , or ] in an array"),
            (r#"[1,]"#, "does not start a value"),
            (r#"01"#, "leading zero"),
            (r#"-0.5x"#, "trailing content"),
            (r#"1.  "#, "expected a digit"),
            (r#"tru"#, "expected true"),
            (r#""unterminated"#, "ends inside a string"),
            (r#""\q""#, "is not an escape"),
            (r#""\ud83d""#, "no low surrogate"),
            // A `\u` escape is four hex digits and nothing else. The sign forms
            // are here because the obvious implementation of this parse accepts
            // them: `from_str_radix` reads `+123` as 0x123, so `\u+123` would
            // decode to a character instead of being refused.
            (r#""\u+123""#, "four hex digits"),
            (r#""\u-123""#, "four hex digits"),
            (r#""\u 123""#, "four hex digits"),
            (r#""\u12g4""#, "four hex digits"),
            (r#""\u12""#, "four hex digits"),
            (r#"{} {}"#, "trailing content"),
            (r#"1e999"#, "out of range"),
        ] {
            let error: JsonError = Json::parse(text).unwrap_err();
            assert!(
                error.message.contains(fragment),
                "{text:?} gave {error} rather than {fragment}"
            );
        }
    }

    #[test]
    fn an_infinite_number_is_written_as_null_not_as_a_large_one() {
        let value = object(vec![
            ("psnr", number(f64::INFINITY)),
            ("name", string("lossless")),
        ]);
        assert_eq!(value.to_compact(), r#"{"psnr":null,"name":"lossless"}"#);
    }

    #[test]
    fn awkward_doubles_survive_being_written_and_read() {
        for value in [
            0.1_f64,
            1.0 / 3.0,
            f64::MIN_POSITIVE,
            -1.234_567_890_123_456_7e-8,
            9.007_199_254_740_993e15,
            48.130_803_608_679_1,
        ] {
            let text = Json::Number(value).to_compact();
            let parsed = Json::parse(&text).unwrap().as_f64().unwrap();
            assert_eq!(parsed.to_bits(), value.to_bits(), "{value} became {text}");
        }
    }

    #[test]
    fn pretty_output_indents_and_ends_with_a_newline() {
        let value = object(vec![("a", Json::Array(vec![Json::Number(1.0)]))]);
        assert_eq!(value.to_pretty(2), "{\n  \"a\": [\n    1\n  ]\n}\n");
        assert_eq!(Json::Array(vec![]).to_pretty(2), "[]\n");
    }

    #[test]
    fn accessors_refuse_the_wrong_shape_instead_of_guessing() {
        let value = Json::parse(r#"[1]"#).unwrap();
        assert!(value.get("a").is_none());
        assert!(value.as_str().is_none());
        assert!(value.as_f64().is_none());
        assert!(!value.is_null());
        assert_eq!(value.as_array().map(<[Json]>::len), Some(1));
        assert!(Json::Null.is_null());
    }
}
