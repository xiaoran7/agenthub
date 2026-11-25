function renderLedgerBreakdownTable(kind, wrap, badge) {
  const list =
    (usageDigest &&
      usageDigest.by &&
      (kind === "providers" ? usageDigest.by.provider : usageDigest.by.model)) ||
    [];
  const filtered = list.filter((g) => {
    if (!ledQuery) return true;
    const text = `${g.name || ""} ${g.id || ""} ${g.agent || ""}`.toLowerCase();
    return text.includes(ledQuery);
  });
  if (badge) {
    badge.textContent =
      kind === "providers"
        ? `共 ${filtered.length} 个供应商`
        : `共 ${filtered.length} 个模型`;
  }
  if (!filtered.length) {
    wrap.innerHTML = `<div class="led-none">本周期没有匹配的${kind === "providers" ? "供应商" : "模型"}聚合记录</div>`;
    return;
  }
  const titleCol = kind === "providers" ? "供应商" : "模型";
  wrap.innerHTML = `
    <table class="led">
      <thead><tr>
        <th>${titleCol}</th>
        <th class="n">请求数</th>
        <th class="n">输入 Token</th>
        <th class="n">输出 Token</th>
        <th class="n">缓存读取</th>
        <th class="n">总计 Token</th>
        <th class="n">成本</th>
        <th>成功率</th>
      </tr></thead>
      <tbody>${filtered
        .map((g) => {
          const calls = g.calls || 0;
          const errs = g.errors || 0;
          const okRate =
            calls > 0 ? Math.max(0, Math.round(((calls - errs) * 100) / calls)) : 100;
          const labelCell =
            kind === "models"
              ? `${getModelIcon(g.name || g.id, 14)}${escapeHtml(g.name || g.id)}`
              : `<span class="led-prov-pill">${escapeHtml(g.name || g.id)}</span>`;
          return `<tr>
            <td class="model-value">${labelCell}</td>
            <td class="n">${calls.toLocaleString()}</td>
            <td class="n">${fmtN(g.input || 0)}</td>
            <td class="n">${fmtN(g.output || 0)}</td>
            <td class="n">${fmtN(g.cache_read || 0)}</td>
            <td class="n">${fmtN(g.tokens || 0)}</td>
            <td class="n">${fmtMoney(g.cost || 0)}</td>
            <td><span class="tag ${errs ? "warn" : "ok"}">${okRate}%</span></td>
          </tr>`;
        })
        .join("")}</tbody>
    </table>`;
}

function loadMoreLedgerRows() {
  ledPageLimit += LED_PAGE_STEP;
  renderLedgerRequests();
}

function renderLedgerRequests() {
  const wrap = document.getElementById("ledWrap");
  const badge = document.getElementById("ledCount");
  if (!wrap) return;
  updateLedgerFilterUi();
  const kept = wrap.scrollTop;
  if (!usageDigest) {
    wrap.innerHTML = `<div class="led-none">正在读取用量…</div>`;
    return;
  }
  if (ledSubView === "providers" || ledSubView === "models") {
    renderLedgerBreakdownTable(ledSubView, wrap, badge);
    wrap.scrollTop = kept;
    return;
  }
  const rows = ledFilteredRows();
  const anyFilter = Boolean(
    ledFilterAgent ||
      ledFilterProvider ||
      ledFilterModel ||
      ledStatus ||
      ledFilterTier ||
      ledQuery,
  );
  if (badge) {
    const totalCount =
      !anyFilter && usageDigest && usageDigest.calls > rows.length
        ? usageDigest.calls
        : rows.length;
    badge.textContent = `共 ${totalCount.toLocaleString()} 条记录`;
  }
  if (!rows.length) {
    wrap.innerHTML = `<div class="led-none">${
      anyFilter
        ? "最近的请求里没有符合筛选条件的记录"
        : "本周期没有请求"
    }</div>`;
    if (ledStatus === "failed" && usageDigest.errors) {
      wrap.firstElementChild.textContent += `（本周期共 ${usageDigest.errors} 次失败）`;
    }
    return;
  }
  const activeCols = LED_COLUMNS.filter((c) => ledVisibleCols.has(c.id));
  const slice = rows.slice(0, ledPageLimit);
  const moreHtml =
    rows.length > slice.length
      ? `<div class="led-more-bar">
           <button type="button" class="text action" onclick="loadMoreLedgerRows()">
             <span>加载更多（已显示 ${slice.length.toLocaleString()} / ${rows.length.toLocaleString()} 条）</span>
           </button>
         </div>`
      : "";
  wrap.innerHTML = `
    <table class="led">
      <thead><tr>
        ${activeCols
          .map(
            (c) =>
              `<th${c.numeric ? ' class="n"' : ""}>${escapeHtml(c.label)}</th>`,
          )
          .join("")}
      </tr></thead>
      <tbody>${slice
        .map((r) => {
          const failed = r.error && !r.recovered;
          return `<tr${failed ? ' class="bad"' : ""}>${activeCols
            .map((c) => renderLedgerColumnCell(c.id, r))
            .join("")}</tr>`;
        })
        .join("")}</tbody>
    </table>${moreHtml}`;
  wrap.scrollTop = kept;
}

function loadMoreLedgerSessions() {
  ledSessionPageLimit += LED_PAGE_STEP;
  renderLedgerSessions();
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
  const slice = rows.slice(0, ledSessionPageLimit);
  const moreHtml =
    rows.length > slice.length
      ? `<div class="led-more-bar">
           <button type="button" class="text action" onclick="loadMoreLedgerSessions()">
             <span>加载更多会话（已显示 ${slice.length.toLocaleString()} / ${rows.length.toLocaleString()} 个）</span>
           </button>
         </div>`
      : "";
  wrap.innerHTML = `
    <table class="led">
      <thead><tr>
        <th>最后活动</th><th>智能体</th><th>模型</th><th>目录</th><th class="n">请求</th><th class="n">Token</th><th class="n">费用</th><th>状态</th>
      </tr></thead>
      <tbody>${slice
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
    </table>${moreHtml}`;
  wrap.scrollTop = kept;
}

function renderLedgerContext() {
  const wrap = document.getElementById("usageContext");
  if (!wrap) return;
  const kept = wrap.scrollTop;
  const rows = ((usageDigest && usageDigest.session_rows) || [])
    .filter((s) => s.context > 0)
    .sort((a, b) => b.context - a.context)
    .slice(0, 120);
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

let lastExportedCsvPath = "";

async function revealExportedCsv() {
  if (!lastExportedCsvPath) return;
  try {
    const resp = await localApiPost("/api/reveal-path", { path: lastExportedCsvPath });
    if (resp.ok) {
      toast("已在资源管理器中定位该 CSV 文件");
    } else {
      toast("无法打开资源管理器", "err");
    }
  } catch (e) {
    toast("无法连接本地服务", "err");
  }
}

function copyExportedCsvPath() {
  if (!lastExportedCsvPath) return;
  copyText(lastExportedCsvPath);
  toast("已复制 CSV 文件完整路径");
}

function hideExportNotice() {
  const notice = document.getElementById("ledExportNotice");
  if (notice) notice.hidden = true;
}

// The CSV is saved directly to the user's Downloads folder by the daemon so
// the exact disk path can be shown and revealed in File Explorer.
async function exportLedgerCsv() {
  if (!usageDigest) return;
  const rows = ledFilteredRows();
  if (!rows.length) {
    toast("当前筛选没有可导出的请求", "err");
    return;
  }
  const head = [
    "time",
    "agent",
    "provider",
    "model",
    "session",
    "cwd",
    "input",
    "output",
    "cache_read",
    "cache_write",
    "cost_usd",
    "duration_ms",
    "status",
    "error",
  ];
  const cell = (v) => {
    let s = String(v == null ? "" : v);
    if (/^[=+\-@\t\r]/.test(s)) {
      s = `'${s}`;
    }
    return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
  };
  const body = rows.map((r) => {
    const status = r.error ? (r.recovered ? "recovered" : "failed") : r.retried ? "retried" : "ok";
    return [
      new Date(r.t).toISOString(),
      r.agent,
      r.provider || "",
      r.model,
      r.session,
      r.cwd,
      r.input,
      r.output,
      r.cache_read,
      r.cache_write,
      (Number(r.cost) || 0).toFixed(6),
      r.duration_ms != null ? r.duration_ms : "",
      status,
      r.error || "",
    ]
      .map(cell)
      .join(",");
  });
  const filename = `agenthub-usage-${usagePeriod}-${heatISO(new Date())}.csv`;
  const csv = "﻿" + [head.join(","), ...body].join("\r\n");

  try {
    const resp = await localApiPost("/api/usage/export-csv", { filename, csv });
    if (resp.ok) {
      const res = await resp.json();
      if (res && res.ok && res.path) {
        lastExportedCsvPath = res.path;
        copyText(res.path);
        const notice = document.getElementById("ledExportNotice");
        if (notice) {
          notice.hidden = false;
          notice.innerHTML = `
            <span>已导出 <b>${rows.length.toLocaleString()}</b> 条记录至：</span>
            <code class="path">${escapeHtml(res.path)}</code>
            <button type="button" class="text action" onclick="revealExportedCsv()">在文件夹中定位</button>
            <button type="button" class="text action" onclick="copyExportedCsvPath()">复制路径</button>
            <button type="button" class="text action" onclick="hideExportNotice()" aria-label="关闭提示">×</button>
          `;
        }
        toast(`已导出至 ${res.path}（路径已复制）`);
        return;
      }
    }
  } catch (e) {}

  // Fallback for standalone preview
  const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
  toast(`已导出 ${rows.length.toLocaleString()} 条请求至浏览器默认下载目录 (${filename})`);
}

