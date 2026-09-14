"use client";

import { useCallback, useEffect, useState } from "react";
import { RemoteConsole } from "../src/components/RemoteConsole";
import { AccountGate } from "../src/components/AccountGate";
import {
  currentAccount,
  logoutAccount,
  type AccountView,
} from "../src/lib/account";
import { DeviceConnectDialog } from "../src/components/DeviceConnectDialog";
import { loadConfig, type PanelConfig } from "../src/lib/config";

export default function PanelPage() {
  const [account, setAccount] = useState<AccountView | null | undefined>(
    undefined,
  );
  const [config, setConfig] = useState<PanelConfig | undefined>(undefined);
  const [deviceConnectOpen, setDeviceConnectOpen] = useState(false);

  useEffect(() => {
    setConfig(loadConfig(window.localStorage));
    void currentAccount()
      .then((next) => setAccount(next ?? null))
      .catch(() => setAccount(null));
  }, []);

  const handleRequireLogin = useCallback(() => {
    setAccount(null);
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
    <>
      <RemoteConsole
        onAuthenticationRequired={handleRequireLogin}
        onManage={() => setDeviceConnectOpen(true)}
        account={account}
        onLogout={() => {
          void logoutAccount().finally(() => {
            setAccount(null);
          });
        }}
      />
      {deviceConnectOpen && config !== undefined && (
        <DeviceConnectDialog
          config={config}
          onClose={() => setDeviceConnectOpen(false)}
        />
      )}
    </>
  );
}
