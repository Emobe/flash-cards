import { CoreError, type DeckSummary, type NoteTypeSummary } from "core-client";
import type { EditorView } from "prosemirror-view";
import { useCallback, useEffect, useRef, useState } from "react";
import { useCore } from "../core";
import { highestCloze, insertMedia } from "../editor/commands";
import { FieldEditor } from "../editor/FieldEditor";
import { MediaRefused, prepareImage, prepareSound } from "../editor/media";
import { Toolbar } from "../editor/Toolbar";
import { PageHeading } from "../router";
import { WarningIcon } from "../shell/icons";
import { BottomAction, useShellKeyboardInset } from "../shell/slot";
import { TagInput, withTyped } from "./add/TagInput";
import "./add/add.css";

const LAST_KEY = "fc.add.last";
const DRAFT_KEY = "fc.add.draft";
const DRAFT_DELAY_MS = 300;
const DUPLICATE_DELAY_MS = 400;

type Draft = {
  deckId: string | null;
  noteTypeId: string | null;
  /** Field HTML by field name, so a note type with the same names keeps what was typed. */
  fields: Record<string, string>;
  tags: string[];
};

function read<T>(key: string): Partial<T> | null {
  try {
    const text = localStorage.getItem(key);
    const value: unknown = text ? JSON.parse(text) : null;
    return value && typeof value === "object" ? (value as Partial<T>) : null;
  } catch {
    return null;
  }
}

function write(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // The draft only lasts until the app closes.
  }
}

function readDraft(): Draft {
  const last = read<{ deckId: string; noteTypeId: string }>(LAST_KEY);
  const draft = read<Draft>(DRAFT_KEY);
  const fields: Record<string, string> = {};
  for (const [name, html] of Object.entries(draft?.fields ?? {})) {
    if (typeof html === "string") fields[name] = html;
  }
  return {
    deckId: draft?.deckId ?? last?.deckId ?? null,
    noteTypeId: draft?.noteTypeId ?? last?.noteTypeId ?? null,
    fields,
    tags: Array.isArray(draft?.tags) ? draft.tags.filter((t) => typeof t === "string") : [],
  };
}

type Loaded =
  | { status: "loading" }
  | { status: "failed"; error: CoreError }
  | {
      status: "ready";
      decks: DeckSummary[];
      noteTypes: NoteTypeSummary[];
      knownTags: string[];
    };

function asCoreError(error: unknown, fallback: string): CoreError {
  return error instanceof CoreError ? error : new CoreError("internal", fallback);
}

function fieldKey(type: NoteTypeSummary, fieldId: string): string {
  return `${type.id}:${fieldId}`;
}

function cardsText(count: number): string {
  return `${count} ${count === 1 ? "card" : "cards"}`;
}

/**
 * Add a note (step 2.4, ADR 0011 decision 5): deck, note type, one editor per field, tags. The
 * deck, note type and tags stay after Add, and what is typed is kept as a draft on this device, so
 * an accidental back or Android closing the app does not lose it.
 */
export function AddScreen() {
  const core = useCore();
  const [loaded, setLoaded] = useState<Loaded>({ status: "loading" });
  const [initial] = useState(readDraft);
  const [chosenDeck, setChosenDeck] = useState<string | null>(initial.deckId);
  const [chosenType, setChosenType] = useState<string | null>(initial.noteTypeId);
  const [values, setValues] = useState<Record<string, string>>(initial.fields);
  const [tags, setTags] = useState<string[]>(initial.tags);
  const [typedTag, setTypedTag] = useState("");
  const [pending, setPending] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [duplicate, setDuplicate] = useState(false);
  // The editor the toolbar acts on, and a counter that re-renders the toolbar after each change.
  const [active, setActive] = useState<EditorView | null>(null);
  const [, setTick] = useState(0);
  const [mediaBusy, setMediaBusy] = useState(false);
  const views = useRef(new Map<string, EditorView>());
  // Files picked or fetched in this visit, so a picture just added is not read back from the core.
  const mediaCache = useRef(new Map<string, Blob>());
  const adding = useRef(false);
  const formRef = useRef<HTMLFormElement>(null);
  const keyboardInset = useShellKeyboardInset();

  const load = useCallback(() => {
    setLoaded({ status: "loading" });
    Promise.all([
      core.call("getDeckList", null),
      core.call("getNoteTypes", null),
      core.call("getTags", null),
    ])
      .then(([decks, noteTypes, tags]) => {
        setLoaded({
          status: "ready",
          decks: decks.decks,
          noteTypes: noteTypes.noteTypes,
          knownTags: tags.tags,
        });
      })
      .catch((error: unknown) =>
        setLoaded({
          status: "failed",
          error: asCoreError(error, "Something went wrong while opening the Add screen."),
        }),
      );
  }, [core]);
  useEffect(load, [load]);

  const ready = loaded.status === "ready" ? loaded : null;
  // A remembered deck or note type that is gone falls back to the Default deck and Basic.
  const deckId =
    ready &&
    (
      ready.decks.find((d) => d.id === chosenDeck) ??
      ready.decks.find((d) => d.name === "Default" && d.depth === 0) ??
      ready.decks[0]
    )?.id;
  const noteType =
    ready &&
    (ready.noteTypes.find((t) => t.id === chosenType) ??
      ready.noteTypes.find((t) => t.name === "Basic") ??
      ready.noteTypes[0]);
  const noteTypeId = noteType?.id ?? null;
  const isCloze = noteType?.kind === "cloze";
  const fieldHtml = (name: string) => values[name] ?? "";

  // The cloze shortcuts read the fields when they are used, so they count what was typed last.
  const latestValues = useRef({ noteType, values });
  latestValues.current = { noteType, values };
  const highest = useCallback(() => {
    const { noteType, values } = latestValues.current;
    return highestCloze((noteType?.fields ?? []).map((f) => values[f.name] ?? ""));
  }, []);

  // ---- The draft and remembered choices ----

  const draftNow = useRef<Draft | null>(null);
  draftNow.current =
    ready && deckId && noteTypeId ? { deckId, noteTypeId, fields: values, tags } : null;

  useEffect(() => {
    if (!deckId || !noteTypeId || !ready) return;
    write(LAST_KEY, { deckId, noteTypeId });
  }, [deckId, noteTypeId, ready]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `values` and `tags` are what changes; the draft itself is read from the ref.
  useEffect(() => {
    const timer = window.setTimeout(() => {
      if (draftNow.current) write(DRAFT_KEY, draftNow.current);
    }, DRAFT_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [values, tags, deckId, noteTypeId]);

  // Leaving the screen or the app being closed must not lose the last second of typing.
  useEffect(() => {
    const flush = () => {
      if (draftNow.current) write(DRAFT_KEY, draftNow.current);
    };
    window.addEventListener("pagehide", flush);
    document.addEventListener("visibilitychange", flush);
    return () => {
      flush();
      window.removeEventListener("pagehide", flush);
      document.removeEventListener("visibilitychange", flush);
    };
  }, []);

  // ---- Keeping the caret in view above the keyboard and the toolbar ----

  // biome-ignore lint/correctness/useExhaustiveDependencies: only a change of the keyboard inset should scroll.
  useEffect(() => {
    if (active?.hasFocus()) active.dispatch(active.state.tr.scrollIntoView());
  }, [keyboardInset]);

  const onEditorFocus = useCallback((view: EditorView) => {
    setActive(view);
    // After the keyboard has had a moment to come up; the inset effect covers the rest.
    window.requestAnimationFrame(() => {
      if (view.hasFocus()) view.dispatch(view.state.tr.scrollIntoView());
    });
  }, []);

  // ---- The duplicate warning ----

  const firstField = noteType?.fields[0];
  const firstHtml = firstField ? fieldHtml(firstField.name) : "";
  useEffect(() => {
    if (!noteTypeId || !firstHtml.trim()) {
      setDuplicate(false);
      return;
    }
    let current = true;
    const timer = window.setTimeout(() => {
      core
        .call("findDuplicates", { noteTypeId, value: firstHtml })
        .then((found) => current && setDuplicate(found.noteIds.length > 0))
        .catch(() => current && setDuplicate(false));
    }, DUPLICATE_DELAY_MS);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [core, noteTypeId, firstHtml]);

  // ---- Pictures and sounds ----

  const loadMedia = useCallback(
    async (name: string) => {
      const cached = mediaCache.current.get(name);
      if (cached) return cached;
      const { output, bytes } = await core.call("getMedia", { name });
      // A copy, because the reply may be a view on a larger buffer.
      const blob = new Blob([new Uint8Array(bytes)], { type: output.contentType });
      mediaCache.current.set(name, blob);
      return blob;
    },
    [core],
  );

  // Media is stored as soon as it is picked (ADR 0011 decision 3), so the field gets the real name.
  async function attach(kind: "image" | "sound", files: File[]) {
    // The editor last used, unless a change of note type has removed it.
    const view = active?.dom.isConnected ? active : [...views.current.values()][0];
    if (!view || adding.current) return;
    setMediaBusy(true);
    setError(null);
    setStatus(files.length === 1 ? "Adding the file..." : `Adding ${files.length} files...`);
    const problems: string[] = [];
    for (const file of files) {
      try {
        const prepared = kind === "image" ? await prepareImage(file) : await prepareSound(file);
        const added = await core.call(
          "addMedia",
          { name: prepared.name },
          { bytes: prepared.bytes },
        );
        mediaCache.current.set(
          added.name,
          new Blob([new Uint8Array(prepared.bytes)], { type: prepared.type }),
        );
        if (view.dom.isConnected) insertMedia(kind, added.name)(view.state, view.dispatch);
        if (prepared.warning) problems.push(prepared.warning);
      } catch (failure) {
        problems.push(
          failure instanceof MediaRefused || failure instanceof CoreError
            ? failure.message
            : "The file could not be added. Try again.",
        );
      }
    }
    setStatus("");
    setError(problems.length > 0 ? problems.join(" ") : null);
    setMediaBusy(false);
    if (view.dom.isConnected) view.focus();
  }

  // ---- Adding ----

  // Ctrl+Enter anywhere in the form adds the note.
  const loadedStatus = loaded.status;
  // biome-ignore lint/correctness/useExhaustiveDependencies: the form only exists once loaded, so the listener is attached again when the status changes.
  useEffect(() => {
    const form = formRef.current;
    if (!form) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
        event.preventDefault();
        void addRef.current();
      }
    };
    form.addEventListener("keydown", onKeyDown);
    return () => form.removeEventListener("keydown", onKeyDown);
  }, [loadedStatus]);

  async function add() {
    if (adding.current || mediaBusy || !noteType || !deckId) return;
    adding.current = true;
    setPending(true);
    setError(null);
    setStatus("");
    const finalTags = withTyped(tags, typedTag);
    try {
      const out = await core.call("addNote", {
        deckId,
        noteTypeId: noteType.id,
        fields: noteType.fields.map((f) => ({ fieldId: f.id, value: fieldHtml(f.name) })),
        tags: finalTags,
      });
      const first = noteType.fields[0]?.name.toLowerCase() ?? "first field";
      setStatus(
        `Added (${cardsText(out.cardCount)}).${
          out.duplicates.length > 0 ? ` Another note with this ${first} already exists.` : ""
        }`,
      );
      setValues({});
      mediaCache.current.clear();
      setTags(finalTags);
      setTypedTag("");
      setDuplicate(false);
      core
        .call("getTags", null)
        .then((t) => setLoaded((l) => (l.status === "ready" ? { ...l, knownTags: t.tags } : l)))
        .catch(() => {});
      const firstView =
        noteType.fields[0] && views.current.get(fieldKey(noteType, noteType.fields[0].id));
      firstView?.focus();
    } catch (failure) {
      setError(asCoreError(failure, "The note could not be added. Try again.").message);
    } finally {
      adding.current = false;
      setPending(false);
    }
  }
  const addRef = useRef(add);
  addRef.current = add;

  function clear() {
    setValues({});
    setTags([]);
    setTypedTag("");
    setError(null);
    setStatus("");
  }

  if (loaded.status === "loading") {
    return (
      <>
        <PageHeading>Add</PageHeading>
        <p role="status">Opening...</p>
      </>
    );
  }
  if (loaded.status === "failed") {
    return (
      <>
        <PageHeading>Add</PageHeading>
        <div className="problem" role="alert">
          <WarningIcon />
          <p>{loaded.error.message}</p>
          <button type="button" className="button" onClick={load}>
            Try again
          </button>
        </div>
      </>
    );
  }

  return (
    // Ctrl+Enter anywhere in the form adds the note. Plain Enter in a text box must not submit it.
    <form
      className="add"
      aria-label="Add a note"
      onSubmit={(event) => event.preventDefault()}
      ref={formRef}
    >
      <PageHeading>Add</PageHeading>
      <div className="add-choices">
        <div className="add-choice">
          <label htmlFor="add-deck" className="add-label">
            Deck
          </label>
          <select
            id="add-deck"
            value={deckId ?? ""}
            onChange={(e) => setChosenDeck(e.target.value)}
          >
            {loaded.decks.map((deck) => (
              <option key={deck.id} value={deck.id}>
                {deck.path}
              </option>
            ))}
          </select>
        </div>
        <div className="add-choice">
          <label htmlFor="add-type" className="add-label">
            Type
          </label>
          <select
            id="add-type"
            value={noteTypeId ?? ""}
            onChange={(e) => setChosenType(e.target.value)}
          >
            {loaded.noteTypes.map((type) => (
              <option key={type.id} value={type.id}>
                {type.name}
              </option>
            ))}
          </select>
        </div>
      </div>

      {noteType?.fields.map((field, index) => {
        const labelId = `add-field-${noteType.id}-${field.id}`;
        return (
          <div className="add-field" key={fieldKey(noteType, field.id)}>
            <span id={labelId} className="add-label">
              {field.name}
            </span>
            <FieldEditor
              value={fieldHtml(field.name)}
              onChange={(html) => {
                setValues((v) => ({ ...v, [field.name]: html }));
                setStatus("");
              }}
              labelId={labelId}
              cloze={isCloze}
              highestCloze={highest}
              loadMedia={loadMedia}
              onFocus={onEditorFocus}
              onTransaction={(view) => {
                if (view === active) setTick((n) => n + 1);
              }}
              onReady={(view) => {
                const key = fieldKey(noteType, field.id);
                if (view) views.current.set(key, view);
                else views.current.delete(key);
              }}
            />
            {index === 0 && duplicate && (
              <p className="add-warning">
                <WarningIcon />
                <span>A note with this {field.name.toLowerCase()} already exists.</span>
              </p>
            )}
          </div>
        );
      })}

      <TagInput
        tags={tags}
        typed={typedTag}
        known={ready?.knownTags ?? []}
        onTags={setTags}
        onTyped={setTypedTag}
      />

      <p role="status" className="add-status">
        {status}
      </p>
      {error && (
        <p role="alert" className="add-error">
          <WarningIcon />
          <span>{error}</span>
        </p>
      )}
      <button type="button" className="button add-clear" onClick={clear}>
        Clear
      </button>

      <BottomAction>
        <div className="add-bar">
          <Toolbar
            view={active}
            cloze={isCloze}
            highestCloze={highest}
            onFiles={(kind, files) => void attach(kind, files)}
            busy={mediaBusy}
          />
          <button
            type="button"
            className="button button-primary add-submit"
            disabled={pending || mediaBusy || !noteType}
            title="Add (Ctrl+Enter)"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => void add()}
          >
            Add
          </button>
        </div>
      </BottomAction>
    </form>
  );
}
