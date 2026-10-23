//! Error type for the SteamLanguage (`.steamd`) tools.

use std::fmt;

/// Result alias used throughout the tool.
pub type LangResult<T> = Result<T, LangError>;

/// Everything that can go wrong while parsing `.steamd` definitions.
#[derive(Debug)]
pub enum LangError {
    /// A definition file could not be read.
    Io {
        /// Path that failed to load.
        path: String,
        /// Underlying I/O failure.
        source: std::io::Error,
    },

    /// The token stream did not match the expected grammar.
    Parse {
        /// File the problem was found in.
        file: String,
        /// 1-based line number.
        line: usize,
        /// 1-based column number.
        column: usize,
        /// Human readable description of the problem.
        message: String,
    },

    /// A construct the parser doesn't support.
    Unsupported {
        /// File the problem was found in.
        file: String,
        /// Human readable description of the unsupported construct.
        message: String,
    },
}

impl LangError {
    /// Builds a [`LangError::Parse`] for the given source position.
    pub fn parse(file: &str, line: usize, column: usize, message: impl Into<String>) -> Self {
        LangError::Parse {
            file: file.to_owned(),
            line,
            column,
            message: message.into(),
        }
    }

    /// Builds a [`LangError::Unsupported`].
    pub fn unsupported(file: &str, message: impl Into<String>) -> Self {
        LangError::Unsupported {
            file: file.to_owned(),
            message: message.into(),
        }
    }
}

impl fmt::Display for LangError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LangError::Io { path, source } => write!(f, "unable to read '{path}': {source}"),
            LangError::Parse {
                file,
                line,
                column,
                message,
            } => write!(f, "{file}:{line},{column}: {message}"),
            LangError::Unsupported { file, message } => {
                write!(f, "{file}: unsupported construct: {message}")
            }
        }
    }
}

impl std::error::Error for LangError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LangError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
