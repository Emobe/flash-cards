import type { ReactElement } from "react";
import { AddIcon, BrowseIcon, DecksIcon, SettingsIcon } from "./icons";

export type Destination = {
  id: string;
  label: string;
  icon: ReactElement;
  path: string;
  /** Pressed after `g` to go here. */
  shortcut: string;
  /** Routes that count as being inside this destination, for `aria-current`. */
  routes: readonly string[];
};

/** The main navigation, in order. Adding a destination is one entry here (ADR 0010). */
export const destinations: readonly Destination[] = [
  {
    id: "decks",
    label: "Decks",
    icon: <DecksIcon />,
    path: "/decks",
    shortcut: "d",
    routes: ["decks"],
  },
  { id: "add", label: "Add", icon: <AddIcon />, path: "/add", shortcut: "a", routes: ["add"] },
  {
    id: "browse",
    label: "Browse",
    icon: <BrowseIcon />,
    path: "/browse",
    shortcut: "b",
    routes: ["browse"],
  },
  {
    id: "settings",
    label: "Settings",
    icon: <SettingsIcon />,
    path: "/settings",
    shortcut: "s",
    routes: ["settings", "developer"],
  },
];
