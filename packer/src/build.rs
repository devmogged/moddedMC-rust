//! Building a pack for its targets: the stock server, `.mmc` files and a `pack.lock`.

use std::{
    fmt::Write as _,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use sha2::{Digest, Sha256};

use crate::manifest::{self, Kind, Resolved, hex};

pub struct Options {
    pub pack_file: PathBuf,
    pub targets: Vec<String>,
    pub profile: String,
    pub force: bool,
}

const BUILDER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where a Rust target puts things and what its `.mmc` entry is called.
struct Platform {
    triple: String,
    os: &'static str,
    arch: String,
    lib_prefix: &'static str,
    lib_ext: &'static str,
    exe_ext: &'static str,
}

impl Platform {
    fn new(triple: &str) -> Result<Self, String> {
        let arch = triple.split('-').next().unwrap_or_default().to_string();
        let (os, lib_prefix, lib_ext, exe_ext) = if triple.contains("windows") {
            ("windows", "", "dll", ".exe")
        } else if triple.contains("linux") {
            ("linux", "lib", "so", "")
        } else if triple.contains("apple") {
            ("macos", "lib", "dylib", "")
        } else {
            return Err(format!("unsupported target {triple}"));
        };
        Ok(Self {
            triple: triple.to_string(),
            os,
            arch,
            lib_prefix,
            lib_ext,
            exe_ext,
        })
    }
}

pub fn build(options: &Options) -> Result<(), String> {
    let pack_file = fs::canonicalize(&options.pack_file)
        .map_err(|e| format!("can't find {}: {e}", options.pack_file.display()))?;
    // Windows canonical paths are `\\?\N:\...`, which cargo can't read in a manifest.
    let pack_file = pack_file
        .to_str()
        .and_then(|p| p.strip_prefix(r"\\?\"))
        .map_or(pack_file.clone(), PathBuf::from);
    let base = pack_file
        .parent()
        .ok_or("pack.toml has no folder")?
        .to_path_buf();
    let repo = find_repo(&base)?;
    let pack = manifest::load(&pack_file)?;
    let mods = pack
        .mods
        .iter()
        .map(|(id, spec)| manifest::resolve(&base, id, spec))
        .collect::<Result<Vec<_>, _>>()?;

    let names: Vec<String> = if options.targets.is_empty() {
        pack.targets.keys().cloned().collect()
    } else {
        options.targets.clone()
    };
    let host = host_triple(&repo)?;
    let lock = lock_text(&pack, &mods, &repo, &options.profile);
    // `.gitmodules` ignores changes inside the Pumpkin submodule, so ask it directly.
    let engine_dirty =
        git_dirty(&repo.join("engine")) || git_dirty(&repo.join("engine").join("pumpkin"));

    for name in &names {
        let triple = pack
            .targets
            .get(name)
            .ok_or_else(|| format!("the pack has no target {name}"))?;
        let platform = Platform::new(triple)?;
        let out = repo
            .join("target")
            .join("packs")
            .join(&pack.pack.id)
            .join(name);
        let binary = out.join(format!("pack-{}{}", pack.pack.id, platform.exe_ext));
        let key = hex(&Sha256::digest(format!("{lock}\n{triple}").as_bytes()));
        let key_file = out.join(".build-key");
        // A dirty engine tree can change the binary without changing the lock.
        if !options.force
            && !engine_dirty
            && binary.exists()
            && mods
                .iter()
                .all(|m| out.join("mods").join(format!("{}.mmc", m.id)).is_file())
            && fs::read_to_string(&key_file).is_ok_and(|k| k == key)
        {
            println!("{name}: up to date ({})", out.display());
            continue;
        }

        let started = Instant::now();
        let mods_dir = out.join("mods");
        if mods_dir.exists() {
            fs::remove_dir_all(&mods_dir)
                .map_err(|e| format!("can't clear {}: {e}", mods_dir.display()))?;
        }
        fs::create_dir_all(&mods_dir)
            .map_err(|e| format!("can't create {}: {e}", mods_dir.display()))?;

        for m in &mods {
            match &m.kind {
                Kind::DropIn { package, lib } => {
                    let library = build_drop_in(&repo, &host, &platform, package, lib)?;
                    write_mmc(
                        &repo,
                        &mods_dir.join(format!("{}.mmc", m.id)),
                        m,
                        &platform,
                        &library,
                    )?;
                }
                Kind::Prebuilt => {
                    fs::copy(&m.path, mods_dir.join(format!("{}.mmc", m.id)))
                        .map_err(|e| format!("{}: can't copy {}: {e}", m.id, m.path.display()))?;
                }
            }
        }

        let built = build_server(&repo, &host, &platform, &options.profile)?;
        fs::copy(&built, &binary).map_err(|e| {
            format!(
                "can't copy {} to {}: {e}",
                built.display(),
                binary.display()
            )
        })?;
        fs::write(out.join("pack.lock"), &lock).map_err(|e| e.to_string())?;
        fs::write(&key_file, &key).map_err(|e| e.to_string())?;
        println!(
            "{name}: built {} in {:.1}s ({} mods)",
            out.display(),
            started.elapsed().as_secs_f32(),
            mods.len()
        );
    }
    Ok(())
}

/// The moddedMC-rust checkout: the nearest folder above the pack with `engine/Cargo.toml`.
fn find_repo(start: &Path) -> Result<PathBuf, String> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join("engine").join("Cargo.toml").exists() && d.join("api").exists() {
            return Ok(d.to_path_buf());
        }
        dir = d.parent();
    }
    // Packs kept elsewhere build against the checkout the builder runs from.
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    if cwd.join("engine").join("Cargo.toml").exists() {
        return Ok(cwd);
    }
    Err("run this inside a moddedMC-rust checkout".into())
}

fn host_triple(repo: &Path) -> Result<String, String> {
    let output = Command::new("rustc")
        .arg("-vV")
        .current_dir(repo.join("engine"))
        .output()
        .map_err(|e| format!("can't run rustc: {e}"))?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .map(str::to_owned)
        .ok_or_else(|| "rustc didn't say its host target".into())
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_dirty(dir: &Path) -> bool {
    git(dir, &["status", "--porcelain", "--", "."]).is_none_or(|s| !s.is_empty())
}

fn lock_text(pack: &manifest::Pack, mods: &[Resolved], repo: &Path, profile: &str) -> String {
    let engine = repo.join("engine");
    let pumpkin =
        git(&engine.join("pumpkin"), &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let platform = git(repo, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# Written by moddedmc-pack {BUILDER_VERSION}. What this build contains, exactly."
    );
    let _ = writeln!(text, "[pack]");
    let _ = writeln!(text, "id = {:?}", pack.pack.id);
    let _ = writeln!(text, "name = {:?}", pack.pack.name);
    let _ = writeln!(text, "version = {:?}", pack.pack.version);
    let _ = writeln!(text, "profile = {profile:?}");
    let _ = writeln!(text, "\n[engine]");
    let _ = writeln!(text, "pumpkin = {pumpkin:?}");
    let _ = writeln!(text, "moddedmc = {platform:?}");
    for m in mods {
        let _ = writeln!(text, "\n[[mod]]");
        let _ = writeln!(text, "id = {:?}", m.id);
        let _ = writeln!(text, "version = {:?}", m.version);
        let _ = writeln!(text, "kind = {:?}", "drop-in");
        let _ = writeln!(text, "sha256 = {:?}", m.hash);
    }
    text
}

/// `cargo build` for the host, `cargo zigbuild --target` for anything else (the Pi).
fn cargo(dir: &Path, host: &str, platform: &Platform) -> Command {
    let mut command = Command::new("cargo");
    command.current_dir(dir);
    if platform.triple == host {
        command.arg("build");
    } else {
        command.args(["zigbuild", "--target", &platform.triple]);
    }
    command
}

fn run(mut command: Command, what: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|e| format!("can't run cargo for {what}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("building {what} failed ({status})"))
    }
}

fn target_dir(root: &Path, host: &str, platform: &Platform, profile: &str) -> PathBuf {
    let target = root.join("target");
    if platform.triple == host {
        target.join(profile)
    } else {
        target.join(&platform.triple).join(profile)
    }
}

fn build_drop_in(
    repo: &Path,
    host: &str,
    platform: &Platform,
    package: &str,
    lib: &str,
) -> Result<PathBuf, String> {
    let mut command = cargo(repo, host, platform);
    command.args(["--release", "-p", package]);
    run(command, package)?;
    let library = target_dir(repo, host, platform, "release")
        .join(format!("{}{lib}.{}", platform.lib_prefix, platform.lib_ext));
    if library.exists() {
        Ok(library)
    } else {
        Err(format!(
            "{package} built, but {} is missing",
            library.display()
        ))
    }
}

/// Folders under a mod crate's `resources/` that ship in its `.mmc` under the same name.
const RESOURCE_FOLDERS: [&str; 2] = ["data", "assets"];

/// A `.mmc` like `scripts/pack-mod.ps1` makes: `mod.json`, the library for this platform and the
/// crate's `resources/data/` and `resources/assets/` as `data/` and `assets/`.
fn write_mmc(
    repo: &Path,
    out: &Path,
    m: &Resolved,
    platform: &Platform,
    library: &Path,
) -> Result<(), String> {
    let abi_source = fs::read_to_string(repo.join("api").join("src").join("abi.rs"))
        .map_err(|e| e.to_string())?;
    let abi: u32 = abi_source
        .split("ABI_VERSION: u32 = ")
        .nth(1)
        .and_then(|rest| rest.split(';').next())
        .and_then(|n| n.trim().parse().ok())
        .ok_or("can't find ABI_VERSION in api/src/abi.rs")?;
    let manifest = serde_json::json!({ "id": m.id, "version": m.version, "abi": abi });
    let file = fs::File::create(out).map_err(|e| format!("can't write {}: {e}", out.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let library_bytes = fs::read(library).map_err(|e| e.to_string())?;
    let entry = format!("{}-{}.{}", platform.os, platform.arch, platform.lib_ext);
    let mut resources = Vec::new();
    for folder in RESOURCE_FOLDERS {
        let dir = m.path.join("resources").join(folder);
        if dir.is_dir() {
            let mut files = Vec::new();
            manifest::collect(&dir, &dir, &mut files)?;
            files.sort();
            resources.extend(
                files
                    .into_iter()
                    .map(|relative| (folder, dir.join(&relative), relative)),
            );
        }
    }
    let written = (|| -> zip::result::ZipResult<()> {
        zip.start_file("mod.json", options)?;
        zip.write_all(manifest.to_string().as_bytes())?;
        zip.start_file(entry, options)?;
        zip.write_all(&library_bytes)?;
        for (folder, source, relative) in &resources {
            zip.start_file(format!("{folder}/{relative}"), options)?;
            zip.write_all(&fs::read(source)?)?;
        }
        zip.finish()?;
        Ok(())
    })();
    written.map_err(|e| format!("can't write {}: {e}", out.display()))
}

/// All packs use the same server for a given target and profile.
fn build_server(
    repo: &Path,
    host: &str,
    platform: &Platform,
    profile: &str,
) -> Result<PathBuf, String> {
    let engine = repo.join("engine");
    let mut command = cargo(&engine, host, platform);
    command.args(["-p", "pack-dev", "--profile", profile]);
    run(command, "pack-dev")?;
    let binary =
        target_dir(&engine, host, platform, profile).join(format!("pack-dev{}", platform.exe_ext));
    if binary.exists() {
        Ok(binary)
    } else {
        Err(format!(
            "pack-dev built, but {} is missing",
            binary.display()
        ))
    }
}
