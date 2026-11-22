import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import ResetPasswordPage from "../pages/ResetPasswordPage";
import { initThemeListener } from "../lib/theme";
import "../styles/globals.css";

initThemeListener();

const container = document.getElementById("root");
if (container === null) {
  throw new Error("agenthub-web: 缺少 #root 挂载点");
}

createRoot(container).render(
  <StrictMode>
    <ResetPasswordPage />
  </StrictMode>,
);
