//! Port of Node's `util.stripVTControlCharacters`, using the same pattern, so
//! saved output and review briefs are filtered identically by both builds.

use regex::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;

static ANSI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"[\x{1B}\x{9B}][\[\]()#;?]*",
        r"(?:(?:(?:(?:;[-a-zA-Z\d/#&.:=?%@~_]+)*",
        r"|[a-zA-Z\d]+(?:;[-a-zA-Z\d/#&.:=?%@~_]*)*)?",
        r"\x{07})",
        r"|(?:(?:\d{1,4}(?:;\d{0,4})*)?[\dA-PR-TZcf-nq-uy=><~]))"
    ))
    .unwrap()
});

pub fn strip(text: &str) -> Cow<'_, str> {
    ANSI.replace_all(text, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_colour_and_cursor_sequences() {
        assert_eq!(strip("\x1b[32mDone\x1b[0m"), "Done");
        assert_eq!(strip("a\x1b[2Kb"), "ab");
        assert_eq!(strip("plain"), "plain");
    }
}
