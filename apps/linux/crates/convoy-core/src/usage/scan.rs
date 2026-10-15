//! Reading token counts out of the agents' own transcripts, a few new bytes
//! at a time.
//!
//! Each file's totals are kept in `usage-cache.json` with how far it was read,
//! so a rescan reads only what was appended. A file that shrank or whose start
//! changed was rewritten, and is read again from the top. A file that is gone —
//! Claude Code deletes old transcripts — keeps the totals it had: the usage
//! happened all the same.
//!
//! Totals are kept per quarter hour, model and working folder rather than per
//! day and project: the day is the reader's local one, decided when a report
//! is made, and which project a folder belongs to can change after the scan.

use super::Tokens;
use crate::time::parse_iso8601;
use crate::workspace::model::Agent;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const CACHE_VERSION: u32 = 1;
/// The length of a time slot, in seconds. Every zone's offset is a multiple
/// of it, so a slot never straddles a local midnight.
pub const SLOT: i64 = 900;
/// How much of a file's start is fingerprinted to notice a rewrite.
const HEAD: u64 = 1024;
/// How deep below `projects/` Claude's transcripts are looked for; subagents
/// sit at `<folder>/<session>/subagents/agent-*.jsonl`.
const CLAUDE_DEPTH: u32 = 5;

/// One run of tokens: `(slot, model index, folder index, counts)`, the counts
/// in [`Tokens::to_array`] order.
pub type Bucket = (i64, u32, u32, [u64; 7]);

/// The message last counted in a Claude file. Claude writes a message again
/// for each part of its reply, each copy with the usage so far, so the next
/// lines may raise it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Last {
    pub id: u64,
    pub bucket: u32,
    pub tokens: [u64; 7],
}

/// Everything known about one transcript.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FileState {
    pub agent: Option<Agent>,
    pub size: u64,
    /// Modification time, in milliseconds since the epoch.
    pub modified: i64,
    /// Bytes read: always the end of a whole line.
    pub offset: u64,
    /// A fingerprint of the first `head_len` bytes.
    pub head: u64,
    pub head_len: u64,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub cwds: Vec<String>,
    #[serde(default)]
    pub buckets: Vec<Bucket>,
    /// Claude message ids this file counted, fingerprinted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<Last>,
    /// The folder the agent works in, as last stated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Codex: the model of the turn in progress.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Codex: the running total last seen, to count only what each event adds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<[u64; 5]>,
    /// No longer on disk; its totals still count.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
}

impl FileState {
    fn index(list: &mut Vec<String>, value: &str) -> u32 {
        match list.iter().position(|item| item == value) {
            Some(at) => at as u32,
            None => {
                list.push(value.to_string());
                (list.len() - 1) as u32
            }
        }
    }

    /// Adds to the run for a slot, model and folder; returns its index.
    fn add(
        &mut self,
        lookup: &mut HashMap<(i64, u32, u32), u32>,
        slot: i64,
        model: &str,
        cwd: &str,
        tokens: [u64; 7],
    ) -> u32 {
        let model = Self::index(&mut self.models, model);
        let cwd = Self::index(&mut self.cwds, cwd);
        let at = *lookup.entry((slot, model, cwd)).or_insert_with(|| {
            self.buckets.push((slot, model, cwd, [0; 7]));
            (self.buckets.len() - 1) as u32
        });
        let bucket = &mut self.buckets[at as usize].3;
        for (sum, value) in bucket.iter_mut().zip(tokens) {
            *sum += value;
        }
        at
    }
}

/// `usage-cache.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cache {
    pub version: u32,
    /// By the transcript's path.
    pub files: BTreeMap<String, FileState>,
}

impl Cache {
    /// The cache on disk; an unreadable or older one starts over, since
    /// everything in it can be read again from the transcripts.
    pub fn load(file: &Path) -> Cache {
        fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str::<Cache>(&text).ok())
            .filter(|cache| cache.version == CACHE_VERSION)
            .unwrap_or_else(|| Cache {
                version: CACHE_VERSION,
                files: BTreeMap::new(),
            })
    }

    pub fn save(&self, file: &Path) -> Result<()> {
        let mut copy = self.clone();
        copy.version = CACHE_VERSION;
        super::prices::write_atomic(file, &serde_json::to_vec(&copy)?)
    }
}

/// A transcript to read.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub agent: Agent,
    pub path: PathBuf,
}

fn claude_files(folder: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth > CLAUDE_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => claude_files(&path, depth + 1, out),
            Ok(kind)
                if kind.is_file() && path.extension().is_some_and(|value| value == "jsonl") =>
            {
                out.push(path)
            }
            _ => {}
        }
    }
}

/// Every transcript under the given agent homes. A home is searched for both
/// agents — a managed account's folder does not say whose it is — and one
/// reached twice, through a link or a repeated entry, is read once.
pub fn sources(homes: &[PathBuf]) -> Vec<Source> {
    let mut seen_homes = HashSet::new();
    let mut seen_files = HashSet::new();
    let mut result = Vec::new();
    for home in homes {
        let key = fs::canonicalize(home).unwrap_or_else(|_| home.clone());
        if !seen_homes.insert(key) {
            continue;
        }
        let mut claude = Vec::new();
        claude_files(&home.join("projects"), 0, &mut claude);
        let mut codex = Vec::new();
        crate::provider::sweep::rollouts(&home.join("sessions"), 0, &mut codex);
        crate::provider::sweep::rollouts(&home.join("archived_sessions"), 0, &mut codex);
        for (agent, files) in [(Agent::Claude, claude), (Agent::Codex, codex)] {
            for path in files {
                let key = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if seen_files.insert(key) {
                    result.push(Source { agent, path });
                }
            }
        }
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    result
}

/// FNV-1a: a stable fingerprint with no dependency.
pub fn fingerprint(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn head_of(file: &Path, length: u64) -> Option<u64> {
    let mut raw = Vec::new();
    fs::File::open(file)
        .ok()?
        .take(length)
        .read_to_end(&mut raw)
        .ok()?;
    (raw.len() as u64 == length).then(|| fingerprint(&raw))
}

fn modified_ms(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_millis() as i64)
        .unwrap_or(0)
}

/// What a scan did.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stats {
    pub files: usize,
    /// Files read, wholly or in part.
    pub read: usize,
    /// Files read again from the top.
    pub rewritten: usize,
    pub bytes: u64,
}

/// Brings the cache up to date with the sources. `checkpoint` is called now
/// and then with the cache as it stands, for the caller to save, so a long
/// first scan that is cut short keeps what it did.
pub fn scan(cache: &mut Cache, sources: &[Source], checkpoint: &mut dyn FnMut(&Cache)) -> Stats {
    cache.version = CACHE_VERSION;
    let mut stats = Stats {
        files: sources.len(),
        ..Stats::default()
    };
    // Which file counted each Claude message: the same one is in several
    // files when a conversation was copied or forked.
    let mut owners: HashMap<u64, String> = HashMap::new();
    for (path, state) in &cache.files {
        for id in &state.ids {
            owners.entry(*id).or_insert_with(|| path.clone());
        }
    }
    let present: HashSet<String> = sources
        .iter()
        .map(|source| source.path.to_string_lossy().into_owned())
        .collect();
    for (path, state) in cache.files.iter_mut() {
        state.missing = !present.contains(path);
    }
    let mut since_checkpoint = 0u64;
    for source in sources {
        let key = source.path.to_string_lossy().into_owned();
        let Ok(metadata) = fs::metadata(&source.path) else {
            continue;
        };
        let size = metadata.len();
        let modified = modified_ms(&metadata);
        let mut state = cache.files.remove(&key).unwrap_or_default();
        let unchanged = state.agent == Some(source.agent)
            && state.size == size
            && state.modified == modified
            && state.offset <= size;
        if unchanged {
            cache.files.insert(key, state);
            continue;
        }
        let rewritten = state.agent != Some(source.agent)
            || size < state.offset
            || (state.head_len > 0 && head_of(&source.path, state.head_len) != Some(state.head));
        if rewritten {
            if state.offset > 0 {
                stats.rewritten += 1;
            }
            for id in &state.ids {
                if owners.get(id) == Some(&key) {
                    owners.remove(id);
                }
            }
            state = FileState::default();
        }
        if state.head_len == 0 || state.head_len < HEAD.min(size) {
            state.head_len = HEAD.min(size);
            state.head = head_of(&source.path, state.head_len).unwrap_or(0);
        }
        state.agent = Some(source.agent);
        let before = state.offset;
        read_file(source.agent, &source.path, &key, &mut state, &mut owners);
        state.size = size;
        state.modified = modified;
        stats.read += 1;
        stats.bytes += state.offset.saturating_sub(before);
        since_checkpoint += state.offset.saturating_sub(before);
        cache.files.insert(key, state);
        if since_checkpoint > 64 * 1024 * 1024 {
            since_checkpoint = 0;
            checkpoint(cache);
        }
    }
    stats
}

/// Reads the whole lines after the offset and moves it to the end of the last.
fn read_file(
    agent: Agent,
    path: &Path,
    key: &str,
    state: &mut FileState,
    owners: &mut HashMap<u64, String>,
) {
    let Ok(mut handle) = fs::File::open(path) else {
        return;
    };
    if handle.seek(SeekFrom::Start(state.offset)).is_err() {
        return;
    }
    let mut reader = BufReader::with_capacity(256 * 1024, handle);
    let mut lookup: HashMap<(i64, u32, u32), u32> = state
        .buckets
        .iter()
        .enumerate()
        .map(|(at, bucket)| ((bucket.0, bucket.1, bucket.2), at as u32))
        .collect();
    let mut line = Vec::new();
    loop {
        line.clear();
        let read = match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        // A line still being written is left for the next scan.
        if line.last() != Some(&b'\n') {
            break;
        }
        state.offset += read as u64;
        match agent {
            Agent::Claude => claude_line(&line, key, state, owners, &mut lookup),
            Agent::Codex => codex_line(&line, state, &mut lookup),
            // Gemini's usage is not counted yet.
            Agent::Gemini => {}
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[derive(Deserialize)]
struct ClaudeLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    cwd: Option<String>,
    message: Option<ClaudeMessage>,
}

#[derive(Deserialize)]
struct ClaudeMessage {
    id: Option<String>,
    model: Option<String>,
    usage: Option<ClaudeUsage>,
}

#[derive(Deserialize, Default)]
struct ClaudeUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
    cache_creation: Option<ClaudeCacheCreation>,
    output_tokens_details: Option<ClaudeOutputDetails>,
}

#[derive(Deserialize, Default)]
struct ClaudeCacheCreation {
    ephemeral_1h_input_tokens: Option<u64>,
}

#[derive(Deserialize, Default)]
struct ClaudeOutputDetails {
    thinking_tokens: Option<u64>,
}

/// The slot an ISO-8601 time falls in.
fn slot_of(timestamp: Option<&str>) -> Option<i64> {
    Some(parse_iso8601(timestamp?)?.div_euclid(SLOT))
}

fn claude_line(
    line: &[u8],
    key: &str,
    state: &mut FileState,
    owners: &mut HashMap<u64, String>,
    lookup: &mut HashMap<(i64, u32, u32), u32>,
) {
    if !contains(line, b"\"assistant\"") || !contains(line, b"\"usage\"") {
        // Other records still say where the agent is working.
        if contains(line, b"\"cwd\"") {
            if let Ok(record) = serde_json::from_slice::<ClaudeLine>(line) {
                if let Some(cwd) = record.cwd.filter(|cwd| !cwd.is_empty()) {
                    state.cwd = Some(cwd);
                }
            }
        }
        return;
    }
    let Ok(record) = serde_json::from_slice::<ClaudeLine>(line) else {
        return;
    };
    if let Some(cwd) = record.cwd.as_deref().filter(|cwd| !cwd.is_empty()) {
        state.cwd = Some(cwd.to_string());
    }
    if record.kind.as_deref() != Some("assistant") {
        return;
    }
    let Some(message) = record.message else {
        return;
    };
    let model = message.model.unwrap_or_default();
    // Claude Code's own notes ("<synthetic>") never reached a model.
    if model.is_empty() || model.starts_with('<') {
        return;
    }
    let Some(usage) = message.usage else {
        return;
    };
    let output = usage.output_tokens.unwrap_or(0);
    let tokens = Tokens {
        input: usage.input_tokens.unwrap_or(0),
        output,
        cache_read: usage.cache_read_input_tokens.unwrap_or(0),
        cache_write: usage.cache_creation_input_tokens.unwrap_or(0),
        cache_write_1h: usage
            .cache_creation
            .and_then(|value| value.ephemeral_1h_input_tokens)
            .unwrap_or(0),
        reasoning: usage
            .output_tokens_details
            .and_then(|value| value.thinking_tokens)
            .unwrap_or(0)
            .min(output),
        requests: 0,
    }
    .to_array();
    let id = message
        .id
        .filter(|id| !id.is_empty())
        .map(|id| fingerprint(id.as_bytes()));
    if let (Some(id), Some(last)) = (id, state.last.as_mut()) {
        if last.id == id {
            // Another copy of the message just counted: add what it grew by.
            let bucket = &mut state.buckets[last.bucket as usize].3;
            for at in 0..6 {
                if tokens[at] > last.tokens[at] {
                    bucket[at] += tokens[at] - last.tokens[at];
                    last.tokens[at] = tokens[at];
                }
            }
            return;
        }
    }
    if let Some(id) = id {
        if owners.contains_key(&id) {
            return;
        }
        owners.insert(id, key.to_string());
        state.ids.push(id);
    }
    let Some(slot) = slot_of(record.timestamp.as_deref()) else {
        return;
    };
    let cwd = state.cwd.clone().unwrap_or_default();
    let mut counted = tokens;
    counted[6] = 1;
    let bucket = state.add(lookup, slot, &model, &cwd, counted);
    state.last = id.map(|id| Last { id, bucket, tokens });
}

#[derive(Deserialize)]
struct CodexLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    payload: Option<CodexPayload>,
}

#[derive(Deserialize)]
struct CodexPayload {
    #[serde(rename = "type")]
    kind: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    info: Option<CodexInfo>,
}

#[derive(Deserialize)]
struct CodexInfo {
    total_token_usage: Option<CodexUsage>,
    last_token_usage: Option<CodexUsage>,
}

#[derive(Deserialize, Clone, Copy)]
struct CodexUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    cache_write_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    reasoning_output_tokens: u64,
}

impl CodexUsage {
    fn array(self) -> [u64; 5] {
        [
            self.input_tokens,
            self.cached_input_tokens,
            self.cache_write_input_tokens,
            self.output_tokens,
            self.reasoning_output_tokens,
        ]
    }
}

fn codex_line(line: &[u8], state: &mut FileState, lookup: &mut HashMap<(i64, u32, u32), u32>) {
    let token_count = contains(line, b"\"token_count\"");
    if !token_count && !contains(line, b"\"turn_context\"") && !contains(line, b"\"session_meta\"")
    {
        return;
    }
    let Ok(record) = serde_json::from_slice::<CodexLine>(line) else {
        return;
    };
    let Some(payload) = record.payload else {
        return;
    };
    match record.kind.as_deref() {
        Some("session_meta") => {
            if state.cwd.is_none() {
                state.cwd = payload.cwd.filter(|cwd| !cwd.is_empty());
            }
        }
        Some("turn_context") => {
            if let Some(cwd) = payload.cwd.filter(|cwd| !cwd.is_empty()) {
                state.cwd = Some(cwd);
            }
            if let Some(model) = payload.model.filter(|model| !model.is_empty()) {
                state.model = Some(model);
            }
        }
        Some("event_msg") if payload.kind.as_deref() == Some("token_count") => {
            let Some(info) = payload.info else {
                return;
            };
            let total = info.total_token_usage.map(CodexUsage::array);
            let last = info.last_token_usage.map(CodexUsage::array);
            // What this turn added: the running total's step when it only
            // grew, else (a first event, or a total that started over) the
            // turn's own figure. A repeated event adds nothing.
            let delta = match (total, state.total) {
                (Some(now), Some(before)) if now.iter().zip(before).all(|(a, b)| *a >= b) => {
                    let mut step = [0; 5];
                    for at in 0..5 {
                        step[at] = now[at] - before[at];
                    }
                    step
                }
                (Some(now), _) => last.unwrap_or(now),
                (None, _) => match last {
                    Some(last) => last,
                    None => return,
                },
            };
            if total.is_some() {
                state.total = total;
            }
            if delta.iter().all(|value| *value == 0) {
                return;
            }
            let Some(slot) = slot_of(record.timestamp.as_deref()) else {
                return;
            };
            let [input, cached, written, output, reasoning] = delta;
            // Codex counts cached input inside input_tokens; reasoning is
            // inside output_tokens.
            let tokens = Tokens {
                input: input.saturating_sub(cached).saturating_sub(written),
                output,
                cache_read: cached,
                cache_write: written,
                cache_write_1h: 0,
                reasoning: reasoning.min(output),
                requests: 1,
            };
            let model = state.model.clone().unwrap_or_else(|| "unknown".into());
            let cwd = state.cwd.clone().unwrap_or_default();
            state.add(lookup, slot, &model, &cwd, tokens.to_array());
        }
        _ => {}
    }
}
