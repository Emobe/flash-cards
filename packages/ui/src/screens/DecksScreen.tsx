import type { DeckList, DeckSummary } from "core-client";
import { CoreError } from "core-client";
import { useCallback, useEffect, useState } from "react";
import { EmptyState } from "../components/EmptyState";
import { useCore } from "../core";
import { Link, PageHeading, studyPath, useRouter } from "../router";
import { ChevronIcon, WarningIcon } from "../shell/icons";

const COLLAPSED_KEY = "fc.collapsedDecks";

type Loaded =
  | { status: "loading" }
  | { status: "ready"; list: DeckList }
  | { status: "failed"; error: CoreError };

/**
 * Home: the decks with what is left to study today (step 2.2). One tap on a deck starts studying
 * it. The list is read again when the screen opens and when the app is brought back, because a
 * new day or a learning card coming due changes the counts.
 */
export function DecksScreen() {
  const core = useCore();
  const { navigate } = useRouter();
  const [loaded, setLoaded] = useState<Loaded>({ status: "loading" });

  const load = useCallback(() => {
    core
      .call("getDeckList", null)
      .then((list) => setLoaded({ status: "ready", list }))
      .catch((error: unknown) =>
        setLoaded((previous) =>
          // A refresh that fails keeps what is on screen. Only a first load shows the error.
          previous.status === "ready"
            ? previous
            : {
                status: "failed",
                error:
                  error instanceof CoreError
                    ? error
                    : new CoreError("internal", "Something went wrong while reading your decks."),
              },
        ),
      );
  }, [core]);

  useEffect(() => {
    load();
    const onVisible = () => {
      if (document.visibilityState === "visible") load();
    };
    window.addEventListener("focus", load);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.removeEventListener("focus", load);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [load]);

  return (
    <>
      <PageHeading>Decks</PageHeading>
      {loaded.status === "loading" && <p role="status">Loading your decks...</p>}
      {loaded.status === "failed" && (
        <div className="problem" role="alert">
          <WarningIcon />
          <p>{loaded.error.message}</p>
          <button
            type="button"
            className="button"
            onClick={() => {
              setLoaded({ status: "loading" });
              load();
            }}
          >
            Try again
          </button>
        </div>
      )}
      {loaded.status === "ready" &&
        (loaded.list.totalCards === 0 ? (
          <EmptyState action={{ label: "Add your first card", onClick: () => navigate("/add") }}>
            You have no cards yet. Add your first card to start studying.
          </EmptyState>
        ) : (
          <DeckTree decks={loaded.list.decks} />
        ))}
    </>
  );
}

function DeckTree({ decks }: { decks: DeckSummary[] }) {
  const [collapsed, setCollapsed] = useState<Set<string>>(readCollapsed);
  const hasChildren = new Set(decks.flatMap((d) => (d.parentId ? [d.parentId] : [])));
  const nothingDue = decks.every((d) => d.newCount + d.learningCount + d.reviewCount === 0);

  function toggle(id: string) {
    const next = new Set(collapsed);
    if (!next.delete(id)) next.add(id);
    setCollapsed(next);
    try {
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...next]));
    } catch {
      // The choice only lasts until the app closes.
    }
  }

  // The list is in tree order, so what is inside a collapsed deck is the run of deeper decks after it.
  const rows: DeckSummary[] = [];
  let hiddenBelow: number | null = null;
  for (const deck of decks) {
    if (hiddenBelow !== null && deck.depth > hiddenBelow) continue;
    hiddenBelow = null;
    rows.push(deck);
    if (collapsed.has(deck.id) && hasChildren.has(deck.id)) hiddenBelow = deck.depth;
  }

  return (
    <>
      {nothingDue && (
        <p role="status" className="deck-done">
          You're done for now. Nothing is due today.
        </p>
      )}
      <div className="deck-head" aria-hidden="true">
        <span>New</span>
        <span>Learning</span>
        <span>To review</span>
      </div>
      <ul className="deck-list">
        {rows.map((deck) => (
          <DeckRow
            key={deck.id}
            deck={deck}
            expandable={hasChildren.has(deck.id)}
            expanded={!collapsed.has(deck.id)}
            onToggle={() => toggle(deck.id)}
          />
        ))}
      </ul>
    </>
  );
}

function DeckRow({
  deck,
  expandable,
  expanded,
  onToggle,
}: {
  deck: DeckSummary;
  expandable: boolean;
  expanded: boolean;
  onToggle: () => void;
}) {
  return (
    <li className="deck-row" style={{ "--depth": deck.depth } as React.CSSProperties}>
      {expandable ? (
        <button
          type="button"
          className="deck-toggle"
          aria-expanded={expanded}
          aria-label={`${deck.name}, decks inside`}
          onClick={onToggle}
        >
          <ChevronIcon />
        </button>
      ) : (
        <span className="deck-toggle" aria-hidden="true" />
      )}
      <Link path={studyPath(deck.id)} className="deck-link">
        <span className="deck-name">{deck.name}</span>
        <Count value={deck.newCount} label="new" />
        <Count value={deck.learningCount} label="learning" />
        <Count value={deck.reviewCount} label="to review" />
      </Link>
    </li>
  );
}

/** The number is what people see. The label is for screen readers, since the column heads are not read. */
function Count({ value, label }: { value: number; label: string }) {
  return (
    <span className="deck-count" data-zero={value === 0}>
      {value}
      <span className="visually-hidden"> {label}</span>
    </span>
  );
}

function readCollapsed(): Set<string> {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? "[]");
    return new Set(Array.isArray(parsed) ? parsed.filter((x) => typeof x === "string") : []);
  } catch {
    return new Set();
  }
}
