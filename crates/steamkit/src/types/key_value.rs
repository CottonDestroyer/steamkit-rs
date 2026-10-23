//! `KeyValue`: Valve's key-value format, in both the binary (VDF1) form used
//! for app info, depots and published files, and the text (VDF2) form.
//!
//! The binary reader matches SteamKit2's byte for byte, quirks included: only
//! the first top-level entry is kept, and the text reader works the same way.

use std::fmt;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::sync::OnceLock;

use crate::base::generated::steam_language::EDepotFileFlag;
use crate::error::SteamError;

/// The value types a `KeyValue` can hold on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyValueType {
    None = 0,
    String = 1,
    Int32 = 2,
    Float32 = 3,
    Pointer = 4,
    WideString = 5,
    Color = 6,
    UInt64 = 7,
    End = 8,
    Int64 = 10,
    AlternateEnd = 11,
}

impl KeyValueType {
    fn from_byte(value: u8) -> Option<KeyValueType> {
        match value {
            0 => Some(KeyValueType::None),
            1 => Some(KeyValueType::String),
            2 => Some(KeyValueType::Int32),
            3 => Some(KeyValueType::Float32),
            4 => Some(KeyValueType::Pointer),
            5 => Some(KeyValueType::WideString),
            6 => Some(KeyValueType::Color),
            7 => Some(KeyValueType::UInt64),
            8 => Some(KeyValueType::End),
            10 => Some(KeyValueType::Int64),
            11 => Some(KeyValueType::AlternateEnd),
            _ => None,
        }
    }
}

/// The escape sequences of the text format.
const ESCAPED_MAPPING: [(char, char); 4] = [('\\', '\\'), ('n', '\n'), ('r', '\r'), ('t', '\t')];

/// A tree of string keys to values or child nodes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyValue {
    name: Option<String>,
    value: Option<String>,
    children: Vec<KeyValue>,
}

/// The shared "invalid" node returned when a child doesn't exist.
static INVALID_KEY_VALUE: OnceLock<KeyValue> = OnceLock::new();

impl KeyValue {
    /// Creates an empty node.
    pub fn new() -> KeyValue {
        KeyValue {
            name: None,
            value: None,
            children: Vec::new(),
        }
    }

    /// Creates a node with a name.
    pub fn with_name(name: impl Into<String>) -> KeyValue {
        KeyValue {
            name: Some(name.into()),
            value: None,
            children: Vec::new(),
        }
    }

    /// Creates a node with a name and a value.
    pub fn with_name_and_value(name: impl Into<String>, value: impl Into<String>) -> KeyValue {
        KeyValue {
            name: Some(name.into()),
            value: Some(value.into()),
            children: Vec::new(),
        }
    }

    /// The node returned when a looked-up child doesn't exist.
    pub fn invalid() -> &'static KeyValue {
        INVALID_KEY_VALUE.get_or_init(KeyValue::new)
    }

    /// The name of this node.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Sets the name of this node.
    pub fn set_name(&mut self, name: Option<String>) {
        self.name = name;
    }

    /// The value of this node.
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    /// Sets the value of this node.
    pub fn set_value(&mut self, value: Option<String>) {
        self.value = value;
    }

    /// The children of this node.
    pub fn children(&self) -> &[KeyValue] {
        &self.children
    }

    /// The children of this node, for modification.
    pub fn children_mut(&mut self) -> &mut Vec<KeyValue> {
        &mut self.children
    }

    /// The child with the given key (ordinal, case-insensitive), or
    /// [`KeyValue::invalid`] if there's none.
    pub fn get(&self, key: &str) -> &KeyValue {
        self.children
            .iter()
            .find(|child| {
                child
                    .name
                    .as_deref()
                    .is_some_and(|name| ordinal_ignore_case_eq(name, key))
            })
            .unwrap_or_else(|| KeyValue::invalid())
    }

    /// Sets the child with the given key: removes any existing child with that
    /// key, renames `value` to `key` and appends it.
    pub fn set(&mut self, key: &str, value: KeyValue) {
        if let Some(index) = self.children.iter().position(|child| {
            child
                .name
                .as_deref()
                .is_some_and(|name| ordinal_ignore_case_eq(name, key))
        }) {
            self.children.remove(index);
        }

        let mut value = value;
        value.name = Some(key.to_string());
        self.children.push(value);
    }

    /// The value as a string.
    pub fn as_string(&self) -> Option<&str> {
        self.value.as_deref()
    }

    /// The value as a `u8`, or `default_value` if it isn't one.
    pub fn as_unsigned_byte(&self, default_value: u8) -> u8 {
        parse_integer(self.value.as_deref().unwrap_or(""))
            .and_then(|value| u8::try_from(value).ok())
            .unwrap_or(default_value)
    }

    /// The value as a `u16`, or `default_value` if it isn't one.
    pub fn as_unsigned_short(&self, default_value: u16) -> u16 {
        parse_integer(self.value.as_deref().unwrap_or(""))
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(default_value)
    }

    /// The value as an `i32`, or `default_value` if it isn't one.
    pub fn as_integer(&self, default_value: i32) -> i32 {
        parse_integer(self.value.as_deref().unwrap_or(""))
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(default_value)
    }

    /// The value as a `u32`, or `default_value` if it isn't one.
    pub fn as_unsigned_integer(&self, default_value: u32) -> u32 {
        parse_integer(self.value.as_deref().unwrap_or(""))
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(default_value)
    }

    /// The value as an `i64`, or `default_value` if it isn't one.
    pub fn as_long(&self, default_value: i64) -> i64 {
        parse_integer(self.value.as_deref().unwrap_or("")).unwrap_or(default_value)
    }

    /// The value as a `u64`, or `default_value` if it isn't one.
    pub fn as_unsigned_long(&self, default_value: u64) -> u64 {
        parse_unsigned_integer(self.value.as_deref().unwrap_or("")).unwrap_or(default_value)
    }

    /// The value as an `f32`, or `default_value` if it isn't one.
    pub fn as_float(&self, default_value: f32) -> f32 {
        parse_float(self.value.as_deref().unwrap_or("")).unwrap_or(default_value)
    }

    /// The value as a boolean, or `default_value` if it isn't one.
    pub fn as_boolean(&self, default_value: bool) -> bool {
        match parse_integer(self.value.as_deref().unwrap_or("")) {
            Some(value) => value != 0,
            None => default_value,
        }
    }

    /// The value as an enum, or `default_value` if it isn't one.
    pub fn as_enum<T: FromKeyValue>(&self, default_value: T) -> T {
        self.value
            .as_deref()
            .and_then(T::from_key_value)
            .unwrap_or(default_value)
    }
}

impl fmt::Display for KeyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} = {}",
            self.name.as_deref().unwrap_or(""),
            self.value.as_deref().unwrap_or("")
        )
    }
}

/// Types that can be parsed from a `KeyValue` string. Enums accept the member
/// name or its numeric value.
pub trait FromKeyValue: Sized {
    /// Parses `text`, or `None` if it isn't a valid value.
    fn from_key_value(text: &str) -> Option<Self>;
}

macro_rules! impl_from_key_value_for_integer {
    ($($type:ty),*) => {
        $(
            impl FromKeyValue for $type {
                fn from_key_value(text: &str) -> Option<Self> {
                    parse_unsigned_integer(text).and_then(|value| <$type>::try_from(value).ok())
                }
            }
        )*
    };
}

impl_from_key_value_for_integer!(u8, u16, u32, u64, usize);

impl FromKeyValue for i32 {
    fn from_key_value(text: &str) -> Option<Self> {
        parse_integer(text).and_then(|value| i32::try_from(value).ok())
    }
}

impl FromKeyValue for i64 {
    fn from_key_value(text: &str) -> Option<Self> {
        parse_integer(text)
    }
}

impl FromKeyValue for f32 {
    fn from_key_value(text: &str) -> Option<Self> {
        parse_float(text)
    }
}

impl FromKeyValue for bool {
    fn from_key_value(text: &str) -> Option<Self> {
        parse_integer(text).map(|value| value != 0)
    }
}

impl FromKeyValue for EDepotFileFlag {
    fn from_key_value(text: &str) -> Option<Self> {
        let trimmed = text.trim();

        for flag in EDepotFileFlag::ALL {
            if flag_name(*flag).eq_ignore_ascii_case(trimmed) {
                return Some(*flag);
            }
        }

        let numeric = parse_unsigned_integer(trimmed)?;
        if numeric > i32::MAX as u64 {
            return None;
        }

        EDepotFileFlag::try_from(numeric as i32).ok()
    }
}

/// The name of an `EDepotFileFlag` variant.
fn flag_name(flag: EDepotFileFlag) -> &'static str {
    match flag {
        EDepotFileFlag::UserConfig => "UserConfig",
        EDepotFileFlag::VersionedUserConfig => "VersionedUserConfig",
        EDepotFileFlag::Encrypted => "Encrypted",
        EDepotFileFlag::ReadOnly => "ReadOnly",
        EDepotFileFlag::Hidden => "Hidden",
        EDepotFileFlag::Executable => "Executable",
        EDepotFileFlag::Directory => "Directory",
        EDepotFileFlag::CustomExecutable => "CustomExecutable",
        EDepotFileFlag::InstallScript => "InstallScript",
        EDepotFileFlag::Symlink => "Symlink",
    }
}

/// Parses an integer: optional surrounding whitespace and an optional sign.
fn parse_integer(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    text.parse::<i64>().ok()
}

/// Parses an unsigned integer, with the same rules as [`parse_integer`].
fn parse_unsigned_integer(text: &str) -> Option<u64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    text.parse::<u64>().ok()
}

/// Parses a float (invariant culture, thousands separators allowed).
fn parse_float(text: &str) -> Option<f32> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    text.replace(',', "").parse::<f32>().ok()
}

/// Ordinal, case-insensitive string equality.
pub(crate) fn ordinal_ignore_case_eq(left: &str, right: &str) -> bool {
    let mut left_chars = left.chars();
    let mut right_chars = right.chars();

    loop {
        match (left_chars.next(), right_chars.next()) {
            (None, None) => return true,
            (Some(left_char), Some(right_char)) => {
                if left_char == right_char {
                    continue;
                }

                let left_upper = left_char.to_uppercase().next().unwrap_or(left_char);
                let right_upper = right_char.to_uppercase().next().unwrap_or(right_char);
                if left_upper != right_upper {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

impl KeyValue {
    /// Loads a text `KeyValue` from a file. `None` on any error.
    pub async fn load_as_text(path: impl AsRef<Path>) -> Option<KeyValue> {
        let bytes = tokio::fs::read(path).await.ok()?;
        let mut key_value = KeyValue::new();
        let mut cursor = Cursor::new(bytes);

        match key_value.read_as_text(&mut cursor) {
            Ok(()) => Some(key_value),
            Err(_) => None,
        }
    }

    /// Loads a binary `KeyValue` from a file. `None` on any error.
    pub async fn try_load_as_binary(path: impl AsRef<Path>) -> Option<KeyValue> {
        let bytes = tokio::fs::read(path).await.ok()?;
        let mut key_value = KeyValue::new();
        let mut cursor = Cursor::new(bytes);

        if key_value.try_read_as_binary(&mut cursor) {
            Some(key_value)
        } else {
            None
        }
    }

    /// Parses a text `KeyValue` from a string. `None` on any error.
    pub fn load_from_string(input: &str) -> Option<KeyValue> {
        let mut key_value = KeyValue::new();
        let mut cursor = Cursor::new(input.as_bytes());

        match key_value.read_as_text(&mut cursor) {
            Ok(()) => Some(key_value),
            Err(_) => None,
        }
    }

    /// Reads this node from a text `KeyValue` stream.
    ///
    /// A byte order mark is detected and stripped, so UTF-8 (`EF BB BF`),
    /// UTF-16 LE (`FF FE`) and UTF-16 BE (`FE FF`) documents all work. Without
    /// one, the bytes are read as UTF-8.
    pub fn read_as_text(&mut self, input: &mut dyn Read) -> Result<(), SteamError> {
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes)?;

        self.children.clear();

        let mut reader = KvTextReader::new(&bytes)?;
        let mut is_first_root = true;

        loop {
            let Some((token, _, _)) = reader.read_token() else {
                break;
            };

            if token.is_empty() {
                break;
            }

            // Only the first root object is kept; any further roots are parsed
            // and thrown away.
            let mut throwaway = KeyValue::new();

            let target: &mut KeyValue = if is_first_root {
                is_first_root = false;
                self
            } else {
                &mut throwaway
            };

            target.name = Some(token);

            let (mut value, mut was_quoted, was_conditional) = reader
                .read_token()
                .ok_or_else(|| malformed("LoadFromBuffer: missing {"))?;

            if was_conditional {
                let token = reader
                    .read_token()
                    .ok_or_else(|| malformed("LoadFromBuffer: missing {"))?;
                value = token.0;
                was_quoted = token.1;
            }

            if !(value.starts_with('{') && !was_quoted) {
                return Err(malformed("LoadFromBuffer: missing {"));
            }

            target.recursive_load_from_buffer(&mut reader)?;

            if reader.end_of_stream() {
                break;
            }
        }

        Ok(())
    }

    /// Reads this node from a text `KeyValue` file. The encoding is detected
    /// like in [`KeyValue::read_as_text`].
    pub async fn read_file_as_text(&mut self, filename: impl AsRef<Path>) -> Result<(), SteamError> {
        let bytes = tokio::fs::read(filename).await?;
        let mut cursor = Cursor::new(bytes);
        self.read_as_text(&mut cursor)
    }

    /// Saves this node to a file.
    pub async fn save_to_file(
        &self,
        path: impl AsRef<Path>,
        as_binary: bool,
    ) -> Result<(), SteamError> {
        let mut buffer = Vec::new();
        self.save_to_stream(&mut buffer, as_binary)?;
        tokio::fs::write(path, buffer).await?;
        Ok(())
    }

    /// Saves this node to a stream.
    pub fn save_to_stream(
        &self,
        stream: &mut dyn Write,
        as_binary: bool,
    ) -> Result<(), SteamError> {
        if as_binary {
            self.recursive_save_binary_to_stream(stream)?;
        } else {
            self.recursive_save_text_to_stream(stream, 0)?;
        }

        Ok(())
    }

    /// Reads this node from a binary `KeyValue` stream. Returns `false` on
    /// failure.
    pub fn try_read_as_binary(&mut self, input: &mut dyn Read) -> bool {
        let mut reader = ByteReader::new(input);

        // Only the children are replaced, so the node's own name and value
        // survive a parse that yields no entry.
        let carrier = KeyValue {
            name: self.name.clone(),
            value: self.value.clone(),
            children: Vec::new(),
        };

        match try_read_as_binary_core(&mut reader, carrier, None) {
            Ok(filled) => {
                *self = filled;
                true
            }
            Err(_) => false,
        }
    }

    /// Reads the children of this node from the text reader.
    fn recursive_load_from_buffer(&mut self, reader: &mut KvTextReader) -> Result<(), SteamError> {
        loop {
            let Some((name, was_quoted, _was_conditional)) = reader.read_token() else {
                return Err(malformed("RecursiveLoadFromBuffer: got EOF or empty keyname"));
            };

            if name.is_empty() {
                return Err(malformed("RecursiveLoadFromBuffer: got EOF or empty keyname"));
            }

            // Top level closed, stop reading.
            if name.starts_with('}') && !was_quoted {
                break;
            }

            let mut child = KeyValue::new();
            child.name = Some(name);

            let (mut value, mut was_quoted, mut was_conditional) = reader
                .read_token()
                .ok_or_else(|| malformed("RecursiveLoadFromBuffer:  got NULL key"))?;

            if was_conditional {
                let token = reader
                    .read_token()
                    .ok_or_else(|| malformed("RecursiveLoadFromBuffer:  got NULL key"))?;
                value = token.0;
                was_quoted = token.1;
                was_conditional = token.2;
            }

            if value.starts_with('}') && !was_quoted {
                return Err(malformed("RecursiveLoadFromBuffer:  got } in key"));
            }

            if value.starts_with('{') && !was_quoted {
                child.recursive_load_from_buffer(reader)?;
            } else {
                if was_conditional {
                    return Err(malformed(
                        "RecursiveLoadFromBuffer:  got conditional between key and value",
                    ));
                }

                child.value = Some(value);
            }

            self.children.push(child);
        }

        Ok(())
    }

    /// Writes this node in the binary format, with the end marker.
    fn recursive_save_binary_to_stream(&self, stream: &mut dyn Write) -> Result<(), SteamError> {
        self.recursive_save_binary_to_stream_core(stream)?;
        stream.write_all(&[KeyValueType::End as u8])?;
        Ok(())
    }

    /// Writes this node in the binary format.
    fn recursive_save_binary_to_stream_core(
        &self,
        stream: &mut dyn Write,
    ) -> Result<(), SteamError> {
        match self.value.as_deref() {
            // Only these are supported for now:
            // 1. a node with children (and no value)
            // 2. a string value
            None => {
                stream.write_all(&[KeyValueType::None as u8])?;
                write_null_term_string(stream, &self.name_for_serialization()?)?;

                for child in &self.children {
                    child.recursive_save_binary_to_stream_core(stream)?;
                }

                stream.write_all(&[KeyValueType::End as u8])?;
            }
            Some(value) => {
                stream.write_all(&[KeyValueType::String as u8])?;
                write_null_term_string(stream, &self.name_for_serialization()?)?;
                write_null_term_string(stream, value)?;
            }
        }

        Ok(())
    }

    /// Writes this node in the text format.
    fn recursive_save_text_to_stream(
        &self,
        stream: &mut dyn Write,
        indent_level: usize,
    ) -> Result<(), SteamError> {
        // Header
        write_indents(stream, indent_level)?;
        write_string(stream, &self.name_for_serialization()?, true)?;
        write_string(stream, "\n", false)?;
        write_indents(stream, indent_level)?;
        write_string(stream, "{\n", false)?;

        // Write every child.
        for child in &self.children {
            match child.value.as_deref() {
                None => child.recursive_save_text_to_stream(stream, indent_level + 1)?,
                Some(value) => {
                    write_indents(stream, indent_level + 1)?;
                    write_string(stream, &child.name_for_serialization()?, true)?;
                    write_string(stream, "\t\t", false)?;
                    write_string(stream, &escape_text(value), true)?;
                    write_string(stream, "\n", false)?;
                }
            }
        }

        write_indents(stream, indent_level)?;
        write_string(stream, "}\n", false)?;

        Ok(())
    }

    /// The name to serialize, or an error if the node has none.
    fn name_for_serialization(&self) -> Result<&str, SteamError> {
        self.name.as_deref().ok_or_else(|| {
            SteamError::invalid_argument("Cannot serialise a KeyValue object with a null name!")
        })
    }
}

/// Escapes the special characters of the text format.
fn escape_text(value: &str) -> String {
    let mut escaped = value.to_string();

    for (key, replacement) in ESCAPED_MAPPING {
        escaped = escaped.replace(replacement, &format!("\\{}", key));
    }

    escaped
}

/// Writes `indent_level` tabs.
fn write_indents(stream: &mut dyn Write, indent_level: usize) -> Result<(), SteamError> {
    let indents = "\t".repeat(indent_level);
    write_string(stream, &indents, false)
}

/// Writes a string, optionally quoted.
fn write_string(stream: &mut dyn Write, value: &str, quote: bool) -> Result<(), SteamError> {
    let mut buffer = String::with_capacity(value.len() + 2);

    if quote {
        buffer.push('"');
    }

    buffer.push_str(&value.replace('"', "\\\""));

    if quote {
        buffer.push('"');
    }

    stream.write_all(buffer.as_bytes())?;
    Ok(())
}

/// Writes a null-terminated UTF-8 string.
fn write_null_term_string(stream: &mut dyn Write, value: &str) -> Result<(), SteamError> {
    stream.write_all(value.as_bytes())?;
    stream.write_all(&[0])?;
    Ok(())
}

/// Builds a malformed data error.
fn malformed(message: &str) -> SteamError {
    SteamError::Malformed(message.to_string())
}

/// Reads a null-terminated UTF-8 string.
fn read_null_term_string(reader: &mut ByteReader<'_>) -> std::io::Result<String> {
    reader.read_null_term_string()
}

/// Reads binary `KeyValue` entries. Each filled entry is moved into the parent's
/// children; without a parent, the first entry is kept.
fn try_read_as_binary_core(
    reader: &mut ByteReader<'_>,
    mut current: KeyValue,
    mut parent: Option<&mut Vec<KeyValue>>,
) -> Result<KeyValue, ()> {
    current.children.clear();

    let mut first_entry: Option<KeyValue> = None;

    loop {
        let Some(type_byte) = reader.read_byte().ok().flatten() else {
            // End of stream doesn't match any type, so the parse fails.
            return Err(());
        };

        let Some(value_type) = KeyValueType::from_byte(type_byte) else {
            return Err(());
        };

        if value_type == KeyValueType::End || value_type == KeyValueType::AlternateEnd {
            break;
        }

        current.name = Some(read_null_term_string(reader).map_err(|_| ())?);

        match value_type {
            KeyValueType::None => {
                let _child =
                    try_read_as_binary_core(reader, KeyValue::new(), Some(&mut current.children))?;
            }
            KeyValueType::String => {
                current.value = Some(read_null_term_string(reader).map_err(|_| ())?);
            }
            KeyValueType::WideString => {
                tracing::warn!(
                    "Encountered WideString type when parsing binary KeyValue, which is unsupported. Returning false."
                );
                return Err(());
            }
            KeyValueType::Int32 | KeyValueType::Color | KeyValueType::Pointer => {
                current.value = Some(reader.read_i32().map_err(|_| ())?.to_string());
            }
            KeyValueType::UInt64 => {
                current.value = Some(reader.read_u64().map_err(|_| ())?.to_string());
            }
            KeyValueType::Float32 => {
                current.value = Some(reader.read_f32().map_err(|_| ())?.to_string());
            }
            KeyValueType::Int64 => {
                current.value = Some(reader.read_i64().map_err(|_| ())?.to_string());
            }
            KeyValueType::End | KeyValueType::AlternateEnd => break,
        }

        match parent.as_deref_mut() {
            Some(parent_children) => {
                parent_children.push(current);
                current = KeyValue::new();
            }
            None => {
                // Without a parent, keep the first entry and drop any later
                // top-level ones.
                if first_entry.is_none() {
                    first_entry = Some(std::mem::replace(&mut current, KeyValue::new()));
                }
            }
        }
    }

    Ok(first_entry.unwrap_or(current))
}

/// A small buffered reader for the primitives of the binary format.
struct ByteReader<'a> {
    inner: &'a mut dyn Read,
    buffer: Vec<u8>,
    position: usize,
}

impl<'a> ByteReader<'a> {
    fn new(inner: &'a mut dyn Read) -> ByteReader<'a> {
        ByteReader {
            inner,
            buffer: Vec::new(),
            position: 0,
        }
    }

    /// Makes sure at least one byte is buffered. Returns `false` at the end of
    /// the stream.
    fn fill(&mut self) -> std::io::Result<bool> {
        if self.position < self.buffer.len() {
            return Ok(true);
        }

        self.buffer.clear();
        self.position = 0;

        let mut chunk = [0u8; 4096];
        let read = self.inner.read(&mut chunk)?;
        if read == 0 {
            return Ok(false);
        }

        self.buffer.extend_from_slice(&chunk[..read]);
        Ok(true)
    }

    fn read_byte(&mut self) -> std::io::Result<Option<u8>> {
        if !self.fill()? {
            return Ok(None);
        }

        let byte = self.buffer[self.position];
        self.position += 1;
        Ok(Some(byte))
    }

    fn read_exact_bytes(&mut self, count: usize) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(count);

        while bytes.len() < count {
            if !self.fill()? {
                break;
            }

            let available = self.buffer.len() - self.position;
            let take = available.min(count - bytes.len());
            bytes.extend_from_slice(&self.buffer[self.position..self.position + take]);
            self.position += take;
        }

        Ok(bytes)
    }

    fn read_null_term_string(&mut self) -> std::io::Result<String> {
        let mut bytes = Vec::new();

        while let Some(byte) = self.read_byte()? {
            if byte == 0 {
                break;
            }

            bytes.push(byte);
        }

        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn read_u64(&mut self) -> std::io::Result<u64> {
        let bytes = self.read_exact_bytes(8)?;
        let array: [u8; 8] = bytes.try_into().map_err(|_| unexpected_eof())?;
        Ok(u64::from_le_bytes(array))
    }

    fn read_i32(&mut self) -> std::io::Result<i32> {
        let bytes = self.read_exact_bytes(4)?;
        let array: [u8; 4] = bytes.try_into().map_err(|_| unexpected_eof())?;
        Ok(i32::from_le_bytes(array))
    }

    fn read_i64(&mut self) -> std::io::Result<i64> {
        let bytes = self.read_exact_bytes(8)?;
        let array: [u8; 8] = bytes.try_into().map_err(|_| unexpected_eof())?;
        Ok(i64::from_le_bytes(array))
    }

    fn read_f32(&mut self) -> std::io::Result<f32> {
        let bytes = self.read_exact_bytes(4)?;
        let array: [u8; 4] = bytes.try_into().map_err(|_| unexpected_eof())?;
        Ok(f32::from_le_bytes(array))
    }
}

/// Builds an "unexpected end of stream" error.
fn unexpected_eof() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::UnexpectedEof,
        "unexpected end of stream",
    )
}

/// The UTF-8 byte order mark.
const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// The UTF-16 little endian byte order mark.
const UTF16_LE_BOM: [u8; 2] = [0xFF, 0xFE];

/// The UTF-16 big endian byte order mark.
const UTF16_BE_BOM: [u8; 2] = [0xFE, 0xFF];

/// What malformed input decodes to.
const REPLACEMENT_CHARACTER: char = '\u{FFFD}';

/// The byte order of a UTF-16 encoded document.
#[derive(Clone, Copy)]
enum Utf16Endian {
    Little,
    Big,
}

/// Detects a byte order mark, strips it and decodes the document: UTF-8
/// (`EF BB BF`), UTF-16 LE (`FF FE`) or UTF-16 BE (`FE FF`). Without one, it's
/// read as UTF-8.
fn decode_text(bytes: &[u8]) -> Result<String, SteamError> {
    if bytes.starts_with(&UTF8_BOM) {
        return Ok(String::from_utf8_lossy(&bytes[UTF8_BOM.len()..]).into_owned());
    }

    if bytes.starts_with(&UTF16_LE_BOM) {
        return decode_utf16(&bytes[UTF16_LE_BOM.len()..], Utf16Endian::Little);
    }

    if bytes.starts_with(&UTF16_BE_BOM) {
        return decode_utf16(&bytes[UTF16_BE_BOM.len()..], Utf16Endian::Big);
    }

    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// Decodes UTF-16. Unpaired surrogates become U+FFFD; a trailing half code unit
/// is an error.
fn decode_utf16(bytes: &[u8], endian: Utf16Endian) -> Result<String, SteamError> {
    if bytes.len() % 2 != 0 {
        return Err(malformed(
            "ReadAsText: the utf-16 document ends with an incomplete code unit",
        ));
    }

    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| match endian {
            Utf16Endian::Little => u16::from_le_bytes([pair[0], pair[1]]),
            Utf16Endian::Big => u16::from_be_bytes([pair[0], pair[1]]),
        })
        .collect();

    let mut text = String::with_capacity(units.len());
    let mut index = 0;

    while index < units.len() {
        let unit = units[index];

        if !(0xD800..=0xDBFF).contains(&unit) {
            if (0xDC00..=0xDFFF).contains(&unit) {
                text.push(REPLACEMENT_CHARACTER);
            } else {
                text.push(char::from_u32(u32::from(unit)).unwrap_or(REPLACEMENT_CHARACTER));
            }

            index += 1;
            continue;
        }

        match units.get(index + 1) {
            Some(&low) if (0xDC00..=0xDFFF).contains(&low) => {
                let code =
                    0x1_0000 + (u32::from(unit) - 0xD800) * 0x400 + (u32::from(low) - 0xDC00);
                text.push(char::from_u32(code).unwrap_or(REPLACEMENT_CHARACTER));
                index += 2;
            }
            _ => {
                text.push(REPLACEMENT_CHARACTER);
                index += 1;
            }
        }
    }

    Ok(text)
}

/// Reader for the text format.
struct KvTextReader {
    chars: Vec<char>,
    position: usize,
}

impl KvTextReader {
    /// Creates a reader over the given bytes, detecting the encoding (see
    /// [`decode_text`]).
    fn new(bytes: &[u8]) -> Result<KvTextReader, SteamError> {
        let text = decode_text(bytes)?;

        Ok(KvTextReader {
            chars: text.chars().collect(),
            position: 0,
        })
    }

    fn end_of_stream(&self) -> bool {
        self.position >= self.chars.len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn read_char(&mut self) -> Option<char> {
        let ch = self.peek();
        if ch.is_some() {
            self.position += 1;
        }
        ch
    }

    fn read_line(&mut self) {
        while let Some(ch) = self.read_char() {
            if ch == '\n' {
                break;
            }
        }
    }

    fn eat_white_space(&mut self) {
        while !self.end_of_stream() {
            match self.peek() {
                Some(ch) if ch.is_whitespace() => {
                    self.position += 1;
                }
                _ => break,
            }
        }
    }

    /// A single forward slash comments out the rest of the line.
    fn eat_cpp_comment(&mut self) -> bool {
        if !self.end_of_stream() && self.peek() == Some('/') {
            self.read_line();
            return true;
        }

        false
    }

    /// Reads the next token. Returns the token, whether it was quoted and
    /// whether it was a conditional.
    fn read_token(&mut self) -> Option<(String, bool, bool)> {
        let mut was_quoted = false;
        let mut was_conditional = false;

        loop {
            self.eat_white_space();

            if self.end_of_stream() {
                return None;
            }

            if !self.eat_cpp_comment() {
                break;
            }
        }

        if self.end_of_stream() {
            return None;
        }

        let next = self.peek()?;

        if next == '"' {
            was_quoted = true;

            // "
            self.read_char();

            let mut token = String::new();
            while !self.end_of_stream() {
                if self.peek() == Some('\\') {
                    self.read_char();

                    let escaped = self.read_char();
                    match escaped {
                        Some('\\') => token.push('\\'),
                        Some('n') => token.push('\n'),
                        Some('r') => token.push('\r'),
                        Some('t') => token.push('\t'),
                        Some(other) => token.push(other),
                        None => break,
                    }

                    continue;
                }

                if self.peek() == Some('"') {
                    break;
                }

                match self.read_char() {
                    Some(ch) => token.push(ch),
                    None => break,
                }
            }

            // "
            self.read_char();

            return Some((token, was_quoted, was_conditional));
        }

        if next == '{' || next == '}' {
            self.read_char();
            return Some((next.to_string(), was_quoted, was_conditional));
        }

        let mut conditional_start = false;
        let mut token = String::new();

        while !self.end_of_stream() {
            let next = self.peek()?;

            if next == '"' || next == '{' || next == '}' {
                break;
            }

            if next == '[' {
                conditional_start = true;
            }

            if next == ']' && conditional_start {
                was_conditional = true;
            }

            if next.is_whitespace() {
                break;
            }

            token.push(next);
            self.read_char();
        }

        Some((token, was_quoted, was_conditional))
    }
}
