//! Error type shared by `steamkit-proto-gen` and `steamkit-proto-dump`.

use std::fmt;

/// Result alias used by the protobuf tooling.
pub type ProtoResult<T> = Result<T, ProtoError>;

/// Everything that can go wrong while working with protobuf definitions.
#[derive(Debug)]
pub enum ProtoError {
    /// A file could not be read or written.
    Io {
        /// Path that failed to load.
        path: String,
        /// Underlying I/O failure.
        source: std::io::Error,
    },

    /// The `protoc` compiler could not be located or could not be run.
    Protoc {
        /// Human readable description of the problem.
        message: String,
    },

    /// A `.proto` file could not be compiled.
    Compile {
        /// The file that failed to compile.
        file: String,
        /// Human readable description of the problem.
        message: String,
    },

    /// A protobuf descriptor could not be decoded.
    Decode {
        /// Human readable description of the problem.
        message: String,
    },

    /// A file or directory that was expected to exist does not.
    NotFound {
        /// The path that was looked for.
        path: String,
    },

    /// The command line could not be understood.
    InvalidArgument {
        /// Human readable description of the problem.
        message: String,
    },
}

impl ProtoError {
    /// Builds a [`ProtoError::Io`] for the given path.
    pub fn io(path: impl Into<String>, source: std::io::Error) -> Self {
        ProtoError::Io {
            path: path.into(),
            source,
        }
    }

    /// Builds a [`ProtoError::NotFound`] for the given path.
    pub fn not_found(path: impl Into<String>) -> Self {
        ProtoError::NotFound { path: path.into() }
    }

    /// Builds a [`ProtoError::InvalidArgument`].
    pub fn invalid_argument(message: impl Into<String>) -> Self {
        ProtoError::InvalidArgument {
            message: message.into(),
        }
    }
}

impl fmt::Display for ProtoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtoError::Io { path, source } => write!(f, "unable to read '{path}': {source}"),
            ProtoError::Protoc { message } => write!(f, "protoc failed: {message}"),
            ProtoError::Compile { file, message } => write!(f, "{file}: {message}"),
            ProtoError::Decode { message } => write!(f, "unable to decode descriptor: {message}"),
            ProtoError::NotFound { path } => write!(f, "'{path}' does not exist"),
            ProtoError::InvalidArgument { message } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ProtoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProtoError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
