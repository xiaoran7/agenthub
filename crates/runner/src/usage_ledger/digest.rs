use super::*;

/// What the page's overview needs from the calls of one period: the totals,
/// the series the trend is drawn from, the ranking dimensions, the heatmap's
/// days, the sessions' rows and the request list.
#[derive(Debug, Default, Serialize)]
pub struct LedgerDigest {
    pub period: &'static str,
    /// The first and last days the period covers, as `YYYY-MM-DD` in local
    /// time.
    pub from: String,
    pub to: String,
    /// The heatmap's window: 53 weeks ending today, whatever the period is —
    /// the calendar is about the year, the totals are about the period.
    pub heat_from: String,
    pub heat_to: String,
    pub calls: u64,
    pub sessions: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub tokens: u64,
    pub cost: f64,
    /// Calls that failed and nothing answered.
    pub errors: u64,
    /// Failed calls a later try answered.
    pub recovered: u64,
    /// Calls that answered an earlier failure.
    pub retried: u64,
    /// The period's measured speed and whole-turn floor, summed over every
    /// reply the same way the rankings are (see [`Timing`]).
    #[serde(flatten)]
    pub timing: Timing,
    /// Calls grouped into the period's buckets: hours for a day, days for a
    /// week, weeks for longer spans.
    pub series: Vec<SeriesPoint>,
    /// One entry per ranking dimension: model, model at its agent, agent.
    pub by: Breakdown,
    /// One row per local day with activity, for the heatmap.
    pub heat: Vec<DayPoint>,
    /// One row per session, newest first.
    pub session_rows: Vec<SessionRow>,
    /// The most recent calls, newest first, for the request table.
    pub recent: Vec<LedgerRecord>,
}

#[derive(Debug, Default, Serialize)]
pub struct SeriesPoint {
    /// The bucket's start, in milliseconds since the epoch.
    pub t: i64,
    pub calls: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub tokens: u64,
    pub cost: f64,
    pub errors: u64,
    /// The bucket's own floor, over every reply in it (see [`Timing`]). A
    /// speed is not a share of anything, so the chart draws it as a track
    /// with each model's mark on it.
    #[serde(flatten)]
    pub timing: Timing,
    /// The bucket's calls by each ranking dimension, so the chart can stack
    /// them in the ranking's colours. Only the ids that were active in the
    /// bucket are here; the chart fills the rest with zero.
    pub by: SeriesBy,
}

#[derive(Debug, Default, Serialize)]
pub struct SeriesBy {
    pub model: HashMap<String, Part>,
    pub model_at: HashMap<String, Part>,
    pub agent: HashMap<String, Part>,
    pub provider: HashMap<String, Part>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Part {
    pub calls: u64,
    pub tokens: u64,
    pub output: u64,
    pub cost: f64,
    /// The floor of the replies of this part: their output over the whole
    /// turn each was asked in (see [`Timing`]).
    #[serde(flatten)]
    pub timing: Timing,
}

/// The speed a group of replies reads at, as magpie tells it for a file
/// source: one figure, a floor.
///
/// A transcript file never says when an answer began — its first line is the
/// first content block, not the first token — so a reply's speed after its
/// first token cannot be measured from it. What the file does say is when the
/// reply was asked for and when it ended, and the tokens over that whole turn
/// are the least the reply wrote at. A speed is summed, not averaged: the
/// group's output over the group's time. An average of per-reply rates lets
/// one short, fast reply stand for a long, slow one.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Timing {
    pub floor_out: u64,
    pub floor_ms: u64,
    pub floor_n: u64,
}

impl Timing {
    fn observe(&mut self, record: &LedgerRecord) {
        if let Some(ms) = record.floor_ms() {
            self.floor_out = self.floor_out.saturating_add(record.output);
            self.floor_ms = self.floor_ms.saturating_add(ms);
            self.floor_n = self.floor_n.saturating_add(1);
        }
    }

    /// The least the replies wrote at, in tokens a second; 0 for none timed.
    pub fn floor(&self) -> f64 {
        if self.floor_ms == 0 {
            0.0
        } else {
            self.floor_out as f64 / (self.floor_ms as f64 / 1000.0)
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Breakdown {
    pub model: Vec<RankRow>,
    pub model_at: Vec<RankRow>,
    pub agent: Vec<RankRow>,
    pub provider: Vec<RankRow>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RankRow {
    pub id: String,
    pub name: String,
    /// The agent of a `model_at` row (`claude_code` and so on).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub agent: String,
    pub calls: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub tokens: u64,
    pub cost: f64,
    pub errors: u64,
    /// The row's speed and floor (see [`Timing`]).
    #[serde(flatten)]
    pub timing: Timing,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DayPoint {
    /// `YYYY-MM-DD`, local.
    pub date: String,
    pub calls: u64,
    pub tokens: u64,
    pub cost: f64,
    pub errors: u64,
}

/// One session of the period, as the 会话 and 上下文 tabs read it.
#[derive(Debug, Default, Serialize)]
pub struct SessionRow {
    pub id: String,
    /// The agent the session's calls belong to; a session that changed agent
    /// (a transcript resumed in another entrypoint) says so by its latest.
    pub agent: &'static str,
    pub agent_name: &'static str,
    pub cwd: String,
    pub calls: u64,
    pub tokens: u64,
    pub cost: f64,
    pub errors: u64,
    /// The first and last call of the session within the period.
    pub first: i64,
    pub last: i64,
    /// The models the session used, most tokens first.
    pub models: Vec<String>,
    /// The model of the session's latest call.
    pub model: String,
    /// The prompt size of the session's latest successful call: what the
    /// model was handed at the end, i.e. how full the context had grown.
    pub context: u64,
}

/// Aggregates the records of a period.
///
/// `recent_limit` bounds the request table's rows; the series, rankings and
/// heatmap are always computed from every call in the period.
pub fn digest(records: &[LedgerRecord], period: Period, recent_limit: usize) -> LedgerDigest {
    digest_with_heat(records, period, recent_limit, None)
}

pub(crate) fn digest_with_heat(
    records: &[LedgerRecord],
    period: Period,
    recent_limit: usize,
    precomputed_heat: Option<Vec<DayPoint>>,
) -> LedgerDigest {
    let now = chrono::Local::now().timestamp_millis();
    let since = period.since_ms(now);
    let selected: Vec<&LedgerRecord> = records.iter().filter(|r| r.t >= since).collect();

    let mut digest = LedgerDigest {
        period: period.key(),
        ..LedgerDigest::default()
    };
    let (from, to) = local_day_bounds(period, now, &selected);
    digest.from = from;
    digest.to = to;
    digest.heat_from = local_day(heat_window_start(now));
    digest.heat_to = local_day(now);

    let bucket = bucket_of(period);
    let mut points: std::collections::BTreeMap<i64, SeriesPoint> = Default::default();
    let mut models: HashMap<String, RankRow> = HashMap::new();
    let mut model_at: HashMap<(String, &'static str), RankRow> = HashMap::new();
    let mut agents: HashMap<&'static str, RankRow> = HashMap::new();
    let mut providers: HashMap<String, RankRow> = HashMap::new();
    let mut sessions: HashMap<&str, SessionAgg> = HashMap::new();

    let mut recent: Vec<&LedgerRecord> = Vec::new();
    for record in &selected {
        let failed = record.error.is_some() && !record.recovered;
        digest.calls = digest.calls.saturating_add(1);
        digest.input = digest.input.saturating_add(record.input);
        digest.output = digest.output.saturating_add(record.output);
        digest.cache_read = digest.cache_read.saturating_add(record.cache_read);
        digest.cache_write = digest.cache_write.saturating_add(record.cache_write);
        digest.cost += record.cost;
        if failed {
            digest.errors = digest.errors.saturating_add(1);
        }
        if record.recovered {
            digest.recovered = digest.recovered.saturating_add(1);
        }
        if record.retried {
            digest.retried = digest.retried.saturating_add(1);
        }
        digest.timing.observe(record);

        let at = bucket_start(record.t, bucket);
        let point = points.entry(at).or_insert_with(|| SeriesPoint {
            t: at,
            ..SeriesPoint::default()
        });
        accumulate(point, record);

        sessions
            .entry(record.session.as_ref())
            .or_insert_with(|| SessionAgg {
                id: record.session.to_string(),
                first: record.t,
                ..SessionAgg::default()
            })
            .observe(record, failed);

        // A failed call names no model; the rankings are about what answered.
        if record.error.is_none() {
            rank(
                models
                    .entry(record.model.to_string())
                    .or_insert_with(|| RankRow {
                        id: record.model.to_string(),
                        name: record.model.to_string(),
                        ..RankRow::default()
                    }),
                record,
            );
            rank(
                model_at
                    .entry((record.model.to_string(), record.agent))
                    .or_insert_with(|| RankRow {
                        id: format!("{}@{}", record.model, record.agent),
                        name: record.model.to_string(),
                        agent: record.agent.to_owned(),
                        ..RankRow::default()
                    }),
                record,
            );
            rank(
                agents.entry(record.agent).or_insert_with(|| RankRow {
                    id: record.agent.to_owned(),
                    name: agent_label(record.agent).to_owned(),
                    agent: record.agent.to_owned(),
                    ..RankRow::default()
                }),
                record,
            );
            let prov_label = if record.provider.is_empty() {
                resolve_provider_label(record.agent, &record.model, &HashMap::new())
            } else {
                Arc::clone(&record.provider)
            };
            rank(
                providers
                    .entry(prov_label.to_string())
                    .or_insert_with(|| RankRow {
                        id: prov_label.to_string(),
                        name: prov_label.to_string(),
                        ..RankRow::default()
                    }),
                record,
            );
        }
        recent.push(record);
    }

    digest.sessions = sessions.len() as u64;
    digest.tokens = digest
        .input
        .saturating_add(digest.output)
        .saturating_add(digest.cache_read)
        .saturating_add(digest.cache_write);
    digest.cost = round_money(digest.cost);
    // the trend reads left to right: a HashMap's order would scatter the
    // buckets across the chart
    digest.series = points.into_values().collect();
    digest.series.sort_by_key(|p| p.t);
    digest.heat = precomputed_heat.unwrap_or_else(|| build_heat_days(records, now));

    // every ranking sorts by tokens, then by name so a tie never reshuffles
    let sorted = |mut rows: Vec<RankRow>| {
        rows.sort_by(|a, b| {
            b.tokens
                .cmp(&a.tokens)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.agent.cmp(&b.agent))
        });
        rows
    };
    digest.by.model = sorted(models.into_values().collect());
    digest.by.model_at = sorted(model_at.into_values().collect());
    digest.by.agent = sorted(agents.into_values().collect());
    digest.by.provider = sorted(providers.into_values().collect());

    let mut session_rows: Vec<SessionRow> = sessions.into_values().map(SessionRow::from).collect();
    session_rows.sort_by(|a, b| b.last.cmp(&a.last).then_with(|| a.id.cmp(&b.id)));
    session_rows.truncate(SESSION_ROW_LIMIT);
    digest.session_rows = session_rows;

    recent.truncate(recent_limit);
    digest.recent = recent.into_iter().cloned().collect();
    digest
}

/// The most session rows the page is given; the list is a reading list, not
/// the store's index.
const SESSION_ROW_LIMIT: usize = 300;

#[derive(Debug, Default)]
struct SessionAgg {
    id: String,
    agent: &'static str,
    cwd: String,
    calls: u64,
    tokens: u64,
    cost: f64,
    errors: u64,
    first: i64,
    last: i64,
    latest_t: i64,
    model: String,
    context: u64,
    models: HashMap<String, u64>,
}

impl SessionAgg {
    fn observe(&mut self, record: &LedgerRecord, failed: bool) {
        if self.agent.is_empty() {
            self.agent = record.agent;
        }
        if self.cwd.is_empty() && !record.cwd.is_empty() {
            self.cwd = record.cwd.to_string();
        }
        self.calls = self.calls.saturating_add(1);
        self.tokens = self.tokens.saturating_add(record.total_tokens());
        self.cost += record.cost;
        if failed {
            self.errors = self.errors.saturating_add(1);
        }
        self.first = self.first.min(record.t);
        self.last = self.last.max(record.t);
        // the latest successful call is the session's last state: its model
        // and the prompt it was handed
        if record.error.is_none() && record.t >= self.latest_t {
            self.latest_t = record.t;
            self.agent = record.agent;
            self.model = record.model.to_string();
            self.context = record.prompt_tokens();
        }
        if record.error.is_none() && !record.model.is_empty() {
            *self.models.entry(record.model.to_string()).or_insert(0) += record.total_tokens();
        }
    }
}

impl From<SessionAgg> for SessionRow {
    fn from(mut agg: SessionAgg) -> Self {
        let mut models: Vec<(String, u64)> = agg.models.drain().collect();
        models.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        SessionRow {
            id: agg.id,
            agent: agg.agent,
            agent_name: agent_label(agg.agent),
            cwd: agg.cwd,
            calls: agg.calls,
            tokens: agg.tokens,
            cost: round_money(agg.cost),
            errors: agg.errors,
            first: agg.first,
            last: agg.last,
            models: models.into_iter().take(3).map(|(m, _)| m).collect(),
            model: agg.model,
            context: agg.context,
        }
    }
}

/// Computes the 53-week daily activity heatmap from every record in the calendar
/// window, decoupled from the selected period totals.
pub(crate) fn build_heat_days(records: &[LedgerRecord], now: i64) -> Vec<DayPoint> {
    let heat_since = heat_window_start(now);
    let heat_from = local_day(heat_since);
    let rough_floor = heat_since.saturating_sub(86_400_000);
    let mut heat: std::collections::BTreeMap<String, DayPoint> = Default::default();
    for record in records {
        if record.t < rough_floor {
            continue;
        }
        let day = local_day(record.t);
        if day.as_str() < heat_from.as_str() {
            continue;
        }
        let failed = record.error.is_some() && !record.recovered;
        let day_point = heat.entry(day.clone()).or_insert_with(|| DayPoint {
            date: day,
            ..DayPoint::default()
        });
        day_point.calls = day_point.calls.saturating_add(1);
        day_point.tokens = day_point.tokens.saturating_add(record.total_tokens());
        day_point.cost += record.cost;
        if failed {
            day_point.errors = day_point.errors.saturating_add(1);
        }
    }
    heat.into_values().collect()
}

/// The heatmap's first day: 53 weeks back, Monday-first, so the grid is whole.
pub(crate) fn heat_window_start(now: i64) -> i64 {
    let day = 86_400_000i64;
    let offset = local_offset_ms(now);
    let local = now + offset;
    let day_index = local.div_euclid(day);
    let monday_index = day_index - (day_index + 3).rem_euclid(7);
    (monday_index - 52 * 7) * day - offset
}

fn accumulate(point: &mut SeriesPoint, record: &LedgerRecord) {
    point.calls = point.calls.saturating_add(1);
    point.input = point.input.saturating_add(record.input);
    point.output = point.output.saturating_add(record.output);
    point.cache_read = point.cache_read.saturating_add(record.cache_read);
    point.cache_write = point.cache_write.saturating_add(record.cache_write);
    point.tokens = point.tokens.saturating_add(record.total_tokens());
    point.cost += record.cost;
    if record.error.is_some() && !record.recovered {
        point.errors = point.errors.saturating_add(1);
    }
    point.timing.observe(record);
    if record.error.is_none() {
        part(
            point.by.model.entry(record.model.to_string()).or_default(),
            record,
        );
        part(
            point
                .by
                .model_at
                .entry(format!("{}@{}", record.model, record.agent))
                .or_default(),
            record,
        );
        part(
            point.by.agent.entry(record.agent.to_owned()).or_default(),
            record,
        );
        let prov_label = if record.provider.is_empty() {
            resolve_provider_label(record.agent, &record.model, &HashMap::new())
        } else {
            Arc::clone(&record.provider)
        };
        part(
            point.by.provider.entry(prov_label.to_string()).or_default(),
            record,
        );
    }
}

fn part(p: &mut Part, record: &LedgerRecord) {
    p.calls = p.calls.saturating_add(1);
    p.tokens = p.tokens.saturating_add(record.total_tokens());
    p.output = p.output.saturating_add(record.output);
    p.cost += record.cost;
    p.timing.observe(record);
}

fn rank(row: &mut RankRow, record: &LedgerRecord) {
    row.calls = row.calls.saturating_add(1);
    row.input = row.input.saturating_add(record.input);
    row.output = row.output.saturating_add(record.output);
    row.cache_read = row.cache_read.saturating_add(record.cache_read);
    row.cache_write = row.cache_write.saturating_add(record.cache_write);
    row.tokens = row.tokens.saturating_add(record.total_tokens());
    row.cost += record.cost;
    if record.error.is_some() && !record.recovered {
        row.errors = row.errors.saturating_add(1);
    }
    row.timing.observe(record);
}

fn round_money(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// The agent ids the console shows, with the names it shows them under.
pub fn agent_label(agent: &str) -> &'static str {
    match agent {
        "claude_code" => "Coding Assistant",
        "claude_desktop" => "Claude Desktop",
        "codex" => "Codex",
        "antigravity" => "Antigravity",
        "deepseek_harness" => "DeepSeek Harness",
        "zcode" => "ZCode",
        "grok_build" => "Grok",
        "gemini_cli" => "Gemini CLI",
        _ => "Plugin Agent",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bucket {
    Hour,
    Day,
    Week,
}

fn bucket_of(period: Period) -> Bucket {
    match period {
        Period::Today => Bucket::Hour,
        Period::Days(days) if days <= 31 => Bucket::Day,
        _ => Bucket::Week,
    }
}

pub(crate) fn bucket_start(t: i64, bucket: Bucket) -> i64 {
    let day = 86_400_000i64;
    match bucket {
        Bucket::Hour => {
            let offset = local_offset_ms(t);
            let local = t + offset;
            (local / 3_600_000) * 3_600_000 - offset
        }
        Bucket::Day => {
            let offset = local_offset_ms(t);
            let local = t + offset;
            (local / day) * day - offset
        }
        Bucket::Week => {
            // Monday first, as magpie's heatmap and every weekly chart read
            let offset = local_offset_ms(t);
            let local = t + offset;
            let day_index = local.div_euclid(day);
            let monday_index = day_index - (day_index + 3).rem_euclid(7);
            monday_index * day - offset
        }
    }
}

pub(crate) fn local_day(t: i64) -> String {
    let offset = local_offset_ms(t);
    chrono::DateTime::from_timestamp_millis(t + offset)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// The first and last local day the period covers. For a rolling window the
/// first is the window's own edge; the last is the latest call's day, or
/// today — the heatmap should reach the reader's now, not stop at the last
/// quiet day.
fn local_day_bounds(period: Period, now: i64, selected: &[&LedgerRecord]) -> (String, String) {
    let since = period.since_ms(now);
    let from = local_day(since.max(0));
    let latest = selected.iter().map(|r| r.t).max().unwrap_or(now);
    (from, local_day(latest.max(now)))
}
