//! Code generation and analysis tools for steamkit.
//!
//! * [`tokenizer`], [`analyzer`] and [`generator`] parse Valve's
//!   SteamLanguage definition files (`.steamd`) and generate Rust code from
//!   them (the `steamkit-lang-gen` binary). This part has no third-party
//!   dependencies.
//! * [`proto_gen`] turns `.proto` files into `prost` types
//!   (`steamkit-proto-gen`).
//! * [`proto_dumper`] extracts protobuf definitions from Steam binaries
//!   (`steamkit-proto-dump`).
//! * [`nethook`] reads NetHook2 capture dumps
//!   (`steamkit-nethook-analyzer`).

pub mod analyzer;
pub mod error;
pub mod generator;
pub mod nethook;
pub mod proto_dumper;
pub mod proto_error;
pub mod proto_gen;
pub mod tokenizer;

use std::path::{Path, PathBuf};

pub use analyzer::{analyze, ClassNode, EnumNode, Member, Node, Root, TypeRef};
pub use error::{LangError, LangResult};
pub use generator::Generator;
pub use proto_error::{ProtoError, ProtoResult};
pub use tokenizer::{tokenize, Token, TokenKind};

/// Parses a definition file (and everything it imports) and returns the root
/// node.
///
/// `language_dir` is the directory that `#import` paths are resolved against.
pub fn parse_file(entry: &Path, language_dir: &Path) -> LangResult<Root> {
    let text = std::fs::read_to_string(entry).map_err(|source| LangError::Io {
        path: entry.display().to_string(),
        source,
    })?;
    let tokens = tokenize(&text, &entry.display().to_string())?;
    analyze(&tokens, language_dir)
}

/// The directory that holds the copies of the Valve definition files.
pub fn default_language_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("lang")
}

/// The default output file: the generated module of the `steamkit` crate.
pub fn default_output_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("steamkit")
        .join("src")
        .join("base")
        .join("generated")
        .join("steam_language.rs")
}
