import type {
  DiscoveryCandidate,
  RemoteAttachment,
  RemoteRunner,
  RemoteSession,
} from "../../lib/remote-types";
import { checkAttachmentSize } from "../../lib/remote-session-state";
import {
  getDeviceLabel,
  getProviderInfo,
  workspaceLabel,
  type DeviceGroup,
  type PluginTreeHint,
} from "../../lib/session-tree";
import type { LaunchAgentSpec } from "../remote-console-constants";
import type { LaunchAgentType } from "../RemoteLaunchModal";

export const sessionKey = (session: RemoteSession): string =>
  `${session.runner_id}/${session.binding.hub_session_id}`;

export function isCodexThreadId(id: string): boolean {
  return /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/iu.test(id);
}

const TEXT_ATTACHMENT_EXT_RE =
  /\.(txt|md|markdown|json|jsonl|yaml|yml|toml|xml|csv|tsv|py|ts|tsx|js|jsx|mjs|cjs|rs|go|java|kt|c|cpp|h|hpp|cs|rb|php|swift|sh|bash|zsh|ps1|bat|cmd|sql|html|css|scss|less|vue|svelte|ini|conf|cfg|env|log)$/iu;

export async function readSelectedAttachments(
  fileList: FileList | File[],
  onError: (message: string) => void,
): Promise<RemoteAttachment[]> {
  const files = Array.from(fileList);
  if (files.length === 0) return [];
  const loaded: RemoteAttachment[] = [];
  for (const file of files.slice(0, 16)) {
    const sizeStatus = checkAttachmentSize(file.size);
    if (sizeStatus === "empty") {
      onError(`附件 ${file.name} 为空文件（0 字节），已跳过`);
      continue;
    }
    if (sizeStatus === "too_large") {
      onError(`附件 ${file.name} 超过 7.5MB 大小限制`);
      continue;
    }
    const isText =
      file.type.startsWith("text/") ||
      file.type === "application/json" ||
      file.type === "application/xml" ||
      file.type === "application/javascript" ||
      TEXT_ATTACHMENT_EXT_RE.test(file.name);
    try {
      if (isText) {
        const content = await file.text();
        loaded.push({
          name: file.name || "attachment.txt",
          media_type: file.type || "text/plain",
          data: content,
          encoding: "utf8",
        });
      } else {
        const base64 = await new Promise<string>((resolve, reject) => {
          const reader = new FileReader();
          reader.onload = () => {
            const result =
              typeof reader.result === "string" ? reader.result : "";
            const comma = result.indexOf(",");
            resolve(comma >= 0 ? result.slice(comma + 1) : result);
          };
          reader.onerror = () => reject(reader.error);
          reader.readAsDataURL(file);
        });
        if (base64) {
          loaded.push({
            name: file.name || "image.png",
            media_type: file.type || "application/octet-stream",
            data: base64,
            encoding: "base64",
          });
        }
      }
    } catch {
      onError(`读取附件 ${file.name} 失败`);
    }
  }
  return loaded;
}

export function buildOnlineDeviceOptions(runners: RemoteRunner[]) {
  return runners
    .filter((item) => !item.revoked && item.online)
    .map((item) => ({
      value: item.runner_id,
      label: getDeviceLabel(item.runner_id, item.hello),
    }));
}

export function buildLaunchWorkspaceOptions(runnerInfo?: RemoteRunner) {
  const paths = runnerInfo?.hello?.workspaces ?? [];
  const displayPaths = runnerInfo?.hello?.workspace_labels ?? {};
  const labels = paths.map(workspaceLabel);
  const seen = new Map<string, number>();
  for (const label of labels) seen.set(label, (seen.get(label) ?? 0) + 1);
  return paths.map((path, index) => {
    const label = labels[index] ?? path;
    const displayPath = displayPaths[path]?.trim() ?? "";
    const tail = path.startsWith("ws-")
      ? (path.match(/-([0-9a-f]{16})$/u)?.[1] ?? "").slice(0, 4)
      : "";
    const duplicated = (seen.get(label) ?? 0) > 1 && tail !== "";
    const description =
      displayPath !== ""
        ? duplicated
          ? `${displayPath} · #${tail}`
          : displayPath
        : duplicated
          ? `#${tail}`
          : "";
    return {
      value: path,
      label,
      ...(description !== "" ? { description } : {}),
    };
  });
}

export function buildLaunchSessionOptions(
  runnerInfo: RemoteRunner | undefined,
  workspace: string,
  plugins?: ReadonlyArray<PluginTreeHint>,
) {
  const normalize = (path: string) =>
    path
      .replace(/^\\\\\?\\/u, "")
      .replaceAll("\\", "/")
      .toLowerCase();
  const target = normalize(workspace);
  const candidates = (runnerInfo?.hello?.discovery?.candidates ?? []).filter(
    (item) => normalize(item.workspace_path) === target,
  );
  return [
    { value: "", label: "创建新会话" },
    ...candidates.map((item) => {
      const provider = getProviderInfo(
        item.provider_session_id,
        item.provider,
        plugins,
      );
      return {
        value: item.provider_session_id,
        label: item.title ?? "未命名会话",
        title: item.title ?? item.provider_session_id,
        tag: provider.name,
        tagTone: provider.badgeClass,
      };
    }),
  ];
}

export function buildLaunchModelOptions(
  runnerInfo: RemoteRunner | undefined,
  agentType: LaunchAgentType,
  availableLaunchAgents: readonly LaunchAgentSpec[],
) {
  const hostModels = runnerInfo?.hello?.models;
  const engine = availableLaunchAgents.find((item) => item.value === agentType);
  const seen = new Set<string>();
  const models: Array<{ id: string; display_name?: string }> = [];
  const pushModel = (id?: string | null, displayName?: string | null) => {
    const clean = id?.trim();
    if (!clean || clean === "<synthetic>") return;
    const lower = clean.toLowerCase();
    if (seen.has(lower)) return;
    seen.add(lower);
    models.push({
      id: clean,
      display_name: displayName?.trim() || clean.replace(/^models\//u, ""),
    });
  };
  const keys: readonly string[] =
    agentType === "claude_code" && hostModels?.agents?.claude_code === undefined
      ? ["claude"]
      : (engine?.catalog ?? []);
  for (const k of keys) {
    const list = hostModels?.agents?.[k];
    if (list !== undefined && list.length > 0) {
      for (const item of list) pushModel(item.id, item.display_name);
    }
  }
  if (models.length === 0 && agentType === "codex") {
    for (const item of hostModels?.data ?? []) {
      pushModel(
        item.id,
        item.display_name ?? item.displayName ?? item.name ?? item.id,
      );
    }
  }
  return [
    { value: "", label: "默认模型" },
    ...models.map((item) => ({
      value: item.id,
      label: item.display_name ?? item.id,
    })),
  ];
}

export function buildSessionModelOptions(
  session: RemoteSession | undefined,
  runners: RemoteRunner[],
  discovered: Map<string, DiscoveryCandidate>,
  plugins?: ReadonlyArray<PluginTreeHint>,
) {
  if (!session) return [{ value: "", label: "默认模型" }];
  const sessionRunner = runners.find(
    (item) => item.runner_id === session.runner_id,
  );
  const hostModels = sessionRunner?.hello?.models;
  const candidate = discovered.get(
    `${session.runner_id}/${session.binding.provider_session_id}`,
  );
  const effectiveProvider = candidate?.provider ?? session.provider;
  const providerType = getProviderInfo(
    session.binding.provider_session_id,
    effectiveProvider,
    plugins,
  ).type;
  const rawProvider = (effectiveProvider ?? "").toLowerCase();
  const sid = session.binding.provider_session_id;
  const catalogKeys =
    rawProvider === "claude_desktop" || providerType === "claude_desktop"
      ? ["claude_desktop"]
      : rawProvider === "claude_code" ||
          rawProvider === "claude" ||
          providerType === "claude" ||
          sid.startsWith("claude_")
        ? hostModels?.agents?.claude_code !== undefined
          ? ["claude_code"]
          : ["claude_code", "claude"]
        : providerType === "antigravity" || rawProvider === "antigravity"
          ? ["antigravity"]
          : rawProvider === "deepseek_harness" ||
              providerType === "deepseek_harness"
            ? ["deepseek_harness"]
            : rawProvider && rawProvider !== "codex"
              ? [rawProvider]
              : providerType !== "codex"
                ? [providerType]
                : ["codex"];
  const seen = new Set<string>();
  const models: Array<{ id: string; display_name?: string }> = [];
  const pushModel = (id?: string | null, displayName?: string | null) => {
    const clean = id?.trim();
    if (!clean || clean === "<synthetic>") return;
    const lower = clean.toLowerCase();
    if (seen.has(lower)) return;
    seen.add(lower);
    models.push({
      id: clean,
      display_name: displayName?.trim() || clean.replace(/^models\//u, ""),
    });
  };
  for (const k of catalogKeys) {
    for (const item of hostModels?.agents?.[k] ?? []) {
      pushModel(item.id, item.display_name);
    }
  }
  if (catalogKeys.includes("codex") && models.length === 0) {
    for (const item of hostModels?.data ?? []) {
      pushModel(
        item.id,
        item.display_name ?? item.displayName ?? item.name ?? item.id,
      );
    }
  }
  const normalizedCandidateModel = candidate?.model
    ?.trim()
    .replace(/\[1m\]$/iu, "")
    .toLowerCase();
  const matchedCatalogModel = normalizedCandidateModel
    ? models.find(
        (item) =>
          item.id.toLowerCase() === normalizedCandidateModel ||
          item.id.replace(/^models\//u, "").toLowerCase() ===
            normalizedCandidateModel,
      )
    : undefined;
  const defaultLabel = matchedCatalogModel
    ? `默认 (${matchedCatalogModel.display_name ?? matchedCatalogModel.id})`
    : "默认模型";
  return [
    { value: "", label: defaultLabel },
    ...models.map((item) => ({
      value: item.id,
      label: item.display_name ?? item.id,
    })),
  ];
}

export function filterDeviceGroups(
  deviceGroups: DeviceGroup[],
  sessionSearchQuery: string,
): DeviceGroup[] {
  const q = sessionSearchQuery.trim().toLowerCase();
  if (!q) return deviceGroups;

  return deviceGroups
    .map((dev) => {
      const matchesDev = dev.label.toLowerCase().includes(q);
      const filteredAgents = dev.workspaces
        .map((agent) => {
          const matchesAgent = agent.label.toLowerCase().includes(q);
          const filteredWorkspaces = agent.workspaces
            .map((ws) => {
              const matchesWs = ws.label.toLowerCase().includes(q);
              const filteredItems = ws.items.filter((item) => {
                if (matchesDev || matchesAgent || matchesWs) return true;
                const itemKey =
                  item.kind === "active" ? item.sessionKey : item.key;
                return (
                  item.title.toLowerCase().includes(q) ||
                  itemKey.toLowerCase().includes(q)
                );
              });
              if (filteredItems.length === 0 && !matchesWs) return null;
              return {
                ...ws,
                items: filteredItems,
              };
            })
            .filter((ws): ws is NonNullable<typeof ws> => ws !== null);

          if (filteredWorkspaces.length === 0 && !matchesAgent) return null;
          return {
            ...agent,
            totalCount: filteredWorkspaces.reduce(
              (sum, w) => sum + w.items.length,
              0,
            ),
            workspaces: filteredWorkspaces,
          };
        })
        .filter((agent): agent is NonNullable<typeof agent> => agent !== null);

      if (filteredAgents.length === 0 && !matchesDev) return null;
      return {
        ...dev,
        totalCount: filteredAgents.reduce((sum, a) => sum + a.totalCount, 0),
        workspaces: filteredAgents,
      };
    })
    .filter((dev): dev is NonNullable<typeof dev> => dev !== null);
}
