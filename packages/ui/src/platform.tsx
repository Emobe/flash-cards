import { createContext, type ReactNode, useContext } from "react";

/** What differs per platform and is not the core. Each app shell supplies it. */
export type Platform = {
  /** URL of the card-frame page (`card/frame.html`), which the platform serves (ADR 0005). */
  cardFrameUrl: string;
  /**
   * Tells the platform which theme is in effect, so the system bars and title bar match (ADR 0010).
   * Called on start and on every change of the effective theme. `followSystem` is true when the
   * user's setting is System: a desktop window must then follow the desktop theme again instead of
   * being pinned to `theme`, or `prefers-color-scheme` would stop following the system.
   */
  /** Whether the device keeps a backups folder (the native apps), so Settings lists backups. */
  localBackups?: boolean;
  setSystemTheme(theme: "light" | "dark", followSystem: boolean): void;
};

const PlatformContext = createContext<Platform | null>(null);

export function PlatformProvider({
  platform,
  children,
}: {
  platform: Platform;
  children: ReactNode;
}) {
  return <PlatformContext.Provider value={platform}>{children}</PlatformContext.Provider>;
}

export function usePlatform(): Platform {
  const platform = useContext(PlatformContext);
  if (!platform) {
    throw new Error("usePlatform must be used inside a PlatformProvider");
  }
  return platform;
}
