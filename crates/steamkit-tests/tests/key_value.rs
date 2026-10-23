//! `KeyValue` tests.
//!
//! Temporary files go to the system temp directory and are removed again.
//!
//! * `KeyValue` is a value type, so setting a child renames only the stored
//!   copy, not the caller's.
//! * The binary reader buffers its input, so the tests that check how much
//!   input was consumed feed it one byte per read.
//! * `load_from_string` returns an `Option` instead of failing on null input.

use std::io::Cursor;

use steamkit::types::key_value::KeyValue;

/// A small binary test object.
const TEST_OBJECT_HEX: &str = "00546573744F626A65637400016B65790076616C7565000808";

/// Sixteen bytes of leftover data appended after the test object.
const LEFTOVER_HEX: &str = "0123456789abcdef0123456789abcdef";

/// Saves the given `KeyValue` as text and returns the written text.
fn save_to_text(key_value: &KeyValue) -> String {
    let mut buffer = Vec::new();
    key_value
        .save_to_stream(&mut buffer, false)
        .expect("should save a KeyValue as text");

    String::from_utf8(buffer).expect("the text format is utf-8")
}

/// The tree the text save and Unicode load tests build.
fn root_node() -> KeyValue {
    let mut root = KeyValue::with_name("RootNode");

    let mut key2 = KeyValue::with_name("key2");
    key2.children_mut()
        .push(KeyValue::with_name_and_value("ChildKey", "ChildValue"));

    root.children_mut()
        .push(KeyValue::with_name_and_value("key1", "value1"));
    root.children_mut().push(key2);

    root
}

/// Finds the child with the given name for mutation.
///
/// `KeyValue::get` returns a shared reference, so this is needed to change
/// values.
fn child_mut<'a>(key_value: &'a mut KeyValue, name: &str) -> &'a mut KeyValue {
    let position = key_value
        .children()
        .iter()
        .position(|child| child.name() == Some(name))
        .unwrap_or_else(|| panic!("the child {name} should exist"));

    &mut key_value.children_mut()[position]
}

/// A unique path inside the temporary directory.
fn temporary_file_name(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("steamkit-tests-{}-{name}", std::process::id()));
    path
}

/// Encodes the given text as UTF-16LE with a byte order mark.
fn utf16_le_with_preamble(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];

    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }

    bytes
}

/// Encodes the given text as UTF-16BE with a byte order mark.
fn utf16_be_with_preamble(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFE, 0xFF];

    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }

    bytes
}

/// A `Read` implementation that hands out at most one byte per call.
///
/// The binary reader buffers its input in 4096 byte chunks, which would drain
/// a `Cursor` in one call. One byte at a time makes the number of consumed
/// bytes observable.
struct SingleByteReader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> SingleByteReader<'a> {
    fn new(data: &'a [u8]) -> SingleByteReader<'a> {
        SingleByteReader { data, position: 0 }
    }
}

impl std::io::Read for SingleByteReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() || self.position >= self.data.len() {
            return Ok(0);
        }

        buffer[0] = self.data[self.position];
        self.position += 1;

        Ok(1)
    }
}

#[test]
fn key_value_initializes_correctly() {
    let kv = KeyValue::with_name_and_value("name", "value");

    assert_eq!(Some("name"), kv.name());
    assert_eq!(Some("value"), kv.value());

    assert!(kv.children().is_empty());
}

#[test]
fn key_value_indexer_returns_valid_and_invalid() {
    let mut kv = KeyValue::new();

    kv.children_mut()
        .push(KeyValue::with_name_and_value("exists", "value"));

    assert_eq!(Some("value"), kv.get("exists").value());
    assert!(std::ptr::eq(
        kv.get("thiskeydoesntexist"),
        KeyValue::invalid()
    ));
}

#[test]
fn key_value_indexer_doesntallow_duplicates() {
    let mut kv = KeyValue::new();

    kv.set("key", KeyValue::new());

    assert_eq!(1, kv.children().len());

    kv.set("key", KeyValue::new());

    assert_eq!(1, kv.children().len());

    kv.set("key2", KeyValue::new());

    assert_eq!(2, kv.children().len());
}

/// `KeyValue` is a value type, so `KeyValue::set` renames only the copy it
/// stores, not the caller's.
#[test]
fn key_value_indexer_updates_key() {
    let mut kv = KeyValue::new();

    let subkey = KeyValue::new();

    assert!(subkey.name().is_none());

    kv.set("subkey", subkey);

    assert_eq!(Some("subkey"), kv.get("subkey").name());
}

#[test]
fn key_value_indexer_handles_case_insensitive_duplicates() {
    let mut kv = KeyValue::new();

    kv.set("key", KeyValue::new());
    assert_eq!(1, kv.children().len());

    kv.set("KEY", KeyValue::new()); // Different case
    assert_eq!(1, kv.children().len()); // Should still be 1, not 2

    kv.set("Key", KeyValue::new()); // Another variation
    assert_eq!(1, kv.children().len()); // Should still be 1
}

#[test]
fn key_value_loads_from_string() {
    let kv = KeyValue::load_from_string(
        r#""root" 
  {
      "name" "value"
      "subkey"
      {
          "name2" "value2"
      }
  }"#,
    )
    .expect("should load the text document");

    assert_eq!(Some("root"), kv.name());
    assert!(kv.value().is_none());

    assert_eq!(Some("name"), kv.get("name").name());
    assert_eq!(Some("value"), kv.get("name").value());
    assert!(kv.get("name").children().is_empty());

    let sub_key = kv.get("subkey");

    assert_eq!(1, sub_key.children().len());

    assert_eq!(Some("name2"), sub_key.get("name2").name());
    assert_eq!(Some("value2"), sub_key.get("name2").value());
    assert!(sub_key.get("name2").children().is_empty());
}

#[test]
fn key_values_missing_keys_give_invalid() {
    let kv = KeyValue::new();

    assert!(std::ptr::eq(kv.get("missingkey"), KeyValue::invalid()));
}

#[test]
fn key_values_keys_are_case_insensitive() {
    let kv = KeyValue::load_from_string(
        r#""root"
  {
      "name" "value"
  }"#,
    )
    .expect("should load the text document");

    assert_eq!(Some("value"), kv.get("name").value());
    assert_eq!(Some("value"), kv.get("NAME").value());
    assert_eq!(Some("value"), kv.get("NAme").value());
}

#[test]
fn key_values_handles_bool() {
    let mut kv = KeyValue::load_from_string(
        r#""root"
  {
      "name" "1"
  }"#,
    )
    .expect("should load the text document");

    let child = child_mut(&mut kv, "name");

    assert!(child.as_boolean(false));

    child.set_value(Some("0".to_string()));
    assert!(!child.as_boolean(false));

    child.set_value(Some("100".to_string()));
    assert!(child.as_boolean(false), "values other than 0 are truthy");

    child.set_value(Some("invalidbool".to_string()));
    assert!(
        !child.as_boolean(false),
        "values that cannot be converted to integers are falsey"
    );
}

#[test]
fn key_values_handles_float() {
    let mut kv = KeyValue::load_from_string(
        r#""root"
  {
      "name" "123.456"
  }"#,
    )
    .expect("should load the text document");

    assert_eq!(123.456f32, kv.get("name").as_float(0.0));

    child_mut(&mut kv, "name").set_value(Some("invalidfloat".to_string()));
    assert_eq!(321.654f32, kv.get("name").as_float(321.654)); // invalid parse returns the default
}

#[test]
fn key_values_handles_int() {
    let mut kv = KeyValue::load_from_string(
        r#""root"
  {
      "name" "123"
  }"#,
    )
    .expect("should load the text document");

    assert_eq!(123, kv.get("name").as_integer(0));

    child_mut(&mut kv, "name").set_value(Some("invalidint".to_string()));
    assert_eq!(987, kv.get("name").as_integer(987)); // invalid parse returns the default
}

#[test]
fn key_values_handles_long() {
    let mut kv = KeyValue::load_from_string(
        r#""root"
  {
      "name" "-5001050759734897745"
  }"#,
    )
    .expect("should load the text document");

    assert_eq!(-5001050759734897745i64, kv.get("name").as_long(0));

    child_mut(&mut kv, "name").set_value(Some("invalidlong".to_string()));
    assert_eq!(678, kv.get("name").as_long(678)); // invalid parse returns the default
}

#[test]
fn key_values_handles_string() {
    let kv = KeyValue::load_from_string(
        r#""root"
  {
      "name" "stringvalue"
  }"#,
    )
    .expect("should load the text document");

    assert_eq!(Some("stringvalue"), kv.get("name").as_string());
    assert_eq!(Some("stringvalue"), kv.get("name").value());
}

/// The tree the binary serialization tests build.
fn rich_presence() -> KeyValue {
    let mut kv = KeyValue::with_name("RP");

    kv.children_mut()
        .push(KeyValue::with_name_and_value("status", "#DOTA_RP_BOTPRACTICE"));
    kv.children_mut()
        .push(KeyValue::with_name_and_value("num_params", "0"));
    kv.children_mut().push(KeyValue::with_name_and_value(
        "watching_server",
        "[A:1:2108935169:5412]",
    ));
    kv.children_mut().push(KeyValue::with_name_and_value(
        "watching_from_server",
        "[A:1:864468994:5412]",
    ));

    kv
}

/// The exact bytes the binary serialization tests expect.
const EXPECTED_RICH_PRESENCE_HEX: &str = "00525000017374617475730023444F54415F52505F424F54505241435449434500016E756D5F706172616D730030000\
                                          17761746368696E675F736572766572005B413A313A323130383933353136393A353431325D00017761746368696E67\
                                          5F66726F6D5F736572766572005B413A313A3836343436383939343A353431325D000808";

#[tokio::test]
async fn key_values_writes_binary_to_file() {
    let kv = rich_presence();

    let temporary_file = temporary_file_name("key-values-writes-binary-to-file.bin");

    kv.save_to_file(&temporary_file, true)
        .await
        .expect("should save a KeyValue as binary");

    let binary_value = std::fs::read(&temporary_file).expect("should read the file back");
    let _ = std::fs::remove_file(&temporary_file);

    assert_eq!(EXPECTED_RICH_PRESENCE_HEX, hex::encode_upper(&binary_value));
}

#[test]
fn key_values_writes_binary_to_stream() {
    let kv = rich_presence();

    let mut binary_value: Vec<u8> = Vec::new();
    kv.save_to_stream(&mut binary_value, true)
        .expect("should save a KeyValue as binary");

    assert_eq!(
        EXPECTED_RICH_PRESENCE_HEX,
        hex::encode_upper(&binary_value)
    );
}

#[test]
fn key_value_binary_serialization_is_symmetric() {
    let mut kv = KeyValue::with_name("MessageObject");
    kv.children_mut()
        .push(KeyValue::with_name_and_value("key", "value"));

    let mut buffer: Vec<u8> = Vec::new();
    kv.save_to_stream(&mut buffer, true)
        .expect("should save a KeyValue as binary");

    let mut deserialized_kv = KeyValue::new();
    let loaded = deserialized_kv.try_read_as_binary(&mut Cursor::new(buffer));

    assert!(loaded);

    assert_eq!(kv.name(), deserialized_kv.name());
    assert_eq!(kv.children().len(), deserialized_kv.children().len());

    for (index, original_child) in kv.children().iter().enumerate() {
        let deserialized_child = &deserialized_kv.children()[index];

        assert_eq!(original_child.name(), deserialized_child.name());
        assert_eq!(original_child.value(), deserialized_child.value());
    }
}

#[test]
fn key_values_try_read_as_binary_reads_binary() {
    let data = hex::decode(TEST_OBJECT_HEX).expect("the test object is valid hex");

    let mut kv = KeyValue::new();
    let mut reader = SingleByteReader::new(&data);
    let success = kv.try_read_as_binary(&mut reader);

    assert_eq!(data.len(), reader.position);

    assert!(success, "should have read test object");
    assert_eq!(Some("TestObject"), kv.name());
    assert_eq!(1, kv.children().len());
    assert_eq!(Some("key"), kv.children()[0].name());
    assert_eq!(Some("value"), kv.children()[0].value());
}

#[test]
fn key_values_reads_binary_with_leftover_data() {
    let mut data = hex::decode(TEST_OBJECT_HEX).expect("the test object is valid hex");
    data.extend_from_slice(&hex::decode(LEFTOVER_HEX).expect("the leftover data is valid hex"));

    let mut kv = KeyValue::new();
    let mut reader = SingleByteReader::new(&data);
    let success = kv.try_read_as_binary(&mut reader);

    assert_eq!(TEST_OBJECT_HEX.len() / 2, reader.position);
    assert_eq!(16, data.len() - reader.position);

    assert!(success, "should have read test object");
    assert_eq!(Some("TestObject"), kv.name());
    assert_eq!(1, kv.children().len());
    assert_eq!(Some("key"), kv.children()[0].name());
    assert_eq!(Some("value"), kv.children()[0].value());
}

/// Test every possible truncation boundary we have.
#[test]
fn key_values_fails_to_read_truncated_binary() {
    let data = hex::decode(TEST_OBJECT_HEX).expect("the test object is valid hex");

    for length in 0..data.len() {
        let truncated = &data[..length];

        let mut kv = KeyValue::new();
        let mut reader = SingleByteReader::new(truncated);
        let success = kv.try_read_as_binary(&mut reader);

        assert_eq!(length, reader.position);

        assert!(
            !success,
            "should not have read test object truncated to {length} bytes"
        );
    }
}

#[test]
fn key_values_reads_binary_with_multiple_children() {
    let hex = "00546573744f626a65637400016b6579310076616c75653100016b6579320076616c756532000808";
    let binary = hex::decode(hex).expect("the hex is valid");

    let mut kv = KeyValue::new();
    let success = kv.try_read_as_binary(&mut Cursor::new(binary));

    assert!(success);

    assert_eq!(Some("TestObject"), kv.name());
    assert_eq!(2, kv.children().len());
    assert_eq!(Some("key1"), kv.children()[0].name());
    assert_eq!(Some("value1"), kv.children()[0].value());
    assert_eq!(Some("key2"), kv.children()[1].name());
    assert_eq!(Some("value2"), kv.children()[1].value());
}

#[tokio::test]
async fn key_values_saves_text_to_file() {
    let expected = "\"RootNode\"\n{\n\t\"key1\"\t\t\"value1\"\n\t\"key2\"\n\t{\n\t\t\"ChildKey\"\t\t\"ChildValue\"\n\t}\n}\n";

    let kv = root_node();

    let temporary_file = temporary_file_name("key-values-saves-text-to-file.txt");

    kv.save_to_file(&temporary_file, false)
        .await
        .expect("should save a KeyValue as text");

    let text = std::fs::read_to_string(&temporary_file).expect("should read the file back");
    let _ = std::fs::remove_file(&temporary_file);

    assert_eq!(expected, text);
}

#[test]
fn key_values_saves_text_to_stream() {
    let expected = "\"RootNode\"\n{\n\t\"key1\"\t\t\"value1\"\n\t\"key2\"\n\t{\n\t\t\"ChildKey\"\t\t\"ChildValue\"\n\t}\n}\n";

    let kv = root_node();

    let text = save_to_text(&kv);

    assert_eq!(expected, text);
}

/// Loads a UTF-16 document with a byte order mark.
#[tokio::test]
async fn can_load_unicode_text_document() {
    let expected = "\"RootNode\"\n{\n\t\"key1\"\t\t\"value1\"\n\t\"key2\"\n\t{\n\t\t\"ChildKey\"\t\t\"ChildValue\"\n\t}\n}\n";
    let mut kv = KeyValue::new();

    let temporary_file = temporary_file_name("can-load-unicode-text-document.txt");

    std::fs::write(&temporary_file, utf16_le_with_preamble(expected))
        .expect("should write the utf-16 document");

    let read = kv.read_file_as_text(&temporary_file).await;
    let _ = std::fs::remove_file(&temporary_file);

    match read {
        Ok(()) => {}
        Err(error) => panic!("a utf-16 text document should load: {error}"),
    }

    assert_eq!(Some("RootNode"), kv.name());
    assert_eq!(2, kv.children().len());
    assert_eq!(Some("key1"), kv.children()[0].name());
    assert_eq!(Some("value1"), kv.children()[0].value());
    assert_eq!(Some("key2"), kv.children()[1].name());
    assert_eq!(1, kv.children()[1].children().len());
    assert_eq!(Some("ChildKey"), kv.children()[1].children()[0].name());
    assert_eq!(
        Some("ChildValue"),
        kv.children()[1].children()[0].value()
    );
}

/// Same as above, from a stream.
#[test]
fn can_load_unicode_text_stream() {
    let expected = "\"RootNode\"\n{\n\t\"key1\"\t\t\"value1\"\n\t\"key2\"\n\t{\n\t\t\"ChildKey\"\t\t\"ChildValue\"\n\t}\n}\n";
    let mut kv = KeyValue::new();

    let bytes = utf16_le_with_preamble(expected);

    match kv.read_as_text(&mut Cursor::new(bytes)) {
        Ok(()) => {}
        Err(error) => panic!("a utf-16 text stream should load: {error}"),
    }

    assert_eq!(Some("RootNode"), kv.name());
    assert_eq!(2, kv.children().len());
    assert_eq!(Some("key1"), kv.children()[0].name());
    assert_eq!(Some("value1"), kv.children()[0].value());
    assert_eq!(Some("key2"), kv.children()[1].name());
    assert_eq!(1, kv.children()[1].children().len());
    assert_eq!(Some("ChildKey"), kv.children()[1].children()[0].name());
    assert_eq!(
        Some("ChildValue"),
        kv.children()[1].children()[0].value()
    );
}

#[test]
fn writes_new_line_as_slash_n() {
    let mut kv = KeyValue::with_name("abc");
    kv.children_mut()
        .push(KeyValue::with_name_and_value("def", "ghi\njkl"));

    let text = save_to_text(&kv);

    let expected = "\"abc\"\n{\n\t\"def\"\t\t\"ghi\\njkl\"\n}\n";

    assert_eq!(expected, text);
}

#[test]
fn key_values_unsigned_byte_conversion() {
    let expected_value: u8 = 37;

    let mut kv = KeyValue::with_name_and_value("key", "37");
    assert_eq!(expected_value, kv.as_unsigned_byte(0));

    kv.set_value(Some("256".to_string()));
    assert_eq!(expected_value, kv.as_unsigned_byte(expected_value));
}

#[test]
fn key_values_unsigned_short_conversion() {
    let expected_value: u16 = 1337;

    let mut kv = KeyValue::with_name_and_value("key", "1337");
    assert_eq!(expected_value, kv.as_unsigned_short(0));

    kv.set_value(Some("123456".to_string()));
    assert_eq!(expected_value, kv.as_unsigned_short(expected_value));
}

#[test]
fn key_values_escapes_text_when_serializing() {
    let mut kv = KeyValue::with_name("key");
    kv.children_mut()
        .push(KeyValue::with_name_and_value("slashes", "\\o/"));
    kv.children_mut()
        .push(KeyValue::with_name_and_value("newline", "\r\n"));

    let text = save_to_text(&kv);

    let expected_value = "\"key\"\n{\n\t\"slashes\"\t\t\"\\\\o/\"\n\t\"newline\"\t\t\"\\r\\n\"\n}\n";
    assert_eq!(expected_value, text);
}

#[test]
fn key_values_text_preserve_empty_objects() {
    let mut kv = KeyValue::with_name("key");
    kv.children_mut().push(KeyValue::with_name("emptyObj"));
    kv.children_mut()
        .push(KeyValue::with_name_and_value("emptyString", ""));

    let text = save_to_text(&kv);

    let expected_value = "\"key\"\n{\n\t\"emptyObj\"\n\t{\n\t}\n\t\"emptyString\"\t\t\"\"\n}\n";
    assert_eq!(expected_value, text);
}

#[test]
fn key_values_binary_preserve_empty_objects() {
    let expected_hex_string = "006B65790000656D7074794F626A000801656D707479537472696E6700000808";

    let mut kv = KeyValue::with_name("key");
    kv.children_mut().push(KeyValue::with_name("emptyObj"));
    kv.children_mut()
        .push(KeyValue::with_name_and_value("emptyString", ""));

    let mut binary_value: Vec<u8> = Vec::new();
    kv.save_to_stream(&mut binary_value, true)
        .expect("should save a KeyValue as binary");

    let mut deserialized_kv = KeyValue::new();
    let loaded = deserialized_kv.try_read_as_binary(&mut Cursor::new(binary_value.clone()));

    assert!(loaded);

    assert_eq!(expected_hex_string, hex::encode_upper(&binary_value));

    assert!(deserialized_kv.get("emptyObj").value().is_none());
    assert!(deserialized_kv.get("emptyObj").children().is_empty());
    assert_eq!(Some(""), deserialized_kv.get("emptyString").value());
}

#[test]
fn decodes_binary_with_field_type_10() {
    let hex = "00546573744F626A656374000A6B65790001020304050607080808";
    let binary = hex::decode(hex).expect("the hex is valid");

    let mut kv = KeyValue::new();
    let read = kv.try_read_as_binary(&mut Cursor::new(binary));
    assert!(read);

    assert_eq!(0x0807060504030201, kv.get("key").as_long(0));
}

#[test]
fn decodes_binary_with_alternate_end() {
    let hex = "00546573744F626a656374000A6B65790001020304050607080B0B";
    let binary = hex::decode(hex).expect("the hex is valid");

    let mut kv = KeyValue::new();
    let read = kv.try_read_as_binary(&mut Cursor::new(binary));
    assert!(read);

    assert_eq!(0x0807060504030201, kv.get("key").as_long(0));
}

/// The text reader detects a byte order mark, so every encoding of the same
/// document yields the same tree.
#[test]
fn can_load_text_documents_with_a_byte_order_mark() {
    let expected = "\"RootNode\"\n{\n\t\"key1\"\t\t\"value1\"\n\t\"key2\"\n\t{\n\t\t\"ChildKey\"\t\t\"ChildValue\"\n\t}\n}\n";

    let mut utf8_with_preamble = vec![0xEF, 0xBB, 0xBF];
    utf8_with_preamble.extend_from_slice(expected.as_bytes());

    let documents = [
        ("utf-8", expected.as_bytes().to_vec()),
        ("utf-8 with a byte order mark", utf8_with_preamble),
        (
            "utf-16le with a byte order mark",
            utf16_le_with_preamble(expected),
        ),
        (
            "utf-16be with a byte order mark",
            utf16_be_with_preamble(expected),
        ),
    ];

    for (name, bytes) in documents {
        let mut kv = KeyValue::new();

        match kv.read_as_text(&mut Cursor::new(bytes)) {
            Ok(()) => {}
            Err(error) => panic!("a {name} text stream should load: {error}"),
        }

        assert_eq!(Some("RootNode"), kv.name(), "{name}");
        assert_eq!(2, kv.children().len(), "{name}");
        assert_eq!(Some("key1"), kv.children()[0].name(), "{name}");
        assert_eq!(Some("value1"), kv.children()[0].value(), "{name}");
        assert_eq!(Some("key2"), kv.children()[1].name(), "{name}");
        assert_eq!(1, kv.children()[1].children().len(), "{name}");
        assert_eq!(
            Some("ChildKey"),
            kv.children()[1].children()[0].name(),
            "{name}"
        );
        assert_eq!(
            Some("ChildValue"),
            kv.children()[1].children()[0].value(),
            "{name}"
        );
    }
}
