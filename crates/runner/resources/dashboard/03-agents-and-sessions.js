const AGENT_ICONS = {
  claude_code: "claudecode-color",
  coding_assistant: "claudecode-color",
  claude_desktop: "claude-color",
  claude: "claude-color",
  codex: "codex-color",
  antigravity: "antigravity-color",
  deepseek_harness: "deepseek-color",
};

// magpie's family table (internal/agent/icons.go): a model id's prefix names
// its vendor's mark, first match wins.
const MODEL_FAMILIES = [
  ["claude", "claude-color"], ["sonnet", "claude-color"], ["opus", "claude-color"], ["haiku", "claude-color"], ["fable", "claude-color"], ["mythos", "claude-color"],
  ["codex", "codex-color"],
  ["gpt", "openai"], ["o1", "openai"], ["o3", "openai"], ["o4", "openai"],
  ["gemini", "gemini-color"], ["gemma", "gemini-color"],
  ["deepseek", "deepseek-color"],
  ["grok", "xai"],
  ["kimi", "kimi"], ["moonshot", "kimi"],
  ["glm", "zhipu-color"], ["chatglm", "chatglm-color"], ["zhipu", "zhipu-color"],
  ["qwen", "qwen-color"], ["qwq", "qwen-color"],
  ["mistral", "mistral-color"], ["codestral", "mistral-color"], ["devstral", "mistral-color"], ["magistral", "mistral-color"],
  ["minimax", "minimax-color"], ["abab", "minimax-color"],
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
  const hit =
    MODEL_FAMILIES.find(([prefix]) => id.startsWith(prefix)) ||
    MODEL_FAMILIES.find(([prefix]) => prefix.length >= 3 && id.includes(prefix));
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

function getAgentIcon(id, size = 18, iconUrl = "") {
  const key = String(id || "").toLowerCase().trim();
  const dyn = dynamicPluginMeta.get(key);
  const resolvedUrl = iconUrl || (dyn && dyn.iconUrl) || "";
  if (resolvedUrl) {
    return `<img class="mark" src="${escapeHtml(resolvedUrl)}" width="${size}" height="${size}" alt="${escapeHtml(providerLabel(key))}" loading="lazy" onerror="this.style.display='none'">`;
  }
  return iconMark(AGENT_ICONS[key] || "", size, providerLabel(key));
}

function getModelIcon(name, size = 18) {
  return iconMark(modelIconName(name), size, name);
}

function renderAgents(agents, mode = "data") {
  const container = document.getElementById("agentsGrid");
  const countBadge = document.getElementById("agentsCountBadge");
  if (mode === "loading") {
    if (countBadge) countBadge.textContent = "正在加载…";
    if (container) container.innerHTML = renderAgentSkeleton(4);
    return;
  }
  if (countBadge) {
    const count = agents ? agents.length : 0;
    countBadge.textContent = `${count} 个智能体`;
  }
  if (!container) return;

  if (!agents || agents.length === 0) {
    container.innerHTML = `<div class="empty-state"><b>没有发现可用的智能体</b><span class="why">安装 Coding Assistant、Codex 或前往设置 › 插件启用外部扩展，刷新后即可在这里看到。</span></div>`;
    return;
  }

  container.innerHTML = agents
    .map((agent) => {
      const isPlugin = !!agent.is_plugin;
      const isReady = isPlugin
        ? !!agent.available || agent.sessions_count > 0
        : agent.installed || agent.sessions_count > 0;
      let tagHtml;
      if (isPlugin) {
        tagHtml = agent.available
          ? `<span class="tag ok">已就绪</span>`
          : `<span class="tag warn">${escapeHtml(agent.runtime_status || "未检测到运行时")}</span>`;
      } else if (agent.id === "deepseek_harness" && !agent.installed) {
        tagHtml = `<span class="tag">待接入</span>`;
      } else {
        tagHtml = isReady
          ? `<span class="tag ok">已就绪</span>`
          : `<span class="tag">未检测到</span>`;
      }
      const sourceTag = isPlugin
        ? `<span class="tag ok">ACP 插件${agent.plugin_version ? ` v${escapeHtml(agent.plugin_version)}` : ""}</span>`
        : `<span class="tag">原生</span>`;
      const pathText = agent.path
        ? escapeHtml(agent.path)
        : "未找到独立可执行文件，可通过命令别名调用";

      return `
      <div class="row">
        <span class="ic">${getAgentIcon(agent.id, 22, agent.icon_url || "")}</span>
        <div class="who">
          <div class="name-line">
            <span class="name">${escapeHtml(agent.name)}</span>
            <span class="tag mono">${escapeHtml(agent.command || agent.id)}</span>
            ${sourceTag}
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

function renderAgentSkeleton(bars = 4) {
  const nameWidths = ["120px", "140px", "110px", "130px"];
  const descWidths = ["65%", "75%", "50%", "60%"];
  const rows = [];
  for (let i = 0; i < bars; i++) {
    const isLast = i === bars - 1;
    rows.push(`
      <div class="row" style="display: flex; align-items: center; gap: 14px; min-height: 52px; ${isLast ? "" : "border-bottom: var(--rule-2);"}">
        <div class="skeleton-bar" style="width: 26px; height: 26px; border-radius: 6px; flex: none;"></div>
        <div class="who" style="flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 6px;">
          <div style="display: flex; align-items: center; gap: 8px;">
            <div class="skeleton-bar" style="width: ${nameWidths[i % nameWidths.length]}; height: 15px;"></div>
            <div class="skeleton-bar" style="width: 60px; height: 15px; border-radius: 4px;"></div>
            <div class="skeleton-bar" style="width: 48px; height: 15px; border-radius: 4px;"></div>
          </div>
          <div class="skeleton-bar" style="width: ${descWidths[i % descWidths.length]}; height: 11px;"></div>
        </div>
        <div class="skeleton-bar" style="width: 64px; height: 20px; border-radius: 4px; flex: none;"></div>
      </div>
    `);
  }
  return `<div class="skeleton-card" style="padding: 0 16px; gap: 0;" aria-busy="true" aria-label="正在加载智能体…">${rows.join("")}</div>`;
}

function renderWorkspaceSkeleton(bars = 5) {
  const nameWidths = ["48%", "60%", "42%", "55%", "38%"];
  const pathWidths = ["75%", "65%", "80%", "70%", "60%"];
  const rows = [];
  for (let i = 0; i < bars; i++) {
    const isLast = i === bars - 1;
    rows.push(`
      <div class="row stat" style="display: grid; grid-template-columns: 32px minmax(0, 1fr) 112px 92px; align-items: center; gap: 16px; min-height: 56px; ${isLast ? "" : "border-bottom: var(--rule-2);"}">
        <div class="skeleton-bar" style="width: 32px; height: 32px; border-radius: var(--r-s);"></div>
        <div style="min-width: 0; display: flex; flex-direction: column; gap: 6px;">
          <div class="skeleton-bar" style="width: ${nameWidths[i % nameWidths.length]}; height: 16px;"></div>
          <div class="skeleton-bar" style="width: ${pathWidths[i % pathWidths.length]}; height: 11px;"></div>
        </div>
        <div style="display: flex; flex-direction: column; align-items: flex-end; gap: 5px;">
          <div class="skeleton-bar" style="width: 58px; height: 16px;"></div>
          <div class="skeleton-bar" style="width: 44px; height: 11px;"></div>
        </div>
        <div style="display: flex; flex-direction: column; align-items: flex-end; gap: 5px;">
          <div class="skeleton-bar" style="width: 52px; height: 14px;"></div>
          <div class="skeleton-bar" style="width: 40px; height: 11px;"></div>
        </div>
      </div>
    `);
  }
  return `<div class="skeleton-card" style="padding: 0 16px; gap: 0;" aria-busy="true" aria-label="正在加载工作区…">${rows.join("")}</div>`;
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
        <span class="why">运行任意原生智能体或前往设置 › 插件启用 ACP 扩展，对话结束后会话就会出现在这里。如果刚结束的会话还没出现，稍等几秒再看。</span>
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

  const isSearching = Boolean(
    document.getElementById("sessionSearchInput")?.value?.trim(),
  );
  const allBase = isSearching
    ? dedupeSessions(lastGoodSessions.length ? lastGoodSessions : allSessionsData)
    : [];

  if (badge) {
    if (isSearching && allBase.length > 0) {
      badge.textContent = `${dedupedSessions.length.toLocaleString()} / ${allBase.length.toLocaleString()} 个历史会话`;
    } else {
      badge.textContent = `${dedupedSessions.length.toLocaleString()} 个历史会话`;
    }
  }

  // Sort sessions chronologically descending (newest activity first)
  const sortedSessions = [...dedupedSessions].sort(sessionRecency);

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

  const origWorkspaceCounts = {};
  if (isSearching && allBase.length > 0) {
    allBase.forEach((s) => {
      const key = s.workspace_path || s.cwd || "默认工作区";
      origWorkspaceCounts[key] = (origWorkspaceCounts[key] || 0) + 1;
    });
  }

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
      const origCount = origWorkspaceCounts[k] || count;
      let countLabel = `${count} 个会话`;
      let countTitle = `${count} 个会话`;
      if (isSearching && origCount > count) {
        countLabel = `${count} / ${origCount} 个会话`;
        countTitle = `筛选出 ${count} 个会话，该工作区共 ${origCount} 个`;
      }

      const limit = isSearching
        ? 120
        : sessionGroupLimits.get(k) || SESSION_GROUP_INITIAL_LIMIT;
      const visibleItems = grp.items.slice(0, limit);
      const hiddenCount = Math.max(0, grp.items.length - visibleItems.length);

      const itemsHtml = visibleItems
        .map((item, index) => {
          const rawId =
            item.provider_session_id || item.session_id || item.id || "";
          const hasId = !!rawId && rawId !== "unknown";
          const prompt = escapeHtml(
            cleanSessionTitleText(
              item.prompt ||
                item.title ||
                item.first_prompt ||
                item.name ||
                "",
            ) || "无标题会话",
          );
          const rawProvider =
            item.provider ||
            item.agent_name ||
            (rawId.startsWith("antigravity_")
              ? "antigravity"
              : rawId.startsWith("claude_")
                ? "claude_code"
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
          const deletable =
            rawProvider === "claude_code" ||
            rawProvider === "claude_desktop" ||
            rawProvider === "claude" ||
            rawProvider === "codex" ||
            rawProvider === "antigravity";
          const picked = pickedSessions.has(rawId);
          const subsClosed = !hasId || !expandedSubagents.has(rawId);
          const subHtml = subs.length && !subsClosed ? renderSubagents(subs, false) : "";
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
            : `<span class="sm-del-space" aria-hidden="true"></span>`;
          const check = deletable
            ? `<input type="checkbox" class="sm-check" ${picked ? "checked" : ""} aria-label="选择这个会话" onclick="event.stopPropagation()" onchange="pickSession(this.closest('.row.sess').dataset.id, this.checked)">`
            : `<span class="sm-check-space"></span>`;
          const animIdx = Math.min(index, 10);
          const animTotal = Math.min(visibleItems.length, 10);

          return `
        <div class="row sess${subs.length ? " has-subs" : ""}${subsClosed ? " subs-closed" : ""}" data-id="${fullId}" style="--i:${animIdx};--n:${animTotal}" title="${escapeHtml(providerLabel(rawProvider))}${fullId ? " · " + fullId : ""}${modelName ? " · " + modelName : ""}${rawTokens > 0 ? " · " + fmtN(rawTokens) + " Tokens" : ""}"${subs.length ? ` onclick="if(!event.target.closest('.sm-del, .sm-check, button, input, a')) toggleSubagents(this)"` : ""}>
          ${check}
          <div class="who">
            <div class="name" title="${prompt}">${prompt}</div>
            <div class="sub">${escapeHtml(providerLabel(rawProvider))}${subCount}${rawTokens > 0 ? ` · ${fmtN(rawTokens)} Tokens` : ""}</div>
          </div>
          ${modelName ? `<span class="model" title="${modelName}">${modelName}</span>` : ""}
          <time class="when">${escapeHtml(timeStr)}</time>
          ${deleteBtn}
        </div>
        ${subHtml}
      `;
        })
        .join("");

      const moreHtml =
        hiddenCount > 0
          ? `<button type="button" class="row sess-more" data-group-key="${escapeHtml(k)}" onclick="expandSessionGroup(this.dataset.groupKey)">
               <span>展开更多会话（剩余 ${hiddenCount} 个）</span>
             </button>`
          : "";

      return `
      <div class="list" data-group="${escapeHtml(k)}">
        <div class="group-head${closed ? " closed" : ""}" role="button" tabindex="0" aria-expanded="${!closed}" onclick="toggleSessionGroup(this)">
          <span class="ic accent sm">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
          </span>
          <span class="gname" title="${safeName}">${safeName}</span>
          <span class="gpath" title="${safePath}">${safePath}</span>
          <span class="gcount" title="${escapeHtml(countTitle)}">${escapeHtml(countLabel)}</span>
          <span class="chev">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m6 9 6 6 6-6"/></svg>
          </span>
        </div>
        <div class="group-body${closed ? " folded" : ""}" data-group-body>
          <div class="group-rows">${itemsHtml}${moreHtml}</div>
        </div>
      </div>
    `;
    })
    .join("");
  sessionsRenderedAt = Date.now();
  syncSessionPickUi();
}

const SESSION_GROUP_INITIAL_LIMIT = 40;
const SESSION_GROUP_STEP = 60;
const sessionGroupLimits = new Map();

function expandSessionGroup(groupKey) {
  if (!groupKey) return;
  const current = sessionGroupLimits.get(groupKey) || SESSION_GROUP_INITIAL_LIMIT;
  sessionGroupLimits.set(groupKey, current + SESSION_GROUP_STEP);
  filterSessionsList();
}

function cleanSessionTitleText(raw) {
  const s = String(raw || "");
  if (!s.trim()) return "";
  const cmdMatch = /<command-name>\s*([^<]+?)\s*<\/command-name>/i.exec(s);
  let cleaned = s
    .replace(/<command-message>[\s\S]*?<\/command-message>/gi, " ")
    .replace(/<command-args>([\s\S]*?)<\/command-args>/gi, " $1 ")
    .replace(/<command-name>\s*([^<]+?)\s*<\/command-name>/gi, " $1 ")
    .replace(/<\/?[a-zA-Z][a-zA-Z0-9_-]*(?:\s[^>]*)?>/g, " ")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/[\r\n\t]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (!cleaned && cmdMatch) cleaned = cmdMatch[1].trim();
  return cleaned;
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

