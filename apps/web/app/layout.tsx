import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";

import "./globals.css";

export const metadata: Metadata = {
  title: "AgentHub - 远程接续原生 Agent 会话",
  description: "从 Web 接续电脑上的原生 Agent 会话。",
  manifest: "/manifest.webmanifest",
  icons: {
    // Every physical size a browser or OS may ask for, served as a native
    // PNG so a high-DPI taskbar never has to resample a frame.
    icon: [
      { url: "/icon-28.png", sizes: "28x28", type: "image/png" },
      { url: "/icon-30.png", sizes: "30x30", type: "image/png" },
      { url: "/icon-36.png", sizes: "36x36", type: "image/png" },
      { url: "/icon-42.png", sizes: "42x42", type: "image/png" },
      { url: "/icon-48.png", sizes: "48x48", type: "image/png" },
      { url: "/icon-56.png", sizes: "56x56", type: "image/png" },
      { url: "/icon-64.png", sizes: "64x64", type: "image/png" },
      { url: "/icon-72.png", sizes: "72x72", type: "image/png" },
      { url: "/icon-96.png", sizes: "96x96", type: "image/png" },
      { url: "/icon-128.png", sizes: "128x128", type: "image/png" },
      { url: "/icon-192.png", sizes: "192x192", type: "image/png" },
      { url: "/icon-256.png", sizes: "256x256", type: "image/png" },
      { url: "/icon-512.png", sizes: "512x512", type: "image/png" },
      { url: "/favicon.ico", sizes: "32x32" },
      { url: "/icon.svg", type: "image/svg+xml" },
    ],
    apple: "/icon-192.png",
  },
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  viewportFit: "cover",
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#FAF8F5" },
    { media: "(prefers-color-scheme: dark)", color: "#1F1E1D" },
  ],
};

const antiFoucScript = `(function(){try{var t=localStorage.getItem("agenthub_theme")||"system";var d=t==="dark"||(t==="system"&&window.matchMedia("(prefers-color-scheme: dark)").matches);document.documentElement.setAttribute("data-theme",d?"dark":"light");document.documentElement.style.colorScheme=d?"dark":"light";}catch(e){}})();`;

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="zh-CN" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: antiFoucScript }} />
      </head>
      <body>{children}</body>
    </html>
  );
}
