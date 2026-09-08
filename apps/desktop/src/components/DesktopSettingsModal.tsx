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
  onOpenLocalHost?: () => void;
}

type SettingsSection =
  | "general"
  | "import"
  | "connections"
  | "account";

export function DesktopSettingsModal({
  preferences,
  snapshot,
  onClose,
  onSaved,
  onLogout,
  onOpenLocalHost,
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
      group: "核心设置",
      items: [
        { id: "general", label: "常规与刷新", icon: <Settings size={15} /> },
        { id: "import", label: "Agent 路径", icon: <Download size={15} /> },
        { id: "connections", label: "远程连接与 Host", icon: <Radio size={15} /> },
        { id: "account", label: "账户与退出", icon: <CircleUserRound size={15} /> },
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
                      {onOpenLocalHost && (
                        <>
                          <div className="settings-divider" />
                          <div className="settings-row-item">
                            <div className="row-text">
                              <strong>本机 Remote Host 守护</strong>
                              <p>
                                启动本机 Codex 远程接管
                                Host，支持工作目录配置与多端接续。
                              </p>
                            </div>
                            <button
                              type="button"
                              className="outline-button"
                              onClick={() => {
                                onClose();
                                onOpenLocalHost();
                              }}
                            >
                              管理本机连接
                            </button>
                          </div>
                        </>
                      )}
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
