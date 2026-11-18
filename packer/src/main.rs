//! `moddedmc-pack build <pack.toml> [--target <name>]... [--profile pack|release] [--force]`
//!
//! Builds every target in the pack (or the ones named) into
//! `target/packs/<id>/<target>/`: `pack-<id>[.exe]`, `mods/*.mmc` and `pack.lock`.

mod build;
mod manifest;

use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: moddedmc-pack build <pack.toml> [--target <name>]... [--profile pack|release] [--force]";

fn run(args: Vec<String>) -> Result<(), String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("build") => {}
        _ => return Err(USAGE.into()),
    }
    let mut pack_file = None;
    let mut targets = Vec::new();
    let mut profile = "pack".to_string();
    let mut force = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--target" => targets.push(args.next().ok_or(USAGE)?),
            "--profile" => profile = args.next().ok_or(USAGE)?,
            "--force" => force = true,
            _ if pack_file.is_none() && !arg.starts_with("--") => pack_file = Some(PathBuf::from(arg)),
            _ => return Err(format!("unknown argument {arg}\n{USAGE}")),
        }
    }
    if !matches!(profile.as_str(), "pack" | "release") {
        return Err(format!("profile must be pack or release, not {profile}"));
    }
    let pack_file = pack_file.ok_or(USAGE)?;
    build::build(&build::Options {
        pack_file,
        targets,
        profile,
        force,
    })
}
