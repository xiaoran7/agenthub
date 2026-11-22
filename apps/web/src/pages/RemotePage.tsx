import { Suspense, lazy, useCallback, useEffect, useState } from "react";
import { AccountGate } from "../components/AccountGate";
import {
  currentAccount,
  logoutAccount,
  type AccountView,
} from "../lib/account";

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

export default function RemotePage() {
  const [account, setAccount] = useState<AccountView | null | undefined>(
    undefined,
  );
  const requireLogin = useCallback(() => setAccount(null), []);

  useEffect(() => {
    void currentAccount()
      .then((next) => setAccount(next ?? null))
      .catch(() => setAccount(null));
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
        onAuthenticationRequired={requireLogin}
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
