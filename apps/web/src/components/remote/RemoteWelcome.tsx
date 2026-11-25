export interface RemoteWelcomeProps {
  loading: boolean;
  isMobile: boolean;
  onNewSession: () => void;
  onOpenSettings: () => void;
}

export function RemoteWelcome({
  loading,
  isMobile,
  onNewSession,
  onOpenSettings,
}: RemoteWelcomeProps) {
  return (
    <div key="welcome" className="remote-welcome remote-view-enter">
      <span className="remote-eyebrow">YOUR WORK IN FLOW</span>
      <h2>{loading ? "正在连接…" : "打开一个会话，继续刚才的工作。"}</h2>
      <p>
        消息、工具执行与权限审批均直接同步自你的原生终端 Agent。
        即便离开网页，已派发的工作流依然在电脑后台稳步推进。
      </p>
      <div className="remote-welcome-chips">
        {[
          {
            title: "新建会话",
            desc: "在指定设备工作区派发新 Agent",
            action: onNewSession,
          },
          {
            title: "外观设置",
            desc: "在青岚浅色与森墨深色间切换",
            action: onOpenSettings,
          },
        ].map((chip) => (
          <button
            key={chip.title}
            type="button"
            className="remote-welcome-chip"
            onClick={chip.action}
          >
            <strong>{chip.title}</strong>
            <span>{chip.desc}</span>
          </button>
        ))}
      </div>
      {isMobile && (
        <button
          type="button"
          className="remote-mobile-fab"
          onClick={onNewSession}
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.4"
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
          <span>新任务</span>
        </button>
      )}
    </div>
  );
}
