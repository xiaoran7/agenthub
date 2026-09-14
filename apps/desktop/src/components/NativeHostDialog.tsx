import { invoke } from "@tauri-apps/api/core";
import { FolderPlus, Radio, RadioTower, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

interface Profile {
  workspace_paths: string[];
  codex_binary: string;
  remote_enabled?: boolean;
}

interface Status {
  phase:
    "loading" | "starting" | "stopped" | "local" | "connecting" | "connected";
  profile: Profile;
}

interface HostPollState {
  phase: Status["phase"];
  profile: Profile;
  initialized: boolean;
  error: string;
}

export function applyHostStatus(
  current: HostPollState,
  status: Status,
  forceProfile = false,
): HostPollState {
  const profileToUse =
    forceProfile || !current.initialized ? status.profile : current.profile;
  return {
    phase: status.phase,
    profile: {
      ...profileToUse,
      workspace_paths: (profileToUse.workspace_paths ?? []).map((p) =>
        p.replace(/^\\\\\?\\/u, ""),
      ),
    },
    initialized: true,
    error: "",
  };
}

const initialPollState: HostPollState = {
  phase: "loading",
  profile: {
    workspace_paths: [],
    codex_binary: "",
    remote_enabled: false,
  },
  initialized: false,
  error: "",
};

const statusCopy: Record<Status["phase"], string> = {
  loading: "正在读取本机状态",
  starting: "agenthubd 正在启动 Codex Runtime",
  stopped: "agenthubd 将在首次启动 Codex 会话时自动运行",
  local: "本地会话可用，Remote 当前关闭",
  connecting: "Remote 已开启，正在连接 Gateway",
  connected: "Remote 已连接，其他设备可继续当前会话",
};

export function NativeHostDialog({ onClose }: { onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [pollState, setPollState] = useState(initialPollState);
  const { profile, phase } = pollState;
  const [actionError, setActionError] = useState("");
  const [busy, setBusy] = useState(false);
  const remoteEnabled = phase === "connecting" || phase === "connected";
  const daemonRunning = phase !== "loading" && phase !== "stopped";

  const refreshStatus = async (forceProfile = false) => {
    const status = await invoke<Status>("remote_host_status");
    setPollState((current) => applyHostStatus(current, status, forceProfile));
  };

  useEffect(() => {
    dialog.current?.showModal();
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const status = await invoke<Status>("remote_host_status");
        if (disposed) return;
        setPollState((current) => applyHostStatus(current, status));
      } catch (failure) {
        if (!disposed)
          setPollState((current) => ({
            ...current,
            error: String(failure),
          }));
      }
      if (!disposed) timer = setTimeout(() => void poll(), 1200);
    };
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, []);

  const selectWorkspace = async () => {
    try {
      const path = await invoke<string | null>("pick_folder", {
        startDir: profile.workspace_paths.at(-1) ?? null,
      });
      if (path) {
        const cleanPath = path.replace(/^\\\\\?\\/u, "");
        setPollState((current) => ({
          ...current,
          profile: {
            ...current.profile,
            workspace_paths: [
              ...new Set([...current.profile.workspace_paths, cleanPath]),
            ],
          },
        }));
      }
    } catch (failure) {
      setActionError(String(failure));
    }
  };

  const setRemote = async (enabled: boolean) => {
    setBusy(true);
    setActionError("");
    try {
      if (enabled) {
        await invoke("start_remote_host", { input: profile });
      } else {
        await invoke("stop_remote_host");
      }
      await refreshStatus(true);
    } catch (failure) {
      setActionError(String(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <dialog ref={dialog} className="remote-launch" onCancel={onClose}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!remoteEnabled) void setRemote(true);
        }}
      >
        <header>
          <div>
            <RadioTower size={18} />
            <h2>本机与 Remote</h2>
          </div>
          <button type="button" onClick={onClose} aria-label="关闭">
            <X size={16} />
          </button>
        </header>

        <div className="daemon-status-line" data-phase={phase} role="status">
          <i aria-hidden="true" />
          <span>{statusCopy[phase]}</span>
        </div>
        {(actionError || pollState.error) && (
          <p role="alert">{actionError || pollState.error}</p>
        )}

        <label>
          Codex 可执行文件
          <input
            value={profile.codex_binary}
            disabled={busy || phase !== "stopped"}
            onChange={(event) =>
              setPollState((current) => ({
                ...current,
                profile: {
                  ...current.profile,
                  codex_binary: event.target.value,
                },
              }))
            }
            placeholder="codex 或 Codex 可执行文件路径"
          />
        </label>

        <div className="remote-workspace-heading">
          <span>允许 Remote 使用的工作目录</span>
          <button
            type="button"
            disabled={busy || daemonRunning}
            onClick={() => void selectWorkspace()}
            title="添加工作目录"
          >
            <FolderPlus size={14} />
            添加
          </button>
        </div>
        <div className="remote-workspace-list">
          {profile.workspace_paths.length === 0 ? (
            <p>开启前先添加至少一个工作目录。</p>
          ) : (
            profile.workspace_paths.map((path) => (
              <div key={path}>
                <span title={path}>{path}</span>
                <button
                  type="button"
                  disabled={busy || daemonRunning}
                  onClick={() =>
                    setPollState((current) => ({
                      ...current,
                      profile: {
                        ...current.profile,
                        workspace_paths: current.profile.workspace_paths.filter(
                          (item) => item !== path,
                        ),
                      },
                    }))
                  }
                  aria-label={`移除 ${path}`}
                >
                  <X size={13} />
                </button>
              </div>
            ))
          )}
        </div>

        <footer>
          <span>
            关闭 Remote 只断开 Gateway，本机 agenthubd 和正在运行的 Session
            会继续。
          </span>
          <button
            type={remoteEnabled ? "button" : "submit"}
            className={remoteEnabled ? "" : "remote-primary"}
            disabled={
              busy ||
              phase === "loading" ||
              phase === "starting" ||
              (!remoteEnabled &&
                (!profile.workspace_paths.length ||
                  !profile.codex_binary.trim()))
            }
            onClick={remoteEnabled ? () => void setRemote(false) : undefined}
          >
            <Radio size={14} />
            {busy ? "正在更新" : remoteEnabled ? "关闭 Remote" : "开启 Remote"}
          </button>
        </footer>
      </form>
    </dialog>
  );
}
