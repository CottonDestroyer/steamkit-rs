//! Async read/write helpers for tokio streams: little-endian numbers and
//! null-terminated strings.

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::SteamError;

/// The text encodings the null-terminated string helpers support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamEncoding {
    /// UTF-8, which Steam uses nearly everywhere.
    Utf8,
    /// US-ASCII; bytes above `0x7F` are replaced with `?`.
    Ascii,
    /// ISO-8859-1 (Latin-1); code points above `U+00FF` are replaced with `?`.
    Latin1,
}

impl StreamEncoding {
    /// How many bytes one character takes (1 for everything but UTF-8, which
    /// is handled separately).
    fn character_size(self) -> usize {
        1
    }

    /// Encodes `value`, without a terminator.
    fn encode(self, value: &str) -> Vec<u8> {
        match self {
            StreamEncoding::Utf8 => value.as_bytes().to_vec(),
            StreamEncoding::Ascii => value
                .chars()
                .map(|character| {
                    if (character as u32) < 0x80 {
                        character as u8
                    } else {
                        b'?'
                    }
                })
                .collect(),
            StreamEncoding::Latin1 => value
                .chars()
                .map(|character| {
                    if (character as u32) < 0x100 {
                        character as u8
                    } else {
                        b'?'
                    }
                })
                .collect(),
        }
    }

    /// Decodes `bytes`.
    fn decode(self, bytes: &[u8]) -> String {
        match self {
            StreamEncoding::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            StreamEncoding::Ascii => bytes
                .iter()
                .map(|byte| {
                    if *byte < 0x80 {
                        *byte as char
                    } else {
                        '?'
                    }
                })
                .collect(),
            StreamEncoding::Latin1 => bytes.iter().map(|byte| *byte as char).collect(),
        }
    }
}

/// Async read helpers.
#[async_trait]
pub trait StreamReadHelpers: AsyncRead + Send + Unpin {
    /// Reads a little endian 16 bit signed integer.
    async fn read_int16(&mut self) -> Result<i16, SteamError> {
        let mut data = [0u8; 2];
        self.read_exact(&mut data).await?;
        Ok(i16::from_le_bytes(data))
    }

    /// Reads a little endian 16 bit unsigned integer.
    async fn read_uint16(&mut self) -> Result<u16, SteamError> {
        let mut data = [0u8; 2];
        self.read_exact(&mut data).await?;
        Ok(u16::from_le_bytes(data))
    }

    /// Reads a little endian 32 bit signed integer.
    async fn read_int32(&mut self) -> Result<i32, SteamError> {
        let mut data = [0u8; 4];
        self.read_exact(&mut data).await?;
        Ok(i32::from_le_bytes(data))
    }

    /// Reads a little endian 64 bit signed integer.
    async fn read_int64(&mut self) -> Result<i64, SteamError> {
        let mut data = [0u8; 8];
        self.read_exact(&mut data).await?;
        Ok(i64::from_le_bytes(data))
    }

    /// Reads a little endian 32 bit unsigned integer.
    async fn read_uint32(&mut self) -> Result<u32, SteamError> {
        let mut data = [0u8; 4];
        self.read_exact(&mut data).await?;
        Ok(u32::from_le_bytes(data))
    }

    /// Reads a little endian 64 bit unsigned integer.
    async fn read_uint64(&mut self) -> Result<u64, SteamError> {
        let mut data = [0u8; 8];
        self.read_exact(&mut data).await?;
        Ok(u64::from_le_bytes(data))
    }

    /// Reads a little endian 32 bit floating point number.
    async fn read_float(&mut self) -> Result<f32, SteamError> {
        let mut data = [0u8; 4];
        self.read_exact(&mut data).await?;
        Ok(f32::from_le_bytes(data))
    }

    /// Reads a null-terminated string in the given encoding, stopping at the
    /// terminator (which isn't included) or the end of the stream.
    async fn read_null_term_string(
        &mut self,
        encoding: StreamEncoding,
    ) -> Result<String, SteamError> {
        let mut result: Vec<u8> = Vec::new();

        if encoding == StreamEncoding::Utf8 {
            // UTF-8: read one byte at a time, since the terminator is a single
            // zero byte.
            loop {
                let mut byte = [0u8; 1];

                let read = self.read(&mut byte).await?;

                if read == 0 || byte[0] == 0 {
                    break;
                }

                result.push(byte[0]);
            }
        } else {
            let character_size = encoding.character_size();
            let mut chunk = vec![0u8; character_size];

            loop {
                chunk.fill(0);

                // Read a whole character, unless the stream ends first.
                let mut read = 0;

                while read < chunk.len() {
                    let count = self.read(&mut chunk[read..]).await?;

                    if count == 0 {
                        break;
                    }

                    read += count;
                }

                if read == 0 {
                    break;
                }

                if encoding.decode(&chunk) == "\0" {
                    break;
                }

                result.extend_from_slice(&chunk);
            }
        }

        Ok(encoding.decode(&result))
    }

    /// Reads until `buffer` is full or the stream ends, and returns how many
    /// bytes were read.
    async fn read_all(&mut self, buffer: &mut [u8]) -> Result<usize, SteamError> {
        let mut read = 0;

        while read < buffer.len() {
            let count = self.read(&mut buffer[read..]).await?;

            if count == 0 {
                break;
            }

            read += count;
        }

        Ok(read)
    }
}

/// Async write helpers.
#[async_trait]
pub trait StreamWriteHelpers: AsyncWrite + Send + Unpin {
    /// Writes `value` followed by a null terminator.
    async fn write_null_term_string(
        &mut self,
        value: &str,
        encoding: StreamEncoding,
    ) -> Result<(), SteamError> {
        let mut bytes = encoding.encode(value);
        bytes.push(0);

        self.write_all(&bytes).await?;
        self.flush().await?;

        Ok(())
    }
}

impl<R> StreamReadHelpers for R where R: AsyncRead + Send + Unpin {}

impl<W> StreamWriteHelpers for W where W: AsyncWrite + Send + Unpin {}
