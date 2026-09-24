//! Files & Changes: reading a repository and writing to it.
//!
//! Every destructive action is narrow on purpose, and the confirmations are
//! the front end's — the rules about what may be done at all are here.

use super::{directory_or_project, Workspace};
use convoy_core::git::diff::{digest, hunks, split, SplitRow};
use convoy_core::git::mutate::Action;
use convoy_core::git::{ReadRequest, Snapshot};
use convoy_core::Git;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Serialize)]
pub struct ChangeView {
    pub path: String,
    pub original: Option<String>,
    pub status: String,
    pub untracked: bool,
    pub staged: bool,
    pub conflict: bool,
}

#[derive(Serialize)]
pub struct CommitView {
    pub id: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    pub date: String,
}

#[derive(Serialize)]
pub struct SnapshotView {
    pub branch: String,
    pub changes: Vec<ChangeView>,
    pub files: Vec<String>,
    pub log: Vec<CommitView>,
    pub branches: Vec<String>,
    pub directory: String,
}

fn view_of(snapshot: Snapshot, directory: String) -> SnapshotView {
    SnapshotView {
        branch: snapshot.branch,
        changes: snapshot
            .changes
            .into_iter()
            .map(|change| ChangeView {
                status: format!("{}{}", change.index, change.worktree),
                staged: change.index != ' ' && change.index != '?',
                untracked: change.untracked,
                conflict: change.conflict,
                path: change.path,
                original: change.original,
            })
            .collect(),
        files: snapshot.files,
        log: snapshot
            .log
            .into_iter()
            .map(|commit| CommitView {
                id: commit.id,
                short: commit.short,
                subject: commit.subject,
                author: commit.author,
                date: commit.date,
            })
            .collect(),
        branches: snapshot.branches,
        directory,
    }
}

#[tauri::command]
pub fn files_snapshot(id: String, workspace: State<'_, Workspace>) -> Result<SnapshotView, String> {
    let directory = directory_or_project(&workspace, &id)?;
    let snapshot = Git::default()
        .snapshot(&directory)
        .map_err(|error| error.to_string())?;
    Ok(view_of(snapshot, directory.to_string_lossy().into_owned()))
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Selection {
    File { path: String },
    Untracked { path: String },
    Staged { path: String },
    Unstaged { path: String },
    Commit { commit: String },
}

impl From<&Selection> for ReadRequest {
    fn from(selection: &Selection) -> Self {
        match selection {
            Selection::File { path } => ReadRequest::File { path: path.clone() },
            Selection::Untracked { path } => ReadRequest::Untracked { path: path.clone() },
            Selection::Staged { path } => ReadRequest::Staged { path: path.clone() },
            Selection::Unstaged { path } => ReadRequest::Unstaged { path: path.clone() },
            Selection::Commit { commit } => ReadRequest::Commit {
                commit: commit.clone(),
            },
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Preview {
    /// A diff or a source file, with the hash a hunk discard must still match.
    Text {
        text: String,
        hash: String,
        language: Option<String>,
    },
    /// The same diff read side by side, with the line numbers each side has.
    Split {
        rows: Vec<SplitRowView>,
        hash: String,
    },
    Markdown {
        text: String,
    },
    Image {
        mime: String,
        data: String,
    },
}

#[derive(Serialize)]
pub struct SplitRowView {
    pub left: Option<(u32, String)>,
    pub right: Option<(u32, String)>,
    pub separator: Option<String>,
}

fn split_view(rows: Vec<SplitRow>) -> Vec<SplitRowView> {
    rows.into_iter()
        .map(|row| SplitRowView {
            left: row.left,
            right: row.right,
            separator: row.separator,
        })
        .collect()
}

/// Reads one selection. Images come back as bytes rather than a data URI on
/// the Rust side; the front end needs a URL, so they are encoded here once.
#[tauri::command]
pub fn files_read(
    id: String,
    selection: Selection,
    side_by_side: bool,
    workspace: State<'_, Workspace>,
) -> Result<Preview, String> {
    let directory = directory_or_project(&workspace, &id)?;

    if let Selection::File { path } = &selection {
        let content = convoy_core::files::file_content(&directory, path)
            .map_err(|error| error.to_string())?;
        return Ok(match content {
            convoy_core::files::Content::Image { mime, bytes } => Preview::Image {
                mime: mime.to_string(),
                data: base64(&bytes),
            },
            convoy_core::files::Content::Text {
                text,
                markdown: true,
            } => Preview::Markdown { text },
            convoy_core::files::Content::Text { text, .. } => Preview::Text {
                hash: digest(&text),
                language: language_for(path),
                text,
            },
        });
    }

    let request = ReadRequest::from(&selection);
    let text = Git::default()
        .read(&directory, &request)
        .map_err(|error| error.to_string())?;
    let hash = digest(&text);

    if matches!(selection, Selection::Untracked { .. }) {
        let path = match &selection {
            Selection::Untracked { path } => path.clone(),
            _ => String::new(),
        };
        return Ok(Preview::Text {
            language: language_for(&path),
            text,
            hash,
        });
    }
    if side_by_side {
        return Ok(Preview::Split {
            rows: split_view(split(&text)),
            hash,
        });
    }
    Ok(Preview::Text {
        text,
        hash,
        language: Some("diff".into()),
    })
}

/// The number of hunks a diff has, so the front end can offer a choice.
#[tauri::command]
pub fn files_hunks(
    id: String,
    path: String,
    workspace: State<'_, Workspace>,
) -> Result<Vec<String>, String> {
    let directory = directory_or_project(&workspace, &id)?;
    let text = Git::default()
        .read(&directory, &ReadRequest::Unstaged { path })
        .map_err(|error| error.to_string())?;
    Ok(hunks(&text)
        .patches
        .iter()
        .map(|patch| patch.lines().next().unwrap_or_default().to_string())
        .collect())
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum Mutation {
    Stage {
        path: String,
        original: Option<String>,
    },
    Unstage {
        path: String,
        original: Option<String>,
    },
    StageAll,
    Discard {
        path: String,
    },
    DiscardHunk {
        path: String,
        hunk: usize,
        hash: String,
    },
    Commit {
        message: String,
        amend: bool,
    },
    Fetch,
    Pull,
    Push,
    Switch {
        branch: String,
    },
    Branch {
        branch: String,
    },
    Revert {
        commit: String,
    },
    ResetSoft {
        commit: String,
    },
    ResetMixed {
        commit: String,
    },
}

impl From<Mutation> for Action {
    fn from(mutation: Mutation) -> Self {
        match mutation {
            Mutation::Stage { path, original } => Action::Stage { path, original },
            Mutation::Unstage { path, original } => Action::Unstage { path, original },
            Mutation::StageAll => Action::StageAll,
            Mutation::Discard { path } => Action::Discard { path },
            Mutation::DiscardHunk { path, hunk, hash } => Action::DiscardHunk { path, hunk, hash },
            Mutation::Commit { message, amend } => Action::Commit { message, amend },
            Mutation::Fetch => Action::Fetch,
            Mutation::Pull => Action::Pull,
            Mutation::Push => Action::Push,
            Mutation::Switch { branch } => Action::Switch { branch },
            Mutation::Branch { branch } => Action::Branch { branch },
            Mutation::Revert { commit } => Action::Revert { commit },
            Mutation::ResetSoft { commit } => Action::ResetSoft { commit },
            Mutation::ResetMixed { commit } => Action::ResetMixed { commit },
        }
    }
}

#[tauri::command]
pub fn files_mutate(
    id: String,
    mutation: Mutation,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    let directory = directory_or_project(&workspace, &id)?;
    Git::default()
        .mutate(&directory, &Action::from(mutation))
        .map_err(|error| error.to_string())
}

/// Only untracked files go to Trash: anything Git knows about can be restored
/// from Git instead. The parent is resolved rather than the file itself, so a
/// symlink cannot be used to trash something outside the repository.
#[tauri::command]
pub fn files_trash(
    id: String,
    path: String,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    let directory = directory_or_project(&workspace, &id)?;
    let snapshot = Git::default()
        .snapshot(&directory)
        .map_err(|error| error.to_string())?;
    let untracked = snapshot
        .changes
        .iter()
        .any(|change| change.path == path && change.untracked);
    if !untracked {
        return Err("Only untracked files can be moved to Trash.".into());
    }
    let target = convoy_core::files::paths::trash_target(&directory, &path)
        .map_err(|error| error.to_string())?;
    trash_file(&target)
}

fn trash_file(path: &std::path::Path) -> Result<(), String> {
    // The Recycle Bin on Windows, the freedesktop trash on Linux. Both are
    // recoverable, which is what the button promises.
    trash::delete(path).map_err(|error| format!("Could not move it to the trash: {error}"))
}

/// Asks Claude for a commit message for the staged diff. One of the few places
/// Convoy makes a provider request, so it is always the user pressing a button.
#[tauri::command]
pub fn commit_generate(id: String, workspace: State<'_, Workspace>) -> Result<String, String> {
    let directory = directory_or_project(&workspace, &id)?;
    let storage = workspace.storage.clone();
    let account = workspace.act(|core| {
        let session = core.session(&id).cloned().unwrap_or_else(|_| {
            convoy_core::model::Session::new(
                String::new(),
                convoy_core::model::Agent::Claude,
                "commit message",
            )
        });
        convoy_core::session::prepare_account(core, &storage, &session)
    })?;
    convoy_core::repository::generate_message(
        &Git::default(),
        &convoy_core::StdRunner,
        &directory,
        &account.env,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn pr_create(id: String, workspace: State<'_, Workspace>) -> Result<String, String> {
    let directory = directory_or_project(&workspace, &id)?;
    convoy_core::repository::create_pr(&convoy_core::StdRunner, &directory)
        .map_err(|error| error.to_string())
}

fn language_for(name: &str) -> Option<String> {
    let extension = std::path::Path::new(name)
        .extension()?
        .to_string_lossy()
        .to_lowercase();
    // Only the mapping the highlighter needs; unknown extensions render plain.
    let language = match extension.as_str() {
        "rs" => "rust",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" => "typescript",
        "jsx" | "tsx" => "jsx",
        "py" => "python",
        "go" => "go",
        "swift" => "swift",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "rb" => "ruby",
        "php" => "php",
        "c" | "h" => "c",
        "cc" | "cpp" | "hpp" | "hh" => "cpp",
        "cs" => "csharp",
        "sh" | "bash" | "zsh" => "bash",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "sql" => "sql",
        "html" | "htm" => "html",
        "css" => "css",
        "xml" => "xml",
        "diff" | "patch" => "diff",
        _ => return None,
    };
    Some(language.to_string())
}

/// Base64 without a dependency: the alphabet is sixty-four characters and the
/// rule is three bytes to four.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let packed = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(packed >> 18) as usize & 63] as char);
        out.push(ALPHABET[(packed >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(packed >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[packed as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{base64, trash_file};

    #[test]
    fn base64_matches_the_standard_alphabet_and_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    /// Trash is not delete. The file has to leave the folder and still exist
    /// somewhere the user can get it back from.
    #[test]
    fn trashing_a_file_takes_it_out_of_the_folder_without_destroying_it() {
        let folder = std::env::temp_dir().join(format!("convoy-trash-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("notes.txt");
        std::fs::write(&file, b"keep me").unwrap();

        trash_file(&file).unwrap();

        assert!(!file.exists(), "the file is still in the folder");
        assert!(folder.exists(), "the folder went with it");
        std::fs::remove_dir_all(&folder).ok();
    }
}
