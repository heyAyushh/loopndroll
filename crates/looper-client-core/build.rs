use std::{env, error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let repo_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .ok_or("failed to resolve repo root")?;
    let proto_dir = repo_root.join("crates/agent-control-plane/proto");
    let proto_file = proto_dir.join("looper/v1/control_plane.proto");

    println!("cargo:rerun-if-changed={}", proto_file.display());

    let protoc_path = protoc_bin_vendored::protoc_bin_path()?;
    let mut prost_config = tonic_prost_build::Config::new();
    prost_config.protoc_executable(protoc_path);

    tonic_prost_build::configure().compile_with_config(
        prost_config,
        &[proto_file],
        &[proto_dir],
    )?;

    Ok(())
}
