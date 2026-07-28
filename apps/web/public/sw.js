/*
 * Agent Platform service worker.
 *
 * Push delivery only. There is deliberately no fetch handler and no cache:
 * a live monitoring panel gains nothing from serving stale data offline, and
 * skipping the cache avoids a whole class of invalidation bugs.
 */

const WAITING_TITLES = {
  WAITING_PERMISSION: "等待权限批准",
  WAITING_INPUT: "等待输入",
};

const AGENT_LABELS = {
  claude_code: "Claude Code",
  codex: "Codex",
};

self.addEventListener("install", () => {
  // Take over as soon as the user turns notifications on, instead of waiting
  // for every panel tab to close.
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener("push", (event) => {
  let payload = {};
  try {
    payload = event.data ? event.data.json() : {};
  } catch {
    // A payload we cannot read is still worth surfacing generically.
  }
  const title = WAITING_TITLES[payload.activity_status] ?? "会话需要处理";
  const project = payload.project ?? "未知项目";
  const agent = AGENT_LABELS[payload.agent_type] ?? payload.agent_type ?? "";
  event.waitUntil(showWaitingNotification(title, project, agent, payload));
});

async function showWaitingNotification(title, project, agent, payload) {
  await self.registration.showNotification(title, {
    body: agent ? `${project} · ${agent}` : project,
    // One notification per session: a session that keeps re-entering the
    // waiting state replaces its own notification instead of stacking.
    tag: `${payload.device_id}:${payload.agent_type}:${payload.agent_session_id}`,
    renotify: true,
    timestamp: Date.parse(payload.observed_at ?? "") || Date.now(),
    data: {
      device_id: payload.device_id,
      agent_type: payload.agent_type,
      agent_session_id: payload.agent_session_id,
    },
  });
  // This receipt contains no session, device or project metadata. It only
  // proves that the service worker decrypted the push and the browser accepted
  // showNotification(); Windows may still suppress the visible toast.
  const clients = await self.clients.matchAll({
    type: "window",
    includeUncontrolled: true,
  });
  const receipt = {
    type: "notification-presented",
    presented_at_ms: Date.now(),
  };
  for (const client of clients) {
    client.postMessage(receipt);
  }
}

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  event.waitUntil(openPanel(event.notification.data));
});

async function openPanel(session) {
  const clients = await self.clients.matchAll({
    type: "window",
    includeUncontrolled: true,
  });
  for (const client of clients) {
    if (new URL(client.url).origin !== self.location.origin) {
      continue;
    }
    await client.focus();
    // Hand the session over in a message rather than a URL: session and device
    // identifiers must not end up in history or a shared link.
    client.postMessage({ type: "select-session", session });
    return;
  }
  await self.clients.openWindow("/");
}
