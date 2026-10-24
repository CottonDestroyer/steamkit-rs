use std::path::{Path, PathBuf};
use std::process::Command;

use prost::Message;
use prost_types::FileDescriptorSet;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let proto_root = manifest_dir.join("proto");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    let protoc_include = protoc_bin_vendored::include_path().expect("vendored protoc include dir");

    println!("cargo:rerun-if-changed={}", proto_root.display());

    let manifest =
        std::fs::read_to_string(proto_root.join("manifest.txt")).expect("proto manifest.txt");

    for line in manifest.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let dir = parts.next().expect("proto set name");
        let files: Vec<String> = parts.map(str::to_string).collect();
        // `steam` is always built; the others only with their feature.
        if dir != "steam" && std::env::var_os(format!("CARGO_FEATURE_{}", dir.to_uppercase())).is_none() {
            continue;
        }
        let include_dir = proto_root.join(dir);
        compile_set(
            &protoc,
            &protoc_include,
            &include_dir,
            dir,
            &files,
            &out_dir,
        );
    }

    compile_set(
        &protoc,
        &protoc_include,
        &proto_root,
        "gc",
        &["gc.proto".to_string()],
        &out_dir,
    );
}

fn compile_set(
    protoc: &Path,
    protoc_include: &Path,
    include_dir: &Path,
    name: &str,
    files: &[String],
    out_dir: &Path,
) {
    let set_out = out_dir.join(name);
    std::fs::create_dir_all(&set_out).unwrap();
    let desc_path = out_dir.join(format!("{name}.desc"));

    let mut cmd = Command::new(protoc);
    cmd.arg(format!("-I{}", protoc_include.display()));
    cmd.arg(format!("-I{}", include_dir.display()));
    cmd.arg(format!("--descriptor_set_out={}", desc_path.display()));
    cmd.arg("--include_imports");
    for file in files {
        cmd.arg(include_dir.join(file));
    }

    let status = cmd.status().expect("protoc invocation");
    assert!(status.success(), "protoc failed for proto set {name}");

    let bytes = std::fs::read(&desc_path).unwrap();
    let fds = FileDescriptorSet::decode(bytes.as_slice()).unwrap();

    let mut config = prost_build::Config::new();
    config.out_dir(&set_out);
    config.default_package_filename(name);
    config.compile_fds(fds).unwrap();
}
