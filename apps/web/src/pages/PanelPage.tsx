import { Suspense, lazy, useCallback, useEffect, useState } from "react";
import { AccountGate } from "../components/AccountGate";
import {
  currentAccount,
  logoutAccount,
  type AccountView,
} from "../lib/account";
import { loadConfig, type PanelConfig } from "../lib/config";

const RemoteConsole = lazy(() =>
  import("../components/RemoteConsole").then((m) => ({
    default: m.RemoteConsole,
  })),
);

function LoadingScreen() {
  return (
    <div
      className="remote-loading-screen"
      role="status"
      aria-label="正在连接 AgentHub Remote"
    >
      <div className="remote-loading-spinner" />
      <p>正在连接 AgentHub…</p>
    </div>
  );
}

export default function PanelPage() {
  const [account, setAccount] = useState<AccountView | null | undefined>(
    undefined,
  );
  const [config, setConfig] = useState<PanelConfig | undefined>(undefined);

  useEffect(() => {
    setConfig(loadConfig(window.localStorage));
    void currentAccount()
      .then((next) => {
        setAccount(next ?? null);
      })
      .catch(() => {
        setAccount(null);
      });
  }, []);

  const handleRequireLogin = useCallback(() => {
    setAccount(null);
  }, []);

  if (account === undefined) {
    return <LoadingScreen />;
  }

  if (account === null) {
    return (
      <AccountGate
        onAuthenticated={(authenticated) => {
          setAccount(authenticated);
        }}
      />
    );
  }

  return (
    <Suspense fallback={<LoadingScreen />}>
      <RemoteConsole
        account={account}
        config={config}
        onAuthenticationRequired={handleRequireLogin}
        onLoggedOut={() => setAccount(null)}
        onLogout={() => {
          void logoutAccount().finally(() => {
            setAccount(null);
          });
        }}
      />
    </Suspense>
  );
}
