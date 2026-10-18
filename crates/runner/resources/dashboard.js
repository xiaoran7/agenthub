let dashboardData = null;
let lastRenderState = {
  agentsJson: "",
  workspacesJson: "",
  modelsJson: "",
  gatewayJson: "",
  deviceJson: "",
  sessionsJson: "",
  usageJson: "",
};
let consecutiveFailures = 0;
let consecutiveEmptyResponses = 0;
let allSessionsData = []; // what the renders rank and search
let lastGoodSessions = []; // the last snapshot that had sessions
let refreshing = false;
let renderSeq = 0;
let sessionsRenderedAt = Date.now();

const VIEW_LABELS = {
  sessions: "会话",
  agents: "智能体",
  usage: "用量",
  models: "模型",
  workspaces: "工作区",
  gateway: "网关",
};

const VIEW_EN_LABELS = {
  sessions: "Sessions",
  agents: "Agents",
  usage: "Usage",
  models: "Models",
  workspaces: "Workspaces",
  gateway: "Gateway",
};

function updateCrumb(viewId) {
  const title = document.getElementById("topCrumbTitle");
  const sub = document.getElementById("topCrumbSub");
  if (title) title.textContent = VIEW_LABELS[viewId] || viewId;
  if (sub) sub.textContent = VIEW_EN_LABELS[viewId] || viewId;
}

const PROVIDER_LABELS = {
  claude_code: "Claude Code",
  claude: "Claude",
  claude_desktop: "Claude Desktop",
  codex: "Codex",
  zcode: "ZCode",
  antigravity: "Antigravity",
  grok_build: "Grok Build",
  gemini: "Gemini CLI",
  gemini_cli: "Gemini CLI",
  cursor: "Cursor",
  cline: "Cline",
  opencode: "OpenCode",
};

function providerLabel(id) {
  const key = String(id || "")
    .toLowerCase()
    .trim();
  return PROVIDER_LABELS[key] || String(id || "");
}

const LAYOUT = window.AppLayout || {};
const STORE = Object.assign({}, LAYOUT.storage);
const PORT = String(location.port || LAYOUT.webPort || 4200);
// where the upstream gateway listens, when nothing on this machine says otherwise
const GATEWAY_URL = `http://127.0.0.1:${LAYOUT.gatewayPort}`;
const AFFINITY_HEADER = "x-session-affinity";
// how often the console says it polls; the cadence itself is AppLayout's
const POLL_HINT = `每 ${Math.round((LAYOUT.dashboardPollMs || 4000) / 1000)} 秒自动探测一次`;
const TOAST_MS = { ok: 2400, err: 4200 };

// whether the reader asked the system for less motion; every scripted
// animation checks this once, the way magpie's navCalm() does
const calmMotion = () =>
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// The theme the bootstrap script already applied; the button only redraws
function initTheme() {
  updateThemeButtonUI();
  // the window's caption is painted before the page is up, from the copy
  // the console left last time; a first window in this profile, or one
  // whose copy went stale, brings that copy up to the page's theme
  syncWindowTheme();
}

// the caption's theme, kept where the next window reads it as it opens
function syncWindowTheme() {
  const dark = document.documentElement.classList.contains("dark");
  fetch("/api/window-theme", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ dark }),
  }).catch(() => {});
}

async function init() {
  // the first paint is drawn once, in order; the class comes off on the
  // next frame so a poll or a switch never replays it
  document.documentElement.classList.add("booting");
  layoutNav();
  initSideEdge();
  initTheme();
  const endpoint = document.getElementById("footEndpoint");
  if (endpoint)
    endpoint.textContent = `${location.hostname || "127.0.0.1"}:${PORT}`;
  const footPort = document.getElementById("footPort");
  if (footPort) footPort.textContent = `端口 ${PORT}`;
  const savedView = localStorage.getItem(STORE.view);
  switchView(VIEW_LABELS[savedView] ? savedView : "sessions", true);
  // the first probe can land while the daemon is still scanning its
  // transcripts; ask again before telling anyone there are no sessions
  const ok = await refreshDashboard();
  if (!ok || (dashboardData && (dashboardData.sessions || []).length === 0)) {
    for (
      let i = 0;
      i < (LAYOUT.firstProbeRetries || 0) &&
      !(dashboardData && (dashboardData.sessions || []).length > 0);
      i++
    ) {
      await new Promise((r) => setTimeout(r, LAYOUT.firstProbeRetryMs || 1500));
      await refreshDashboard();
    }
  }
  setInterval(refreshDashboard, LAYOUT.dashboardPollMs || 4000);
  // the page follows the window it is drawn in; a turn tucked away or
  // opened with the button only holds until the next resize
  window.addEventListener("resize", clampSideWidth, { passive: true });
  // the header takes a hairline shadow once the open page has scrolled;
  // one listener on the slot catches every page's own scroll
  const slot = document.querySelector(".view-slot");
  if (slot) {
    slot.addEventListener(
      "scroll",
      () => {
        const scrolled = slot.scrollTop > 4;
        if (
          scrolled !== document.documentElement.classList.contains("scrolled")
        ) {
          document.documentElement.classList.toggle("scrolled", scrolled);
        }
      },
      { passive: true, capture: true },
    );
  }
  // the opening animation has played; from here the console moves only
  // when the reader does something
  setTimeout(() => document.documentElement.classList.remove("booting"), 700);
}

// The sidebar's #nav is the header's switch when the sidebar is away. It is
// the same #nav with its buttons and handlers, moved between the two, as
// magpie does: a Mac sidebar turning into a tab bar (app.js layoutNav).
// beside the sidebar the header says where the reader is; without it, #nav
// takes the centre and the brand goes back into the header
function layoutNav() {
  const nav = document.getElementById("nav");
  const sideSlot = document.getElementById("navSideSlot");
  const topSlot = document.getElementById("navTopSlot");
  const top = document.querySelector(".top");
  const on = !document.documentElement.classList.contains("no-side");
  if (!nav || !sideSlot || !topSlot || !top) return;
  // the brand never moves: the rail's head carries it beside the rail, the
  // header carries its own copy; what moves with the switch is #nav alone
  if (on) sideSlot.appendChild(nav);
  else topSlot.appendChild(nav);
  // beside the rail the names are the rail's own rows, not the header's
  // rounded track; over the page they go back to being that track
  nav.classList.toggle("seg", !on);
  const toggle = document.getElementById("sideToggle");
  if (toggle) {
    const label = on ? "隐藏侧边栏" : "展开侧边栏";
    toggle.title = `${label} (Ctrl+Shift+S)`;
    toggle.setAttribute("aria-label", label);
    toggle.setAttribute("aria-pressed", String(on));
  }
  document.body.classList.toggle("side-nav", on);
  fitNavTrack();
  scaleSegThumb(false);
  scaleRailHl(false);
  return on;
}

// The rail is the reader's: the button in its head, the button in the
// header and Ctrl+Shift+S all turn it, and the choice outlives the window.
// No resize ever takes the button away — the header's toggle is always
// there, whether the rail is open or not.
var sidePinned = null;
clampSideWidth();

function sidebarFits() {
  return (window.innerWidth || 0) >= (LAYOUT.sidebarAt || 768);
}

function setSideNav(on, remember) {
  if (remember) sidePinned = on;
  try {
    localStorage.setItem(STORE.sideNav, on ? "on" : "off");
  } catch (e) {}
  document.body.classList.toggle("side-nav", on);
  document.documentElement.classList.toggle("no-side", !on);
  document.documentElement.dataset.nav = on ? "side" : "top";
  layoutNav();
}

function toggleSideNav() {
  setSideNav(document.documentElement.classList.contains("no-side"), true);
}

function followWindow() {
  sidePinned = null;
  setSideNav(sidebarFits(), false);
}

// the rail's width is shared with the page: a rail turned on in a window
// too narrow for both takes a quarter of it and leaves the rest to the page
function clampSideWidth() {
  const w = Math.min(
    window.innerWidth || 0,
    document.documentElement.clientWidth || 0,
  );
  if (w) document.documentElement.style.setProperty("--viewport-w", w + "px");
}

// the sidebar's edge drags it wider or narrower; double-click gives it back
// to the window's own width, and a narrower window takes it no wider than
// a quarter of itself so the page keeps the room
function initSideEdge() {
  const edge = document.getElementById("sideEdge");
  if (!edge) return;
  const win = LAYOUT.window || {};
  const lo = win.sideMinWidth || 168;
  const hi = win.sideMaxWidth || 340;
  const fit = (w) =>
    Math.round(Math.max(lo, Math.min(w, hi, window.innerWidth * 0.25)));
  let dragging = false;

  edge.addEventListener("pointerdown", (e) => {
    dragging = true;
    try {
      edge.setPointerCapture(e.pointerId);
    } catch (err) {}
    document.body.classList.add("side-drag");
    e.preventDefault();
  });
  edge.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    document.documentElement.style.setProperty(
      "--side-w-override",
      fit(e.clientX) + "px",
    );
  });
  const end = () => {
    if (!dragging) return;
    dragging = false;
    document.body.classList.remove("side-drag");
    const w = Math.round(edge.getBoundingClientRect().left);
    try {
      localStorage.setItem(STORE.sideWidth, String(w));
    } catch (e) {}
    // reading the edge's place made the browser flush the override into
    // the rail's width; give it back to the tokens, which carry the same
    // width, so the next token pass cannot drop it
    document.documentElement.style.removeProperty("--side-w-override");
  };
  edge.addEventListener("pointerup", end);
  edge.addEventListener("pointercancel", end);
  edge.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const cur = Math.round(edge.getBoundingClientRect().left);
    document.documentElement.style.setProperty(
      "--side-w-override",
      fit(cur + (e.key === "ArrowRight" ? 12 : -12)) + "px",
    );
    try {
      localStorage.setItem(
        STORE.sideWidth,
        String(Math.round(edge.getBoundingClientRect().left)),
      );
    } catch (err) {}
    document.documentElement.style.removeProperty("--side-w-override");
    e.preventDefault();
  });
  edge.addEventListener("dblclick", () => {
    document.documentElement.style.removeProperty("--side-w-override");
    try {
      localStorage.removeItem(STORE.sideWidth);
    } catch (e) {}
  });
}

// Ctrl+Shift+S toggles the sidebar, as magpie's title-bar button has it
window.addEventListener("keydown", (e) => {
  if (
    e.ctrlKey &&
    e.shiftKey &&
    !e.altKey &&
    (e.key === "S" || e.key === "s")
  ) {
    e.preventDefault();
    toggleSideNav();
  }
});

// The thumb under the open tab, in the header's switch only. It is written
// straight into the track and moved by transform alone, so a click never
// makes the header lay itself out a second time.
function scaleSegThumb(animate) {
  const nav = document.getElementById("nav");
  const thumb = document.getElementById("segThumb");
  // the thumb belongs to the header's page switch; the rail shows the
  // open page as a tinted row instead
  if (!nav || !thumb || nav.closest("#navSideSlot")) return;
  const btn = nav.querySelector("button.on");
  if (!btn) return;
  // a transform moves the box without changing what it measures, so where
  // the track sits is read first, before this thumb moves at all
  const track = nav.getBoundingClientRect();
  const at = btn.getBoundingClientRect();
  const x = Math.round(
    at.left -
      track.left -
      nav.clientLeft -
      (parseFloat(getComputedStyle(thumb).translate) || 0),
  );
  thumb.style.transition = animate === false ? "none" : "";
  thumb.style.width = Math.round(at.width) + "px";
  thumb.style.translate = Math.max(0, x) + "px 0";
  if (animate === false) {
    void thumb.offsetWidth;
    thumb.style.transition = "";
  }
}

// In the rail the open page is one shape that slides from row to row, as
// magpie's .nav-hl does. It is placed on the button the page has just
// opened; a click glides, a fresh layout lands still.
function scaleRailHl(animate) {
  const nav = document.getElementById("nav");
  const hl = document.getElementById("railHl");
  // the shape belongs to the rail; the header's track has its own thumb
  if (!nav || !hl || !nav.closest("#navSideSlot")) return;
  const btn = nav.querySelector("button.on");
  hl.classList.toggle("still", animate === false);
  if (!btn) {
    hl.style.height = "0px";
    return;
  }
  hl.style.transform = `translateY(${btn.offsetTop}px)`;
  hl.style.height = btn.offsetHeight + "px";
  if (animate === false)
    requestAnimationFrame(() => hl.classList.remove("still"));
}

// In the header the track sits centred and shows the open page and what is
// either side of it — the switch is for turning the page, not for listing
// every page twice. What fits is worked out from the header's free space,
// measured with the track out of the flow, so the track never squeezes the
// icons beside it.
function fitNavTrack() {
  const slot = document.getElementById("navTopSlot");
  const nav = document.getElementById("nav");
  const top = document.querySelector(".top");
  if (!slot || !nav || !top) return;
  const buttons = [...nav.querySelectorAll("button[data-view]")];
  if (!buttons.length) return;

  // When the rail holds the nav, restore buttons to full names and skip track slicing
  if (nav.parentElement !== slot) {
    buttons.forEach((b) => {
      b.style.display = "";
      const span = b.querySelector("span");
      if (span && span.dataset.full) span.textContent = span.dataset.full;
    });
    nav.classList.remove("tight");
    return;
  }

  // Ensure every button's dataset.full is memorized
  buttons.forEach((b) => {
    const span = b.querySelector("span");
    if (span && !span.dataset.full) span.dataset.full = span.textContent.trim();
  });

  // On standard desktop and tablet displays (>= 640px or unmeasured), keep all navigation buttons
  // cleanly visible with their icons and full labels
  const viewWidth = window.innerWidth || document.documentElement.clientWidth || 0;
  if (viewWidth >= 640 || viewWidth === 0) {
    buttons.forEach((b) => {
      b.style.display = "";
      const span = b.querySelector("span");
      if (span && span.dataset.full) span.textContent = span.dataset.full;
    });
    nav.classList.remove("tight");
    return;
  }

  const gap = parseFloat(getComputedStyle(nav).columnGap) || 4;
  const pad = parseFloat(getComputedStyle(nav).paddingLeft) * 2 || 4;
  const widths = buttons.map((b) => b.getBoundingClientRect().width || 60);

  const topStyle = getComputedStyle(top);
  const inner =
    top.clientWidth -
    parseFloat(topStyle.paddingLeft) -
    parseFloat(topStyle.paddingRight);
  const gaps =
    (top.children.length - 1) * (parseFloat(topStyle.columnGap) || 0);

  // Measure content width of non-slot elements rather than their expanded 1fr grid columns
  const others = [...top.children]
    .filter((el) => el !== slot && el !== nav && el.offsetParent !== null)
    .reduce((sum, el) => {
      let contentW = 0;
      for (const child of el.children) contentW += child.getBoundingClientRect().width;
      return sum + Math.max(contentW, 120);
    }, 0);
  const free = Math.max(0, inner - others - gaps);

  let open = buttons.findIndex((b) => b.classList.contains("on"));
  if (open < 0) open = 0;
  const pick = (r) => {
    const from = Math.max(0, open - r);
    const to = Math.min(buttons.length - 1, open + r);
    let wid = pad + gap * (to - from);
    for (let j = from; j <= to; j++) wid += widths[j] || 0;
    return { from, to, wid };
  };
  let best = pick(0);
  for (let r = 1; r < buttons.length; r++) {
    const next = pick(r);
    if (next.wid > free) break;
    best = next;
  }
  // the open page alone never hides; what is left of the header sets how
  // the track takes it: with room the names stand with their icons, then
  // the icons give way, then the names fold to their first two characters
  buttons.forEach((b, j) => {
    b.style.display = j >= best.from && j <= best.to ? "" : "none";
  });
  nav.classList.remove("tight");
  nav.classList.toggle("tight", pick(best.to - best.from).wid > free);
  buttons.forEach((b) => {
    const span = b.querySelector("span");
    if (span && span.dataset.full)
      span.textContent = nav.classList.contains("tight")
        ? span.dataset.full.slice(0, 2)
        : span.dataset.full;
  });
}

// Switch between the views
function switchView(viewId, silent) {
  if (!VIEW_LABELS[viewId]) return;
  // a click turns the page as one motion (view-transition in dashboard.css,
  // the way magpie's pages turn); the first paint, a restored page and a
  // reader who asked for less motion get the page at once
  const turn = () => {
    document
      .querySelectorAll("#nav button[data-view]")
      .forEach((b) => b.classList.remove("on"));
    document
      .querySelectorAll(".view")
      .forEach((v) => v.classList.remove("active"));

    const btn =
      document.getElementById("nav-" + viewId) ||
      document.querySelector(`#nav button[data-view="${viewId}"]`);
    if (btn) btn.classList.add("on");

    const view = document.getElementById("view-" + viewId);
    if (view) {
      view.classList.add("active");
      if (!silent) view.scrollTop = 0;
    }

    if (!silent) {
      try {
        localStorage.setItem(STORE.view, viewId);
      } catch (e) {}
    }
    updateCrumb(viewId);
    // the clone the track measures itself with follows the page
    const seen = document.getElementById("navMeasure");
    if (seen)
      seen
        .querySelectorAll("button[data-view]")
        .forEach((b) => b.classList.toggle("on", b.dataset.view === viewId));
    fitNavTrack();
    scaleSegThumb(!silent);
    scaleRailHl(!silent);
  };
  if (silent || calmMotion() || !document.startViewTransition) {
    turn();
    return;
  }
  document.documentElement.classList.add("page-turn");
  const vt = document.startViewTransition(turn);
  vt.finished.finally(() =>
    document.documentElement.classList.remove("page-turn"),
  );
}

// manual: the reader asked for this probe, so the button that asked turns
// while it is out. The four-second poll stays quiet — a button that is
// always spinning says nothing about whether anything is happening.
async function refreshDashboard(manual) {
  // a probe can take over a second; never let two snapshots render out of order
  if (refreshing) return false;
  refreshing = true;
  const refreshBtn = document.getElementById("refreshBtn");
  if (manual && refreshBtn) refreshBtn.classList.add("busy");
  if (manual && lastGoodSessions.length === 0) {
    const container = document.getElementById("sessionsContainer");
    if (container && !container.querySelector(".skeleton-card")) {
      container.innerHTML = renderSkeletonCard(5);
    }
  }
  const seq = ++renderSeq;
  try {
    const resp = await fetch("/api/dashboard", { cache: "no-store" });
    if (!resp.ok) {
      handleFetchFailure();
      return false;
    }
    const data = await resp.json();
    if (seq !== renderSeq) return false;
    consecutiveFailures = 0;
    updateConnectionStatus(true);

    dashboardData = data;
    renderStats(dashboardData.stats, dashboardData.gateway);

    // the sessions feed the workspace and model rankings, so publish them first.
    // a zero-length snapshot is kept off the rankings: the lists keep their last
    // good figures until sessions come back, instead of flapping to empty
    const sessions = dashboardData.sessions || [];
    if (sessions.length > 0) {
      const sessionsStr = JSON.stringify(sessions);
      const sessionsChanged = sessionsStr !== lastRenderState.sessionsJson;
      lastGoodSessions = sessions;
      allSessionsData = sessions;
      lastRenderState.sessionsJson = sessionsStr;

      // Dirty-check DOM updates
      const agentsStr = JSON.stringify(dashboardData.agents || []);
      if (agentsStr !== lastRenderState.agentsJson) {
        renderAgents(dashboardData.agents);
        lastRenderState.agentsJson = agentsStr;
      }

      const wsStr = JSON.stringify(dashboardData.workspaces || []);
      if (wsStr !== lastRenderState.workspacesJson || sessionsChanged) {
        renderWorkspaces(dashboardData.workspaces);
        lastRenderState.workspacesJson = wsStr;
      }

      const modelsStr = JSON.stringify(dashboardData.models || []);
      if (modelsStr !== lastRenderState.modelsJson || sessionsChanged) {
        renderModels(dashboardData.models);
        lastRenderState.modelsJson = modelsStr;
      }

      const usageStr = JSON.stringify(dashboardData.usage || {});
      if (usageStr !== lastRenderState.usageJson || sessionsChanged) {
        renderUsageAnalytics(
          dashboardData.usage,
          sessions,
          dashboardData.stats,
        );
        lastRenderState.usageJson = usageStr;
      }

      if (sessionsChanged) {
        renderSessions(sessions);
      }
    } else {
      // an empty answer only earns the empty state once it has held for a
      // while; a single zero snapshot is a probe, not a verdict
      consecutiveEmptyResponses++;
      const settled =
        consecutiveEmptyResponses >= (LAYOUT.emptyStrikes || 3) &&
        Date.now() - sessionsRenderedAt > (LAYOUT.emptyGraceMs || 15000);
      if (lastGoodSessions.length === 0) {
        renderSessions([], settled ? "empty" : "pending");
      }
    }

    const gatewayStr = JSON.stringify(dashboardData.gateway || {});
    if (gatewayStr !== lastRenderState.gatewayJson) {
      renderGateway(dashboardData.gateway);
      lastRenderState.gatewayJson = gatewayStr;
    }

    const deviceStr = JSON.stringify(dashboardData.device || {});
    if (deviceStr !== lastRenderState.deviceJson) {
      renderDevice(dashboardData.device);
      lastRenderState.deviceJson = deviceStr;
    }
    return true;
  } catch (e) {
    handleFetchFailure();
    return false;
  } finally {
    refreshing = false;
    if (refreshBtn) refreshBtn.classList.remove("busy");
  }
}

function handleFetchFailure() {
  consecutiveFailures++;
  updateConnectionStatus(false);
}

// whether the daemon is still answering; the ledger's tiles say so, the
// header only settles the pairing dot down while the link is broken
function updateConnectionStatus(online) {
  document.documentElement.classList.toggle("link-off", !online);
  if (!online) {
    consecutiveEmptyResponses = 0;
  }
}

// A figure that changes climbs to its new value instead of being stamped
// there; one that has not changed is left alone, so a poll that finds the
// same number never makes the ledger twitch. Only the first value on a
// fresh page animates from zero — after that every change counts from
// where the reader last saw it.
const shownFigures = new Map();
function countTo(el, value) {
  if (!el) return;
  const to = Number(value) || 0;
  const from = shownFigures.has(el) ? shownFigures.get(el) : 0;
  shownFigures.set(el, to);
  if (from === to) {
    el.textContent = String(to);
    return;
  }
  if (calmMotion()) {
    el.textContent = String(to);
    return;
  }
  cancelAnimationFrame(el._countRaf);
  const start = performance.now();
  const step = (now) => {
    const t = Math.min(1, (now - start) / 420);
    // the same ease-out the page moves on, so the figures settle with it
    const eased = 1 - Math.pow(1 - t, 3);
    el.textContent = String(Math.round(from + (to - from) * eased));
    if (t < 1) el._countRaf = requestAnimationFrame(step);
  };
  el._countRaf = requestAnimationFrame(step);
}

function renderStats(stats, gateway) {
  if (!stats) return;
  const totalSessions = stats.total_sessions || 0;
  countTo(document.getElementById("statTotalSessions"), totalSessions);
  const breakdown = document.getElementById("statSessionsBreakdown");
  breakdown.textContent = `Coding Assistant ${stats.claude_sessions || 0} · Codex ${stats.codex_sessions || 0} · 其他 ${(stats.antigravity_sessions || 0) + (stats.grok_sessions || 0) + (stats.zcode_sessions || 0)}`;
  breakdown.title = breakdown.textContent;

  countTo(document.getElementById("statTotalModels"), stats.total_models || 0);
  if (gateway && gateway.detected) {
    let src = String(gateway.source || "").trim();
    if (src.includes("host-creds") || src.includes(".json")) {
      src = "动态检测网关";
    }
    document.getElementById("statGatewaySource").textContent =
      src || "多模型自动路由";
  } else {
    document.getElementById("statGatewaySource").textContent = "多模型自动路由";
  }

  countTo(
    document.getElementById("statTotalWorkspaces"),
    stats.total_workspaces || 0,
  );
  const wsSub = document.getElementById("statWorkspacesSub");
  if (wsSub) {
    const agents = stats.active_agents || 0;
    wsSub.textContent =
      agents > 0 ? `${agents} 个智能体在线` : "本地项目开发根目录";
  }
}

function getAgentIcon(id, size = 18) {
  const k = String(id || "")
    .toLowerCase()
    .trim();
  // Claude Code / Claude Desktop: the Claude mark, which stays legible at 18px
  if (
    k === "claude_code" ||
    k === "claude" ||
    k.includes("claude-code") ||
    k.includes("claudecode") ||
    k.includes("claude_desktop") ||
    k.includes("desktop")
  ) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><title>Claude</title><path d="M9.6 3.2 11 8.6l-4.4-3.3-1.2 1.7 4.7 3.1-5.5 1.3.5 2 5.4-1.9-3.4 4.5 1.7 1.2 3.2-4.7 1.4 5.4 2-.5-1.9-5.4 4.6 3.4 1.2-1.7-4.8-3.2 5.5-1.3-.5-2-5.5 1.9 3.5-4.6-1.7-1.2L12.9 10 11.6 4.6l-2 .5z" fill="#D97757"/></svg>`;
  }
  // Codex
  if (k.includes("codex")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><title>Codex</title><path d="M19.503 0H4.496A4.496 4.496 0 000 4.496v15.007A4.496 4.496 0 004.496 24h15.007A4.496 4.496 0 0024 19.503V4.496A4.496 4.496 0 0019.503 0z" fill="#3941FF"/><path d="M9.064 3.344a4.578 4.578 0 012.285-.312c1 .115 1.891.54 2.673 1.275.01.01.024.017.037.021a.09.09 0 00.043 0 4.55 4.55 0 013.046.275l.047.022.116.057a4.581 4.581 0 012.188 2.399c.209.51.313 1.041.315 1.595a4.24 4.24 0 01-.134 1.223.123.123 0 00.03.115c.594.607.988 1.33 1.183 2.17.289 1.425-.007 2.71-.887 3.854l-.136.166a4.548 4.548 0 01-2.201 1.388.123.123 0 00-.081.076c-.191.551-.383 1.023-.74 1.494-.9 1.187-2.222 1.846-3.711 1.838-1.187-.006-2.239-.44-3.157-1.302a.107.107 0 00-.105-.024c-.388.125-.78.143-1.204.138a4.441 4.441 0 01-1.945-.466 4.544 4.544 0 01-1.61-1.335c-.152-.202-.303-.392-.414-.617a5.81 5.81 0 01-.37-.961 4.582 4.582 0 01-.014-2.298.124.124 0 00.006-.056.085.085 0 00-.027-.048 4.467 4.467 0 01-1.034-1.651 3.896 3.896 0 01-.251-1.192 5.189 5.189 0 01.141-1.6c.337-1.112.982-1.985 1.933-2.618.212-.141.413-.251.601-.33.215-.089.43-.164.646-.227a.098.098 0 00.065-.066 4.51 4.51 0 01.829-1.615 4.535 4.535 0 011.837-1.388zm3.482 10.565a.637.637 0 000 1.272h3.636a.637.637 0 100-1.272h-3.636zM8.462 9.23a.637.637 0 00-1.106.631l1.272 2.224-1.266 2.136a.636.636 0 101.095.649l1.454-2.455a.636.636 0 00.005-.64L8.462 9.23z" fill="#FFFFFF"/></svg>`;
  }
  // ZCode (dark badge with a white Z)
  if (k.includes("zcode") || k.startsWith("sess_")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><title>ZCode</title><rect width="24" height="24" rx="4.5" fill="#111113"/><path d="M6.2 6.5h11.6v2.6l-6.8 6.5h6.8v2.6H6.2v-2.6l6.8-6.5H6.2V6.5z" fill="#ffffff"/></svg>`;
  }
  // Gemini CLI
  if (k.includes("gemini")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 28 28" fill="none"><title>Gemini CLI</title><defs><radialGradient id="gemini-sparkle-agent-${size}" cx="0" cy="0" r="1" gradientUnits="userSpaceOnUse" gradientTransform="translate(2.77876 11.3795) rotate(18.6832) scale(29.8025 238.737)"><stop offset="0.0671246" stop-color="#9168C0"/><stop offset="0.342551" stop-color="#5684D1"/><stop offset="0.672076" stop-color="#1BA1E3"/></radialGradient></defs><path d="M14 28C14 26.0633 13.6267 24.2433 12.88 22.54C12.1567 20.8367 11.165 19.355 9.905 18.095C8.645 16.835 7.16333 15.8433 5.46 15.12C3.75667 14.3733 1.93667 14 0 14C1.93667 14 3.75667 13.6383 5.46 12.915C7.16333 12.1683 8.645 11.165 9.905 9.905C11.165 8.645 12.1567 7.16333 12.88 5.46C13.6267 3.75667 14 1.93667 14 0C14 1.93667 14.3617 3.75667 15.085 5.46C15.8317 7.16333 16.835 8.645 18.095 9.905C19.355 11.165 20.8367 12.1683 22.54 12.915C24.2433 13.6383 26.0633 14 28 14C26.0633 14 24.2433 14.3733 22.54 15.12C20.8367 15.8433 19.355 16.835 18.095 18.095C16.835 19.355 15.8317 20.8367 15.085 22.54C14.3617 24.2433 14 26.0633 14 28Z" fill="url(#paint0_radial_gemini_agent_${size})"/><defs><radialGradient id="paint0_radial_gemini_agent_${size}" cx="0" cy="0" r="1" gradientUnits="userSpaceOnUse" gradientTransform="translate(2.77876 11.3795) rotate(18.6832) scale(29.8025 238.737)"><stop offset="0.0671246" stop-color="#9168C0"/><stop offset="0.342551" stop-color="#5684D1"/><stop offset="0.672076" stop-color="#1BA1E3"/></radialGradient></defs></svg>`;
  }
  // Grok
  if (k.includes("grok")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><title>Grok</title><path d="M9.27 15.29l7.978-5.897c.391-.29.95-.177 1.137.272.98 2.369.542 5.215-1.41 7.169-1.951 1.954-4.667 2.382-7.149 1.406l-2.711 1.257c3.889 2.661 8.611 2.003 11.562-.953 2.341-2.344 3.066-5.539 2.388-8.42l.006.007c-.983-4.232.242-5.924 2.75-9.383.06-.082.12-.164.179-.248l-3.301 3.305v-.01L9.267 15.292M7.623 16.723c-2.792-2.67-2.31-6.801.071-9.184 1.761-1.763 4.647-2.483 7.166-1.425l2.705-1.25a7.808 7.808 0 00-1.829-1A8.975 8.975 0 005.984 5.83c-2.533 2.536-3.33 6.436-1.962 9.764 1.022 2.487-.653 4.246-2.34 6.022-.599.63-1.199 1.259-1.682 1.925l7.62-6.815" fill="#111113"/></svg>`;
  }
  // Antigravity
  if (k.includes("antigravity")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><title>Antigravity</title><path d="M21.751 22.607c1.34 1.005 3.35.335 1.508-1.508C17.73 15.74 18.904 1 12.037 1 5.17 1 6.342 15.74.815 21.1c-2.01 2.009.167 2.511 1.507 1.506 5.192-3.517 4.857-9.714 9.715-9.714 4.857 0 4.522 6.197 9.714 9.715z" fill="#00B95C"/><path d="M15.269 7.747c1.058 4.557 5.691 7.374 10.348 6.293 4.657-1.082 7.575-5.653 6.516-10.21-1.058-4.556-5.691-7.374-10.348-6.292-4.657 1.082-7.575 5.653-6.516 10.21z" fill="#FC413D"/><path d="M9.932 27.617c1.04 4.482 5.384 7.303 9.7 6.3 4.316-1.002 6.971-5.448 5.93-9.93-1.04-4.483-5.384-7.304-9.7-6.301-4.316 1.002-6.971 5.448-5.93 9.93z" fill="#3186FF"/><path d="M2.572-8.185C.392-3.329 2.778 2.472 7.9 4.771c5.122 2.3 11.042.227 13.222-4.63 2.18-4.855-.205-10.656-5.327-12.955-5.122-2.3-11.042-.227-13.222 4.63z" fill="#FBBC04"/></svg>`;
  }
  // Cursor
  if (k.includes("cursor")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><title>Cursor</title><path d="M22.106 5.68L12.5.135a.998.998 0 00-.998 0L1.893 5.68a.84.84 0 00-.419.726v11.186c0 .3.16.577.42.727l9.607 5.547a.999.999 0 00.998 0l9.608-5.547a.84.84 0 00.42-.727V6.407a.84.84 0 00-.42-.726zm-.603 1.176L12.228 22.92c-.063.108-.228.064-.228-.061V12.34a.59.59 0 00-.295-.51l-9.11-5.26c-.107-.062-.063-.228.062-.228h18.55c.264 0 .428.286.296.514z" fill="#000000"/></svg>`;
  }
  // Cline
  if (k.includes("cline")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><title>Cline</title><path d="M17.035 3.991c2.75 0 4.98 2.24 4.98 5.003v1.667l1.45 2.896a1.01 1.01 0 01-.002.909l-1.448 2.864v1.668c0 2.762-2.23 5.002-4.98 5.002H7.074c-2.751 0-4.98-2.24-4.98-5.002V17.33l-1.48-2.855a1.01 1.01 0 01-.003-.927l1.482-2.887V8.994c0-2.763 2.23-5.003 4.98-5.003h9.962zM8.265 9.6a2.274 2.274 0 00-2.274 2.274v4.042a2.274 2.274 0 004.547 0v-4.042A2.274 2.274 0 008.265 9.6zm7.326 0a2.274 2.274 0 00-2.274 2.274v4.042a2.274 2.274 0 104.548 0v-4.042A2.274 2.274 0 0015.59 9.6z" fill="#0EA5E9"/><path d="M12.054 5.558a2.779 2.779 0 100-5.558 2.779 2.779 0 000 5.558z" fill="#0EA5E9"/></svg>`;
  }
  // OpenCode
  if (k.includes("opencode")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><title>OpenCode</title><path d="M16 6H8v12h8V6zm4 16H4V2h16v20z" fill="#10A37F"/></svg>`;
  }
  // Default neutral terminal badge
  return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="11" width="18" height="10" rx="2"/><circle cx="12" cy="5" r="2"/><path d="M12 7v4M8 16h0M16 16h0"/></svg>`;
}

function renderAgents(agents) {
  const container = document.getElementById("agentsGrid");
  const countBadge = document.getElementById("agentsCountBadge");
  if (countBadge) {
    const count = agents ? agents.length : 0;
    countBadge.textContent = `${count} 个智能体`;
  }
  if (!container) return;

  if (!agents || agents.length === 0) {
    container.innerHTML = `<div class="empty-state"><b>没有发现可用的智能体</b><span class="why">安装 Coding Assistant 或 Codex 并确保它在 PATH 中，刷新后即可在这里看到。</span></div>`;
    return;
  }

  container.innerHTML = agents
    .map((agent) => {
      const isReady = agent.installed || agent.sessions_count > 0;
      const tagHtml = isReady
        ? `<span class="tag ok">已就绪</span>`
        : `<span class="tag">未检测到</span>`;
      const pathText = agent.path
        ? escapeHtml(agent.path)
        : "未找到独立程序，智能体仍可通过命令别名直连";

      return `
      <div class="row">
        <span class="ic">${getAgentIcon(agent.id, 22)}</span>
        <div class="who">
          <div class="name-line">
            <span class="name">${escapeHtml(agent.name)}</span>
            <span class="tag mono">${escapeHtml(agent.command || agent.id)}</span>
            ${tagHtml}
          </div>
          <div class="sub flow">
            <span>${escapeHtml(agent.description || "")}</span>
            <span class="mono faint">${pathText}</span>
          </div>
        </div>
        <span class="tag">${agent.sessions_count || 0} 个会话</span>
      </div>
    `;
    })
    .join("");
}

// Shimmer skeleton loading placeholder matching user specifications
function renderSkeletonCard(bars = 5) {
  const widths = ["100%", "85%", "92%", "76%", "60%", "80%"];
  const rows = [];
  for (let i = 0; i < bars; i++) {
    const w = widths[i % widths.length];
    rows.push(`<div class="skeleton-row"><div class="skeleton-bar" style="width: ${w};"></div></div>`);
  }
  return `
    <div class="skeleton-card" aria-busy="true" aria-label="正在加载内容…">
      ${rows.join("")}
    </div>
  `;
}

// The sessions tree, grouped by workspace.
//   mode 'data'    a snapshot with sessions;
//   mode 'pending' a zero snapshot that has not held long enough to trust;
//   mode 'empty'   no sessions, and the daemon keeps saying so.
// The badge and the filter live here too, so every render resets them
// instead of leaving a filtered count behind.
function renderSessions(sessions, mode = "data") {
  const container = document.getElementById("sessionsContainer");
  const badge = document.getElementById("sessionCountBadge");
  if (!container) return;

  const dedupedSessions = dedupeSessions(sessions || []);

  if (dedupedSessions.length === 0) {
    if (mode === "pending" || mode === "loading") {
      container.innerHTML = renderSkeletonCard(5);
      if (badge) badge.textContent = "正在加载…";
      return;
    }
    container.innerHTML = `
      <div class="empty-state">
        <b>还没有可读取的会话</b>
        <span class="why">在任意目录下运行一次 Coding Assistant、Codex 或 ZCode，对话结束后会话就会出现在这里。如果刚结束的会话还没出现，稍等几秒再看。</span>
        <button class="text action retry" onclick="refreshDashboard(true)">
          <svg class="ic" viewBox="0 0 20 20" width="14" height="14" aria-hidden="true"><g fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M17.5 3v4h-4M2.5 17v-4h4"/><path d="M3 9.5a7 7 0 0 1 13-2.5M17 10.5a7 7 0 0 1-13 2.5"/></g></svg>
          <span>重新读取</span>
        </button>
      </div>
    `;
    if (badge) badge.textContent = "0 个历史会话";
    return;
  }

  if (badge)
    badge.textContent = `${dedupedSessions.length.toLocaleString()} 个历史会话`;

  // Sort sessions chronologically descending (newest activity first)
  const sortedSessions = [...dedupedSessions].sort(sessionRecency);
  const total = sortedSessions.length;

  // Group sessions by workspace while preserving recency order
  const groups = {};
  sortedSessions.forEach((s) => {
    const key = s.workspace_path || s.cwd || "默认工作区";
    if (!groups[key]) {
      groups[key] = {
        path: key,
        name:
          s.workspace_name || key.split(/[/\\]/).filter(Boolean).pop() || key,
        items: [],
      };
    }
    groups[key].items.push(s);
  });

  // Sort workspace groups by their latest session's activity timestamp descending
  const groupKeys = Object.keys(groups).sort((k1, k2) => {
    const max1 = Math.max(
      0,
      ...groups[k1].items.slice(0, 1).map(sessionRecencyValue),
    );
    const max2 = Math.max(
      0,
      ...groups[k2].items.slice(0, 1).map(sessionRecencyValue),
    );
    return max2 - max1;
  });

  container.innerHTML = groupKeys
    .map((k) => {
      const grp = groups[k];
      const safePath = escapeHtml(grp.path);
      const safeName = escapeHtml(grp.name);
      const count = grp.items.length;
      const closed = collapsedGroups.has(k);

      const itemsHtml = grp.items
        .map((item, index) => {
          const rawId = item.session_id || item.id || "";
          const hasId = !!rawId && rawId !== "unknown";
          const prompt = escapeHtml(
            item.prompt ||
              item.title ||
              item.first_prompt ||
              item.name ||
              "无标题会话",
          );
          const rawProvider =
            item.provider ||
            item.agent_name ||
            (rawId.startsWith("sess_")
              ? "zcode"
              : rawId.startsWith("antigravity_")
                ? "antigravity"
                : rawId.startsWith("claude_")
                  ? "claude_code"
                  : rawId.startsWith("grok_")
                    ? "grok_build"
                    : "codex");

          let timeStr = item.relative_time;
          if (!timeStr) {
            const ts = sessionRecencyValue(item);
            if (ts) {
              const ms = ts < 10000000000 ? ts * 1000 : ts;
              const date = new Date(ms);
              const now = Date.now();
              const diffMin = Math.round((now - ms) / 60000);
              if (diffMin < 1) timeStr = "刚刚";
              else if (diffMin < 60) timeStr = `${diffMin} 分钟前`;
              else if (diffMin < 1440)
                timeStr = `${Math.floor(diffMin / 60)} 小时前`;
              else {
                timeStr = `${date.getMonth() + 1}月${date.getDate()}日 ${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
              }
            } else {
              timeStr = item.created_at || "最近";
            }
          }

          const rawTokens =
            Number(item.total_tokens) ||
            (Number(item.prompt_tokens) || 0) +
              (Number(item.completion_tokens) || 0);
          const modelName = escapeHtml(item.model || item.model_name || "");
          const fullId = hasId ? escapeHtml(rawId) : "";

          return `
        <div class="row" style="--i:${index};--n:${grp.items.length}" title="${providerLabel(rawProvider)}${fullId ? " · " + fullId : ""}${modelName ? " · " + modelName : ""}${rawTokens > 0 ? " · " + fmtN(rawTokens) + " Tokens" : ""}">
          <div class="who">
            <div class="name" title="${prompt}">${prompt}</div>
          </div>
          ${modelName ? `<span class="model" title="${modelName}">${modelName}</span>` : ""}
          <time class="when">${escapeHtml(timeStr)}</time>
        </div>
      `;
        })
        .join("");

      return `
      <div class="list" data-group="${escapeHtml(k)}">
        <div class="group-head${closed ? " closed" : ""}" role="button" tabindex="0" aria-expanded="${!closed}" onclick="toggleSessionGroup(this)">
          <span class="ic accent sm">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
          </span>
          <span class="gname" title="${safeName}">${safeName}</span>
          <span class="gpath" title="${safePath}">${safePath}</span>
          <span class="gcount" title="${count} 个会话${count !== total ? "，共 " + total + " 个" : ""}">${count}${count !== total ? " / " + total : ""}</span>
          <span class="chev">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>
          </span>
        </div>
        <div class="group-body${closed ? " folded" : ""}" data-group-body>
          <div class="group-rows">${itemsHtml}</div>
        </div>
      </div>
    `;
    })
    .join("");
  sessionsRenderedAt = Date.now();
}

function sessionRecencyValue(s) {
  return (
    s.last_activity_at_ms ||
    s.time_updated ||
    s.updatedAt ||
    s.recencyAt ||
    s.createdAt ||
    0
  );
}

function sessionRecency(a, b) {
  return sessionRecencyValue(b) - sessionRecencyValue(a);
}

function dedupeSessions(sessions) {
  const seen = new Set();
  const out = [];
  for (const s of sessions) {
    const sid = s.provider_session_id || s.id || s.session_id;
    if (sid) {
      if (seen.has(sid)) continue;
      seen.add(sid);
    }
    out.push(s);
  }
  return out;
}

// a group folds from its head; the fold survives re-renders and reloads.
// The body is a grid whose rows go 1fr → 0fr, so the fold is the same
// motion whether the group opens or shuts (dashboard.css .group-body);
// the rows' own staggered arrival is CSS's, from --i / --n.
const collapsedGroups = new Set(
  (() => {
    try {
      return JSON.parse(localStorage.getItem(STORE.closedGroups) || "[]");
    } catch (e) {
      return [];
    }
  })(),
);

function toggleSessionGroup(head) {
  const list = head.closest(".list");
  const body = list && list.querySelector(".group-body");
  if (!body) return;
  const key = list.getAttribute("data-group") || "";
  const closing = !head.classList.contains("closed");
  head.classList.toggle("closed", closing);
  head.setAttribute("aria-expanded", String(!closing));
  body.classList.toggle("folded", closing);
  if (closing) collapsedGroups.add(key);
  else collapsedGroups.delete(key);
  try {
    localStorage.setItem(
      STORE.closedGroups,
      JSON.stringify([...collapsedGroups]),
    );
  } catch (e) {}
}

function filterSessionsList() {
  const q = (document.getElementById("sessionSearchInput")?.value || "")
    .toLowerCase()
    .trim();
  const badge = document.getElementById("sessionCountBadge");
  const base = dedupeSessions(lastGoodSessions);
  if (!q) {
    if (badge && base.length)
      badge.textContent = `${base.length.toLocaleString()} 个历史会话`;
    renderSessions(lastGoodSessions);
    return;
  }
  const filtered = base.filter((s) => {
    const text =
      `${s.prompt || ""} ${s.title || ""} ${s.session_id || ""} ${s.id || ""} ${s.workspace_name || ""} ${s.workspace_path || ""} ${s.model || ""} ${s.model_name || ""}`.toLowerCase();
    return text.includes(q);
  });
  // the rows are drawn without the badge being reset behind our back
  renderSessions(filtered, "data");
  if (badge)
    badge.textContent = `${filtered.length.toLocaleString()} / ${base.length.toLocaleString()} 个历史会话`;
}

// Number and cost formatting
function fmtN(n, formatUnitTag = false) {
  if (typeof n !== "number") n = Number(n) || 0;
  let val = "";
  let unit = "";
  if (n >= 1e9) {
    val = (n / 1e9).toFixed(2);
    unit = "B";
  } else if (n >= 1e7) {
    val = String(Math.round(n / 1e6));
    unit = "M";
  } else if (n >= 1e6) {
    val = (n / 1e6).toFixed(1);
    unit = "M";
  } else if (n >= 1e5) {
    val = String(Math.round(n / 1e3));
    unit = "K";
  } else if (n >= 1e3) {
    val = (n / 1e3).toFixed(1);
    unit = "K";
  } else {
    return String(n);
  }
  if (formatUnitTag) {
    return `${val}<i class="u">${unit}</i>`;
  }
  return `${val}${unit}`;
}

function getModelIcon(name, size = 18) {
  const k = String(name || "")
    .toLowerCase()
    .trim();
  if (
    k.includes("claude") ||
    k.includes("sonnet") ||
    k.includes("opus") ||
    k.includes("haiku") ||
    k.includes("fable") ||
    k.includes("mythos")
  ) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect width="24" height="24" rx="4.5" fill="#FAF3EF"/><path d="M4.709 15.955l4.72-2.647.08-.23-.08-.128H9.2l-.79-.048-2.698-.073-2.339-.097-2.266-.122-.571-.121L0 11.784l.055-.352.48-.321.686.06 1.52.103 2.278.158 1.652.097 2.449.255h.389l.055-.157-.134-.098-.103-.097-2.358-1.596-2.552-1.688-1.336-.972-.724-.491-.364-.462-.158-1.008.656-.722.881.06.225.061.893.686 1.908 1.476 2.491 1.833.365.304.145-.103.019-.073-.164-.274-1.355-2.446-1.446-2.49-.644-1.032-.17-.619a2.97 2.97 0 01-.104-.729L6.283.134 6.696 0l.996.134.42.364.62 1.414 1.002 2.229 1.555 3.03.456.898.243.832.091.255h.158V9.01l.128-1.706.237-2.095.23-2.695.08-.76.376-.91.747-.492.584.28.48.685-.067.444-.286 1.851-.559 2.903-.364 1.942h.212l.243-.242.985-1.306 1.652-2.064.73-.82.85-.904.547-.431h1.033l.76 1.129-.34 1.166-1.064 1.347-.881 1.142-1.264 1.7-.79 1.36.073.11.188-.02 2.856-.606 1.543-.28 1.841-.315.833.388.091.395-.328.807-1.969.486-2.309.462-3.439.813-.042.03.049.061 1.549.146.662.036h1.622l3.02.225.79.522.474.638-.079.485-1.215.62-1.64-.389-3.829-.91-1.312-.329h-.182v.11l1.093 1.068 2.006 1.81 2.509 2.33.127.578-.322.455-.34-.049-2.205-1.657-.851-.747-1.926-1.62h-.128v.17l.444.649 2.345 3.521.122 1.08-.17.353-.608.213-.668-.122-1.374-1.925-1.415-2.167-1.143-1.943-.14.08-.674 7.254-.316.37-.729.28-.607-.461-.322-.747.322-1.476.389-1.924.315-1.53.286-1.9.17-.632-.012-.042-.14.018-1.434 1.967-2.18 2.945-1.726 1.845-.414.164-.717-.37.067-.662.401-.589 2.388-3.036 1.44-1.882.93-1.086-.006-.158h-.055L4.132 18.56l-1.13.146-.487-.456.061-.746.231-.243 1.908-1.312-.006.006z" fill="#D97757" fill-rule="nonzero"/></svg>`;
  }
  if (
    k.includes("gpt") ||
    k.includes("codex") ||
    k.includes("o1") ||
    k.includes("o3") ||
    k.includes("o4")
  ) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect width="24" height="24" rx="4.5" fill="#3941FF"/><circle cx="12" cy="12" r="5" fill="#FFFFFF"/></svg>`;
  }
  if (k.includes("deepseek")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect width="24" height="24" rx="4.5" fill="#0066FF"/><path d="M6 14c2-4 6-6 10-4-3 5-7 6-10 4z" fill="#FFFFFF"/></svg>`;
  }
  if (k.includes("gemini")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 28 28" fill="none"><defs><radialGradient id="m-gemini-sparkle-${size}" cx="0" cy="0" r="1" gradientUnits="userSpaceOnUse" gradientTransform="translate(2.77876 11.3795) rotate(18.6832) scale(29.8025 238.737)"><stop offset="0.0671246" stop-color="#9168C0"/><stop offset="0.342551" stop-color="#5684D1"/><stop offset="0.672076" stop-color="#1BA1E3"/></radialGradient></defs><path d="M14 28C14 26.0633 13.6267 24.2433 12.88 22.54C12.1567 20.8367 11.165 19.355 9.905 18.095C8.645 16.835 7.16333 15.8433 5.46 15.12C3.75667 14.3733 1.93667 14 0 14C1.93667 14 3.75667 13.6383 5.46 12.915C7.16333 12.1683 8.645 11.165 9.905 9.905C11.165 8.645 12.1567 7.16333 12.88 5.46C13.6267 3.75667 14 1.93667 14 0C14 1.93667 14.3617 3.75667 15.085 5.46C15.8317 7.16333 16.835 8.645 18.095 9.905C19.355 11.165 20.8367 12.1683 22.54 12.915C24.2433 13.6383 26.0633 14 28 14C26.0633 14 24.2433 14.3733 22.54 15.12C20.8367 15.8433 19.355 16.835 18.095 18.095C16.835 19.355 15.8317 20.8367 15.085 22.54C14.3617 24.2433 14 26.0633 14 28Z" fill="url(#m-gemini-sparkle-${size})"/></svg>`;
  }
  if (k.includes("qwen")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect width="24" height="24" rx="4.5" fill="#615CED"/><path d="M12 5.5l5.5 3.5v6l-5.5 3.5L6.5 15V9L12 5.5z" stroke="#FFFFFF" stroke-width="2" stroke-linejoin="round"/></svg>`;
  }
  if (k.includes("minimax") || k.includes("abab")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect width="24" height="24" rx="4.5" fill="#FF4D4F"/><path d="M6 15V9l4 4 4-4v6" stroke="#FFFFFF" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
  }
  if (k.includes("glm") || k.includes("zhipu")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect width="24" height="24" rx="4.5" fill="#0F62FE"/><circle cx="12" cy="12" r="5" stroke="#FFFFFF" stroke-width="2.2"/><circle cx="12" cy="12" r="1.8" fill="#FFFFFF"/></svg>`;
  }
  if (k.includes("grok")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect width="24" height="24" rx="4.5" fill="#111113"/><path d="M7 17l10-10M17 17L7 7" stroke="#FFFFFF" stroke-width="2.5" stroke-linecap="round"/></svg>`;
  }
  if (k.includes("step")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect width="24" height="24" rx="4.5" fill="#2563EB"/><path d="M6 18h4v-4h4v-4h4V6" stroke="#FFFFFF" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
  }
  if (k.includes("stealth") || k.includes("bunny")) {
    return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none"><rect width="24" height="24" rx="4.5" fill="#7C3AED"/><path d="M12 4l3 6-3 4-3-4 3-6zm0 10l4 6H8l4-6z" fill="#FFFFFF"/></svg>`;
  }
  return `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="4" width="16" height="16" rx="3"/><path d="M9 9h6v6H9zM9 1v3M15 1v3M9 20v3M15 20v3M20 9h3M20 15h3M1 9h3M1 15h3"/></svg>`;
}

function fmtCostUsd(cost) {
  if (cost >= 100) return "≈$" + Math.round(cost);
  return "≈$" + Number(cost).toFixed(2);
}

// a row the daemon has not priced shows a dash, never a locally derived
// number: an unpriced model must not read as free. A model the daemon did
// price at zero is a different fact and keeps its ≈$0.00.
function costCell(cost) {
  if (cost === null || cost === undefined) return "—";
  return fmtCostUsd(cost);
}

// the daemon is the only place that turns tokens into money: it prices
// input, output and cache reads separately (web_server.rs calc_cost). The
// page reads those figures back and never derives one of its own, so the
// 模型 and 用量 views cannot report two different numbers for one model.
// A model the daemon has not priced reads as a dash, not a local guess.
function backendModelCosts() {
  const models =
    (dashboardData && dashboardData.usage && dashboardData.usage.models) || [];
  const map = {};
  models.forEach((m) => {
    if (!m || !m.name) return;
    // keep "no price" distinguishable from "priced at zero": only a real
    // number goes into the map
    const cost = m.estimated_cost_usd;
    if (cost === null || cost === undefined) return;
    const n = Number(cost);
    if (Number.isFinite(n)) map[String(m.name).toLowerCase()] = n;
  });
  return map;
}

function renderUsageAnalytics(usage, sessions, stats) {
  let costUsd = null; // null until the daemon's own pricing arrives
  let totalTokens = 0;
  let inputTokens = 0;
  let outputTokens = 0;
  let cacheRead = 0;
  let cacheHitRate = 0;
  let calls = 0;
  let modelsList = [];
  let agentsList = [];

  if (
    usage &&
    typeof usage === "object" &&
    (usage.total_tokens ||
      usage.calls ||
      (usage.models && usage.models.length > 0))
  ) {
    // Authoritative backend aggregated metrics. A missing or non-numeric cost
    // stays null so the tile can tell "not priced" from "priced at zero".
    const rawCost = usage.cost_usd;
    costUsd =
      rawCost === null || rawCost === undefined
        ? null
        : Number.isFinite(Number(rawCost))
          ? Number(rawCost)
          : null;
    totalTokens = Number(usage.total_tokens) || 0;
    inputTokens = Number(usage.input) || 0;
    outputTokens = Number(usage.output) || 0;
    cacheRead = Number(usage.cache_read) || 0;
    cacheHitRate = Number(usage.cache_hit_rate_pct) || 0;
    calls = Number(usage.calls) || (sessions ? sessions.length : 0);
    modelsList = Array.isArray(usage.models) ? usage.models : [];
    agentsList = Array.isArray(usage.agents) ? usage.agents : [];
  } else {
    // Robust client-side fallback aggregation. Token counts can be summed
    // here; money cannot — pricing lives in the daemon, so these rows carry
    // no cost and the renderer shows a dash instead of a made-up figure.
    const modelMap = {};
    const agentMap = {};
    (sessions || []).forEach((s) => {
      const pt = Number(s.prompt_tokens) || 0;
      const ct = Number(s.completion_tokens) || 0;
      const tt = Number(s.total_tokens) || pt + ct;
      const cr = Number(s.cache_read_tokens) || 0;
      const reqCount = Number(s.request_count || s.calls) || 1;
      inputTokens += pt;
      outputTokens += ct;
      totalTokens += tt;
      cacheRead += cr;
      calls += reqCount;

      let m = (s.model || s.model_name || "").trim() || "default";
      if (!modelMap[m])
        modelMap[m] = {
          name: m,
          calls: 0,
          total_tokens: 0,
          prompt_tokens: 0,
          cache_read_tokens: 0,
        };
      modelMap[m].calls += reqCount;
      modelMap[m].total_tokens += tt;
      modelMap[m].prompt_tokens += pt;
      modelMap[m].cache_read_tokens += cr;

      let a = (s.provider || s.agent_name || "agent").trim();
      if (!agentMap[a])
        agentMap[a] = {
          id: a,
          name: a,
          calls: 0,
          total_tokens: 0,
          prompt_tokens: 0,
          cache_read_tokens: 0,
        };
      agentMap[a].calls += reqCount;
      agentMap[a].total_tokens += tt;
      agentMap[a].prompt_tokens += pt;
      agentMap[a].cache_read_tokens += cr;
    });

    if (inputTokens > 0) {
      cacheHitRate = Math.min(100, Math.round((cacheRead / inputTokens) * 100));
    }

    const totalAll = Math.max(1, totalTokens);
    modelsList = Object.values(modelMap).map((m) => {
      const hr =
        m.prompt_tokens > 0
          ? Math.min(
              100,
              Math.round((m.cache_read_tokens / m.prompt_tokens) * 100),
            )
          : 0;
      return {
        name: m.name,
        calls: m.calls,
        total_tokens: m.total_tokens,
        share_pct: Math.min(100, Math.round((m.total_tokens / totalAll) * 100)),
        cache_hit_rate_pct: hr,
      };
    });
    modelsList.sort((a, b) => b.total_tokens - a.total_tokens);

    agentsList = Object.values(agentMap).map((a) => {
      const hr =
        a.prompt_tokens > 0
          ? Math.min(
              100,
              Math.round((a.cache_read_tokens / a.prompt_tokens) * 100),
            )
          : 0;
      return {
        id: a.id,
        name: a.name,
        calls: a.calls,
        total_tokens: a.total_tokens,
        share_pct: Math.min(100, Math.round((a.total_tokens / totalAll) * 100)),
        cache_hit_rate_pct: hr,
      };
    });
    agentsList.sort((a, b) => b.total_tokens - a.total_tokens);
  }

  // Update the 4 KPI tiles
  const costEl = document.getElementById("kpiCost");
  const tokensEl = document.getElementById("kpiTokens");
  const tokensSubEl = document.getElementById("kpiTokensSub");
  const cacheEl = document.getElementById("kpiCache");
  const cacheSubEl = document.getElementById("kpiCacheSub");
  const callsEl = document.getElementById("kpiCalls");
  const callsSubEl = document.getElementById("kpiCallsSub");

  // the figure is the daemon's; when the fallback path ran there is none,
  // and the tile says so rather than showing a locally invented total
  if (costEl)
    costEl.textContent = costUsd === null ? "—" : `≈$${costUsd.toFixed(2)}`;
  if (tokensEl) tokensEl.innerHTML = fmtN(totalTokens, true);
  if (tokensSubEl)
    tokensSubEl.textContent = `${fmtN(inputTokens)} in · ${fmtN(outputTokens)} out`;
  if (cacheEl) cacheEl.innerHTML = fmtN(cacheRead, true);
  if (cacheSubEl) cacheSubEl.textContent = `输入命中 ${cacheHitRate}%`;
  if (cacheSubEl)
    cacheSubEl.title = `${fmtN(cacheRead)} 输入 Token 由上游缓存直接返回，未重新计算`;
  if (callsEl) callsEl.textContent = calls.toLocaleString();
  const sessionsCount = Number((usage && usage.sessions) ?? (sessions ? sessions.length : 0));
  if (callsSubEl) {
    if (sessionsCount > 0 && calls > sessionsCount) {
      callsSubEl.textContent = `${sessionsCount.toLocaleString()} 个会话 · ${calls.toLocaleString()} 次请求`;
    } else {
      callsSubEl.textContent = "跨多智能体累计请求";
    }
  }

  // Render the models breakdown
  const modelsContainer = document.getElementById("usageModels");
  const modelsCountEl = document.getElementById("usageModelsCount");
  if (modelsCountEl) modelsCountEl.textContent = `${modelsList.length} 种模型`;

  const isUsageLoading = !dashboardData || (!dashboardData.usage && (!sessions || sessions.length === 0));

  if (modelsContainer) {
    if (isUsageLoading) {
      modelsContainer.innerHTML = renderSkeletonCard(4);
    } else if (modelsList.length === 0) {
      modelsContainer.innerHTML = `
        <div class="empty-state">
          <b>暂无模型调用数据</b>
          会话产生实际交互后将自动统计 Token 消耗
        </div>
      `;
    } else {
      modelsContainer.innerHTML = modelsList
        .map((m) => {
          const pct = Math.max(0, Math.min(100, m.share_pct || 0));
          const cStr = costCell(m.estimated_cost_usd);
          return `
          <div class="row stat">
            <span class="ic">${getModelIcon(m.name, 20)}</span>
            <div class="who">
              <div class="name-line">
                <span class="name">${escapeHtml(m.name)}</span>
              </div>
              <div class="sub">${(m.calls || 0).toLocaleString()} 次调用 · 缓存命中 ${m.cache_hit_rate_pct || 0}%</div>
            </div>
            <div class="share" title="${pct}% 调用占比">
              <i style="width: ${Math.max(2, pct)}%;"></i>
            </div>
            <div class="num">
              <b>${fmtN(m.total_tokens || 0)}</b>
              <small>${pct}%</small>
            </div>
            <div class="cost">${cStr}</div>
          </div>
        `;
        })
        .join("");
    }
  }

  // Render the agents breakdown
  const agentsContainer = document.getElementById("usageAgents");
  const agentsCountEl = document.getElementById("usageAgentsCount");
  if (agentsCountEl)
    agentsCountEl.textContent = `${agentsList.length} 个智能体`;

  if (agentsContainer) {
    if (isUsageLoading) {
      agentsContainer.innerHTML = renderSkeletonCard(4);
    } else if (agentsList.length === 0) {
      agentsContainer.innerHTML = `
        <div class="empty-state">
          <b>暂无智能体调用数据</b>
          会话产生实际交互后将自动统计
        </div>
      `;
    } else {
      agentsContainer.innerHTML = agentsList
        .map((a) => {
          const pct = Math.max(0, Math.min(100, a.share_pct || 0));
          const cStr = costCell(a.estimated_cost_usd);
          return `
          <div class="row stat">
            <span class="ic">${getAgentIcon(a.id || a.name, 20)}</span>
            <div class="who">
              <div class="name-line">
                <span class="name">${escapeHtml(a.name)}</span>
              </div>
              <div class="sub">${(a.calls || 0).toLocaleString()} 次调用 · 缓存命中 ${a.cache_hit_rate_pct || 0}%</div>
            </div>
            <div class="share" title="${pct}% 调用占比">
              <i style="width: ${Math.max(2, pct)}%;"></i>
            </div>
            <div class="num">
              <b>${fmtN(a.total_tokens || 0)}</b>
              <small>${pct}%</small>
            </div>
            <div class="cost">${cStr}</div>
          </div>
        `;
        })
        .join("");
    }
  }
}

function renderModels(models) {
  const container = document.getElementById("modelsGrid");
  const countBadge = document.getElementById("modelsCountBadge");
  if (!container) return;

  let list = Array.isArray(models) && models.length > 0 ? [...models] : [];

  // Calculate token counts and call counts from sessions
  const modelStats = {};
  let totalTokensAll = 0;
  (allSessionsData || []).forEach((s) => {
    const rawM = (s.model || s.model_name || "").trim();
    const m = rawM.toLowerCase();
    const pt = s.prompt_tokens || 0;
    const ct = s.completion_tokens || 0;
    const tt = pt + ct || s.total_tokens || 0;
    if (m) {
      if (!modelStats[m])
        modelStats[m] = { in: 0, out: 0, total: 0, calls: 0, rawName: rawM };
      modelStats[m].in += pt;
      modelStats[m].out += ct;
      modelStats[m].total += tt;
      modelStats[m].calls += 1;
      totalTokensAll += tt;
    }
  });

  // If the backend sent no catalog, build one from the sessions seen
  if (list.length === 0) {
    Object.keys(modelStats).forEach((k) => {
      const st = modelStats[k];
      list.push({
        id: k,
        display_name: st.rawName || k,
        supports_vision: false,
        supports_reasoning: false,
        context_window_str: "",
      });
    });
  }

  if (list.length === 0) {
    if (countBadge) countBadge.textContent = "0 种模型";
    container.innerHTML = `
      <div class="empty-state">
        <b>暂无模型记录</b>
        会话中使用模型后将自动提取并展示真实模型画册
      </div>
    `;
    return;
  }

  const norm = (s) =>
    String(s || "")
      .toLowerCase()
      .replace(/[^a-z0-9]/g, "");
  // the transcripts record the name the CLI reported, the catalog carries the
  // gateway's id, so match on a normalised form and only on exact equality:
  // a loose match would credit one model's tokens to unrelated rows
  const claimed = new Set();
  const matchStats = (id, name) => {
    const direct =
      modelStats[String(id || "").toLowerCase()] ||
      modelStats[String(name || "").toLowerCase()];
    if (direct) return direct;
    const target = norm(id) || norm(name);
    if (!target) return undefined;
    const key = Object.keys(modelStats).find((k) => norm(k) === target);
    return key ? modelStats[key] : undefined;
  };

  const items = list.map((m) => {
    const id = (m.id || m.name || "").toLowerCase();
    const name = m.display_name || m.name || m.id || "Unknown Model";

    const st = matchStats(id, name);
    if (st) claimed.add(st.rawName.toLowerCase());

    const tags = [];
    if (m.supports_vision) tags.push('<span class="tag accent">Vision</span>');
    if (m.supports_reasoning)
      tags.push('<span class="tag warn">Thinking</span>');
    // only a context window the backend declared, never a fabricated default
    const ctxStr =
      m.context_window_str ||
      (m.context_length > 0
        ? m.context_length >= 1000000
          ? Math.round(m.context_length / 1000000) + "M"
          : Math.round(m.context_length / 1000) + "K"
        : "");
    if (ctxStr) {
      tags.push(`<span class="tag ok">${escapeHtml(ctxStr)}</span>`);
    }

    return {
      id: m.id,
      name,
      tags,
      totalTokens: st ? st.total : 0,
      inTokens: st ? st.in : 0,
      outTokens: st ? st.out : 0,
      calls: st ? st.calls : 0,
    };
  });

  // models the transcripts recorded under a name the catalog does not carry
  // still belong in the ranking, with their real token counts
  Object.keys(modelStats).forEach((k) => {
    const st = modelStats[k];
    if (claimed.has(st.rawName.toLowerCase())) return;
    items.push({
      id: st.rawName,
      name: st.rawName,
      tags: [],
      totalTokens: st.total,
      inTokens: st.in,
      outTokens: st.out,
      calls: st.calls,
    });
  });

  items.sort(
    (a, b) => b.totalTokens - a.totalTokens || a.name.localeCompare(b.name),
  );

  if (countBadge) {
    countBadge.textContent = `${items.length} 种模型`;
  }

  const maxTokens = Math.max(1, items[0]?.totalTokens || totalTokensAll || 1);

  const costMap = backendModelCosts();

  container.innerHTML = items
    .map((item) => {
      const safeName = escapeHtml(item.name);
      const pct = Math.min(
        100,
        Math.round((item.totalTokens / maxTokens) * 100),
      );
      const tokensStr = fmtN(item.totalTokens);
      const subText =
        item.calls > 0
          ? `${fmtN(item.inTokens)} in · ${fmtN(item.outTokens)} out`
          : "就绪";
      const costKey = String(item.name).toLowerCase();
      // only the daemon's figure; a model it has not priced reads as a dash,
      // the same as in the usage view
      const costStr = Object.prototype.hasOwnProperty.call(costMap, costKey)
        ? costCell(costMap[costKey])
        : "—";
      const callsText =
        item.calls > 0 ? `<span>${item.calls} 次调用</span>` : "";

      return `<div class="row stat">
        <span class="ic">${getModelIcon(item.name, 20)}</span>
        <div class="who">
          <div class="name-line"><span class="name" title="${safeName}">${safeName}</span></div>
          <div class="sub flow">${callsText}${item.tags.join(" ")}</div>
        </div>
        <div class="share" title="${pct}% 调用占比">
          <i style="width: ${Math.max(item.totalTokens > 0 ? 2 : 0, pct)}%;"></i>
        </div>
        <div class="num">
          <b>${tokensStr}</b>
          <small>${subText}</small>
        </div>
        <div class="cost">${costStr}</div>
      </div>`;
    })
    .join("");
}

async function refreshModelsCatalog(event) {
  const btn = document.getElementById("btnRefreshModels");
  if (btn) {
    btn.disabled = true;
    btn.classList.add("busy");
  }
  try {
    const resp = await fetch("/api/models/refresh", { method: "POST" });
    if (resp.ok) {
      const data = await resp.json();
      if (data && data.data) {
        renderModels(data.data);
        toast(`已同步上游模型列表，共 ${data.data.length} 个可用模型`);
      } else {
        toast("上游没有返回模型列表，请稍后再试", "err");
      }
    } else {
      toast(`同步模型失败（HTTP ${resp.status}）`, "err");
    }
  } catch (e) {
    toast("同步模型失败：" + e.message, "err");
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove("busy");
    }
  }
}

function renderWorkspaces(workspaces) {
  const container = document.getElementById("workspacesList");
  const countBadge = document.getElementById("workspacesCountBadge");
  if (countBadge) {
    const count = workspaces ? workspaces.length : 0;
    countBadge.textContent = `${count} 个工作区`;
  }
  if (!container) return;

  if (!workspaces || workspaces.length === 0) {
    container.innerHTML = `
      <div class="empty-state">
        <b>暂未检测到已注册的工作区</b>
        在项目目录下启动任意官方 Agent 后将自动登记
      </div>
    `;
    return;
  }

  // Map sessions to workspaces by path or name to get token volume
  const wsStats = {};
  let maxWsTokens = 1;
  (allSessionsData || []).forEach((s) => {
    const rawPath = (s.workspace_path || s.cwd || "").toLowerCase();
    const rawName = (s.workspace_name || "").toLowerCase();
    const pt = s.prompt_tokens || 0;
    const ct = s.completion_tokens || 0;
    const tt = pt + ct || s.total_tokens || 0;

    for (const ws of workspaces) {
      const wp = (ws.path || "").toLowerCase();
      const wn = (ws.name || "").toLowerCase();
      if (
        (rawPath &&
          (rawPath === wp || rawPath.endsWith(wp) || wp.endsWith(rawPath))) ||
        (rawName && rawName === wn)
      ) {
        if (!wsStats[ws.path]) wsStats[ws.path] = 0;
        wsStats[ws.path] += tt;
        if (wsStats[ws.path] > maxWsTokens) maxWsTokens = wsStats[ws.path];
        break;
      }
    }
  });

  // Sort workspaces by token volume, then session count
  const sorted = [...workspaces].sort((a, b) => {
    const ta = wsStats[a.path] || 0;
    const tb = wsStats[b.path] || 0;
    if (tb !== ta) return tb - ta;
    return (b.sessions_count || 0) - (a.sessions_count || 0);
  });

  container.innerHTML = sorted
    .map((ws) => {
      const safePath = escapeHtml(ws.path);
      const safeName = escapeHtml(ws.name);
      const relativeTime = escapeHtml(ws.last_active_str || "历史会话");
      const tokens = wsStats[ws.path] || 0;
      const pct = Math.min(100, Math.round((tokens / maxWsTokens) * 100));
      const gitBadge = ws.is_git ? `<span class="tag ok">Git</span>` : "";
      const count = ws.sessions_count || 0;

      return `<div class="row stat" title="${safePath}">
        <span class="ic"><span class="mono ws-ini">${escapeHtml(pathLeaf(ws.path).slice(0, 2).toUpperCase())}</span></span>
        <div class="who">
          <div class="name-line">
            <span class="name" title="${safeName}">${safeName}</span>
            ${gitBadge}
          </div>
          <div class="sub mono ws-loc"><span class="ws-parent">${shortParent(ws.path)}</span><span class="ws-leaf">${escapeHtml(pathLeaf(ws.path))}</span></div>
        </div>
        <div class="share" title="${pct}% 活跃度">
          <i style="width: ${Math.max(tokens > 0 ? 2 : count ? 5 : 0, pct)}%;"></i>
        </div>
        <div class="num">
          <b>${tokens > 0 ? fmtN(tokens) : "—"}</b>
          <small>${count} 会话</small>
        </div>
        <div class="cost">${relativeTime}</div>
      </div>`;
    })
    .join("");
}

function pathParts(p) {
  return String(p || "")
    .split(/[\\/]/)
    .filter(Boolean);
}

function pathLeaf(p) {
  const parts = pathParts(p);
  return parts.length ? parts[parts.length - 1] : String(p || "");
}

// the folder a workspace lives in, trimmed from the front so the nearest
// parent survives: "...\Project\agenthub" reads better than "D:\Claude..."
function shortParent(p) {
  const parts = pathParts(p);
  if (parts.length < 2) return "";
  const parent = parts.slice(0, -1);
  const tail = parent.slice(-2).join("/");
  return (parent.length > 2 ? "…/" : "") + tail + "/";
}

function renderGateway(gateway) {
  if (!gateway) return;
  const baseUrl = gateway.base_url || GATEWAY_URL;
  const host = String(baseUrl).replace(/^https?:\/\//, "");

  const badge = document.getElementById("gatewayBadge");
  if (badge) {
    const source = gateway.source || "gateway";
    const text = gateway.detected ? `已连接 · ${source}` : "标准端点就绪";
    badge.textContent = text;
    // the detected source can carry a full credential file name; keep the row
    // from stretching and let the title hold the whole value
    badge.title = text;
    badge.className = gateway.detected ? "tag ok cap-long" : "tag cap-long";
  }

  const gwBtn = document.getElementById("sideGwBtn");
  const gwText = document.getElementById("sideGwText");
  if (gwBtn) gwBtn.classList.toggle("off", !gateway.detected);
  if (gwText) {
    gwText.textContent = gateway.detected ? `网关 ${host}` : "网关未探测";
  }

  const envElem = document.getElementById("envSnippet");
  if (envElem) {
    envElem.textContent = `export ANTHROPIC_BASE_URL="${baseUrl}"\nexport ANTHROPIC_CUSTOM_HEADERS="${AFFINITY_HEADER}=agenthub-session-1"`;
  }
  const psElem = document.getElementById("psSnippet");
  if (psElem) {
    psElem.textContent = `$env:ANTHROPIC_BASE_URL = "${baseUrl}"\n$env:ANTHROPIC_CUSTOM_HEADERS = "${AFFINITY_HEADER}=agenthub-session-1"`;
  }
  const baseElem = document.getElementById("gwBaseUrl");
  if (baseElem) baseElem.textContent = baseUrl;
  const srcElem = document.getElementById("gwSource");
  if (srcElem) {
    srcElem.textContent = gateway.detected
      ? gateway.source || "上游网关"
      : "未探测到上游网关，当前使用标准端点";
  }
}

function renderDevice(device) {
  if (!device) return;
  const deviceIdElem = document.getElementById("modalDeviceId");
  const deviceKeyElem = document.getElementById("modalDeviceKey");
  const pairedBox = document.getElementById("pairedStatusBox");
  const pairedInfo = document.getElementById("pairedGatewayInfo");

  const pairBtn = document.getElementById("headerPairBtn");
  const pairText = document.getElementById("headerPairText");
  const statStatus = document.getElementById("statWebPairingStatus");
  const statSub = document.getElementById("statWebPairingSub");

  if (deviceIdElem) deviceIdElem.textContent = device.device_id || "-";
  if (deviceKeyElem) {
    const pk = device.public_key || "";
    deviceKeyElem.textContent =
      pk.length > 20
        ? pk.substring(0, 10) + "..." + pk.substring(pk.length - 8)
        : pk;
    deviceKeyElem.title = pk;
  }

  if (device.paired) {
    if (pairBtn) pairBtn.classList.remove("off");
    const gw = device.gateway_url
      ? device.gateway_url.replace(/^https?:\/\//, "")
      : "已配对";
    if (pairText) pairText.textContent = `已绑定 ${gw}`;
    if (statStatus) {
      statStatus.textContent = "已绑定";
      statStatus.style.color = "var(--green)";
    }
    if (statSub) statSub.textContent = gw;
    setPairTile(false);
    if (pairedBox) {
      pairedBox.style.display = "block";
      if (pairedInfo) {
        pairedInfo.textContent = `网关: ${device.gateway_url || "-"} · 配对时间: ${device.paired_at || "-"}`;
      }
    }
  } else {
    if (pairBtn) pairBtn.classList.add("off");
    if (pairText) pairText.textContent = "未绑定 Web 控制台";
    if (statStatus) {
      statStatus.textContent = "未绑定";
      statStatus.style.color = "";
    }
    if (statSub) statSub.textContent = "点击关联 Web 控制台";
    setPairTile(true);
    if (pairedBox) pairedBox.style.display = "none";
  }
}

// the ledger's pairing tile is a call to action only while it is unpaired
function setPairTile(cta) {
  const tile = document.getElementById("pairTile");
  if (!tile) return;
  tile.classList.toggle("cta", !!cta);
  tile.title = cta ? "点击关联云端 Web 控制台" : "已关联云端 Web 控制台";
}

function toggleTheme() {
  const html = document.documentElement;
  const isDark = html.classList.contains("dark");
  const themeMeta = document.getElementById("themeColorMeta");
  const apply = () => {
    if (isDark) {
      html.classList.remove("dark");
      localStorage.setItem(STORE.theme, "light");
      if (themeMeta) themeMeta.setAttribute("content", LAYOUT.themeBg.light);
    } else {
      html.classList.add("dark");
      localStorage.setItem(STORE.theme, "dark");
      if (themeMeta) themeMeta.setAttribute("content", LAYOUT.themeBg.dark);
    }
    updateThemeButtonUI();
    // the window's own caption is the page's colour too, and is painted by
    // the shell the next window opens with: leave the choice where it reads it
    syncWindowTheme();
  };
  // the new theme fades in as one sheet: every surface is given the same
  // colour transition for the length of the switch, then the rule goes
  // away so hover states keep their own quick timings. The old colours
  // have to be computed once after the class lands — a class and a theme
  // changed in the same frame leave the browser nothing to fade from.
  if (calmMotion()) {
    apply();
    return;
  }
  html.classList.add("theme-fade");
  void getComputedStyle(document.body).backgroundColor;
  requestAnimationFrame(apply);
  clearTimeout(toggleTheme._timer);
  toggleTheme._timer = setTimeout(
    () => html.classList.remove("theme-fade"),
    520,
  );
}

function updateThemeButtonUI() {
  const isDark = document.documentElement.classList.contains("dark");
  const iconSpan = document.getElementById("themeIcon");
  const themeMeta = document.getElementById("themeColorMeta");

  if (themeMeta) {
    themeMeta.setAttribute(
      "content",
      isDark ? LAYOUT.themeBg.dark : LAYOUT.themeBg.light,
    );
  }

  if (!iconSpan) return;
  if (isDark) {
    iconSpan.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4.2"/><path d="M12 2v2.2M12 19.8V22M4.22 4.22l1.56 1.56M18.22 18.22l1.56 1.56M2 12h2.2M19.8 12H22M4.22 19.78l1.56-1.56M18.22 5.78l1.56-1.56"/></svg>`;
  } else {
    iconSpan.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>`;
  }
}

async function shutdownRunner() {
  if (
    !confirm(
      "确定要退出 AgentHub Runner 吗？\n退出后右下角托盘图标和本地服务将一并终止。",
    )
  ) {
    return;
  }
  const modal = document.getElementById("shutdownModal");
  if (modal) modal.classList.add("open");

  try {
    await fetch("/api/shutdown", { method: "POST" });
    setTimeout(() => {
      document.getElementById("shutdownModalMsg").textContent =
        "AgentHub Runner 已完全退出。托盘图标已注销。您可以关闭此桌面窗口。";
    }, 600);
  } catch (e) {
    document.getElementById("shutdownModalMsg").textContent =
      "AgentHub Runner 已接收关闭信号并停止服务。";
  }
}

// One line of feedback, in the console's voice, where the action happened.
// A failed action reads in red; it is replaced by the next message.
let toastTimer = 0;
function toast(msg, kind) {
  const el = document.getElementById("toast");
  if (!el) return;
  el.textContent = msg;
  el.classList.toggle("err", kind === "err");
  el.classList.add("on");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(
    () => el.classList.remove("on"),
    TOAST_MS[kind === "err" ? "err" : "ok"],
  );
}

function copyGatewayConfig() {
  const baseUrl =
    dashboardData && dashboardData.gateway && dashboardData.gateway.base_url
      ? dashboardData.gateway.base_url
      : GATEWAY_URL;
  const text = `export ANTHROPIC_BASE_URL="${baseUrl}"\nexport ANTHROPIC_CUSTOM_HEADERS="${AFFINITY_HEADER}=agenthub-session-1"`;
  copyText(text);
  toast("已复制网关环境变量");
}

function copySnippet(id) {
  const elem = document.getElementById(id);
  if (elem) {
    copyText(elem.textContent);
    toast(id === "psSnippet" ? "已复制 PowerShell 配置" : "已复制 Bash 配置");
  }
}

function copyText(text) {
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(text);
  } else {
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    document.body.removeChild(ta);
  }
}

function openPairModal() {
  const modal = document.getElementById("pairModal");
  if (modal) modal.classList.add("open");
  const gatewayInput = document.getElementById("gatewayInput");
  if (gatewayInput && !gatewayInput.value) {
    const savedGateway =
      localStorage.getItem(STORE.customGateway) ||
      (dashboardData &&
        dashboardData.device &&
        dashboardData.device.gateway_url) ||
      (dashboardData && dashboardData.gateway && dashboardData.gateway.url) ||
      "";
    if (savedGateway) gatewayInput.value = savedGateway;
  }
  if (dashboardData && dashboardData.device) {
    renderDevice(dashboardData.device);
  }
}

function closePairModal() {
  const modal = document.getElementById("pairModal");
  if (modal) modal.classList.remove("open");
}

function copyDeviceId() {
  const idElem = document.getElementById("modalDeviceId");
  if (idElem && idElem.textContent && idElem.textContent !== "-") {
    copyText(idElem.textContent);
    toast("已复制设备 ID");
  }
}

async function submitDevicePairing(event) {
  event.preventDefault();
  const gatewayInput = document.getElementById("gatewayInput");
  const codeInput = document.getElementById("bindingCodeInput");
  const submitBtn = document.getElementById("pairSubmitBtn");

  const gateway = (gatewayInput ? gatewayInput.value : "").trim();
  const code = (codeInput ? codeInput.value : "").trim();

  if (!gateway) {
    toast("填写控制台地址，例如 https://agenthub.yourdomain.com", "err");
    return;
  }
  if (!code) {
    toast("填写 Web 控制台里生成的配对绑定码", "err");
    return;
  }

  try {
    localStorage.setItem(STORE.customGateway, gateway);
  } catch (e) {}

  const origText = submitBtn.innerHTML;
  submitBtn.disabled = true;
  submitBtn.innerHTML = "<span>正在验证绑定...</span>";

  try {
    const resp = await fetch("/api/device/bind", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ gateway: gateway, binding_code: code }),
    });

    const resData = await resp.json();
    if (resp.ok && (resData.ok === true || resData.status === "ok")) {
      if (codeInput) codeInput.value = "";
      closePairModal();
      await refreshDashboard();
      toast("已绑定到云端 AgentHub");
    } else {
      toast(
        "配对失败：" + (resData.message || "网关拒绝绑定或绑定码无效"),
        "err",
      );
    }
  } catch (err) {
    toast("配对失败：连不上网关（" + err.message + "）", "err");
  } finally {
    submitBtn.disabled = false;
    submitBtn.innerHTML = origText;
  }
}

function escapeHtml(str) {
  if (!str) return "";
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

window.addEventListener("DOMContentLoaded", init);
// a wider or narrower window: the header's track takes what now fits
window.addEventListener("resize", () => {
  fitNavTrack();
  scaleSegThumb(false);
  scaleRailHl(false);
});
