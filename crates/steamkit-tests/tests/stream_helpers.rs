//! `StreamHelpers` tests.
//!
//! [`StreamEncoding`] has no UTF-16 variant, so there are no UTF-16 cases.

use std::sync::Mutex;

use tokio::io::AsyncReadExt;

use steamkit::error::SteamError;
use steamkit::util::stream_helpers::{StreamEncoding, StreamReadHelpers, StreamWriteHelpers};

/// The encoded form of a string.
fn encode(value: &str, encoding: StreamEncoding) -> Vec<u8> {
    match encoding {
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
        StreamEncoding::Utf8 => value.as_bytes().to_vec(),
    }
}

#[tokio::test]
async fn reads_writes_null_terminated_string() {
    let test_cases = [
        (StreamEncoding::Ascii, "Hello, World!"),
        (StreamEncoding::Utf8, "Hello, 世界!"),
        (StreamEncoding::Utf8, "System default encoding."),
        (StreamEncoding::Utf8, "Hello, World!"),
    ];

    for (encoding, value) in test_cases {
        let encoded_bytes = encode(value, encoding);

        // test eos
        let mut reader: &[u8] = &encoded_bytes[..];
        let result = reader
            .read_null_term_string(encoding)
            .await
            .expect("should read a null terminated string");
        assert_eq!(value, result);

        // test null terminated
        let mut encoded_bytes_null_term = encoded_bytes.clone();
        encoded_bytes_null_term.push(0);

        let mut reader: &[u8] = &encoded_bytes_null_term[..];
        let result_null_term = reader
            .read_null_term_string(encoding)
            .await
            .expect("should read a null terminated string");
        assert_eq!(value, result_null_term);
    }

    // Test null term string write
    let mut write_ms: Vec<u8> = Vec::new();
    write_ms
        .write_null_term_string("A", StreamEncoding::Utf8)
        .await
        .expect("should write a null terminated string");
    assert_eq!(write_ms, [65, 0]);

    // Reuse the buffer.
    write_ms.clear();
    write_ms
        .write_null_term_string("A", StreamEncoding::Utf8)
        .await
        .expect("should write a null terminated string");
    assert_eq!(write_ms, [65, 0]);
}

/// Reads a value whose type depends on `thread_number`, rendered as a string.
async fn read_value(
    thread_number: usize,
    reader: &mut (impl tokio::io::AsyncRead + Send + Unpin),
) -> Result<String, SteamError> {
    let value: i64 = match thread_number % 7 {
        0 => i64::from(reader.read_u8().await?),
        1 => reader.read_int64().await?,
        2 => i64::from(reader.read_uint16().await?),
        3 => i64::from(reader.read_int32().await?),
        4 => i64::from(reader.read_uint32().await?),
        5 => reader.read_int64().await?,
        _ => reader.read_uint64().await? as i64,
    };

    Ok(value.to_string())
}

/// 200 threads each read a value 1000 times. The async methods are driven
/// with `futures::executor::block_on`, which is enough because the stream
/// never actually yields.
#[test]
fn is_thread_safe() {
    const NUM_CONCURRENT_THREADS: usize = 200;
    const ITERATIONS: usize = 1000;

    let thread_exceptions: Mutex<Vec<String>> = Mutex::new(Vec::new());

    std::thread::scope(|scope| {
        let thread_exceptions = &thread_exceptions;

        for thread_number in 0..NUM_CONCURRENT_THREADS {
            scope.spawn(move || {
                let bytes = (thread_number as i64).to_le_bytes();

                for _ in 0..ITERATIONS {
                    let mut reader: &[u8] = &bytes[..];

                    let value = futures::executor::block_on(read_value(thread_number, &mut reader));

                    match value {
                        Ok(value) => {
                            let expected = thread_number.to_string();
                            if value != expected {
                                let mut exceptions =
                                    thread_exceptions.lock().unwrap_or_else(|error| error.into_inner());
                                exceptions.push(format!(
                                    "thread {thread_number} read {value} instead of {expected}"
                                ));
                            }
                        }
                        Err(error) => {
                            let mut exceptions =
                                thread_exceptions.lock().unwrap_or_else(|error| error.into_inner());
                            exceptions.push(format!("thread {thread_number} failed: {error}"));
                        }
                    }
                }
            });
        }
    });

    let exceptions = thread_exceptions
        .into_inner()
        .unwrap_or_else(|error| error.into_inner());

    assert!(
        exceptions.is_empty(),
        "threads reported failures: {exceptions:?}"
    );
}

