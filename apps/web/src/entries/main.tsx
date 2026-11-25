import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { ErrorBoundary } from "../components/ErrorBoundary";
import PanelPage from "../pages/PanelPage";
import { initPwaInstallMonitor } from "../lib/pwa-install";
import { initThemeListener } from "../lib/theme";
import "../styles/globals.css";

initThemeListener();
initPwaInstallMonitor();

const container = document.getElementById("root");
if (container === null) {
  throw new Error("agenthub-web: 缺少 #root 挂载点");
}

createRoot(container).render(
  <StrictMode>
    <ErrorBoundary scope="page">
      <PanelPage />
    </ErrorBoundary>
  </StrictMode>,
);
