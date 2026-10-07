import { createContext, type ReactNode, useContext } from "react";

/** What differs per platform and is not the core. Each app shell supplies it. */
export type Platform = {
  /** URL of the card-frame page (`card/frame.html`), which the platform serves (ADR 0005). */
  cardFrameUrl: string;
  /**
   * Tells the platform which theme is in effect, so the system bars and title bar match (ADR 0010).
   * Called on start and on every change of the effective theme.
   */
  setSystemTheme(theme: "light" | "dark"): void;
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
