use super::prices::{self, Price};
use super::scan::{self, Cache};
use super::*;
use crate::automations::schedule::RuleZone;
use crate::provider::sweep::Target;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn claude(id: &str, model: &str, at: &str, cwd: &str, usage: &str) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{at}","cwd":"{cwd}","isSidechain":false,"message":{{"id":"{id}","model":"{model}","role":"assistant","content":[{{"type":"text","text":"hi"}}],"usage":{usage}}}}}"#
    ) + "\n"
}

fn usage(input: u64, output: u64, read: u64, write: u64) -> String {
    format!(
        r#"{{"input_tokens":{input},"output_tokens":{output},"cache_read_input_tokens":{read},"cache_creation_input_tokens":{write},"cache_creation":{{"ephemeral_1h_input_tokens":{write},"ephemeral_5m_input_tokens":0}}}}"#
    )
}

fn token_count(at: &str, total: [u64; 4], last: [u64; 4]) -> String {
    let part = |v: [u64; 4]| {
        format!(
            r#"{{"input_tokens":{},"cached_input_tokens":{},"cache_write_input_tokens":0,"output_tokens":{},"reasoning_output_tokens":{},"total_tokens":{}}}"#,
            v[0],
            v[1],
            v[2],
            v[3],
            v[0] + v[2]
        )
    };
    format!(
        r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{},"last_token_usage":{},"model_context_window":258400}},"rate_limits":null}}}}"#,
        part(total),
        part(last)
    ) + "\n"
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn append(path: &Path, text: &str) {
    let mut handle = fs::OpenOptions::new().append(true).open(path).unwrap();
    handle.write_all(text.as_bytes()).unwrap();
}

fn totals(cache: &Cache, agent: Agent) -> Tokens {
    let mut sum = Tokens::default();
    for state in cache.files.values().filter(|s| s.agent == Some(agent)) {
        for bucket in &state.buckets {
            sum.add(&Tokens::from_array(bucket.3));
        }
    }
    sum
}

fn scan_home(cache: &mut Cache, home: &Path) -> scan::Stats {
    let sources = scan::sources(&[home.to_path_buf()]);
    scan::scan(cache, &sources, &mut |_| {})
}

#[test]
fn claude_counts_each_message_once_and_skips_synthetic_ones() {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("projects/-p/a.jsonl");
    let mut text = String::new();
    text += r#"{"type":"user","cwd":"/p","timestamp":"2026-10-01T10:00:00Z","message":{"role":"user","content":"go"}}"#;
    text += "\n";
    // One reply written three times as it streamed, its output growing.
    text += &claude(
        "msg_1",
        "claude-opus-5-5",
        "2026-10-01T10:00:01Z",
        "/p",
        &usage(5, 10, 100, 50),
    );
    text += &claude(
        "msg_1",
        "claude-opus-5-5",
        "2026-10-01T10:00:02Z",
        "/p",
        &usage(5, 30, 100, 50),
    );
    text += &claude(
        "msg_1",
        "claude-opus-5-5",
        "2026-10-01T10:00:03Z",
        "/p",
        &usage(5, 30, 100, 50),
    );
    text += &claude(
        "msg_2",
        "claude-opus-5-5",
        "2026-10-01T10:01:00Z",
        "/p",
        &usage(1, 7, 200, 0),
    );
    text += &claude(
        "msg_3",
        "<synthetic>",
        "2026-10-01T10:02:00Z",
        "/p",
        &usage(0, 0, 0, 0),
    );
    write(&file, &text);
    // A subagent's transcript costs too.
    let sub = home.path().join("projects/-p/a/subagents/agent-x.jsonl");
    write(
        &sub,
        &claude(
            "msg_9",
            "claude-haiku-5-5",
            "2026-10-01T11:00:00Z",
            "/p",
            &usage(2, 3, 0, 0),
        ),
    );
    // A copy of the conversation in another folder is not counted again.
    write(&home.path().join("projects/-q/a.jsonl"), &text);

    let mut cache = Cache::default();
    let stats = scan_home(&mut cache, home.path());
    assert_eq!(stats.files, 3);
    let sum = totals(&cache, Agent::Claude);
    assert_eq!(sum.requests, 3, "msg_1, msg_2 and the subagent's msg_9");
    assert_eq!(sum.output, 30 + 7 + 3);
    assert_eq!(sum.input, 5 + 1 + 2);
    assert_eq!(sum.cache_read, 300);
    assert_eq!(sum.cache_write, 50);
    assert_eq!(sum.cache_write_1h, 50);
    let models: Vec<&String> = cache.files.values().flat_map(|s| &s.models).collect();
    assert!(!models.iter().any(|m| m.contains("synthetic")));
}

#[test]
fn codex_counts_what_each_token_event_adds() {
    let home = tempfile::tempdir().unwrap();
    let file = home
        .path()
        .join("sessions/2026/10/01/rollout-2026-10-01T10-00-00-abc.jsonl");
    let mut text = String::new();
    text += r#"{"timestamp":"2026-10-01T10:00:00Z","type":"session_meta","payload":{"id":"abc","cwd":"/p","base_instructions":{"text":"long"}}}"#;
    text += "\n";
    text += r#"{"timestamp":"2026-10-01T10:00:00Z","type":"turn_context","payload":{"cwd":"/p","model":"gpt-6-astra"}}"#;
    text += "\n";
    text += r#"{"timestamp":"2026-10-01T10:00:00Z","type":"event_msg","payload":{"type":"token_count","info":null}}"#;
    text += "\n";
    text += &token_count("2026-10-01T10:00:05Z", [100, 60, 10, 4], [100, 60, 10, 4]);
    // The same event again: nothing new.
    text += &token_count("2026-10-01T10:00:06Z", [100, 60, 10, 4], [100, 60, 10, 4]);
    text += &token_count("2026-10-01T10:01:00Z", [250, 160, 30, 4], [150, 100, 20, 0]);
    text += r#"{"timestamp":"2026-10-01T10:02:00Z","type":"turn_context","payload":{"cwd":"/p/sub","model":"gpt-6-mini"}}"#;
    text += "\n";
    text += &token_count("2026-10-01T10:02:30Z", [300, 160, 35, 4], [50, 0, 5, 0]);
    write(&file, &text);

    let mut cache = Cache::default();
    scan_home(&mut cache, home.path());
    let sum = totals(&cache, Agent::Codex);
    assert_eq!(sum.requests, 3);
    assert_eq!(sum.cache_read, 160);
    assert_eq!(sum.input, 300 - 160, "cached input is not counted twice");
    assert_eq!(sum.output, 35);
    assert_eq!(sum.reasoning, 4);
    let state = cache.files.values().next().unwrap();
    assert_eq!(state.models, ["gpt-6-astra", "gpt-6-mini"]);
    assert_eq!(state.cwds, ["/p", "/p/sub"]);
}

#[test]
fn a_total_that_starts_over_counts_the_turn_itself() {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("sessions/rollout-x.jsonl");
    let mut text = String::new();
    text += &token_count("2026-10-01T10:00:00Z", [100, 0, 10, 0], [100, 0, 10, 0]);
    text += &token_count("2026-10-01T10:01:00Z", [40, 0, 4, 0], [40, 0, 4, 0]);
    write(&file, &text);
    let mut cache = Cache::default();
    scan_home(&mut cache, home.path());
    let sum = totals(&cache, Agent::Codex);
    assert_eq!((sum.input, sum.output, sum.requests), (140, 14, 2));
    assert_eq!(cache.files.values().next().unwrap().models, ["unknown"]);
}

#[test]
fn a_rescan_reads_only_new_lines_and_a_rewritten_file_from_the_top() {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("projects/-p/a.jsonl");
    let first = claude(
        "m1",
        "claude-opus-5-5",
        "2026-10-01T10:00:00Z",
        "/p",
        &usage(1, 1, 0, 0),
    );
    write(&file, &first);
    let mut cache = Cache::default();
    let stats = scan_home(&mut cache, home.path());
    assert_eq!((stats.read, stats.bytes), (1, first.len() as u64));

    // Nothing changed: nothing read.
    let stats = scan_home(&mut cache, home.path());
    assert_eq!(stats.read, 0);

    // A line still being written waits; once whole it is read, alone.
    let second = claude(
        "m2",
        "claude-opus-5-5",
        "2026-10-01T10:05:00Z",
        "/p",
        &usage(2, 2, 0, 0),
    );
    append(&file, &second[..40]);
    scan_home(&mut cache, home.path());
    assert_eq!(totals(&cache, Agent::Claude).requests, 1);
    append(&file, &second[40..]);
    let stats = scan_home(&mut cache, home.path());
    assert_eq!(stats.bytes, second.len() as u64);
    assert_eq!(totals(&cache, Agent::Claude).requests, 2);

    // A message continued across scans still counts once, at its largest.
    let more = claude(
        "m2",
        "claude-opus-5-5",
        "2026-10-01T10:05:01Z",
        "/p",
        &usage(2, 9, 0, 0),
    );
    append(&file, &more);
    scan_home(&mut cache, home.path());
    let sum = totals(&cache, Agent::Claude);
    assert_eq!((sum.requests, sum.output), (2, 1 + 9));

    // The cache survives a save and load.
    let saved = home.path().join("usage-cache.json");
    cache.save(&saved).unwrap();
    let mut cache = Cache::load(&saved);
    assert_eq!(totals(&cache, Agent::Claude).output, 10);

    // Rewritten shorter, with other content: read again from the top.
    let other = claude(
        "m7",
        "claude-sonnet-5-5",
        "2026-10-02T10:00:00Z",
        "/p",
        &usage(4, 4, 0, 0),
    );
    write(&file, &other);
    let stats = scan_home(&mut cache, home.path());
    assert_eq!(stats.rewritten, 1);
    let sum = totals(&cache, Agent::Claude);
    assert_eq!((sum.requests, sum.output), (1, 4));

    // A transcript the agent deleted keeps what it counted.
    fs::remove_file(&file).unwrap();
    scan_home(&mut cache, home.path());
    assert!(cache.files.values().all(|state| state.missing));
    assert_eq!(totals(&cache, Agent::Claude).output, 4);
}

#[test]
fn the_same_size_but_a_new_start_is_a_rewrite() {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("projects/-p/a.jsonl");
    write(
        &file,
        &claude(
            "m1",
            "claude-opus-5-5",
            "2026-10-01T10:00:00Z",
            "/p",
            &usage(1, 1, 0, 0),
        ),
    );
    let mut cache = Cache::default();
    scan_home(&mut cache, home.path());
    // Same length, different message and a later time.
    write(
        &file,
        &claude(
            "m9",
            "claude-opus-5-5",
            "2026-10-01T10:00:00Z",
            "/p",
            &usage(1, 2, 0, 0),
        ),
    );
    let past = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(past)
        .unwrap();
    let stats = scan_home(&mut cache, home.path());
    assert_eq!(stats.rewritten, 1);
    assert_eq!(totals(&cache, Agent::Claude).output, 2);
}

fn target(id: &str, path: &str, alias: bool) -> Target {
    Target {
        project_id: id.into(),
        path: PathBuf::from(path),
        alias,
    }
}

#[test]
fn folders_map_to_the_deepest_project_or_worktree() {
    let targets = vec![
        target("outer", "/work", false),
        target("api", "/work/api", false),
        target("api", "/wt/api-fix", true),
    ];
    let home = Path::new("/home/me");
    assert_eq!(
        project_of("/work/api/src", &targets, home).as_deref(),
        Some("api")
    );
    assert_eq!(
        project_of("/work/web", &targets, home).as_deref(),
        Some("outer")
    );
    assert_eq!(
        project_of("/wt/api-fix", &targets, home).as_deref(),
        Some("api")
    );
    assert_eq!(project_of("/elsewhere", &targets, home), None);
    assert_eq!(project_of("", &targets, home), None);
}

fn cache_with(entries: &[(Agent, &str, &str, &str, Tokens)]) -> Cache {
    let mut cache = Cache::default();
    for (n, (agent, at, model, cwd, tokens)) in entries.iter().enumerate() {
        let slot = crate::time::parse_iso8601(at)
            .unwrap()
            .div_euclid(scan::SLOT);
        cache.files.insert(
            format!("/f{n}"),
            scan::FileState {
                agent: Some(*agent),
                models: vec![model.to_string()],
                cwds: vec![cwd.to_string()],
                buckets: vec![(slot, 0, 0, tokens.to_array())],
                ..Default::default()
            },
        );
    }
    cache
}

fn tokens(input: u64, output: u64) -> Tokens {
    Tokens {
        input,
        output,
        requests: 1,
        ..Default::default()
    }
}

fn query(days: u32, now: &str) -> Query {
    Query {
        days,
        project_id: None,
        agent: None,
        now: crate::time::parse_iso8601(now).unwrap(),
    }
}

#[test]
fn days_are_the_readers_local_ones() {
    let cache = cache_with(&[
        (
            Agent::Claude,
            "2026-10-01T20:00:00Z",
            "claude-opus-5-5",
            "/p",
            tokens(10, 0),
        ),
        (
            Agent::Codex,
            "2026-10-01T03:00:00Z",
            "gpt-6",
            "/p",
            tokens(5, 0),
        ),
    ]);
    let home = Path::new("/home/me");
    let east = RuleZone {
        initial: 5 * 3600,
        changes: vec![],
    };
    let report = report(
        &cache,
        &[],
        home,
        &[],
        &east,
        &query(7, "2026-10-03T12:00:00Z"),
    );
    assert_eq!(report.days.len(), 7);
    let day = |name: &str| report.days.iter().find(|d| d.day == name).cloned().unwrap();
    assert_eq!((day("2026-10-02").claude, day("2026-10-01").codex), (10, 5));

    let west = RuleZone {
        initial: -8 * 3600,
        changes: vec![],
    };
    let report = super::report(
        &cache,
        &[],
        home,
        &[],
        &west,
        &query(7, "2026-10-03T12:00:00Z"),
    );
    let day = |name: &str| report.days.iter().find(|d| d.day == name).cloned().unwrap();
    assert_eq!((day("2026-10-01").claude, day("2026-09-30").codex), (10, 5));
    assert_eq!(report.active_days, 2);
}

#[test]
fn a_report_filters_prices_and_names_the_unpriced() {
    let cache = cache_with(&[
        (
            Agent::Claude,
            "2026-10-02T10:00:00Z",
            "claude-opus-5-5",
            "/work/api",
            tokens(1_000_000, 1_000_000),
        ),
        (
            Agent::Claude,
            "2026-10-02T11:00:00Z",
            "claude-haiku-5-5-20261001",
            "/elsewhere",
            tokens(2_000_000, 0),
        ),
        (
            Agent::Codex,
            "2026-10-02T12:00:00Z",
            "gpt-6-astra",
            "/work/api",
            tokens(3_000_000, 0),
        ),
        // Outside a 7-day range, inside the heatmap.
        (
            Agent::Claude,
            "2026-09-01T12:00:00Z",
            "claude-opus-5-5",
            "/work/api",
            tokens(9, 0),
        ),
    ]);
    let targets = vec![target("api", "/work/api", false)];
    let zone = RuleZone::default();
    let home = Path::new("/home/me");
    let prices = prices::defaults();
    let q = query(7, "2026-10-03T12:00:00Z");
    let all = report(&cache, &targets, home, &prices, &zone, &q);
    assert_eq!(all.total, 7_000_000);
    // Opus 5.5: $4 in + $20 out; Haiku 5.5: 2M × $0.10.
    assert!((all.cost - 24.2).abs() < 1e-9, "{}", all.cost);
    assert_eq!(all.unpriced, ["gpt-6-astra"]);
    assert_eq!(all.top_model.as_deref(), Some("gpt-6-astra"));
    assert_eq!(all.models.len(), 3);
    assert_eq!(
        all.models
            .iter()
            .find(|m| m.model == "gpt-6-astra")
            .unwrap()
            .cost,
        None
    );
    assert_eq!(all.heatmap.len(), HEATMAP_DAYS as usize);
    assert_eq!(all.heatmap.iter().map(|d| d.total).sum::<u64>(), 7_000_009);
    let api = all
        .projects
        .iter()
        .find(|p| p.project_id.as_deref() == Some("api"))
        .unwrap();
    assert_eq!(
        (api.total, api.claude, api.codex, api.partial),
        (5_000_000, 2_000_000, 3_000_000, true)
    );
    let other = all
        .projects
        .iter()
        .find(|p| p.project_id.is_none())
        .unwrap();
    assert_eq!((other.total, other.partial), (2_000_000, false));

    let only_api = report(
        &cache,
        &targets,
        home,
        &prices,
        &zone,
        &Query {
            project_id: Some("api".into()),
            ..q.clone()
        },
    );
    assert_eq!(only_api.total, 5_000_000);
    let only_codex = report(
        &cache,
        &targets,
        home,
        &prices,
        &zone,
        &Query {
            agent: Some(Agent::Codex),
            ..q.clone()
        },
    );
    assert_eq!((only_codex.total, only_codex.cost), (3_000_000, 0.0));
    assert_eq!(only_codex.days.iter().map(|d| d.claude).sum::<u64>(), 0);

    let month = report(
        &cache,
        &targets,
        home,
        &prices,
        &zone,
        &query(90, "2026-10-03T12:00:00Z"),
    );
    assert_eq!(month.days.len(), 90);
    assert_eq!(month.total, 7_000_009);
}

#[test]
fn prices_match_by_family_and_date() {
    let table = prices::defaults();
    let model = |id: &str| prices::find(&table, id).map(|p| p.model.clone());
    assert_eq!(model("claude-opus-5-5").as_deref(), Some("claude-opus-5-5"));
    assert_eq!(model("claude-opus-5").as_deref(), Some("claude-opus-5"));
    assert_eq!(
        model("claude-opus-4-20250514").as_deref(),
        Some("claude-opus-4")
    );
    assert_eq!(
        model("claude-opus-4-1-20250805").as_deref(),
        Some("claude-opus-4-1")
    );
    assert_eq!(
        model("claude-sonnet-4-5-20250929").as_deref(),
        Some("claude-sonnet-4-5")
    );
    assert_eq!(
        model("claude-opus-5-5[1m]").as_deref(),
        Some("claude-opus-5-5")
    );
    assert_eq!(
        model("us.anthropic.claude-haiku-4-5").as_deref(),
        Some("claude-haiku-4-5")
    );
    assert_eq!(
        model("claude-opus-4-9"),
        None,
        "an unknown release is not guessed"
    );
    assert_eq!(
        model("gpt-6-astra"),
        None,
        "OpenAI prices are left to the user"
    );

    let mut table = table;
    table.push(Price {
        model: "claude-opus-4-*".into(),
        input: Some(1.0),
        output: Some(1.0),
        cache_read: None,
        cache_write: None,
        cache_write_1h: None,
    });
    assert_eq!(
        prices::find(&table, "claude-opus-4-9").unwrap().model,
        "claude-opus-4-*"
    );
    assert_eq!(
        prices::find(&table, "claude-opus-4-8").unwrap().model,
        "claude-opus-4-8",
        "an exact id beats a prefix"
    );
}

#[test]
fn cost_uses_every_rate_and_refuses_to_guess_a_missing_one() {
    let price = prices::find(&prices::defaults(), "claude-opus-5-5")
        .cloned()
        .unwrap();
    assert_eq!(
        (price.cache_write, price.cache_write_1h),
        (Some(5.0), Some(8.0))
    );
    let used = Tokens {
        input: 1_000_000,
        output: 500_000,
        cache_read: 2_000_000,
        cache_write: 3_000_000,
        cache_write_1h: 1_000_000,
        reasoning: 400_000,
        requests: 9,
    };
    // 4 + 10 + 0.4 + 2M × 5 + 1M × 8 = 32.4; reasoning is inside output.
    let cost = prices::cost(Some(&price), &used).unwrap();
    assert!((cost - 32.4).abs() < 1e-9, "{cost}");
    assert_eq!(prices::cost(None, &used), None);
    let partial = Price {
        cache_read: None,
        ..price.clone()
    };
    assert_eq!(prices::cost(Some(&partial), &used), None);
    assert_eq!(
        prices::cost(Some(&partial), &tokens(1_000_000, 0)),
        Some(4.0)
    );
}

#[test]
fn the_price_table_saves_checks_and_falls_back_to_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("usage-prices.json");
    assert_eq!(prices::load(&file).unwrap(), prices::defaults());
    let mine = vec![Price {
        model: "  GPT-6-Astra ".into(),
        input: Some(1.5),
        output: Some(6.0),
        cache_read: Some(0.15),
        cache_write: None,
        cache_write_1h: None,
    }];
    let saved = prices::save(&file, mine).unwrap();
    assert_eq!(saved[0].model, "gpt-6-astra");
    assert_eq!(prices::load(&file).unwrap(), saved);
    let twice = vec![saved[0].clone(), saved[0].clone()];
    assert!(prices::save(&file, twice).is_err());
    let negative = vec![Price {
        input: Some(-1.0),
        ..saved[0].clone()
    }];
    assert!(prices::save(&file, negative).is_err());
    let unnamed = vec![Price {
        model: " ".into(),
        ..saved[0].clone()
    }];
    assert!(prices::save(&file, unnamed).is_err());
    assert_eq!(
        prices::load(&file).unwrap(),
        saved,
        "a refused save changes nothing"
    );
    fs::write(&file, "{oops").unwrap();
    assert!(prices::load(&file).is_err());
}

/// Reads this computer's real transcripts, read-only, and prints only totals.
/// `cargo test -p convoy-core usage::tests::real -- --ignored --nocapture`
#[test]
#[ignore]
fn real_transcripts() {
    let home = crate::storage::home(crate::Platform::current());
    let homes = vec![
        std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude")),
        std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex")),
    ];
    let scratch = tempfile::tempdir().unwrap();
    let cache_file = scratch.path().join("usage-cache.json");
    let started = std::time::Instant::now();
    let sources = scan::sources(&homes);
    let mut cache = Cache::default();
    let stats = scan::scan(&mut cache, &sources, &mut |_| {});
    let first = started.elapsed();
    cache.save(&cache_file).unwrap();
    let size = fs::metadata(&cache_file).unwrap().len();
    let started = std::time::Instant::now();
    let mut cache = Cache::load(&cache_file);
    let again = scan::scan(&mut cache, &scan::sources(&homes), &mut |_| {});
    let second = started.elapsed();
    let started = std::time::Instant::now();
    let report = report(
        &cache,
        &[],
        &home,
        &prices::defaults(),
        &crate::automations::LocalZone,
        &Query {
            days: 90,
            project_id: None,
            agent: None,
            now: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        },
    );
    let reporting = started.elapsed();
    let claude = totals(&cache, Agent::Claude);
    let codex = totals(&cache, Agent::Codex);
    println!(
        "files: {} (claude {}, codex {}), read {} MB",
        stats.files,
        sources.iter().filter(|s| s.agent == Agent::Claude).count(),
        sources.iter().filter(|s| s.agent == Agent::Codex).count(),
        stats.bytes / 1_000_000
    );
    println!("first scan: {first:?}; cached rescan: {second:?} ({} files read, {} bytes); report: {reporting:?}; cache file {} KB", again.read, again.bytes, size / 1000);
    println!("claude all-time: total {} tokens, {} replies (in {}, out {}, cache read {}, cache write {})", claude.total(), claude.requests, claude.input, claude.output, claude.cache_read, claude.cache_write);
    println!(
        "codex all-time: total {} tokens, {} turns (in {}, out {}, cache read {})",
        codex.total(),
        codex.requests,
        codex.input,
        codex.output,
        codex.cache_read
    );
    println!("last 90 days: {} tokens, est. ${:.2} for priced models, {} active days, {} models, {} unpriced", report.total, report.cost, report.active_days, report.models.len(), report.unpriced.len());
}
