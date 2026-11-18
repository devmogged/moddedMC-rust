//! Pregeneration on pack servers that include the Chunky port (`engine/ports/chunky`): the agent
//! drives its console commands and reads progress back from its `[Chunky]` log lines.

use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    App,
    servers::{bad_name, error},
};

/// How long the server gets to answer `chunky start` before the agent decides it has no port.
const START_TIMEOUT: Duration = Duration::from_secs(5);
/// The world border's limit, as in the port.
const MAX_RADIUS: i32 = 29_999_984;

#[derive(Deserialize)]
pub struct Start {
    radius: i32,
    #[serde(default)]
    center_x: i32,
    #[serde(default)]
    center_z: i32,
    #[serde(default)]
    circle: bool,
}

#[derive(Serialize, Default, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
enum TaskState {
    #[default]
    Idle,
    Running,
    Paused,
    Finished,
    Cancelled,
}

#[derive(Serialize, Default, PartialEq, Debug)]
pub struct Progress {
    state: TaskState,
    world: Option<String>,
    chunks: Option<u64>,
    percent: Option<f64>,
    eta_secs: Option<u64>,
    rate: Option<f64>,
}

/// `h:mm:ss` from the port's progress line.
fn clock(text: &str) -> Option<u64> {
    let mut secs = 0;
    for part in text.split(':') {
        secs = secs * 60 + part.trim().parse::<u64>().ok()?;
    }
    Some(secs)
}

fn between<'a>(line: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let rest = &line[line.find(start)? + start.len()..];
    Some(rest.find(end).map_or(rest, |i| &rest[..i]))
}

/// The newest `[Chunky]` task line says where the task is. Lines without numbers (paused,
/// continuing) take them from the task's last progress line.
fn parse(lines: &[String]) -> Progress {
    let mut found: Option<Progress> = None;
    for line in lines.iter().rev() {
        let Some(at) = line.find("[Chunky] Task ") else {
            continue;
        };
        let started = line[at..].starts_with("[Chunky] Task started");
        let progress = parse_line(&line[at..]);
        match (&mut found, progress) {
            (None, Some(p))
                if p.chunks.is_none()
                    && !started
                    && matches!(p.state, TaskState::Running | TaskState::Paused) =>
            {
                found = Some(p);
            }
            (None, Some(p)) => return p,
            (Some(f), Some(p)) if p.chunks.is_some() => {
                f.chunks = p.chunks;
                f.percent = p.percent;
                f.eta_secs = p.eta_secs;
                f.rate = p.rate;
                break;
            }
            _ if started => break,
            _ => {}
        }
    }
    found.unwrap_or_default()
}

fn parse_line(line: &str) -> Option<Progress> {
    let world = line.split_once(" for ").and_then(|(_, rest)| {
        rest.split([' ', '.'])
            .next()
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
    });
    let mut progress = Progress {
        world,
        chunks: between(line, "Processed: ", " chunks").and_then(|n| n.parse().ok()),
        percent: between(line, " chunks (", "%").and_then(|n| n.parse().ok()),
        eta_secs: between(line, "ETA: ", ",").and_then(clock),
        rate: between(line, "Rate: ", " cps").and_then(|n| n.parse().ok()),
        ..Progress::default()
    };
    progress.state = if line.starts_with("[Chunky] Task running")
        || line.starts_with("[Chunky] Task started")
        || line.starts_with("[Chunky] Task continuing")
    {
        TaskState::Running
    } else if line.starts_with("[Chunky] Task paused") {
        TaskState::Paused
    } else if line.starts_with("[Chunky] Task finished") {
        TaskState::Finished
    } else if line.starts_with("[Chunky] Task cancelled") {
        TaskState::Cancelled
    } else {
        return None;
    };
    Some(progress)
}

pub async fn get(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    match app.servers.backlog(&name).await {
        Some(lines) => Json(parse(&lines)).into_response(),
        None => error(StatusCode::NOT_FOUND, format!("no server named {name}")),
    }
}

pub async fn start(
    State(app): State<Arc<App>>,
    UrlPath(name): UrlPath<String>,
    Json(body): Json<Start>,
) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    let in_range = |v: i32| (-MAX_RADIUS..=MAX_RADIUS).contains(&v);
    if !(1..=MAX_RADIUS).contains(&body.radius)
        || !in_range(body.center_x)
        || !in_range(body.center_z)
    {
        return error(
            StatusCode::BAD_REQUEST,
            "radius or center out of range".into(),
        );
    }
    let shape = if body.circle { "circle" } else { "square" };
    for line in [
        format!("chunky center {} {}", body.center_x, body.center_z),
        format!("chunky radius {}", body.radius),
        format!("chunky shape {shape}"),
    ] {
        if let Err((status, message)) = app.servers.command(&name, &line).await {
            return error(status, message);
        }
    }
    match app
        .servers
        .command_until(
            &name,
            "chunky start",
            "[Chunky] Task started",
            START_TIMEOUT,
        )
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((StatusCode::GATEWAY_TIMEOUT, _)) => error(
            StatusCode::CONFLICT,
            format!(
                "{name} can't pregenerate: its pack has no port-chunky, or a task is already running"
            ),
        ),
        Err((status, message)) => error(status, message),
    }
}

pub async fn control(
    State(app): State<Arc<App>>,
    UrlPath((name, action)): UrlPath<(String, String)>,
) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    if !matches!(action.as_str(), "pause" | "continue" | "cancel") {
        return error(StatusCode::NOT_FOUND, format!("no pregen action {action}"));
    }
    match app
        .servers
        .command(&name, &format!("chunky {action}"))
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((status, message)) => error(status, message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_ports_progress_lines() {
        let lines = [
            "02:00:00  INFO [Chunky] Task started for minecraft:overworld at 0, 0 with radius 500.",
            "02:00:05  INFO [Chunky] Task running for minecraft:overworld. Processed: 2828 chunks (69.04%), ETA: 0:01:02, Rate: 563.9 cps, Current: 27, 1",
        ]
        .map(String::from);
        assert_eq!(
            parse(&lines),
            Progress {
                state: TaskState::Running,
                world: Some("minecraft:overworld".into()),
                chunks: Some(2828),
                percent: Some(69.04),
                eta_secs: Some(62),
                rate: Some(563.9),
            }
        );
        let paused = [
            lines[1].clone(),
            "[Chunky] Task paused for minecraft:overworld.".to_string(),
        ];
        let p = parse(&paused);
        assert_eq!((p.state, p.chunks), (TaskState::Paused, Some(2828)));
        let done = ["[Chunky] Task finished for minecraft:overworld. Processed: 4225 chunks (100.00%), Total time: 0:00:07".to_string()];
        assert_eq!(parse(&done).state, TaskState::Finished);
        assert_eq!(parse(&[]).state, TaskState::Idle);
    }
}
