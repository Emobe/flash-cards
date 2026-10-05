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

async function openWithRetry(): Promise<boolean> {
  const deadline = Date.now() + OPEN_GIVE_UP_MS;
  for (;;) {
    try {
      await open("collection.db");
      return true;
    } catch (error) {
      console.warn("Could not open the collection yet", error);
      if (Date.now() >= deadline) return false;
      await new Promise((resolve) => setTimeout(resolve, OPEN_RETRY_MS));
    }
  }
}

const opened = (async () => {
  await loadWasm();
  init(onNotice);
  return openWithRetry();
})();

scope.onmessage = async ({ data: request }) => {
  if (!(await opened)) return;
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
  (ok) => scope.postMessage({ type: ok ? "ready" : "openFailed" }),
  (error: unknown) => {
    console.error("The core failed to load", error);
    scope.postMessage({ type: "openFailed" });
  },
);
