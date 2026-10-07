import {
  createContext,
  type MouseEvent,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

/**
 * A small hash router (ADR 0010 decision 2). Every navigation is a history entry, so the browser
 * and Android back buttons pop it. Hash URLs need no server fallback.
 */

export type Route =
  | { name: "decks" | "add" | "browse" | "settings" | "developer"; path: string; title: string }
  | { name: "study"; path: string; title: string; deckId: string };

const DEFAULT_PATH = "/decks";

const titles = {
  decks: "Decks",
  add: "Add",
  browse: "Browse",
  settings: "Settings",
  developer: "Developer tools",
  study: "Study",
} as const;

export function studyPath(deckId: string): string {
  return `/study/${encodeURIComponent(deckId)}`;
}

/** The route for a path such as `/decks` or `/study/42`, or `null` when there is none. */
export function parseRoute(path: string): Route | null {
  const parts = path.split("/").filter(Boolean);
  const [first, second, extra] = parts;
  const flat = { decks: "decks", add: "add", browse: "browse" } as const;
  if (parts.length === 1 && first && first in flat) {
    const name = flat[first as keyof typeof flat];
    return { name, path: `/${first}`, title: titles[name] };
  }
  if (first === "settings" && parts.length === 1) {
    return { name: "settings", path: "/settings", title: titles.settings };
  }
  if (first === "settings" && second === "developer" && !extra) {
    return { name: "developer", path: "/settings/developer", title: titles.developer };
  }
  if (first === "study" && second && !extra) {
    let deckId: string;
    try {
      deckId = decodeURIComponent(second);
    } catch {
      return null;
    }
    return { name: "study", path: studyPath(deckId), title: titles.study, deckId };
  }
  return null;
}

function currentPath(): string {
  return window.location.hash.replace(/^#/, "");
}

type RouterState = {
  route: Route;
  navigate: (path: string) => void;
  back: () => void;
  /** Counts route changes after the first screen, so a page heading knows to take focus. */
  navigations: number;
};

const RouterContext = createContext<RouterState | null>(null);

export function RouterProvider({ children }: { children: ReactNode }) {
  const [route, setRoute] = useState<Route>(() => parseRoute(currentPath()) ?? defaultRoute());
  const [navigations, setNavigations] = useState(0);
  const first = useRef(true);

  useEffect(() => {
    // An unknown or empty hash is replaced, so back does not return to it.
    if (!parseRoute(currentPath())) {
      window.history.replaceState(null, "", `#${DEFAULT_PATH}`);
    }
    const onChange = () => setRoute(parseRoute(currentPath()) ?? defaultRoute());
    window.addEventListener("popstate", onChange);
    window.addEventListener("hashchange", onChange);
    return () => {
      window.removeEventListener("popstate", onChange);
      window.removeEventListener("hashchange", onChange);
    };
  }, []);

  useEffect(() => {
    document.title = `${route.title} · Flash cards`;
    if (first.current) first.current = false;
    else setNavigations((n) => n + 1);
  }, [route]);

  const navigate = useCallback((path: string) => {
    const next = parseRoute(path);
    if (!next || next.path === currentPath()) return;
    window.history.pushState(null, "", `#${next.path}`);
    setRoute(next);
  }, []);

  const back = useCallback(() => window.history.back(), []);

  const value = useMemo(
    () => ({ route, navigate, back, navigations }),
    [route, navigate, back, navigations],
  );
  return <RouterContext.Provider value={value}>{children}</RouterContext.Provider>;
}

function defaultRoute(): Route {
  return { name: "decks", path: DEFAULT_PATH, title: titles.decks };
}

export function useRouter(): RouterState {
  const state = useContext(RouterContext);
  if (!state) throw new Error("useRouter must be used inside a RouterProvider");
  return state;
}

/** A link that navigates without reloading. Modified clicks (new tab) keep their default. */
export function Link({
  path,
  children,
  ...rest
}: { path: string; children: ReactNode } & Omit<
  React.AnchorHTMLAttributes<HTMLAnchorElement>,
  "href" | "onClick"
>) {
  const { navigate } = useRouter();
  function onClick(event: MouseEvent<HTMLAnchorElement>) {
    if (event.defaultPrevented || event.button !== 0) return;
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    event.preventDefault();
    navigate(path);
  }
  return (
    <a href={`#${path}`} onClick={onClick} {...rest}>
      {children}
    </a>
  );
}

/** The page's `h1`. After a navigation it takes focus, so screen readers announce the new page. */
export function PageHeading({ children }: { children: ReactNode }) {
  const { navigations } = useRouter();
  const ref = useRef<HTMLHeadingElement>(null);
  // Not on the first load, only after a route change.
  useEffect(() => {
    if (navigations > 0) ref.current?.focus();
  }, [navigations]);
  return (
    <h1 ref={ref} tabIndex={-1}>
      {children}
    </h1>
  );
}
