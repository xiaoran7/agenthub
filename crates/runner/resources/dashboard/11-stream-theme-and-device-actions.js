const FALLBACK_POLL_MS = 30000;

let streamSource = null;
let streamRetryTimer = 0;
let streamFailures = 0;
const DASHBOARD_STREAM_GIVE_UP = 5;
let fallbackTimer = 0;
let pollVisibilityBound = false;

function readDashboardNow() {
  if (document.visibilityState === "hidden") return;
  if (!streamSource || streamSource.readyState === EventSource.CLOSED) {
    streamFailures = 0;
    openChangeStream();
  }
  refreshDashboard(false);
  if (isUsageOpen()) {
    loadUsage(false);
  } else {
    usageDirty = true;
    usageMemoryCache.clear();
  }
}

// Re-reads the moment the tab becomes visible again. A hidden tab keeps its
// stream open, but a browser that suspended it comes back with a version
// the page never saw.
function bindResumeRefresh() {
  if (pollVisibilityBound) return;
  pollVisibilityBound = true;
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState !== "hidden") readDashboardNow();
  });
  window.addEventListener("focus", readDashboardNow);
}

function startPolling() {
  bindResumeRefresh();
  openChangeStream();
  clearInterval(fallbackTimer);
  fallbackTimer = setInterval(readDashboardNow, FALLBACK_POLL_MS);
}

// One long connection to the daemon. Each `change` frame names the version
// the daemon just published; the page answers it by re-reading the snapshot
// under its ETag, so an unchanged snapshot is still a bare 304.
function openChangeStream() {
  if (typeof EventSource === "undefined") return;
  clearTimeout(streamRetryTimer);
  if (streamSource) streamSource.close();
  const source = new EventSource("/api/events");
  streamSource = source;
  source.addEventListener("open", () => {
    streamFailures = 0;
    updateConnectionStatus(true);
  });
  source.addEventListener("change", () => {
    if (document.visibilityState === "hidden") {
      usageDirty = true;
      usageMemoryCache.clear();
      return;
    }
    readDashboardNow();
  });
  source.addEventListener("error", () => {
    // EventSource reconnects on its own; this only keeps a stream the
    // browser gave up on from leaving the page silent until the fallback.
    streamFailures++;
    updateConnectionStatus(false);
    if (source.readyState !== EventSource.CLOSED) return;
    streamSource = null;
    if (streamFailures > DASHBOARD_STREAM_GIVE_UP) {
      return;
    }
    streamRetryTimer = setTimeout(openChangeStream, 3000);
  });
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
  toast(`已切换至${dark ? "深色" : "浅色"}主题`);
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
    const res = await localApiPost("/api/open-data-dir");
    if (res.ok) {
      toast("已在系统文件资源管理器中打开数据目录");
    } else {
      toast("打开文件资源管理器失败", "err");
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
    toast("请填写本机设备名称", "err");
    return;
  }
  const origText = saveBtn ? saveBtn.innerHTML : "";
  if (saveBtn) {
    saveBtn.disabled = true;
    saveBtn.innerHTML = "<span>保存中…</span>";
  }
  try {
    const resp = await localApiPost("/api/device/name", { name: name });
    const resData = await resp.json();
    if (resp.ok && resData.ok === true) {
      toast("设备名称已更新");
      await refreshDashboard();
    } else {
      toast("修改名称失败：" + (resData.error || "未知错误"), "err");
    }
  } catch (err) {
    toast("修改名称失败：" + err.message, "err");
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
    toast("请填写控制台地址，例如 https://agenthub.yourdomain.com", "err");
    return;
  }
  if (!gateway.startsWith("http://") && !gateway.startsWith("https://")) {
    gateway = "https://" + gateway;
  }
  if (!code) {
    toast("请填写从 Web 控制台获取的配对绑定码", "err");
    return;
  }

  try {
    localStorage.setItem(STORE.customGateway, gateway);
  } catch (e) {}

  const origText = submitBtn.innerHTML;
  submitBtn.disabled = true;
  submitBtn.innerHTML = "<span>正在验证绑定…</span>";

  try {
    const resp = await localApiPost("/api/device/bind", { gateway: gateway, binding_code: code });

    const resData = await resp.json();
    if (resp.ok && (resData.ok === true || resData.status === "ok")) {
      if (codeInput) codeInput.value = "";
      renderSettings();
      await refreshDashboard();
      toast("已绑定到云端 AgentHub");
    } else {
      toast(
        "配对失败：" + (resData.message || resData.error || "网关拒绝绑定或绑定码无效"),
        "err",
      );
    }
  } catch (err) {
    toast("配对失败：无法连接网关（" + err.message + "）", "err");
  } finally {
    submitBtn.disabled = false;
    submitBtn.innerHTML = origText;
  }
}

async function submitDeviceUnpair() {
  const confirmed = await askQuestion(
    "解除与云端网关的绑定？",
    "解除后本机将停止出站同步，Web 控制台不再显示这台电脑；重新配对即可恢复。",
    "解除绑定",
  );
  if (!confirmed) return;
  const unpairBtn = document.getElementById("unpairSubmitBtn");
  const origText = unpairBtn ? unpairBtn.innerHTML : "";
  if (unpairBtn) {
    unpairBtn.disabled = true;
    unpairBtn.innerHTML = "<span>正在解除绑定…</span>";
  }
  try {
    const resp = await localApiPost("/api/device/unbind");
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
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

window.addEventListener("DOMContentLoaded", init);
// a wider or narrower window: the header's track takes what now fits
window.addEventListener("resize", () => {
  fitNavTrack();
  scaleSegThumb(false);
  scaleRailHl(false);
});
