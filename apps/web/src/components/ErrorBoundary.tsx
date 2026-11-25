import { Component, type ErrorInfo, type ReactNode } from "react";

export interface ErrorBoundaryProps {
  children: ReactNode;
  /** `"page"` renders a centered full-viewport fallback; `"item"` renders an inline timeline card. */
  scope?: "page" | "item";
  /** Automatically clears the error state whenever this key changes (e.g. session or item id). */
  resetKey?: string | number | undefined;
  title?: string | undefined;
  description?: string | undefined;
}

interface ErrorBoundaryState {
  hasError: boolean;
}

export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  override state: ErrorBoundaryState = { hasError: false };

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { hasError: true };
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error("[AgentHub ErrorBoundary]", error, info.componentStack);
  }

  override componentDidUpdate(prevProps: ErrorBoundaryProps): void {
    if (this.state.hasError && prevProps.resetKey !== this.props.resetKey) {
      this.setState({ hasError: false });
    }
  }

  private readonly handleReset = (): void => {
    this.setState({ hasError: false });
  };

  private readonly handleReload = (): void => {
    if (typeof window !== "undefined") {
      window.location.reload();
    }
  };

  override render(): ReactNode {
    if (!this.state.hasError) {
      return this.props.children;
    }

    const scope = this.props.scope ?? "page";
    if (scope === "item") {
      return (
        <div
          role="alert"
          className="remote-error-boundary-item"
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 12,
            padding: "10px 14px",
            margin: "6px 0",
            borderRadius: 10,
            border: "1px solid var(--remote-line, #dce7e2)",
            background: "var(--remote-surface, #ffffff)",
            color: "var(--remote-ink, #16221c)",
            fontSize: 13,
          }}
        >
          <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
            <strong>{this.props.title ?? "该条内容渲染异常"}</strong>
            <span
              style={{
                color: "var(--remote-secondary, #46574e)",
                fontSize: 12,
              }}
            >
              {this.props.description ??
                "该消息数据格式异常，已隔离保护其余会话内容。"}
            </span>
          </div>
          <button
            type="button"
            onClick={this.handleReset}
            style={{
              flexShrink: 0,
              padding: "6px 12px",
              borderRadius: 8,
              border: "1px solid var(--remote-line-strong, #bbd1c7)",
              background: "transparent",
              color: "var(--remote-ink, #16221c)",
              fontSize: 12,
              fontWeight: 600,
              cursor: "pointer",
            }}
          >
            重试渲染
          </button>
        </div>
      );
    }

    return (
      <div
        role="alert"
        className="remote-error-boundary-page"
        style={{
          minHeight: "100dvh",
          display: "grid",
          placeItems: "center",
          padding: 24,
          background: "var(--remote-bg, #f7faf8)",
          color: "var(--remote-ink, #16221c)",
          fontFamily: "var(--font-sans, system-ui, sans-serif)",
        }}
      >
        <div
          style={{
            width: "min(100%, 440px)",
            padding: 28,
            borderRadius: 16,
            border: "1px solid var(--remote-line, #dce7e2)",
            background: "var(--remote-surface, #ffffff)",
            boxShadow:
              "var(--remote-modal-shadow, 0 18px 48px rgba(0, 0, 0, 0.08))",
            display: "flex",
            flexDirection: "column",
            gap: 12,
          }}
        >
          <h2 style={{ margin: 0, fontSize: 18, fontWeight: 650 }}>
            {this.props.title ?? "界面渲染遇到意外错误"}
          </h2>
          <p
            style={{
              margin: 0,
              fontSize: 13.5,
              lineHeight: 1.6,
              color: "var(--remote-secondary, #46574e)",
            }}
          >
            {this.props.description ??
              "当前视图未能正常显示，你可以尝试恢复或刷新页面。"}
          </p>
          <div style={{ display: "flex", gap: 10, marginTop: 6 }}>
            <button
              type="button"
              onClick={this.handleReset}
              style={{
                padding: "8px 16px",
                borderRadius: 9,
                border: "1px solid var(--remote-line-strong, #bbd1c7)",
                background: "var(--remote-surface, #ffffff)",
                color: "var(--remote-ink, #16221c)",
                fontSize: 13,
                fontWeight: 600,
                cursor: "pointer",
              }}
            >
              尝试恢复
            </button>
            <button
              type="button"
              onClick={this.handleReload}
              style={{
                padding: "8px 16px",
                borderRadius: 9,
                border: "1px solid transparent",
                background: "var(--remote-terracotta, #267861)",
                color: "#ffffff",
                fontSize: 13,
                fontWeight: 600,
                cursor: "pointer",
              }}
            >
              刷新页面
            </button>
          </div>
        </div>
      </div>
    );
  }
}
