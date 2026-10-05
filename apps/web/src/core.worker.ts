/// Runs the Rust core (wasm) off the main thread and answers one request at a time. The transport
/// decides what runs when: it keeps the queue, and restarts this worker to cancel (ADR 0003).
import loadWasm, { call, init, open } from "./wasm/fc_wasm.js";
import type { FromWorker, ToWorker } from "./workerProtocol";

interface WorkerScope {
  postMessage(message: FromWorker, transfer?: Transferable[]): void;
  onmessage: ((event: { data: ToWorker }) => void) | null;
}
const scope = self as unknown as WorkerScope;

/// How long and how often to retry opening: a terminated worker's handles take about 2 s to free.
const OPEN_RETRY_MS = 50;
const OPEN_GIVE_UP_MS = 5000;
/// Same rate as `fc-native`'s notice hub.
const PROGRESS_INTERVAL_MS = 250;

const lastProgress = new Map<number, number>();

function onNotice(json: string) {
  const notice = JSON.parse(json) as {
    type: string;
    op?: number;
    progress?: { done: number; total: number | null };
  };
  if (notice.type === "progress" && notice.op !== undefined && notice.progress) {
    const isFinal = notice.progress.total === notice.progress.done;
    const now = Date.now();
    const last = lastProgress.get(notice.op);
    if (!isFinal && last !== undefined && now - last < PROGRESS_INTERVAL_MS) return;
    lastProgress.set(notice.op, now);
  }
  scope.postMessage({ type: "notice", notice: json });
}

/** Errors that waiting cannot fix: the collection is newer, or the file is not a collection. */
function isFinal(error: unknown): error is string {
  if (typeof error !== "string") return false;
  try {
    const kind = (JSON.parse(error) as { kind?: string }).kind;
    return kind === "updateRequired" || kind === "invalidInput";
  } catch {
    return false;
  }
}

/**
 * An ID for this browser profile that lives outside the collection (ADR 0006, section 1), so a
 * collection file moved into another browser is recognised as a copy. IndexedDB, because workers
 * have no `localStorage`. If IndexedDB is unavailable the ID is new on every load, which only
 * costs a new device ID, never a shared one.
 */
async function installationId(): Promise<string> {
  try {
    const db = await new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open("fc-installation", 1);
      request.onupgradeneeded = () => request.result.createObjectStore("kv");
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    // Read and write in one transaction so two tabs starting at once agree.
    return await new Promise<string>((resolve, reject) => {
      const tx = db.transaction("kv", "readwrite");
      const store = tx.objectStore("kv");
      const get = store.get("installationId");
      get.onsuccess = () => {
        if (typeof get.result === "string") return resolve(get.result);
        const fresh = crypto.randomUUID();
        store.put(fresh, "installationId");
        tx.oncomplete = () => resolve(fresh);
      };
      tx.onerror = () => reject(tx.error);
      tx.onabort = () => reject(tx.error);
    });
  } catch (error) {
    console.warn("No stored installation ID, using a temporary one", error);
    return crypto.randomUUID();
  }
}

/** Resolves to `undefined` when open, else to the final `ApiError` (JSON) or `null` if just busy. */
async function openWithRetry(installation: string): Promise<string | null | undefined> {
  const deadline = Date.now() + OPEN_GIVE_UP_MS;
  for (;;) {
    try {
      await open("collection.db", installation);
      return undefined;
    } catch (error) {
      if (isFinal(error)) return error;
      console.warn("Could not open the collection yet", error);
      if (Date.now() >= deadline) return null;
      await new Promise((resolve) => setTimeout(resolve, OPEN_RETRY_MS));
    }
  }
}

const opened = (async () => {
  await loadWasm();
  init(onNotice);
  return openWithRetry(await installationId());
})();

scope.onmessage = async ({ data: request }) => {
  if ((await opened) !== undefined) return;
  try {
    const reply = call(request.method, request.input, request.bytes, request.op);
    const bytes = reply.bytes as Uint8Array;
    scope.postMessage(
      { type: "reply", id: request.id, output: reply.output as string, bytes },
      bytes.length > 0 ? [bytes.buffer] : [],
    );
  } catch (error) {
    // The core throws `ApiError` as a JSON string. Anything else is a trap.
    if (typeof error === "string") {
      scope.postMessage({ type: "error", id: request.id, error });
    } else {
      console.error("The core trapped", error);
      scope.postMessage({ type: "trap", id: request.id });
    }
  } finally {
    if (request.op !== undefined) lastProgress.delete(request.op);
  }
};

opened.then(
  (failure) =>
    scope.postMessage(
      failure === undefined
        ? { type: "ready" }
        : { type: "openFailed", ...(failure === null ? {} : { error: failure }) },
    ),
  (error: unknown) => {
    console.error("The core failed to load", error);
    scope.postMessage({ type: "openFailed" });
  },
);
