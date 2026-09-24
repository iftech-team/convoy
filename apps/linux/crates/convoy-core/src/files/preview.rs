//! Port of `preview()` and `fileContent()`.
//!
//! Both resolve through `realpath` and refuse anything that lands outside the
//! selected folder, so a symlink committed to a repository cannot be used to
//! read arbitrary files. Reads are capped at 1 MB and NUL-containing files are
//! reported as binary rather than rendered.

use super::paths::resolve_inside;
use crate::Result;
use std::io::Read;
use std::path::Path;

pub const LIMIT: u64 = 1024 * 1024;

const NOT_PREVIEWABLE: &str = "Preview unavailable: file is not regular or exceeds 1 MB.";
const BINARY: &str = "Binary or oversized file — preview unavailable.";
const IMAGE_TOO_LARGE: &str = "Image preview exceeds 1 MB.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Text {
        text: String,
        markdown: bool,
    },
    /// Raw bytes rather than a `data:` URI: `gtk::Picture` loads bytes
    /// directly, so the base64 round-trip the web renderer needed is gone.
    Image {
        mime: &'static str,
        bytes: Vec<u8>,
    },
}

fn image_type(name: &str) -> Option<&'static str> {
    let extension = Path::new(name)
        .extension()
        .map(|value| value.to_string_lossy().to_lowercase())?;
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn is_markdown(name: &str) -> bool {
    Path::new(name)
        .extension()
        .map(|value| value.to_string_lossy().to_lowercase())
        .is_some_and(|extension| extension == "md" || extension == "markdown")
}

fn read_bounded(path: &Path) -> Result<std::result::Result<Vec<u8>, &'static str>> {
    let mut file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return Ok(Err(NOT_PREVIEWABLE));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize + 1);
    file.by_ref().take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Ok(Err(BINARY));
    }
    Ok(Ok(bytes))
}

pub fn preview(root: &Path, name: &str) -> Result<String> {
    let resolved = resolve_inside(root, name)?;
    match read_bounded(&resolved)? {
        Err(message) => Ok(message.to_string()),
        Ok(bytes) if bytes.contains(&0) => Ok(BINARY.to_string()),
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
    }
}

pub fn file_content(root: &Path, name: &str) -> Result<Content> {
    let Some(mime) = image_type(name) else {
        return Ok(Content::Text {
            text: preview(root, name)?,
            markdown: is_markdown(name),
        });
    };
    let resolved = resolve_inside(root, name)?;
    match read_bounded(&resolved)? {
        Err(_) => Ok(Content::Text {
            text: IMAGE_TOO_LARGE.to_string(),
            markdown: false,
        }),
        Ok(bytes) => Ok(Content::Image { mime, bytes }),
    }
}
