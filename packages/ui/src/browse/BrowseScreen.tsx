import type { SavedSearchSummary, SearchMode, SearchResultRow, UndoToken } from "core-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { EmptyState } from "../components/EmptyState";
import { UndoNotice } from "../components/UndoNotice";
import { useCore } from "../core";
import { NoteEditor } from "../editor/NoteEditor";
import { PageHeading, useRouter } from "../router";
import { CloseIcon, SortIcon } from "../shell/icons";
import { BottomAction } from "../shell/slot";
import { useBackEntry } from "../shell/useBackEntry";
import { useWideLayout } from "../shell/useMediaQuery";
import { isModalOpen, isTypingTarget, useKeyShortcut } from "../shell/useShortcuts";
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
import { toQuery } from "./query";
import { SavedSheet } from "./SavedSheet";
import { SelectionBar } from "./SelectionBar";
import { SortDialog, sortLabel } from "./SortDialog";
import { differsFrom, useBrowseSearch } from "./useBrowseSearch";
import { useLongPress } from "./useLongPress";
import { useSelection, useTargetSummary } from "./useSelection";
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
 * The selection's keys from 600 px (ADR 0014 decision 2), and on a phone with a keyboard: Escape
 * clears, Ctrl+A chooses every match. A checkbox having focus does not count as typing.
 */
function useSelectionKeys(active: boolean, onEscape: () => void, onSelectAll: () => void) {
  const ref = useRef({ active, onEscape, onSelectAll });
  ref.current = { active, onEscape, onSelectAll };
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.isComposing || event.altKey || isModalOpen()) return;
      const target = event.target;
      const checkbox = target instanceof HTMLInputElement && target.type === "checkbox";
      if (isTypingTarget(target) && !checkbox) return;
      const { active, onEscape, onSelectAll } = ref.current;
      if (event.key === "Escape" && active && !event.ctrlKey && !event.metaKey) {
        event.preventDefault();
        onEscape();
      } else if (event.key.toLowerCase() === "a" && (event.ctrlKey || event.metaKey)) {
        event.preventDefault();
        onSelectAll();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, []);
}

/** On a phone a long press on a row selects it, so the browser must not start selecting text. */
function useNoTextSelectionOnRows(enabled: boolean) {
  useEffect(() => {
    if (!enabled) return;
    const onSelectStart = (event: Event) => {
      const node = event.target;
      const element =
        node instanceof Element ? node : node instanceof Node ? node.parentElement : null;
      if (element?.closest(".browse-row")) event.preventDefault();
    };
    document.addEventListener("selectstart", onSelectStart);
    return () => document.removeEventListener("selectstart", onSelectStart);
  }, [enabled]);
}

/**
 * Browse (step 3.1, ADR 0013): a search box and filter chips, sorting, notes or cards, and the
 * results as a list. On a phone the search panel sits at the bottom in thumb reach and above the
 * keyboard; from 600 px it sits above the list. A row opens its note in the editor (step 3.2b), and
 * what a save or delete did stays in a notice with Undo.
 *
 * Rows can be selected (step 3.2c, ADR 0014 decisions 2 to 4): on a phone with Select or a long
 * press, then a tap toggles; from 600 px with a checkbox, Shift for a range and Ctrl+A for every
 * match. While something is selected the selection bar takes the search panel's place, and the
 * notice sits under it.
 */
export function BrowseScreen() {
  const search = useBrowseSearch();
  const { names, reload } = useNames();
  const { navigate } = useRouter();
  const wide = useWideLayout();
  const saved = useSavedSearches();
  const [open, setOpen] = useState<ChipKind | "sort" | "saved" | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [notice, setNotice] = useState<UndoToken | null>(null);
  /** What Undo on the notice did, kept here since the notice moves when the layout changes. */
  const [undone, setUndone] = useState<string | undefined>(undefined);
  const showNotice = useCallback((token: UndoToken) => {
    setNotice(token);
    setUndone(undefined);
  }, []);
  const box = useRef<HTMLInputElement>(null);
  useKeyShortcut("/", () => box.current?.focus());

  const { state, results, problem } = search;
  const rows = results?.rows ?? [];
  const query = toQuery(state.filters, state.text);
  const selection = useSelection(JSON.stringify([query, state.mode]));
  /** Counts the writes made here, so the selection's counts are read again after each. */
  const [changes, setChanges] = useState(0);
  const summary = useTargetSummary(selection.ids, state.mode, changes);
  const idOf = (row: SearchResultRow) => (state.mode === "notes" ? row.noteId : row.cardId);
  const end = useLoadMore(search.hasMore, rows.length, search.loadMore);
  const deckPaths = new Map(names?.decks.map((d) => [d.id, d.path]));
  const openSaved = saved.searches?.find((s) => s.id === state.savedId) ?? null;

  // A remembered saved search that is gone (deleted here or on another device) is dropped quietly.
  const { setSavedId } = search;
  useEffect(() => {
    if (saved.searches && state.savedId !== null && openSaved === null) setSavedId(null);
  }, [saved.searches, state.savedId, openSaved, setSavedId]);

  // After a write here: the same rows read again, and names a save may have added (a new tag).
  const { searchNow } = search;
  const afterChange = useCallback(() => {
    searchNow();
    reload();
    setChanges((n) => n + 1);
  }, [searchNow, reload]);

  const core = useCore();
  const { replace, enter, leave } = selection;
  const selectAll = useCallback(async () => {
    const out = await core.call("searchIds", { query, mode: state.mode });
    replace(out.ids);
    if (!wide) enter();
  }, [core, query, state.mode, replace, enter, wide]);
  useBackEntry(!wide && selection.active, leave);
  useSelectionKeys(selection.active, leave, () => {
    selectAll().catch(() => {
      // Ctrl+A has nowhere to say it failed; the bar's own Select all does.
    });
  });
  useNoTextSelectionOnRows(!wide);

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
      {!wide && (
        <button type="button" className="button browse-sort" onClick={() => enter()}>
          Select
        </button>
      )}
    </form>
  );
  const problemText = problem && (
    <p id="browse-problem" className="browse-problem" role="alert">
      {problem}
    </p>
  );
  const noticeView = notice && (
    <UndoNotice
      key={notice.id}
      token={notice}
      undone={undone}
      onUndone={(message) => {
        setUndone(message);
        afterChange();
      }}
      onDismiss={() => setNotice(null)}
    />
  );
  const top = selection.active ? (
    <SelectionBar
      selection={selection}
      mode={state.mode}
      onSelectAll={selectAll}
      total={results?.total ?? 0}
      wide={wide}
      names={names}
      summary={summary}
      infoCard={
        selection.size === 1
          ? (rows.find((row) => selection.ids.has(idOf(row)))?.cardId ?? null)
          : null
      }
      onDone={(undo, deleted) => {
        if (undo) showNotice(undo);
        if (deleted) leave();
        afterChange();
      }}
    />
  ) : (
    <div className="browse-panel" data-place={wide ? "top" : "bottom"}>
      {wide ? (
        <>
          {searchBox}
          {problemText}
          {chips}
        </>
      ) : (
        <>
          {chips}
          {searchBox}
          {problemText}
        </>
      )}
    </div>
  );
  // The notice sits under the search panel or the selection bar: above the list from 600 px, in
  // the bottom slot on a phone.
  const panel = wide ? (
    <>
      {top}
      {noticeView}
    </>
  ) : (
    <BottomAction>
      <div className="browse-bottom">
        {top}
        {noticeView}
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
        <ul
          className="browse-list"
          aria-label="Results"
          data-stale={problem !== null}
          data-wide={wide}
          data-checks={selection.active ? "shown" : wide ? "hover" : "hidden"}
        >
          {rows.map((row) => {
            const id = idOf(row);
            return (
              <ResultRow
                key={row.cardId}
                row={row}
                deck={deckPaths.get(row.deckId) ?? ""}
                compact={!wide}
                selecting={selection.active}
                selected={selection.ids.has(id)}
                onOpen={(shift) => {
                  if (!wide && selection.active) selection.toggle(id);
                  else if (shift && selection.size > 0) selection.range(id, rows.map(idOf));
                  else setEditing(row.noteId);
                }}
                onCheck={(shift) => {
                  if (shift) selection.range(id, rows.map(idOf));
                  else selection.toggle(id);
                }}
                onLongPress={() => selection.enter(id)}
              />
            );
          })}
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
      {editing !== null && (
        <NoteEditor
          noteId={editing}
          onClose={() => setEditing(null)}
          onDone={(result) => {
            setEditing(null);
            if (result.undo) showNotice(result.undo);
            afterChange();
          }}
        />
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

/**
 * One result: the sort field, the next field with text, then the deck and the state or due. A tap
 * opens its note (in Cards view, the card's note), or in the phone's selection mode toggles it. A
 * checkbox before it selects it: shown on a phone only in selection mode, and from 600 px on hover
 * and focus, and on every row once one is chosen.
 */
function ResultRow({
  row,
  deck,
  compact,
  selecting,
  selected,
  onOpen,
  onCheck,
  onLongPress,
}: {
  row: SearchResultRow;
  deck: string;
  compact: boolean;
  selecting: boolean;
  selected: boolean;
  /** A tap or click on the row; `shift` when Shift was held. */
  onOpen: (shift: boolean) => void;
  onCheck: (shift: boolean) => void;
  onLongPress: () => void;
}) {
  const title = rowText(row.title);
  const press = useLongPress(compact && !selecting, onLongPress);
  return (
    <li className="browse-item" data-selected={selected}>
      <label className="browse-check">
        <input
          type="checkbox"
          aria-label={`Select ${title || "(empty)"}`}
          checked={selected}
          onChange={(event) => onCheck((event.nativeEvent as MouseEvent).shiftKey === true)}
        />
      </label>
      <button
        type="button"
        className="browse-row"
        onClick={(event) => onOpen(event.shiftKey)}
        // A long press on a phone selects; it must not open the browser's menu.
        onContextMenu={compact ? (event) => event.preventDefault() : undefined}
        {...press}
      >
        <span className="browse-title" data-empty={title === ""}>
          {title || "(empty)"}
        </span>
        <span className="browse-detail">{rowText(row.detail)}</span>
        <span className="browse-meta">
          {deck && <span className="browse-deck">{deck}</span>}
          <span>{formatDue(row)}</span>
        </span>
      </button>
    </li>
  );
}
