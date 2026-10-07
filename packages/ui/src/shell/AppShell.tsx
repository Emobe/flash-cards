import { type ReactNode, useCallback, useState } from "react";
import { Link, useRouter } from "../router";
import { destinations } from "./destinations";
import { BackIcon } from "./icons";
import { ShortcutsDialog } from "./ShortcutsDialog";
import { BottomActionSlot, KeyboardInsetContext } from "./slot";
import { useKeyboardInset } from "./useKeyboardInset";
import { useShortcuts } from "./useShortcuts";
import "./shell.css";

/**
 * Owns the navigation, the system-bar and cutout insets, and the keyboard inset, so no screen
 * does (ADR 0010 decisions 2 and 4). Screens render as `children`. `bare` drops the navigation and
 * the back control, for the collection problem screen.
 */
export function AppShell({ children, bare = false }: { children: ReactNode; bare?: boolean }) {
  const { route, navigate, back } = useRouter();
  const keyboardInset = useKeyboardInset();
  const [slot, setSlot] = useState<HTMLElement | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);

  // Study is full screen: no navigation, a back control instead. `bare` (the collection could not
  // be opened) has neither.
  const fullScreen = bare || route.name === "study";

  const goTo = useCallback(
    (letter: string) => {
      const target = destinations.find((d) => d.shortcut === letter);
      if (target) navigate(target.path);
      return target !== undefined;
    },
    [navigate],
  );
  const showHelp = useCallback(() => setHelpOpen(true), []);
  const closeHelp = useCallback(() => setHelpOpen(false), []);
  useShortcuts({ enabled: !fullScreen, goTo, showHelp });

  return (
    <div
      className="shell"
      data-nav={fullScreen ? "off" : "on"}
      data-keyboard={keyboardInset > 0 ? "open" : "closed"}
      style={{ "--keyboard-inset": `${keyboardInset}px` } as React.CSSProperties}
    >
      {!fullScreen && (
        <nav className="shell-nav" aria-label="Main">
          <ul>
            {destinations.map((d) => (
              <li key={d.id}>
                <Link
                  path={d.path}
                  aria-current={d.routes.includes(route.name) ? "page" : undefined}
                >
                  {d.icon}
                  <span>{d.label}</span>
                </Link>
              </li>
            ))}
          </ul>
        </nav>
      )}
      <main className="shell-main">
        {fullScreen && !bare && (
          <button type="button" className="shell-back" onClick={back} aria-label="Back">
            <BackIcon />
          </button>
        )}
        <KeyboardInsetContext.Provider value={keyboardInset}>
          <BottomActionSlot.Provider value={slot}>
            <div className="shell-page">{children}</div>
          </BottomActionSlot.Provider>
        </KeyboardInsetContext.Provider>
      </main>
      <div className="shell-action" ref={setSlot} />
      <ShortcutsDialog open={helpOpen} onClose={closeHelp} />
    </div>
  );
}
