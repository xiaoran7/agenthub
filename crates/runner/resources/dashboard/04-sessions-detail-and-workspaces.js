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
  let subs = row.nextElementSibling;
  if (!subs || !subs.classList.contains("subs")) {
    const base = lastGoodSessions.length ? lastGoodSessions : allSessionsData;
    const hit = base.find(
      (s) => (s.provider_session_id || s.session_id || s.id || "") === id,
    );
    if (!hit || !Array.isArray(hit.subagents) || !hit.subagents.length) return;
    row.insertAdjacentHTML("afterend", renderSubagents(hit.subagents, true));
    subs = row.nextElementSibling;
  }
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
          s.prompt || s.title || s.name || String(id).replace(/^(?:claude|codex|antigravity)_/, "");
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
      "会话的对话文件、子代理记录与文件历史将移入 Runner 回收目录，不会立即永久清除；用量页面的历史统计不受影响。最近 1 分钟内仍在写入的活跃会话将被跳过。";
  }
  const btn = document.getElementById("btnDeleteSessions");
  if (btn) {
    btn.disabled = false;
    btn.classList.remove("busy");
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
    const resp = await localApiPost("/api/sessions/delete", { ids });
    if (!resp.ok) {
      // nothing was done, so nothing the reader ticked is spent: leave the
      // picks and the confirm dialog as they were, and say what happened
      toast(`删除失败：服务返回 HTTP ${resp.status}`, "err");
      return;
    }
    const data = (await resp.json()) || {};
    // a refused session stays picked, so the reader can retry it
    const kept = new Set(
      (data.refused || []).flatMap((r) => {
        const s = String(r.id || "");
        return [s, s.replace(/^(?:claude|codex|antigravity)_/, "")];
      }),
    );
    for (const id of [...pickedSessions]) if (!kept.has(id)) pickedSessions.delete(id);
    closeDeleteSessions();
    if ((data.deleted || []).length) {
      sessionsJustDeleted = true;
      dashboardEtag = "";
      toast(
        data.deleted.length === 1
          ? "会话已移入回收目录"
          : `${data.deleted.length} 个会话已移入回收目录`,
      );
    }
    if ((data.refused || []).length) {
      const first = data.refused[0];
      toast(
        `有 ${data.refused.length} 个会话未删除：${first.error || "正在使用中"}`,
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
      btn.disabled = false;
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
  const base = dedupeSessions(
    lastGoodSessions.length ? lastGoodSessions : allSessionsData,
  );
  if (!q) {
    if (badge && base.length)
      badge.textContent = `${base.length.toLocaleString()} 个历史会话`;
    renderSessions(lastGoodSessions.length ? lastGoodSessions : allSessionsData);
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
  const btn = document.getElementById("setRefreshModels") || document.getElementById("btnRefreshModels");
  if (btn) {
    btn.disabled = true;
    btn.classList.add("busy");
  }
  try {
    const resp = await localApiPost("/api/models/refresh");
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

function getWorkspaceAvatarHtml(ws) {
  const name = ws.name || pathLeaf(ws.path) || "workspace";
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = (hash * 31 + name.charCodeAt(i)) & 0xffffffff;
  }
  const colorIndex = Math.abs(hash) % 6;
  const isGit = Boolean(ws.is_git);

  const iconSvg = isGit
    ? `<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <circle cx="5" cy="5.5" r="2.2"/>
        <circle cx="5" cy="14.5" r="2.2"/>
        <circle cx="15" cy="9" r="2.2"/>
        <path d="M5 7.7v4.6M7 5.5h2.5a3.5 3.5 0 0 1 3.5 3.5"/>
      </svg>`
    : `<svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M2.5 5.5a2 2 0 0 1 2-2h3.2a2 2 0 0 1 1.4.6l1.3 1.4h5.1a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-11a2 2 0 0 1-2-2v-9z"/>
        <path d="m7.5 11.5-2-2 2-2m5-2 2 2-2 2"/>
      </svg>`;

  return `<div class="ws-avatar color-${colorIndex}" title="${isGit ? 'Git 项目工作区' : '项目工作区'}">${iconSvg}</div>`;
}

function renderWorkspaces(workspaces, mode = "data") {
  const container = document.getElementById("workspacesList");
  const countBadge = document.getElementById("workspacesCountBadge");
  if (mode === "loading") {
    if (countBadge) countBadge.textContent = "正在加载…";
    if (container) container.innerHTML = renderWorkspaceSkeleton(5);
    return;
  }
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
  const exactPathMap = new Map();
  const exactNameMap = new Map();
  const wsLookup = workspaces.map((ws) => {
    const wp = (ws.path || "").toLowerCase();
    const wn = (ws.name || "").toLowerCase();
    if (wp && !exactPathMap.has(wp)) exactPathMap.set(wp, ws.path);
    if (wn && !exactNameMap.has(wn)) exactNameMap.set(wn, ws.path);
    return { path: ws.path, wp, wn };
  });
  (allSessionsData || []).forEach((s) => {
    const rawPath = (s.workspace_path || s.cwd || "").toLowerCase();
    const rawName = (s.workspace_name || "").toLowerCase();
    const pt = s.prompt_tokens || 0;
    const ct = s.completion_tokens || 0;
    const tt = pt + ct || s.total_tokens || 0;

    let matchedPath =
      (rawPath && exactPathMap.get(rawPath)) ||
      (rawName && exactNameMap.get(rawName));
    if (!matchedPath && rawPath) {
      for (const item of wsLookup) {
        if (
          item.wp &&
          (rawPath.endsWith(item.wp) || item.wp.endsWith(rawPath))
        ) {
          matchedPath = item.path;
          break;
        }
      }
    }
    if (matchedPath) {
      wsStats[matchedPath] = (wsStats[matchedPath] || 0) + tt;
      if (wsStats[matchedPath] > maxWsTokens) maxWsTokens = wsStats[matchedPath];
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
        ${getWorkspaceAvatarHtml(ws)}
        <div class="who">
          <div class="name-line">
            <span class="name" title="${safeName}">${safeName}</span>
            ${gitBadge}
          </div>
          <div class="sub mono ws-loc"><span class="ws-parent">${escapeHtml(shortParent(ws.path))}</span><span class="ws-leaf">${escapeHtml(pathLeaf(ws.path))}</span></div>
        </div>
        <div class="share" title="${pct}% 活跃度">
          <i style="width: ${Math.max(tokens > 0 ? 2 : count ? 5 : 0, pct)}%;"></i>
        </div>
        <div class="num">
          <b>${tokens > 0 ? fmtN(tokens) : "—"}</b>
          <small>${count} 会话</small>
        </div>
        <div class="ws-time-col">
          <span class="ws-time-val">${relativeTime}</span>
          <span class="ws-time-lbl">最后活跃</span>
        </div>
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

