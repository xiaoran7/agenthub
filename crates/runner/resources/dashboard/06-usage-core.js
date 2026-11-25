// ---- the usage page: the ledger's four readings of one period ------------
// The daemon reads Coding Assistant's transcripts once and answers a digest
// per period (usage_ledger.rs): totals, a series, the rankings, a year of
// days, the sessions and the latest requests. This page draws them in
// magpie's idiom — the same stacked columns, ranking bars and heatmap — and
// never derives a figure of its own: cost, speed and cache reads are the
// daemon's.
const USAGE_TABS = [
  ["overview", "概览"],
  ["requests", "请求"],
  ["sessions", "会话"],
  ["context", "上下文"],
];
const LED_TREND = [
  ["tokens", "Token"],
  ["cost", "费用"],
  ["calls", "请求"],
  ["speed", "速度"],
];
const LED_SPLITS = [
  ["model", "模型"],
  ["provider", "供应商"],
  ["model_at", "模型·智能体"],
  ["agent", "智能体"],
];
const LED_HEAT = [
  ["tokens", "Token"],
  ["calls", "请求"],
  ["cost", "费用"],
];
// the ranking's colours: as many as the chart can tell apart; the rest are
// "其他" in grey, as magpie's LED_SHOWN
const LED_SHOWN = 7;
const LED_STATUSES = {
  "": "全部",
  ok: "成功",
  failed: "失败",
  retried: "重试后成功",
};
const LED_TIERS = {
  "": "全部",
  high_tokens: "高消耗 (≥10K Token)",
  high_cost: "高成本 (≥$0.05)",
  slow: "慢请求 (≥5s)",
  cached: "命中缓存",
};

const LED_COLUMNS = [
  { id: "time", label: "时间", defaultOn: true },
  { id: "agent", label: "应用", defaultOn: true },
  { id: "provider", label: "供应商", defaultOn: true },
  { id: "model", label: "模型", defaultOn: true },
  { id: "input", label: "输入 Token", defaultOn: true, numeric: true },
  { id: "output", label: "输出 Token", defaultOn: true, numeric: true },
  { id: "cache_read", label: "缓存读取", defaultOn: true, numeric: true },
  { id: "cache_write", label: "缓存写入", defaultOn: false, numeric: true },
  { id: "total_tokens", label: "总计 Token", defaultOn: false, numeric: true },
  { id: "cost", label: "成本", defaultOn: true, numeric: true },
  { id: "duration", label: "用时", defaultOn: true, numeric: true },
  { id: "status", label: "状态", defaultOn: true },
];
const USAGE_COLS_KEY = "agenthub_usage_cols";
const OPTIMAL_USAGE_POLL_MS = 30000;
const LED_PAGE_STEP = 100;

const LED_FILTER_CATS = [
  ["agent", "应用"],
  ["provider", "供应商"],
  ["model", "模型"],
  ["status", "状态"],
  ["tier", "高级筛选"],
];

let usageTab = "overview";
let usagePeriod = "7d";
let ledMetric = "tokens";
let ledSplit = "model";
let ledHeatMetric = "tokens";
let ledSubView = "logs";
let ledStatus = "";
let ledFilterAgent = "";
let ledFilterProvider = "";
let ledFilterModel = "";
let ledFilterTier = "";
let ledActiveFilterCat = "agent";
let ledQuery = "";
let ledPageLimit = LED_PAGE_STEP;
let ledSessionPageLimit = LED_PAGE_STEP;
let ledVisibleCols = new Set(
  LED_COLUMNS.filter((c) => c.defaultOn).map((c) => c.id),
);
let usageDigest = null;
let usageDirty = false;
let usageLoading = false;
let usageFetchingPeriod = "";
let usageAbortController = null;
const usageMemoryCache = new Map();
const usageEtags = new Map();
let usageSeq = 0;
let usageTimer = 0;
let ledSelectedDay = "";
// the digest is the daemon's; the page keeps the last one while a refresh is
// out, so a poll never blanks the figures it already showed

try {
  const saved = localStorage.getItem(STORE.usageTab);
  if (USAGE_TABS.some(([id]) => id === saved)) usageTab = saved;
  const period = localStorage.getItem(STORE.usagePeriod);
  if (["today", "7d", "30d", "all"].includes(period)) usagePeriod = period;
  const metric = localStorage.getItem(STORE.ledMetric);
  if (LED_TREND.some(([id]) => id === metric)) ledMetric = metric;
  const split = localStorage.getItem(STORE.ledSplit);
  if (LED_SPLITS.some(([id]) => id === split)) ledSplit = split;
  const heat = localStorage.getItem(STORE.ledHeat);
  if (LED_HEAT.some(([id]) => id === heat)) ledHeatMetric = heat;
  const savedCols = JSON.parse(localStorage.getItem(USAGE_COLS_KEY) || "null");
  if (Array.isArray(savedCols)) {
    const valid = savedCols.filter((id) => LED_COLUMNS.some((c) => c.id === id));
    if (valid.length > 0) ledVisibleCols = new Set(valid);
  }
} catch (e) {}

function initUsage() {
  const tabs = document.getElementById("usageTabs");
  if (tabs)
    tabs.addEventListener("click", (e) => {
      const b = e.target.closest("button[data-utab]");
      if (b) setUsageTab(b.dataset.utab);
    });
  const periods = document.getElementById("usagePeriods");
  if (periods)
    periods.addEventListener("click", (e) => {
      const b = e.target.closest("button[data-period]");
      if (b && b.dataset.period !== usagePeriod) setUsagePeriod(b.dataset.period);
    });
  const subTabs = document.getElementById("ledSubTabs");
  if (subTabs)
    subTabs.addEventListener("click", (e) => {
      const b = e.target.closest("button[data-subview]");
      if (b && b.dataset.subview !== ledSubView) {
        setLedgerSubView(b.dataset.subview);
      }
    });
  const q = document.getElementById("ledQ");
  if (q)
    q.addEventListener("input", () => {
      clearTimeout(initUsage._qTimer);
      initUsage._qTimer = setTimeout(() => {
        ledQuery = q.value.trim().toLowerCase();
        ledPageLimit = LED_PAGE_STEP;
        updateLedgerFilterUi();
        renderLedgerRequests();
      }, 90);
    });
  document.addEventListener("click", (e) => {
    if (
      !e.target.closest(".led-filter-picker") &&
      !e.target.closest(".led-col-picker")
    ) {
      closeAllLedgerMenus();
    }
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeAllLedgerMenus();
  });
  const metric = document.getElementById("ledMetric");
  if (metric)
    metric.addEventListener("click", (e) => {
      const b = e.target.closest("button[data-metric]");
      if (!b || b.dataset.metric === ledMetric) return;
      ledMetric = b.dataset.metric;
      remember("ledMetric", ledMetric);
      pickUsageSegs();
      drawLedgerTrend();
    });
  const split = document.getElementById("ledSplit");
  if (split)
    split.addEventListener("click", (e) => {
      const b = e.target.closest("button[data-split]");
      if (!b || b.dataset.split === ledSplit) return;
      ledSplit = b.dataset.split;
      remember("ledSplit", ledSplit);
      pickUsageSegs();
      drawLedgerTrend();
    });
  const heat = document.getElementById("ledHeatMetric");
  if (heat)
    heat.addEventListener("click", (e) => {
      const b = e.target.closest("button[data-heat]");
      if (!b || b.dataset.heat === ledHeatMetric) return;
      ledHeatMetric = b.dataset.heat;
      remember("ledHeat", ledHeatMetric);
      pickUsageSegs();
      drawLedgerHeat();
    });
  window.addEventListener("resize", () => {
    if (!isUsageOpen()) return;
    clearTimeout(initUsage._resize);
    initUsage._resize = setTimeout(() => {
      if (usageTab === "overview") {
        drawLedgerTrend();
        drawLedgerHeat();
      }
    }, 140);
  });
  setUsageTab(usageTab, true);
  pickUsageSegs();
  updateLedgerFilterUi();
  loadUsage();
  startUsageTimer();
}

function remember(key, value) {
  try {
    localStorage.setItem(STORE[key], value);
  } catch (e) {}
}

function isUsageOpen() {
  return !!document.getElementById("view-usage")?.classList.contains("active");
}

function setUsageTab(tab, silent) {
  if (!USAGE_TABS.some(([id]) => id === tab)) tab = "overview";
  usageTab = tab;
  if (!silent) remember("usageTab", tab);
  document.querySelectorAll("#usageTabs button[data-utab]").forEach((b) => {
    const on = b.dataset.utab === tab;
    b.classList.toggle("on", on);
    b.setAttribute("aria-selected", String(on));
  });
  document.getElementById("usagePaneOverview").hidden = tab !== "overview";
  document.getElementById("usagePaneRequests").hidden = tab !== "requests";
  document.getElementById("usagePaneSessions").hidden = tab !== "sessions";
  document.getElementById("usagePaneContext").hidden = tab !== "context";
  closeAllLedgerMenus();
  if (usageDigest) drawUsage();
}

function setUsagePeriod(period) {
  if (!["today", "7d", "30d", "all"].includes(period)) return;
  usagePeriod = period;
  ledPageLimit = LED_PAGE_STEP;
  ledSessionPageLimit = LED_PAGE_STEP;
  remember("usagePeriod", period);
  pickUsageSegs();

  const cached = usageMemoryCache.get(period);
  if (cached && cached.data) {
    // SWR hit: instant switch with zero flicker
    usageDigest = cached.data;
    drawUsage();
    // Background revalidate if older than 6s
    if (Date.now() - cached.ts > 6000) {
      loadUsage(false);
    }
  } else {
    // SWR miss: show rich skeleton animations
    usageDigest = null;
    drawUsageLoading();
    loadUsage(true);
  }
}

// The usage page rides the same change stream as the sessions page: a
// publish re-reads both. This timer is only the fallback for a blocked
// stream, and it revalidates via ETag / 304 without a full re-reading.
function startUsageTimer() {
  clearInterval(usageTimer);
  usageTimer = setInterval(() => {
    if (isUsageOpen() && !document.hidden) loadUsage(false);
  }, OPTIMAL_USAGE_POLL_MS);
}

function pickUsageSegs() {
  document.querySelectorAll("#usagePeriods button[data-period]").forEach((b) => {
    b.classList.toggle("on", b.dataset.period === usagePeriod);
  });
  document.querySelectorAll("#ledSubTabs button[data-subview]").forEach((b) => {
    const on = b.dataset.subview === ledSubView;
    b.classList.toggle("on", on);
    b.setAttribute("aria-selected", String(on));
  });
  fillSegs("ledMetric", LED_TREND, ledMetric, "data-metric");
  fillSegs("ledSplit", LED_SPLITS, ledSplit, "data-split");
  fillSegs("ledHeatMetric", LED_HEAT, ledHeatMetric, "data-heat");
}

function fillSegs(id, options, current, attr) {
  const seg = document.getElementById(id);
  if (!seg) return;
  seg.innerHTML = options
    .map(
      ([value, label]) =>
        `<button type="button" class="opt${value === current ? " on" : ""}" ${attr}="${value}">${label}</button>`,
    )
    .join("");
}

async function loadUsage(manual) {
  const periodToFetch = usagePeriod;
  if (!manual && usageLoading && usageFetchingPeriod === periodToFetch) return;
  if (!manual && !usageDirty && usageMemoryCache.has(periodToFetch)) {
    const cached = usageMemoryCache.get(periodToFetch);
    if (cached && Date.now() - cached.ts < 5000) {
      if (usageDigest !== cached.data) {
        usageDigest = cached.data;
        drawUsage();
      }
      return;
    }
  }

  if (usageAbortController) {
    try {
      usageAbortController.abort();
    } catch (e) {}
  }
  usageAbortController = new AbortController();

  usageLoading = true;
  usageFetchingPeriod = periodToFetch;
  const seq = ++usageSeq;
  const btn = document.getElementById("usageReload");
  if (btn && manual) btn.classList.add("busy");

  if (!usageDigest) {
    drawUsageLoading();
  }

  try {
    const headers = {};
    const prevEtag = usageEtags.get(periodToFetch);
    if (!manual && prevEtag && usageMemoryCache.has(periodToFetch)) {
      headers["If-None-Match"] = prevEtag;
    }
    const forceParam = manual ? "&force=1" : "";
    const resp = await fetch(
      `/api/usage/ledger?period=${encodeURIComponent(periodToFetch)}&limit=50000${forceParam}`,
      { cache: "no-store", headers, signal: usageAbortController.signal },
    );
    if (resp.status === 304 && usageMemoryCache.has(periodToFetch)) {
      const cached = usageMemoryCache.get(periodToFetch);
      cached.ts = Date.now();
      if (periodToFetch === usagePeriod) {
        usageDirty = false;
        if (!usageDigest) {
          usageDigest = cached.data;
          drawUsage();
        }
      }
      if (manual) toast("用量已是最新状态");
      return;
    }
    if (!resp.ok) {
      toast(`读取用量失败（HTTP ${resp.status}）`, "err");
      return;
    }
    const nextEtag = resp.headers.get("ETag");
    const etagChanged = !prevEtag || nextEtag !== prevEtag;
    if (nextEtag) usageEtags.set(periodToFetch, nextEtag);
    const data = await resp.json();
    if (seq !== usageSeq) return;

    const hadDigest = Boolean(usageDigest);
    usageMemoryCache.set(periodToFetch, { data, ts: Date.now() });

    if (periodToFetch === usagePeriod) {
      usageDirty = false;
      usageDigest = data;
      if (!hadDigest || etagChanged || manual) {
        drawUsage();
      }
    }
    if (manual) toast("已刷新最新用量数据");
  } catch (e) {
    if (e.name === "AbortError") return;
    toast("读取用量失败：" + e.message, "err");
  } finally {
    if (seq === usageSeq) {
      usageLoading = false;
      usageFetchingPeriod = "";
      if (btn) btn.classList.remove("busy");
    }
  }
}

function drawUsageLoading() {
  const set = (id, val) => {
    const el = document.getElementById(id);
    if (el) el.innerHTML = val;
  };
  const sk = (w, h = 22) =>
    `<span class="skeleton-bar" style="width: ${w}; height: ${h}px; display: inline-block;"></span>`;
  set("ukCalls", sk("64px"));
  set("ukCallsSub", sk("100px", 12));
  set("ukTokens", sk("72px"));
  set("ukTokensSub", sk("130px", 12));
  set("ukCost", sk("58px"));
  set("ukCostSub", sk("88px", 12));
  set("ukCache", sk("50px"));
  set("ukCacheSub", sk("110px", 12));
  const meter = document.getElementById("ukCacheMeter");
  if (meter) meter.style.width = "0%";
  set("ukSpeed", sk("68px"));
  set("ukSpeedSub", sk("100px", 12));

  const chart = document.getElementById("ledChart");
  if (chart) {
    chart.innerHTML = `
      <div class="skeleton-chart-box" aria-busy="true">
        <div class="skeleton-chart-bars">
          <div class="skeleton-bar" style="height: 38%;"></div>
          <div class="skeleton-bar" style="height: 65%;"></div>
          <div class="skeleton-bar" style="height: 42%;"></div>
          <div class="skeleton-bar" style="height: 85%;"></div>
          <div class="skeleton-bar" style="height: 52%;"></div>
          <div class="skeleton-bar" style="height: 92%;"></div>
          <div class="skeleton-bar" style="height: 70%;"></div>
          <div class="skeleton-bar" style="height: 48%;"></div>
        </div>
      </div>
    `;
  }

  const rank = document.getElementById("ledRank");
  if (rank) {
    rank.innerHTML = `
      <div class="skeleton-card" style="padding: 12px; gap: 10px;" aria-busy="true">
        <div class="skeleton-row"><div class="skeleton-bar" style="width: 88%;"></div></div>
        <div class="skeleton-row"><div class="skeleton-bar" style="width: 72%;"></div></div>
        <div class="skeleton-row"><div class="skeleton-bar" style="width: 80%;"></div></div>
        <div class="skeleton-row"><div class="skeleton-bar" style="width: 58%;"></div></div>
      </div>
    `;
  }

  const heatScroll = document.getElementById("ledHeatScroll");
  if (heatScroll && !heatScroll.children.length) {
    heatScroll.innerHTML = `<div class="skeleton-card" style="height: 80px; width: 100%; margin: 8px 0;" aria-busy="true"></div>`;
  }

  const wrap = document.getElementById("ledWrap");
  if (wrap) wrap.innerHTML = renderSkeletonCard(4);
  const sessions = document.getElementById("usageSessions");
  if (sessions) sessions.innerHTML = renderSkeletonCard(4);
  const ctx = document.getElementById("usageContext");
  if (ctx) ctx.innerHTML = renderSkeletonCard(4);

  const note = document.getElementById("usageNote");
  if (note) note.textContent = "正在读取 Coding Assistant 与 Claude 的会话记录…";
}

function drawUsage() {
  const d = usageDigest;
  // a hidden page has no width to lay a chart out in; the digest stays in
  // hand and is drawn the moment the page is looked at
  if (!d || !isUsageOpen()) return;
  if (usageTab === "overview") {
    drawUsageKpis(d);
    drawLedgerTrend();
    drawLedgerHeat();
    const note = document.getElementById("usageNote");
    if (note) {
      note.textContent = `统计范围：本机原生智能体与已启用 ACP 插件的本地会话及网关记录（${d.from} 至 ${d.to}）。`;
    }
  } else if (usageTab === "requests") {
    renderLedgerRequests();
  } else if (usageTab === "sessions") {
    renderLedgerSessions();
  } else if (usageTab === "context") {
    renderLedgerContext();
  }
}

function fmtMoney(v) {
  const n = Number(v) || 0;
  if (n >= 1000) return "$" + Math.round(n).toLocaleString();
  if (n >= 100) return "$" + n.toFixed(0);
  return "$" + n.toFixed(2);
}

function fmtDuration(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} 秒`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} 分 ${s % 60} 秒`;
  const h = Math.floor(m / 60);
  return `${h} 小时 ${m % 60} 分`;
}

function fmtWhen(ms, withDate) {
  const d = new Date(ms);
  const hm = `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  if (!withDate) return hm;
  return `${d.getMonth() + 1}月${d.getDate()}日 ${hm}`;
}

function drawUsageKpis(d) {
  const set = (id, value) => {
    const el = document.getElementById(id);
    if (el) el.innerHTML = value;
  };
  const calls = d.calls || 0;
  const prompt = (d.input || 0) + (d.cache_read || 0) + (d.cache_write || 0);
  const hit = prompt > 0 ? (100 * (d.cache_read || 0)) / prompt : 0;
  set("ukCalls", calls.toLocaleString());
  set(
    "ukCallsSub",
    d.errors
      ? `${d.errors.toLocaleString()} 次失败${d.retried ? ` · ${d.retried} 次重试后成功` : ""}`
      : `${(d.sessions || 0).toLocaleString()} 个会话 · 无失败`,
  );
  set("ukTokens", fmtN(d.tokens || 0, true));
  set(
    "ukTokensSub",
    `${fmtN(d.input || 0)} in · ${fmtN(d.output || 0)} out · ${fmtN((d.cache_read || 0) + (d.cache_write || 0))} cache`,
  );
  set("ukCost", fmtMoney(d.cost));
  set(
    "ukCostSub",
    calls ? `平均 ${fmtMoney((d.cost || 0) / calls)} / 次请求` : "按模型费率估算",
  );
  set("ukCache", hit.toFixed(1) + "%");
  set("ukCacheSub", `${fmtN(d.cache_read || 0)} 个输入 Token 由缓存返回`);
  const meter = document.getElementById("ukCacheMeter");
  if (meter) meter.style.width = Math.min(100, hit).toFixed(1) + "%";
  const gs = ledGroupSpeed(d);
  set("ukSpeed", gs ? gs.main : "—");
  set("ukSpeedSub", gs ? gs.sub : "本周期没有可整轮计时的回复");
}

// How fast a group of replies wrote: a floor, the one figure a transcript
// file can honestly tell.
//
// A transcript never says when an answer began — a message's first line is
// its first content block, not its first token, and the blocks land together
// — so timing from that line read thousands of tok/s and is not used. What
// the file does say is when the reply was asked for and when it ended; the
// tokens over that whole turn are the least it wrote at. A speed is summed,
// not averaged: the group's output over the group's time, so one short, fast
// reply cannot stand for a long, slow one.
function ledFloor(x) {
  return x && x.floor_ms > 0 ? (1000 * (x.floor_out || 0)) / x.floor_ms : 0;
}
function ledGroupSpeed(x) {
  const f = ledFloor(x);
  const kf = (x && x.floor_n) || 0;
  if (!f) return null;
  return {
    main: `≥${Math.round(f)} tok/s`,
    sub: `${kf.toLocaleString()} 个整轮计时的回复`,
    value: f,
  };
}

// the metric a chart counts for one point or one share
function ledValue(metric, x) {
  if (metric === "speed") return ledFloor(x);
  if (metric === "cost") return Number(x.cost) || 0;
  if (metric === "calls") return Number(x.calls) || 0;
  return Number(x.tokens) || 0;
}

// a ranking row's speed: what its own row says first, the figure of more
// replies (magpie's ledRankValue)
function ledRankValue(metric, x) {
  if (metric !== "speed") return ledValue(metric, x);
  const gs = ledGroupSpeed(x);
  return gs ? gs.value : 0;
}

function ledPartValue(metric, part) {
  if (metric === "speed") return ledFloor(part);
  if (metric === "cost") return Number(part.cost) || 0;
  if (metric === "calls") return Number(part.calls) || 0;
  return Number(part.tokens) || 0;
}

function ledFormat(metric, v) {
  if (metric === "cost") return fmtMoney(v);
  if (metric === "calls") return Math.round(v).toLocaleString();
  if (metric === "speed") return "≥" + Math.round(v) + " tok/s";
  return fmtN(Math.round(v));
}

// an axis writes its numbers alone: five "300 tok/s" labels push the plot
// aside and say the unit five times; the card's title says it once
function ledAxisFormat(metric, v) {
  if (metric === "speed" || metric === "calls") return Math.round(v).toLocaleString();
  return ledFormat(metric, v);
}

// an axis's top with four steps under it: round numbers, at least the largest
function ledAxis(max) {
  if (!(max > 0)) return 4;
  const e = Math.pow(10, Math.floor(Math.log10(max / 4)));
  for (const m of [1, 2, 2.5, 5, 10]) if (m * e * 4 >= max) return m * e * 4;
  return 10 * e * 4;
}

// what a chart tells apart, most first, and the colour of each; the rest are
// "其他". The ranking and the chart share this list, so a colour means one
// model on both.
function ledCategories(split, metric) {
  const rows = ((usageDigest && usageDigest.by && usageDigest.by[split]) || []).slice();
  rows.sort(
    (a, b) =>
      ledRankValue(metric, b) - ledRankValue(metric, a) ||
      (b.tokens || 0) - (a.tokens || 0),
  );
  const top = rows.slice(0, LED_SHOWN).map((x, i) => ({
    ...x,
    color: `var(--c${i + 1})`,
  }));
  return { top, rest: rows.slice(LED_SHOWN), list: rows };
}

function ledName(x, split) {
  const name = x.name || x.id || "—";
  if (split === "model_at" && x.agent) return `${name} · ${providerLabel(x.agent)}`;
  return name;
}

// the bucket a period reads by, as the daemon chose it: hours for a day,
// days for a week or a month, weeks for the whole store (usage_ledger.rs)
function ledBucketName(d) {
  if (d.period === "today") return "hour";
  if (d.period === "7d" || d.period === "30d") return "day";
  return "week";
}

function ledTick(t, bucket) {
  const d = new Date(t);
  if (bucket === "hour") return String(d.getHours()).padStart(2, "0") + ":00";
  return `${d.getMonth() + 1}/${d.getDate()}`;
}

function ledWhen(t, bucket) {
  const d = new Date(t);
  const day = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  if (bucket === "hour")
    return `${day} ${String(d.getHours()).padStart(2, "0")}:00`;
  return bucket === "week" ? `自 ${day} 起的一周` : day;
}

const SVGNS = "http://www.w3.org/2000/svg";
function sv(tag, attrs, style) {
  const e = document.createElementNS(SVGNS, tag);
  for (const k in attrs) e.setAttribute(k, attrs[k]);
  if (style) Object.assign(e.style, style);
  return e;
}

// The trend: one column per bucket of the period, its parts stacked in the
// ranking's colours (tokens, cost and calls add up; speeds do not, so they are
// drawn as grouped bars per active model at their true speed). The ranking
// beside it is the legend and the other reading.
