import React, { useEffect, useRef, useState, type FormEvent } from "react";
import {
  applyAppearance,
  readAppearance,
  saveAppearance,
  type Appearance,
} from "../appearance";
import { invoke } from "@tauri-apps/api/core";
import {
  Settings,
  Download,
  User,
  SunMoon,
  Mic,
  Sliders,
  Sparkles,
  Keyboard,
  CreditCard,
  CircleUserRound,
  Puzzle,
  Globe,
  Monitor,
  Anchor,
  Radio,
  GitBranch,
  Terminal,
  Cpu,
  ShieldCheck,
  CheckCircle2,
  RefreshCw,
  X,
  Search,
  HardDrive,
  Folder,
  FolderPlus,
  RotateCcw,
  ArrowRight,
  LogOut,
  ExternalLink,
} from "lucide-react";
import { normalizeWorkspacePaths } from "./NativeHostDialog";
import type { DesktopPreferences, Session, Snapshot } from "../types";
import {
  AGENT_PLUGIN_MARKETPLACE,
  readPluginState,
  savePluginState,
  type AgentPlugin,
} from "../plugins";
import { SelectDropdown } from "./SelectDropdown";

interface DesktopSettingsModalProps {
  preferences: DesktopPreferences;
  snapshot: Snapshot;
  initialSection?: SettingsSection;
  onClose: () => void;
  onSaved: (preferences: DesktopPreferences) => void;
  onLogout?: () => void;
  onOpenLocalHost?: () => void;
  onPluginsChanged?: () => void;
}

type SettingsSection =
  "general" | "plugins" | "import" | "connections" | "account";

interface HostProfile {
  workspace_paths: string[];
  codex_binary: string;
  remote_enabled?: boolean;
}

interface HostStatus {
  phase:
    "loading" | "starting" | "stopped" | "local" | "connecting" | "connected";
  profile: HostProfile;
}

export function DesktopSettingsModal({
  preferences,
  snapshot,
  initialSection = "general",
  onClose,
  onSaved,
  onLogout,
  onOpenLocalHost,
  onPluginsChanged,
}: DesktopSettingsModalProps) {
  const [section, setSection] = useState<SettingsSection>(initialSection);
  const [searchQuery, setSearchQuery] = useState("");
  const [refreshSeconds, setRefreshSeconds] = useState(
    preferences.refresh_interval_seconds || 4,
  );
  const [claudeBinary, setClaudeBinary] = useState(
    preferences.claude_binary || "claude",
  );
  const [codexBinary] = useState(preferences.codex_binary || "codex");
  const [grokBinary, setGrokBinary] = useState(
    preferences.grok_binary || "grok",
  );
  const [appearance] = useState(() => readAppearance(preferences));
  const savedAppearance = useRef(appearance);
  const [defaultCwd, setDefaultCwd] = useState(appearance.default_cwd);
  const [appLanguage, setAppLanguage] = useState<Appearance["app_language"]>(
    appearance.app_language,
  );
  const [theme, setTheme] = useState<Appearance["theme"]>(appearance.theme);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  );

  useEffect(
    () => () => {
      applyAppearance(savedAppearance.current);
      clearTimeout(saveTimer.current);
    },
    [],
  );

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [saveSuccess, setSaveSuccess] = useState(false);

  const [hostStatus, setHostStatus] = useState<HostStatus | null>(null);
  const [hostBusy, setHostBusy] = useState(false);
  const [hostError, setHostError] = useState<string | null>(null);

  const fetchHostStatus = async () => {
    try {
      const status = await invoke<HostStatus>("remote_host_status");
      setHostStatus(status);
      setHostError(null);
    } catch (err) {
      setHostError(String(err));
    }
  };

  useEffect(() => {
    if (section === "connections") {
      void fetchHostStatus();
      const timer = setInterval(() => {
        void fetchHostStatus();
      }, 2500);
      return () => clearInterval(timer);
    }
  }, [section]);

  const handleToggleRemote = async (enable: boolean) => {
    setHostBusy(true);
    setHostError(null);
    try {
      if (enable) {
        const profile = hostStatus?.profile ?? {
          workspace_paths: [],
          codex_binary: "codex",
          remote_enabled: true,
        };
        await invoke("start_remote_host", { input: profile });
      } else {
        await invoke("stop_remote_host");
      }
      await fetchHostStatus();
    } catch (err) {
      setHostError(String(err));
    } finally {
      setHostBusy(false);
    }
  };

  const handleAddWorkspace = async () => {
    setHostBusy(true);
    setHostError(null);
    try {
      const path = await invoke<string | null>("pick_folder", {
        startDir: hostStatus?.profile.workspace_paths.at(-1) ?? null,
      });
      if (path) {
        const currentPaths = hostStatus?.profile.workspace_paths ?? [];
        const newPaths = normalizeWorkspacePaths([...currentPaths, path]);
        const updated = await invoke<HostStatus>("save_host_workspaces", {
          workspaces: newPaths,
        });
        setHostStatus(updated);
      }
    } catch (err) {
      setHostError(String(err));
    } finally {
      setHostBusy(false);
    }
  };

  const handleRemoveWorkspace = async (targetPath: string) => {
    setHostBusy(true);
    setHostError(null);
    try {
      const currentPaths = hostStatus?.profile.workspace_paths ?? [];
      const newPaths = currentPaths.filter((p) => p !== targetPath);
      const updated = await invoke<HostStatus>("save_host_workspaces", {
        workspaces: newPaths,
      });
      setHostStatus(updated);
    } catch (err) {
      setHostError(String(err));
    } finally {
      setHostBusy(false);
    }
  };

  const save = async (event?: FormEvent) => {
    if (event) event.preventDefault();
    setBusy(true);
    setError(undefined);
    setSaveSuccess(false);
    try {
      const next = await invoke<DesktopPreferences>(
        "save_desktop_preferences",
        {
          input: {
            refresh_interval_seconds: refreshSeconds,
            claude_binary: claudeBinary,
            grok_binary: grokBinary,
          },
        },
      );
      const fullPreferences: DesktopPreferences = {
        ...next,
        default_cwd: defaultCwd,
        app_language: appLanguage,
        theme,
      };
      const nextAppearance = {
        theme,
        app_language: appLanguage,
        default_cwd: defaultCwd,
      };
      saveAppearance(nextAppearance);
      savedAppearance.current = nextAppearance;
      applyAppearance(nextAppearance);
      onSaved(fullPreferences);
      setSaveSuccess(true);
      clearTimeout(saveTimer.current);
      saveTimer.current = setTimeout(() => setSaveSuccess(false), 2000);
    } catch (nextError) {
      setError(
        typeof nextError === "string" ? nextError : "保存设置失败，请检查参数",
      );
    } finally {
      setBusy(false);
    }
  };

  const [pluginState, setPluginState] = useState(() => readPluginState());

  const togglePlugin = (pluginId: Session["agent_type"]) => {
    setPluginState((prev) => {
      const isCurrentlyEnabled = prev.enabledPluginIds.includes(pluginId);
      const nextIds = isCurrentlyEnabled
        ? prev.enabledPluginIds.filter((id) => id !== pluginId)
        : [...prev.enabledPluginIds, pluginId];
      const nextState = { enabledPluginIds: nextIds };
      savePluginState(nextState);
      if (onPluginsChanged) {
        onPluginsChanged();
      }
      return nextState;
    });
  };

  const pickBinaryFile = async (
    current: string,
    onSelect: (val: string) => void,
  ) => {
    try {
      if (!("__TAURI_INTERNALS__" in window)) return;
      const picked = await invoke<string[] | null>("pick_files", {
        startDir:
          current && (current.includes("\\") || current.includes("/"))
            ? current
            : null,
      });
      if (picked && picked.length > 0 && picked[0]) {
        onSelect(picked[0]);
      }
    } catch {
      // ignore
    }
  };

  const navItems: Array<{
    group: string;
    items: Array<{ id: SettingsSection; label: string; icon: React.ReactNode }>;
  }> = [
    {
      group: "核心设置",
      items: [
        { id: "general", label: "常规与刷新", icon: <Settings size={15} /> },
        { id: "plugins", label: "插件商城", icon: <Puzzle size={15} /> },
        { id: "import", label: "Agent 路径", icon: <Download size={15} /> },
        {
          id: "connections",
          label: "远程连接与 Host",
          icon: <Radio size={15} />,
        },
        {
          id: "account",
          label: "账户与退出",
          icon: <CircleUserRound size={15} />,
        },
      ],
    },
  ];

  return (
    <div className="settings-modal-overlay" onMouseDown={onClose}>
      <div
        className="settings-modal-window"
        onMouseDown={(e) => e.stopPropagation()}
      >
        {/* Left Sidebar Navigation */}
        <aside className="settings-modal-sidebar">
          <div className="settings-sidebar-header">
            <button
              type="button"
              className="settings-back-btn"
              onClick={onClose}
            >
              <ArrowRight size={14} className="rotate-180" />
              <span>返回应用</span>
            </button>
            <div className="settings-search-wrap">
              <Search size={14} className="search-icon" />
              <input
                type="text"
                placeholder="搜索设置..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
              />
            </div>
          </div>

          <nav className="settings-modal-nav">
            {navItems.map((group) => {
              const filteredItems = group.items.filter((item) =>
                item.label.toLowerCase().includes(searchQuery.toLowerCase()),
              );
              if (filteredItems.length === 0) return null;
              return (
                <div key={group.group} className="settings-nav-group">
                  <span className="settings-group-title">{group.group}</span>
                  {filteredItems.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      className={`settings-nav-item ${section === item.id ? "active" : ""}`}
                      onClick={() => setSection(item.id)}
                    >
                      <span className="nav-item-icon">{item.icon}</span>
                      <span className="nav-item-label">{item.label}</span>
                    </button>
                  ))}
                </div>
              );
            })}
          </nav>
        </aside>

        {/* Right Main Content */}
        <main className="settings-modal-main">
          <header className="settings-main-header">
            <h2>
              {navItems.flatMap((g) => g.items).find((i) => i.id === section)
                ?.label || "设置"}
            </h2>
            <div className="settings-header-right">
              {saveSuccess && (
                <span className="save-indicator success">
                  <CheckCircle2 size={14} /> 已保存
                </span>
              )}
              <button
                type="button"
                className="settings-close-btn"
                onClick={onClose}
                aria-label="关闭设置"
              >
                <X size={18} />
              </button>
            </div>
          </header>

          <div className="settings-content-scroll">
            <form onSubmit={save} className="settings-body-form">
              {/* SECTION: GENERAL */}
              {section === "general" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">常规偏好</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>语言</strong>
                          <p>应用界面 UI 呈现语言。</p>
                        </div>
                        <SelectDropdown
                          value={appLanguage}
                          options={[
                            { value: "auto", label: "自动检测" },
                            { value: "zh-CN", label: "简体中文" },
                            { value: "en-US", label: "English" },
                          ]}
                          onChange={(val) => setAppLanguage(val)}
                          size="sm"
                        />
                      </div>

                      <div className="settings-divider" />

                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>界面主题风格</strong>
                          <p>支持深色、浅色以及琥珀色（Amber）暖光主题。</p>
                        </div>
                        <SelectDropdown
                          value={theme}
                          options={[
                            { value: "dark", label: "深色模式 (Dark)" },
                            { value: "light", label: "浅色模式 (Light)" },
                            { value: "amber", label: "琥珀暖色 (Amber)" },
                          ]}
                          onChange={(val) => {
                            setTheme(val);
                            document.documentElement.setAttribute(
                              "data-theme",
                              val,
                            );
                          }}
                          size="sm"
                        />
                      </div>

                      <div className="settings-divider" />

                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>刷新与同步周期</strong>
                          <p>实时通道断开时用于后台安全轮询（秒）。</p>
                        </div>
                        <div className="range-box">
                          <input
                            type="range"
                            min="2"
                            max="60"
                            value={refreshSeconds}
                            onChange={(e) =>
                              setRefreshSeconds(Number(e.target.value))
                            }
                          />
                          <span className="range-badge">{refreshSeconds}s</span>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: PLUGINS MARKETPLACE */}
              {section === "plugins" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <div className="plugins-marketplace-header">
                      <div>
                        <h4 className="card-group-title" style={{ margin: 0 }}>
                          插件商城与实验性扩展
                        </h4>
                        <p className="plugins-sub-desc">
                          探索并接入更多 AI 编程助手与实验性 Agent
                          驱动。开启后将在侧边栏与接管会话中开放对应 Provider。
                        </p>
                      </div>
                    </div>

                    <div className="plugins-card-grid">
                      {AGENT_PLUGIN_MARKETPLACE.map((plugin) => {
                        const isEnabled = pluginState.enabledPluginIds.includes(
                          plugin.id,
                        );
                        return (
                          <div
                            key={plugin.id}
                            className={`plugin-item-card ${isEnabled ? "plugin-installed" : ""}`}
                          >
                            <div className="plugin-card-top">
                              <div
                                className="plugin-icon-box"
                                data-provider={plugin.id}
                              >
                                <Puzzle size={20} />
                              </div>
                              <div className="plugin-header-text">
                                <div className="plugin-title-row">
                                  <strong className="plugin-name">
                                    {plugin.name}
                                  </strong>
                                  <span className="plugin-badge">
                                    {plugin.category}
                                  </span>
                                </div>
                                <span className="plugin-author">
                                  {plugin.author} · v{plugin.version}
                                </span>
                              </div>
                              <div className="plugin-action-switch">
                                <button
                                  type="button"
                                  className={`plugin-toggle-btn ${isEnabled ? "active" : ""}`}
                                  onClick={() => togglePlugin(plugin.id)}
                                >
                                  {isEnabled ? "已启用" : "获取 / 启用"}
                                </button>
                              </div>
                            </div>

                            <p className="plugin-summary-text">
                              {plugin.description}
                            </p>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: IMPORT & AGENTS */}
              {section === "import" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">
                      本机 Agent 运行时与 CLI
                    </h4>
                    <p className="settings-group-desc">
                      配置本地各 Agent Provider
                      的启动命令与路径。由守护进程托管的运行时自动探测环境，无需手动配置。
                    </p>
                    <div className="settings-card">
                      {/* Claude Code */}
                      <div className="settings-row-item agent-path-row">
                        <div className="row-text">
                          <div className="agent-title-row">
                            <strong>Claude Code CLI</strong>
                            <span className="status-badge-chip">
                              命令行工具
                            </span>
                          </div>
                          <p>
                            Anthropic 官方终端 Agent。支持 PATH
                            命令名或本地绝对路径。
                          </p>
                        </div>
                        <div className="agent-path-input-group">
                          <input
                            type="text"
                            value={claudeBinary}
                            onChange={(e) => setClaudeBinary(e.target.value)}
                            placeholder="claude"
                            required
                          />
                          {claudeBinary !== "claude" && (
                            <button
                              type="button"
                              className="outline-button sm"
                              onClick={() => setClaudeBinary("claude")}
                              title="重置为默认命令 'claude'"
                            >
                              <RotateCcw size={12} />
                              <span>重置</span>
                            </button>
                          )}
                          <button
                            type="button"
                            className="outline-button sm"
                            onClick={() =>
                              pickBinaryFile(claudeBinary, setClaudeBinary)
                            }
                            title="浏览选择本地可执行文件"
                          >
                            <Folder size={12} />
                            <span>浏览</span>
                          </button>
                        </div>
                      </div>

                      <div className="settings-divider" />

                      {/* OpenAI Codex */}
                      <div className="settings-row-item agent-path-row">
                        <div className="row-text">
                          <div className="agent-title-row">
                            <strong>OpenAI Codex</strong>
                            <span className="status-badge-chip active">
                              守护进程托管
                            </span>
                          </div>
                          <p>
                            由 agenthubd
                            统一托管运行与自动发现，无需配置外部可执行路径。
                          </p>
                        </div>
                        <button
                          type="button"
                          className="outline-button sm"
                          onClick={() => setSection("connections")}
                        >
                          管理连接
                        </button>
                      </div>

                      <div className="settings-divider" />

                      {/* ZCode Agent */}
                      <div className="settings-row-item agent-path-row">
                        <div className="row-text">
                          <div className="agent-title-row">
                            <strong>ZCode Agent</strong>
                            <span className="status-badge-chip active">
                              内置驱动
                            </span>
                          </div>
                          <p>
                            基于 ACP 协议内置驱动，由 AgentHub
                            自动探测本地环境与工作区。
                          </p>
                        </div>
                        <span className="status-badge-chip active">
                          自动就绪
                        </span>
                      </div>

                      <div className="settings-divider" />

                      {/* xAI Grok Build */}
                      <div className="settings-row-item agent-path-row">
                        <div className="row-text">
                          <div className="agent-title-row">
                            <strong>xAI Grok Build CLI</strong>
                            <span className="status-badge-chip">
                              命令行工具
                            </span>
                          </div>
                          <p>
                            xAI 全栈极速开发 Agent。支持 PATH
                            命令名或本地绝对路径。
                          </p>
                        </div>
                        <div className="agent-path-input-group">
                          <input
                            type="text"
                            value={grokBinary}
                            onChange={(e) => setGrokBinary(e.target.value)}
                            placeholder="grok"
                            required
                          />
                          {grokBinary !== "grok" && (
                            <button
                              type="button"
                              className="outline-button sm"
                              onClick={() => setGrokBinary("grok")}
                              title="重置为默认命令 'grok'"
                            >
                              <RotateCcw size={12} />
                              <span>重置</span>
                            </button>
                          )}
                          <button
                            type="button"
                            className="outline-button sm"
                            onClick={() =>
                              pickBinaryFile(grokBinary, setGrokBinary)
                            }
                            title="浏览选择本地可执行文件"
                          >
                            <Folder size={12} />
                            <span>浏览</span>
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>

                  <div className="settings-card-group">
                    <h4 className="card-group-title">发现状态</h4>
                    <div className="settings-card">
                      <div className="settings-stat-grid">
                        <div className="stat-box">
                          <span className="stat-label">已发现总会话</span>
                          <strong className="stat-value">
                            {snapshot.local.session_count}
                          </strong>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: ACCOUNT */}
              {section === "account" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">账号管理</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>
                            {snapshot.account.offline
                              ? "当前运行模式"
                              : "当前登录账号"}
                          </strong>
                          <p>{snapshot.account.user.email}</p>
                        </div>
                        {onLogout && (
                          <button
                            type="button"
                            className="danger-outline-btn"
                            onClick={onLogout}
                          >
                            <LogOut size={14} />
                            <span>
                              {snapshot.account.offline
                                ? "登录 / 切换模式"
                                : "退出登录"}
                            </span>
                          </button>
                        )}
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: CONNECTIONS */}
              {section === "connections" && (
                <div className="settings-section-block">
                  {hostError && (
                    <div
                      className="settings-error-banner"
                      style={{ marginBottom: "12px" }}
                    >
                      {hostError}
                    </div>
                  )}

                  {/* Card Group 1: 远程连接与网关控制 */}
                  <div className="settings-card-group">
                    <h4 className="card-group-title">
                      远程连接与协同 (Remote Gateway)
                    </h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>启用远程中继连接 (Remote)</strong>
                          <p>
                            开启后，本机会话可通过 Web
                            端（claude.ai/code、AgentHub
                            Web）或手机端远程实时查看与控制，支持双工指令流式交互。
                          </p>
                        </div>
                        <div
                          style={{
                            display: "flex",
                            alignItems: "center",
                            gap: "10px",
                          }}
                        >
                          <span
                            className={`status-badge-chip ${
                              hostStatus?.phase === "connected" ? "active" : ""
                            }`}
                          >
                            {hostStatus?.phase === "connected"
                              ? "Remote 已连接"
                              : hostStatus?.phase === "connecting"
                                ? "连接中..."
                                : hostStatus?.phase === "local"
                                  ? "本机就绪 · Remote 已关闭"
                                  : hostStatus?.phase === "starting"
                                    ? "启动中..."
                                    : hostStatus?.phase === "stopped"
                                      ? "已停止"
                                      : "检测中"}
                          </span>
                          <label className="ios-toggle">
                            <input
                              type="checkbox"
                              checked={
                                hostStatus?.phase === "connected" ||
                                hostStatus?.phase === "connecting" ||
                                Boolean(hostStatus?.profile.remote_enabled)
                              }
                              disabled={
                                hostBusy ||
                                hostStatus?.phase === "starting" ||
                                hostStatus?.phase === "loading"
                              }
                              onChange={(e) =>
                                void handleToggleRemote(e.target.checked)
                              }
                            />
                            <span className="toggle-slider" />
                          </label>
                        </div>
                      </div>

                      <div className="settings-divider" />

                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>Gateway 服务地址</strong>
                          <p
                            style={{
                              fontFamily: "monospace",
                              fontSize: "11px",
                            }}
                          >
                            {snapshot.account.offline
                              ? "纯本地工作区（未接入远程网关）"
                              : snapshot.server_url || "未连接"}
                          </p>
                        </div>
                        <span
                          className={`status-badge-chip ${
                            snapshot.realtime_connected ? "active" : ""
                          }`}
                        >
                          {snapshot.account.offline
                            ? "纯本地"
                            : snapshot.realtime_connected
                              ? "实时双工通道"
                              : "轮询同步"}
                        </span>
                      </div>
                    </div>
                  </div>

                  {/* Card Group 2: 授权工作区文件夹 */}
                  <div className="settings-card-group">
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        marginBottom: "4px",
                      }}
                    >
                      <h4 className="card-group-title">
                        允许 Remote 访问的工作区目录
                      </h4>
                      <button
                        type="button"
                        className="outline-button sm"
                        style={{
                          display: "flex",
                          alignItems: "center",
                          gap: "5px",
                        }}
                        onClick={() => void handleAddWorkspace()}
                        disabled={hostBusy}
                      >
                        <FolderPlus size={13} />
                        <span>添加目录</span>
                      </button>
                    </div>

                    <div className="settings-card">
                      <p
                        style={{
                          margin: "0 0 12px 0",
                          fontSize: "11.5px",
                          color: "var(--text-muted)",
                          lineHeight: 1.45,
                        }}
                      >
                        为保障本机文件安全与隐私，远程端或手机端仅能访问以下已明确授权的工作区目录。默认已为您自动关联当前项目目录。
                      </p>

                      {!hostStatus?.profile.workspace_paths ||
                      hostStatus.profile.workspace_paths.length === 0 ? (
                        <div
                          style={{
                            padding: "16px",
                            textAlign: "center",
                            color: "var(--text-muted)",
                            fontSize: "12px",
                            background: "var(--bg-subtle)",
                            borderRadius: "8px",
                          }}
                        >
                          暂无已授权的工作区文件夹。点击右上角“添加目录”选择项目文件夹。
                        </div>
                      ) : (
                        <div
                          style={{
                            display: "flex",
                            flexDirection: "column",
                            gap: "8px",
                          }}
                        >
                          {hostStatus.profile.workspace_paths.map((wsPath) => (
                            <div
                              key={wsPath}
                              style={{
                                display: "flex",
                                alignItems: "center",
                                justifyContent: "space-between",
                                padding: "8px 12px",
                                background: "var(--bg-subtle)",
                                borderRadius: "6px",
                                border: "1px solid var(--border-subtle)",
                                fontSize: "12px",
                              }}
                            >
                              <div
                                style={{
                                  display: "flex",
                                  alignItems: "center",
                                  gap: "8px",
                                  overflow: "hidden",
                                  textOverflow: "ellipsis",
                                  whiteSpace: "nowrap",
                                  flex: 1,
                                  minWidth: 0,
                                }}
                              >
                                <Folder
                                  size={14}
                                  style={{
                                    color: "var(--text-link)",
                                    flexShrink: 0,
                                  }}
                                />
                                <span
                                  style={{
                                    fontFamily: "monospace",
                                    color: "var(--text-primary)",
                                    overflow: "hidden",
                                    textOverflow: "ellipsis",
                                    whiteSpace: "nowrap",
                                  }}
                                  title={wsPath}
                                >
                                  {wsPath}
                                </span>
                              </div>
                              <button
                                type="button"
                                style={{
                                  background: "none",
                                  border: "none",
                                  color: "var(--text-muted)",
                                  cursor: "pointer",
                                  padding: "4px",
                                  display: "flex",
                                  alignItems: "center",
                                  marginLeft: "8px",
                                  flexShrink: 0,
                                }}
                                title={`移除 ${wsPath}`}
                                onClick={() =>
                                  void handleRemoveWorkspace(wsPath)
                                }
                                disabled={hostBusy}
                              >
                                <X size={14} />
                              </button>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  </div>

                  {/* Card Group 3: 本机会话守护 (agenthubd) */}
                  <div className="settings-card-group">
                    <h4 className="card-group-title">
                      本机会话守护 (agenthubd)
                    </h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>守护进程状态</strong>
                          <p>
                            agenthubd 独立驻留于本机后台，统一管理全
                            Provider（ChatGPT、Claude、ZCode、Antigravity、Grok）会话状态中继、断点续连与多端事件调度。
                          </p>
                        </div>
                        <span className="status-badge-chip active">
                          {hostStatus?.phase !== "stopped" &&
                          hostStatus?.phase !== "loading"
                            ? "守护中 (活跃)"
                            : "未激活"}
                        </span>
                      </div>

                      <div className="settings-divider" />

                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>设备识别码 (Device ID)</strong>
                          <p
                            style={{
                              fontFamily: "monospace",
                              fontSize: "11px",
                            }}
                          >
                            {snapshot.device.device_id}
                          </p>
                        </div>
                        <span className="status-badge-chip">
                          Ed25519 签名防护
                        </span>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {error && <p className="settings-error-banner">{error}</p>}

              <footer className="settings-footer-actions">
                <button
                  type="button"
                  className="settings-cancel-btn"
                  onClick={onClose}
                  disabled={busy}
                >
                  取消
                </button>
                <button
                  type="submit"
                  className="settings-save-btn"
                  disabled={busy}
                >
                  {busy ? (
                    <>
                      <RefreshCw className="spin" size={14} /> 保存中...
                    </>
                  ) : (
                    <>
                      <CheckCircle2 size={14} /> 保存设置
                    </>
                  )}
                </button>
              </footer>
            </form>
          </div>
        </main>
      </div>
    </div>
  );
}
