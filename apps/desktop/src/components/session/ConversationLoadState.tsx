import { AlertTriangle, MessageSquare, RefreshCw } from "lucide-react";

export function ConversationLoadState({
  loading,
  error,
  managed,
  onRetry,
}: {
  loading: boolean;
  error?: string;
  managed: boolean;
  onRetry: () => void;
}) {
  return (
    <div className="conversation-empty" role={error ? "alert" : "status"}>
      {error ? <AlertTriangle /> : loading ? <RefreshCw /> : <MessageSquare />}
      <strong>
        {error
          ? "会话内容加载失败"
          : loading
            ? "正在读取会话内容"
            : managed
              ? "暂无可显示的历史消息"
              : "暂不支持继续此会话"}
      </strong>
      <p>
        {error ??
          (loading
            ? "正在连接本机会话服务，请稍候。"
            : managed
              ? "本次读取未返回可显示的内容；这不代表原始会话没有历史。"
              : "此 Provider 尚未接入远程控制。")}
      </p>
      {managed && !loading && (
        <button className="outline-button" type="button" onClick={onRetry}>
          <RefreshCw size={15} />
          重新加载
        </button>
      )}
    </div>
  );
}
