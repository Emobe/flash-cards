import type { ReactNode } from "react";
import "./components/components.css";
import { RouterProvider, useRouter } from "./router";
import { CollectionProblem } from "./screens/CollectionProblem";
import { DeveloperScreen } from "./screens/DeveloperScreen";
import { AddScreen, BrowseScreen, DecksScreen, StudyScreen } from "./screens/placeholders";
import { SettingsScreen } from "./screens/SettingsScreen";
import { AppShell } from "./shell/AppShell";
import { ThemeProvider } from "./theme";
import { useCollectionState } from "./useCollectionState";

/**
 * The root of the UI. Shared by every platform, so it must not import Tauri or any other platform
 * API (see docs/adr/0001-workspace-layout.md).
 */
export function App() {
  return (
    <ThemeProvider>
      <RouterProvider>
        <Screens />
      </RouterProvider>
    </ThemeProvider>
  );
}

function Screens() {
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
  return <AppShell>{screenFor(route)}</AppShell>;
}

function screenFor(route: ReturnType<typeof useRouter>["route"]): ReactNode {
  switch (route.name) {
    case "decks":
      return <DecksScreen />;
    case "add":
      return <AddScreen />;
    case "browse":
      return <BrowseScreen />;
    case "settings":
      return <SettingsScreen developerTools />;
    case "developer":
      return <DeveloperScreen />;
    case "study":
      return <StudyScreen deckId={route.deckId} />;
  }
}
