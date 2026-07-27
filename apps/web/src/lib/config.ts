export interface PanelConfig {
  /** Gateway base URL; empty string means same origin (dev rewrite or reverse proxy). */
  gatewayBase: string;
  /** Development identity sent as `x-user-id`; replaced by real auth later. */
  userId: string;
}

export const DEFAULT_CONFIG: PanelConfig = {
  gatewayBase: "",
  userId: "local-user",
};

export const CONFIG_STORAGE_KEY = "agent-platform.panel-config.v1";

const LOOPBACK_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]"]);

/**
 * Normalizes a user-supplied gateway base URL. Mirrors the runner's outbound
 * policy: HTTPS anywhere, plain HTTP only toward loopback hosts, and no
 * credentials, query, or fragment baked into the base.
 */
export function normalizeGatewayBase(input: string): string {
  const trimmed = input.trim();
  if (trimmed === "") {
    return "";
  }
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    throw new Error("网关地址必须是完整的 http(s) URL");
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error("网关地址只支持 http 或 https");
  }
  if (url.protocol === "http:" && !LOOPBACK_HOSTS.has(url.hostname)) {
    throw new Error("非回环地址必须使用 https");
  }
  if (url.username !== "" || url.password !== "") {
    throw new Error("网关地址不能包含用户名或密码");
  }
  if (url.search !== "" || url.hash !== "") {
    throw new Error("网关地址不能包含查询参数或片段");
  }
  const path = url.pathname.replace(/\/+$/, "");
  return `${url.origin}${path}`;
}

export function normalizeUserId(input: string): string {
  const trimmed = input.trim();
  if (trimmed.length === 0 || trimmed.length > 128) {
    throw new Error("用户标识长度必须在 1 到 128 个字符之间");
  }
  return trimmed;
}

export function loadConfig(
  storage: Pick<Storage, "getItem"> | undefined,
): PanelConfig {
  if (storage === undefined) {
    return { ...DEFAULT_CONFIG };
  }
  let raw: string | null;
  try {
    raw = storage.getItem(CONFIG_STORAGE_KEY);
  } catch {
    return { ...DEFAULT_CONFIG };
  }
  if (raw === null) {
    return { ...DEFAULT_CONFIG };
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) {
      return { ...DEFAULT_CONFIG };
    }
    const record = parsed as Record<string, unknown>;
    return {
      gatewayBase:
        typeof record.gatewayBase === "string"
          ? normalizeGatewayBase(record.gatewayBase)
          : DEFAULT_CONFIG.gatewayBase,
      userId:
        typeof record.userId === "string"
          ? normalizeUserId(record.userId)
          : DEFAULT_CONFIG.userId,
    };
  } catch {
    return { ...DEFAULT_CONFIG };
  }
}

export function saveConfig(
  storage: Pick<Storage, "setItem">,
  config: PanelConfig,
): void {
  storage.setItem(CONFIG_STORAGE_KEY, JSON.stringify(config));
}
