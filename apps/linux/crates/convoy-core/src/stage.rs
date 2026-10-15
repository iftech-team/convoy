//! Stage completion files: how an agent tells Convoy a stage of a task is
//! finished.
//!
//! An interactive `claude` or `codex` does not exit by itself when it is
//! done, and Codex has no hooks, so neither the process nor the agent's own
//! status says "this stage is over". Each stage's brief therefore ends with
//! the same paragraph: write the result to a file Convoy names, outside the
//! repository. The window polls for those files ([`crate::pipeline::poll`])
//! and moves the task on; a clean exit remains a fallback, and each file is
//! named after the run it belongs to, so whichever comes first wins once.
//!
//! QA keeps its own file, [`crate::qa::verdict_file`].

use crate::workspace::Workspace;
use crate::Storage;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The stages of a task that report back by file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Plan,
    Build,
    Review,
    Qa,
    Docs,
    /// The pull request's title and description.
    Pr,
}

/// What a stage's file said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageResult {
    /// The plan, as Markdown.
    Plan(String),
    /// What the builder did and how it verified it.
    Build {
        summary: String,
        verified: String,
    },
    /// A reviewer's or QA's verdict.
    Verdict {
        passed: bool,
        findings: String,
    },
    /// The documents written, as absolute paths.
    Docs(Vec<PathBuf>),
    Pr {
        title: String,
        body: String,
    },
}

/// Convoy's data folder: where `workspace.json` is.
pub fn storage(workspace: &Workspace) -> Storage {
    Storage::new(
        workspace
            .file()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default(),
    )
}

/// Where completion files are kept: beside `workspace.json`, never in a
/// repository.
pub fn dir(workspace: &Workspace) -> PathBuf {
    storage(workspace).root().join("stages")
}

/// Where QA verdicts are kept, as they always have been.
pub fn qa_dir(workspace: &Workspace) -> PathBuf {
    storage(workspace).qa()
}

/// The file the agent of `run_id` writes when `stage` is done.
pub fn file(workspace: &Workspace, stage: Stage, run_id: &str) -> PathBuf {
    let stages = dir(workspace);
    match stage {
        Stage::Plan => stages.join(format!("{run_id}.plan.md")),
        Stage::Build => stages.join(format!("{run_id}.done.json")),
        Stage::Review => stages.join(format!("{run_id}.verdict.json")),
        Stage::Qa => crate::qa::verdict_file(&qa_dir(workspace), run_id),
        Stage::Docs => stages.join(format!("{run_id}.docs.json")),
        Stage::Pr => stages.join(format!("{run_id}.pr.json")),
    }
}

#[derive(Deserialize)]
struct Done {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    verified: String,
}

#[derive(Deserialize)]
struct Verdict {
    verdict: String,
    #[serde(default)]
    findings: String,
}

#[derive(Deserialize)]
struct Pr {
    title: String,
    #[serde(default)]
    body: String,
}

#[derive(Deserialize)]
struct Docs {
    #[serde(default)]
    files: Vec<PathBuf>,
}

/// Reads a stage's file. `None` while it is missing, empty, or not yet
/// valid — an agent may still be writing it, so a half-written file is
/// simply read again on the next poll.
pub fn read(stage: Stage, path: &Path) -> Option<StageResult> {
    let text = std::fs::read_to_string(path).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    match stage {
        Stage::Plan => Some(StageResult::Plan(text.trim().to_string())),
        Stage::Build => serde_json::from_str::<Done>(&text)
            .ok()
            .map(|done| StageResult::Build {
                summary: done.summary.trim().to_string(),
                verified: done.verified.trim().to_string(),
            }),
        Stage::Review | Stage::Qa => {
            let verdict = serde_json::from_str::<Verdict>(&text).ok()?;
            let verdict_word = verdict.verdict.trim().to_ascii_lowercase();
            let passed = match verdict_word.as_str() {
                "pass" | "passed" => true,
                "fail" | "failed" => false,
                _ => return None,
            };
            Some(StageResult::Verdict {
                passed,
                findings: verdict.findings.trim().to_string(),
            })
        }
        Stage::Docs => serde_json::from_str::<Docs>(&text)
            .ok()
            .map(|docs| StageResult::Docs(docs.files)),
        Stage::Pr => serde_json::from_str::<Pr>(&text)
            .ok()
            .filter(|pr| !pr.title.trim().is_empty())
            .map(|pr| StageResult::Pr {
                title: pr.title.trim().to_string(),
                body: pr.body.trim().to_string(),
            }),
    }
}

/// The paragraph every brief ends with: what to write, where, and then to
/// stop.
pub fn closing(stage: Stage, path: &Path) -> String {
    let path = path.display();
    let what = match stage {
        Stage::Plan => format!("write the whole plan as Markdown to {path}"),
        Stage::Build => format!(
            "write JSON to {path}: {{\"summary\": \"what you changed\", \"verified\": \"how you checked it\"}}"
        ),
        Stage::Review | Stage::Qa => format!(
            "write your verdict as JSON to {path}: {{\"verdict\": \"pass\"}} when everything is right, or {{\"verdict\": \"fail\", \"findings\": \"1. What is wrong, where, how to reproduce it.\\n2. …\"}}. Findings must be concrete and reproducible"
        ),
        Stage::Docs => format!(
            "write JSON to {path}: {{\"files\": [\"/absolute/path/of/each/document\"]}}"
        ),
        Stage::Pr => format!(
            "write JSON to {path}: {{\"title\": \"the pull request's title\", \"body\": \"its description, as Markdown\"}}"
        ),
    };
    format!(
        "## When you are done\nWhen you have finished, {what}. The file is outside the repository: never commit it. Then end your reply; Convoy reads the file and takes it from there."
    )
}

/// The most a brief may hold, as sessions take it.
pub const BRIEF_LIMIT: usize = 32_000;

/// A brief within [`BRIEF_LIMIT`], shortened before its closing paragraph
/// (the one [`closing`] writes) and never through it: an agent that never
/// reads where its file goes never writes it.
pub fn fit(text: &str) -> String {
    if crate::json::head(text, BRIEF_LIMIT).len() == text.len() {
        return text.to_string();
    }
    let Some(at) = text.rfind("## When you are done") else {
        return crate::json::head(text, BRIEF_LIMIT).to_string();
    };
    let (body, end) = text.split_at(at);
    let note = "\n…(shortened)\n\n";
    let room = BRIEF_LIMIT.saturating_sub(end.chars().count() + note.chars().count());
    format!("{}{note}{end}", crate::json::head(body, room).trim_end())
}
