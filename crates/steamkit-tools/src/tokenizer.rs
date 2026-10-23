//! Hand written tokenizer for the SteamLanguage definition language.
//!
//! It behaves like this regex with named groups:
//!
//! ```text
//! (?<whitespace>\s+)
//! |(?<terminator>[;])
//! |["](?<string>.+?)["]
//! |\/\/(?<comment>.*)$
//! |(?<identifier>-?[a-zA-Z_0-9][a-zA-Z0-9_:.]*)
//! |[#](?<preprocess>[a-zA-Z]*)
//! |(?<operator>[{}<>\]=|])
//! |(?<invalid>[^\s]+)
//! ```
//!
//! Details that matter:
//!
//! * alternatives are tried in order at every position, so `//` wins over a
//!   `"` later on the same line;
//! * whitespace and comments don't produce tokens;
//! * strings are lazy: they stop at the first closing quote and never span
//!   lines;
//! * identifiers may start with `-` and end with `:` or `.`;
//! * the operators are `{ } < > ] = |`. `[` is *not* one, so it ends up as
//!   `invalid`.

use crate::error::{LangError, LangResult};

/// The kind of a lexical token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// `;`
    Terminator,
    /// A `"..."` literal; the value excludes the quotes.
    Str,
    /// An identifier such as `EMsg`, `EMsg::Invalid`, `0x01` or `-700`.
    Identifier,
    /// A `#name` preprocessor directive; the value excludes the `#`.
    Preprocess,
    /// One of `{ } < > ] = |`
    Operator,
    /// Anything the grammar does not understand.
    Invalid,
}

impl TokenKind {
    /// The group name, as used in error messages.
    pub fn as_str(self) -> &'static str {
        match self {
            TokenKind::Terminator => "terminator",
            TokenKind::Str => "string",
            TokenKind::Identifier => "identifier",
            TokenKind::Preprocess => "preprocess",
            TokenKind::Operator => "operator",
            TokenKind::Invalid => "invalid",
        }
    }
}

/// A single lexical token together with its source position.
#[derive(Clone, Debug)]
pub struct Token {
    /// The token category.
    pub kind: TokenKind,
    /// The token text (without the delimiters for strings and directives).
    pub value: String,
    /// Name of the file the token came from.
    pub file: String,
    /// 1-based line number.
    pub line: usize,
    /// 1-based column number.
    pub column: usize,
}

impl Token {
    /// True when this token has the given kind and value.
    pub fn is(&self, kind: TokenKind, value: &str) -> bool {
        self.kind == kind && self.value == value
    }
}

/// Whitespace, like the regex `\s`.
///
/// `char::is_whitespace` covers everything that appears in the definition
/// files.
fn is_space(c: char) -> bool {
    c.is_whitespace()
}

fn is_identifier_start(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_identifier_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '.'
}

/// Tokenizes a `.steamd` definition file.
///
/// Comments and whitespace are dropped.
pub fn tokenize(text: &str, file_name: &str) -> LangResult<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens: Vec<Token> = Vec::new();

    let mut index = 0usize;
    let mut line = 1usize;
    let mut column = 1usize;

    while index < chars.len() {
        let current = chars[index];

        // 1. whitespace
        if is_space(current) {
            index += 1;
            if current == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            continue;
        }

        // 2. `//` comment: runs to the end of the line, produces no token.
        if current == '/' && index + 1 < chars.len() && chars[index + 1] == '/' {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
                column += 1;
            }
            continue;
        }

        // 3. `"..."` string: lazy, single line, at least one character.
        if current == '"' {
            if let Some((value, end)) = scan_string(&chars, index) {
                let start_line = line;
                let start_column = column;
                advance(&chars, index, end + 1, &mut line, &mut column);
                tokens.push(Token {
                    kind: TokenKind::Str,
                    value,
                    file: file_name.to_owned(),
                    line: start_line,
                    column: start_column,
                });
                index = end + 1;
                continue;
            }
        }

        // 4. `;` terminator
        if current == ';' {
            tokens.push(Token {
                kind: TokenKind::Terminator,
                value: ";".to_owned(),
                file: file_name.to_owned(),
                line,
                column,
            });
            index += 1;
            column += 1;
            continue;
        }

        // 5. identifier: `-?[a-zA-Z_0-9][a-zA-Z0-9_:.]*`
        if let Some(end) = scan_identifier(&chars, index) {
            let value: String = chars[index..end].iter().collect();
            tokens.push(Token {
                kind: TokenKind::Identifier,
                value,
                file: file_name.to_owned(),
                line,
                column,
            });
            advance(&chars, index, end, &mut line, &mut column);
            index = end;
            continue;
        }

        // 6. `#name` preprocessor directive
        if current == '#' {
            let mut end = index + 1;
            while end < chars.len() && chars[end].is_ascii_alphabetic() {
                end += 1;
            }
            let value: String = chars[index + 1..end].iter().collect();
            tokens.push(Token {
                kind: TokenKind::Preprocess,
                value,
                file: file_name.to_owned(),
                line,
                column,
            });
            advance(&chars, index, end, &mut line, &mut column);
            index = end;
            continue;
        }

        // 7. operator: `{ } < > ] = |`
        if matches!(current, '{' | '}' | '<' | '>' | ']' | '=' | '|') {
            tokens.push(Token {
                kind: TokenKind::Operator,
                value: current.to_string(),
                file: file_name.to_owned(),
                line,
                column,
            });
            index += 1;
            column += 1;
            continue;
        }

        // 8. invalid: a run of non-whitespace characters.
        let mut end = index;
        while end < chars.len() && !is_space(chars[end]) {
            end += 1;
        }
        let value: String = chars[index..end].iter().collect();
        tokens.push(Token {
            kind: TokenKind::Invalid,
            value,
            file: file_name.to_owned(),
            line,
            column,
        });
        advance(&chars, index, end, &mut line, &mut column);
        index = end;
    }

    Ok(tokens)
}

/// Scans a `"..."` literal starting at `start` (which must be a quote).
///
/// Returns the literal's content and the index of the closing quote, or
/// `None` when it's unterminated (it then falls through to `invalid`).
fn scan_string(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut index = start + 1;
    let mut value = String::new();
    while index < chars.len() {
        let current = chars[index];
        if current == '\n' {
            // `.` doesn't match a line break.
            return None;
        }
        if current == '"' {
            if value.is_empty() {
                // `.+?` requires at least one character.
                return None;
            }
            return Some((value, index));
        }
        value.push(current);
        index += 1;
    }
    None
}

/// Scans an identifier starting at `start`, returning the exclusive end index.
fn scan_identifier(chars: &[char], start: usize) -> Option<usize> {
    let mut index = start;
    if chars[index] == '-' {
        index += 1;
        if index >= chars.len() || !is_identifier_start(chars[index]) {
            return None;
        }
    } else if !is_identifier_start(chars[index]) {
        return None;
    }
    index += 1;
    while index < chars.len() && is_identifier_continue(chars[index]) {
        index += 1;
    }
    Some(index)
}

/// Advances the line/column counters over `chars[start..end]`.
fn advance(chars: &[char], start: usize, end: usize, line: &mut usize, column: &mut usize) {
    for current in &chars[start..end] {
        if *current == '\n' {
            *line += 1;
            *column = 1;
        } else {
            *column += 1;
        }
    }
}

/// Convenience wrapper used by the parser to report a positional error.
pub(crate) fn error_at(token: &Token, message: impl Into<String>) -> LangError {
    LangError::parse(&token.file, token.line, token.column, message)
}
