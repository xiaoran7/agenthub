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
    summary.textContent = `已加载 ${skills.length} 个技能 · ${mcpServers.length} 个 MCP 服务 · ${hooks.length} 个生命周期钩子`;
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
      mcpGrid.innerHTML = `<div class="empty-state"><b>没有找到匹配的 MCP 协议服务器</b><span class="why">可点击右上角 "注册 MCP 服务" 添加自定义服务</span></div>`;
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

function formatCapabilityDisplay(raw) {
  const clean = String(raw || "")
    .replace(/[\x00-\x1f]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (!clean) return "—";
  return clean.replace(
    /(-e(?:nc(?:odedcommand)?)?)\s+([A-Za-z0-9+/=]{64,})/gi,
    (_m, flag, b64) => `${flag} <Base64 脚本 · ${b64.length} 字符>`,
  );
}

function capRow({ name, kind, value, facts, agents, meta, copyLabel, title }) {
  const displayValue = formatCapabilityDisplay(value);
  const safeValue = escapeHtml(displayValue);
  const rawCopy = String(value || "").replace(/[\r\n\t]+/g, " ").trim();
  const hoverTitle = String(title || displayValue).slice(0, 320);
  return `
    <div class="row cap-row" title="${escapeHtml(hoverTitle)}">
      <div class="who">
        <div class="name-line">
          <span class="name">${escapeHtml(name || "—")}</span>
          ${kind ? `<span class="tag">${escapeHtml(kind)}</span>` : ""}
        </div>
        <div class="sub flow">${facts.filter(Boolean).join(" · ")}</div>
        <button type="button" class="cap-copy mono" onclick="copyText(this.dataset.v); toast('${copyLabel}')" data-v="${escapeHtml(rawCopy)}" title="点击复制完整内容">
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
    const resp = await localApiPost("/api/capabilities/sync");
    if (resp.ok) {
      const data = await resp.json();
      if (data.capabilities) {
        cachedCapabilities = data.capabilities;
        renderCapabilities(data.capabilities);
      }
      toast("已同步技能、MCP 服务与生命周期钩子配置");
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

// One question, asked on the console's own surface.
//
// `window.confirm` is the browser's dialog: unstyled, modal to the whole
// window, and on Windows it freezes the renderer until it is answered. A
// question about the product is asked in the product's own voice, and the
// caller gets a promise it can await like any other async step.
let askResolve = null;

function askQuestion(title, note, confirmLabel) {
  const modal = document.getElementById("askModal");
  if (!modal) return Promise.resolve(false);
  document.getElementById("askTitle").textContent = title;
  document.getElementById("askNote").textContent = note || "";
  document.getElementById("askConfirmBtn").textContent = confirmLabel || "确定";
  modal.classList.add("open");
  setTimeout(() => document.getElementById("askCancelBtn")?.focus(), 30);
  // A question that is replaced before it is answered is answered "no", so
  // its caller never waits forever.
  if (askResolve) askResolve(false);
  return new Promise((resolve) => {
    askResolve = resolve;
  });
}

function closeAsk(answer) {
  const modal = document.getElementById("askModal");
  if (modal) modal.classList.remove("open");
  const resolve = askResolve;
  askResolve = null;
  if (resolve) resolve(answer);
}

// The dialog's two buttons and its backdrop are bound once, at load.
document.addEventListener("DOMContentLoaded", () => {
  document
    .getElementById("askConfirmBtn")
    ?.addEventListener("click", () => closeAsk(true));
  document
    .getElementById("askCancelBtn")
    ?.addEventListener("click", () => closeAsk(false));
  document.getElementById("askModal")?.addEventListener("click", (e) => {
    if (e.target && e.target.id === "askModal") closeAsk(false);
  });
});

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
    const resp = await localApiPost("/api/capabilities/mcp/register", payload);
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
  const confirmed = await askQuestion(
    "退出 AgentHub Runner？",
    "退出后右下角托盘图标和本地服务将一并终止；正在执行的会话会中断。",
    "退出",
  );
  if (!confirmed) return;
  const modal = document.getElementById("shutdownModal");
  if (modal) modal.classList.add("open");

  try {
    await localApiPost("/api/shutdown");
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

