export function PulseMark({ size = 24 }: { size?: number }) {
  return (
    <span
      className="pulse-mark"
      aria-hidden="true"
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        width: size,
        height: size,
      }}
    >
      <svg viewBox="0 0 512 512" width={size} height={size}>
        <g
          stroke="var(--brand-accent, #c2410c)"
          strokeWidth="20"
          strokeLinecap="round"
          strokeLinejoin="round"
          fill="none"
        >
          <line x1="256" y1="256" x2="256" y2="136" />
          <line x1="256" y1="256" x2="360" y2="196" />
          <line x1="256" y1="256" x2="360" y2="316" />
          <line x1="256" y1="256" x2="256" y2="376" />
          <line x1="256" y1="256" x2="152" y2="316" />
          <line x1="256" y1="256" x2="152" y2="196" />
          <polygon points="256,136 360,196 360,316 256,376 152,316 152,196" />
        </g>
        <g
          stroke="var(--brand-accent, #c2410c)"
          strokeWidth="14"
          fill="var(--bg-surface, #ffffff)"
        >
          <circle cx="256" cy="136" r="22" />
          <circle cx="360" cy="196" r="22" />
          <circle cx="360" cy="316" r="22" />
          <circle cx="256" cy="376" r="22" />
          <circle cx="152" cy="316" r="22" />
          <circle cx="152" cy="196" r="22" />
        </g>
        <circle
          cx="256"
          cy="256"
          r="32"
          fill="var(--brand-accent, #c2410c)"
          opacity="0.18"
        />
        <circle cx="256" cy="256" r="18" fill="var(--brand-accent, #c2410c)" />
      </svg>
    </span>
  );
}

export function Brand({ size = 26 }: { size?: number }) {
  return (
    <div
      className="brand"
      style={{ display: "flex", alignItems: "center", gap: "10px" }}
    >
      <PulseMark size={size} />
      <div
        style={{ display: "flex", flexDirection: "column", lineHeight: 1.15 }}
      >
        <span
          style={{
            fontWeight: 800,
            fontSize: "15px",
            letterSpacing: "0.02em",
            color: "var(--text-heading, var(--text-primary))",
          }}
        >
          Agent{" "}
          <span
            style={{
              background: "linear-gradient(90deg, #00f5d4, #00bbf9, #a855f7)",
              WebkitBackgroundClip: "text",
              WebkitTextFillColor: "transparent",
            }}
          >
            Hub
          </span>
        </span>
        <span
          style={{
            fontSize: "10px",
            color: "var(--text-muted)",
            letterSpacing: "0.06em",
          }}
        >
          智能体集成中心
        </span>
      </div>
    </div>
  );
}
