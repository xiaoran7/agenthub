(function bootstrapAppLayout() {
  let stamped = {};
  const dataEl = document.getElementById("app-layout-data");
  if (dataEl && dataEl.textContent) {
    try {
      const parsed = JSON.parse(dataEl.textContent);
      if (parsed && typeof parsed === "object") stamped = parsed;
    } catch (e) {}
  }
  window.AppLayout = Object.assign(
    {
      version: "dev",
      window: {
        width: 820,
        height: 600,
        minWidth: 640,
        minHeight: 460,
        sideMinWidth: 168,
        sideWidth: 212,
        sideMaxWidth: 300,
      },
      sidebarAt: 768,
      sidebarNarrow: 200,
      contentMax: 1100,
      inset: 16,
      radius: 14,
      radiusSm: 10,
      topBar: 46,
      dashboardPollMs: 4000,
      firstProbeRetries: 2,
      firstProbeRetryMs: 1500,
      emptyGraceMs: 15000,
      emptyStrikes: 3,
      gatewayPort: 3425,
      themeBg: { light: "#f4f5f8", dark: "#1a1a1e" },
      storage: {
        theme: "agenthub_theme",
        view: "agenthub_view",
        layout: "agenthub_layout",
        sideWidth: "agenthub_side_w",
        closedGroups: "agenthub_closed_groups",
        customGateway: "agenthub_custom_gateway",
        sideNav: "agenthub_side_nav",
        settingsTab: "agenthub_settings_tab",
        pollMs: "agenthub_poll_ms",
        motion: "agenthub_motion",
      },
    },
    window.AppLayout || {},
    stamped,
  );

  function layoutTokens(a) {
    const w = a.window || {};
    const side = Math.round(
      Math.max(
        a.sidebarNarrow || 212,
        Math.min(w.sideWidth || 236, Math.round((window.innerWidth || 1280) * 0.24)),
      ),
    );
    const chrome =
      "--layout-side-w:" +
      side +
      "px;" +
      "--layout-side-at:" +
      (a.sidebarAt || 1200) +
      "px;" +
      "--layout-inset:" +
      (a.inset || 18) +
      "px;" +
      "--layout-radius:" +
      (a.radius || 14) +
      "px;" +
      "--layout-radius-s:" +
      (a.radiusSm || 10) +
      "px;" +
      "--layout-top-h:" +
      (a.topBar || 38) +
      "px;" +
      "--layout-content-w:" +
      (a.contentMax || 1280) +
      "px;";
    const s = Math.max(0, Math.min(1, ((window.innerWidth || 1280) - 960) / 560));
    document.documentElement.style.cssText =
      chrome +
      "--fs-note:" +
      (13.5 + s * 1.5).toFixed(1) +
      "px;" +
      "--fs-body:" +
      (14.5 + s * 1.5).toFixed(1) +
      "px;" +
      "--fs-title:" +
      (16 + s * 2).toFixed(1) +
      "px;" +
      "--fs-figure:" +
      (28 + s * 6).toFixed(1) +
      "px;";
  }
  const AppLayout = window.AppLayout;
  const win = AppLayout.window || {};
  window.__layoutTokens = layoutTokens;
  layoutTokens(AppLayout);
  let tokensTimer = 0;
  window.addEventListener(
    "resize",
    () => {
      clearTimeout(tokensTimer);
      tokensTimer = setTimeout(() => layoutTokens(AppLayout), 120);
    },
    { passive: true },
  );

  if (/^Win/i.test(navigator.platform) || /Windows/i.test(navigator.userAgent)) {
    document.documentElement.classList.add("win");
  }

  const store = AppLayout.storage || {};
  const savedTheme = localStorage.getItem(store.theme);
  const dark = savedTheme === "dark";
  document.documentElement.classList.toggle("dark", dark);
  const bg = (AppLayout.themeBg || {})[dark ? "dark" : "light"];
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta && bg) meta.setAttribute("content", bg);

  const sidePref = localStorage.getItem(store.sideNav);
  const showSide = sidePref
    ? sidePref === "on"
    : (window.innerWidth || 0) >= (AppLayout.sidebarAt || 768);
  document.documentElement.classList.toggle("no-side", !showSide);
  document.documentElement.dataset.nav = showSide ? "side" : "top";

  const sideW = parseInt(localStorage.getItem(store.sideWidth) || "", 10);
  const sideCap = Math.min(
    win.sideMaxWidth || 340,
    Math.floor((window.innerWidth || 1280) * 0.25),
  );
  if (sideW >= (win.sideMinWidth || 168) && sideW <= sideCap) {
    document.documentElement.style.setProperty("--side-w-override", sideW + "px");
  }
})();

let dashboardData = null;
let lastRenderState = {
  agentsJson: "",
  workspacesJson: "",
  deviceJson: "",
  sessionsJson: "",
  capabilitiesJson: "",
  pluginsJson: "",
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
const SET_TABS = ["general", "appearance", "gateway", "capabilities", "plugins", "about"];

function updateCrumb(viewId) {
  const title = document.getElementById("topCrumbTitle");
  const sub = document.getElementById("topCrumbSub");
  if (title) title.textContent = Object.hasOwn(VIEW_LABELS, viewId) ? VIEW_LABELS[viewId] : viewId;
  if (sub) sub.textContent = Object.hasOwn(VIEW_EN_LABELS, viewId) ? VIEW_EN_LABELS[viewId] : viewId;
}

const PROVIDER_LABELS = {
  claude_code: "Claude Code",
  claude: "Claude",
  claude_desktop: "Claude Desktop",
  codex: "Codex",
  antigravity: "Antigravity",
  deepseek_harness: "DeepSeek Harness",
};
const dynamicPluginMeta = new Map();

function registerPluginAgentMeta(list) {
  if (!Array.isArray(list)) return;
  for (const item of list) {
    const id = String((item && (item.id || item.plugin_id)) || "")
      .toLowerCase()
      .trim();
    if (!id) continue;
    dynamicPluginMeta.set(id, {
      name: item.name || id,
      iconUrl: item.icon_url || "",
      isPlugin: item.is_plugin !== false,
    });
  }
}

function providerLabel(id) {
  const key = String(id || "")
    .toLowerCase()
    .trim();
  const dyn = dynamicPluginMeta.get(key);
  if (dyn && dyn.name) return dyn.name;
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

function localCsrfToken() {
  return (window.AppLayout && window.AppLayout.csrfToken) || "";
}

function localApiPost(url, payload) {
  return fetch(url, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-AgentHub-Csrf": localCsrfToken(),
    },
    body: JSON.stringify(payload === undefined ? {} : payload),
  });
}

// the caption's theme, kept where the next window reads it as it opens
function syncWindowTheme() {
  const dark = document.documentElement.classList.contains("dark");
  localApiPost("/api/window-theme", { dark }).catch(() => {});
}

// A closed window tears its stream down itself. Without this the browser
// would keep reconnecting for the tab's id until it is finally dropped, which
// on a laptop is a connection every second pointed at a page nobody sees.
window.addEventListener("pagehide", () => {
  clearTimeout(streamRetryTimer);
  if (streamSource) streamSource.close();
});

async function init() {
  // the first paint is drawn once, in order; the class comes off on the
  // next frame so a poll or a switch never replays it
  document.documentElement.classList.add("booting");
  layoutNav();
  initSideEdge();
  initTheme();
  const savedView = localStorage.getItem(STORE.view);
  switchView(savedView && Object.hasOwn(VIEW_LABELS, savedView) ? savedView : "sessions", true);
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
    closeAsk(false);
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
  if (!btn) {
    thumb.style.opacity = "0";
    thumb.style.width = "0px";
    return;
  }
  thumb.style.opacity = "";
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
