import type { SVGProps } from "react";

export type IconName =
  | "overview"
  | "sessions"
  | "devices"
  | "activity"
  | "diagnostics"
  | "settings"
  | "help"
  | "search"
  | "chevron-down"
  | "chevron-right"
  | "external"
  | "close"
  | "arrow-left"
  | "arrow-right"
  | "filter"
  | "clock"
  | "warning"
  | "check"
  | "server";

interface IconProps extends SVGProps<SVGSVGElement> {
  name: IconName;
  size?: number;
}

export function Icon({ name, size = 18, ...props }: IconProps) {
  const common = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.7,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    "aria-hidden": true,
    focusable: false,
    ...props,
  };

  switch (name) {
    case "overview":
      return (
        <svg {...common}>
          <rect x="3" y="3" width="7" height="7" rx="1.5" />
          <rect x="14" y="3" width="7" height="7" rx="1.5" />
          <rect x="3" y="14" width="7" height="7" rx="1.5" />
          <rect x="14" y="14" width="7" height="7" rx="1.5" />
        </svg>
      );
    case "sessions":
      return (
        <svg {...common}>
          <rect x="4" y="3" width="16" height="18" rx="2" />
          <path d="M8 8h8M8 12h8M8 16h5" />
          <path d="M6.5 8h.01M6.5 12h.01M6.5 16h.01" strokeWidth="2.5" />
        </svg>
      );
    case "devices":
      return (
        <svg {...common}>
          <rect x="3" y="4" width="18" height="12" rx="2" />
          <path d="M8 20h8M12 16v4" />
        </svg>
      );
    case "activity":
      return (
        <svg {...common}>
          <path d="M3 12h4l2.2-6 4.1 12 2.4-6H21" />
        </svg>
      );
    case "diagnostics":
      return (
        <svg {...common}>
          <path d="m14.7 6.3 3-3 3 3-3 3" />
          <path d="m17.7 9.3-6.9 6.9" />
          <path d="M8.5 7.5a5.5 5.5 0 0 0 6.4 7.8L21 21" />
          <path d="M3 21h6" />
        </svg>
      );
    case "settings":
      return (
        <svg {...common}>
          <path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4" />
          <circle cx="12" cy="12" r="4" />
        </svg>
      );
    case "help":
      return (
        <svg {...common}>
          <circle cx="12" cy="12" r="9" />
          <path d="M9.7 9a2.4 2.4 0 1 1 3.9 1.9c-1.1.8-1.6 1.2-1.6 2.6" />
          <path d="M12 17h.01" strokeWidth="2.4" />
        </svg>
      );
    case "search":
      return (
        <svg {...common}>
          <circle cx="10.8" cy="10.8" r="6.5" />
          <path d="m16 16 4.5 4.5" />
        </svg>
      );
    case "chevron-down":
      return (
        <svg {...common}>
          <path d="m6 9 6 6 6-6" />
        </svg>
      );
    case "chevron-right":
      return (
        <svg {...common}>
          <path d="m9 6 6 6-6 6" />
        </svg>
      );
    case "external":
      return (
        <svg {...common}>
          <path d="M14 5h5v5M19 5l-8 8" />
          <path d="M19 14v4a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h4" />
        </svg>
      );
    case "close":
      return (
        <svg {...common}>
          <path d="m6 6 12 12M18 6 6 18" />
        </svg>
      );
    case "arrow-left":
      return (
        <svg {...common}>
          <path d="m14 5-7 7 7 7M7 12h13" />
        </svg>
      );
    case "arrow-right":
      return (
        <svg {...common}>
          <path d="m10 5 7 7-7 7M4 12h13" />
        </svg>
      );
    case "filter":
      return (
        <svg {...common}>
          <path d="M4 6h16M7 12h10M10 18h4" />
        </svg>
      );
    case "clock":
      return (
        <svg {...common}>
          <circle cx="12" cy="12" r="8.5" />
          <path d="M12 7v5l3.2 2" />
        </svg>
      );
    case "warning":
      return (
        <svg {...common}>
          <path d="m12 3 9 17H3L12 3Z" />
          <path d="M12 9v4M12 16h.01" strokeWidth="2.2" />
        </svg>
      );
    case "check":
      return (
        <svg {...common}>
          <path d="m5 12 4.5 4.5L19 7" />
        </svg>
      );
    case "server":
      return (
        <svg {...common}>
          <rect x="4" y="4" width="16" height="6" rx="1.5" />
          <rect x="4" y="14" width="16" height="6" rx="1.5" />
          <path d="M8 7h.01M8 17h.01M12 7h5M12 17h5" strokeWidth="2" />
        </svg>
      );
  }
}
