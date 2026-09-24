//! Helpers that keep the validation port readable while preserving JavaScript
//! semantics: `undefined` vs `null`, truthiness, and UTF-16 string length.

use serde_json::Value;

/// JavaScript truthiness. `Value::Null` stands in for both `null` and — where
/// a key is absent — the caller passes `None` instead.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(text) => !text.is_empty(),
        _ => true,
    }
}

pub fn truthy_opt(value: Option<&Value>) -> bool {
    value.is_some_and(truthy)
}

/// `typeof value === 'string'` — note that a missing key and `null` both fail.
pub fn string(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// A non-empty string, i.e. what JavaScript treats as a truthy property.
pub fn nonempty(value: Option<&Value>) -> Option<&str> {
    string(value).filter(|text| !text.is_empty())
}

pub fn boolean(value: Option<&Value>) -> Option<bool> {
    value.and_then(Value::as_bool)
}

pub fn array(value: Option<&Value>) -> Option<&Vec<Value>> {
    value.and_then(Value::as_array)
}

/// `Number.isInteger`.
pub fn integer(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|number| number.fract() == 0.0 && number.is_finite())
}

/// `Number.isSafeInteger`.
pub fn safe_integer(value: Option<&Value>) -> Option<f64> {
    integer(value).filter(|number| number.abs() <= 9_007_199_254_740_991.0)
}

/// `String.prototype.length` counts UTF-16 code units, not bytes or scalars.
/// Every ported length limit is expressed in those units so that a prompt full
/// of emoji is accepted or rejected identically by both builds.
pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// `value.slice(-max)` in UTF-16 terms, rounded outward to a char boundary so
/// the result is always valid UTF-8.
pub fn tail(text: &str, max: usize) -> &str {
    let mut units = 0;
    let mut start = text.len();
    for (index, character) in text.char_indices().rev() {
        let next = units + character.len_utf16();
        if next > max {
            break;
        }
        units = next;
        start = index;
    }
    &text[start..]
}

/// `value.slice(0, max)` in UTF-16 terms.
pub fn head(text: &str, max: usize) -> &str {
    let mut units = 0;
    for (index, character) in text.char_indices() {
        if units + character.len_utf16() > max {
            return &text[..index];
        }
        units += character.len_utf16();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_surrogate_pairs_like_javascript() {
        assert_eq!(utf16_len("abc"), 3);
        assert_eq!(utf16_len("é"), 1);
        assert_eq!(utf16_len("🙂"), 2);
        assert_eq!(utf16_len("a🙂b"), 4);
    }

    #[test]
    fn slices_on_character_boundaries() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("abc", 10), "abc");
        // A surrogate pair that does not fit is dropped whole.
        assert_eq!(tail("a🙂", 2), "🙂");
        assert_eq!(tail("a🙂", 1), "");
        assert_eq!(head("a🙂b", 3), "a🙂");
        assert_eq!(head("a🙂b", 2), "a");
    }
}
