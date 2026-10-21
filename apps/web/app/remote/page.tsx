"use client";

import { useCallback, useEffect, useState } from "react";
import { RemoteConsole } from "../../src/components/RemoteConsole";
import { AccountGate } from "../../src/components/AccountGate";
import { currentAccount, type AccountView } from "../../src/lib/account";

export default function RemotePage() {
  const [account, setAccount] = useState<AccountView | null | undefined>(
    undefined,
  );
  const requireLogin = useCallback(() => setAccount(null), []);

  // The console needs the signed-in account so its requests carry an
  // identity. Without it the Gateway has no way to tell whose sessions to
  // answer with.
  useEffect(() => {
    void currentAccount()
      .then((next) => setAccount(next ?? null))
      .catch(() => setAccount(null));
  }, []);

  if (account === undefined) {
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

  if (account === null) {
    return (
      <AccountGate
        onAuthenticated={(authenticated) => {
          setAccount(authenticated);
          void currentAccount().then((next) => {
            if (next !== undefined) setAccount(next);
          });
        }}
      />
    );
  }

  return (
    <RemoteConsole
      account={account}
      onAuthenticationRequired={requireLogin}
      onLoggedOut={() => setAccount(null)}
    />
  );
}
