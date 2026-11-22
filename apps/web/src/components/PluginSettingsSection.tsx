import { useCallback, useEffect, useState } from "react";
import type {
  AgentPlugin,
  RemotePluginsResponse,
} from "@agent-pulse/contracts";
import {
  describeApiError,
  fetchRemotePlugins,
  sendPluginAction,
  type GatewayIdentity,
} from "../lib/api";
import type { PanelConfig } from "../lib/config";
import type { DeviceRecord } from "../lib/types";
import { followContentStream } from "../lib/stream";
import { PluginsIcon } from "./settings-constants";

interface PluginSettingsSectionProps {
  config: PanelConfig;
  identity: GatewayIdentity;
  devices: DeviceRecord[] | undefined;
  onEnsureDevices: () => void;
}

const CAP_LABELS: ReadonlyArray<
  readonly [keyof NonNullable<AgentPlugin["capabilities"]>, string]
> = [
  ["discover", "发现会话"],
  ["launch", "远程启动"],
  ["resume", "恢复会话"],
  ["models", "模型列表"],
  ["history", "历史消息"],
  ["approvals", "审批交互"],
];

function deviceLabel(d: DeviceRecord): string {
  const name = d.display_name?.trim();
  const tail = d.device_id.slice(-6);
  return name ? `${name} (${tail})` : `Runner · ${tail}`;
}

export function PluginSettingsSection({
  config,
  identity,
  devices,
  onEnsureDevices,
}: PluginSettingsSectionProps) {
  const [response, setResponse] = useState<RemotePluginsResponse | undefined>(
    undefined,
  );
  const [selectedRunner, setSelectedRunner] = useState<string>("");
  const [registryInput, setRegistryInput] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | undefined>(undefined);
  const [receipt, setReceipt] = useState<string | undefined>(undefined);

  useEffect(() => {
    if (devices === undefined) {
      onEnsureDevices();
    }
  }, [devices, onEnsureDevices]);

  const loadPlugins = useCallback(
    async (silent = false) => {
      if (!silent) {
        setLoading(true);
        setError(undefined);
      }
      try {
        const data = await fetchRemotePlugins(config, identity);
        setResponse(data);
      } catch (err) {
        if (!silent) setError(describeApiError(err));
      } finally {
        if (!silent) setLoading(false);
      }
    },
    [config, identity],
  );

  useEffect(() => {
    void loadPlugins(false);
    return followContentStream({
      gatewayBase: config.gatewayBase,
      event: "plugins",
      onChange: () => {
        void loadPlugins(true);
      },
    });
  }, [loadPlugins, config.gatewayBase]);

  const activeDevices = (devices ?? []).filter((d) => !d.revoked);
  const runnerIds = Array.from(
    new Set([
      ...activeDevices.map((d) => d.device_id),
      ...(response?.runners.map((r) => r.runner) ?? []),
    ]),
  );

  useEffect(() => {
    if (!selectedRunner && runnerIds.length > 0) {
      const online = activeDevices.find((d) => d.online);
      setSelectedRunner(online?.device_id ?? runnerIds[0]!);
    }
  }, [selectedRunner, runnerIds, activeDevices]);

  const currentRunnerSnap = response?.runners.find(
    (r) => r.runner === selectedRunner,
  );

  useEffect(() => {
    setRegistryInput(currentRunnerSnap?.registry_url ?? "");
  }, [selectedRunner, currentRunnerSnap?.registry_url]);

  const currentPlugins: AgentPlugin[] =
    currentRunnerSnap?.plugins ??
    (selectedRunner
      ? (response?.plugins.filter(
          (p) => !p.runner || p.runner === selectedRunner,
        ) ?? [])
      : (response?.plugins ?? []));

  const installedPlugins = currentPlugins.filter((p) => p.installed !== false);
  const catalogPlugins = currentPlugins.filter((p) => p.installed === false);

  const runAction = async (
    key: string,
    action: Parameters<typeof sendPluginAction>[3],
    okMessage: string,
  ) => {
    if (!selectedRunner) {
      setError("请先选择一台在线的 Runner 设备。");
      return;
    }
    setBusyKey(key);
    setError(undefined);
    setReceipt(undefined);
    try {
      const res = await sendPluginAction(
        config,
        identity,
        selectedRunner,
        action,
      );
      setResponse((prev) => {
        const nextRunners = [...(prev?.runners ?? [])];
        const idx = nextRunners.findIndex((r) => r.runner === selectedRunner);
        const snap = {
          runner: selectedRunner,
          ...(res.registry_url ? { registry_url: res.registry_url } : {}),
          updated_at: new Date().toISOString(),
          plugins: res.plugins,
        };
        if (idx >= 0) nextRunners[idx] = snap;
        else nextRunners.push(snap);
        const byId = new Map<string, AgentPlugin>();
        for (const r of nextRunners) {
          for (const p of r.plugins) {
            const cur = byId.get(p.id);
            if (
              !cur ||
              (!cur.installed && p.installed) ||
              (!cur.enabled && p.enabled)
            ) {
              byId.set(p.id, p);
            }
          }
        }
        return {
          plugins: [...byId.values()],
          runners: nextRunners,
        };
      });
      setReceipt(okMessage);
    } catch (err) {
      setError(describeApiError(err));
    } finally {
      setBusyKey(null);
    }
  };

  return (
    <section className="settings-section" aria-labelledby="plugins-settings">
      <div className="settings-section-heading">
        <span className="settings-section-icon" aria-hidden="true">
          <PluginsIcon size={18} />
        </span>
        <div>
          <h3 id="plugins-settings">插件扩展 (Runner 权威源)</h3>
          <p>
            原生内置 Antigravity、Codex、Claude Desktop、Claude Code 与 DeepSeek
            Harness；其余外部 ACP 智能体由目标 Runner 在本机校验安装并统一纳管。
          </p>
        </div>
      </div>

      <div className="settings-toggle" style={{ gap: 10, flexWrap: "wrap" }}>
        <span>目标 Runner 设备</span>
        {runnerIds.length === 0 ? (
          <span className="settings-hint" style={{ margin: 0 }}>
            暂无已绑定 Runner，请先在"设备"页完成配对
          </span>
        ) : (
          <select
            value={selectedRunner}
            onChange={(e) => setSelectedRunner(e.target.value)}
            aria-label="选择目标 Runner 设备"
          >
            {runnerIds.map((id) => {
              const dev = activeDevices.find((d) => d.device_id === id);
              const label = dev ? deviceLabel(dev) : `Runner · ${id.slice(-6)}`;
              const status = dev ? (dev.online ? "在线" : "离线") : "已同步";
              return (
                <option key={id} value={id}>
                  {label} · {status}
                </option>
              );
            })}
          </select>
        )}
        <button
          type="button"
          className="secondary"
          disabled={!selectedRunner || busyKey !== null}
          onClick={() =>
            void runAction(
              "refresh_catalog",
              { action: "refresh_catalog" },
              "已由 Runner 同步最新插件仓库目录。",
            )
          }
        >
          {busyKey === "refresh_catalog" ? "正在同步…" : "刷新目录"}
        </button>
      </div>

      {selectedRunner && (
        <div
          className="settings-toggle"
          style={{ gap: 8, flexWrap: "wrap", alignItems: "center" }}
        >
          <input
            value={registryInput}
            onChange={(e) => setRegistryInput(e.target.value)}
            placeholder="https://raw.githubusercontent.com/xiaoran7/agenthub-plugins/main/registry.json"
            aria-label="插件仓库地址"
            style={{ flex: 1, minWidth: 220 }}
          />
          <button
            type="button"
            className="secondary"
            disabled={busyKey !== null}
            onClick={() =>
              void runAction(
                "set_registry",
                {
                  action: "set_registry",
                  registry_url: registryInput.trim(),
                },
                registryInput.trim()
                  ? "已更新 Runner 插件仓库地址。"
                  : "已恢复默认官方插件仓库地址。",
              )
            }
          >
            {busyKey === "set_registry" ? "保存中…" : "保存仓库"}
          </button>
        </div>
      )}

      {error && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
      {receipt && (
        <p className="settings-receipt" role="status">
          {receipt}
        </p>
      )}

      {loading && response === undefined && (
        <p className="settings-hint">正在读取 Runner 插件状态…</p>
      )}

      <span className="settings-subheading">
        已安装插件 ({installedPlugins.length})
      </span>
      {installedPlugins.length === 0 ? (
        <p className="settings-hint">
          当前 Runner 尚未安装外部 ACP 插件；可点击"刷新目录"从下方列表安装。
        </p>
      ) : (
        installedPlugins.map((plugin) => {
          const isToggling = busyKey === `toggle:${plugin.id}`;
          const isUninstalling = busyKey === `uninstall:${plugin.id}`;
          const caps = CAP_LABELS.filter(
            ([k]) => plugin.capabilities?.[k] !== false,
          ).map(([, label]) => label);
          return (
            <div className="settings-plugin-card" key={plugin.id}>
              <div className="settings-plugin-header">
                <div className="settings-plugin-info">
                  <h4 className="settings-plugin-title">
                    {plugin.displayName || plugin.name}
                  </h4>
                  <span className="settings-plugin-version">
                    v{plugin.installedVersion || plugin.version}
                  </span>
                </div>
                <div style={{ display: "flex", gap: 8 }}>
                  <button
                    type="button"
                    className={plugin.enabled ? "secondary" : "primary"}
                    disabled={busyKey !== null}
                    onClick={() =>
                      void runAction(
                        `toggle:${plugin.id}`,
                        {
                          action: "toggle",
                          plugin_id: plugin.id,
                          enabled: !plugin.enabled,
                        },
                        plugin.enabled
                          ? `已停用插件 ${plugin.displayName || plugin.name}。`
                          : `已启用插件 ${plugin.displayName || plugin.name}。`,
                      )
                    }
                  >
                    {isToggling ? "切换中…" : plugin.enabled ? "停用" : "启用"}
                  </button>
                  <button
                    type="button"
                    className="secondary danger-button"
                    disabled={busyKey !== null}
                    onClick={() =>
                      void runAction(
                        `uninstall:${plugin.id}`,
                        {
                          action: "uninstall",
                          plugin_id: plugin.id,
                        },
                        `已从 Runner 卸载插件 ${plugin.displayName || plugin.name}。`,
                      )
                    }
                  >
                    {isUninstalling ? "卸载中…" : "卸载"}
                  </button>
                </div>
              </div>
              <p className="settings-plugin-desc">{plugin.description}</p>
              {plugin.error && (
                <p className="error-text" style={{ margin: "4px 0" }}>
                  {plugin.error}
                </p>
              )}
              <div className="settings-plugin-footer">
                <span
                  className={`settings-plugin-badge ${
                    plugin.enabled
                      ? plugin.runtimeStatus === "downloading"
                        ? "downloading"
                        : plugin.runtimeStatus === "error" ||
                            plugin.available === false ||
                            Boolean(plugin.error)
                          ? "error"
                          : "ready"
                      : "disabled"
                  }`}
                >
                  {plugin.enabled
                    ? plugin.runtimeStatus === "downloading"
                      ? "下载校验中…"
                      : plugin.runtimeStatus === "error"
                        ? `异常 (${plugin.error || "未知"})`
                        : plugin.available === false
                          ? "已启用 · 缺少底层 CLI"
                          : Boolean(plugin.error)
                            ? `异常 (${plugin.error || "未知"})`
                            : plugin.runtimeStatus === "ready"
                              ? "就绪 · 已启用"
                              : "已启用"
                    : "已停用"}
                </span>
                {caps.length > 0 && (
                  <span className="settings-hint" style={{ margin: 0 }}>
                    {caps.join(" · ")}
                  </span>
                )}
              </div>
            </div>
          );
        })
      )}

      <span className="settings-subheading">
        可用插件目录 ({catalogPlugins.length})
      </span>
      {catalogPlugins.length === 0 ? (
        <p className="settings-hint">
          {currentPlugins.length > 0
            ? "目录中的可用插件均已安装。"
            : '点击上方"刷新目录"让在线 Runner 拉取最新插件仓库清单。'}
        </p>
      ) : (
        catalogPlugins.map((plugin) => {
          const isInstalling =
            busyKey === `install:${plugin.id}` ||
            plugin.runtimeStatus === "downloading";
          return (
            <div className="settings-plugin-card" key={plugin.id}>
              <div className="settings-plugin-header">
                <div className="settings-plugin-info">
                  <h4 className="settings-plugin-title">
                    {plugin.displayName || plugin.name}
                  </h4>
                  <span className="settings-plugin-version">
                    v{plugin.version}
                  </span>
                </div>
                <button
                  type="button"
                  className="primary"
                  disabled={busyKey !== null || isInstalling}
                  onClick={() =>
                    void runAction(
                      `install:${plugin.id}`,
                      {
                        action: "install",
                        plugin_id: plugin.id,
                      },
                      `已在 Runner 上安装并启用插件 ${plugin.displayName || plugin.name}。`,
                    )
                  }
                >
                  {isInstalling ? "下载安装中…" : "安装并启用"}
                </button>
              </div>
              <p className="settings-plugin-desc">{plugin.description}</p>
            </div>
          );
        })
      )}
    </section>
  );
}
