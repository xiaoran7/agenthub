// ---- the request table ---------------------------------------------------
function setLedgerSubView(view) {
  if (!["logs", "providers", "models"].includes(view)) view = "logs";
  ledSubView = view;
  ledPageLimit = LED_PAGE_STEP;
  pickUsageSegs();
  closeAllLedgerMenus();
  const colPicker = document.getElementById("ledColPickerWrap");
  if (colPicker) colPicker.hidden = view !== "logs";
  const filterPicker = document.getElementById("ledFilterPickerWrap");
  if (filterPicker) filterPicker.hidden = view !== "logs";
  renderLedgerRequests();
}

function closeAllLedgerMenus() {
  const filterMenu = document.getElementById("ledFilterMenu");
  const filterBtn = document.getElementById("ledFilterBtn");
  if (filterMenu) filterMenu.hidden = true;
  if (filterBtn) filterBtn.setAttribute("aria-expanded", "false");

  const colsMenu = document.getElementById("ledColsMenu");
  const colsBtn = document.getElementById("ledColsBtn");
  if (colsMenu) colsMenu.hidden = true;
  if (colsBtn) colsBtn.setAttribute("aria-expanded", "false");
}

function toggleLedgerColsMenu(ev) {
  if (ev) ev.stopPropagation();
  const colsMenu = document.getElementById("ledColsMenu");
  const colsBtn = document.getElementById("ledColsBtn");
  if (!colsMenu || !colsBtn) return;
  const opening = colsMenu.hidden;
  closeAllLedgerMenus();
  if (!opening) return;
  renderLedgerColsMenu();
  colsMenu.hidden = false;
  colsBtn.setAttribute("aria-expanded", "true");
}

function renderLedgerColsMenu() {
  const colsMenu = document.getElementById("ledColsMenu");
  if (!colsMenu) return;
  colsMenu.innerHTML =
    `<div class="led-pop-head"><span>显示列</span><button type="button" onclick="resetLedgerColumns(event)">恢复默认</button></div>` +
    LED_COLUMNS.map((col) => {
      const on = ledVisibleCols.has(col.id);
      return `<button type="button" class="led-pop-item${on ? " on" : ""}" data-col="${col.id}">
        <span class="led-pop-chk">${on ? "✓" : ""}</span>
        <span class="led-pop-lbl">${escapeHtml(col.label)}</span>
      </button>`;
    }).join("");
  colsMenu.onclick = (e) => {
    e.stopPropagation();
    const item = e.target.closest("button[data-col]");
    if (!item) return;
    const id = item.dataset.col;
    if (ledVisibleCols.has(id)) {
      if (ledVisibleCols.size <= 2) {
        toast("至少保留 2 列显示");
        return;
      }
      ledVisibleCols.delete(id);
    } else {
      ledVisibleCols.add(id);
    }
    try {
      localStorage.setItem(USAGE_COLS_KEY, JSON.stringify([...ledVisibleCols]));
    } catch (err) {}
    renderLedgerColsMenu();
    renderLedgerRequests();
  };
}

function resetLedgerColumns(ev) {
  if (ev) ev.stopPropagation();
  ledVisibleCols = new Set(
    LED_COLUMNS.filter((c) => c.defaultOn).map((c) => c.id),
  );
  try {
    localStorage.setItem(USAGE_COLS_KEY, JSON.stringify([...ledVisibleCols]));
  } catch (err) {}
  renderLedgerColsMenu();
  renderLedgerRequests();
}

function toggleLedgerFilterMenu(ev) {
  if (ev) ev.stopPropagation();
  const filterMenu = document.getElementById("ledFilterMenu");
  const filterBtn = document.getElementById("ledFilterBtn");
  if (!filterMenu || !filterBtn) return;
  const opening = filterMenu.hidden;
  closeAllLedgerMenus();
  if (!opening) return;
  renderLedgerFilterMenu();
  filterMenu.hidden = false;
  filterBtn.setAttribute("aria-expanded", "true");
}

function ledgerFilterOptions(kind) {
  const rows = (usageDigest && usageDigest.recent) || [];
  const counts = new Map();
  if (kind === "agent") {
    const seen = new Map([["", "全部"]]);
    for (const r of rows) {
      if (r.agent) {
        counts.set(r.agent, (counts.get(r.agent) || 0) + 1);
        if (!seen.has(r.agent)) seen.set(r.agent, providerLabel(r.agent));
      }
    }
    const byAgent = (usageDigest && usageDigest.by && usageDigest.by.agent) || [];
    for (const a of byAgent) {
      if (a.id && !seen.has(a.id)) {
        seen.set(a.id, a.name || providerLabel(a.id));
      }
    }
    return [...seen.entries()].map(([k, lbl]) => [
      k,
      lbl,
      k ? counts.get(k) || 0 : rows.length,
    ]);
  }
  if (kind === "provider") {
    const seen = new Map([["", "全部"]]);
    for (const r of rows) {
      const p = r.provider || "";
      if (p) {
        counts.set(p, (counts.get(p) || 0) + 1);
        if (!seen.has(p)) seen.set(p, p);
      }
    }
    const byProv = (usageDigest && usageDigest.by && usageDigest.by.provider) || [];
    for (const p of byProv) {
      if (p.id && !seen.has(p.id)) seen.set(p.id, p.name || p.id);
    }
    return [...seen.entries()].map(([k, lbl]) => [
      k,
      lbl,
      k ? counts.get(k) || 0 : rows.length,
    ]);
  }
  if (kind === "model") {
    const seen = new Map([["", "全部"]]);
    for (const r of rows) {
      if (r.model) {
        counts.set(r.model, (counts.get(r.model) || 0) + 1);
        if (!seen.has(r.model)) seen.set(r.model, r.model);
      }
    }
    const byModel = (usageDigest && usageDigest.by && usageDigest.by.model) || [];
    for (const m of byModel) {
      if (m.id && !seen.has(m.id)) seen.set(m.id, m.name || m.id);
    }
    return [...seen.entries()].map(([k, lbl]) => [
      k,
      lbl,
      k ? counts.get(k) || 0 : rows.length,
    ]);
  }
  if (kind === "status") {
    let okCnt = 0;
    let failCnt = 0;
    let retryCnt = 0;
    for (const r of rows) {
      if (r.error && !r.recovered) failCnt++;
      else okCnt++;
      if (r.retried) retryCnt++;
    }
    const mapCnt = { "": rows.length, ok: okCnt, failed: failCnt, retried: retryCnt };
    return Object.entries(LED_STATUSES).map(([k, lbl]) => [k, lbl, mapCnt[k] || 0]);
  }
  if (kind === "tier") {
    return Object.entries(LED_TIERS).map(([k, lbl]) => [k, lbl, null]);
  }
  return [["", "全部", rows.length]];
}

function currentLedgerFilterValue(kind) {
  if (kind === "agent") return ledFilterAgent;
  if (kind === "provider") return ledFilterProvider;
  if (kind === "model") return ledFilterModel;
  if (kind === "status") return ledStatus;
  if (kind === "tier") return ledFilterTier;
  return "";
}

function currentLedgerFilterLabel(kind) {
  const cur = currentLedgerFilterValue(kind);
  if (!cur) return "全部";
  const opts = ledgerFilterOptions(kind);
  const hit = opts.find(([k]) => k === cur);
  return hit ? hit[1] : cur;
}

function setLedgerFilterValue(kind, val) {
  if (kind === "agent") ledFilterAgent = val;
  else if (kind === "provider") ledFilterProvider = val;
  else if (kind === "model") ledFilterModel = val;
  else if (kind === "status") ledStatus = val;
  else if (kind === "tier") ledFilterTier = val;
  ledPageLimit = LED_PAGE_STEP;
  updateLedgerFilterUi();
  renderLedgerRequests();
}

function renderLedgerFilterMenu() {
  const menu = document.getElementById("ledFilterMenu");
  if (!menu) return;
  const activeCat = ledActiveFilterCat || "agent";
  const options = ledgerFilterOptions(activeCat);
  const curVal = currentLedgerFilterValue(activeCat);

  const catsHtml = LED_FILTER_CATS.map(([kind, label]) => {
    const val = currentLedgerFilterValue(kind);
    const valLabel = currentLedgerFilterLabel(kind);
    const isOn = kind === activeCat;
    return `<button type="button" class="led-cascade-cat${isOn ? " on" : ""}${val ? " filtered" : ""}" data-cat="${kind}">
      <span class="led-cascade-cat-main">
        <span class="led-cascade-cat-lbl">${escapeHtml(label)}</span>
        <span class="led-cascade-cat-val">${escapeHtml(valLabel)}</span>
      </span>
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 4 4 4-4 4"/></svg>
    </button>`;
  }).join("");

  const optsHtml = options
    .map(([val, label, cnt]) => {
      const on = val === curVal;
      return `<button type="button" class="led-pop-item${on ? " on" : ""}" data-fval="${escapeHtml(val)}">
        <span class="led-pop-chk">${on ? "✓" : ""}</span>
        <span class="led-pop-lbl" title="${escapeHtml(label)}">${escapeHtml(label)}</span>
        ${cnt != null ? `<span class="led-pop-cnt">${cnt}</span>` : ""}
      </button>`;
    })
    .join("");

  menu.innerHTML = `
    <div class="led-pop-head">
      <span>多维嵌套筛选</span>
      <button type="button" onclick="resetLedgerFilters(event)">重置全部</button>
    </div>
    <div class="led-cascade-body">
      <div class="led-cascade-cats">${catsHtml}</div>
      <div class="led-cascade-opts">${optsHtml}</div>
    </div>
  `;

  const catsBox = menu.querySelector(".led-cascade-cats");
  if (catsBox) {
    const switchCat = (e) => {
      const btn = e.target.closest("button[data-cat]");
      if (!btn || btn.dataset.cat === ledActiveFilterCat) return;
      ledActiveFilterCat = btn.dataset.cat;
      renderLedgerFilterMenu();
    };
    catsBox.onmouseover = switchCat;
    catsBox.onclick = (e) => {
      e.stopPropagation();
      switchCat(e);
    };
  }
  const optsBox = menu.querySelector(".led-cascade-opts");
  if (optsBox) {
    optsBox.onclick = (e) => {
      e.stopPropagation();
      const btn = e.target.closest("button[data-fval]");
      if (!btn) return;
      setLedgerFilterValue(activeCat, btn.dataset.fval || "");
      renderLedgerFilterMenu();
    };
  }
}

function updateLedgerFilterUi() {
  const activeFilters = [];
  for (const [kind, catLabel] of LED_FILTER_CATS) {
    const val = currentLedgerFilterValue(kind);
    if (val) {
      activeFilters.push([kind, catLabel, currentLedgerFilterLabel(kind)]);
    }
  }
  const filterBtn = document.getElementById("ledFilterBtn");
  const filterBadge = document.getElementById("ledFilterBadge");
  if (filterBtn) {
    filterBtn.classList.toggle("active", activeFilters.length > 0);
  }
  if (filterBadge) {
    filterBadge.hidden = activeFilters.length === 0;
    filterBadge.textContent = String(activeFilters.length);
  }
  const chipsWrap = document.getElementById("ledActiveChips");
  if (chipsWrap) {
    if (!activeFilters.length && !ledQuery) {
      chipsWrap.hidden = true;
      chipsWrap.innerHTML = "";
    } else {
      chipsWrap.hidden = false;
      chipsWrap.innerHTML =
        activeFilters
          .map(
            ([kind, catLabel, valLabel]) =>
              `<button type="button" class="led-chip" onclick="setLedgerFilterValue('${kind}', '')" title="点击移除此筛选条件">
                <b>${escapeHtml(catLabel)}:</b>
                <span>${escapeHtml(valLabel)}</span>
                <i aria-hidden="true">×</i>
              </button>`,
          )
          .join("") +
        `<button type="button" class="led-chip-reset" onclick="resetLedgerFilters(event)">清除全部筛选</button>`;
    }
  }
}

function resetLedgerFilters(ev) {
  if (ev) ev.stopPropagation();
  ledFilterAgent = "";
  ledFilterProvider = "";
  ledFilterModel = "";
  ledStatus = "";
  ledFilterTier = "";
  ledQuery = "";
  ledPageLimit = LED_PAGE_STEP;
  const q = document.getElementById("ledQ");
  if (q) q.value = "";
  updateLedgerFilterUi();
  const filterMenu = document.getElementById("ledFilterMenu");
  if (filterMenu && !filterMenu.hidden) {
    renderLedgerFilterMenu();
  }
  renderLedgerRequests();
}

// The rows the period holds, then the page's own multi-dimensional filters and search.
function ledFilteredRows() {
  const rows = (usageDigest && usageDigest.recent) || [];
  return rows.filter((r) => {
    if (ledFilterAgent && r.agent !== ledFilterAgent) return false;
    if (ledFilterProvider && (r.provider || "") !== ledFilterProvider) return false;
    if (ledFilterModel && (r.model || "") !== ledFilterModel) return false;
    if (ledStatus === "ok" && (r.error && !r.recovered)) return false;
    if (ledStatus === "failed" && !(r.error && !r.recovered)) return false;
    if (ledStatus === "retried" && !r.retried) return false;
    if (ledFilterTier === "high_tokens") {
      const total =
        (r.input || 0) + (r.output || 0) + (r.cache_read || 0) + (r.cache_write || 0);
      if (total < 10000) return false;
    } else if (ledFilterTier === "high_cost") {
      if ((Number(r.cost) || 0) < 0.05) return false;
    } else if (ledFilterTier === "slow") {
      if ((Number(r.duration_ms) || 0) < 5000) return false;
    } else if (ledFilterTier === "cached") {
      if ((r.cache_read || 0) <= 0) return false;
    }
    if (!ledQuery) return true;
    const text = `${r.model || ""} ${r.provider || ""} ${r.agent || ""} ${providerLabel(r.agent)} ${r.session || ""} ${r.cwd || ""} ${r.error || ""}`.toLowerCase();
    return text.includes(ledQuery);
  });
}

function ledStatusCell(r) {
  if (r.retried) return `<span class="tag ok">200 · 重试</span>`;
  if (r.error && !r.recovered)
    return `<span class="tag err" title="${escapeHtml(r.error)}">失败</span>`;
  if (r.error) return `<span class="tag warn" title="${escapeHtml(r.error)}">已恢复</span>`;
  return `<span class="tag ok">200</span>`;
}

function fmtDurationMs(ms) {
  const n = Number(ms);
  if (!Number.isFinite(n) || n <= 0) return "—";
  if (n < 1000) return `${Math.round(n)}ms`;
  return `${(n / 1000).toFixed(n >= 10000 ? 1 : 2)}s`;
}

function renderLedgerColumnCell(colId, r) {
  switch (colId) {
    case "time":
      return `<td class="when">${escapeHtml(fmtWhen(r.t, usagePeriod !== "today"))}</td>`;
    case "agent":
      return `<td class="muted">${getAgentIcon(r.agent, 14)} ${escapeHtml(providerLabel(r.agent))}</td>`;
    case "provider":
      return `<td class="muted"><span class="led-prov-pill">${escapeHtml(r.provider || "直连")}</span></td>`;
    case "model":
      return `<td class="model-value">${r.model ? getModelIcon(r.model, 14) + escapeHtml(r.model) : "—"}</td>`;
    case "input":
      return `<td class="n">${fmtN(r.input || 0)}</td>`;
    case "output":
      return `<td class="n">${fmtN(r.output || 0)}</td>`;
    case "cache_read":
      return `<td class="n">${fmtN(r.cache_read || 0)}</td>`;
    case "cache_write":
      return `<td class="n">${fmtN(r.cache_write || 0)}</td>`;
    case "total_tokens": {
      const tot =
        (r.input || 0) + (r.output || 0) + (r.cache_read || 0) + (r.cache_write || 0);
      return `<td class="n">${fmtN(tot)}</td>`;
    }
    case "cost":
      return `<td class="n">${fmtMoney(r.cost || 0)}</td>`;
    case "duration":
      return `<td class="n muted">${fmtDurationMs(r.duration_ms)}</td>`;
    case "status":
      return `<td>${ledStatusCell(r)}</td>`;
    default:
      return `<td>—</td>`;
  }
}

