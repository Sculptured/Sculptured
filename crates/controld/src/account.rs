//! The account page's summary of your own use: tokens and costs across every team, by day, by CLI and
//! by team, plus a few activity counts. Read from this Sculptured's event journal, so it covers what ran
//! here. Costs are only those the CLI reported or a list price gives; reports without one are counted.
use crate::error::ApiError;
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use serde::Serialize;
use std::collections::BTreeMap;
use ts_rs::TS;

const DAYS: i64 = 30;
const TOP_TEAMS: usize = 5;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct UsageTotals {
    #[ts(type = "number")]
    pub input: u64,
    #[ts(type = "number")]
    pub output: u64,
    #[ts(type = "number")]
    pub cache_read: u64,
    #[ts(type = "number")]
    pub cache_write: u64,
    /// How many usage reports the CLIs made.
    #[ts(type = "number")]
    pub reports: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct SourceCost {
    pub source: String,
    pub usd: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct CostSummary {
    pub usd: f64,
    pub by_source: Vec<SourceCost>,
    /// Reports with no cost (no list price for that model, or an older CLI): counted, never guessed.
    #[ts(type = "number")]
    pub reports_without_cost: u64,
    /// When the OpenAI list prices used for Codex were last checked.
    pub openai_prices_checked: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct DayTokens {
    /// UTC date, `YYYY-MM-DD`.
    pub day: String,
    #[ts(type = "number")]
    pub tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct RuntimeTokens {
    pub runtime: String,
    #[ts(type = "number")]
    pub tokens: u64,
    pub usd: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct TeamTokens {
    pub team: String,
    pub name: String,
    #[ts(type = "number")]
    pub tokens: u64,
    pub usd: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct Activity {
    #[ts(type = "number")]
    pub teams: u64,
    #[ts(type = "number")]
    pub turns_completed: u64,
    #[ts(type = "number")]
    pub agent_sessions: u64,
    #[ts(type = "number")]
    pub messages_you_sent: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct AccountUsage {
    pub totals: UsageTotals,
    pub cost: CostSummary,
    /// The last 30 days, oldest first, days without use included.
    pub by_day: Vec<DayTokens>,
    pub by_runtime: Vec<RuntimeTokens>,
    /// The teams that used the most tokens, most first.
    pub top_teams: Vec<TeamTokens>,
    pub activity: Activity,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/account/usage", axum::routing::get(account_usage))
}

async fn account_usage(State(st): State<AppState>) -> Response {
    crate::error::respond(
        tokio::task::spawn_blocking(move || summarize(&st, model::now_ms()))
            .await
            .map_err(|_| ApiError::internal("usage summary failed"))
            .and_then(|r| r.map_err(ApiError::from)),
    )
}

fn summarize(st: &AppState, now_ms: i64) -> rusqlite::Result<AccountUsage> {
    let (rows, names, activity) = {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        (
            store.usage_rows()?,
            store.team_names()?,
            store.activity_counts()?,
        )
    };
    let names: BTreeMap<String, String> = names.into_iter().collect();
    let today = now_ms.div_euclid(DAY_MS);
    let mut totals = UsageTotals::default();
    let (mut usd, mut without_cost) = (0.0, 0u64);
    let mut by_source: BTreeMap<String, f64> = BTreeMap::new();
    let mut by_day = vec![0u64; DAYS as usize];
    let mut by_runtime: BTreeMap<String, (u64, f64)> = BTreeMap::new();
    let mut by_team: BTreeMap<String, (u64, f64)> = BTreeMap::new();
    for r in rows {
        let tokens = r.input + r.output + r.cache_read + r.cache_write;
        totals.input += r.input;
        totals.output += r.output;
        totals.cache_read += r.cache_read;
        totals.cache_write += r.cache_write;
        totals.reports += u64::from(!r.in_turn);
        let cost = r.cost_usd.unwrap_or(0.0);
        match (r.cost_usd, r.cost_source) {
            (Some(c), Some(source)) => {
                usd += c;
                *by_source.entry(source).or_default() += c;
            }
            _ if r.in_turn => {}
            _ => without_cost += 1,
        }
        let age = today - r.at_ms.div_euclid(DAY_MS);
        if (0..DAYS).contains(&age) {
            by_day[(DAYS - 1 - age) as usize] += tokens;
        }
        let rt = by_runtime.entry(r.runtime).or_default();
        rt.0 += tokens;
        rt.1 += cost;
        let t = by_team.entry(r.team).or_default();
        t.0 += tokens;
        t.1 += cost;
    }
    let mut top_teams: Vec<TeamTokens> = by_team
        .into_iter()
        .map(|(team, (tokens, usd))| TeamTokens {
            name: names.get(&team).cloned().unwrap_or_else(|| team.clone()),
            team,
            tokens,
            usd,
        })
        .filter(|t| t.tokens > 0)
        .collect();
    top_teams.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.team.cmp(&b.team)));
    top_teams.truncate(TOP_TEAMS);
    let mut by_runtime: Vec<RuntimeTokens> = by_runtime
        .into_iter()
        .map(|(runtime, (tokens, usd))| RuntimeTokens {
            runtime,
            tokens,
            usd,
        })
        .collect();
    by_runtime.sort_by_key(|runtime| std::cmp::Reverse(runtime.tokens));
    Ok(AccountUsage {
        totals,
        cost: CostSummary {
            usd,
            by_source: by_source
                .into_iter()
                .map(|(source, usd)| SourceCost { source, usd })
                .collect(),
            reports_without_cost: without_cost,
            openai_prices_checked: model::pricing::OPENAI_PRICES_CHECKED.to_string(),
        },
        by_day: by_day
            .into_iter()
            .enumerate()
            .map(|(i, tokens)| DayTokens {
                day: utc_date(today - (DAYS - 1 - i as i64)),
                tokens,
            })
            .collect(),
        by_runtime,
        top_teams,
        activity: Activity {
            teams: activity.teams,
            turns_completed: activity.turns_completed,
            agent_sessions: activity.agent_sessions,
            messages_you_sent: activity.messages_you_sent,
        },
    })
}

/// `YYYY-MM-DD` for a count of days since 1970-01-01 (civil-from-days, Howard Hinnant).
fn utc_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn utc_dates_from_day_numbers() {
        assert_eq!(super::utc_date(0), "1970-01-01");
        assert_eq!(super::utc_date(20_739), "2026-10-13");
        assert_eq!(super::utc_date(11_016), "2000-02-29");
    }
}
