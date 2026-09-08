import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

interface Profile {
  workspace_paths: string[];
  codex_binary: string;
}
interface Status {
  phase: string;
  profile: Profile;
}
const labels: Record<string, string> = {
  stopped: "未连接",
  connecting: "正在连接，或等待网络恢复",
  connected: "已连接到你的 Hub",
  stopping: "正在停止",
  failed: "连接未能继续，请检查 Codex 路径、登录与 Gateway 设置。",
};

export function NativeHostDialog({ onClose }: { onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [profile, setProfile] = useState<Profile>({
    workspace_paths: [],
    codex_binary: "",
  });
  const [phase, setPhase] = useState("loading");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const editable = ["stopped", "failed"].includes(phase) && !busy;
  useEffect(() => {
    dialog.current?.showModal();
    let stopped = false;
    let initial = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const status = await invoke<Status>("remote_host_status");
        if (stopped) return;
        setPhase(status.phase);
        if (initial) {
          setProfile(status.profile);
          initial = false;
        }
      } catch (failure) {
        if (!stopped) setError(String(failure));
      }
      if (!stopped) timer = setTimeout(() => void poll(), 1000);
    };
    void poll();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, []);
  async function selectWorkspace() {
    try {
      const path = await invoke<string | null>("pick_folder", {
        startDir: profile.workspace_paths.at(-1) ?? null,
      });
      if (path)
        setProfile((previous) => ({
          ...previous,
          workspace_paths: [...new Set([...previous.workspace_paths, path])],
        }));
    } catch (failure) {
      setError(String(failure));
    }
  }
  async function changeConnection(start: boolean) {
    setBusy(true);
    setError("");
    try {
      await invoke(
        start ? "start_remote_host" : "stop_remote_host",
        start ? { input: profile } : undefined,
      );
      const status = await invoke<Status>("remote_host_status");
      setPhase(status.phase);
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusy(false);
    }
  }
  return (
    <dialog ref={dialog} className="remote-launch" onCancel={onClose}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (editable) void changeConnection(true);
        }}
      >
        <header>
          <h2>连接这台电脑</h2>
          <button type="button" onClick={onClose} aria-label="关闭">
            ×
          </button>
        </header>
        <p role="status">{labels[phase] ?? "正在读取本机设置…"}</p>
        {error && <p role="alert">{error}</p>}
        <label>
          Codex 可执行文件
          <input
            value={profile.codex_binary}
            disabled={!editable}
            onChange={(event) =>
              setProfile({ ...profile, codex_binary: event.target.value })
            }
            placeholder="Codex 原生程序路径"
          />
        </label>
        <p>
          Windows 请使用 Codex 安装目录中的 codex.exe。登录由 Codex 自己管理。
        </p>
        <label>允许远程使用的工作目录</label>
        {profile.workspace_paths.map((path) => (
          <div
            key={path}
            style={{ display: "flex", gap: 8, alignItems: "center" }}
          >
            <span style={{ overflowWrap: "anywhere", flex: 1 }}>{path}</span>
            <button
              type="button"
              disabled={!editable}
              onClick={() =>
                setProfile({
                  ...profile,
                  workspace_paths: profile.workspace_paths.filter(
                    (item) => item !== path,
                  ),
                })
              }
            >
              移除
            </button>
          </div>
        ))}
        <button
          type="button"
          disabled={!editable}
          onClick={() => void selectWorkspace()}
        >
          选择目录
        </button>
        <p>
          选中目录内的新会话可从其他设备继续，正文与工具操作会同步到你的
          Hub。请保持桌面应用开启；停止连接可能中断当前任务。
        </p>
        {editable ? (
          <button
            className="remote-primary"
            disabled={
              !profile.workspace_paths.length || !profile.codex_binary.trim()
            }
            type="submit"
          >
            启动连接
          </button>
        ) : (
          <button
            type="button"
            disabled={busy || phase === "stopping" || phase === "loading"}
            onClick={() => void changeConnection(false)}
          >
            停止本机连接
          </button>
        )}
      </form>
    </dialog>
  );
}
