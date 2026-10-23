//! Compiles `.proto` files into a protobuf descriptor set.
//!
//! Uses the same vendored `protoc` as `crates/steamkit-protos/build.rs`, so
//! the descriptors match the ones the generated types are built from.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::proto_dumper::raw_options::RawDescriptor;
use crate::proto_dumper::wire;
use crate::proto_error::{ProtoError, ProtoResult};

/// Counter that keeps the temporary descriptor set files apart.
static TEMP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// The `protoc` executable that compiles the `.proto` files.
///
/// The binary is vendored, so the tools work on a machine that has no
/// protobuf compiler installed.
pub fn protoc_path() -> ProtoResult<PathBuf> {
    protoc_bin_vendored::protoc_bin_path().map_err(|error| ProtoError::Protoc {
        message: error.to_string(),
    })
}

/// The directory that holds the well known `.proto` files (`google/protobuf/...`).
pub fn protoc_include_path() -> ProtoResult<PathBuf> {
    protoc_bin_vendored::include_path().map_err(|error| ProtoError::Protoc {
        message: error.to_string(),
    })
}

/// Lists every `.proto` file in a directory, sorted by name.
pub fn proto_files_in(directory: &Path) -> ProtoResult<Vec<PathBuf>> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| ProtoError::io(directory.display().to_string(), error))?;

    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| ProtoError::io(directory.display().to_string(), error))?;
        let path = entry.path();

        if path.is_file()
            && path
                .extension()
                .map(|extension| extension.eq_ignore_ascii_case("proto"))
                .unwrap_or(false)
        {
            files.push(path);
        }
    }

    files.sort();

    Ok(files)
}

/// A compiled set of `.proto` files.
#[derive(Clone, Debug, Default)]
pub struct ProtoSet {
    /// Every compiled file, in the order `protoc` reported them.
    pub files: Vec<RawDescriptor>,
}

impl ProtoSet {
    /// Compiles the given `.proto` files into a descriptor set.
    ///
    /// `includes` are the directories that are searched for imported files; the
    /// directory of every compiled file is added automatically.
    pub fn compile(protos: &[PathBuf], includes: &[PathBuf]) -> ProtoResult<ProtoSet> {
        if protos.is_empty() {
            return Err(ProtoError::invalid_argument("no .proto files to compile"));
        }

        let protoc = protoc_path()?;
        let protoc_include = protoc_include_path()?;

        let temp_dir = std::env::temp_dir().join(format!(
            "steamkit-proto-gen-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&temp_dir)
            .map_err(|error| ProtoError::io(temp_dir.display().to_string(), error))?;
        let descriptor_path = temp_dir.join("descriptor.bin");

        let mut command = Command::new(&protoc);
        command.arg(format!("-I{}", protoc_include.display()));
        for include in includes {
            command.arg(format!("-I{}", include.display()));
        }
        for proto in protos {
            if let Some(parent) = proto.parent() {
                if !parent.as_os_str().is_empty() {
                    command.arg(format!("-I{}", parent.display()));
                }
            }
        }
        command.arg(format!(
            "--descriptor_set_out={}",
            descriptor_path.display()
        ));
        command.arg("--include_imports");
        for proto in protos {
            command.arg(proto);
        }

        let output = command.output().map_err(|error| ProtoError::Protoc {
            message: format!("unable to run '{}': {}", protoc.display(), error),
        })?;

        let descriptor_bytes = if output.status.success() {
            std::fs::read(&descriptor_path)
                .map_err(|error| ProtoError::io(descriptor_path.display().to_string(), error))?
        } else {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(ProtoError::Compile {
                file: protos
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                message: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        };

        let _ = std::fs::remove_dir_all(&temp_dir);

        Ok(ProtoSet {
            files: split_descriptor_set(&descriptor_bytes)?,
        })
    }

    /// Returns the descriptor of the file with the given virtual name.
    pub fn file(&self, name: &str) -> Option<&RawDescriptor> {
        self.files
            .iter()
            .find(|file| file.proto.name.as_deref() == Some(name))
    }

    /// Returns the descriptor of the file with the given path.
    ///
    /// The name may be the virtual name (`steammessages_base.proto`) or a
    /// path; only the file name part is compared.
    pub fn file_by_path(&self, path: &Path) -> Option<&RawDescriptor> {
        let file_name = path.file_name()?.to_string_lossy().into_owned();
        self.files
            .iter()
            .find(|file| file.proto.name.as_deref() == Some(file_name.as_str()))
    }

    /// Returns the file with the given name plus everything it imports.
    ///
    /// The result is ordered so that a file always comes after the files it
    /// imports, which is the order `protoc` reports them in.
    pub fn closure(&self, root: &str) -> ProtoResult<Vec<RawDescriptor>> {
        let by_name: HashMap<&str, &RawDescriptor> = self
            .files
            .iter()
            .filter_map(|file| file.proto.name.as_deref().map(|name| (name, file)))
            .collect();

        let mut visited: Vec<String> = Vec::new();
        let mut result: Vec<RawDescriptor> = Vec::new();

        collect(root, &by_name, &mut visited, &mut result)?;

        Ok(result)
    }
}

/// Splits a `FileDescriptorSet` into the wire bytes of its files.
///
/// The set is read on the wire level so that the bytes of every file are
/// preserved; `prost` would drop the fields it does not know.
fn split_descriptor_set(bytes: &[u8]) -> ProtoResult<Vec<RawDescriptor>> {
    let fields = wire::parse_message(bytes)?;

    let mut files = Vec::new();
    for field in fields {
        if field.number != 1 {
            continue;
        }

        if let Some(data) = field.value.as_bytes() {
            files.push(RawDescriptor::from_bytes(data.to_vec())?);
        }
    }

    Ok(files)
}

/// Depth first walk that appends a file after everything it imports.
fn collect(
    name: &str,
    by_name: &HashMap<&str, &RawDescriptor>,
    visited: &mut Vec<String>,
    result: &mut Vec<RawDescriptor>,
) -> ProtoResult<()> {
    if visited.iter().any(|seen| seen == name) {
        return Ok(());
    }

    let file = *by_name
        .get(name)
        .ok_or_else(|| ProtoError::NotFound { path: name.to_owned() })?;

    visited.push(name.to_owned());

    for dependency in &file.proto.dependency {
        collect(dependency, by_name, visited, result)?;
    }

    result.push(file.clone());

    Ok(())
}
