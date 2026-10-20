"use client";

export const dynamic = "force-dynamic";
export const revalidate = 0;

import { useCallback, useState } from "react";
import { RemoteConsole } from "../../src/components/RemoteConsole";
import { AccountGate } from "../../src/components/AccountGate";
export default function RemotePage() {
  const [login, setLogin] = useState(false);
  const requireLogin = useCallback(() => setLogin(true), []);
  return login ? (
    <AccountGate onAuthenticated={() => setLogin(false)} />
  ) : (
    <RemoteConsole onAuthenticationRequired={requireLogin} />
  );
}
