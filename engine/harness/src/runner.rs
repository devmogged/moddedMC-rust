use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use crate::{
    diff::{Check, Report},
    packets,
    process::{Process, Server, ServerProgram},
    scenario::{self, Action, Area, Scenario, Step},
    snapshot,
};

const BOT_TIMEOUT: Duration = Duration::from_secs(75);
const SELF_TEST: &str = "seed 12345\narea 0 150 0 0 150 0\nstart\ncmd setblock 0 150 0 minecraft:stone\ntick 2\nsnapshot planted\nstop\n";

pub struct Options {
    pub reference: ServerProgram,
    pub subject: ServerProgram,
    pub bot: PathBuf,
    pub root: PathBuf,
    pub keep: bool,
}

pub fn run(path: &Path, options: &Options) -> Report {
    let mut report = Report {
        scenario: path.display().to_string(),
        ..Report::default()
    };
    let result = fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|source| scenario::parse(&source))
        .and_then(|scenario| execute(&scenario, options, &mut report, false));
    report.error = result.err();
    let remove = !options.keep && report.error.is_none() && report.differences.is_empty();
    finalize(&mut report, remove);
    report
}

pub fn self_test(options: &Options) -> ([Report; 2], bool) {
    let mut reports = [false, true].map(|perturb| {
        let mut report = Report {
            scenario: if perturb {
                "self-test deliberately changed block"
            } else {
                "self-test identical vanilla"
            }
            .into(),
            ..Report::default()
        };
        report.error = scenario::parse(SELF_TEST)
            .and_then(|scenario| execute(&scenario, options, &mut report, perturb))
            .err();
        report
    });
    let [same, different] = &reports;
    let passed = same.error.is_none()
        && same.differences.is_empty()
        && different.error.is_none()
        && matches!(different.differences.as_slice(), [d]
            if d.line == 6 && d.field == "block" && d.position == Some([0.0, 150.0, 0.0])
                && d.reference.as_deref() == Some("minecraft:stone")
                && d.subject.as_deref() == Some("minecraft:dirt"));
    for report in &mut reports {
        finalize(report, passed && !options.keep);
    }
    let passed = passed && reports.iter().all(|report| report.error.is_none());
    (reports, passed)
}

fn finalize(report: &mut Report, remove: bool) {
    if report.directory.is_empty() {
        return;
    }
    let dir = PathBuf::from(&report.directory);
    let json_path = dir.join("report.json");
    if let Err(error) = save_report(report, &json_path) {
        let reason = report
            .error
            .take()
            .map_or_else(String::new, |reason| format!("{reason}. "));
        report.error = Some(format!("{reason}{error}"));
        return;
    }
    if remove && let Err(error) = remove_checkpoints(&dir) {
        report.error = Some(format!(
            "removing checkpoints from {}: {error}",
            dir.display()
        ));
        if let Err(error) = save_report(report, &json_path)
            && let Some(reason) = &mut report.error
        {
            reason.push_str(". ");
            reason.push_str(&error);
        }
    }
}

fn save_report(report: &Report, path: &Path) -> Result<(), String> {
    let parent = path.parent().ok_or("report has no parent directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("writing {}: {error}", path.display()))?;
    serde_json::to_writer_pretty(file.as_file_mut(), report).map_err(|error| error.to_string())?;
    file.persist(path)
        .map(|_| ())
        .map_err(|error| format!("saving {}: {error}", path.display()))
}

fn remove_checkpoints(dir: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .starts_with("checkpoint-")
            && entry.file_type()?.is_dir()
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

fn execute(
    scenario: &Scenario,
    options: &Options,
    report: &mut Report,
    perturb: bool,
) -> Result<(), String> {
    let dir = tempfile::Builder::new()
        .prefix("scenario-")
        .tempdir_in(&options.root)
        .map_err(|error| error.to_string())?
        .keep();
    report.directory = dir.display().to_string();
    let mut ends = scenario
        .steps
        .iter()
        .enumerate()
        .filter_map(|(i, step)| matches!(step.action, Action::Snapshot { .. }).then_some(i))
        .collect::<Vec<_>>();
    let stop = scenario.steps.len() - 1;
    if ends.last().is_none_or(|end| end + 1 < stop) {
        ends.push(stop);
    }
    let mut compare_from = 0;
    for (round, end) in ends.into_iter().enumerate() {
        let checkpoint = dir.join(format!("checkpoint-{}", round + 1));
        fs::create_dir(&checkpoint).map_err(|e| e.to_string())?;
        let mut pair = Pair::start(scenario, options, &checkpoint)
            .map_err(|e| format!("line {}: {e}", scenario.steps[0].line))?;
        for (i, step) in scenario.steps[1..=end].iter().enumerate() {
            let i = i + 1;
            let mut check = Check {
                line: step.line,
                name: label(step),
                report,
                allows: &scenario.allows,
            };
            if let Err(error) = pair.step(step, &mut check, i >= compare_from, i == end, perturb) {
                let _ = pair.stop();
                return Err(format!("line {}: {error}", step.line));
            }
        }
        compare_from = end + 1;
    }
    Ok(())
}

fn label(step: &Step) -> String {
    match &step.action {
        Action::Snapshot { name, .. } => format!("snapshot {name}"),
        Action::Probe(command) => format!("probe {command}"),
        Action::Command(command) => format!("cmd {command}"),
        Action::Expect(regex) => format!("expect {regex}"),
        _ => "step".into(),
    }
}

struct Pair {
    reference: Server,
    subject: Server,
    bots: BTreeMap<String, ([Bot; 2], String)>,
    elapsed: u64,
    area: Area,
    bot_exe: PathBuf,
    capture_packets: bool,
}

impl Pair {
    fn start(scenario: &Scenario, options: &Options, dir: &Path) -> Result<Self, String> {
        let (reference, subject) = std::thread::scope(|scope| {
            let reference =
                scope.spawn(|| Server::start(&options.reference, scenario, dir.join("reference")));
            let subject = Server::start(&options.subject, scenario, dir.join("subject"));
            (
                reference
                    .join()
                    .map_err(|_| "reference startup thread panicked"),
                subject,
            )
        });
        let mut reference = reference??;
        let mut subject = subject?;
        std::thread::scope(|scope| {
            let prep = scope.spawn(|| reference.prepare(scenario));
            let result = subject.prepare(scenario);
            prep.join()
                .map_err(|_| "reference setup thread panicked")??;
            result
        })?;
        Ok(Self {
            reference,
            subject,
            bots: BTreeMap::new(),
            elapsed: 0,
            area: scenario.area,
            bot_exe: options.bot.clone(),
            capture_packets: scenario
                .steps
                .iter()
                .any(|step| matches!(step.action, Action::Snapshot { packets: true, .. })),
        })
    }

    fn step(
        &mut self,
        step: &Step,
        check: &mut Check<'_>,
        compare: bool,
        last: bool,
        perturb: bool,
    ) -> Result<(), String> {
        match &step.action {
            Action::Start => Err("unexpected second start".into()),
            Action::Stop => self.stop(),
            Action::Tick(count) => self.tick(*count),
            Action::Command(command) => self.command(command, check, compare),
            Action::Expect(regex) => {
                self.expect(regex, check, compare);
                Ok(())
            }
            Action::Probe(command) => self.probe(command, check, compare),
            Action::Join(name) => self.join(name),
            Action::Bot { name, input } => self.bot(name, input),
            Action::Snapshot { packets, .. } => self.capture(check, last, perturb, *packets),
        }
    }

    fn tick(&mut self, count: u32) -> Result<(), String> {
        self.check_clock()?;
        self.reference.begin_step(count)?;
        self.subject.begin_step(count)?;
        self.elapsed += u64::from(count);
        self.reference.finish_step(self.elapsed, count)?;
        self.subject.finish_step(self.elapsed, count)
    }

    fn check_clock(&mut self) -> Result<(), String> {
        self.reference.verify_clock(self.elapsed)?;
        self.subject.verify_clock(self.elapsed)
    }

    fn command(
        &mut self,
        command: &str,
        check: &mut Check<'_>,
        compare: bool,
    ) -> Result<(), String> {
        let a = self.reference.command(command)?;
        let b = self.subject.command(command)?;
        if compare && (rejection(&a) || rejection(&b)) {
            check.add(
                None,
                "command.rejected".into(),
                Some(a.join("\n")),
                Some(b.join("\n")),
            );
        }
        Ok(())
    }

    fn probe(&mut self, command: &str, check: &mut Check<'_>, compare: bool) -> Result<(), String> {
        let a = self.reference.command(command)?;
        let b = self.subject.command(command)?;
        if compare && (a != b || a.is_empty() || rejection(&a) || rejection(&b)) {
            check.add(None, "probe.output".into(), as_output(&a), as_output(&b));
        }
        Ok(())
    }

    fn expect(&mut self, regex: &regex::Regex, check: &mut Check<'_>, compare: bool) {
        let a = expect(&mut self.reference, regex);
        let b = expect(&mut self.subject, regex);
        if compare && (a.is_none() || b.is_none()) {
            check.add(None, "expect.match".into(), a, b);
        }
    }

    fn join(&mut self, name: &str) -> Result<(), String> {
        let a = Bot::join(&self.bot_exe, &self.reference, name, self.capture_packets)?;
        let b = Bot::join(&self.bot_exe, &self.subject, name, self.capture_packets)?;
        self.bots
            .insert(name.to_ascii_lowercase(), ([a, b], name.into()));
        self.check_clock()
    }

    fn bot(&mut self, name: &str, input: &str) -> Result<(), String> {
        let (bots, _) = self
            .bots
            .get_mut(&name.to_ascii_lowercase())
            .ok_or_else(|| format!("unknown bot {name}"))?;
        bots.iter_mut().try_for_each(|bot| bot.action(input))?;
        self.check_clock()
    }

    fn capture(
        &mut self,
        check: &mut Check<'_>,
        last: bool,
        perturb: bool,
        compare_packets: bool,
    ) -> Result<(), String> {
        self.check_clock()?;
        if compare_packets && self.bots.is_empty() {
            return Err("packet snapshot needs at least one joined bot".into());
        }
        if self.capture_packets {
            for ([a, b], name) in self.bots.values_mut() {
                let a = a.packet_window(&mut self.reference, name, check.line)?;
                let b = b.packet_window(&mut self.subject, name, check.line)?;
                if compare_packets && last {
                    packets::compare(check, name, &a, &b);
                }
            }
        }
        if perturb && last {
            self.subject
                .require("setblock 0 150 0 minecraft:dirt", "Changed the block")?;
        }
        self.reference.flush()?;
        self.subject.flush()?;
        self.check_clock()?;
        if !last {
            return Ok(());
        }
        for (_, name) in self.bots.values() {
            let a = snapshot::player(&self.reference.dir, name, self.reference.is_vanilla())?;
            let b = snapshot::player(&self.subject.dir, name, self.subject.is_vanilla())?;
            check.compounds(None, &format!("player.{name}"), &a, &b);
        }
        self.stop()?;
        check.world(
            snapshot::world(&self.reference.dir, self.area)?,
            snapshot::world(&self.subject.dir, self.area)?,
        );
        Ok(())
    }

    fn stop(&mut self) -> Result<(), String> {
        let sent = [&mut self.reference, &mut self.subject].map(Server::request_stop);
        let finished = [&mut self.reference, &mut self.subject].map(Server::finish_stop);
        sent.into_iter().chain(finished).collect()
    }
}

fn expect(server: &mut Server, regex: &regex::Regex) -> Option<String> {
    let result = server
        .history
        .iter()
        .enumerate()
        .skip(server.expect_cursor)
        .find(|(_, line)| regex.is_match(line))
        .map(|(index, line)| (index, line.clone()));
    server.expect_cursor = result.as_ref().map_or(server.history.len(), |(i, _)| i + 1);
    result.map(|(_, line)| line)
}

fn as_output(lines: &[String]) -> Option<String> {
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn rejection(lines: &[String]) -> bool {
    lines.iter().any(|line| {
        line.contains("<--[HERE]")
            || [
                "Unknown or incomplete command",
                "Incorrect argument",
                "Unknown command",
                "That position is not loaded",
                "No entity was found",
                "No player was found",
                "Invalid argument",
                "Could not set the block",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
    })
}

struct Bot {
    process: Process,
    sequence: usize,
    packets: packets::Decoder,
}

impl Bot {
    fn join(exe: &Path, server: &Server, name: &str, capture: bool) -> Result<Self, String> {
        let mut command = Command::new(exe);
        command
            .arg(format!("127.0.0.1:{}", server.port))
            .arg(name)
            .env_remove("MODDEDMC_BOT_COMPANION")
            .env_remove("MODDEDMC_BOT_TRACE")
            .env("MODDEDMC_BOT_HARNESS", "1")
            .env_remove("MODDEDMC_BOT_CAPTURE");
        if capture {
            command.env("MODDEDMC_BOT_CAPTURE", "1");
        }
        let process = Process::spawn(command, &server.dir, &format!("bot-{name}"))?;
        process.until("bot join", BOT_TIMEOUT, |line| {
            line.starts_with(&format!("[bot] joined as {name} at "))
        })?;
        let mut bot = Self {
            process,
            sequence: 0,
            packets: packets::Decoder::default(),
        };
        // The ordered acknowledgement proves the server processed the join teleport.
        bot.fence()
            .map_err(|e| format!("waiting for {name} to load: {e}"))?;
        Ok(bot)
    }

    fn packet_window(
        &mut self,
        server: &mut Server,
        name: &str,
        index: usize,
    ) -> Result<Vec<packets::Packet>, String> {
        let marker = format!("moddedmc_harness_packet_fence_{index}");
        let feedback = server.command(&format!(r#"tellraw {name} {{"text":"{marker}"}}"#))?;
        if rejection(&feedback) {
            return Err(format!(
                "packet fence for {name}: {}",
                feedback.join(
                    "
"
                )
            ));
        }
        let ack = format!("[bot] {marker}");
        self.process
            .until("client packet fence", BOT_TIMEOUT, |line| line == ack)?;
        let path = server.dir.join(format!("bot-{name}.packets"));
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        self.packets
            .window(&bytes)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    fn action(&mut self, input: &str) -> Result<(), String> {
        if input == "await-teleport" {
            self.process
                .until("teleport confirmation", BOT_TIMEOUT, |line| {
                    line.starts_with("[bot] teleported to ")
                })?;
            return Ok(());
        }
        let verb = input.split_whitespace().next().unwrap_or_default();
        if matches!(verb, "move" | "look" | "attack" | "inventory-click") {
            if matches!(verb, "attack" | "inventory-click") {
                self.fence()?;
            }
            self.send_and_wait(input, None)?;
            return self.fence();
        }
        self.block_action(input)
    }

    fn fence(&mut self) -> Result<(), String> {
        // This position is beyond reach anywhere inside the world border.
        self.block_action("break 33554431 2047 33554431")
    }

    fn block_action(&mut self, input: &str) -> Result<(), String> {
        self.sequence += 1;
        self.send_and_wait(input, Some(self.sequence))
    }

    fn send_and_wait(&mut self, input: &str, sequence: Option<usize>) -> Result<(), String> {
        let ack = sequence.map(|sequence| format!("[bot] block ack {sequence}"));
        let sent = format!("[bot] did: {input}");
        self.process.send(input)?;
        let (mut received_ack, mut received_sent) = (ack.is_none(), false);
        let mut failure = None;
        self.process.until(input, BOT_TIMEOUT, |line| {
            if line.starts_with("[bot] error:") {
                failure = Some(line.to_owned());
                return true;
            }
            received_ack |= ack.as_deref() == Some(line);
            received_sent |= line == sent;
            received_ack && received_sent
        })?;
        failure.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_keeps_json_and_can_preserve_checkpoints() -> Result<(), String> {
        let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
        let world = scratch.path().join("checkpoint-1/reference/world");
        fs::create_dir_all(&world).map_err(|error| error.to_string())?;

        let mut report = Report {
            scenario: "cleanup test".into(),
            directory: scratch.path().display().to_string(),
            ..Report::default()
        };
        finalize(&mut report, false);
        assert!(report.error.is_none());
        assert!(world.is_dir());

        finalize(&mut report, true);
        assert!(report.error.is_none());
        assert!(!world.exists());
        let json =
            fs::read(scratch.path().join("report.json")).map_err(|error| error.to_string())?;
        let json: serde_json::Value =
            serde_json::from_slice(&json).map_err(|error| error.to_string())?;
        assert_eq!(json["scenario"], "cleanup test");
        Ok(())
    }
}
