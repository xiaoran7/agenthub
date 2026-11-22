use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Today,
    Days(u32),
    All,
}

impl Period {
    /// The period a query string names; anything else reads as "all".
    pub fn parse(raw: Option<&str>) -> Self {
        match raw.unwrap_or("") {
            "today" => Self::Today,
            "7d" => Self::Days(7),
            "30d" => Self::Days(30),
            _ => Self::All,
        }
    }

    /// The first moment of the period, in milliseconds since the epoch. The
    /// day boundaries are local: "today" means the reader's today.
    pub fn since_ms(self, now_ms: i64) -> i64 {
        match self {
            Self::All => i64::MIN,
            Self::Days(days) => {
                let day = 86_400_000i64;
                let local_offset = local_offset_ms(now_ms);
                let local_now = now_ms + local_offset;
                let start_local = (local_now / day - i64::from(days) + 1) * day;
                start_local - local_offset
            }
            Self::Today => {
                let day = 86_400_000i64;
                let local_offset = local_offset_ms(now_ms);
                let local_now = now_ms + local_offset;
                (local_now / day) * day - local_offset
            }
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Days(7) => "7d",
            Self::Days(30) => "30d",
            Self::Days(_) => "all",
            Self::All => "all",
        }
    }
}

/// The machine's UTC offset, read from the clock at `now_ms` rather than from
/// a fixed table: a laptop that travels changes its answer between boots.
pub(crate) fn local_offset_ms(now_ms: i64) -> i64 {
    use chrono::Local;
    let Some(utc) = DateTime::from_timestamp_millis(now_ms) else {
        return 0;
    };
    let local = utc.with_timezone(&Local);
    i64::from(local.offset().local_minus_utc()) * 1000
}

/// Global thread-safe string interner for usage ledger records to eliminate duplicate heap allocations.
static INTERNER: OnceLock<RwLock<HashSet<Arc<str>>>> = OnceLock::new();

/// Interns a string slice into a shared `Arc<str>`.
pub fn intern(s: &str) -> Arc<str> {
    if s.is_empty() {
        static EMPTY: OnceLock<Arc<str>> = OnceLock::new();
        return Arc::clone(EMPTY.get_or_init(|| Arc::from("")));
    }
    let pool = INTERNER.get_or_init(|| RwLock::new(HashSet::new()));
    if let Ok(guard) = pool.read() {
        if let Some(existing) = guard.get(s) {
            return Arc::clone(existing);
        }
    }
    if let Ok(mut guard) = pool.write() {
        if let Some(existing) = guard.get(s) {
            return Arc::clone(existing);
        }
        let arc: Arc<str> = Arc::from(s);
        guard.insert(Arc::clone(&arc));
        arc
    } else {
        Arc::from(s)
    }
}

/// Drops interned strings whose sole remaining reference lives in [`INTERNER`]
/// (`Arc::strong_count == 1`) and shrinks excess bucket capacity.
pub(crate) fn prune_interner() -> usize {
    let Some(pool) = INTERNER.get() else {
        return 0;
    };
    let Ok(mut guard) = pool.write() else {
        return 0;
    };
    let before = guard.len();
    guard.retain(|entry| Arc::strong_count(entry) > 1);
    if guard.capacity() > guard.len().saturating_mul(2).max(64) {
        guard.shrink_to_fit();
    }
    before.saturating_sub(guard.len())
}

pub(crate) fn arc_str_is_empty(s: &Arc<str>) -> bool {
    s.is_empty()
}

pub(crate) fn serialize_arc_str<S>(s: &Arc<str>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(s)
}

pub(crate) fn serialize_opt_arc_str<S>(
    opt: &Option<Arc<str>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match opt {
        Some(s) => serializer.serialize_some(&**s),
        None => serializer.serialize_none(),
    }
}

/// One model call as an agent's own file records it.
#[derive(Debug, Clone, Serialize)]
pub struct LedgerRecord {
    /// The call's last write, in milliseconds since the epoch.
    pub t: i64,
    /// The moment the reply was asked for: the last prompt or tool result
    /// before its first line, or the end of the previous reply. It is what
    /// [`LedgerRecord::floor_ms`] measures a whole turn from, so it never
    /// enters the wire — the page reads the floor the daemon computed.
    #[serde(skip)]
    pub asked: i64,
    /// `claude_code`, `claude_desktop`, `codex`, `grok_build`, `zcode`,
    /// `gemini_cli`, `antigravity`.
    pub agent: &'static str,
    /// Upstream provider or configured gateway route (`Anthropic`, `OpenAI`,
    /// `Google`, `xAI`, `ZCode`, or the custom gateway label).
    #[serde(
        skip_serializing_if = "arc_str_is_empty",
        serialize_with = "serialize_arc_str"
    )]
    pub provider: Arc<str>,
    /// The model as the provider answered, e.g. `claude-sonnet-5`.
    #[serde(serialize_with = "serialize_arc_str")]
    pub model: Arc<str>,
    /// The session the call belongs to.
    #[serde(serialize_with = "serialize_arc_str")]
    pub session: Arc<str>,
    /// The workspace the call ran in, when the transcript names one.
    #[serde(
        skip_serializing_if = "arc_str_is_empty",
        serialize_with = "serialize_arc_str"
    )]
    pub cwd: Arc<str>,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// Measured turn duration in milliseconds when bounded within a single turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// The API error a failed call was written with (`rate_limit`,
    /// `authentication_failed`, …). Such a call has no tokens and no cost; it
    /// exists so the request list can say a request failed.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_opt_arc_str"
    )]
    pub error: Option<Arc<str>>,
    /// Pre-computed by the daemon: the page never prices a call itself.
    pub cost: f64,
    /// A successful call that answered a failed one in the same session
    /// shortly before it: the retry, not a failure, is what the list says.
    #[serde(skip_serializing_if = "is_false")]
    pub retried: bool,
    /// A failed call a later try answered. It keeps its error kind, but the
    /// digest counts it as answered rather than failed.
    #[serde(skip_serializing_if = "is_false")]
    pub recovered: bool,
}

pub(crate) fn is_false(v: &bool) -> bool {
    !*v
}

pub(crate) const MAX_TURN_DURATION_MS: i64 = 10 * 60 * 1000;

pub(crate) fn turn_duration_ms(asked: i64, t: i64) -> Option<u64> {
    if asked <= 0 || t <= asked {
        return None;
    }
    let gap = t.saturating_sub(asked);
    (gap <= MAX_TURN_DURATION_MS).then_some(gap as u64)
}

/// Resolves the provider label for a model call from active gateway routes
/// (when routed through a non-official gateway) or model/agent vendor identity.
pub fn resolve_provider_label(
    agent: &str,
    model: &str,
    routed_providers: &HashMap<String, String>,
) -> Arc<str> {
    if let Some(routed) = routed_providers.get(agent).filter(|s| !s.is_empty()) {
        return intern(routed);
    }
    let lower = model.to_ascii_lowercase();
    let name = if lower.starts_with("claude")
        || lower.contains("anthropic")
        || lower.contains("fable")
        || lower.contains("mythos")
    {
        "Anthropic"
    } else if lower.starts_with("gpt-")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
        || lower.contains("codex")
    {
        "OpenAI"
    } else if lower.starts_with("gemini")
        || lower.contains("gemini")
        || lower.contains("gemma")
        || lower.contains("argon")
    {
        "Google"
    } else if lower.starts_with("grok") || lower.contains("grok") {
        "xAI"
    } else if lower.starts_with("deepseek") || lower.contains("deepseek") {
        "DeepSeek"
    } else if lower.starts_with("qwen") || lower.contains("qwen") || lower.contains("qwq") {
        "Qwen"
    } else if lower.starts_with("kimi") || lower.contains("moonshot") {
        "Moonshot"
    } else if lower.starts_with("minimax") || lower.contains("abab") {
        "MiniMax"
    } else if lower.starts_with("glm") || lower.contains("zcode") || lower.contains("zhipu") {
        "ZCode"
    } else {
        match agent {
            "claude_code" | "claude_desktop" => "Anthropic",
            "codex" => "OpenAI",
            "gemini_cli" | "antigravity" => "Google",
            "grok_build" => "xAI",
            "zcode" | "opencode" => "ZCode",
            _ => "Other",
        }
    };
    intern(name)
}

pub(crate) fn active_routed_providers() -> HashMap<String, String> {
    let mut map = HashMap::new();
    for summary in crate::claude_gateway::discover_all_client_route_summaries() {
        if summary.is_routed && !summary.provider_name.trim().is_empty() {
            map.insert(summary.agent_id, summary.provider_name);
        }
    }
    map
}

impl LedgerRecord {
    pub fn total_tokens(&self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    /// What the model was handed: every input token, cached or not.
    pub fn prompt_tokens(&self) -> u64 {
        self.input
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write)
    }

    /// The whole turn a reply read from a transcript file can be timed over:
    /// from the moment it was asked for to its last line, the wait for the
    /// first token included.
    ///
    /// A transcript file has no first-token time. Its message lines each carry
    /// the finished usage, and a message's first line is its first *content
    /// block*, not its first token — the blocks land together, so the span
    /// from the first line to the last says nothing about how long the model
    /// wrote. The only honest timing is the whole turn: magpie's `Row.Floor`.
    /// It is the least the reply wrote at, not its speed, and is told only for
    /// a reply of 1,000 tokens or more over a second or more (a shorter one is
    /// mostly wait, and would read far under its real speed) and no faster
    /// than 10,000 tok/s.
    pub fn floor_ms(&self) -> Option<u64> {
        if self.asked <= 0 || self.asked >= self.t {
            return None;
        }
        let ms: u64 = self.t.saturating_sub(self.asked).try_into().ok()?;
        if self.output < FLOOR_MIN_OUTPUT || ms < FLOOR_MIN_MS {
            return None;
        }
        // output * 1000 <= 10_000 * ms, without overflowing on a huge reply
        if self.output.saturating_mul(1000) > MAX_DECODE_SPEED.saturating_mul(ms) {
            return None;
        }
        Some(ms)
    }
}

/// The fastest a real stream writes: 10,000 tokens a second (magpie's
/// `usage.MaxDecodeSpeed`). A whole turn that reads faster than this timed
/// something other than the writing.
pub(crate) const MAX_DECODE_SPEED: u64 = 10_000;
/// The least a whole-turn floor is told for: a shorter reply is mostly the
/// wait for its first token (magpie's `Row.Floor`).
pub(crate) const FLOOR_MIN_OUTPUT: u64 = 1_000;
pub(crate) const FLOOR_MIN_MS: u64 = 1_000;

/// The per-model rate card, one place for both the summary and the ledger.
/// Values are US dollars per million tokens.
pub fn rates(model_name: &str) -> (f64, f64, f64) {
    let m = model_name.to_ascii_lowercase();
    if m.contains("opus") {
        (15.0, 75.0, 1.5)
    } else if m.contains("sonnet") || m.contains("fable") || m.contains("mythos") {
        (3.0, 15.0, 0.3)
    } else if m.contains("haiku") || m.contains("flash") || m.contains("mini") || m.contains("lite")
    {
        (0.25, 1.25, 0.025)
    } else if m.contains("gemini-4")
        || m.contains("gemini4")
        || m.contains("argon")
        || m.contains("gemini-3")
        || m.contains("gemini")
    {
        (2.0, 12.0, 0.2)
    } else if m.contains("deepseek") {
        (0.14, 0.28, 0.014)
    } else if m.contains("codex")
        || m.contains("gpt-6")
        || m.contains("gpt-5")
        || m.contains("gpt-4")
        || m.starts_with("o1")
        || m.starts_with("o3")
        || m.starts_with("o4")
        || m.contains("grok")
    {
        (2.5, 10.0, 1.25)
    } else if m.contains("qwen") || m.contains("glm") || m.contains("kimi") || m.contains("minimax")
    {
        (0.5, 2.0, 0.05)
    } else {
        (3.0, 15.0, 0.3)
    }
}

/// What a call cost at the rate card. Cache reads bill at the cache rate;
/// regular input is everything that was not read from the cache.
pub fn calc_cost(model_name: &str, prompt: u64, completion: u64, cache_read: u64) -> f64 {
    let (input_rate, output_rate, cache_read_rate) = rates(model_name);
    let regular_prompt = prompt.saturating_sub(cache_read);
    (regular_prompt as f64 * input_rate
        + completion as f64 * output_rate
        + cache_read as f64 * cache_read_rate)
        / 1_000_000.0
}

/// A rollout or transcript file's live size and modification/change timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileMark {
    pub(crate) len: u64,
    pub(crate) modified_ms: i64,
}

/// Incremental byte-offset reader state for one `.jsonl` transcript file.
///
/// When an active transcript appends new lines, we seek directly to `offset`
/// and parse only the newly completed lines rather than re-reading the whole
/// file from byte 0. If the file was truncated (`meta.len() < offset`), we
/// reset to byte 0 cleanly.
#[derive(Debug, Clone, Default)]
pub(crate) struct TranscriptState {
    pub(crate) offset: u64,
    pub(crate) modified_ms: i64,
    pub(crate) last_user_ms: i64,
    pub(crate) last_assistant_ms: i64,
    pub(crate) requested_model: Option<String>,
    pub(crate) default_agent: &'static str,
    pub(crate) index: HashMap<String, usize>,
    pub(crate) calls: Vec<LedgerRecord>,
    /// True once `index` was cleared on an idle transcript to free key strings.
    pub(crate) index_pruned: bool,
    /// True once this file's immutable calls were spilled to `usage_index.db`.
    pub(crate) cold_archived: bool,
}
