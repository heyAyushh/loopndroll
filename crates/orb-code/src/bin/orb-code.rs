use std::{fs, path::PathBuf};

use clap::{Parser, Subcommand};
use orb_code::{
    GenerateOrbRequest, OrbError, OrbId, derive_orb_id, generate_orb_image, scan_orb_image,
    verify_orb_image,
};

#[derive(Debug, Parser)]
#[command(name = "orb-code")]
#[command(about = "Generate and scan deterministic orb codes")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Id {
        #[arg(long)]
        data: String,
    },
    Generate {
        #[arg(long, conflicts_with = "data")]
        id: Option<String>,
        #[arg(long, conflicts_with = "id")]
        data: Option<String>,
        #[arg(long, default_value_t = 1024)]
        size: u32,
        #[arg(long)]
        out: PathBuf,
    },
    Scan {
        image: PathBuf,
    },
    Verify {
        image: PathBuf,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), OrbError> {
    let cli = Cli::parse();
    match cli.command {
        Command::Id { data } => run_id(data),
        Command::Generate {
            id,
            data,
            size,
            out,
        } => run_generate(id, data, size, out),
        Command::Scan { image } => run_scan(image),
        Command::Verify { image } => run_verify(image),
    }
}

fn run_id(data: String) -> Result<(), OrbError> {
    println!("{}", derive_orb_id(&data));
    Ok(())
}

fn run_generate(
    id: Option<String>,
    data: Option<String>,
    size: u32,
    output_path: PathBuf,
) -> Result<(), OrbError> {
    let orb_id = match (id, data) {
        (Some(raw_id), None) => OrbId::parse(raw_id)?,
        (None, Some(raw_data)) => derive_orb_id(&raw_data),
        _ => return Err(OrbError::GenerateInputRequired),
    };

    let request = GenerateOrbRequest::new(orb_id.clone()).with_image_size(size)?;
    let orb_image = generate_orb_image(&request)?;
    orb_image.save_png(output_path)?;
    println!("{}", orb_image.orb_id);
    Ok(())
}

fn run_scan(image_path: PathBuf) -> Result<(), OrbError> {
    let image_bytes = fs::read(image_path)?;
    let scan_result = scan_orb_image(&image_bytes)?;
    println!("{}", scan_result.orb_id);
    Ok(())
}

fn run_verify(image_path: PathBuf) -> Result<(), OrbError> {
    let image_bytes = fs::read(image_path)?;
    let verification_result = verify_orb_image(&image_bytes)?;
    if !verification_result.is_match {
        return Err(OrbError::VerificationFailed {
            distance: verification_result.distance,
            threshold: verification_result.threshold,
        });
    }

    println!(
        "{} verified (distance {} <= {})",
        verification_result.orb_id, verification_result.distance, verification_result.threshold
    );
    Ok(())
}
