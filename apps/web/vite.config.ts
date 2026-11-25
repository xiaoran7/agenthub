import { fileURLToPath, URL } from "node:url";

import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";

/**
 * The shared contract package ships raw TypeScript sources, so every browser
 * side import has to be pointed at the source file rather than a build
 * artifact. Regex aliases keep the four subpath exports distinct: a plain
 * string alias would swallow `/remote`, `/account` and `/avatar-rules` as
 * prefixes of the bare package specifier.
 */
const contractsSource = (name: string): string =>
  fileURLToPath(
    new URL(`../../packages/contracts/src/${name}.ts`, import.meta.url),
  );

const contractsAlias = [
  {
    find: /^@agent-pulse\/contracts\/remote$/,
    replacement: contractsSource("remote"),
  },
  {
    find: /^@agent-pulse\/contracts\/account$/,
    replacement: contractsSource("account"),
  },
  {
    find: /^@agent-pulse\/contracts\/avatar-rules$/,
    replacement: contractsSource("avatar-rules"),
  },
  {
    find: /^@agent-pulse\/contracts$/,
    replacement: contractsSource("index"),
  },
];

/**
 * Every physical size a browser or OS may ask for, served as a native PNG so
 * a high-DPI taskbar never has to resample a frame.
 */
const ICON_SIZES = [
  28, 30, 36, 42, 48, 56, 64, 72, 96, 128, 192, 256, 512,
] as const;

const sharedHead = [
  '<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content" />',
  '<meta name="mobile-web-app-capable" content="yes" />',
  '<meta name="apple-mobile-web-app-capable" content="yes" />',
  '<meta name="apple-mobile-web-app-status-bar-style" content="default" />',
  '<link rel="manifest" href="/manifest.webmanifest" />',
  ...ICON_SIZES.map(
    (size) =>
      `<link rel="icon" type="image/png" sizes="${size}x${size}" href="/icon-${size}.png" />`,
  ),
  '<link rel="icon" type="image/x-icon" sizes="32x32" href="/favicon.ico" />',
  '<link rel="apple-touch-icon" href="/icon-192.png" />',
  '<meta name="theme-color" media="(prefers-color-scheme: light)" content="#f7faf8" />',
  '<meta name="theme-color" media="(prefers-color-scheme: dark)" content="#0f1110" />',
  // Applied before the first paint so the console never flashes the wrong
  // theme while React boots, loaded externally so CSP needs no 'unsafe-inline'.
  '<script src="/theme-init.js"></script>',
].join("\n    ");

/**
 * A static build has no server component to render `metadata`/`viewport`, so
 * the shared head block is injected into every entry at build time.
 */
function agenthubHeadPlugin(): Plugin {
  return {
    name: "agenthub-head",
    transformIndexHtml: {
      order: "pre",
      handler(html) {
        return html.replace("<!--agenthub-head-->", sharedHead);
      },
    },
  };
}

const entry = (path: string): string =>
  fileURLToPath(new URL(path, import.meta.url));

export default defineConfig({
  plugins: [react(), agenthubHeadPlugin()],
  resolve: {
    alias: contractsAlias,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // The console ships as a handful of hashed bundles; the default warning
    // threshold would fire on the React runtime chunk for no reason.
    chunkSizeWarningLimit: 900,
    rollupOptions: {
      input: {
        main: entry("index.html"),
        remote: entry("remote/index.html"),
        verifyEmail: entry("verify-email/index.html"),
        resetPassword: entry("reset-password/index.html"),
      },
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (
            id.includes("/react/") ||
            id.includes("/react-dom/") ||
            id.includes("/scheduler/")
          ) {
            return "vendor-react";
          }
          if (
            id.includes("react-markdown") ||
            id.includes("remark-") ||
            id.includes("micromark") ||
            id.includes("mdast-") ||
            id.includes("hast-") ||
            id.includes("unist-") ||
            id.includes("vfile")
          ) {
            return "vendor-markdown";
          }
          return undefined;
        },
      },
    },
  },
  server: {
    port: Number(process.env.WEB_DEV_PORT ?? 5173),
    proxy: {
      "/v1": {
        target: process.env.GATEWAY_ORIGIN ?? "http://127.0.0.1:3001",
        changeOrigin: false,
        ws: true,
      },
      "/v3": {
        target: process.env.GATEWAY_ORIGIN ?? "http://127.0.0.1:3001",
        changeOrigin: false,
        ws: true,
      },
    },
  },
  preview: {
    port: Number(process.env.WEB_DEV_PORT ?? 5173),
  },
});
