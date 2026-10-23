//! Literal formatting helpers.

/// Formats a string as a protobuf string literal.
///
/// Uses the short escapes for control characters, `\\` and `\"`, and a three
/// digit octal escape for anything else below `0x20` and for `0x7F`.
pub fn to_literal(input: &str) -> String {
    let mut literal = String::with_capacity(input.len() + 2);
    literal.push('"');

    for character in input.chars() {
        match character {
            '\u{7}' => literal.push_str("\\a"),
            '\u{8}' => literal.push_str("\\b"),
            '\u{c}' => literal.push_str("\\f"),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\t' => literal.push_str("\\t"),
            '\u{b}' => literal.push_str("\\v"),
            '\\' => literal.push_str("\\\\"),
            '"' => literal.push_str("\\\""),
            _ if (character as u32) < 0x20 || character as u32 == 0x7F => {
                append_octal(&mut literal, character as u32);
            }
            _ => literal.push(character),
        }
    }

    literal.push('"');
    literal
}

/// Formats a byte slice as a protobuf bytes literal.
pub fn to_literal_bytes(input: &[u8]) -> String {
    let mut literal = String::with_capacity(input.len() + 2);
    literal.push('"');

    for byte in input {
        if *byte == b'\\' || *byte == b'"' {
            literal.push('\\');
            literal.push(*byte as char);
        } else if *byte < 0x20 || *byte >= 0x7F {
            append_octal(&mut literal, *byte as u32);
        } else {
            literal.push(*byte as char);
        }
    }

    literal.push('"');
    literal
}

/// Formats a double as a protobuf literal.
pub fn to_literal_double(value: f64) -> String {
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_owned();
    }

    if value.is_nan() {
        return "nan".to_owned();
    }

    format!("{value}")
}

/// Formats a float as a protobuf literal.
pub fn to_literal_float(value: f32) -> String {
    if !value.is_finite() {
        return to_literal_double(f64::from(value));
    }

    format!("{value}")
}

/// Appends a three digit octal escape.
fn append_octal(literal: &mut String, value: u32) {
    literal.push('\\');
    literal.push_str(&format!("{value:03o}"));
}

/// An ordered list of option key/value pairs.
#[derive(Clone, Debug, Default)]
pub struct OptionList {
    entries: Vec<(String, String)>,
}

impl OptionList {
    /// Creates an empty list.
    pub fn new() -> Self {
        OptionList {
            entries: Vec::new(),
        }
    }

    /// Appends an option.
    pub fn add(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.push((key.into(), value.into()));
    }

    /// Appends every option of another list.
    pub fn extend(&mut self, other: OptionList) {
        self.entries.extend(other.entries);
    }

    /// The number of options in the list.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the list holds no options.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterates over the options in the order they were added.
    pub fn iter(&self) -> impl Iterator<Item = &(String, String)> {
        self.entries.iter()
    }
}
