//! `convoy --session …`: the command line a manager's agent runs.
//!
//! It starts no window. Each command is one request file and one result file
//! (see the module above); `wait` asks for the helpers' status until they are
//! all settled or its time is up.
//!
//! Exit codes: 0 success; 1 Convoy refused or a helper failed or stopped;
//! 2 bad usage or not run from a manager; 3 Convoy is not running or did not
//! answer; 124 `wait` timed out with a helper still running.

use super::{
    alive, requests_dir, take_result, write_request, Command, HelperStatus, HelperView, Request,
    Spawn, ENV_REQUESTS, ENV_SESSION, ENV_TOKEN,
};
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

pub const USAGE: &str = "\
usage: convoy --session <command>

  spawn --agent claude|codex --prompt <text|@file> [--title <t>] [--model <m>]
        [--effort <e>] [--worktree <branch> | --worktree-auto]
        Start a helper session in this project; prints its id.
  wait <id>... [--timeout <seconds>]
        Wait until each helper finishes its turn; one JSON line per helper:
        {\"id\",\"status\":\"done|failed|stopped|timeout\",\"title\"}.
  output <id>
        Print the helper's last answer.
  list
        Your helpers and their status, one JSON line each.

Run from a Convoy manager session, which sets CONVOY_SESSION and CONVOY_TOKEN.
Exit codes: 0 ok, 1 refused/failed, 2 usage, 3 Convoy not answering, 124 timeout.";

/// How the command line talks to the app; tests shorten the waits.
#[derive(Debug, Clone)]
pub struct Timing {
    /// How often a result file is looked for.
    pub poll: Duration,
    /// How long a quick request may take.
    pub quick: Duration,
    /// How long a spawn may take: it may create a worktree first.
    pub spawn: Duration,
    /// Between two status checks while waiting.
    pub wait_step: Duration,
    /// `wait` without `--timeout`.
    pub wait_default: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            poll: Duration::from_millis(100),
            quick: Duration::from_secs(20),
            spawn: Duration::from_secs(120),
            wait_step: Duration::from_secs(1),
            // Under the two minutes Claude Code gives a shell command.
            wait_default: Duration::from_secs(90),
        }
    }
}

/// A failure: its exit code and what is said on stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: i32,
    pub message: String,
}

fn fail(code: i32, message: impl Into<String>) -> Failure {
    Failure {
        code,
        message: message.into(),
    }
}

/// What a run needs from the environment.
pub struct Context {
    pub session: String,
    pub token: String,
    pub requests: PathBuf,
}

impl Context {
    /// From `CONVOY_SESSION`, `CONVOY_TOKEN` and, when set, `CONVOY_REQUESTS`.
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Result<Self, Failure> {
        let value = |key: &str| get(key).filter(|value| !value.trim().is_empty());
        let (Some(session), Some(token)) = (value(ENV_SESSION), value(ENV_TOKEN)) else {
            return Err(fail(
                2,
                "Run this from a Convoy manager session: CONVOY_SESSION and CONVOY_TOKEN are not set.",
            ));
        };
        let requests = value(ENV_REQUESTS)
            .map(PathBuf::from)
            .unwrap_or_else(|| requests_dir(&crate::Storage::default()));
        Ok(Context {
            session,
            token,
            requests,
        })
    }
}

/// Runs `args` (what follows `--session`). Returns the exit code.
pub fn run(
    args: &[String],
    get: &dyn Fn(&str) -> Option<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    run_with(args, get, out, err, &Timing::default())
}

pub fn run_with(
    args: &[String],
    get: &dyn Fn(&str) -> Option<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    timing: &Timing,
) -> i32 {
    if args.is_empty() || matches!(args[0].as_str(), "help" | "--help" | "-h") {
        if args.is_empty() {
            let _ = writeln!(err, "{USAGE}");
            return 2;
        }
        let _ = writeln!(out, "{USAGE}");
        return 0;
    }
    let outcome = parse(args).and_then(|parsed| {
        let context = Context::from_env(get)?;
        execute(&context, parsed, timing, out)
    });
    match outcome {
        Ok(code) => code,
        Err(failure) => {
            let _ = writeln!(err, "convoy: {}", failure.message);
            if failure.code == 2 && failure.message.starts_with("usage") {
                let _ = writeln!(err, "{USAGE}");
            }
            failure.code
        }
    }
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Spawn(Spawn),
    Wait {
        ids: Vec<String>,
        timeout: Option<Duration>,
    },
    Output(String),
    List,
}

/// Reads `--prompt @file`.
fn prompt_text(value: &str) -> Result<String, Failure> {
    match value.strip_prefix('@') {
        Some(path) if !path.is_empty() => std::fs::read_to_string(path)
            .map_err(|error| fail(2, format!("Cannot read the prompt file {path}: {error}"))),
        _ => Ok(value.to_string()),
    }
}

pub fn parse(args: &[String]) -> Result<Parsed, Failure> {
    let usage = |message: &str| fail(2, format!("usage: {message}"));
    let (command, rest) = args.split_first().ok_or_else(|| usage("no command"))?;
    let mut rest = rest.iter();
    match command.as_str() {
        "spawn" => {
            let mut spawn = Spawn::default();
            let mut prompt = None;
            while let Some(flag) = rest.next() {
                let mut value = || {
                    rest.next()
                        .cloned()
                        .ok_or_else(|| usage(&format!("{flag} needs a value")))
                };
                match flag.as_str() {
                    "--agent" => spawn.agent = value()?,
                    "--model" => spawn.model = Some(value()?),
                    "--effort" => spawn.effort = Some(value()?),
                    "--title" => spawn.title = Some(value()?),
                    "--worktree" => spawn.worktree = Some(value()?),
                    "--worktree-auto" => spawn.worktree_auto = true,
                    "--prompt" => prompt = Some(prompt_text(&value()?)?),
                    other => return Err(usage(&format!("unknown option {other}"))),
                }
            }
            if spawn.agent.is_empty() {
                return Err(usage("spawn needs --agent claude|codex"));
            }
            spawn.prompt = prompt.ok_or_else(|| usage("spawn needs --prompt"))?;
            if spawn.worktree.is_some() && spawn.worktree_auto {
                return Err(usage("choose --worktree or --worktree-auto, not both"));
            }
            Ok(Parsed::Spawn(spawn))
        }
        "wait" => {
            let mut ids = Vec::new();
            let mut timeout = None;
            while let Some(arg) = rest.next() {
                if arg == "--timeout" {
                    let seconds: f64 = rest
                        .next()
                        .and_then(|value| value.parse().ok())
                        .filter(|seconds: &f64| seconds.is_finite() && *seconds >= 0.0)
                        .ok_or_else(|| usage("--timeout needs a number of seconds"))?;
                    timeout = Some(Duration::from_secs_f64(seconds.min(86_400.0)));
                } else if arg.starts_with('-') {
                    return Err(usage(&format!("unknown option {arg}")));
                } else if !ids.contains(arg) {
                    ids.push(arg.clone());
                }
            }
            if ids.is_empty() {
                return Err(usage("wait needs at least one helper id"));
            }
            Ok(Parsed::Wait { ids, timeout })
        }
        "output" => match (rest.next(), rest.next()) {
            (Some(id), None) => Ok(Parsed::Output(id.clone())),
            _ => Err(usage("output takes one helper id")),
        },
        "list" => match rest.next() {
            None => Ok(Parsed::List),
            Some(_) => Err(usage("list takes no arguments")),
        },
        other => Err(usage(&format!("unknown command {other}"))),
    }
}

/// One request and its answer.
pub fn ask(
    context: &Context,
    command: Command,
    patience: Duration,
    timing: &Timing,
) -> Result<Value, Failure> {
    if !alive(&context.requests, SystemTime::now()) {
        return Err(fail(3, "Convoy is not running. Open Convoy and try again."));
    }
    let request = Request::new(&context.session, &context.token, command);
    let path = write_request(&context.requests, &request)
        .map_err(|error| fail(3, format!("Cannot reach Convoy: {error}")))?;
    let deadline = Instant::now() + patience;
    loop {
        if let Some(response) = take_result(&context.requests, &request.id) {
            return if response.ok {
                Ok(response.value)
            } else {
                Err(fail(
                    1,
                    response.error.unwrap_or_else(|| "Convoy refused.".into()),
                ))
            };
        }
        if Instant::now() >= deadline {
            let _ = std::fs::remove_file(&path);
            return Err(fail(
                3,
                if alive(&context.requests, SystemTime::now()) {
                    "Convoy did not answer in time."
                } else {
                    "Convoy stopped running."
                },
            ));
        }
        std::thread::sleep(timing.poll);
    }
}

fn views(value: Value) -> Result<Vec<HelperView>, Failure> {
    serde_json::from_value(value).map_err(|_| fail(1, "Convoy sent an unexpected answer."))
}

fn line(out: &mut dyn Write, view: &HelperView) {
    let _ = writeln!(
        out,
        "{}",
        serde_json::json!({ "id": view.id, "status": view.status, "title": view.title })
    );
}

/// What `wait` exits with, given the final statuses.
pub fn wait_code(statuses: &[HelperStatus]) -> i32 {
    if statuses.contains(&HelperStatus::Timeout) {
        124
    } else if statuses.iter().all(|status| *status == HelperStatus::Done) {
        0
    } else {
        1
    }
}

/// The statuses `wait` prints: anything still running when time ran out
/// timed out.
pub fn final_statuses(views: &mut [HelperView], timed_out: bool) {
    if timed_out {
        for view in views.iter_mut() {
            if view.status == HelperStatus::Running {
                view.status = HelperStatus::Timeout;
            }
        }
    }
}

fn execute(
    context: &Context,
    parsed: Parsed,
    timing: &Timing,
    out: &mut dyn Write,
) -> Result<i32, Failure> {
    match parsed {
        Parsed::Spawn(spawn) => {
            let value = ask(context, Command::Spawn(spawn), timing.spawn, timing)?;
            let id = value
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| fail(1, "Convoy sent an unexpected answer."))?;
            let _ = writeln!(out, "{id}");
            Ok(0)
        }
        Parsed::Output(id) => {
            let value = ask(context, Command::Output { id }, timing.quick, timing)?;
            let text = value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let _ = writeln!(out, "{text}");
            Ok(0)
        }
        Parsed::List => {
            let value = ask(context, Command::List, timing.quick, timing)?;
            let list = views(value)?;
            for view in &list {
                let _ = writeln!(
                    out,
                    "{}",
                    serde_json::json!({ "id": view.id, "status": view.status, "agent": view.agent, "title": view.title })
                );
            }
            Ok(0)
        }
        Parsed::Wait { ids, timeout } => {
            let deadline = Instant::now() + timeout.unwrap_or(timing.wait_default);
            loop {
                let value = ask(
                    context,
                    Command::Status { ids: ids.clone() },
                    timing.quick,
                    timing,
                )?;
                let mut list = views(value)?;
                let settled = list.iter().all(|view| view.status.settled());
                let timed_out = !settled && Instant::now() >= deadline;
                if settled || timed_out {
                    final_statuses(&mut list, timed_out);
                    for view in &list {
                        line(out, view);
                    }
                    let statuses: Vec<HelperStatus> = list.iter().map(|view| view.status).collect();
                    return Ok(wait_code(&statuses));
                }
                let left = deadline.saturating_duration_since(Instant::now());
                std::thread::sleep(timing.wait_step.min(left.max(Duration::from_millis(10))));
            }
        }
    }
}
