//! Coder traits from the 7-Zip LZMA SDK (`ICoder`).

use crate::error::SteamError;

/// The exception that is thrown when the value of an argument is outside the allowable range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Invalid Parameter")]
pub struct InvalidParamException;

impl From<InvalidParamException> for SteamError {
    fn from(value: InvalidParamException) -> Self {
        SteamError::InvalidArgument(value.to_string())
    }
}

/// Callback progress.
pub trait ICodeProgress {
    /// Reports progress. Either size is -1 if unknown.
    fn set_progress(&mut self, in_size: i64, out_size: i64);
}

/// Something that compresses an input buffer into an output buffer.
pub trait ICoder {
    /// Compresses `in_stream` and appends the result to `out_stream`.
    ///
    /// `in_size` and `out_size` are -1 if unknown.
    fn code(
        &mut self,
        in_stream: Vec<u8>,
        out_stream: &mut Vec<u8>,
        in_size: i64,
        out_size: i64,
        progress: Option<&mut dyn ICodeProgress>,
    ) -> Result<(), SteamError>;
}

/// Coder property identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoderPropID {
    /// Default property.
    DefaultProp = 0,
    /// Dictionary size.
    DictionarySize,
    /// Memory size for PPM*.
    UsedMemorySize,
    /// Order for PPM methods.
    Order,
    /// Block size.
    BlockSize,
    /// Number of position state bits for LZMA (0 <= x <= 4).
    PosStateBits,
    /// Number of literal context bits for LZMA (0 <= x <= 8).
    LitContextBits,
    /// Number of literal position bits for LZMA (0 <= x <= 4).
    LitPosBits,
    /// Number of fast bytes for LZ*.
    NumFastBytes,
    /// Match finder. LZMA: "BT2", "BT4" or "BT4B".
    MatchFinder,
    /// Number of match finder cycles.
    MatchFinderCycles,
    /// Number of passes.
    NumPasses,
    /// Algorithm number.
    Algorithm,
    /// Number of threads.
    NumThreads,
    /// Write an end marker.
    EndMarker,
}

/// A coder property value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoderProperty {
    /// A 32-bit signed integer property.
    Int32(i32),
    /// A 32-bit unsigned integer property.
    UInt32(u32),
    /// A boolean property.
    Boolean(bool),
    /// A string property.
    String(String),
}

impl CoderProperty {
    /// Returns the value as an `i32`, if it is an integer property.
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            CoderProperty::Int32(v) => Some(*v),
            CoderProperty::UInt32(v) => Some(*v as i32),
            _ => None,
        }
    }

    /// Returns the value as a `bool`, if it is a boolean property.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            CoderProperty::Boolean(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the value as a `&str`, if it is a string property.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            CoderProperty::String(v) => Some(v.as_str()),
            _ => None,
        }
    }
}

/// Sets coder properties.
pub trait ISetCoderProperties {
    /// Sets the coder properties.
    ///
    /// Fails with [`InvalidParamException`] when a property has the wrong
    /// type or an out-of-range value.
    fn set_coder_properties(
        &mut self,
        prop_ids: &[CoderPropID],
        properties: &[CoderProperty],
    ) -> Result<(), InvalidParamException>;
}

/// Writes coder properties.
pub trait IWriteCoderProperties {
    /// Writes the coder properties (the 5 byte LZMA header) to `out_stream`.
    fn write_coder_properties(&mut self, out_stream: &mut Vec<u8>);
}
