//! What a model's tokens cost, in US dollars per million, from a table the
//! user can edit: `usage-prices.json` beside the workspace.
//!
//! The defaults hold Anthropic's published API rates. OpenAI's are left out on
//! purpose — Codex models come in empty for the user to fill — so nothing here
//! claims a price nobody checked. Every figure built from them is an estimate:
//! a subscription plan is not billed per token at all.

use super::Tokens;
use crate::{ensure, ConvoyError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

const FILE_VERSION: u32 = 1;

/// One model's rates. A missing rate is unknown, not free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Price {
    /// A model id, `claude-opus-4-8`, which also matches its dated releases
    /// (`claude-opus-4-8-20260101`); or a prefix ending in `*`.
    pub model: String,
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    /// Writing to the five-minute cache.
    #[serde(default)]
    pub cache_write: Option<f64>,
    /// Writing to the one-hour cache, which Claude Code uses for its sessions.
    #[serde(default)]
    pub cache_write_1h: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PriceFile {
    version: u32,
    prices: Vec<Price>,
}

/// Input, output and cache read per million tokens; cache writes are the
/// API's multiples of input: 1.25× for five minutes, 2× for an hour.
fn anthropic(model: &str, input: f64, output: f64, cache_read: f64) -> Price {
    Price {
        model: model.to_string(),
        input: Some(input),
        output: Some(output),
        cache_read: Some(cache_read),
        cache_write: Some(round(input * 1.25)),
        cache_write_1h: Some(round(input * 2.0)),
    }
}

fn round(value: f64) -> f64 {
    (value * 1e6).round() / 1e6
}

/// Anthropic's API prices, newest models first.
pub fn defaults() -> Vec<Price> {
    vec![
        anthropic("claude-fable-5-1", 10.0, 50.0, 0.25),
        anthropic("claude-mythos-5-1", 10.0, 50.0, 0.25),
        anthropic("claude-fable-5", 10.0, 50.0, 1.0),
        anthropic("claude-mythos-5", 10.0, 50.0, 1.0),
        anthropic("claude-opus-5-5", 4.0, 20.0, 0.20),
        anthropic("claude-opus-5", 5.0, 25.0, 0.50),
        anthropic("claude-opus-4-8", 5.0, 25.0, 0.50),
        anthropic("claude-opus-4-7", 5.0, 25.0, 0.50),
        anthropic("claude-opus-4-6", 5.0, 25.0, 0.50),
        anthropic("claude-opus-4-5", 5.0, 25.0, 0.50),
        anthropic("claude-opus-4-1", 15.0, 75.0, 1.50),
        anthropic("claude-opus-4", 15.0, 75.0, 1.50),
        anthropic("claude-sonnet-5-5", 2.0, 10.0, 0.20),
        anthropic("claude-sonnet-5", 2.0, 10.0, 0.20),
        anthropic("claude-sonnet-4-6", 3.0, 15.0, 0.30),
        anthropic("claude-sonnet-4-5", 3.0, 15.0, 0.30),
        anthropic("claude-sonnet-4", 3.0, 15.0, 0.30),
        anthropic("claude-3-7-sonnet", 3.0, 15.0, 0.30),
        anthropic("claude-haiku-5-5", 0.10, 0.50, 0.01),
        anthropic("claude-haiku-4-5", 1.0, 5.0, 0.10),
        anthropic("claude-3-5-haiku", 0.80, 4.0, 0.08),
    ]
}

/// The table on disk, or the defaults when there is none. A damaged file is
/// an error, so a save cannot silently replace what the user wrote.
pub fn load(file: &Path) -> Result<Vec<Price>> {
    match fs::read_to_string(file) {
        Ok(text) => {
            let parsed: PriceFile = serde_json::from_str(&text).map_err(|_| {
                ConvoyError::message(format!("The prices file is damaged: {}", file.display()))
            })?;
            ensure!(
                parsed.version <= FILE_VERSION,
                "The prices file was written by a newer Convoy."
            );
            Ok(parsed.prices)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(defaults()),
        Err(error) => Err(error.into()),
    }
}

/// Checks and tidies a table: ids trimmed and lower-cased, each once, every
/// rate a finite number no less than zero.
pub fn clean(prices: Vec<Price>) -> Result<Vec<Price>> {
    let mut result: Vec<Price> = Vec::with_capacity(prices.len());
    for mut price in prices {
        price.model = price.model.trim().to_lowercase();
        ensure!(!price.model.is_empty(), "Every price needs a model id.");
        ensure!(
            !result.iter().any(|seen| seen.model == price.model),
            "{} is listed twice.",
            price.model
        );
        for rate in [
            price.input,
            price.output,
            price.cache_read,
            price.cache_write,
            price.cache_write_1h,
        ]
        .into_iter()
        .flatten()
        {
            ensure!(
                rate.is_finite() && rate >= 0.0,
                "Prices are dollars per million tokens, zero or more."
            );
        }
        result.push(price);
    }
    Ok(result)
}

/// Writes the table whole, atomically.
pub fn save(file: &Path, prices: Vec<Price>) -> Result<Vec<Price>> {
    let prices = clean(prices)?;
    let body = serde_json::to_string_pretty(&PriceFile {
        version: FILE_VERSION,
        prices: prices.clone(),
    })?;
    write_atomic(file, body.as_bytes())?;
    Ok(prices)
}

pub(crate) fn write_atomic(file: &Path, body: &[u8]) -> Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut temporary = file.to_path_buf().into_os_string();
    temporary.push(format!(".{}.tmp", Uuid::new_v4()));
    let temporary = std::path::PathBuf::from(temporary);
    let outcome = (|| -> Result<()> {
        let mut handle = crate::fs::create_private(&temporary)?;
        std::io::Write::write_all(&mut handle, body)?;
        handle.sync_all()?;
        drop(handle);
        fs::rename(&temporary, file)?;
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    outcome
}

/// A model id as the table compares it: lower case, without a provider's
/// prefix (`us.anthropic.`), a context marker (`[1m]`) or a Vertex `@date`.
pub fn canonical(model: &str) -> String {
    let mut id = model.trim().to_lowercase();
    if let Some(at) = id.find('[') {
        id.truncate(at);
    }
    if let Some(at) = id.find('@') {
        id.truncate(at);
    }
    if let Some(at) = id.rfind("anthropic.") {
        id = id[at + "anthropic.".len()..].to_string();
    }
    id.trim().to_string()
}

/// How closely a pattern matches an id, if it does: its length, so the most
/// specific entry wins. `claude-opus-4` matches `claude-opus-4-20250514` but
/// not `claude-opus-4-8`; `claude-opus-4-*` matches both.
fn specificity(pattern: &str, id: &str) -> Option<usize> {
    if let Some(prefix) = pattern.strip_suffix('*') {
        return id.starts_with(prefix).then_some(prefix.len());
    }
    let rest = id.strip_prefix(pattern)?;
    let dated = rest
        .strip_prefix('-')
        .is_some_and(|date| date.len() >= 8 && date.bytes().all(|b| b.is_ascii_digit()));
    (rest.is_empty() || dated).then_some(pattern.len() + 1)
}

/// The entry for a model, the most specific one when several match.
pub fn find<'a>(prices: &'a [Price], model: &str) -> Option<&'a Price> {
    let id = canonical(model);
    prices
        .iter()
        .filter_map(|price| Some((specificity(&canonical(&price.model), &id)?, price)))
        .max_by_key(|(score, _)| *score)
        .map(|(_, price)| price)
}

/// Dollars for some tokens, or none when a rate they need is unknown.
/// Reasoning is part of output and is not counted twice.
pub fn cost(price: Option<&Price>, tokens: &Tokens) -> Option<f64> {
    let price = price?;
    let five_minute = tokens.cache_write.saturating_sub(tokens.cache_write_1h);
    let parts = [
        (tokens.input, price.input),
        (tokens.output, price.output),
        (tokens.cache_read, price.cache_read),
        (five_minute, price.cache_write),
        (tokens.cache_write_1h, price.cache_write_1h),
    ];
    let mut total = 0.0;
    for (count, rate) in parts {
        if count == 0 {
            continue;
        }
        total += count as f64 * rate? / 1_000_000.0;
    }
    Some(total)
}
