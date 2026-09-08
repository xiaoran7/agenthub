import React, { useState, type FormEvent } from "react";
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
  ArrowRight,
  LogOut,
  ExternalLink,
} from "lucide-react";
import type { DesktopPreferences, Snapshot } from "../types";
import { SelectDropdown } from "./SelectDropdown";

interface DesktopSettingsModalProps {
  preferences: DesktopPreferences;
  snapshot: Snapshot;
  onClose: () => void;
  onSaved: (preferences: DesktopPreferences) => void;
  onLogout?: () => void;
}

type SettingsSection =
  | "general"
  | "import"
  | "profile"
  | "appearance"
  | "voice"
  | "config"
  | "personalization"
  | "shortcuts"
  | "billing"
  | "account"
  | "plugins"
  | "browser"
  | "computer"
  | "hooks"
  | "connections"
  | "git"
  | "environment";

export function DesktopSettingsModal({
  preferences,
  snapshot,
  onClose,
  onSaved,
  onLogout,
}: DesktopSettingsModalProps) {
  const [section, setSection] = useState<SettingsSection>("general");
  const [searchQuery, setSearchQuery] = useState("");
  const [refreshSeconds, setRefreshSeconds] = useState(
    preferences.refresh_interval_seconds || 4,
  );
  const [claudeBinary, setClaudeBinary] = useState(
    preferences.claude_binary || "claude",
  );
  const [codexBinary, setCodexBinary] = useState(
    preferences.codex_binary || "codex",
  );
  const [grokBinary, setGrokBinary] = useState(
    preferences.grok_binary || "grok",
  );
  const [defaultCwd, setDefaultCwd] = useState(
    preferences.default_cwd || "d:/ClaudeSpace",
  );
  const [editorTarget, setEditorTarget] = useState(
    preferences.editor_target || "vscode",
  );
  const [terminalShell, setTerminalShell] = useState(
    preferences.terminal_shell || "powershell",
  );
  const [appLanguage, setAppLanguage] = useState(
    preferences.app_language || "auto",
  );
  const [theme, setTheme] = useState(preferences.theme || "dark");
  const [permDefault, setPermDefault] = useState(
    preferences.permission_default ?? true,
  );
  const [permAutoReview, setPermAutoReview] = useState(
    preferences.permission_auto_review ?? true,
  );
  const [permFullAccess, setPermFullAccess] = useState(
    preferences.permission_full_access ?? true,
  );

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [saveSuccess, setSaveSuccess] = useState(false);

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
            codex_binary: codexBinary,
            grok_binary: grokBinary,
          },
        },
      );
      const fullPreferences: DesktopPreferences = {
        ...next,
        default_cwd: defaultCwd,
        editor_target: editorTarget,
        terminal_shell: terminalShell,
        app_language: appLanguage,
        theme,
        permission_default: permDefault,
        permission_auto_review: permAutoReview,
        permission_full_access: permFullAccess,
      };
      window.localStorage.setItem(
        "agenthub:extended-preferences",
        JSON.stringify(fullPreferences),
      );
      onSaved(fullPreferences);
      setSaveSuccess(true);
      setTimeout(() => setSaveSuccess(false), 2000);
    } catch (nextError) {
      setError(
        typeof nextError === "string" ? nextError : "保存设置失败，请检查参数",
      );
    } finally {
      setBusy(false);
    }
  };

  const navItems: Array<{
    group: string;
    items: Array<{ id: SettingsSection; label: string; icon: React.ReactNode }>;
  }> = [
    {
      group: "常规设置",
      items: [
        { id: "general", label: "常规", icon: <Settings size={15} /> },
        { id: "import", label: "导入与 Agent", icon: <Download size={15} /> },
        { id: "profile", label: "个人资料", icon: <User size={15} /> },
        { id: "appearance", label: "外观", icon: <SunMoon size={15} /> },
        { id: "voice", label: "语音", icon: <Mic size={15} /> },
        { id: "config", label: "配置", icon: <Sliders size={15} /> },
        {
          id: "personalization",
          label: "个性化",
          icon: <Sparkles size={15} />,
        },
        { id: "shortcuts", label: "键盘快捷键", icon: <Keyboard size={15} /> },
        {
          id: "billing",
          label: "使用情况和计费",
          icon: <CreditCard size={15} />,
        },
        { id: "account", label: "账户", icon: <CircleUserRound size={15} /> },
      ],
    },
    {
      group: "集成",
      items: [
        { id: "plugins", label: "插件与 MCP", icon: <Puzzle size={15} /> },
        { id: "browser", label: "浏览器控制", icon: <Globe size={15} /> },
        { id: "computer", label: "电脑操控", icon: <Monitor size={15} /> },
      ],
    },
    {
      group: "编码与环境",
      items: [
        { id: "hooks", label: "钩子", icon: <Anchor size={15} /> },
        { id: "connections", label: "连接", icon: <Radio size={15} /> },
        { id: "git", label: "Git 集成", icon: <GitBranch size={15} /> },
        {
          id: "environment",
          label: "环境与 Shell",
          icon: <Terminal size={15} />,
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
                    <h4 className="card-group-title">权限</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>默认权限</strong>
                          <p>
                            默认情况下，Agent
                            可以读取和编辑其工作空间中的文件。需要时，它可以请求额外访问权限。
                          </p>
                        </div>
                        <label className="ios-toggle">
                          <input
                            type="checkbox"
                            checked={permDefault}
                            onChange={(e) => setPermDefault(e.target.checked)}
                          />
                          <span className="toggle-slider" />
                        </label>
                      </div>

                      <div className="settings-divider" />

                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>自动审核</strong>
                          <p>
                            Agent 可以读取和编辑其工作空间中的文件。Agent
                            会自动审查额外访问权限请求。
                          </p>
                        </div>
                        <label className="ios-toggle">
                          <input
                            type="checkbox"
                            checked={permAutoReview}
                            onChange={(e) =>
                              setPermAutoReview(e.target.checked)
                            }
                          />
                          <span className="toggle-slider" />
                        </label>
                      </div>

                      <div className="settings-divider" />

                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>完全访问权限</strong>
                          <p>
                            当 Agent
                            以完整访问权限运行时，它无需你的批准即可编辑你电脑上的任何文件，并运行可访问网络的命令。
                          </p>
                        </div>
                        <label className="ios-toggle">
                          <input
                            type="checkbox"
                            checked={permFullAccess}
                            onChange={(e) =>
                              setPermFullAccess(e.target.checked)
                            }
                          />
                          <span className="toggle-slider" />
                        </label>
                      </div>
                    </div>
                  </div>

                  <div className="settings-card-group">
                    <h4 className="card-group-title">常规偏好</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>默认文件打开目标</strong>
                          <p>默认打开文件和文件夹的编辑器应用。</p>
                        </div>
                        <SelectDropdown
                          value={editorTarget}
                          options={[
                            { value: "vscode", label: "VS Code" },
                            { value: "cursor", label: "Cursor" },
                            { value: "windsurf", label: "Windsurf" },
                            { value: "idea", label: "IntelliJ IDEA" },
                            { value: "system", label: "系统默认" },
                          ]}
                          onChange={(val) => setEditorTarget(val)}
                          size="sm"
                        />
                      </div>

                      <div className="settings-divider" />

                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>集成终端 Shell</strong>
                          <p>选择在集成终端中执行命令的默认 Shell 环境。</p>
                        </div>
                        <SelectDropdown
                          value={terminalShell}
                          options={[
                            { value: "powershell", label: "PowerShell" },
                            { value: "cmd", label: "Command Prompt (CMD)" },
                            { value: "gitbash", label: "Git Bash" },
                            { value: "wsl", label: "WSL (Linux)" },
                          ]}
                          onChange={(val) => setTerminalShell(val)}
                          size="sm"
                        />
                      </div>

                      <div className="settings-divider" />

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

              {/* SECTION: IMPORT & AGENTS */}
              {section === "import" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">本机 Agent CLI 路径</h4>
                    <div className="settings-card">
                      <div className="settings-input-row">
                        <label>
                          <span>Claude Code CLI</span>
                          <input
                            type="text"
                            value={claudeBinary}
                            onChange={(e) => setClaudeBinary(e.target.value)}
                            placeholder="claude"
                            required
                          />
                        </label>
                      </div>
                      <div className="settings-divider" />
                      <div className="settings-input-row">
                        <label>
                          <span>OpenAI Codex CLI</span>
                          <input
                            type="text"
                            value={codexBinary}
                            onChange={(e) => setCodexBinary(e.target.value)}
                            placeholder="codex"
                            required
                          />
                        </label>
                      </div>
                      <div className="settings-divider" />
                      <div className="settings-input-row">
                        <label>
                          <span>xAI Grok Build CLI</span>
                          <input
                            type="text"
                            value={grokBinary}
                            onChange={(e) => setGrokBinary(e.target.value)}
                            placeholder="grok"
                            required
                          />
                        </label>
                      </div>
                    </div>
                  </div>

                  <div className="settings-card-group">
                    <h4 className="card-group-title">本地存储与索引状态</h4>
                    <div className="settings-card">
                      <div className="settings-stat-grid">
                        <div className="stat-box">
                          <span className="stat-label">已发现总会话</span>
                          <strong className="stat-value">
                            {snapshot.local.session_count}
                          </strong>
                        </div>
                        <div className="stat-box">
                          <span className="stat-label">Codex 存储</span>
                          <strong className="stat-value">
                            {(
                              snapshot.local.memory.codex_bytes /
                              1024 /
                              1024
                            ).toFixed(1)}{" "}
                            MB
                          </strong>
                        </div>
                        <div className="stat-box">
                          <span className="stat-label">Claude 存储</span>
                          <strong className="stat-value">
                            {(
                              snapshot.local.memory.claude_bytes / 1024
                            ).toFixed(1)}{" "}
                            KB
                          </strong>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: PROFILE */}
              {section === "profile" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">账号与身份</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>用户名称</strong>
                          <p>
                            {snapshot.account.user.display_name || "未设置"}
                          </p>
                        </div>
                      </div>
                      <div className="settings-divider" />
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>登录邮箱</strong>
                          <p>{snapshot.account.user.email}</p>
                        </div>
                      </div>
                      <div className="settings-divider" />
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>订阅计划</strong>
                          <p>
                            {snapshot.account.subscription.plan.toUpperCase()} ·
                            激活中
                          </p>
                        </div>
                      </div>
                      <div className="settings-divider" />
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>设备 ID</strong>
                          <p className="monospace-text">
                            {snapshot.device.device_id}
                          </p>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: APPEARANCE */}
              {section === "appearance" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">外观主题</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>颜色主题</strong>
                          <p>选择界面的外观模式。</p>
                        </div>
                        <SelectDropdown
                          value={theme}
                          options={[
                            { value: "dark", label: "深色模式 (Dark)" },
                            { value: "light", label: "浅色模式 (Light)" },
                            { value: "system", label: "跟随系统" },
                          ]}
                          onChange={(val) => setTheme(val)}
                          size="sm"
                        />
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: SHORTCUTS */}
              {section === "shortcuts" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">快捷键速查</h4>
                    <div className="settings-card">
                      <div className="shortcut-row">
                        <span>新建任务会话</span>
                        <kbd>Ctrl + N</kbd>
                      </div>
                      <div className="settings-divider" />
                      <div className="shortcut-row">
                        <span>搜索本地会话</span>
                        <kbd>Ctrl + F</kbd>
                      </div>
                      <div className="settings-divider" />
                      <div className="shortcut-row">
                        <span>刷新索引</span>
                        <kbd>Ctrl + R</kbd>
                      </div>
                      <div className="settings-divider" />
                      <div className="shortcut-row">
                        <span>打开设置面板</span>
                        <kbd>Ctrl + ,</kbd>
                      </div>
                      <div className="settings-divider" />
                      <div className="shortcut-row">
                        <span>换行发送</span>
                        <kbd>Shift + Enter</kbd>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: PLUGINS / INTEGRATIONS */}
              {section === "plugins" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">MCP 智能体插件服务</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>Model Context Protocol (MCP)</strong>
                          <p>统一支持工具调用、上下文抓取与多代理通信。</p>
                        </div>
                        <span className="status-badge-chip active">已启用</span>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: BROWSER */}
              {section === "browser" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">浏览器集成</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>Chrome DevTools 探针</strong>
                          <p>支持无头浏览器自动化与页面 DOM 结构抓取。</p>
                        </div>
                        <span className="status-badge-chip active">已就绪</span>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: COMPUTER */}
              {section === "computer" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">
                      电脑操控 (Agent Computer Bridge)
                    </h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>本地桌面操控桥</strong>
                          <p>
                            允许 Agent 驱动屏幕截屏、鼠标键盘交互与窗口聚焦。
                          </p>
                        </div>
                        <span className="status-badge-chip active">运行中</span>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: GIT */}
              {section === "git" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">Git 版本控制</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>CHANGELOG 自动留痕规范</strong>
                          <p>每次代码变更与发版必须同步递增版本并更新留痕。</p>
                        </div>
                        <span className="status-badge-chip active">
                          铁律生效
                        </span>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: ENVIRONMENT */}
              {section === "environment" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">默认工作空间目录</h4>
                    <div className="settings-card">
                      <div className="settings-input-row">
                        <label>
                          <span>默认工作目录</span>
                          <input
                            type="text"
                            value={defaultCwd}
                            onChange={(e) => setDefaultCwd(e.target.value)}
                            placeholder="d:/ClaudeSpace"
                          />
                        </label>
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
                          <strong>当前登录账号</strong>
                          <p>{snapshot.account.user.email}</p>
                        </div>
                        {onLogout && (
                          <button
                            type="button"
                            className="danger-outline-btn"
                            onClick={onLogout}
                          >
                            <LogOut size={14} />
                            <span>退出登录</span>
                          </button>
                        )}
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: BILLING */}
              {section === "billing" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">计费与配额</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>当前方案</strong>
                          <p>
                            {snapshot.account.subscription.plan.toUpperCase()}
                          </p>
                        </div>
                      </div>
                      <div className="settings-divider" />
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>设备上限</strong>
                          <p>
                            当前已绑定 1 台 / 上限{" "}
                            {snapshot.account.entitlements?.max_devices ?? 5} 台
                          </p>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: CONFIG */}
              {section === "config" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">通用配置</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>本地优先模式</strong>
                          <p>对话与执行直接在当前主机运行，保护数据隐私。</p>
                        </div>
                        <span className="status-badge-chip active">已启用</span>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: PERSONALIZATION */}
              {section === "personalization" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">个性化与记忆</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>记忆偏好</strong>
                          <p>允许 Agent 参考跨会话的历史上下文与偏好习惯。</p>
                        </div>
                        <label className="ios-toggle">
                          <input type="checkbox" defaultChecked />
                          <span className="toggle-slider" />
                        </label>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: VOICE */}
              {section === "voice" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">语音输入与输出</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>语音唤醒与识别</strong>
                          <p>启用系统麦克风进行语音指令输入。</p>
                        </div>
                        <label className="ios-toggle">
                          <input type="checkbox" />
                          <span className="toggle-slider" />
                        </label>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: HOOKS */}
              {section === "hooks" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">任务钩子 (Hooks)</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>会话结束自动钩子</strong>
                          <p>支持在回合完成后自动触发构建测试或通知。</p>
                        </div>
                        <label className="ios-toggle">
                          <input type="checkbox" defaultChecked />
                          <span className="toggle-slider" />
                        </label>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {/* SECTION: CONNECTIONS */}
              {section === "connections" && (
                <div className="settings-section-block">
                  <div className="settings-card-group">
                    <h4 className="card-group-title">远程连接</h4>
                    <div className="settings-card">
                      <div className="settings-row-item">
                        <div className="row-text">
                          <strong>服务器端点</strong>
                          <p className="monospace-text">
                            {snapshot.server_url}
                          </p>
                        </div>
                        <span className="status-badge-chip active">
                          {snapshot.realtime_connected ? "实时连接" : "轮询中"}
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
                  className="outline-button"
                  onClick={onClose}
                  disabled={busy}
                >
                  取消
                </button>
                <button
                  type="submit"
                  className="primary-button"
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
