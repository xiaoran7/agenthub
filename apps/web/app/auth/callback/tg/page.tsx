import { Suspense } from "react";
import { TgOAuthCallbackClient } from "../../../../src/components/TgOAuthCallbackClient";

export default function TgOAuthCallbackPage() {
  return (
    <Suspense
      fallback={<div style={{ minHeight: "100dvh", background: "#11110f" }} />}
    >
      <TgOAuthCallbackClient />
    </Suspense>
  );
}
