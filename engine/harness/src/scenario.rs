use std::collections::{BTreeMap, BTreeSet};

use glob::Pattern;
use regex::Regex;

use crate::oracle;

pub type Position = [i32; 3];

#[derive(Clone, Copy, Debug)]
pub struct Area {
    pub min: Position,
    pub max: Position,
}

impl Area {
    pub fn contains(self, pos: Position) -> bool {
        (0..3).all(|i| (self.min[i]..=self.max[i]).contains(&pos[i]))
    }

    pub fn contains_entity(self, pos: [f64; 3]) -> bool {
        (0..3).all(|i| pos[i] >= f64::from(self.min[i]) && pos[i] < f64::from(self.max[i]) + 1.0)
    }

    pub fn chunks(self) -> impl Iterator<Item = (i32, i32)> {
        (self.min[0] >> 4..=self.max[0] >> 4)
            .flat_map(move |x| (self.min[2] >> 4..=self.max[2] >> 4).map(move |z| (x, z)))
    }
}

#[derive(Debug)]
pub enum Action {
    Start,
    Command(String),
    Tick(u32),
    Join(String),
    Bot { name: String, input: String },
    Snapshot { name: String, packets: bool },
    Expect(Regex),
    Probe(String),
    Stop,
}

#[derive(Debug)]
pub struct Step {
    pub line: usize,
    pub action: Action,
}

pub struct Allow {
    pub pattern: Pattern,
    pub reason: String,
    pub line: usize,
}

pub struct Scenario {
    pub seed: i64,
    pub area: Area,
    pub properties: BTreeMap<String, String>,
    pub ids: BTreeMap<String, String>,
    pub steps: Vec<Step>,
    pub allows: Vec<Allow>,
}

#[derive(Default)]
struct Parser {
    seed: Option<i64>,
    area: Option<Area>,
    properties: BTreeMap<String, String>,
    ids: BTreeMap<String, String>,
    explicit_rules: bool,
    started: bool,
    stopped: bool,
    bots: BTreeSet<String>,
    snapshots: BTreeSet<String>,
    steps: Vec<Step>,
    allows: Vec<Allow>,
}

pub fn parse(text: &str) -> Result<Scenario, String> {
    let mut parser = Parser::default();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let (code, comment) = uncomment(raw);
        if !code.is_empty() {
            parser
                .line(code, comment, line)
                .map_err(|error| format!("line {line}: {error}"))?;
        }
    }
    if !parser.stopped {
        return Err("scenario must end with stop".into());
    }
    if !parser.steps.iter().any(|step| {
        matches!(
            step.action,
            Action::Snapshot { .. } | Action::Probe(_) | Action::Expect(_)
        )
    }) {
        return Err("scenario must contain at least one snapshot, probe, or expect".into());
    }
    Ok(Scenario {
        seed: parser.seed.unwrap_or(12345),
        area: parser.area.ok_or("scenario requires an area")?,
        properties: parser.properties,
        ids: parser.ids,
        steps: parser.steps,
        allows: parser.allows,
    })
}

impl Parser {
    fn line(&mut self, code: &str, comment: &str, line: usize) -> Result<(), String> {
        if self.stopped {
            return Err("step after stop".into());
        }
        let (verb, rest) = split(code);
        if verb == "allow" {
            if comment.is_empty() || rest.is_empty() {
                return Err("allow needs a field glob and an inline reason".into());
            }
            self.allows.push(Allow {
                pattern: Pattern::new(rest).map_err(|error| error.to_string())?,
                reason: comment.into(),
                line,
            });
            return Ok(());
        }
        if !self.started && verb != "start" {
            return self.setup(verb, rest);
        }
        let action = match verb {
            "start" if !self.started && rest.is_empty() => {
                if self.area.is_none() {
                    return Err("define area before start".into());
                }
                self.started = true;
                Action::Start
            }
            "stop" if rest.is_empty() => {
                self.stopped = true;
                Action::Stop
            }
            "cmd" => Action::Command(command(rest)?),
            "probe" => Action::Probe(probe(rest)?),
            "tick" => Action::Tick(
                rest.parse::<u32>()
                    .ok()
                    .filter(|n| (1..=10_000).contains(n))
                    .ok_or("tick requires an integer from 1 to 10000")?,
            ),
            "expect" if !rest.is_empty() => {
                Action::Expect(Regex::new(rest).map_err(|error| error.to_string())?)
            }
            "snapshot" => {
                let (name, suffix) = split(rest);
                if !identifier(name)
                    || !matches!(suffix, "" | "packets")
                    || !self.snapshots.insert(name.into())
                {
                    return Err("snapshot requires a unique name and optional 'packets'".into());
                }
                Action::Snapshot {
                    name: name.into(),
                    packets: suffix == "packets",
                }
            }
            "bot" => self.bot(rest)?,
            _ => return Err(format!("invalid step '{code}'")),
        };
        self.steps.push(Step { line, action });
        Ok(())
    }

    fn setup(&mut self, verb: &str, rest: &str) -> Result<(), String> {
        match verb {
            "seed" if self.seed.is_none() => {
                self.seed = Some(
                    rest.parse()
                        .map_err(|_| "seed must be a signed 64-bit integer")?,
                );
            }
            "area" if self.area.is_none() => self.area = Some(parse_area(rest)?),
            "gamerules" if rest == "deterministic" && !self.explicit_rules => {
                self.explicit_rules = true;
            }
            "idmap" => {
                let (subject, reference) = split(rest);
                if !oracle::resource_id(subject) || !oracle::resource_id(reference) {
                    return Err("idmap requires subject and reference resource IDs".into());
                }
                if self.ids.contains_key(subject) || self.ids.values().any(|old| old == reference) {
                    return Err("idmap entries must be one to one".into());
                }
                self.ids.insert(subject.into(), reference.into());
            }
            "properties" => {
                let (key, value) = rest
                    .split_once('=')
                    .ok_or("properties requires key=value")?;
                let (key, value) = (key.trim(), value.trim());
                if !valid_property(key, value)
                    || self.properties.insert(key.into(), value.into()).is_some()
                {
                    return Err(format!(
                        "unsupported, invalid, or repeated property '{key}'"
                    ));
                }
            }
            _ => return Err(format!("invalid or repeated setup directive '{verb}'")),
        }
        Ok(())
    }

    fn bot(&mut self, rest: &str) -> Result<Action, String> {
        let (name, input) = split(rest);
        if name == "join" {
            if !identifier(input)
                || input.contains('-')
                || input.len() > 16
                || !self.bots.insert(input.to_ascii_lowercase())
            {
                return Err("bot join requires a unique Java name of up to 16 characters".into());
            }
            return Ok(Action::Join(input.into()));
        }
        if !self.bots.contains(&name.to_ascii_lowercase())
            || input.is_empty()
            || split(input).0 == "quit"
        {
            return Err("bot action requires a joined name and an action other than quit".into());
        }
        let (verb, arguments) = split(input);
        if !valid_bot_action(verb, arguments) {
            return Err(format!("invalid arguments for bot action '{verb}'"));
        }
        Ok(Action::Bot {
            name: name.into(),
            input: input.into(),
        })
    }
}

fn valid_bot_action(verb: &str, arguments: &str) -> bool {
    let args = arguments.split_whitespace().collect::<Vec<_>>();
    let coords = |words: &[&str]| {
        words.len() == 3
            && words
                .iter()
                .all(|word| word.parse::<f64>().is_ok_and(f64::is_finite))
    };
    match (verb, args.as_slice()) {
        ("await-teleport", []) | ("physics", ["off" | "manual"]) => true,
        ("physics-tick", [count]) => count.parse::<u16>().is_ok_and(|n| (1..=1200).contains(&n)),
        ("expect", ["pos" | "vel", x, y, z, "within", tolerance]) => {
            coords(&[x, y, z])
                && tolerance
                    .parse::<f64>()
                    .is_ok_and(|d| d.is_finite() && d >= 0.0)
        }
        ("use" | "place" | "break", [x, y, z]) => [x, y, z]
            .into_iter()
            .all(|word| word.parse::<i32>().is_ok()),
        ("move", position) => coords(position),
        ("look", [yaw, pitch]) => {
            yaw.parse::<f32>().is_ok_and(f32::is_finite)
                && pitch
                    .parse::<f32>()
                    .is_ok_and(|pitch| (-90.0..=90.0).contains(&pitch))
        }
        ("attack", [entity, position @ ..]) => {
            pumpkin_data::entity::EntityType::from_name(entity).is_some() && coords(position)
        }
        ("inventory-click", [slot, button]) => {
            slot.parse::<i16>().is_ok_and(|slot| slot >= 0)
                && matches!(*button, "left" | "right" | "shift-left" | "shift-right")
        }
        _ => false,
    }
}

fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn valid_property(key: &str, value: &str) -> bool {
    match key {
        "difficulty" => matches!(value, "peaceful" | "easy" | "normal" | "hard"),
        "gamemode" => matches!(value, "survival" | "creative" | "adventure" | "spectator"),
        "hardcore" => matches!(value, "true" | "false"),
        "view-distance" | "simulation-distance" => {
            value.parse::<u8>().is_ok_and(|v| (3..=32).contains(&v))
        }
        "spawn-protection" => value.parse::<u16>().is_ok_and(|v| v <= 64),
        "max-players" => value.parse::<u16>().is_ok_and(|v| (1..=1024).contains(&v)),
        _ => false,
    }
}

fn parse_area(rest: &str) -> Result<Area, String> {
    let coords = rest
        .split_whitespace()
        .map(str::parse::<i32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "area needs six integer coordinates")?;
    let [x0, y0, z0, x1, y1, z1] = coords.as_slice() else {
        return Err("area needs six integer coordinates".into());
    };
    let area = Area {
        min: [*x0, *y0, *z0],
        max: [*x1, *y1, *z1],
    };
    let lengths = (0..3).map(|i| i64::from(area.max[i]) - i64::from(area.min[i]) + 1);
    if !area
        .min
        .into_iter()
        .chain(area.max)
        .all(|x| (-29_999_984..=29_999_984).contains(&x))
        || !(-64..320).contains(y0)
        || !(-64..320).contains(y1)
        || !lengths.clone().all(|n| n > 0)
        || lengths.product::<i64>() > 1_048_576
        || area.chunks().take(257).count() > 256
    {
        return Err("area must be ordered, in the overworld build bounds and at most 1048576 blocks and 256 chunks".into());
    }
    Ok(area)
}

fn command(rest: &str) -> Result<String, String> {
    let text = rest.strip_prefix('/').unwrap_or(rest);
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err("missing or invalid command".into());
    }
    // Token checks also cover execute run and namespaced commands.
    if text.split_whitespace().any(|word| {
        matches!(
            word.trim_start_matches("minecraft:"),
            "tick" | "stop" | "save-off" | "save-on" | "save-all" | "reload"
        ) || word.contains("moddedmc_harness:")
    }) {
        return Err("command cannot change harness tick control, saves or lifecycle".into());
    }
    Ok(text.into())
}

fn probe(rest: &str) -> Result<String, String> {
    let text = command(rest)?;
    let mut words = text.split_whitespace();
    let head = words
        .next()
        .unwrap_or_default()
        .trim_start_matches("minecraft:");
    let read_only = match head {
        "seed" | "list" | "difficulty" => words.next().is_none(),
        "data" => words.next() == Some("get"),
        "time" => words.next() == Some("query"),
        "gamerule" => words.count() == 1,
        "scoreboard" => words.next() == Some("players") && words.next() == Some("get"),
        "execute" => {
            matches!(words.next(), Some("if" | "unless"))
                && !words.any(|word| matches!(word, "run" | "store"))
        }
        _ => false,
    };
    if !read_only {
        return Err("probe requires a supported read-only console query".into());
    }
    Ok(text)
}

fn split(text: &str) -> (&str, &str) {
    text.split_once(char::is_whitespace)
        .map_or((text, ""), |(first, rest)| (first, rest.trim()))
}

fn uncomment(raw: &str) -> (&str, &str) {
    let mut quote = None;
    let mut escaped = false;
    let mut whitespace = true;
    for (index, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() {
            if ch == '#' && whitespace {
                return (raw[..index].trim(), raw[index + 1..].trim());
            }
            if matches!(ch, '\'' | '"') {
                quote = Some(ch);
            }
        }
        whitespace = ch.is_whitespace();
    }
    (raw.trim(), "")
}
