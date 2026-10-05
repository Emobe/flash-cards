import { CoreError } from "core-client";
import { type FormEvent, useCallback, useEffect, useRef, useState } from "react";
import { useCore } from "ui";

/** Temporary spike UI (step 0.4): notes stored through the core, plus the cancel and crash paths. */
export function SpikePanel() {
  const core = useCore();
  const [notes, setNotes] = useState<string[]>([]);
  const [text, setText] = useState("");
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const [storage, setStorage] = useState("Checking storage...");
  const [progress, setProgress] = useState<string | null>(null);
  const slow = useRef<AbortController | null>(null);

  const fail = useCallback((e: unknown) => {
    setError(e instanceof CoreError ? `${e.kind}: ${e.message}` : String(e));
  }, []);

  useEffect(() => {
    core
      .call("spikeListNotes", null)
      .then((out) => setNotes(out.notes))
      .catch(fail);
  }, [core, fail]);

  useEffect(() => {
    (async () => {
      const persisted = (await navigator.storage?.persist?.()) ?? false;
      const estimate = await navigator.storage?.estimate?.();
      setStorage(
        `persisted: ${persisted}, used: ${estimate?.usage ?? "unknown"} bytes, quota: ${estimate?.quota ?? "unknown"} bytes`,
      );
    })().catch(() => setStorage("Storage information is not available."));
  }, []);

  async function add(event: FormEvent) {
    event.preventDefault();
    setError("");
    try {
      const out = await core.call("spikeAddNote", { text });
      setNotes(out.notes);
      setText("");
    } catch (e) {
      fail(e);
    }
  }

  async function runSlow() {
    setError("");
    const controller = new AbortController();
    slow.current = controller;
    const started = performance.now();
    try {
      const out = await core.call(
        "debugSlow",
        { steps: 10, stepMs: 500 },
        { signal: controller.signal, onProgress: (p) => setProgress(`${p.done} of ${p.total}`) },
      );
      setStatus(`Slow call completed ${out.completed} steps.`);
    } catch (e) {
      fail(e);
    } finally {
      slow.current = null;
      setProgress(null);
      setStatus((s) => `${s} (${Math.round(performance.now() - started)} ms)`);
    }
  }

  async function crash() {
    setError("");
    try {
      await core.call("debugPanic", null);
    } catch (e) {
      fail(e);
    }
  }

  async function ping() {
    setError("");
    const started = performance.now();
    try {
      const info = await core.call("getCoreInfo", null);
      setStatus(
        `Core ${info.coreVersion} answered in ${Math.round(performance.now() - started)} ms.`,
      );
    } catch (e) {
      fail(e);
    }
  }

  return (
    <section className="spike">
      <h2>Storage spike</h2>
      <form onSubmit={add} className="divide">
        <label>
          Note
          <input value={text} onChange={(e) => setText(e.target.value)} />
        </label>
        <button type="submit">Add note</button>
      </form>
      <ul aria-label="Notes">
        {notes.map((note, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: notes are append-only and have no id
          <li key={i}>{note}</li>
        ))}
      </ul>
      <div className="row">
        <button type="button" onClick={runSlow} disabled={progress !== null}>
          Run slow call
        </button>
        <button type="button" onClick={() => slow.current?.abort()} disabled={progress === null}>
          Cancel
        </button>
        <button type="button" onClick={crash}>
          Crash the core
        </button>
        <button type="button" onClick={ping}>
          Ping the core
        </button>
      </div>
      {progress && <p role="status">Progress: {progress}</p>}
      {status && <p role="status">{status}</p>}
      {error && (
        <p role="alert" className="error">
          {error}
        </p>
      )}
      <p>{storage}</p>
    </section>
  );
}
