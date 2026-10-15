//! Token usage and its estimated cost, read from the transcripts Claude Code
//! and Codex keep on disk — every conversation on the computer, not only the
//! ones Convoy started.
//!
//! [`scan`] keeps a per-file cache up to date; [`report`] turns it into what
//! the Usage page shows for a range of local days, a project and an agent.
//! [`prices`] holds the editable table the cost is estimated from.

pub mod prices;
pub mod scan;
#[cfg(test)]
mod tests;

use crate::automations::schedule::{local_day, Zone};
use crate::provider::sweep::{folder_match, normalise, Target};
use crate::time::civil_from_days;
use crate::workspace::model::Agent;
use prices::Price;
use scan::{Cache, SLOT};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

/// Days the heatmap covers: twelve weeks.
pub const HEATMAP_DAYS: i64 = 84;

/// Token counts of one kind of work. `input` is what was neither read from
/// nor written to the cache; `reasoning` is the part of `output` spent
/// thinking; `cache_write_1h` the part of `cache_write` kept for an hour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cache_write_1h: u64,
    pub reasoning: u64,
    /// Model replies (Claude) or turns (Codex).
    pub requests: u64,
}

impl Tokens {
    /// Every token sent or received, each counted once.
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }

    pub fn to_array(self) -> [u64; 7] {
        [
            self.input,
            self.output,
            self.cache_read,
            self.cache_write,
            self.cache_write_1h,
            self.reasoning,
            self.requests,
        ]
    }

    pub fn from_array(values: [u64; 7]) -> Tokens {
        let [input, output, cache_read, cache_write, cache_write_1h, reasoning, requests] = values;
        Tokens {
            input,
            output,
            cache_read,
            cache_write,
            cache_write_1h,
            reasoning,
            requests,
        }
    }

    pub fn add(&mut self, other: &Tokens) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.cache_write_1h += other.cache_write_1h;
        self.reasoning += other.reasoning;
        self.requests += other.requests;
    }
}

/// What to report on.
#[derive(Debug, Clone)]
pub struct Query {
    /// Local days up to and including today.
    pub days: u32,
    pub project_id: Option<String>,
    pub agent: Option<Agent>,
    /// Seconds since the epoch.
    pub now: i64,
}

/// One local day of the chart.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Day {
    /// `YYYY-MM-DD`.
    pub day: String,
    pub claude: u64,
    pub codex: u64,
    /// Estimated dollars for the priced part.
    pub cost: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelRow {
    pub agent: Agent,
    pub model: String,
    pub tokens: Tokens,
    pub total: u64,
    /// None when the model has no price.
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectRow {
    /// None for work done outside every project.
    pub project_id: Option<String>,
    pub tokens: Tokens,
    pub total: u64,
    /// Estimated dollars for its priced models.
    pub cost: f64,
    /// Some of its tokens are of a model with no price.
    pub partial: bool,
    pub claude: u64,
    pub codex: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HeatDay {
    pub day: String,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub days: Vec<Day>,
    pub models: Vec<ModelRow>,
    pub projects: Vec<ProjectRow>,
    /// The last twelve weeks, oldest first, whatever the range.
    pub heatmap: Vec<HeatDay>,
    pub tokens: Tokens,
    pub total: u64,
    /// Estimated dollars for the priced models.
    pub cost: f64,
    /// Models in the range with no price, left out of `cost`.
    pub unpriced: Vec<String>,
    pub active_days: usize,
    pub top_model: Option<String>,
    /// Transcripts the totals come from.
    pub files: usize,
}

/// `YYYY-MM-DD` of a day counted from 1970-01-01.
pub fn date(day: i64) -> String {
    let (year, month, day) = civil_from_days(day);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The project a working folder belongs to: the deepest project folder or
/// session worktree holding it.
pub fn project_of(cwd: &str, targets: &[Target], home: &Path) -> Option<String> {
    if cwd.is_empty() {
        return None;
    }
    let path = normalise(cwd, home, None)?;
    folder_match(&path, targets).map(|target| target.project_id.clone())
}

/// What the Usage page shows.
pub fn report(
    cache: &Cache,
    targets: &[Target],
    home: &Path,
    prices: &[Price],
    zone: &dyn Zone,
    query: &Query,
) -> Report {
    let today = local_day(zone, query.now);
    let first = today - i64::from(query.days.max(1)) + 1;
    let heat_first = today - HEATMAP_DAYS + 1;
    let earliest = first.min(heat_first);

    let mut projects_by_cwd: HashMap<&str, Option<String>> = HashMap::new();
    let mut by_day: BTreeMap<i64, (u64, u64, f64)> = BTreeMap::new();
    let mut heat: BTreeMap<i64, u64> = BTreeMap::new();
    let mut by_model: BTreeMap<(&'static str, String), Tokens> = BTreeMap::new();
    let mut by_project: BTreeMap<Option<String>, (Tokens, f64, bool, u64, u64)> = BTreeMap::new();
    let mut price_of: HashMap<String, Option<&Price>> = HashMap::new();

    for state in cache.files.values() {
        let Some(agent) = state.agent else {
            continue;
        };
        if query.agent.is_some_and(|wanted| wanted != agent) {
            continue;
        }
        for (slot, model, cwd, counts) in &state.buckets {
            let day = local_day(zone, slot * SLOT);
            if day < earliest || day > today {
                continue;
            }
            let cwd = state
                .cwds
                .get(*cwd as usize)
                .map(String::as_str)
                .unwrap_or("");
            let project = projects_by_cwd
                .entry(cwd)
                .or_insert_with(|| project_of(cwd, targets, home))
                .clone();
            if let Some(wanted) = &query.project_id {
                if project.as_deref() != Some(wanted.as_str()) {
                    continue;
                }
            }
            let tokens = Tokens::from_array(*counts);
            let total = tokens.total();
            if day >= heat_first {
                *heat.entry(day).or_default() += total;
            }
            if day < first {
                continue;
            }
            let model = state
                .models
                .get(*model as usize)
                .cloned()
                .unwrap_or_default();
            let price = *price_of
                .entry(model.clone())
                .or_insert_with(|| prices::find(prices, &model));
            let cost = prices::cost(price, &tokens);
            let entry = by_day.entry(day).or_default();
            match agent {
                Agent::Claude => entry.0 += total,
                Agent::Codex => entry.1 += total,
                Agent::Gemini => {}
            }
            entry.2 += cost.unwrap_or(0.0);
            by_model
                .entry((agent.as_str(), model))
                .or_default()
                .add(&tokens);
            let row = by_project.entry(project).or_default();
            row.0.add(&tokens);
            row.1 += cost.unwrap_or(0.0);
            row.2 |= cost.is_none() && total > 0;
            match agent {
                Agent::Claude => row.3 += total,
                Agent::Codex => row.4 += total,
                Agent::Gemini => {}
            }
        }
    }

    let days: Vec<Day> = (first..=today)
        .map(|day| {
            let (claude, codex, cost) = by_day.get(&day).copied().unwrap_or_default();
            Day {
                day: date(day),
                claude,
                codex,
                cost,
            }
        })
        .collect();
    let heatmap = (heat_first..=today)
        .map(|day| HeatDay {
            day: date(day),
            total: heat.get(&day).copied().unwrap_or(0),
        })
        .collect();

    let mut models: Vec<ModelRow> = by_model
        .into_iter()
        .filter(|(_, tokens)| tokens.total() > 0)
        .map(|((agent, model), tokens)| ModelRow {
            agent: Agent::parse(agent).unwrap_or(Agent::Claude),
            cost: prices::cost(prices::find(prices, &model), &tokens),
            model,
            total: tokens.total(),
            tokens,
        })
        .collect();
    models.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.model.cmp(&b.model)));

    let mut projects: Vec<ProjectRow> = by_project
        .into_iter()
        .filter(|(_, row)| row.0.total() > 0)
        .map(
            |(project_id, (tokens, cost, partial, claude, codex))| ProjectRow {
                project_id,
                total: tokens.total(),
                tokens,
                cost,
                partial,
                claude,
                codex,
            },
        )
        .collect();
    projects.sort_by_key(|row| std::cmp::Reverse(row.total));

    let mut tokens = Tokens::default();
    for row in &models {
        tokens.add(&row.tokens);
    }
    let unpriced: BTreeSet<String> = models
        .iter()
        .filter(|row| row.cost.is_none())
        .map(|row| row.model.clone())
        .collect();
    Report {
        active_days: days.iter().filter(|day| day.claude + day.codex > 0).count(),
        cost: models.iter().filter_map(|row| row.cost).sum(),
        top_model: models.first().map(|row| row.model.clone()),
        total: tokens.total(),
        tokens,
        unpriced: unpriced.into_iter().collect(),
        files: cache.files.len(),
        days,
        models,
        projects,
        heatmap,
    }
}
