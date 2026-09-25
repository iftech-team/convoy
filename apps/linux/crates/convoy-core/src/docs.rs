//! Project docs and specs, as the macOS app keeps them: plain Markdown in the
//! repository, so agents read them as files. `.specdesk/PROJECT.md` is the
//! project doc, `.specdesk/specs/*.md` are specifications, and well-known
//! repository docs are listed beside them.
//!
//! Every path handed in from the window is relative, Markdown, and resolved
//! inside the project — a crafted `../` never reaches another file.

use crate::{bail, ensure, Result};
use std::path::{Component, Path, PathBuf};

pub const PROJECT_DOC: &str = ".specdesk/PROJECT.md";
pub const SPECS: &str = ".specdesk/specs";
const WELL_KNOWN: [&str; 5] = [
    "README.md",
    "CLAUDE.md",
    "AGENTS.md",
    "CONTRIBUTING.md",
    "ARCHITECTURE.md",
];
const LIMIT: u64 = 1024 * 1024;

fn markdown_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
                .filter(|name| name.ends_with(".md"))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Everything worth reading, in the macOS app's order: the project doc,
/// specs, the well-known files, then up to 30 files from `docs/`.
pub fn files(root: &Path) -> Vec<String> {
    let mut list = Vec::new();
    if root.join(PROJECT_DOC).is_file() {
        list.push(PROJECT_DOC.to_string());
    }
    list.extend(
        markdown_in(&root.join(SPECS))
            .into_iter()
            .map(|name| format!("{SPECS}/{name}")),
    );
    list.extend(
        WELL_KNOWN
            .iter()
            .filter(|name| root.join(name).is_file())
            .map(|name| name.to_string()),
    );
    list.extend(
        markdown_in(&root.join("docs"))
            .into_iter()
            .take(30)
            .map(|name| format!("docs/{name}")),
    );
    list
}

/// A relative Markdown path inside `root`, or an error.
pub fn resolve(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    ensure!(
        !relative.is_empty()
            && relative.ends_with(".md")
            && path
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
        "Only Markdown files inside the project can be opened."
    );
    let full = root.join(path);
    // A symlink could still lead outside; follow it and check where it lands.
    if let (Ok(real), Ok(base)) = (std::fs::canonicalize(&full), std::fs::canonicalize(root)) {
        ensure!(real.starts_with(&base), "That file is outside the project.");
    }
    Ok(full)
}

pub fn read(root: &Path, relative: &str) -> Result<String> {
    let path = resolve(root, relative)?;
    let size = std::fs::metadata(&path)?.len();
    ensure!(size <= LIMIT, "That file is too large to show here.");
    Ok(std::fs::read_to_string(path)?)
}

pub fn write(root: &Path, relative: &str, text: &str) -> Result<()> {
    ensure!(
        text.len() as u64 <= LIMIT,
        "That text is too large to save."
    );
    let path = resolve(root, relative)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("md.convoy-tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

/// The first four words of a title, lowercased and joined by dashes — the
/// macOS app's slug, so both name a spec the same.
pub fn slug(title: &str) -> String {
    let words: Vec<String> = title
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(4)
        .map(str::to_string)
        .collect();
    if words.is_empty() {
        "spec".into()
    } else {
        words.join("-")
    }
}

pub fn spec_template(title: &str) -> String {
    format!(
        "# {title}\n\n## Problem\nWhat is wrong or missing today, and for whom.\n\n## Goal\nThe outcome in one or two sentences.\n\n## Requirements\n- \n\n## Acceptance criteria\n- [ ] \n\n## Out of scope\n- \n\n## Notes\nConstraints, links, decisions.\n"
    )
}

/// Creates `.specdesk/specs/<slug>.md` from the template, keeping a file that
/// is already there. Returns its relative path.
pub fn create_spec(root: &Path, title: &str) -> Result<String> {
    let title = title.trim();
    if title.is_empty() {
        bail!("Give the spec a title.");
    }
    let relative = format!("{SPECS}/{}.md", slug(title));
    let path = resolve(root, &relative)?;
    if !path.exists() {
        write(root, &relative, &spec_template(title))?;
    }
    Ok(relative)
}

pub const PROJECT_DOC_PROMPT: &str = "Write the project document at .specdesk/PROJECT.md (create the folder if needed). It is read by every agent before it starts a task and by the team as the single overview. Study the repository first. Include: purpose and domain in plain words; architecture and main modules with paths; how to build, run and test; conventions (code style, branching, commit messages, review expectations); environment and secrets handling (names only, never values); known pitfalls; and a short glossary. Keep it under ~250 lines, accurate to the code, and note anything you could not verify. Do not change any other file.";

pub fn spec_prompt(path: &str, title: &str) -> String {
    format!("Draft the specification at {path} for: {title}. Keep the existing section headings (Problem, Goal, Requirements, Acceptance criteria, Out of scope, Notes). Study the codebase so requirements reference real modules and current behaviour. Acceptance criteria must be checkable. Ask me in the terminal if a decision is genuinely ambiguous; otherwise state your assumption in Notes. Do not implement anything and do not change other files.")
}
