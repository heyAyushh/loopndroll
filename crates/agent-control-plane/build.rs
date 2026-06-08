use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=proto/looper/v1/control_plane.proto");

    let protoc_path = protoc_bin_vendored::protoc_bin_path()?;
    let mut prost_config = tonic_prost_build::Config::new();
    prost_config.protoc_executable(protoc_path);

    tonic_prost_build::configure().compile_with_config(
        prost_config,
        &["proto/looper/v1/control_plane.proto"],
        &["proto"],
    )?;

    Ok(())
}
