import { isApiError, type Notice, type Transport } from "core-client";
import type { FromWorker, WorkerLike } from "./workerProtocol";

const CANCELLED = { kind: "cancelled", message: "The operation was cancelled." } as const;
const TRAPPED = {
  kind: "internal",
  message: "Something went wrong. Reload the page to continue.",
} as const;
const OPEN_ELSEWHERE = {
  kind: "internal",
  message: "The collection is open in another tab. Close the other tab, then reload this one.",
} as const;

type Pending = {
  id: number;
  method: string;
  input: unknown;
  bytes?: Uint8Array;
  op?: number;
  resolve: (reply: { output: unknown; bytes?: Uint8Array }) => void;
  reject: (error: unknown) => void;
};

/**
 * Talks to the core in a dedicated worker (ADR 0003). The queue lives here, so the transport always
 * knows which operation is running: one request is in flight at a time, cancelling a queued call
 * removes it, and cancelling the running call terminates the worker and starts a new one.
 */
export function createWebTransport(createWorker: () => WorkerLike): Transport {
  const listeners = new Set<(notice: Notice) => void>();
  const queue: Pending[] = [];
  let running: Pending | undefined;
  let worker: WorkerLike;
  let state: "starting" | "ready" | "failed" = "starting";
  let nextId = 1;

  function start() {
    const current = createWorker();
    worker = current;
    state = "starting";
    current.onmessage = ({ data }) => {
      if (current === worker) handle(data);
    };
    current.onerror = (event) => {
      console.error("The core worker failed", event);
      if (current === worker) handle({ type: "openFailed" });
    };
  }

  /** Replaces the worker. The old instance is never used again. */
  function restart() {
    worker.onmessage = null;
    worker.onerror = null;
    worker.terminate();
    running = undefined;
    start();
  }

  function pump() {
    if (state === "failed") {
      for (const item of queue.splice(0)) item.reject(OPEN_ELSEWHERE);
      return;
    }
    const next = queue[0];
    if (state !== "ready" || running || !next) return;
    queue.shift();
    running = next;
    // Transfer the attachment instead of copying it. A view onto a larger buffer is copied first,
    // since transferring would detach the whole buffer.
    let bytes = next.bytes;
    if (bytes && (bytes.byteOffset !== 0 || bytes.byteLength !== bytes.buffer.byteLength)) {
      bytes = bytes.slice();
    }
    worker.postMessage(
      {
        type: "call",
        id: next.id,
        method: next.method,
        input: JSON.stringify(next.input ?? null),
        bytes,
        op: next.op,
      },
      bytes ? [bytes.buffer] : [],
    );
  }

  function handle(message: FromWorker) {
    switch (message.type) {
      case "ready":
        state = "ready";
        pump();
        return;
      case "openFailed":
        state = "failed";
        running?.reject(OPEN_ELSEWHERE);
        running = undefined;
        pump();
        return;
      case "notice":
        for (const listener of listeners) listener(JSON.parse(message.notice) as Notice);
        return;
      case "reply": {
        const item = running;
        if (item?.id !== message.id) return;
        running = undefined;
        item.resolve({
          output: JSON.parse(message.output),
          bytes: message.bytes.length > 0 ? message.bytes : undefined,
        });
        pump();
        return;
      }
      case "error": {
        const item = running;
        if (item?.id !== message.id) return;
        running = undefined;
        item.reject(parseError(message.error));
        pump();
        return;
      }
      case "trap": {
        const item = running;
        if (item?.id !== message.id) return;
        item.reject(TRAPPED);
        restart();
        return;
      }
    }
  }

  start();

  return {
    call({ method, input, bytes, op }) {
      return new Promise((resolve, reject) => {
        if (state === "failed") {
          reject(OPEN_ELSEWHERE);
          return;
        }
        queue.push({
          id: nextId++,
          method,
          input,
          bytes: bytes && bytes.length > 0 ? bytes : undefined,
          op,
          resolve,
          reject,
        });
        pump();
      });
    },
    cancel(op) {
      const queued = queue.findIndex((item) => item.op === op);
      if (queued >= 0) {
        queue.splice(queued, 1)[0]?.reject(CANCELLED);
      } else if (running?.op === op) {
        running.reject(CANCELLED);
        restart();
      }
    },
    subscribe(onNotice) {
      listeners.add(onNotice);
      return () => {
        listeners.delete(onNotice);
      };
    },
  };
}

function parseError(json: string): unknown {
  try {
    const error: unknown = JSON.parse(json);
    if (isApiError(error)) return error;
  } catch {
    // Fall through to the generic error.
  }
  return TRAPPED;
}
