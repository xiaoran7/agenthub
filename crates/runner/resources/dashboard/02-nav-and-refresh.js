function switchView(viewId, silent) {
  if (!Object.hasOwn(VIEW_LABELS, viewId)) return;
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
      if (usageDigest) {
        drawUsage();
      }
      if (!usageDigest || usageDirty) {
        loadUsage(!usageDigest);
      }
      startUsageTimer();
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
let dashboardEtag = "";
let sessionsJustDeleted = false;

function applySettingsHash() {
  const raw = location.hash || "";
  const m = /^#settings(?:\/([a-z]+))?$/.exec(raw);
  if (m) {
    history.replaceState(null, "", location.pathname + location.search);
    openSettings(SET_TABS.includes(m[1]) ? m[1] : "general");
    return;
  }
  const vm = /^#(sessions|agents|usage|workspaces)$/.exec(raw);
  if (vm) {
    history.replaceState(null, "", location.pathname + location.search);
    switchView(vm[1], true);
  }
}

window.addEventListener("hashchange", applySettingsHash);

// manual: the reader asked for this probe, so the button that asked turns
// while it is out. Background polls pass If-None-Match so unchanged state
// answers 304 Not Modified with zero JSON transfer or DOM diffing.
async function refreshDashboard(manual) {
  if (!manual && document.hidden && dashboardData) return true;
  // a probe can take over a second; never let two snapshots render out of order
  if (refreshing) return false;
  refreshing = true;
  const refreshBtn = document.getElementById("refreshBtn");
  const setRefreshBtn = document.getElementById("setRefreshBtn");
  if (manual) {
    if (refreshBtn) refreshBtn.classList.add("busy");
    if (setRefreshBtn) setRefreshBtn.classList.add("busy");
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

  // Workspaces skeleton on initial load or manual refresh if not yet populated
  const wsContainer = document.getElementById("workspacesList");
  if (wsContainer && (!dashboardData || !dashboardData.workspaces || dashboardData.workspaces.length === 0)) {
    if (!wsContainer.querySelector(".skeleton-card")) {
      wsContainer.innerHTML = renderWorkspaceSkeleton(5);
    }
    const wsBadge = document.getElementById("workspacesCountBadge");
    if (wsBadge && (!dashboardData || !dashboardData.workspaces)) wsBadge.textContent = "正在加载…";
  }

  // Agents skeleton on initial load or manual refresh if not yet populated
  const agentsContainer = document.getElementById("agentsGrid");
  if (agentsContainer && (!dashboardData || !dashboardData.agents || dashboardData.agents.length === 0)) {
    if (!agentsContainer.querySelector(".skeleton-card")) {
      agentsContainer.innerHTML = renderAgentSkeleton(4);
    }
    const agBadge = document.getElementById("agentsCountBadge");
    if (agBadge && (!dashboardData || !dashboardData.agents)) agBadge.textContent = "正在加载…";
  }

  const seq = ++renderSeq;
  try {
    const url = manual ? "/api/dashboard?force=1" : "/api/dashboard";
    const headers = !manual && dashboardEtag ? { "If-None-Match": dashboardEtag } : {};
    const resp = await fetch(url, { cache: "no-store", headers });
    if (resp.status === 304 && dashboardData) {
      consecutiveFailures = 0;
      updateConnectionStatus(true);
      if (manual) toast("已是最新数据");
      return true;
    }
    if (!resp.ok) {
      handleFetchFailure();
      return false;
    }
    const etag = resp.headers.get("ETag");
    if (etag) dashboardEtag = etag;
    const data = await resp.json();
    if (seq !== renderSeq) return false;
    consecutiveFailures = 0;
    updateConnectionStatus(true);
    if (manual) {
      usageDirty = true;
      usageMemoryCache.clear();
      if (isUsageOpen()) loadUsage(true);
      toast("已刷新最新会话与工作区状态");
    }

    dashboardData = data;
    if (data.pending_nav) {
      location.hash = data.pending_nav;
      applySettingsHash();
    }
    registerPluginAgentMeta(dashboardData.agents);
    if (dashboardData.plugins && Array.isArray(dashboardData.plugins.items)) {
      registerPluginAgentMeta(dashboardData.plugins.items);
      const pluginsStr = JSON.stringify(dashboardData.plugins);
      if (pluginsStr !== lastRenderState.pluginsJson) {
        renderPluginsSection(dashboardData.plugins);
        lastRenderState.pluginsJson = pluginsStr;
      }
    }
    renderStats(dashboardData.stats, dashboardData.gateway);

    // Agents: unconditionally render when agents change or upon arrival
    const agentsStr = JSON.stringify(dashboardData.agents || []);
    if (agentsStr !== lastRenderState.agentsJson) {
      renderAgents(dashboardData.agents);
      lastRenderState.agentsJson = agentsStr;
    }

    // Sessions handling
    const sessions = dashboardData.sessions || [];
    const isReady = dashboardData.ready !== false && dashboardData.status !== "starting";
    const sessionsLoading = dashboardData.sessions_loading === true || !isReady;
    let sessionsChanged = false;

    if (sessionsLoading) {
      // Transcript scanning or daemon startup is in progress:
      // never show partial sessions (e.g. Codex-only before Claude finishes scanning)
      if (lastGoodSessions.length === 0) {
        renderSessions([], "loading");
      }
      setTimeout(() => {
        if (!refreshing) refreshDashboard();
      }, 250);
    } else if (sessions.length > 0) {
      consecutiveEmptyResponses = 0;
      sessionsJustDeleted = false;
      const sessionsStr = JSON.stringify(sessions);
      sessionsChanged = sessionsStr !== lastRenderState.sessionsJson;
      lastGoodSessions = sessions;
      allSessionsData = sessions;
      lastRenderState.sessionsJson = sessionsStr;

      if (sessionsChanged) {
        renderSessions(sessions);
      }
    } else {
      // an empty answer earns the empty state immediately after an explicit
      // user deletion or once it has held across the grace window
      consecutiveEmptyResponses++;
      const settled =
        sessionsJustDeleted ||
        (consecutiveEmptyResponses >= (LAYOUT.emptyStrikes || 3) &&
          Date.now() - sessionsRenderedAt > (LAYOUT.emptyGraceMs || 15000));
      if (settled) {
        sessionsJustDeleted = false;
        sessionsChanged =
          lastGoodSessions.length > 0 || lastRenderState.sessionsJson !== "[]";
        lastGoodSessions = [];
        allSessionsData = [];
        lastRenderState.sessionsJson = "[]";
        renderSessions([], "empty");
      } else if (lastGoodSessions.length === 0) {
        renderSessions([], "pending");
      }
    }

    // Workspaces: unconditionally render when workspaces or sessions change,
    // decoupled from whether sessions.length > 0
    const wsStr = JSON.stringify(dashboardData.workspaces || []);
    if (wsStr !== lastRenderState.workspacesJson || sessionsChanged) {
      renderWorkspaces(dashboardData.workspaces);
      lastRenderState.workspacesJson = wsStr;
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
    if (setRefreshBtn) setRefreshBtn.classList.remove("busy");
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
  const counts = stats.provider_counts || {};
  const nativeKeys = new Set([
    "claude_code",
    "claude_desktop",
    "antigravity",
    "codex",
    "deepseek_harness",
  ]);
  const parts = [
    `Coding Assistant ${stats.claude_sessions || 0}`,
    `Claude Desktop ${stats.claude_desktop_sessions || 0}`,
    `Antigravity ${stats.antigravity_sessions || 0}`,
    `Codex ${stats.codex_sessions || 0}`,
    `DeepSeek Harness ${stats.deepseek_harness_sessions || 0}`,
  ];
  for (const [k, v] of Object.entries(counts)) {
    if (nativeKeys.has(k)) continue;
    if (Number(v) > 0) parts.push(`${providerLabel(k)} ${v}`);
  }
  breakdown.textContent = parts.join(" · ");
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
