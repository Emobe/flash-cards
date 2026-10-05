import type { CoreClient } from "core-client";
import { createContext, type ReactNode, useContext } from "react";

const CoreContext = createContext<CoreClient | null>(null);

/** Gives the UI its `CoreClient`. The app shell supplies it, so the UI never knows the platform. */
export function CoreProvider({ client, children }: { client: CoreClient; children: ReactNode }) {
  return <CoreContext.Provider value={client}>{children}</CoreContext.Provider>;
}

export function useCore(): CoreClient {
  const client = useContext(CoreContext);
  if (!client) {
    throw new Error("useCore must be used inside a CoreProvider");
  }
  return client;
}
