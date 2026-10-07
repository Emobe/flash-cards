import type { DeckList, DeckSummary } from "core-client";
import { CoreError } from "core-client";
import { useCallback, useEffect, useState } from "react";
import { EmptyState } from "../components/EmptyState";
import { useCore } from "../core";
import { Link, optionsPath, PageHeading, studyPath, useRouter } from "../router";
import { ChevronIcon, WarningIcon } from "../shell/icons";
import { type DeckAction, DeckDialog } from "./DeckDialog";

const COLLAPSED_KEY = "fc.collapsedDecks";

type Loaded =
  | { status: "loading" }
  | { status: "ready"; list: DeckList }
  | { status: "failed"; error: CoreError };

/** What the undo notice after a delete remembers. */
type Undo = { deckId: string; message: string };

/**
 * Home: the decks with what is left to study today (step 2.2). One tap on a deck starts studying
 * it. The list is read again when the screen opens and when the app is brought back, because a
 * new day or a learning card coming due changes the counts.
 */
export function DecksScreen() {
  const core = useCore();
  const { navigate } = useRouter();
  const [loaded, setLoaded] = useState<Loaded>({ status: "loading" });
  const [managing, setManaging] = useState(false);
  const [action, setAction] = useState<DeckAction | null>(null);
  const [undo, setUndo] = useState<Undo | null>(null);
  const [undoError, setUndoError] = useState<string | null>(null);

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

  async function submit(values: { name: string; parentId: string | null }) {
    if (!action) return;
    if (action.kind === "create") {
      await core.call("createDeck", { name: values.name, parentId: values.parentId });
    } else if (action.kind === "rename") {
      await core.call("renameDeck", { deckId: action.deck.id, name: values.name });
    } else if (action.kind === "move") {
      await core.call("moveDeck", { deckId: action.deck.id, parentId: values.parentId });
    } else {
      const gone = await core.call("deleteDeck", { deckId: action.deck.id });
      setUndoError(null);
      setUndo({
        deckId: action.deck.id,
        message: `Deleted "${action.deck.name}"${deleted(gone.decks, gone.cards)}.`,
      });
    }
    if (action.kind !== "delete") setUndo(null);
    setAction(null);
    load();
  }

  async function restore(deckId: string) {
    try {
      await core.call("restoreDeck", { deckId });
      setUndo(null);
      setUndoError(null);
      load();
    } catch (e) {
      setUndoError(e instanceof CoreError ? e.message : "Could not bring the deck back.");
    }
  }

  const ready = loaded.status === "ready" ? loaded.list : null;

  return (
    <>
      <div className="page-head">
        <PageHeading>Decks</PageHeading>
        {ready && (
          <div className="page-actions">
            <button
              type="button"
              className="button"
              onClick={() => setAction({ kind: "create", parentId: null })}
            >
              Add deck
            </button>
            <button
              type="button"
              className="button"
              aria-pressed={managing}
              onClick={() => setManaging(!managing)}
            >
              {managing ? "Done" : "Manage"}
            </button>
          </div>
        )}
      </div>
      {undo && (
        <div className="deck-undo" role="status">
          <span>{undoError ?? undo.message}</span>
          <button type="button" className="button" onClick={() => restore(undo.deckId)}>
            Undo
          </button>
          <button
            type="button"
            className="button"
            onClick={() => {
              setUndo(null);
              setUndoError(null);
            }}
          >
            Dismiss
          </button>
        </div>
      )}
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
      {ready &&
        (ready.totalCards === 0 && ready.decks.length <= 1 ? (
          <EmptyState action={{ label: "Add your first card", onClick: () => navigate("/add") }}>
            You have no cards yet. Add your first card to start studying.
          </EmptyState>
        ) : (
          <DeckTree decks={ready.decks} managing={managing} onAction={setAction} />
        ))}
      {action && ready && (
        <DeckDialog
          action={action}
          decks={ready.decks}
          onSubmit={submit}
          onClose={() => setAction(null)}
        />
      )}
    </>
  );
}

function deleted(decks: number, cards: number): string {
  const parts = [];
  if (decks > 0) parts.push(`${decks} ${decks === 1 ? "deck" : "decks"} inside`);
  parts.push(`${cards} ${cards === 1 ? "card" : "cards"}`);
  return `, with ${parts.join(" and ")}`;
}

function DeckTree({
  decks,
  managing,
  onAction,
}: {
  decks: DeckSummary[];
  managing: boolean;
  onAction: (action: DeckAction) => void;
}) {
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
      {nothingDue && !managing && (
        <p role="status" className="deck-done">
          You're done for now. Nothing is due today.
        </p>
      )}
      <div className="deck-head" aria-hidden="true" hidden={managing}>
        <span>New</span>
        <span>Learn</span>
        <span>Review</span>
      </div>
      <ul className="deck-list">
        {rows.map((deck) => (
          <DeckRow
            key={deck.id}
            deck={deck}
            expandable={hasChildren.has(deck.id)}
            expanded={!collapsed.has(deck.id)}
            onToggle={() => toggle(deck.id)}
            managing={managing}
            onAction={onAction}
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
  managing,
  onAction,
}: {
  deck: DeckSummary;
  expandable: boolean;
  expanded: boolean;
  onToggle: () => void;
  managing: boolean;
  onAction: (action: DeckAction) => void;
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
      {managing ? (
        <div className="deck-manage">
          <span className="deck-name deck-manage-name">{deck.name}</span>
          <span className="deck-buttons">
            <button
              type="button"
              className="button"
              onClick={() => onAction({ kind: "create", parentId: deck.id })}
            >
              Add inside<span className="visually-hidden"> {deck.name}</span>
            </button>
            <Link path={optionsPath(deck.id)} className="button">
              Options<span className="visually-hidden"> {deck.name}</span>
            </Link>
            <button
              type="button"
              className="button"
              onClick={() => onAction({ kind: "rename", deck })}
            >
              Rename<span className="visually-hidden"> {deck.name}</span>
            </button>
            <button
              type="button"
              className="button"
              onClick={() => onAction({ kind: "move", deck })}
            >
              Move<span className="visually-hidden"> {deck.name}</span>
            </button>
            {!deck.isDefault && (
              <button
                type="button"
                className="button button-danger-text"
                onClick={() => onAction({ kind: "delete", deck })}
              >
                Delete<span className="visually-hidden"> {deck.name}</span>
              </button>
            )}
          </span>
        </div>
      ) : (
        <Link path={studyPath(deck.id)} className="deck-link">
          <span className="deck-name">{deck.name}</span>
          <Count value={deck.newCount} label="new" />
          <Count value={deck.learningCount} label="learning" />
          <Count value={deck.reviewCount} label="to review" />
        </Link>
      )}
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
