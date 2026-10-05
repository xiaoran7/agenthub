"use client";

import { useCallback, useEffect, useState } from "react";
import { RemoteConsole } from "../src/components/RemoteConsole";
import { AccountGate } from "../src/components/AccountGate";
import {
  currentAccount,
  logoutAccount,
  type AccountView,
} from "../src/lib/account";
import { loadConfig, type PanelConfig } from "../src/lib/config";

function defaultSelfHostAccount(): AccountView {
  return {
    user: {
      user_id: "selfhost-owner",
      email: "owner@self-hosted.local",
      display_name: "Self-Host Owner",
      status: "active",
      email_verified_at: new Date(0).toISOString(),
      created_at: new Date(0).toISOString(),
    },
    workspace: {
      workspace_owner_id: "selfhost-owner",
      name: "Default Workspace",
      role: "owner",
      current: true,
    },
    entitlements: {
      max_devices: 999,
      max_concurrent_tasks: 999,
      history_days: 3650,
      remote_tasks: true,
      team_members: 1,
    },
    subscription: {
      plan: "pro",
      status: "active",
    },
  };
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
        if (next !== undefined) {
          setAccount(next);
        } else if (
          typeof window !== "undefined" &&
          window.localStorage.getItem("agenthub_selfhost_direct") === "true"
        ) {
          setAccount(defaultSelfHostAccount());
        } else {
          setAccount(null);
        }
      })
      .catch(() => {
        if (
          typeof window !== "undefined" &&
          window.localStorage.getItem("agenthub_selfhost_direct") === "true"
        ) {
          setAccount(defaultSelfHostAccount());
        } else {
          setAccount(null);
        }
      });
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
  );
}
