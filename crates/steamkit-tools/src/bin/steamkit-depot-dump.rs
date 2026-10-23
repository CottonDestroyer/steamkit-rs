//! Prints a depot manifest (and optionally a decrypted chunk) as plain text,
//! so the output can be diffed against another implementation.
//!
//! Usage:
//!   steamkit-depot-dump <manifest-file> <depot-key-hex> [chunk-file <chunk-id-hex>]

use std::env;
use std::fs;
use std::process::ExitCode;

use steamkit::steam::cdn::depot_chunk::DepotChunk;
use steamkit::types::depot_manifest::DepotManifest;

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if cleaned.len() % 2 != 0 {
        return Err(format!("{text} is not a whole number of hex bytes"));
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&cleaned[i..i + 2], 16).map_err(|e| format!("bad hex: {e}"))
        })
        .collect()
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("usage: steamkit-depot-dump <manifest-file> <depot-key-hex> \
                    [chunk-file <chunk-id-hex>]"
            .to_owned());
    }

    let manifest_bytes = fs::read(&args[0]).map_err(|e| format!("reading the manifest: {e}"))?;
    let key = parse_hex(&args[1])?;

    let mut manifest =
        DepotManifest::deserialize(&manifest_bytes).map_err(|e| format!("deserializing: {e}"))?;

    let was_encrypted = manifest.filenames_encrypted();
    let decrypted = manifest.decrypt_filenames(&key);
    let files = manifest.files().unwrap_or(&[]);

    println!(
        "MANIFEST depot_id={} gid={} encrypted={} decrypt_ok={} file_count={} \
         total_uncompressed={} total_compressed={}",
        manifest.depot_id(),
        manifest.manifest_gid(),
        was_encrypted,
        decrypted,
        files.len(),
        manifest.total_uncompressed_size(),
        manifest.total_compressed_size(),
    );

    for (index, file) in files.iter().enumerate() {
        println!(
            "FILE {index} name={} name_sha1={} size={} flags={} file_sha1={} link_target={} \
             chunks={}",
            file.file_name,
            hex(&file.file_name_hash),
            file.total_size,
            file.flags,
            hex(&file.file_hash),
            file.link_target.as_deref().unwrap_or(""),
            file.chunks.len(),
        );

        for (chunk_index, chunk) in file.chunks.iter().enumerate() {
            println!(
                "CHUNK {index} {chunk_index} id={} csize={} usize={} sum={}",
                chunk
                    .chunk_id
                    .as_deref()
                    .map(hex)
                    .unwrap_or_else(|| "none".to_owned()),
                chunk.compressed_length,
                chunk.uncompressed_length,
                chunk.checksum,
            );
        }
    }

    if args.len() >= 4 {
        let chunk_bytes = fs::read(&args[2]).map_err(|e| format!("reading the chunk: {e}"))?;
        let wanted = parse_hex(&args[3])?;

        let chunk = files
            .iter()
            .flat_map(|f| f.chunks.iter())
            .find(|c| c.chunk_id.as_deref() == Some(wanted.as_slice()))
            .ok_or_else(|| "no chunk with that id in this manifest".to_owned())?;

        let mut destination = vec![0u8; chunk.uncompressed_length as usize];
        let written = DepotChunk::process(chunk, &chunk_bytes, &mut destination, &key)
            .map_err(|e| format!("decrypting the chunk: {e}"))?;

        println!("CHUNK_DECRYPT len={written} first64={}", hex(&destination[..written.min(64)]));
    }

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("steamkit-depot-dump: {message}");
            ExitCode::FAILURE
        }
    }
}
