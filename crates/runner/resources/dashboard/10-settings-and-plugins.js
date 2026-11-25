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
  plugins: "插件 · Plugins",
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
    none.textContent = `没有匹配 "${value.trim()}" 的设置`;
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
  if (ver) ver.textContent = `AgentHub v${LAYOUT.version || "dev"}`;

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
      ? "直接读取 resources/ 目录下的页面、样式与脚本，保存后窗口自动刷新"
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
  if (lastPluginsPayload) {
    renderPluginsSection(lastPluginsPayload);
  } else if (dashboardData && dashboardData.plugins) {
    renderPluginsSection(dashboardData.plugins);
  }
}

let lastPluginsPayload = null;

function normalizePluginCaps(caps) {
  if (!caps || typeof caps !== "object") return [];
  const labels = [
    ["discover", "发现会话"],
    ["launch", "远程启动"],
    ["resume", "恢复会话"],
    ["models", "模型列表"],
    ["history", "历史消息"],
    ["approvals", "审批交互"],
  ];
  return labels.filter(([k]) => !!caps[k]).map(([, label]) => label);
}

function renderPluginRow(p, section) {
  const id = String(p.id || "");
  const name = escapeHtml(p.displayName || p.display_name || p.name || id);
  const ver = escapeHtml(p.installedVersion || p.installed_version || p.version || "");
  const desc = escapeHtml(p.description || "外部 ACP 智能体插件");
  const rtStatus = String(p.runtimeStatus || p.runtime_status || (p.available ? "ready" : "unknown"));
  const available = !!p.available;
  const enabled = !!p.enabled;
  const installed = !!p.installed;
  const hasIcon = installed && !!(p.icon || p.icon_url || p.iconUrl);
  const iconHtml = hasIcon
    ? `<img class="mark" src="/api/plugins/icon/${encodeURIComponent(id)}" width="20" height="20" alt="${name}" loading="lazy" onerror="this.style.display='none'">`
    : `<span class="mark none" style="width:20px;height:20px" aria-hidden="true"></span>`;

  const capsHtml = normalizePluginCaps(p.capabilities)
    .map((label) => `<span class="tag">${escapeHtml(label)}</span>`)
    .join("");

  let statusTag = "";
  if (installed) {
    if (!enabled) {
      statusTag = `<span class="tag">已停用</span>`;
    } else if (available) {
      statusTag = `<span class="tag ok">运行就绪</span>`;
    } else {
      statusTag = `<span class="tag warn">${escapeHtml(rtStatus)}</span>`;
    }
  } else {
    statusTag = `<span class="tag">未安装</span>`;
  }

  let actionsHtml = "";
  if (section === "installed") {
    const toggleLabel = enabled ? "停用" : "启用";
    const nextEnabled = enabled ? "false" : "true";
    actionsHtml = `
      <button type="button" class="text action" data-plugin-id="${escapeHtml(id)}" data-next-enabled="${nextEnabled}" onclick="togglePluginById(this.dataset.pluginId, this.dataset.nextEnabled === 'true')">${toggleLabel}</button>
      <button type="button" class="text action danger" data-plugin-id="${escapeHtml(id)}" onclick="uninstallPluginById(this.dataset.pluginId)">卸载</button>
    `;
  } else {
    actionsHtml = `
      <button type="button" class="text action primary" data-plugin-id="${escapeHtml(id)}" onclick="installPluginById(this.dataset.pluginId)">安装</button>
    `;
  }

  return `
    <div class="row cap-row">
      <span class="ic">${iconHtml}</span>
      <div class="who">
        <div class="name-line">
          <span class="name">${name}</span>
          <span class="tag mono">${escapeHtml(id)}</span>
          ${ver ? `<span class="tag mono">v${ver}</span>` : ""}
          ${statusTag}
        </div>
        <div class="sub flow">
          <span>${desc}</span>
          ${p.error ? `<span class="tag warn">${escapeHtml(p.error)}</span>` : ""}
        </div>
        ${capsHtml ? `<div class="plugin-caps">${capsHtml}</div>` : ""}
      </div>
      <div class="plugin-actions">${actionsHtml}</div>
    </div>
  `;
}

function renderPluginsSection(payload) {
  if (!payload) return;
  lastPluginsPayload = payload;
  const items = Array.isArray(payload.items)
    ? payload.items
    : Array.isArray(payload.plugins)
      ? payload.plugins
      : [];
  const regInput = document.getElementById("pluginRegistryInput");
  if (regInput && document.activeElement !== regInput && payload.registry_url) {
    regInput.value = payload.registry_url;
  }
  const sourceTag = document.getElementById("pluginsSourceTag");
  if (sourceTag) {
    if (payload.error) {
      sourceTag.textContent = "目录拉取失败";
      sourceTag.className = "tag warn";
    } else if (payload.refreshed) {
      sourceTag.textContent = "目录已同步";
      sourceTag.className = "tag ok";
    } else {
      sourceTag.textContent = "本地状态";
      sourceTag.className = "tag";
    }
  }

  const installed = items.filter((p) => !!p.installed);
  const available = items.filter((p) => !p.installed);

  const installedList = document.getElementById("pluginsInstalledList");
  const installedBadge = document.getElementById("pluginsInstalledBadge");
  if (installedBadge) installedBadge.textContent = `${installed.length} 个已安装`;
  if (installedList) {
    installedList.innerHTML = installed.length
      ? installed.map((p) => renderPluginRow(p, "installed")).join("")
      : `<div class="empty-state"><b>尚未安装任何 ACP 插件</b><span class="why">点击上方"刷新目录"即可从 Runner 配置的仓库安装外部智能体适配器。</span></div>`;
  }

  const catalogList = document.getElementById("pluginsCatalogList");
  const catalogBadge = document.getElementById("pluginsCatalogBadge");
  if (catalogBadge) catalogBadge.textContent = `${available.length} 个可用`;
  if (catalogList) {
    catalogList.innerHTML = available.length
      ? available.map((p) => renderPluginRow(p, "catalog")).join("")
      : `<div class="empty-state"><b>当前没有待安装的目录插件</b><span class="why">${items.length ? "目录中的插件均已安装。" : "点击\"刷新目录\"从插件仓库拉取最新清单。"}</span></div>`;
  }
}

async function refreshPluginCatalog() {
  const btn = document.getElementById("pluginRefreshCatalogBtn");
  const orig = btn ? btn.textContent : "";
  if (btn) {
    btn.disabled = true;
    btn.textContent = "正在同步…";
  }
  try {
    const resp = await localApiPost("/api/plugins/catalog", {});
    const data = await resp.json();
    data.refreshed = resp.ok && data.ok !== false;
    renderPluginsSection(data);
    if (data.ok !== false) {
      toast("已从插件仓库同步最新目录");
    } else {
      toast("目录拉取失败：" + (data.error || "未知错误"), "err");
    }
  } catch (err) {
    toast("目录拉取失败：" + err.message, "err");
  } finally {
    if (btn) {
      btn.disabled = false;
      btn.textContent = orig;
    }
  }
}

async function savePluginRegistry() {
  const regInput = document.getElementById("pluginRegistryInput");
  const registry_url = regInput ? regInput.value.trim() : "";
  try {
    const resp = await localApiPost("/api/plugins/registry", {
      registry_url: registry_url || null,
    });
    const data = await resp.json();
    if (resp.ok && data.ok !== false) {
      renderPluginsSection(data);
      toast(registry_url ? "已保存插件仓库地址" : "已恢复默认官方插件仓库");
    } else {
      toast("保存失败：" + (data.error || "无效的仓库地址"), "err");
    }
  } catch (err) {
    toast("保存失败：" + err.message, "err");
  }
}

async function installPluginById(pluginId) {
  if (!pluginId) return;
  toast(`正在下载并校验插件 ${pluginId}…`);
  try {
    const resp = await localApiPost("/api/plugins/install", {
      plugin_id: pluginId,
      enable_after: true,
    });
    const data = await resp.json();
    if (resp.ok && data.ok !== false) {
      renderPluginsSection(data);
      await refreshDashboard();
      toast(`已安装并启用插件 ${pluginId}`);
    } else {
      toast("安装失败：" + (data.error || "校验或下载失败"), "err");
    }
  } catch (err) {
    toast("安装失败：" + err.message, "err");
  }
}

async function togglePluginById(pluginId, enabled) {
  if (!pluginId) return;
  try {
    const resp = await localApiPost("/api/plugins/toggle", {
      plugin_id: pluginId,
      enabled,
    });
    const data = await resp.json();
    if (resp.ok && data.ok !== false) {
      renderPluginsSection(data);
      await refreshDashboard();
      toast(enabled ? `已启用插件 ${pluginId}` : `已停用插件 ${pluginId}`);
    } else {
      toast("切换失败：" + (data.error || "未知错误"), "err");
    }
  } catch (err) {
    toast("切换失败：" + err.message, "err");
  }
}

async function uninstallPluginById(pluginId) {
  if (!pluginId) return;
  const confirmed = await askQuestion(
    `卸载插件 ${pluginId}？`,
    "卸载会移除本地插件目录中的适配器文件，并从智能体列表移除；你随时可以从插件目录重新安装。",
    "确认卸载",
  );
  if (!confirmed) return;
  try {
    const resp = await localApiPost("/api/plugins/uninstall", {
      plugin_id: pluginId,
    });
    const data = await resp.json();
    if (resp.ok && data.ok !== false) {
      renderPluginsSection(data);
      await refreshDashboard();
      toast(`已卸载插件 ${pluginId}`);
    } else {
      toast("卸载失败：" + (data.error || "未知错误"), "err");
    }
  } catch (err) {
    toast("卸载失败：" + err.message, "err");
  }
}

// The page follows the daemon's change stream instead of a timer. The
// daemon publishes once per scan pass that found a change, so every row of a
// snapshot reaches the page at the same moment; a timer would instead let
// each provider's cache expire on its own clock and grow the list a row at a
// time. The slow interval below is only a safety net: it re-reads in case a
// stream was blocked by a proxy, so the console still works without one.
