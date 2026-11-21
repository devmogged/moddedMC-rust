mod diff;
mod oracle;
mod packets;
mod process;
mod runner;
mod scenario;
mod snapshot;
#[cfg(test)]
mod tests;

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

use serde::Serialize;

use crate::{diff::Report, process::ServerProgram, runner::Options};

const USAGE: &str = "moddedmc-harness run <scenario> | run-all <directory> | self-test \
--reference <vanilla:jar|oracle:descriptor.json> [--subject <exe|vanilla:jar>] [--subject-mods <directory>] [--bot <exe>] [--run-root <dir>] [--jobs <n>] [--keep] [--json]\nmoddedmc-harness compare-biomes <reference-run-dir> <subject-run-dir> <x0> <y0> <z0> <x1> <y1> <z1>\nmoddedmc-harness compare-blocks <reference-region-dir> <subject-region-dir> <x0> <y0> <z0> <x1> <y1> <z1>";

#[expect(clippy::print_stderr, reason = "CLI diagnostics")]
fn main() -> ExitCode {
    match cli(&std::env::args().skip(1).collect::<Vec<_>>()) {
        Ok(result) => ExitCode::from(result),
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn cli(args: &[String]) -> Result<u8, String> {
    let mut args = args.iter().map(String::as_str);
    let mode = args.next().ok_or("missing subcommand")?;
    if matches!(mode, "--help" | "-h" | "help") {
        print_help();
        return Ok(0);
    }
    if mode == "compare-biomes" {
        return compare_biomes(args);
    }
    if mode == "compare-blocks" {
        return compare_blocks(args);
    }
    let path = match mode {
        "run" | "run-all" => Some(PathBuf::from(
            args.next().ok_or("missing scenario or directory")?,
        )),
        "self-test" => None,
        _ => return Err(format!("unknown subcommand {mode}")),
    };
    let flags = parse_flags(args)?;
    let json = flags.contains_key("--json");
    let reference = ServerProgram::parse(
        flags
            .get("--reference")
            .copied()
            .ok_or("--reference is required")?,
    )?;
    let subject = if mode == "self-test" {
        if !reference.is_vanilla() || flags.contains_key("--subject") {
            return Err("self-test uses the same vanilla --reference on both sides".into());
        }
        reference.clone()
    } else {
        ServerProgram::parse(
            flags
                .get("--subject")
                .copied()
                .ok_or("--subject is required")?,
        )?
    };
    if matches!(subject, ServerProgram::Oracle(_)) {
        return Err("oracle is a reference-only server program".into());
    }
    let subject_mods = subject_mods_for(flags.get("--subject-mods").copied(), &subject)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let sibling = exe.with_file_name(format!("moddedmc-bot{}", std::env::consts::EXE_SUFFIX));
    let bot = flags.get("--bot").map_or(sibling, PathBuf::from);
    let bot = std::path::absolute(bot).map_err(|e| e.to_string())?;
    let root = std::path::absolute(flags.get("--run-root").copied().unwrap_or("run/harness"))
        .map_err(|e| e.to_string())?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let available = thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let jobs = flags
        .get("--jobs")
        .map_or(Ok((available / 2).max(1)), |text| {
            text.parse::<usize>()
                .ok()
                .filter(|n| (1..=available).contains(n))
                .ok_or_else(|| format!("--jobs must be between 1 and {available}"))
        })?;
    reference.prepare_bundler()?;
    if subject != reference {
        subject.prepare_bundler()?;
    }
    let options = Options {
        reference,
        subject,
        subject_mods,
        bot,
        root,
        keep: flags.contains_key("--keep"),
    };
    if mode == "self-test" {
        let (reports, verified) = runner::self_test(&options);
        print_reports(&reports, json, Some(verified))?;
        return Ok(u8::from(!verified));
    }
    let path = path.ok_or("missing path")?;
    let reports = if mode == "run" {
        vec![runner::run(&path, &options)]
    } else {
        let mut paths = Vec::new();
        discover(&path, &mut paths)?;
        paths.sort();
        if paths.is_empty() {
            return Err("no .txt scenarios found".into());
        }
        parallel(&paths, jobs, &options)
    };
    print_reports(&reports, json, None)?;
    Ok(if reports.iter().any(|r| !r.differences.is_empty()) {
        1
    } else if reports.iter().any(|r| r.error.is_some()) {
        2
    } else {
        0
    })
}

#[expect(clippy::print_stdout, reason = "CLI biome parity report")]
fn compare_biomes<'a>(mut args: impl Iterator<Item = &'a str>) -> Result<u8, String> {
    let reference = args.next().ok_or("missing reference run directory")?;
    let subject = args.next().ok_or("missing subject run directory")?;
    let area = scenario::parse_area(&args.collect::<Vec<_>>().join(" "))?;
    let reference = snapshot::biomes(Path::new(reference), area)?;
    let subject = snapshot::biomes(Path::new(subject), area)?;
    let mut differences = 0;
    for (quart, name) in &reference {
        let actual = subject
            .get(quart)
            .ok_or("subject biome snapshot is incomplete")?;
        if name != actual {
            let [x, y, z] = quart;
            println!("biome @ quart {x} {y} {z}: {name} vs {actual}");
            differences += 1;
        }
    }
    println!(
        "{differences} differences across {} biome quarts",
        reference.len()
    );
    Ok(u8::from(differences != 0))
}

#[expect(clippy::print_stdout, reason = "CLI block parity report")]
fn compare_blocks<'a>(mut args: impl Iterator<Item = &'a str>) -> Result<u8, String> {
    let reference = args.next().ok_or("missing reference region directory")?;
    let subject = args.next().ok_or("missing subject region directory")?;
    let area = scenario::parse_area(&args.collect::<Vec<_>>().join(" "))?;
    let reference = snapshot::block_states(Path::new(reference), area)?;
    let subject = snapshot::block_states(Path::new(subject), area)?;
    let mut states = BTreeMap::new();
    let mut differences = 0;
    for x in area.min[0]..=area.max[0] {
        for z in area.min[2]..=area.max[2] {
            for y in area.min[1]..=area.max[1] {
                let position = [x, y, z];
                let expected = reference
                    .get(&position)
                    .map_or("minecraft:air", String::as_str);
                let actual = subject
                    .get(&position)
                    .map_or("minecraft:air", String::as_str);
                if expected == actual {
                    continue;
                }
                if differences < 20 {
                    println!("block @ {x} {y} {z}: {expected} vs {actual}");
                }
                differences += 1;
                *states.entry((expected, actual)).or_insert(0) += 1;
            }
        }
    }
    for ((expected, actual), count) in states {
        println!("{count} blocks: {expected} vs {actual}");
    }
    let count: i32 = (0..3).map(|i| area.max[i] - area.min[i] + 1).product();
    println!("{differences} differences across {count} blocks");
    Ok(u8::from(differences != 0))
}

fn subject_mods_for(
    value: Option<&str>,
    subject: &ServerProgram,
) -> Result<Option<PathBuf>, String> {
    value
        .map(|path| {
            if !matches!(subject, ServerProgram::Pumpkin(_)) {
                return Err("--subject-mods requires a pack executable".into());
            }
            let path = std::path::absolute(path).map_err(|error| error.to_string())?;
            if !path.is_dir() {
                return Err(format!("{} is not a mod directory", path.display()));
            }
            Ok(path)
        })
        .transpose()
}

fn parse_flags<'a>(
    mut args: impl Iterator<Item = &'a str>,
) -> Result<BTreeMap<&'a str, &'a str>, String> {
    let mut flags = BTreeMap::new();
    while let Some(flag) = args.next() {
        let value = match flag {
            "--json" | "--keep" => "",
            "--reference" | "--subject" | "--subject-mods" | "--bot" | "--run-root" | "--jobs" => {
                args.next()
                    .filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| format!("missing value for {flag}"))?
            }
            _ => return Err(format!("unknown flag {flag}")),
        };
        if flags.insert(flag, value).is_some() {
            return Err(format!("repeated flag {flag}"));
        }
    }
    Ok(flags)
}

fn discover(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if kind.is_dir() {
            discover(&path, paths)?;
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "txt") {
            paths.push(path);
        }
    }
    Ok(())
}

fn parallel(paths: &[PathBuf], jobs: usize, options: &Options) -> Vec<Report> {
    let next = AtomicUsize::new(0);
    let (send, receive) = mpsc::channel();
    thread::scope(|scope| {
        for _ in 0..jobs.min(paths.len()) {
            let send = send.clone();
            let next = &next;
            scope.spawn(move || {
                while let Some((index, path)) = {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    paths.get(index).map(|path| (index, path))
                } {
                    if send.send((index, runner::run(path, options))).is_err() {
                        break;
                    }
                }
            });
        }
    });
    drop(send);
    receive
        .into_iter()
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect()
}

#[derive(Serialize)]
struct Output<'a> {
    reports: &'a [Report],
    self_test_passed: Option<bool>,
}

#[expect(clippy::print_stdout, reason = "CLI usage text")]
fn print_help() {
    println!("{USAGE}");
}

#[expect(clippy::print_stdout, reason = "CLI diff and summaries")]
fn print_reports(
    reports: &[Report],
    json: bool,
    self_test_passed: Option<bool>,
) -> Result<(), String> {
    if json {
        let result = Output {
            reports,
            self_test_passed,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    for report in reports {
        println!("{}", report.scenario);
        for difference in &report.differences {
            let at = difference
                .position
                .map_or_else(String::new, |[x, y, z]| format!(" @ {x} {y} {z}"));
            let value = |value: Option<&str>| value.unwrap_or("<missing>").replace('\n', "\\n");
            println!(
                "  {}{at}: {} {} vs {} (line {})",
                difference.check,
                difference.field,
                value(difference.reference.as_deref()),
                value(difference.subject.as_deref()),
                difference.line
            );
        }
        for match_ in &report.allowed {
            println!(
                "  allowed line {}: {} ({})",
                match_.allow_line, match_.difference.field, match_.reason
            );
        }
        if let Some(error) = &report.error {
            println!("  error: {error}");
        }
        println!("  evidence: {}", report.directory);
    }
    println!("\nScenario | Differences | Allowed | Result");
    for report in reports {
        let result = if report.error.is_some() {
            "ERROR"
        } else if report.differences.is_empty() {
            "PASS"
        } else {
            "DIFFERENT"
        };
        println!(
            "{} | {} | {} | {result}",
            report.scenario,
            report.differences.len(),
            report.allowed.len()
        );
    }
    if let Some(passed) = self_test_passed {
        println!("Self-test {}", if passed { "PASS" } else { "FAIL" });
    }
    Ok(())
}
