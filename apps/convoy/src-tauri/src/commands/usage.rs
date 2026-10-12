//! The Usage page: tokens and estimated cost from every Claude Code and Codex
//! transcript on this computer. The rules are `convoy_core::usage`; this keeps
//! the cache in memory, scans in the background and tells the window when
//! fresher numbers are ready (`usage:updated`).

use super::Workspace;
use convoy_core::model::Agent;
use convoy_core::provider::sweep::{self, Target};
use convoy_core::usage::prices::{self, Price};
use convoy_core::usage::scan::{self, Cache};
use convoy_core::usage::{self, Query, Report};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, State};

/// How long a finished scan is trusted before the page asks for another.
const FRESH: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct Usage {
    cache: Mutex<Option<Arc<Cache>>>,
    scanning: AtomicBool,
    scanned_at: Mutex<Option<Instant>>,
}

#[derive(Serialize)]
pub struct UsageView {
    #[serde(flatten)]
    pub report: Report,
    /// A scan is running; `usage:updated` follows when it ends.
    pub scanning: bool,
    /// No scan has finished since Convoy started: the numbers are the last
    /// session's, or none.
    pub stale: bool,
    /// The price table could not be read, so the defaults were used.
    pub prices_error: Option<String>,
}

#[derive(Serialize)]
pub struct PricesView {
    pub prices: Vec<Price>,
    pub defaults: Vec<Price>,
}

/// Every home an agent may have written to: the system login's (honouring
/// `CLAUDE_CONFIG_DIR` and `CODEX_HOME`), each managed account's and each
/// session's recorded one.
fn homes(core: &convoy_core::Workspace, storage: &convoy_core::Storage) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Ok(roots) = super::sweep::roots(core, storage) {
        result.push(roots.claude);
        result.push(roots.codex);
    }
    if let Ok(entries) = std::fs::read_dir(storage.accounts()) {
        let mut managed: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect();
        managed.sort();
        result.extend(managed);
    }
    for session in &core.state().sessions {
        if let Some(home) = &session.agent_home {
            if !result.contains(home) {
                result.push(home.clone());
            }
        }
    }
    result
}

fn cached(usage: &Usage, storage: &convoy_core::Storage) -> Arc<Cache> {
    let mut slot = usage
        .cache
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    slot.get_or_insert_with(|| Arc::new(Cache::load(&storage.usage_cache())))
        .clone()
}

/// Starts a scan unless one is running or the last is recent enough.
fn refresh(
    app: tauri::AppHandle,
    usage: Arc<Usage>,
    storage: convoy_core::Storage,
    homes: Vec<PathBuf>,
) -> bool {
    let recent = usage
        .scanned_at
        .lock()
        .ok()
        .and_then(|at| *at)
        .is_some_and(|at| at.elapsed() < FRESH);
    if recent || usage.scanning.swap(true, Ordering::SeqCst) {
        return usage.scanning.load(Ordering::SeqCst);
    }
    let start = cached(&usage, &storage);
    std::thread::spawn(move || {
        let file = storage.usage_cache();
        let mut cache = (*start).clone();
        let sources = scan::sources(&homes);
        scan::scan(&mut cache, &sources, &mut |partial| {
            let _ = partial.save(&file);
        });
        let _ = cache.save(&file);
        if let Ok(mut slot) = usage.cache.lock() {
            *slot = Some(Arc::new(cache));
        }
        if let Ok(mut at) = usage.scanned_at.lock() {
            *at = Some(Instant::now());
        }
        usage.scanning.store(false, Ordering::SeqCst);
        let _ = app.emit("usage:updated", ());
    });
    true
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0)
}

/// The report for a range of local days, from what is cached now. A scan is
/// started in the background when the numbers may be old; the window asks
/// again on `usage:updated`.
#[tauri::command]
pub async fn usage_stats(
    days: u32,
    project_id: Option<String>,
    agent: Option<String>,
    app: tauri::AppHandle,
    workspace: State<'_, Workspace>,
    usage: State<'_, Arc<Usage>>,
) -> Result<UsageView, String> {
    let storage = workspace.storage.clone();
    let (targets, homes, home): (Vec<Target>, Vec<PathBuf>, PathBuf) = workspace.act(|core| {
        let home = convoy_core::storage::home(convoy_core::Platform::current());
        Ok((
            sweep::targets(core.state(), &home),
            homes(core, &storage),
            home,
        ))
    })?;
    let usage = usage.inner().clone();
    let scanning = refresh(app, usage.clone(), storage.clone(), homes);
    let stale = usage
        .scanned_at
        .lock()
        .map(|at| at.is_none())
        .unwrap_or(true);
    let query = Query {
        days: days.clamp(1, 366),
        project_id: project_id.filter(|id| !id.is_empty()),
        agent: agent.as_deref().and_then(Agent::parse),
        now: now(),
    };
    let cache = cached(&usage, &storage);
    tauri::async_runtime::spawn_blocking(move || {
        let (table, prices_error) = match prices::load(&storage.usage_prices()) {
            Ok(table) => (table, None),
            Err(error) => (prices::defaults(), Some(error.to_string())),
        };
        let report = usage::report(
            &cache,
            &targets,
            &home,
            &table,
            &convoy_core::automations::LocalZone,
            &query,
        );
        UsageView {
            report,
            scanning,
            stale,
            prices_error,
        }
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn usage_prices(workspace: State<'_, Workspace>) -> Result<PricesView, String> {
    Ok(PricesView {
        prices: prices::load(&workspace.storage.usage_prices())
            .map_err(|error| error.to_string())?,
        defaults: prices::defaults(),
    })
}

#[tauri::command]
pub fn usage_prices_save(
    prices: Vec<Price>,
    app: tauri::AppHandle,
    workspace: State<'_, Workspace>,
) -> Result<Vec<Price>, String> {
    let saved = prices::save(&workspace.storage.usage_prices(), prices)
        .map_err(|error| error.to_string())?;
    let _ = app.emit("usage:updated", ());
    Ok(saved)
}

#[tauri::command]
pub fn usage_prices_reset(
    app: tauri::AppHandle,
    workspace: State<'_, Workspace>,
) -> Result<Vec<Price>, String> {
    let saved = prices::save(&workspace.storage.usage_prices(), prices::defaults())
        .map_err(|error| error.to_string())?;
    let _ = app.emit("usage:updated", ());
    Ok(saved)
}
