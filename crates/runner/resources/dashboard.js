let dashboardData = null;
let lastRenderState = {
  agentsJson: "",
  workspacesJson: "",
  deviceJson: "",
  sessionsJson: "",
  capabilitiesJson: "",
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
  workspaces: "工作区",
  settings: "设置",
};

const VIEW_EN_LABELS = {
  sessions: "Sessions",
  agents: "Agents",
  usage: "Usage",
  workspaces: "Workspaces",
  settings: "Settings",
};

// the settings page's own parts, one tab each, as magpie's Settings
const SET_TABS = ["general", "appearance", "gateway", "capabilities", "about"];

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
  grok_build: "Grok",
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
const GATEWAY_URL = `http://${location.hostname || "127.0.0.1"}:${LAYOUT.gatewayPort || 4000}`;
const TOAST_MS = { ok: 2400, err: 4200 };

// whether the reader asked for less motion — in the system, or in the
// console's own Settings › 外观 row; every scripted animation checks this
// once, the way magpie's navCalm() does
const calmMotion = () =>
  motionOff() || window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// The theme the bootstrap script already applied; the sheet only redraws it
function initTheme() {
  updateThemeChoice();
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
  const savedView = localStorage.getItem(STORE.view);
  switchView(VIEW_LABELS[savedView] ? savedView : "sessions", true);
  initSettingsPage();
  initUsage();
  // the tray's "gateway" row opens `#settings/gateway`; a saved page would
  // otherwise win over it
  applySettingsHash();
  // the first probe can land while the daemon is still scanning its
  // transcripts; ask again before telling anyone there are no sessions
  const ok = await refreshDashboard();
  if (!ok || (dashboardData && (dashboardData.ready === false || (dashboardData.sessions || []).length === 0))) {
    for (
      let i = 0;
      i < (LAYOUT.firstProbeRetries || 5) &&
      !(dashboardData && (dashboardData.sessions || []).length > 0 && dashboardData.ready !== false);
      i++
    ) {
      await new Promise((r) => setTimeout(r, LAYOUT.firstProbeRetryMs || 800));
      await refreshDashboard();
    }
  }
  startPolling();
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

// Keyboard shortcuts: Ctrl+Shift+S (sidebar), Ctrl+, (settings), Esc (close
// whatever is open, and back out of settings to the page it came from)
window.addEventListener("keydown", (e) => {
  if (
    e.ctrlKey &&
    e.shiftKey &&
    !e.altKey &&
    (e.key === "S" || e.key === "s")
  ) {
    e.preventDefault();
    toggleSideNav();
  } else if (e.ctrlKey && !e.shiftKey && !e.altKey && e.key === ",") {
    e.preventDefault();
    openSettings();
  } else if (e.key === "Escape") {
    closeDeleteSessions();
    closeRegisterMcpModal();
    leaveSettings();
  } else if (
    isSettingsOpen() &&
    (e.key === "ArrowLeft" || e.key === "ArrowRight") &&
    !e.ctrlKey &&
    !e.altKey &&
    !e.metaKey
  ) {
    // the tabs answer the arrow keys, as magpie's Settings tabs do
    const at = SET_TABS.indexOf(settingsTab);
    const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(e.target?.tagName || "");
    if (at >= 0 && !typing) {
      e.preventDefault();
      const next =
        SET_TABS[(at + (e.key === "ArrowRight" ? 1 : -1) + SET_TABS.length) % SET_TABS.length];
      setSetTab(next, true);
      document.getElementById("setTab-" + next)?.focus();
    }
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
  // Settings is a page like any other, but not a row in the rail: the page
  // the reader came from is kept, so leaving Settings turns back to it
  if (viewId !== "settings" && lastNavView !== viewId) lastNavView = viewId;
  // a click turns the page as one motion (view-transition in dashboard.css,
  // the way magpie's pages turn); the first paint, a restored page and a
  // reader who asked for less motion get the page at once.
  // A turn's own drawing runs after the browser has snapshotted the old page,
  // so a second switch made before that lands would otherwise draw an older
  // page over the newer one; each switch takes a number and only the newest
  // is allowed to draw it
  const gen = ++turnSeq;
  const turn = () => {
    if (gen !== turnSeq) return;
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
    // Settings is no rail row: its gear is the row, and it reads as open the
    // way magpie's #prefs does
    document
      .getElementById("btnSettings")
      ?.classList.toggle("on", viewId === "settings");

    const view = document.getElementById("view-" + viewId);
    if (view) {
      view.classList.add("active");
      if (!silent) view.scrollTop = 0;
      // the settings head's hairline follows the page's own scroll: it is
      // drawn only once the rows are under the head
      if (viewId === "settings") {
        const head = view.querySelector(".set-head");
        if (head && !head.dataset.scrollBound) {
          head.dataset.scrollBound = "1";
          view.addEventListener(
            "scroll",
            () => view.classList.toggle("scrolled", view.scrollTop > 2),
            { passive: true },
          );
        }
        view.classList.toggle("scrolled", view.scrollTop > 2);
      }
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
    // the settings page's own state is drawn as it comes into view
    if (viewId === "settings") renderSettings();
    // the usage page's charts need a laid-out card to measure: a page drawn
    // while hidden has no width, so the digest is drawn as it comes into view
    if (viewId === "usage") {
      if (!usageDigest && !usageLoading) {
        loadUsage(true);
      } else {
        drawUsage();
      }
    }
  };
  // a turn only draws when the window is painting frames: a tucked-away
  // console (or one in a background tab) would hold the switch until it is
  // looked at again, so there the page is simply drawn at once
  if (
    silent ||
    document.hidden ||
    calmMotion() ||
    !document.startViewTransition
  ) {
    turn();
    return;
  }
  document.documentElement.classList.add("page-turn");
  const vt = document.startViewTransition(turn);
  // a window that is up but not being painted (occluded, or a pane nobody is
  // looking at) never starts the turn at all: the page is handed over anyway
  // after a frame's worth of waiting, instead of the switch hanging on a frame
  // that will not come
  let drawn = false;
  const atOnce = setTimeout(() => {
    if (drawn) return;
    drawn = true;
    turn();
    if (gen === turnSeq) document.documentElement.classList.remove("page-turn");
  }, 300);
  vt.ready.then(
    () => {
      drawn = true;
      clearTimeout(atOnce);
    },
    () => clearTimeout(atOnce),
  );
  // the turn's own class comes off with the newest turn only: an older one
  // finishing late must not uncover a page still turning. A turn that a newer
  // one skipped finishes as a rejection, which is not a failure here
  vt.finished
    .catch(() => {})
    .finally(() => {
      if (gen === turnSeq) document.documentElement.classList.remove("page-turn");
    });
}

// the page the rail last held; leaving Settings turns back to it
let lastNavView = "sessions";
// the newest page turn asked for; an older turn still waiting to draw sees a
// newer number and gives up, so the page on screen is always the last asked
let turnSeq = 0;

// The gear in the header opens Settings as a page of its own (magpie's
// `.icon#prefs`), under the same header and the same turn as every other
// page, rather than as a sheet over the page it was opened from. A second
// press, with Settings already open, gives the page back.
function toggleSettings() {
  const open = isSettingsOpen();
  if (open) leaveSettings();
  else openSettings();
}

function isSettingsOpen() {
  return !!document
    .getElementById("view-settings")
    ?.classList.contains("active");
}

function openSettings(tab) {
  if (tab) setSetTab(tab, true);
  switchView("settings");
}

// Esc, or a second press of the gear, gives the page back to the reader
function leaveSettings() {
  if (!isSettingsOpen()) return;
  switchView(lastNavView || "sessions");
}

// `#settings/<tab>` is how the tray menu opens a settings part: the runner's
// own window has no other way to be told which page to show. The hash is
// cleared once it is taken, so a reload keeps the reader's saved page.
function applySettingsHash() {
  const m = /^#settings(?:\/([a-z]+))?$/.exec(location.hash || "");
  if (!m) return;
  history.replaceState(null, "", location.pathname + location.search);
  openSettings(SET_TABS.includes(m[1]) ? m[1] : "general");
}

window.addEventListener("hashchange", applySettingsHash);

// manual: the reader asked for this probe, so the button that asked turns
// while it is out. The four-second poll stays quiet — a button that is
// always spinning says nothing about whether anything is happening.
async function refreshDashboard(manual) {
  // a probe can take over a second; never let two snapshots render out of order
  if (refreshing) return false;
  refreshing = true;
  const refreshBtn = document.getElementById("refreshBtn");
  if (manual && refreshBtn) refreshBtn.classList.add("busy");
  if (manual) {
    consecutiveEmptyResponses = 0;
    sessionsRenderedAt = Date.now();
  }
  if ((manual || allSessionsData.length === 0) && lastGoodSessions.length === 0) {
    const container = document.getElementById("sessionsContainer");
    if (container && !container.querySelector(".skeleton-card")) {
      container.innerHTML = renderSkeletonCard(5);
    }
    const badge = document.getElementById("sessionCountBadge");
    if (badge && lastGoodSessions.length === 0) badge.textContent = "正在加载…";
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
    const isReady = dashboardData.ready !== false && dashboardData.status !== "starting";
    const sessionsLoading = dashboardData.sessions_loading === true || !isReady;

    if (sessionsLoading) {
      // Transcript scanning (Claude, Claude Desktop, Grok, etc.) or daemon startup is in progress:
      // never show partial sessions (e.g. Codex-only before Claude finishes scanning)
      if (lastGoodSessions.length === 0) {
        renderSessions([], "loading");
      }
      setTimeout(() => {
        if (!refreshing) refreshDashboard();
      }, 250);
    } else if (sessions.length > 0) {
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

    const capStr = JSON.stringify(dashboardData.capabilities || {});
    if (capStr !== lastRenderState.capabilitiesJson) {
      renderCapabilities(dashboardData.capabilities);
      lastRenderState.capabilitiesJson = capStr;
    }

    const deviceStr = JSON.stringify(dashboardData.device || {});
    if (deviceStr !== lastRenderState.deviceJson) {
      renderDevice(dashboardData.device);
      lastRenderState.deviceJson = deviceStr;
    }
    // the settings page's rows say what this snapshot found; they are only
    // written while the page is the one on screen
    if (isSettingsOpen()) renderSettings();
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
    breakdown.textContent = `Coding Assistant ${stats.claude_sessions || 0} · Claude Desktop ${stats.claude_desktop_sessions || 0} · Codex ${stats.codex_sessions || 0} · Grok ${stats.grok_sessions || 0} · Gemini ${stats.gemini_sessions || 0}`;
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

// The vendor marks are served from /icons/: the files magpie ships
// (internal/gui/assets/icons, MIT) copied verbatim, never redrawn here. A
// model the family table does not name gets a neutral plate rather than a
// made-up logo, which is what made the old inline marks look wrong.
const AGENT_ICONS = {
  claude_code: "claudecode-color",
  claude_desktop: "claude-color",
  claude: "claude-color",
  codex: "codex-color",
  grok_build: "xai",
  grok: "xai",
  gemini_cli: "geminicli-color",
  gemini: "gemini-color",
  antigravity: "antigravity-color",
  zcode: "zcode.png",
};

// magpie's family table (internal/agent/icons.go): a model id's prefix names
// its vendor's mark, first match wins.
const MODEL_FAMILIES = [
  ["claude", "claude-color"],
  ["gpt", "openai"], ["o1", "openai"], ["o3", "openai"], ["o4", "openai"], ["codex", "openai"],
  ["gemini", "gemini-color"], ["gemma", "gemini-color"],
  ["deepseek", "deepseek-color"],
  ["grok", "xai"],
  ["kimi", "kimi"], ["moonshot", "kimi"],
  ["glm", "zhipu-color"], ["chatglm", "chatglm-color"],
  ["qwen", "qwen-color"], ["qwq", "qwen-color"],
  ["mistral", "mistral-color"], ["codestral", "mistral-color"], ["devstral", "mistral-color"], ["magistral", "mistral-color"],
  ["minimax", "minimax-color"],
  ["mimo", "mimocode"], ["xiaomi", "mimocode"],
  ["step", "stepfun-color"],
];

// marks drawn with `currentColor`: as an <img> they paint black, so the dark
// theme inverts them (dashboard.css img.mark.mono)
const MONO_MARKS = new Set(["openai", "grok", "xai", "kimi", "moonshot", "mimocode"]);

function modelIconName(name) {
  let id = String(name || "").toLowerCase().trim();
  // "vendor/model" on a relay: the model's own name decides
  const slash = id.lastIndexOf("/");
  if (slash >= 0) id = id.slice(slash + 1);
  const hit = MODEL_FAMILIES.find(([prefix]) => id.startsWith(prefix));
  return hit ? hit[1] : "";
}

function iconMark(name, size, alt) {
  if (!name) {
    return `<span class="mark none" style="width:${size}px;height:${size}px" aria-hidden="true"></span>`;
  }
  const file = name.endsWith(".png") ? name : `${name}.svg`;
  const mono = MONO_MARKS.has(name.replace(/\.svg$/, "")) ? " mono" : "";
  return `<img class="mark${mono}" src="/icons/${file}" width="${size}" height="${size}" alt="${escapeHtml(alt || "")}" loading="lazy">`;
}

function getAgentIcon(id, size = 18) {
  const key = String(id || "").toLowerCase().trim();
  return iconMark(AGENT_ICONS[key] || "", size);
}

function getModelIcon(name, size = 18) {
  return iconMark(modelIconName(name), size, name);
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
      syncSessionPickUi();
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
    // with no rows there is nothing left to select: a pick kept from the
    // page before would otherwise show a count and a live delete button over
    // an empty page, aimed at sessions that are already in the trash
    pickedSessions.clear();
    syncSessionPickUi();
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
          const rawId =
            item.provider_session_id || item.session_id || item.id || "";
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
          // a Coding Assistant session's subagents are listed under the
          // conversation that spawned them, never as conversations of their
          // own: their lines carry the parent's session id
          const subs = Array.isArray(item.subagents) ? item.subagents : [];
          const deletable = rawProvider === "claude_code" || rawProvider === "claude_desktop" || rawProvider === "claude";
          const picked = pickedSessions.has(rawId);
          const subsClosed = !fullId || !expandedSubagents.has(fullId);
          const subHtml = subs.length ? renderSubagents(subs, subsClosed) : "";
          const subCount = subs.length
            ? `<button type="button" class="tag sub-count sm-toggle-subs" onclick="event.stopPropagation();toggleSubagents(this)" title="${subsClosed ? "展开子代理" : "收起子代理"}" aria-expanded="${!subsClosed}">
                 <span>${subs.length} 个子代理</span>
                 <svg class="sub-chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>
               </button>`
            : "";
          const deleteBtn = deletable
            ? `<button type="button" class="text icon sm-del" title="删除这个会话" aria-label="删除这个会话" onclick="event.stopPropagation();askDeleteSessions(this.closest('.row.sess').dataset.id)">
                 <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/><path d="M10 11v5M14 11v5"/></svg>
               </button>`
            : "";
          const check = deletable
            ? `<input type="checkbox" class="sm-check" ${picked ? "checked" : ""} aria-label="选择这个会话" onclick="event.stopPropagation()" onchange="pickSession(this.closest('.row.sess').dataset.id, this.checked)">`
            : `<span class="sm-check-space"></span>`;

          return `
        <div class="row sess${subs.length ? " has-subs" : ""}${subsClosed ? " subs-closed" : ""}" data-id="${fullId}" style="--i:${index};--n:${grp.items.length}" title="${providerLabel(rawProvider)}${fullId ? " · " + fullId : ""}${modelName ? " · " + modelName : ""}${rawTokens > 0 ? " · " + fmtN(rawTokens) + " Tokens" : ""}"${subs.length ? ` onclick="if(!event.target.closest('.sm-del, .sm-check, button, input, a')) toggleSubagents(this)"` : ""}>
          ${check}
          <div class="who">
            <div class="name" title="${prompt}">${prompt}</div>
            <div class="sub">${providerLabel(rawProvider)}${subCount}${rawTokens > 0 ? ` · ${fmtN(rawTokens)} Tokens` : ""}</div>
          </div>
          ${modelName ? `<span class="model" title="${modelName}">${modelName}</span>` : ""}
          <time class="when">${escapeHtml(timeStr)}</time>
          ${deleteBtn}
        </div>
        ${subHtml}
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
  syncSessionPickUi();
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

// ---- the sessions view: subagents under their conversation, and delete ----

// the sessions the reader ticked; a delete takes every one of them
const pickedSessions = new Set();

// subagents expanded states (subagents are collapsed by default; user-expanded sessions persist across renders and reloads)
const expandedSubagents = new Set(
  (() => {
    try {
      // Clean up legacy inverted key if present
      localStorage.removeItem("agenthub_closed_subagents");
    } catch (e) {}
    try {
      return JSON.parse(
        localStorage.getItem(STORE.expandedSubagents || "agenthub_expanded_subagents") || "[]",
      );
    } catch (e) {
      return [];
    }
  })(),
);

function toggleSubagents(el) {
  const row = el.closest(".row.sess");
  if (!row) return;
  const id = row.getAttribute("data-id") || "";
  const subs = row.nextElementSibling;
  if (!subs || !subs.classList.contains("subs")) return;
  const closing = !subs.classList.contains("folded");
  subs.classList.toggle("folded", closing);
  row.classList.toggle("subs-closed", closing);
  const toggleBtn = row.querySelector(".sm-toggle-subs");
  if (toggleBtn) {
    toggleBtn.setAttribute("aria-expanded", String(!closing));
    toggleBtn.setAttribute("title", closing ? "展开子代理" : "收起子代理");
  }
  if (id) {
    if (closing) expandedSubagents.delete(id);
    else expandedSubagents.add(id);
    try {
      localStorage.setItem(
        STORE.expandedSubagents || "agenthub_expanded_subagents",
        JSON.stringify([...expandedSubagents]),
      );
    } catch (e) {}
  }
}

// one subagent a line, under its conversation: what it was for, which model
// wrote it, and what it spent. A subagent another subagent started is indented
// under that one, the way its meta file says it was spawned.
function renderSubagents(subs, subsClosed = true) {
  const byId = new Map(subs.map((s) => [s.id, s]));
  const order = [];
  const depthOf = (s) => {
    let depth = 0;
    let cur = s;
    const seen = new Set();
    while (cur && cur.parent && byId.has(cur.parent) && !seen.has(cur.parent) && depth < 8) {
      seen.add(cur.parent);
      cur = byId.get(cur.parent);
      depth += 1;
    }
    return depth;
  };
  const sorted = [...subs].sort((a, b) => {
    const da = depthOf(a);
    const db = depthOf(b);
    if (da !== db) return da - db;
    return (b.last_activity_at_ms || 0) - (a.last_activity_at_ms || 0);
  });
  return `<div class="subs${subsClosed ? " folded" : ""}">${sorted
    .map((s) => {
      const depth = Math.min(3, depthOf(s));
      const kind = escapeHtml(s.kind || "子代理");
      const desc = escapeHtml(s.description || "");
      const model = escapeHtml(s.model || "");
      const tokens =
        (Number(s.prompt_tokens) || 0) + (Number(s.completion_tokens) || 0);
      const calls = Number(s.request_count) || 0;
      const facts = [
        model,
        calls ? `${calls} 次请求` : "",
        tokens ? `${fmtN(tokens)} Tokens` : "",
      ]
        .filter(Boolean)
        .join(" · ");
      return `<div class="sub-row" style="--d:${depth}" title="${kind}${desc ? " · " + desc : ""}${facts ? " · " + facts : ""}">
        <span class="sub-kind">${kind}</span>
        <span class="sub-desc">${desc || "（没有留下任务说明）"}</span>
        <span class="sub-facts">${facts}</span>
      </div>`;
    })
    .join("")}</div>`;
}

function pickSession(id, on) {
  if (!id) return;
  if (on) pickedSessions.add(id);
  else pickedSessions.delete(id);
  syncSessionPickUi();
}

function syncSessionPickUi() {
  const count = document.getElementById("sessionPickedCount");
  const del = document.getElementById("sessionDeleteBtn");
  const all = document.getElementById("sessionPickAll");
  const wrap = document.getElementById("sessionPickAllWrap");
  const boxes = [...document.querySelectorAll("#sessionsContainer .sm-check")];
  if (count) {
    count.hidden = pickedSessions.size === 0;
    count.textContent = `${pickedSessions.size} 个已选`;
  }
  if (del) del.disabled = pickedSessions.size === 0;
  if (wrap) wrap.hidden = boxes.length === 0;
  if (all) {
    const n = boxes.filter((b) => b.checked).length;
    all.checked = boxes.length > 0 && n === boxes.length;
    all.indeterminate = n > 0 && n < boxes.length;
  }
}

function togglePickAllSessions(on) {
  document.querySelectorAll("#sessionsContainer .sm-check").forEach((box) => {
    const row = box.closest(".row.sess");
    const id = row && row.dataset.id;
    box.checked = on;
    if (id) {
      if (on) pickedSessions.add(id);
      else pickedSessions.delete(id);
    }
  });
  syncSessionPickUi();
}

function askDeleteSessions(one) {
  const ids = one ? [one] : [...pickedSessions];
  if (!ids.length) return;
  const byId = new Map(
    dedupeSessions(lastGoodSessions).map((s) => [
      s.provider_session_id || s.session_id || s.id || "",
      s,
    ]),
  );
  const list = document.getElementById("deleteSessionsList");
  if (list) {
    list.replaceChildren(
      ...ids.slice(0, 5).map((id) => {
        const s = byId.get(id) || {};
        const li = document.createElement("li");
        li.textContent =
          s.prompt || s.title || s.name || String(id).replace(/^claude_/, "");
        return li;
      }),
    );
    if (ids.length > 5) {
      const li = document.createElement("li");
      li.className = "more";
      li.textContent = `还有 ${ids.length - 5} 个`;
      list.append(li);
    }
  }
  const title = document.getElementById("deleteSessionsTitle");
  if (title) {
    title.textContent =
      ids.length === 1 ? "删除这个会话？" : `删除这 ${ids.length} 个会话？`;
  }
  const note = document.getElementById("deleteSessionsNote");
  if (note) {
    note.textContent =
      "会话的对话文件、子代理记录与文件历史会移入 Runner 的回收目录，不会直接抹掉；Usage 页的用量统计保留。最近一分钟内还在写入的会话会被跳过。";
  }
  const modal = document.getElementById("deleteSessionsModal");
  if (modal) modal.classList.add("open");
  pendingDeleteSessions = ids;
}

let pendingDeleteSessions = [];

function closeDeleteSessions() {
  const modal = document.getElementById("deleteSessionsModal");
  if (modal) modal.classList.remove("open");
  pendingDeleteSessions = [];
}

async function confirmDeleteSessions() {
  const ids = pendingDeleteSessions.slice();
  if (!ids.length) return;
  const btn = document.getElementById("btnDeleteSessions");
  if (btn) {
    btn.disabled = true;
    btn.classList.add("busy");
  }
  try {
    const resp = await fetch("/api/sessions/delete", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ ids }),
    });
    if (!resp.ok) {
      // nothing was done, so nothing the reader ticked is spent: leave the
      // picks and the confirm dialog as they were, and say what happened
      toast(`删除失败：服务返回 HTTP ${resp.status}`, "err");
      return;
    }
    const data = (await resp.json()) || {};
    // a refused session stays picked, so the reader can retry it
    const kept = new Set(
      (data.refused || []).map((r) => String(r.id || "").replace(/^claude_/, "")),
    );
    for (const id of [...pickedSessions]) if (!kept.has(id)) pickedSessions.delete(id);
    closeDeleteSessions();
    if ((data.deleted || []).length) {
      toast(
        data.deleted.length === 1
          ? "会话已移入回收目录"
          : `${data.deleted.length} 个会话已移入回收目录`,
      );
    }
    if ((data.refused || []).length) {
      const first = data.refused[0];
      toast(
        `有 ${data.refused.length} 个会话没有删除：${first.error || "正在使用中"}`,
        "err",
      );
    }
    syncSessionPickUi();
    refreshDashboard(true);
  } catch (e) {
    // a network failure deleted nothing either: the picks survive the retry
    toast("删除失败：" + e.message, "err");
  } finally {
    if (btn) {
      btn.disabled = pickedSessions.size === 0;
      btn.classList.remove("busy");
    }
  }
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


function copyValue(elementId, label) {
  const el = document.getElementById(elementId);
  if (el && el.textContent && el.textContent !== "-") {
    copyText(el.textContent);
    toast(`已复制 ${label}`);
  }
}

// ---- capabilities view: unified multi-agent library (skills, mcp, hooks) ----
let currentCapTab = "all";
let currentCapSearch = "";
let cachedCapabilities = null;
let currentMcpTransport = "stdio";

function renderCapabilities(caps) {
  cachedCapabilities = caps || { skills: [], mcp_servers: [], hooks: [] };
  const skills = cachedCapabilities.skills || [];
  const mcpServers = cachedCapabilities.mcp_servers || [];
  const hooks = cachedCapabilities.hooks || [];

  // the tab's own line says what the scan found, in one sentence
  const summary = document.getElementById("capSummarySub");
  if (summary) {
    summary.textContent = `${skills.length} 个技能 · ${mcpServers.length} 个 MCP 服务 · ${hooks.length} 个生命周期钩子，从本机各智能体的配置里读出`;
  }

  // Subtab count badges
  const totalCount = skills.length + mcpServers.length + hooks.length;
  const countAll = document.getElementById("capCountAll");
  const countSkills = document.getElementById("capCountSkills");
  const countMcp = document.getElementById("capCountMcp");
  const countHooks = document.getElementById("capCountHooks");

  if (countAll) countAll.textContent = totalCount;
  if (countSkills) countSkills.textContent = skills.length;
  if (countMcp) countMcp.textContent = mcpServers.length;
  if (countHooks) countHooks.textContent = hooks.length;

  applyCapabilitiesFilter();
}

function switchCapTab(tab) {
  currentCapTab = tab;
  document.querySelectorAll("#capSubTabs .seg-btn").forEach((btn) => {
    btn.classList.toggle("on", btn.dataset.tab === tab);
  });
  applyCapabilitiesFilter();
}

function filterCapabilities() {
  const inp = document.getElementById("capSearchInput");
  currentCapSearch = (inp ? inp.value : "").trim().toLowerCase();
  applyCapabilitiesFilter();
}

function applyCapabilitiesFilter() {
  if (!cachedCapabilities) return;
  const skills = cachedCapabilities.skills || [];
  const mcpServers = cachedCapabilities.mcp_servers || [];
  const hooks = cachedCapabilities.hooks || [];

  const q = currentCapSearch;

  // Filter skills
  const matchedSkills = skills.filter((s) => {
    if (!q) return true;
    return (
      (s.name || "").toLowerCase().includes(q) ||
      (s.description || "").toLowerCase().includes(q) ||
      (s.source || "").toLowerCase().includes(q) ||
      (s.path || "").toLowerCase().includes(q) ||
      (s.agents || []).join(" ").toLowerCase().includes(q)
    );
  });

  // Filter MCP
  const matchedMcp = mcpServers.filter((m) => {
    if (!q) return true;
    return (
      (m.name || "").toLowerCase().includes(q) ||
      (m.command || "").toLowerCase().includes(q) ||
      (m.args || []).join(" ").toLowerCase().includes(q) ||
      (m.url || "").toLowerCase().includes(q) ||
      (m.source || "").toLowerCase().includes(q) ||
      (m.transport || "").toLowerCase().includes(q) ||
      (m.agents || []).join(" ").toLowerCase().includes(q)
    );
  });

  // Filter Hooks
  const matchedHooks = hooks.filter((h) => {
    if (!q) return true;
    return (
      (h.name || "").toLowerCase().includes(q) ||
      (h.event || "").toLowerCase().includes(q) ||
      (h.event_display || "").toLowerCase().includes(q) ||
      (h.matcher || "").toLowerCase().includes(q) ||
      (h.command || "").toLowerCase().includes(q) ||
      (h.source || "").toLowerCase().includes(q) ||
      (h.agents || []).join(" ").toLowerCase().includes(q)
    );
  });

  const skillsSection = document.getElementById("capSkillsSection");
  const mcpSection = document.getElementById("capMcpSection");
  const hooksSection = document.getElementById("capHooksSection");

  const skillsGrid = document.getElementById("capSkillsGrid");
  const mcpGrid = document.getElementById("capMcpGrid");
  const hooksGrid = document.getElementById("capHooksGrid");

  const skillsBadge = document.getElementById("capSkillsSectionBadge");
  const mcpBadge = document.getElementById("capMcpSectionBadge");
  const hooksBadge = document.getElementById("capHooksSectionBadge");

  const showSkills = currentCapTab === "all" || currentCapTab === "skills";
  const showMcp = currentCapTab === "all" || currentCapTab === "mcp";
  const showHooks = currentCapTab === "all" || currentCapTab === "hooks";

  if (skillsSection) skillsSection.style.display = showSkills ? "" : "none";
  if (mcpSection) mcpSection.style.display = showMcp ? "" : "none";
  if (hooksSection) hooksSection.style.display = showHooks ? "" : "none";

  if (skillsBadge) skillsBadge.textContent = `${matchedSkills.length} 个技能`;
  if (mcpBadge) mcpBadge.textContent = `${matchedMcp.length} 个服务`;
  if (hooksBadge) hooksBadge.textContent = `${matchedHooks.length} 个钩子`;

  // Render skills
  if (skillsGrid && showSkills) {
    if (matchedSkills.length === 0) {
      skillsGrid.innerHTML = `<div class="empty-state"><b>没有找到匹配的扩展技能</b><span class="why">检查检索关键词，或在 ~/.claude/skills 与工作区放入 SKILL.md</span></div>`;
    } else {
      skillsGrid.innerHTML = matchedSkills.map(renderSkillCard).join("");
    }
  }

  // Render MCP
  if (mcpGrid && showMcp) {
    if (matchedMcp.length === 0) {
      mcpGrid.innerHTML = `<div class="empty-state"><b>没有找到匹配的 MCP 协议服务器</b><span class="why">可点击右上角“注册 MCP 服务”添加自定义协议进程</span></div>`;
    } else {
      mcpGrid.innerHTML = matchedMcp.map(renderMcpCard).join("");
    }
  }

  // Render Hooks
  if (hooksGrid && showHooks) {
    if (matchedHooks.length === 0) {
      hooksGrid.innerHTML = `<div class="empty-state"><b>没有找到匹配的生命周期钩子</b><span class="why">在 Claude Code 或 Codex 配置 PreToolUse、SessionStart 等钩子后自动纳管</span></div>`;
    } else {
      hooksGrid.innerHTML = matchedHooks.map(renderHookCard).join("");
    }
  }
}

// One capability a row, in the console's own idiom: a title, one quiet line
// of facts, and a value column. No icon chips, no coloured event badges, no
// lifted cards: the capability page is a list of what this machine can do,
// read the same way as a session or a setting. A long path or command is cut
// with an ellipsis rather than wrapped, so no row grows past its card.
function capAgentMarks(agents, fallback) {
  const list = (agents || []).filter(Boolean);
  if (!list.length) return `<span class="cap-agents-none">${fallback}</span>`;
  return list
    .map(
      (a) =>
        `<span class="cap-agent">${getAgentIcon(a, 13)}<span>${escapeHtml(providerLabel(a))}</span></span>`,
    )
    .join("");
}

function capRow({ name, kind, value, facts, agents, meta, copyLabel, title }) {
  const safeValue = escapeHtml(value || "—");
  return `
    <div class="row cap-row" title="${escapeHtml(title || "")}">
      <div class="who">
        <div class="name-line">
          <span class="name">${escapeHtml(name || "—")}</span>
          ${kind ? `<span class="tag">${escapeHtml(kind)}</span>` : ""}
        </div>
        <div class="sub flow">${facts.filter(Boolean).join(" · ")}</div>
        <button type="button" class="cap-copy mono" onclick="copyText(this.dataset.v); toast('${copyLabel}')" data-v="${escapeHtml(value || "")}" title="${escapeHtml(value || "")}">
          <span class="cap-copy-text">${safeValue}</span>
        </button>
      </div>
      <div class="cap-side">
        <div class="cap-agents">${capAgentMarks(agents, "全局")}</div>
        ${meta ? `<span class="cap-meta">${meta}</span>` : ""}
      </div>
    </div>
  `;
}

function renderSkillCard(skill) {
  return capRow({
    name: skill.name,
    kind: skill.source,
    value: skill.path,
    copyLabel: "已复制技能文件路径",
    title: skill.description || "",
    facts: [
      escapeHtml(skill.description || "暂无描述说明"),
      skill.has_scripts ? "含可执行脚本" : "",
    ],
    agents: skill.agents,
    meta: skill.enabled ? "已启用" : "已停用",
  });
}

function renderMcpCard(mcp) {
  const transport = (mcp.transport || "stdio").toLowerCase();
  const cmdOrUrl =
    transport === "stdio"
      ? `${mcp.command || ""} ${(mcp.args || []).join(" ")}`.trim()
      : mcp.url || "";
  return capRow({
    name: mcp.name,
    kind: transport,
    value: cmdOrUrl,
    copyLabel: "已复制 MCP 执行参数",
    title: cmdOrUrl,
    facts: [escapeHtml(mcp.source || "")],
    agents: mcp.agents,
    meta:
      mcp.status === "active"
        ? `<span class="cap-live"><i></i>活跃运行</span>`
        : "已配置",
  });
}

function renderHookCard(hook) {
  const facts = [escapeHtml(hook.event_display || hook.event || "")];
  if (hook.matcher) facts.push(`拦截 ${escapeHtml(hook.matcher)}`);
  return capRow({
    name: hook.name,
    kind: hook.event || "",
    value: hook.command,
    copyLabel: "已复制 Hook 执行命令",
    title: hook.event_display || hook.event || "",
    facts,
    agents: hook.agents,
    meta: `${hook.timeout_sec || 10}s`,
  });
}

async function syncCapabilities(event) {
  const btn = document.getElementById("btnSyncCapabilities");
  if (btn) {
    btn.disabled = true;
    btn.classList.add("busy");
  }
  try {
    const resp = await fetch("/api/capabilities/sync", { method: "POST" });
    if (resp.ok) {
      const data = await resp.json();
      if (data.capabilities) {
        cachedCapabilities = data.capabilities;
        renderCapabilities(data.capabilities);
      }
      toast("已完成智能体扩展生态 (Skills/MCP/Hooks) 探测与统一对齐");
    } else {
      toast(`同步失败 (HTTP ${resp.status})`, "err");
    }
  } catch (err) {
    toast("同步能力生态失败: " + err.message, "err");
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.classList.remove("busy");
    }
  }
}

function openRegisterMcpModal() {
  const modal = document.getElementById("registerMcpModal");
  if (!modal) return;
  document.getElementById("mcpFormName").value = "";
  document.getElementById("mcpFormCommand").value = "";
  document.getElementById("mcpFormArgs").value = "";
  document.getElementById("mcpFormUrl").value = "";
  document.getElementById("mcpFormStatus").textContent = "";
  selectMcpTransport("stdio");
  modal.classList.add("open");
}

function closeRegisterMcpModal() {
  const modal = document.getElementById("registerMcpModal");
  if (modal) modal.classList.remove("open");
}

function selectMcpTransport(transport) {
  currentMcpTransport = transport;
  const segs = document.getElementById("mcpTransportSegs");
  if (segs) {
    segs.querySelectorAll(".seg-btn").forEach((btn) => {
      btn.classList.toggle("on", btn.dataset.transport === transport);
    });
  }
  const stdioFields = document.getElementById("mcpStdioFields");
  const httpFields = document.getElementById("mcpHttpFields");
  if (stdioFields)
    stdioFields.style.display = transport === "stdio" ? "contents" : "none";
  if (httpFields)
    httpFields.style.display = transport === "stdio" ? "none" : "contents";
}

async function submitRegisterMcp(event) {
  event.preventDefault();
  const name = document.getElementById("mcpFormName").value.trim();
  const cmd = document.getElementById("mcpFormCommand").value.trim();
  const argsRaw = document.getElementById("mcpFormArgs").value.trim();
  const url = document.getElementById("mcpFormUrl").value.trim();
  const submitBtn = document.getElementById("btnSubmitMcp");

  if (!name) {
    toast("请输入服务标识名称", "err");
    return;
  }

  const isStdio = currentMcpTransport === "stdio";
  if (isStdio && !cmd) {
    toast("本地 Stdio 进程必须填写启动命令", "err");
    return;
  }
  if (!isStdio && !url) {
    toast("远程端点必须填写有效的服务 URL", "err");
    return;
  }

  const args = argsRaw ? argsRaw.split(/\s+/).filter(Boolean) : [];
  const payload = {
    name,
    command: isStdio ? cmd : null,
    args: isStdio ? args : [],
    url: isStdio ? null : url,
    headers: {},
  };

  const origHtml = submitBtn.innerHTML;
  submitBtn.disabled = true;
  submitBtn.innerHTML = "<span>保存中…</span>";

  try {
    const resp = await fetch("/api/capabilities/mcp/register", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    const resData = await resp.json();
    if (resp.ok && resData.ok) {
      closeRegisterMcpModal();
      toast(`已成功注册 MCP 服务 ${name}`);
      await refreshDashboard(true);
    } else {
      toast(`注册 MCP 失败: ${resData.error || "未知错误"}`, "err");
    }
  } catch (err) {
    toast(`注册 MCP 异常: ${err.message}`, "err");
  } finally {
    submitBtn.disabled = false;
    submitBtn.innerHTML = origHtml;
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
  const pairTag = document.getElementById("pairStateTag");

  if (deviceIdElem) deviceIdElem.textContent = device.device_id || "-";
  const deviceNameInput = document.getElementById("deviceNameInput");
  if (deviceNameInput && document.activeElement !== deviceNameInput) {
    deviceNameInput.value = device.device_name || "";
  }
  if (deviceKeyElem) {
    const pk = device.public_key || "";
    deviceKeyElem.textContent =
      pk.length > 20
        ? pk.substring(0, 10) + "..." + pk.substring(pk.length - 8)
        : pk;
    deviceKeyElem.title = pk;
  }

  if (device.paired) {
    const gw = device.gateway_url
      ? device.gateway_url.replace(/^https?:\/\//, "")
      : "已配对";
    const isConnected = !!device.connected;

    if (pairBtn) {
      pairBtn.classList.remove("off");
      pairBtn.classList.toggle("connected", isConnected);
      pairBtn.classList.toggle("disconnected", !isConnected);
      pairBtn.title = isConnected
        ? `Web 控制台已连接 (${gw})`
        : `Web 控制台未连接 (${gw})，请检查网络或服务端服务`;
    }

    if (isConnected) {
      if (pairText) pairText.textContent = `已连接 ${gw}`;
      if (pairTag) {
        pairTag.textContent = "已连接";
        pairTag.className = "tag ok";
      }
      if (statStatus) {
        statStatus.textContent = "已连接";
        statStatus.style.color = "var(--green)";
      }
      if (statSub) statSub.textContent = gw;
    } else {
      if (pairText) pairText.textContent = `未连接 (${gw})`;
      if (pairTag) {
        pairTag.textContent = "未连接";
        pairTag.className = "tag warn";
      }
      if (statStatus) {
        statStatus.textContent = "未连接";
        statStatus.style.color = "var(--red)";
      }
      if (statSub) statSub.textContent = `${gw} (未建立连接或离线)`;
    }

    setPairTile(false);
    if (pairedBox) {
      pairedBox.style.display = "block";
      if (pairedInfo) {
        pairedInfo.textContent = `网关: ${device.gateway_url || "-"} · 状态: ${isConnected ? "在线连接" : "离线未连"} · 配对时间: ${device.paired_at || "-"}`;
      }
    }
  } else {
    if (pairBtn) {
      pairBtn.classList.remove("connected", "disconnected");
      pairBtn.classList.add("off");
      pairBtn.title = "点击绑定 Web 控制台";
    }
    if (pairText) pairText.textContent = "未绑定 Web 控制台";
    if (pairTag) {
      pairTag.textContent = "未绑定";
      pairTag.className = "tag";
    }
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
const LED_STATUSES = { "": "全部", failed: "失败", retried: "重试后成功" };

let usageTab = "overview";
let usagePeriod = "7d";
let ledMetric = "tokens";
let ledSplit = "model";
let ledHeatMetric = "tokens";
let ledStatus = "";
let ledQuery = "";
let usageDigest = null;
let usageLoading = false;
let usageFetchingPeriod = "";
let usageAbortController = null;
const usageMemoryCache = new Map();
let usageSeq = 0;
let usageTimer = 0;
let usageEvery = 30000;
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
  const every = parseInt(localStorage.getItem(STORE.usageEvery) || "", 10);
  if ([0, 10000, 30000, 60000].includes(every)) usageEvery = every;
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
  const every = document.getElementById("usageEvery");
  if (every)
    every.addEventListener("click", (e) => {
      const b = e.target.closest("button[data-ms]");
      if (b) setUsageEvery(Number(b.dataset.ms));
    });
  const status = document.getElementById("ledStatus");
  if (status)
    status.addEventListener("click", (e) => {
      const b = e.target.closest("button[data-status]");
      if (!b) return;
      ledStatus = b.dataset.status || "";
      pickUsageSegs();
      renderLedgerRequests();
    });
  const q = document.getElementById("ledQ");
  if (q)
    q.addEventListener("input", () => {
      ledQuery = q.value.trim().toLowerCase();
      renderLedgerRequests();
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
  // the period steers every tab but the request list's own status, which the
  // list keeps; drawing again reads the digest already in hand
  if (usageDigest) drawUsage();
}

function setUsagePeriod(period) {
  if (!["today", "7d", "30d", "all"].includes(period)) return;
  usagePeriod = period;
  remember("usagePeriod", period);
  pickUsageSegs();

  const cached = usageMemoryCache.get(period);
  if (cached && cached.data) {
    // SWR hit: instant switch with zero flicker
    usageDigest = cached.data;
    drawUsage();
    // Background revalidate if older than 15s
    if (Date.now() - cached.ts > 15000) {
      loadUsage(false);
    }
  } else {
    // SWR miss: show rich skeleton animations
    usageDigest = null;
    drawUsageLoading();
    loadUsage(true);
  }
}

function setUsageEvery(ms) {
  usageEvery = ms;
  try {
    localStorage.setItem(STORE.usageEvery, String(ms));
  } catch (e) {}
  pickUsageSegs();
  startUsageTimer();
  toast(ms ? `用量每 ${Math.round(ms / 1000)} 秒自动刷新` : "用量改为手动刷新");
}

// the cadence is the reader's: the console's own rule is never under half a
// second, and the ledger's answer is cached for five, so ten is the floor here
function startUsageTimer() {
  clearInterval(usageTimer);
  if (!usageEvery) return;
  usageTimer = setInterval(() => {
    if (isUsageOpen() && !document.hidden) loadUsage();
  }, Math.max(10000, usageEvery));
}

function pickUsageSegs() {
  document.querySelectorAll("#usagePeriods button[data-period]").forEach((b) => {
    b.classList.toggle("on", b.dataset.period === usagePeriod);
  });
  document.querySelectorAll("#usageEvery button[data-ms]").forEach((b) => {
    b.classList.toggle("on", Number(b.dataset.ms) === usageEvery);
  });
  document.querySelectorAll("#ledStatus button[data-status]").forEach((b) => {
    b.classList.toggle("on", (b.dataset.status || "") === ledStatus);
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
  if (usageLoading && usageFetchingPeriod === periodToFetch) return;

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
  if (btn) btn.classList.add("busy");

  if (!usageDigest) {
    drawUsageLoading();
  }

  try {
    const resp = await fetch(
      `/api/usage/ledger?period=${encodeURIComponent(periodToFetch)}&limit=1000`,
      { cache: "no-store", signal: usageAbortController.signal },
    );
    if (!resp.ok) {
      toast(`读取用量失败（HTTP ${resp.status}）`, "err");
      return;
    }
    const data = await resp.json();
    if (seq !== usageSeq) return;

    usageMemoryCache.set(periodToFetch, { data, ts: Date.now() });

    if (periodToFetch === usagePeriod) {
      usageDigest = data;
      drawUsage();
    }
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
  if (wrap) wrap.innerHTML = `<div class="led-none">正在读取用量…</div>`;
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
  drawUsageKpis(d);
  // the overview's charts are measured from their card: while that tab is
  // not the one in view, there is no width to draw in
  if (!document.getElementById("usagePaneOverview").hidden) {
    drawLedgerTrend();
    drawLedgerHeat();
  }
  renderLedgerRequests();
  renderLedgerSessions();
  renderLedgerContext();
  const note = document.getElementById("usageNote");
  if (note) {
    note.textContent = `统计范围：Coding Assistant 与 Claude Desktop 的本地会话记录（${d.from} 至 ${d.to}）。Codex、Grok、Gemini 的会话只有整段合计，不在这里逐条列出。`;
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
// ranking's colours (tokens, cost and calls add up; a speed does not, so each
// model is a mark at its own speed on the bucket's track, as magpie draws
// #860). The ranking beside it is the legend and the other reading.
function drawLedgerTrend() {
  const box = document.getElementById("ledChart");
  if (!box || !usageDigest) return;
  const d = usageDigest;
  const bucket = ledBucketName(d);
  box.replaceChildren();
  const plot = document.createElement("div");
  plot.className = "plot";
  box.append(plot);
  const W = plot.clientWidth;
  const H = plot.clientHeight || 236;
  const pts = d.series || [];
  if (W < 100 || !pts.length) {
    const none = document.createElement("div");
    none.className = "none";
    none.textContent = "本周期没有请求";
    plot.append(none);
    drawLedgerRank(d, bucket);
    return;
  }
  const { top } = ledCategories(ledSplit, ledMetric);
  const totals = pts.map((p) => ledValue(ledMetric, p));
  const speed = ledMetric === "speed";
  const partOf = (p, id) => (p.by && p.by[ledSplit] && p.by[ledSplit][id]) || {};
  const max = Math.max(
    0,
    ...totals,
    ...(speed ? pts.flatMap((p) => top.map((c) => ledFloor(partOf(p, c.id)))) : []),
  );
  if (!(max > 0)) {
    const none = document.createElement("div");
    none.className = "none";
    none.textContent =
      ledMetric === "cost"
        ? "本周期没有已知价格的请求"
        : speed
          ? "本周期没有可整轮计时的回复"
          : "本周期没有请求";
    plot.append(none);
    drawLedgerRank(d, bucket);
    return;
  }
  const topV = ledAxis(max);
  const g = sv("svg", { viewBox: `0 0 ${W} ${H}`, role: "img" });
  g.setAttribute("aria-label", "用量趋势");
  plot.append(g);
  const labels = [0, 1, 2, 3, 4].map((i) => {
    const lab = sv("text", { "text-anchor": "end", class: "axis" });
    lab.textContent = ledAxisFormat(ledMetric, topV * (1 - i / 4));
    g.append(lab);
    return lab;
  });
  let widest = 0;
  labels.forEach((lab) => {
    try {
      widest = Math.max(widest, lab.getComputedTextLength());
    } catch (e) {}
  });
  const M = { l: widest > 0 ? Math.ceil(widest) + 8 : 48, r: 8, t: 10, b: 24 };
  const pw = W - M.l - M.r;
  const ph = H - M.t - M.b;
  const n = pts.length;
  const slot = pw / n;
  const bw = Math.max(2, Math.min(30, slot * 0.68));
  const Y = (v) => M.t + ph - (ph * v) / topV;
  labels.forEach((lab, i) => {
    const y = M.t + (ph * i) / 4;
    lab.setAttribute("x", M.l - 6);
    lab.setAttribute("y", y + 3.5);
    g.insertBefore(
      sv("line", { x1: M.l, x2: W - M.r, y1: y, y2: y, class: "grid" }),
      lab,
    );
  });
  const room = Math.max(2, Math.floor(pw / 62));
  const every = Math.ceil(n / room);
  pts.forEach((p, i) => {
    if (i % every) return;
    const x = sv("text", {
      x: M.l + slot * (i + 0.5),
      y: H - 6,
      "text-anchor": "middle",
      class: "axis",
    });
    x.textContent = ledTick(p.t, bucket);
    g.append(x);
  });
  const hot = sv(
    "rect",
    { class: "hot", y: M.t, height: ph, rx: 3, width: slot },
    { display: "none" },
  );
  g.append(hot);

  const segs = [];
  pts.forEach((p, i) => {
    const x = M.l + slot * (i + 0.5) - bw / 2;
    let y0 = 0;
    const draw = (key, color, v) => {
      if (!(v > 0)) return;
      const r = sv(
        "rect",
        {
          x,
          width: bw,
          y: Y(y0 + v),
          height: Math.max(0.5, Y(y0) - Y(y0 + v)),
          rx: 1,
          class: "col",
          "data-k": key,
          "data-color": color,
        },
        { fill: color },
      );
      g.append(r);
      segs.push(r);
      y0 += v;
    };
    if (speed) {
      // the bucket's own speed as a track, each model a mark at its own
      if (totals[i] > 0) {
        draw("\0all", "var(--pill)", totals[i]);
        segs[segs.length - 1].classList.add("all");
      }
      for (const c of top) {
        const v = ledFloor(partOf(p, c.id));
        if (!(v > 0)) continue;
        if (v > totals[i]) {
          const topY = Y(totals[i]);
          const stem = sv(
            "line",
            {
              x1: x + bw / 2,
              x2: x + bw / 2,
              y1: topY,
              y2: Math.min(Y(v) + 1.5, topY),
              class: "col stem",
              "data-k": c.id,
              "data-color": c.color,
            },
            { stroke: c.color, strokeWidth: 1, strokeOpacity: ".45" },
          );
          g.append(stem);
          segs.push(stem);
        }
        const r = sv(
          "rect",
          {
            x,
            width: bw,
            y: Y(v) - 1.5,
            height: 3,
            rx: 1.5,
            class: "col mark",
            "data-k": c.id,
            "data-color": c.color,
          },
          { fill: c.color },
        );
        g.append(r);
        segs.push(r);
      }
      return;
    }
    let stacked = 0;
    for (const c of top) {
      const v = ledPartValue(ledMetric, partOf(p, c.id));
      stacked += v;
      draw(c.id, c.color, v);
    }
    draw("\0other", "var(--faint)", Math.max(0, totals[i] - stacked));
  });

  const tip = document.createElement("div");
  tip.className = "tip";
  tip.hidden = true;
  plot.append(tip);
  const nameOf = (c) => ledName(c, ledSplit);
  const at = (e) => {
    const r = g.getBoundingClientRect();
    const i = Math.max(
      0,
      Math.min(
        n - 1,
        Math.floor((((e.clientX - r.left) / r.width) * W - M.l) / slot),
      ),
    );
    const p = pts[i];
    hot.setAttribute("x", M.l + slot * i);
    hot.style.display = "";
    tip.replaceChildren();
    const head = document.createElement("b");
    head.textContent = ledWhen(p.t, bucket);
    tip.append(head);
    const rows = top.map((c) => {
      const part = partOf(p, c.id);
      return [nameOf(c), c.color, ledPartValue(ledMetric, part)];
    });
    if (!speed) {
      rows.push([
        "其他",
        "var(--faint)",
        Math.max(0, totals[i] - rows.reduce((a, x) => a + x[2], 0)),
      ]);
    }
    for (const [nm, color, v] of rows.filter((x) => x[2] > 0).sort((a, b) => b[2] - a[2])) {
      const row = document.createElement("div");
      const sw = document.createElement("i");
      sw.style.background = color;
      const label = document.createElement("span");
      label.textContent = nm;
      const val = document.createElement("em");
      val.textContent = ledFormat(ledMetric, v);
      row.append(sw, label, val);
      tip.append(row);
    }
    const sum = document.createElement("div");
    sum.className = "sum";
    const a = document.createElement("span");
    a.textContent = speed ? "本时段" : "总计";
    const b = document.createElement("em");
    b.textContent = ledFormat(ledMetric, totals[i]);
    if (ledMetric !== "calls" && p.calls)
      b.textContent += ` · ${p.calls.toLocaleString()} 次请求`;
    sum.append(a, b);
    tip.append(sum);
    tip.hidden = false;
    const x = M.l + slot * (i + 0.5);
    const w = tip.offsetWidth;
    tip.style.left = Math.max(0, x + 14 + w > W ? x - w - 14 : x + 14) + "px";
  };
  g.onpointermove = at;
  g.onpointerdown = at;
  g.onpointerleave = () => {
    hot.style.display = "none";
    tip.hidden = true;
  };
  box.emphasize = (key) => {
    for (const r of segs) {
      r.style.opacity = key != null && r.dataset.k !== key ? ".22" : "";
    }
  };
  drawLedgerRank(d, bucket);
}

// the ranking beside the chart: who the requests were of, by the metric, the
// most first, each with its share, its figures and a bar in its colour.
function drawLedgerRank(d, bucket) {
  const box = document.getElementById("ledRank");
  if (!box) return;
  const kept = box.scrollTop;
  box.replaceChildren();
  const { top, rest, list } = ledCategories(ledSplit, ledMetric);
  if (!list.length) return;
  const total = list.reduce((a, x) => a + ledValue(ledMetric, x), 0) || 1;
  const leader = Math.max(1, ledRankValue(ledMetric, top[0]));
  const one = (x, color, plain) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "rk" + (plain ? " plain" : "");
    const v = ledRankValue(ledMetric, x);
    const gs = ledGroupSpeed(x);
    const head = document.createElement("div");
    head.className = "rk-a";
    const sw = document.createElement("i");
    sw.className = "rk-sw";
    sw.style.background = color;
    head.append(sw);
    const name = document.createElement("span");
    name.className = "rk-nm";
    name.textContent = ledName(x, ledSplit);
    name.title = name.textContent;
    const val = document.createElement("span");
    val.className = "rk-val";
    val.textContent = ledMetric === "speed" ? (gs ? gs.main : "—") : ledFormat(ledMetric, v);
    if (ledMetric === "speed" && gs) val.title = gs.sub;
    head.append(name, val);
    const bar = document.createElement("div");
    bar.className = "rk-bar";
    const fill = document.createElement("i");
    fill.style.width = Math.max(1.5, (100 * v) / leader).toFixed(1) + "%";
    fill.style.background = color;
    bar.append(fill);
    const more = document.createElement("div");
    more.className = "rk-b";
    const facts = [];
    if (ledMetric !== "calls") facts.push(`${(x.calls || 0).toLocaleString()} 次请求`);
    if (ledMetric !== "tokens") facts.push(`${fmtN(x.tokens || 0)} Token`);
    if (ledMetric !== "cost") facts.push(fmtMoney(x.cost));
    if (x.errors) facts.push(`${x.errors} 次失败`);
    // a speed is no one's share of all the tokens
    if (ledMetric !== "speed") facts.push(`${Math.round((100 * v) / total)}%`);
    more.textContent = facts.join(" · ");
    b.append(head, bar, more);
    if (!plain) {
      b.onpointerenter = () => box.parentElement?.querySelector(".led-chart")?.emphasize?.(x.id);
      b.onpointerleave = () => box.parentElement?.querySelector(".led-chart")?.emphasize?.(null);
    }
    return b;
  };
  for (const x of top) box.append(one(x, x.color));
  if (rest.length) {
    const other = rest.reduce(
      (acc, x) => {
        for (const k of [
          "calls", "tokens", "cost", "errors", "output",
          "floor_out", "floor_ms", "floor_n",
        ])
          acc[k] = (acc[k] || 0) + (x[k] || 0);
        return acc;
      },
      { id: "\0other", name: `其他 (${rest.length})` },
    );
    box.append(one(other, "var(--faint)", true));
  }
  box.scrollTop = kept;
}

// ---- the heatmap: 53 weeks of days, Monday on top ------------------------
function heatDate(s) {
  const [y, m, d] = String(s).split("-").map(Number);
  return new Date(y, m - 1, d);
}

function heatISO(d) {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

// the five levels, from the distribution of the days that had anything: a
// small store reads by its own scale rather than by an absolute one
function heatLevels(values) {
  const nz = values.filter((v) => v > 0).sort((a, b) => a - b);
  if (!nz.length) return () => 0;
  if (nz.length < 8) {
    const most = nz[nz.length - 1];
    return (v) => (v <= 0 ? 0 : Math.min(4, Math.max(1, Math.ceil((4 * v) / most - 1e-9))));
  }
  const q = (p) => nz[Math.min(nz.length - 1, Math.floor(p * nz.length))];
  const [a, b, c, top] = [q(0.25), q(0.5), q(0.75), nz[nz.length - 1]];
  return (v) => (v <= 0 ? 0 : v >= top ? 4 : v <= a ? 1 : v <= b ? 2 : v <= c ? 3 : 4);
}

function drawLedgerHeat() {
  const box = document.getElementById("ledHeat");
  const scroll = document.getElementById("ledHeatScroll");
  if (!box || !scroll || !usageDigest) return;
  const d = usageDigest;
  if (!d.heat_from || !d.heat_to) {
    box.hidden = true;
    return;
  }
  box.hidden = false;
  const at = new Map((d.heat || []).map((x) => [x.date, x]));
  const first = heatDate(d.heat_from);
  const last = heatDate(d.heat_to);
  const days = [];
  for (let cur = new Date(first); cur <= last; cur.setDate(cur.getDate() + 1)) {
    const x = at.get(heatISO(cur)) || { calls: 0, tokens: 0, cost: 0 };
    days.push({ ...x, day: new Date(cur) });
  }
  const value = (x) =>
    ledHeatMetric === "cost" ? x.cost : ledHeatMetric === "calls" ? x.calls : x.tokens;
  const level = heatLevels(days.map(value));
  const shape = [d.heat_from, d.heat_to].join("|");
  let grid = scroll.querySelector(".heat-grid");
  if (!grid || grid.dataset.shape !== shape) {
    grid = document.createElement("div");
    grid.className = "heat-grid";
    grid.dataset.shape = shape;
    grid.setAttribute("role", "img");
    const names = ["一", "二", "三", "四", "五", "六", "日"];
    for (let i = 0; i < 7; i++) {
      const l = document.createElement("span");
      l.className = "wd";
      l.textContent = i % 2 ? "" : names[i];
      l.style.gridArea = `${i + 2} / 1`;
      grid.append(l);
    }
    const weeks = Math.ceil(days.length / 7);
    const labels = [];
    let month = -1;
    days.forEach((x, i) => {
      const w = Math.floor(i / 7);
      const wd = i % 7;
      if (wd === 0 && x.day.getMonth() !== month) {
        month = x.day.getMonth();
        if (
          (!labels.length || w - labels[labels.length - 1].w >= 3) &&
          !(w === 0 && x.day.getDate() > 14) &&
          w < weeks - 1
        ) {
          const m = document.createElement("span");
          m.className = "mo";
          m.textContent = `${x.day.getMonth() + 1}月`;
          labels.push({ m, w });
          grid.append(m);
        }
      }
      const c = document.createElement("i");
      c.dataset.date = heatISO(x.day);
      c.style.gridArea = `${wd + 2} / ${w + 2}`;
      grid.append(c);
    });
    labels.forEach(({ m, w }, i) => {
      m.style.gridArea = `1 / ${w + 2} / 2 / ${(labels[i + 1]?.w ?? weeks) + 2}`;
    });
    grid.onpointerover = (e) => {
      if (e.target.dataset?.date) heatTip(e.target);
    };
    grid.onpointerleave = () => {
      const tip = document.getElementById("ledHeatTip");
      if (tip) tip.hidden = true;
    };
    scroll.replaceChildren(grid);
    // the newest week is what the reader came for: the card opens at its end
    scroll.scrollLeft = scroll.scrollWidth;
  }
  grid.setAttribute("aria-label", "用量热力图");
  const cells = grid.querySelectorAll("i");
  days.forEach((x, i) => {
    const c = cells[i];
    if (!c) return;
    c.className = "l" + level(value(x));
    c.day = x;
  });
  const legend = box.querySelector(".heat-foot");
  if (legend) {
    legend.replaceChildren();
    const span = document.createElement("span");
    span.className = "sess-legend";
    span.append(Object.assign(document.createElement("span"), { textContent: "少" }));
    for (let i = 0; i <= 4; i++) {
      const sw = document.createElement("i");
      sw.className = "l" + i;
      span.append(sw);
    }
    span.append(Object.assign(document.createElement("span"), { textContent: "多" }));
    legend.append(span);
  }
  const tip = document.getElementById("ledHeatTip");
  if (tip && !tip.hidden && tip.cell?.isConnected) heatTip(tip.cell);
  else if (tip) tip.hidden = true;
}

// the tip over a day, above it and kept inside the card
function heatTip(cell) {
  const box = document.getElementById("ledHeat");
  const tip = document.getElementById("ledHeatTip");
  if (!box || !tip || !cell.day) return;
  const x = cell.day;
  tip.replaceChildren();
  const head = document.createElement("b");
  head.textContent = `${x.day.getFullYear()}年${x.day.getMonth() + 1}月${x.day.getDate()}日 周${["日", "一", "二", "三", "四", "五", "六"][x.day.getDay()]}`;
  tip.append(head);
  if (!x.calls) {
    const none = document.createElement("span");
    none.textContent = "没有请求";
    tip.append(none);
  } else {
    const parts = [`${fmtN(x.tokens)} Token`, `${x.calls} 次请求`];
    if (x.cost) parts.push(fmtMoney(x.cost));
    if (x.errors) parts.push(`${x.errors} 次失败`);
    const span = document.createElement("span");
    span.textContent = parts.join(" · ");
    tip.append(span);
  }
  tip.cell = cell;
  tip.hidden = false;
  const b = box.getBoundingClientRect();
  const c = cell.getBoundingClientRect();
  const w = tip.offsetWidth;
  const h = tip.offsetHeight;
  const left = Math.max(6, Math.min(b.width - w - 6, c.left - b.left + c.width / 2 - w / 2));
  let top = c.top - b.top - h - 6;
  if (top < 4) top = c.bottom - b.top + 6;
  tip.style.left = left + "px";
  tip.style.top = top + "px";
}

// ---- the request table ---------------------------------------------------
// The rows the period holds, then the page's own status and search. The
// statuses are the daemon's readings: a failure a later try answered is
// "retried", not failed; the call that answered it says "retried".
function ledFilteredRows() {
  const rows = (usageDigest && usageDigest.recent) || [];
  return rows.filter((r) => {
    if (ledStatus === "failed" && !(r.error && !r.recovered)) return false;
    if (ledStatus === "retried" && !r.retried) return false;
    if (!ledQuery) return true;
    const text = `${r.model || ""} ${r.agent || ""} ${r.session || ""} ${r.cwd || ""} ${r.error || ""}`.toLowerCase();
    return text.includes(ledQuery);
  });
}

function ledStatusCell(r) {
  if (r.retried) return `<span class="tag ok">重试后成功</span>`;
  if (r.error && !r.recovered)
    return `<span class="tag err" title="${escapeHtml(r.error)}">失败</span>`;
  if (r.error) return `<span class="tag warn" title="${escapeHtml(r.error)}">已恢复</span>`;
  return `<span class="tag">成功</span>`;
}

function renderLedgerRequests() {
  const wrap = document.getElementById("ledWrap");
  const badge = document.getElementById("ledCount");
  if (!wrap) return;
  // a poll redraws the same list: the reader's place in it stays put
  const kept = wrap.scrollTop;
  if (!usageDigest) {
    wrap.innerHTML = `<div class="led-none">正在读取用量…</div>`;
    return;
  }
  const rows = ledFilteredRows();
  if (badge) badge.textContent = `${rows.length.toLocaleString()} 条请求`;
  if (!rows.length) {
    // the list holds the period's latest calls, not every call of the period:
    // a filter that finds nothing here says so rather than implying the period
    // had no failure at all
    wrap.innerHTML = `<div class="led-none">${
      ledStatus || ledQuery
        ? "最近的请求里没有符合筛选条件的记录"
        : "本周期没有请求"
    }</div>`;
    // a reader looking for a rare failure is told where it may still be: the
    // table draws the latest calls, the figures above count every call
    if (ledStatus && usageDigest.errors) {
      wrap.firstElementChild.textContent += `（本周期共 ${usageDigest.errors} 次失败，最近的一千条里没有）`;
    }
    return;
  }
  wrap.innerHTML = `
    <table class="led">
      <thead><tr>
        <th>时间</th><th>智能体</th><th>模型</th><th class="n">输入</th><th class="n">输出</th><th class="n">缓存读</th><th class="n">费用</th><th>状态</th>
      </tr></thead>
      <tbody>${rows
        .map((r) => {
          const failed = r.error && !r.recovered;
          return `<tr${failed ? ' class="bad"' : ""}>
            <td class="when">${escapeHtml(fmtWhen(r.t, usagePeriod !== "today"))}</td>
            <td class="muted">${escapeHtml(providerLabel(r.agent))}</td>
            <td class="model-value">${r.model ? getModelIcon(r.model, 14) + escapeHtml(r.model) : "—"}</td>
            <td class="n">${fmtN(r.input || 0)}</td>
            <td class="n">${fmtN(r.output || 0)}</td>
            <td class="n">${fmtN(r.cache_read || 0)}</td>
            <td class="n">${fmtMoney(r.cost || 0)}</td>
            <td>${ledStatusCell(r)}</td>
          </tr>`;
        })
        .join("")}</tbody>
    </table>`;
  wrap.scrollTop = kept;
}

// ---- the sessions and the context ---------------------------------------
function renderLedgerSessions() {
  const wrap = document.getElementById("usageSessions");
  if (!wrap) return;
  const kept = wrap.scrollTop;
  const rows = (usageDigest && usageDigest.session_rows) || [];
  if (!rows.length) {
    wrap.innerHTML = `<div class="led-none">本周期没有会话</div>`;
    return;
  }
  wrap.innerHTML = `
    <table class="led">
      <thead><tr>
        <th>最后活动</th><th>智能体</th><th>模型</th><th>目录</th><th class="n">请求</th><th class="n">Token</th><th class="n">费用</th><th>状态</th>
      </tr></thead>
      <tbody>${rows
        .map(
          (s) => `<tr>
            <td class="when">${escapeHtml(fmtWhen(s.last, usagePeriod !== "today"))}</td>
            <td class="muted">${escapeHtml(s.agent_name)}</td>
            <td class="model-value">${s.model ? getModelIcon(s.model, 14) + escapeHtml(s.model) : "—"}</td>
            <td class="muted mono" title="${escapeHtml(s.cwd)}">${escapeHtml(shortPath(s.cwd))}</td>
            <td class="n">${(s.calls || 0).toLocaleString()}</td>
            <td class="n">${fmtN(s.tokens || 0)}</td>
            <td class="n">${fmtMoney(s.cost || 0)}</td>
            <td>${s.errors ? `<span class="tag err">${s.errors} 次失败</span>` : `<span class="tag ok">正常</span>`}</td>
          </tr>`,
        )
        .join("")}</tbody>
    </table>`;
  wrap.scrollTop = kept;
}

function renderLedgerContext() {
  const wrap = document.getElementById("usageContext");
  if (!wrap) return;
  const kept = wrap.scrollTop;
  const rows = ((usageDigest && usageDigest.session_rows) || [])
    .filter((s) => s.context > 0)
    .sort((a, b) => b.context - a.context);
  if (!rows.length) {
    wrap.innerHTML = `<div class="led-none">本周期没有可读的上下文</div>`;
    return;
  }
  const max = Math.max(1, rows[0].context);
  wrap.innerHTML = rows
    .map((s) => {
      const pct = Math.max(1, Math.round((100 * s.context) / max));
      return `<div class="row stat ctx-row">
        <span class="ic">${getAgentIcon(s.agent, 18)}</span>
        <div class="who">
          <div class="name-line"><span class="name">${escapeHtml(shortPath(s.cwd) || s.id.slice(0, 8))}</span>
            <span class="tag mono">${escapeHtml(s.id.slice(0, 8))}</span></div>
          <div class="sub">${escapeHtml(s.agent_name)} · ${escapeHtml(s.model || "—")} · ${(s.calls || 0).toLocaleString()} 次请求</div>
        </div>
        <div class="share" title="上下文 ${fmtN(s.context)} Token"><i style="width:${pct}%;"></i></div>
        <div class="num"><b>${fmtN(s.context)}</b><small>提示词 Token</small></div>
        <div class="ctx-when">${escapeHtml(fmtWhen(s.last, false))}</div>
      </div>`;
    })
    .join("");
  wrap.scrollTop = kept;
}

// the tail of a path, so a row keeps its identity without the drive letters
function shortPath(p) {
  const parts = String(p || "")
    .split(/[\\/]/)
    .filter(Boolean);
  if (!parts.length) return "";
  return parts.slice(-2).join("/");
}

// The CSV is the filtered list, every row the period answered, not just the
// thousand the table draws: a reader exporting wants the period's requests.
function exportLedgerCsv() {
  if (!usageDigest) return;
  const rows = ledFilteredRows();
  if (!rows.length) {
    toast("当前筛选没有可导出的请求", "err");
    return;
  }
  const head = ["time", "agent", "model", "session", "cwd", "input", "output", "cache_read", "cache_write", "cost_usd", "status", "error"];
  const cell = (v) => {
    const s = String(v == null ? "" : v);
    return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  };
  const body = rows.map((r) => {
    const status = r.error ? (r.recovered ? "recovered" : "failed") : r.retried ? "retried" : "ok";
    return [
      new Date(r.t).toISOString(),
      r.agent,
      r.model,
      r.session,
      r.cwd,
      r.input,
      r.output,
      r.cache_read,
      r.cache_write,
      (Number(r.cost) || 0).toFixed(6),
      status,
      r.error || "",
    ]
      .map(cell)
      .join(",");
  });
  const csv = "﻿" + [head.join(","), ...body].join("\r\n");
  const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `agenthub-usage-${usagePeriod}-${heatISO(new Date())}.csv`;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  toast(`已导出 ${rows.length.toLocaleString()} 条请求`);
}

// ---- the settings page -----------------------------------------------------
// magpie's Settings: a tab per part across the head, that part's rows under
// it, and a box that searches every part at once while it holds words. The
// page is not a sheet — the header, the rail and the footer stay put, and
// the part's DOM is read where it already is.
let settingsTab = "general";
let settingsQuery = "";

const SET_TAB_LABELS = {
  general: "通用 · General",
  appearance: "外观 · Appearance",
  gateway: "设备与网关 · Gateway",
  capabilities: "能力生态 · Capabilities",
  about: "关于 · About",
};

function initSettingsPage() {
  const saved = localStorage.getItem(STORE.settingsTab);
  if (SET_TABS.includes(saved)) settingsTab = saved;
  const tabs = document.getElementById("setTabs");
  if (tabs)
    tabs.addEventListener("click", (e) => {
      const b = e.target.closest("button[data-set]");
      if (b) setSetTab(b.dataset.set, true);
    });
  const q = document.getElementById("setQ");
  if (q) {
    q.addEventListener("input", () => searchSettings(q.value));
    q.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && q.value) {
        e.preventDefault();
        e.stopPropagation();
        q.value = "";
        searchSettings("");
      }
    });
  }
  setSetTab(settingsTab, false);
}

function setSetTab(tab, remember) {
  if (!SET_TABS.includes(tab)) tab = "general";
  settingsTab = tab;
  if (remember) {
    try {
      localStorage.setItem(STORE.settingsTab, tab);
    } catch (e) {}
  }
  SET_TABS.forEach((id) => {
    const b = document.getElementById("setTab-" + id);
    const page = document.getElementById("setPage-" + id);
    const on = id === tab;
    if (b) {
      b.classList.toggle("on", on);
      b.setAttribute("aria-selected", String(on));
      b.tabIndex = on ? 0 : -1;
    }
    if (page) page.hidden = !on;
  });
}

// The page's own text, so a search can say which row said the word
const setText = (n) => (n ? n.textContent : "").toLowerCase();

// While the box holds words, every part's rows that say them are shown under
// the part's own name and the tabs step aside; emptied, the part comes back.
function searchSettings(value) {
  settingsQuery = value.trim().toLowerCase();
  const page = document.getElementById("view-settings");
  if (!page) return;
  const none = document.getElementById("setNone");
  if (!settingsQuery) {
    page.classList.remove("set-searching");
    page
      .querySelectorAll(".set-miss")
      .forEach((n) => n.classList.remove("set-miss"));
    if (none) none.hidden = true;
    setSetTab(settingsTab, false);
    return;
  }
  page.classList.add("set-searching");
  const words = settingsQuery.split(/\s+/);
  let any = false;
  SET_TABS.forEach((id) => {
    const part = document.getElementById("setPage-" + id);
    const tab = document.getElementById("setTab-" + id);
    if (!part) return;
    // the part's name counts as one of the words every row may answer to
    const partText = setText(tab) + " " + (SET_TAB_LABELS[id] || "");
    let hits = 0;
    part.querySelectorAll(".row").forEach((row) => {
      const says = partText + " " + setText(row);
      const hit = words.every((w) => says.includes(w));
      row.classList.toggle("set-miss", !hit);
      if (hit) hits++;
    });
    part.querySelectorAll(".list").forEach((list) => {
      const visible = [...list.children].some(
        (c) => c.classList.contains("row") && !c.classList.contains("set-miss"),
      );
      list.classList.toggle("set-miss", !visible);
    });
    part.dataset.part = (tab ? tab.textContent : id) || id;
    part.hidden = hits === 0;
    if (hits > 0) any = true;
  });
  if (none) {
    none.hidden = any;
    none.textContent = `没有匹配“${value.trim()}”的设置`;
  }
}

// every pick on the page is one shape: the row's own options, one of them on
function pickIn(segsId, match) {
  const segs = document.getElementById(segsId);
  if (!segs) return;
  segs.querySelectorAll(".opt").forEach((b) => b.classList.remove("on"));
  const on = match(segs);
  if (on) on.classList.add("on");
}

// draws every value the rows carry from the state they describe
function renderSettings() {
  const dark = document.documentElement.classList.contains("dark");
  const optLight = document.getElementById("themeOptLight");
  const optDark = document.getElementById("themeOptDark");
  if (optLight) optLight.classList.toggle("on", !dark);
  if (optDark) optDark.classList.toggle("on", dark);

  pickIn("pollSegs", (s) =>
    s.querySelector(`.opt[data-ms="${pollIntervalMs()}"]`),
  );
  pickIn("sideNavSegs", (s) =>
    s.querySelector(
      `.opt[data-side="${document.documentElement.classList.contains("no-side") ? "off" : "on"}"]`,
    ),
  );
  pickIn("motionSegs", (s) =>
    s.querySelector(
      `.opt[data-motion="${motionOff() ? "off" : "on"}"]`,
    ),
  );

  const ep = document.getElementById("settingsEndpoint");
  if (ep)
    ep.textContent = `http://${location.hostname || "127.0.0.1"}:${PORT} (内嵌 HTTP / SSE)`;
  const ver = document.getElementById("settingsVersion");
  if (ver) ver.textContent = `AgentHub Runner v${LAYOUT.version || "dev"}`;

  const gwUrl = document.getElementById("settingsGatewayUrl");
  if (gwUrl)
    gwUrl.textContent =
      (dashboardData &&
        dashboardData.gateway &&
        dashboardData.gateway.base_url) ||
      GATEWAY_URL;
  const gwSub = document.getElementById("settingsGatewaySub");
  if (gwSub) {
    const gw = dashboardData && dashboardData.gateway;
    // the source the daemon reports is a path or a credential-file name;
    // the row says what it means, as the ledger's own sub does
    let src = String((gw && gw.source) || "").trim();
    if (src.includes("host-creds") || src.includes(".json"))
      src = "动态检测网关";
    // the console only says what it saw; a snapshot from before the first
    // probe reads as one still on its way
    gwSub.textContent = gw
      ? gw.detected
        ? `已探测 · ${src || "标准端点"}`
        : "尚未探测到上游网关"
      : "正在等待探测…";
  }

  // the console reads resources/ on every request only in a debug build;
  // the tag says which build this window is running
  const dev = !!LAYOUT.devAssets;
  const hotTag = document.getElementById("hotReloadTag");
  if (hotTag) {
    hotTag.textContent = dev ? "开发构建 · 已开启" : "发布构建";
    hotTag.classList.toggle("ok", dev);
  }
  const hotSub = document.getElementById("hotReloadSub");
  if (hotSub)
    hotSub.textContent = dev
      ? "直接读取 resources/ 下的页面、样式与脚本，保存后窗口自己刷新（热重载）"
      : "发布构建从二进制内嵌副本读取；用 cargo run 启动即为开发构建";

  // the pairing form is filled from the device the daemon last reported
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
  if (dashboardData && dashboardData.device) renderDevice(dashboardData.device);
}

// the automatic probe's cadence, the reader's to set (never under half a
// second: the console's own rule for a self-refreshing surface)
function pollIntervalMs() {
  const saved = parseInt(localStorage.getItem(STORE.pollMs) || "", 10);
  return [2000, 4000, 10000].includes(saved)
    ? saved
    : LAYOUT.dashboardPollMs || 4000;
}

let pollTimer = 0;
function startPolling() {
  clearInterval(pollTimer);
  pollTimer = setInterval(refreshDashboard, pollIntervalMs());
}

function setPollInterval(ms) {
  try {
    localStorage.setItem(STORE.pollMs, String(ms));
  } catch (e) {}
  startPolling();
  pickIn("pollSegs", (s) => s.querySelector(`.opt[data-ms="${ms}"]`));
  toast(`已改为每 ${Math.round(ms / 1000)} 秒自动探测`);
}

function setSideNavFromSettings(on) {
  setSideNav(on, true);
  pickIn("sideNavSegs", (s) =>
    s.querySelector(`.opt[data-side="${on ? "on" : "off"}"]`),
  );
}

function motionOff() {
  try {
    return localStorage.getItem(STORE.motion) === "off";
  } catch (e) {
    return false;
  }
}

// the reader's own motion choice, over the system's: "off" reads every
// scripted animation as already finished (calmMotion consults it)
function setMotion(mode) {
  try {
    if (mode === "off") localStorage.setItem(STORE.motion, "off");
    else localStorage.removeItem(STORE.motion);
  } catch (e) {}
  pickIn("motionSegs", (s) => s.querySelector(`.opt[data-motion="${mode}"]`));
  toast(mode === "off" ? "已精简动效" : "已恢复完整动效");
}

function setAppTheme(theme) {
  const dark = theme === "dark";
  applyTheme(dark);
  toast(`已切换至${dark ? "暗黑" : "明亮"}外观`);
}

// One theme switch, whichever control asked for it: the class, the stored
// choice, the caption's copy and the row's own pick all move together.
function applyTheme(dark) {
  const html = document.documentElement;
  const themeMeta = document.getElementById("themeColorMeta");
  const apply = () => {
    html.classList.toggle("dark", dark);
    try {
      localStorage.setItem(STORE.theme, dark ? "dark" : "light");
    } catch (e) {}
    if (themeMeta)
      themeMeta.setAttribute(
        "content",
        dark ? LAYOUT.themeBg.dark : LAYOUT.themeBg.light,
      );
    const optLight = document.getElementById("themeOptLight");
    const optDark = document.getElementById("themeOptDark");
    if (optLight) optLight.classList.toggle("on", !dark);
    if (optDark) optDark.classList.toggle("on", dark);
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
  clearTimeout(applyTheme._timer);
  applyTheme._timer = setTimeout(() => html.classList.remove("theme-fade"), 520);
}

// the theme row's two picks, redrawn from the theme the page is painted in
function updateThemeChoice() {
  const dark = document.documentElement.classList.contains("dark");
  const themeMeta = document.getElementById("themeColorMeta");
  if (themeMeta)
    themeMeta.setAttribute(
      "content",
      dark ? LAYOUT.themeBg.dark : LAYOUT.themeBg.light,
    );
  const optLight = document.getElementById("themeOptLight");
  const optDark = document.getElementById("themeOptDark");
  if (optLight) optLight.classList.toggle("on", !dark);
  if (optDark) optDark.classList.toggle("on", dark);
}

async function openDataDirectory() {
  try {
    const res = await fetch("/api/open-data-dir", { method: "POST" });
    if (res.ok) {
      toast("已在系统文件资源管理器中打开数据目录");
    } else {
      toast("唤起资源管理器失败", "err");
    }
  } catch (e) {
    toast("无法连接本地 Runner 服务", "err");
  }
}

function copyDeviceId() {
  const idElem = document.getElementById("modalDeviceId");
  if (idElem && idElem.textContent && idElem.textContent !== "-") {
    copyText(idElem.textContent);
    toast("已复制设备 ID");
  }
}

async function submitDeviceName() {
  const input = document.getElementById("deviceNameInput");
  const saveBtn = document.getElementById("deviceNameSaveBtn");
  if (!input) return;
  const name = input.value.trim();
  if (!name) {
    toast("请填写这台电脑的名字", "err");
    return;
  }
  const origText = saveBtn ? saveBtn.innerHTML : "";
  if (saveBtn) {
    saveBtn.disabled = true;
    saveBtn.innerHTML = "<span>保存中...</span>";
  }
  try {
    const resp = await fetch("/api/device/name", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: name }),
    });
    const resData = await resp.json();
    if (resp.ok && resData.ok === true) {
      toast("设备名称已更新");
      await refreshDashboard();
    } else {
      toast("改名失败：" + (resData.error || "未知错误"), "err");
    }
  } catch (err) {
    toast("改名失败：" + err.message, "err");
  } finally {
    if (saveBtn) {
      saveBtn.disabled = false;
      saveBtn.innerHTML = origText;
    }
  }
}

async function submitDevicePairing(event) {
  event.preventDefault();
  const gatewayInput = document.getElementById("gatewayInput");
  const codeInput = document.getElementById("bindingCodeInput");
  const submitBtn = document.getElementById("pairSubmitBtn");

  let gateway = (gatewayInput ? gatewayInput.value : "").trim();
  const code = (codeInput ? codeInput.value : "").trim();

  if (!gateway) {
    toast("填写控制台地址，例如 https://agenthub.yourdomain.com", "err");
    return;
  }
  if (!gateway.startsWith("http://") && !gateway.startsWith("https://")) {
    gateway = "https://" + gateway;
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
      renderSettings();
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

async function submitDeviceUnpair() {
  if (!confirm("确定要解除本机与云端网关的绑定关系吗？解除后本机将停止出站同步。")) return;
  const unpairBtn = document.getElementById("unpairSubmitBtn");
  const origText = unpairBtn ? unpairBtn.innerHTML : "";
  if (unpairBtn) {
    unpairBtn.disabled = true;
    unpairBtn.innerHTML = "<span>正在解除绑定...</span>";
  }
  try {
    const resp = await fetch("/api/device/unbind", {
      method: "POST",
    });
    const resData = await resp.json();
    if (resp.ok && (resData.ok === true || resData.status === "ok")) {
      renderSettings();
      await refreshDashboard();
      toast("已解除与云端 AgentHub 的绑定");
    } else {
      toast("解绑失败：" + (resData.message || resData.error || "未知错误"), "err");
    }
  } catch (err) {
    toast("解绑失败：" + err.message, "err");
  } finally {
    if (unpairBtn) {
      unpairBtn.disabled = false;
      unpairBtn.innerHTML = origText;
    }
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
