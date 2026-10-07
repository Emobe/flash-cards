import { EditorState } from "prosemirror-state";
import { EditorView } from "prosemirror-view";
import { useEffect, useRef } from "react";
import { editorPlugins } from "./commands";
import { loadField, saveField } from "./html";

type Props = {
  /** The field's HTML. A value other than the last one this editor sent replaces the content. */
  value: string;
  onChange: (html: string) => void;
  /** The id of the element that labels this field. */
  labelId: string;
  /** Only a Cloze note type has the cloze shortcuts. */
  cloze: boolean;
  /** The highest cloze number across every field of the note, read when a shortcut is used. */
  highestCloze: () => number;
  /** The editor took focus: the toolbar acts on this one. */
  onFocus: (view: EditorView) => void;
  /** Any transaction, so the toolbar can show what is on under the cursor. */
  onTransaction?: (view: EditorView) => void;
  /** The view once it exists, and null when it is gone. */
  onReady?: (view: EditorView | null) => void;
};

/**
 * One ProseMirror editor for one field (ADR 0011 decision 1). The field's HTML goes in through
 * `loadField` and out through `saveField`, so only what the schema allows is ever shown or stored.
 * Mount one per field and give it a `key` that changes with the note type: the plugins, and so the
 * cloze shortcuts, are set when it mounts.
 */
export function FieldEditor({
  value,
  onChange,
  labelId,
  cloze,
  highestCloze,
  onFocus,
  onTransaction,
  onReady,
}: Props) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const latest = useRef({ onChange, onFocus, onTransaction, onReady, highestCloze });
  latest.current = { onChange, onFocus, onTransaction, onReady, highestCloze };
  const sent = useRef(value);

  // biome-ignore lint/correctness/useExhaustiveDependencies: the editor is made once; later props are read through `latest`, and `value` through the effect below.
  useEffect(() => {
    const element = host.current;
    if (!element) return;
    const editor: EditorView = new EditorView(element, {
      state: EditorState.create({
        doc: loadField(value),
        plugins: editorPlugins({ cloze, highestCloze: () => latest.current.highestCloze() }),
      }),
      attributes: {
        role: "textbox",
        "aria-multiline": "true",
        "aria-labelledby": labelId,
        class: "field-editor-content",
      },
      scrollMargin: 16,
      scrollThreshold: 16,
      dispatchTransaction(transaction) {
        const next = editor.state.apply(transaction);
        editor.updateState(next);
        if (transaction.docChanged) {
          const html = saveField(next.doc);
          sent.current = html;
          latest.current.onChange(html);
        }
        latest.current.onTransaction?.(editor);
      },
      handleDOMEvents: {
        focus: () => {
          latest.current.onFocus(editor);
          return false;
        },
      },
    });
    view.current = editor;
    latest.current.onReady?.(editor);
    return () => {
      latest.current.onReady?.(null);
      editor.destroy();
      view.current = null;
    };
  }, []);

  useEffect(() => {
    const editor = view.current;
    if (!editor || value === sent.current) return;
    sent.current = value;
    // A new state, so undo does not bring back what a clear or a draft restore replaced.
    editor.updateState(
      EditorState.create({ doc: loadField(value), plugins: editor.state.plugins }),
    );
    latest.current.onTransaction?.(editor);
  }, [value]);

  return <div className="field-editor" ref={host} />;
}
