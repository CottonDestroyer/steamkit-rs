//! `KeyValue`-backed message objects, as some Steam messages use.

use std::io::{Read, Write};

use crate::error::SteamError;
use crate::types::key_value::KeyValue;

/// A [`KeyValue`]-backed message object, as some Steam messages use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageObject {
    key_values: KeyValue,
}

impl Default for MessageObject {
    fn default() -> MessageObject {
        MessageObject::new()
    }
}

impl MessageObject {
    /// Creates an empty message object.
    pub fn new() -> MessageObject {
        MessageObject {
            key_values: KeyValue::with_name("MessageObject"),
        }
    }

    /// Creates a message object from the given key values.
    pub fn with_key_values(key_values: KeyValue) -> MessageObject {
        MessageObject { key_values }
    }

    /// The underlying [`KeyValue`].
    pub fn key_values(&self) -> &KeyValue {
        &self.key_values
    }

    /// The underlying [`KeyValue`], for modification.
    pub fn key_values_mut(&mut self) -> &mut KeyValue {
        &mut self.key_values
    }

    /// Reads this message object from a stream. Returns `false` on failure.
    pub fn read_from_stream(&mut self, stream: &mut dyn Read) -> bool {
        self.key_values.try_read_as_binary(stream)
    }

    /// Writes this message object to a stream.
    pub fn write_to_stream(&self, stream: &mut dyn Write) -> Result<(), SteamError> {
        self.key_values.save_to_stream(stream, true)
    }
}
