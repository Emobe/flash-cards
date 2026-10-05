import { CoreError } from "core-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { useCore } from "ui";

/** Temporary dev panel (step 0.4): collection info, plus the cancel and crash paths. */
export function SpikePanel() {
  const core = useCore();
  const [collection, setCollection] = useState("Opening the collection...");
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
      .call("getCollectionInfo", null)
      .then((info) =>
        setCollection(
          `Collection storage version ${info.schemaVersion} (this build supports up to ${info.supportedSchemaVersion}), created by core ${info.createdBy}.`,
        ),
      )
      .catch((e: unknown) => {
        setCollection("The collection is not available.");
        fail(e);
      });
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
      <h2>Core spike</h2>
      <p>{collection}</p>
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
