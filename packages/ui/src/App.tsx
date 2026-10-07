import type { ReactNode } from "react";
import "./components/components.css";
import { isDeveloperBuild } from "./developer";
import { RouterProvider, useRouter } from "./router";
import { CollectionProblem } from "./screens/CollectionProblem";
import { DecksScreen } from "./screens/DecksScreen";
import { DeveloperScreen } from "./screens/DeveloperScreen";
import { AddScreen, BrowseScreen } from "./screens/placeholders";
import { SettingsScreen } from "./screens/SettingsScreen";
import { StudyScreen } from "./screens/StudyScreen";
import { AppShell } from "./shell/AppShell";
import { ThemeProvider } from "./theme";
import { useCollectionState } from "./useCollectionState";

/**
 * The root of the UI. Shared by every platform, so it must not import Tauri or any other platform
 * API (see docs/adr/0001-workspace-layout.md).
 */
export function App({ extraDeveloperTools }: { extraDeveloperTools?: ReactNode }) {
  return (
    <ThemeProvider>
      <RouterProvider>
        <Screens extraDeveloperTools={extraDeveloperTools} />
      </RouterProvider>
    </ThemeProvider>
  );
}

function Screens({ extraDeveloperTools }: { extraDeveloperTools?: ReactNode }) {
  const collection = useCollectionState();
  const { route } = useRouter();

  if (collection.status === "problem") {
    return (
      <AppShell bare>
        <CollectionProblem error={collection.error} />
      </AppShell>
    );
  }
  if (collection.status === "opening") {
    return (
      <AppShell bare>
        <p role="status">Opening your cards...</p>
      </AppShell>
    );
  }
  return <AppShell>{screenFor(route, extraDeveloperTools)}</AppShell>;
}

function screenFor(
  route: ReturnType<typeof useRouter>["route"],
  extraDeveloperTools: ReactNode,
): ReactNode {
  const developer = isDeveloperBuild();
  switch (route.name) {
    case "decks":
      return <DecksScreen />;
    case "add":
      return <AddScreen />;
    case "browse":
      return <BrowseScreen />;
    case "settings":
      return <SettingsScreen developerTools={developer} />;
    case "developer":
      // A release build has no such screen: the link is gone, and a typed URL lands on Settings.
      return developer ? (
        <DeveloperScreen extraTools={extraDeveloperTools} />
      ) : (
        <SettingsScreen developerTools={false} />
      );
    case "study":
      return <StudyScreen deckId={route.deckId} />;
  }
}
