import type { SavedSearchSummary, SearchMode, SearchResultRow } from "core-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { EmptyState } from "../components/EmptyState";
import { useCore } from "../core";
import { PageHeading, useRouter } from "../router";
import { CloseIcon, SortIcon } from "../shell/icons";
import { BottomAction } from "../shell/slot";
import { useWideLayout } from "../shell/useMediaQuery";
import { useKeyShortcut } from "../shell/useShortcuts";
import { ChipRow } from "./ChipRow";
import {
  addedLabels,
  answeredLabels,
  type ChipKind,
  chipNames,
  chosen,
  dueLabels,
  multiple,
  type Names,
  stateLabels,
  withChosen,
} from "./chips";
import { formatDue, rowText } from "./format";
import { PickerDialog, type PickerOption } from "./PickerDialog";
import { SavedSheet } from "./SavedSheet";
import { SortDialog, sortLabel } from "./SortDialog";
import { differsFrom, useBrowseSearch } from "./useBrowseSearch";
import "./browse.css";

/**
 * The deck, tag and note type names for the pickers, chip labels and rows. Read when the screen
 * opens, when the app comes back, and when a picker opens. A failed read keeps what was there.
 */
function useNames() {
  const core = useCore();
  const [names, setNames] = useState<Names | null>(null);
  const load = useCallback(() => {
    Promise.all([
      core.call("getDeckList", null),
      core.call("getTags", null),
      core.call("getNoteTypes", null),
    ])
      .then(([decks, tags, noteTypes]) =>
        setNames({
          decks: decks.decks,
          tags: tags.tags,
          noteTypes: noteTypes.noteTypes.map((n) => n.name),
          totalCards: decks.totalCards,
        }),
      )
      .catch(() => {
        // The search still works without them: chips show what was chosen, rows no deck.
      });
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
  return { names, reload: load };
}

/**
 * The saved searches, read when the screen opens and after each write. `reload` answers once the
 * list is in, so a caller can mark a new one open only after the list has it. A failed read keeps
 * what was there.
 */
function useSavedSearches() {
  const core = useCore();
  const [searches, setSearches] = useState<SavedSearchSummary[] | null>(null);
  const reload = useCallback(
    () =>
      core
        .call("getSavedSearches", null)
        .then((list) => setSearches(list.searches))
        .catch(() => {
          // The screen works without them; the sheet shows what it last had.
        }),
    [core],
  );
  useEffect(() => {
    void reload();
  }, [reload]);
  return { searches, reload };
}

function options(kind: ChipKind, names: Names | null): PickerOption[] {
  const labelled = (labels: Record<string, string>) =>
    Object.entries(labels).map(([value, label]) => ({ value, label }));
  switch (kind) {
    case "decks":
      return (names?.decks ?? []).map((d) => ({
        value: d.path,
        label: d.name,
        depth: d.depth,
        match: d.path,
      }));
    case "tags":
      return (names?.tags ?? []).map((t) => ({ value: t, label: t }));
    case "noteTypes":
      return (names?.noteTypes ?? []).map((n) => ({ value: n, label: n }));
    case "states":
      return labelled(stateLabels);
    case "due":
      return labelled(dueLabels);
    case "added":
      return labelled(addedLabels);
    case "answered":
      return labelled(answeredLabels);
  }
}

function count(total: number, mode: SearchMode): string {
  const unit = mode === "notes" ? "note" : "card";
  return `${total.toLocaleString()} ${unit}${total === 1 ? "" : "s"}`;
}

/** Loads the next page when the end of the list comes near, and again if it is still near. */
function useLoadMore(hasMore: boolean, rows: number, loadMore: () => Promise<void>) {
  const ref = useRef<HTMLDivElement>(null);
  const near = useRef(false);
  const loadRef = useRef(loadMore);
  loadRef.current = loadMore;
  useEffect(() => {
    const end = ref.current;
    if (!end || !hasMore || typeof IntersectionObserver !== "function") return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        near.current = entry?.isIntersecting ?? false;
        if (near.current) void loadRef.current();
      },
      { rootMargin: "0px 0px 600px 0px" },
    );
    observer.observe(end);
    return () => {
      observer.disconnect();
      near.current = false;
    };
  }, [hasMore]);
  useEffect(() => {
    if (near.current && hasMore && rows > 0) void loadRef.current();
  }, [rows, hasMore]);
  return ref;
}

/**
 * Browse (step 3.1, ADR 0013): a search box and filter chips, sorting, notes or cards, and the
 * results as a list. On a phone the search panel sits at the bottom in thumb reach and above the
 * keyboard; from 600 px it sits above the list. Rows do nothing when tapped yet (3.2 edits).
 */
export function BrowseScreen() {
  const search = useBrowseSearch();
  const { names, reload } = useNames();
  const { navigate } = useRouter();
  const wide = useWideLayout();
  const saved = useSavedSearches();
  const [open, setOpen] = useState<ChipKind | "sort" | "saved" | null>(null);
  const box = useRef<HTMLInputElement>(null);
  useKeyShortcut("/", () => box.current?.focus());

  const { state, results, problem } = search;
  const rows = results?.rows ?? [];
  const end = useLoadMore(search.hasMore, rows.length, search.loadMore);
  const deckPaths = new Map(names?.decks.map((d) => [d.id, d.path]));
  const openSaved = saved.searches?.find((s) => s.id === state.savedId) ?? null;

  // A remembered saved search that is gone (deleted here or on another device) is dropped quietly.
  const { setSavedId } = search;
  useEffect(() => {
    if (saved.searches && state.savedId !== null && openSaved === null) setSavedId(null);
  }, [saved.searches, state.savedId, openSaved, setSavedId]);

  function openPicker(kind: ChipKind) {
    if (kind === "decks" || kind === "tags" || kind === "noteTypes") reload();
    setOpen(kind);
  }

  const chips = (
    <ChipRow
      filters={state.filters}
      names={names}
      onSaved={() => {
        void saved.reload();
        setOpen("saved");
      }}
      onOpen={openPicker}
      onClear={(kind) => search.setFilters(withChosen(state.filters, kind, []))}
    />
  );
  const searchBox = (
    <form
      className="browse-box"
      aria-label="Search"
      onSubmit={(event) => {
        event.preventDefault();
        search.searchNow();
      }}
    >
      <input
        ref={box}
        type="search"
        aria-label="Search your cards"
        placeholder="Search your cards"
        value={state.text}
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        enterKeyHint="search"
        aria-invalid={problem !== null}
        aria-describedby={problem ? "browse-problem" : undefined}
        onChange={(event) => search.setText(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape" && state.text !== "") {
            event.preventDefault();
            search.clearText();
          }
        }}
      />
      <button
        type="button"
        className="button browse-sort"
        title={sortLabel(state.sort)}
        onClick={() => setOpen("sort")}
      >
        <SortIcon />
        <span>Sort</span>
      </button>
    </form>
  );
  const problemText = problem && (
    <p id="browse-problem" className="browse-problem" role="alert">
      {problem}
    </p>
  );
  const panel = wide ? (
    <div className="browse-panel" data-place="top">
      {searchBox}
      {problemText}
      {chips}
    </div>
  ) : (
    <BottomAction>
      <div className="browse-panel" data-place="bottom">
        {chips}
        {searchBox}
        {problemText}
      </div>
    </BottomAction>
  );

  const noCards = names?.totalCards === 0;
  return (
    <>
      <div className="browse-head">
        <PageHeading>Browse</PageHeading>
        <p className="browse-count" role="status">
          {search.searching || !results ? "Searching…" : count(results.total, state.mode)}
        </p>
        {openSaved && (
          <p className="browse-saved">
            <span>
              Saved: <strong>{openSaved.name}</strong>
              {differsFrom(state, openSaved) && <span className="browse-changed"> · changed</span>}
            </span>
            <button
              type="button"
              className="browse-saved-close"
              aria-label={`Close saved search ${openSaved.name}`}
              title="Close saved search"
              onClick={() => setSavedId(null)}
            >
              <CloseIcon />
            </button>
          </p>
        )}
      </div>
      {panel}
      {results &&
        results.total === 0 &&
        problem === null &&
        (noCards ? (
          <EmptyState action={{ label: "Add your first card", onClick: () => navigate("/add") }}>
            You have no cards yet. Add your first card to start studying.
          </EmptyState>
        ) : (
          <EmptyState action={{ label: "Clear filters", onClick: search.clearSearch }}>
            No cards match. Try fewer filters.
          </EmptyState>
        ))}
      {rows.length > 0 && (
        <ul className="browse-list" aria-label="Results" data-stale={problem !== null}>
          {rows.map((row) => (
            <ResultRow key={row.cardId} row={row} deck={deckPaths.get(row.deckId) ?? ""} />
          ))}
        </ul>
      )}
      {search.hasMore && (
        <div ref={end} className="browse-more">
          <button
            type="button"
            className="button"
            disabled={search.loadingMore}
            onClick={() => void search.loadMore()}
          >
            {search.loadingMore ? "Loading more…" : "Show more"}
          </button>
        </div>
      )}
      {open === "sort" && (
        <SortDialog
          sort={state.sort}
          mode={state.mode}
          onSort={search.setSort}
          onMode={search.setMode}
          onClose={() => setOpen(null)}
        />
      )}
      {open === "saved" && (
        <SavedSheet
          state={state}
          searches={saved.searches}
          reload={saved.reload}
          onOpen={search.openSaved}
          onOpened={setSavedId}
          onClose={() => setOpen(null)}
        />
      )}
      {open !== null && open !== "sort" && open !== "saved" && (
        <PickerDialog
          key={open}
          title={chipNames[open]}
          multiple={multiple[open]}
          options={options(open, names)}
          selected={chosen(state.filters, open)}
          filterable={open === "decks" || open === "tags"}
          onApply={(values) => {
            setOpen(null);
            search.setFilters(withChosen(state.filters, open, values));
          }}
          onClose={() => setOpen(null)}
        />
      )}
    </>
  );
}

/** One result: the sort field, the next field with text, then the deck and the state or due. */
function ResultRow({ row, deck }: { row: SearchResultRow; deck: string }) {
  const title = rowText(row.title);
  return (
    <li className="browse-row">
      <span className="browse-title" data-empty={title === ""}>
        {title || "(empty)"}
      </span>
      <span className="browse-detail">{rowText(row.detail)}</span>
      <span className="browse-meta">
        {deck && <span className="browse-deck">{deck}</span>}
        <span>{formatDue(row)}</span>
      </span>
    </li>
  );
}
