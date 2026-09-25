//! Project images, as the macOS app offers them: a GitHub avatar, a site's
//! favicon, or an uploaded PNG or JPEG. Each is copied into Convoy's own
//! `icons` folder and the project's icon becomes `gh:<path>` or `img:<path>`,
//! the same values the macOS app writes.

use super::Workspace;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::State;

const LIMIT: usize = 512 * 1024;

fn is_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.starts_with(&[0xff, 0xd8, 0xff])
}

fn icons_dir(workspace: &Workspace) -> PathBuf {
    workspace.storage.root().join("icons")
}

fn save(
    workspace: &Workspace,
    id: &str,
    name: &str,
    bytes: &[u8],
    prefix: &str,
) -> Result<String, String> {
    if bytes.len() > LIMIT || !is_image(bytes) {
        return Err("That is not a PNG or JPEG of 512 KB or less.".into());
    }
    let dir = icons_dir(workspace);
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let file = dir.join(name);
    std::fs::write(&file, bytes).map_err(|error| error.to_string())?;
    let icon = format!("{prefix}{}", file.to_string_lossy());
    workspace.act(|core| {
        core.edit_project(
            id,
            convoy_core::workspace::ProjectPatch {
                icon: Some(icon.clone()),
                ..Default::default()
            },
        )
        .map(|_| ())
    })?;
    Ok(icon)
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build()
        .get(url)
        .call()
        .map_err(|error| format!("Could not download the image: {error}"))?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(response.into_reader(), LIMIT as u64 + 1),
        &mut bytes,
    )
    .map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn project_path(workspace: &Workspace, id: &str) -> Result<PathBuf, String> {
    workspace.act(|core| Ok(core.project(id)?.path.clone()))
}

/// The owner of a GitHub `origin` remote, in either URL form.
fn github_owner(remote: &str) -> Option<String> {
    let rest = remote.split("github.com").nth(1)?;
    let owner = rest.trim_start_matches([':', '/']).split('/').next()?;
    (!owner.is_empty() && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .then(|| owner.to_string())
}

/// Sets the project icon from an image source: `github` (the origin
/// remote's owner), `favicon` (a domain) or `upload` (a file path).
#[tauri::command]
pub async fn project_icon_set(
    id: String,
    source: String,
    value: String,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    match source.as_str() {
        "upload" => {
            let bytes = std::fs::read(&value).map_err(|error| error.to_string())?;
            save(&workspace, &id, &format!("{id}.png"), &bytes, "img:")
        }
        "github" => {
            let path = project_path(&workspace, &id)?;
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&path)
                .args(["remote", "get-url", "origin"])
                .output()
                .map_err(|error| error.to_string())?;
            let remote = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let owner = github_owner(&remote).ok_or("This project has no GitHub origin remote.")?;
            let url = format!("https://github.com/{owner}.png?size=64");
            let bytes = tauri::async_runtime::spawn_blocking(move || download(&url))
                .await
                .map_err(|error| error.to_string())??;
            save(&workspace, &id, &format!("gh-{owner}.png"), &bytes, "gh:")
        }
        "favicon" => {
            let host: String = value
                .trim()
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .split('/')
                .next()
                .unwrap_or_default()
                .to_lowercase();
            if host.is_empty()
                || !host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            {
                return Err("Enter a domain such as example.com.".into());
            }
            let url = format!("https://www.google.com/s2/favicons?domain={host}&sz=64");
            let bytes = tauri::async_runtime::spawn_blocking(move || download(&url))
                .await
                .map_err(|error| error.to_string())??;
            save(
                &workspace,
                &id,
                &format!("favicon-{host}.png"),
                &bytes,
                "img:",
            )
        }
        _ => Err("Unknown image source.".into()),
    }
}

/// An image icon as a data URL for the web view. Only files inside Convoy's
/// own icons folder are read, whatever path the workspace holds.
#[tauri::command]
pub fn icon_data(path: String, workspace: State<'_, Workspace>) -> Result<String, String> {
    let dir =
        std::fs::canonicalize(icons_dir(&workspace)).map_err(|_| "No icons yet.".to_string())?;
    let file = std::fs::canonicalize(Path::new(&path)).map_err(|error| error.to_string())?;
    if !file.starts_with(&dir) {
        return Err("That image is not in Convoy's icons folder.".into());
    }
    let bytes = std::fs::read(&file).map_err(|error| error.to_string())?;
    if bytes.len() > LIMIT || !is_image(&bytes) {
        return Err("Not an image.".into());
    }
    let kind = if bytes.starts_with(b"\x89PNG") {
        "png"
    } else {
        "jpeg"
    };
    Ok(format!(
        "data:image/{kind};base64,{}",
        convoy_core::integrations::base64(&bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::github_owner;

    #[test]
    fn reads_the_owner_from_either_remote_form() {
        assert_eq!(
            github_owner("git@github.com:iftech-team/convoy.git").as_deref(),
            Some("iftech-team")
        );
        assert_eq!(
            github_owner("https://github.com/iftech-team/convoy").as_deref(),
            Some("iftech-team")
        );
        assert_eq!(github_owner("https://gitlab.com/a/b"), None);
    }
}
