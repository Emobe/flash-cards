import type { ApiError } from "./generated/ApiError";
import type { CoreEvent } from "./generated/CoreEvent";
import type { Methods } from "./generated/Methods";
import type { Notice } from "./generated/Notice";
import type { Progress } from "./generated/Progress";
import type { Transport } from "./transport";

/** What a fake handler can do, mirroring Rust's `OpContext`. */
export type FakeContext = {
  /** The attachment sent with the call. */
  bytes: Uint8Array | undefined;
  progress(progress: Progress): void;
  /** Throws a `cancelled` error once the caller has cancelled this call. */
  checkpoint(): void;
  /** Sets the attachment returned with the reply. */
  setBytes(bytes: Uint8Array): void;
};

type Handlers = {
  [M in keyof Methods]?: (
    input: Methods[M]["input"],
    ctx: FakeContext,
  ) => Methods[M]["output"] | Promise<Methods[M]["output"]>;
};

export type FakeTransport = Transport & {
  /** Delivers an event to subscribers, as the core would. */
  emit(event: CoreEvent): void;
  /** Operation IDs passed to `cancel`, in order. */
  readonly cancelled: readonly number[];
};

/**
 * A transport backed by plain functions, for UI tests. A handler may throw an `ApiError`-shaped
 * object (`{ kind, message }`), as the real transports reject.
 */
export function createFakeTransport(handlers: Handlers): FakeTransport {
  const listeners = new Set<(notice: Notice) => void>();
  const cancelled: number[] = [];
  const send = (notice: Notice) => {
    for (const listener of listeners) listener(notice);
  };
  return {
    cancelled,
    emit: (event) => send({ type: "event", event }),
    cancel: (op) => {
      cancelled.push(op);
    },
    subscribe(onNotice) {
      listeners.add(onNotice);
      return () => listeners.delete(onNotice);
    },
    async call({ method, input, bytes, op }) {
      const handler = handlers[method as keyof Methods] as
        | ((input: unknown, ctx: FakeContext) => unknown)
        | undefined;
      if (!handler) {
        throw { kind: "unknownMethod", message: `No fake handler for "${method}".` };
      }
      let reply: Uint8Array | undefined;
      const ctx: FakeContext = {
        bytes,
        progress: (progress) => {
          if (op !== undefined) send({ type: "progress", op, progress });
        },
        checkpoint: () => {
          if (op !== undefined && cancelled.includes(op)) {
            const error: ApiError = {
              kind: "cancelled",
              message: "The operation was cancelled.",
            };
            throw error;
          }
        },
        setBytes: (b) => {
          reply = b;
        },
      };
      return { output: await handler(input, ctx), bytes: reply };
    },
  };
}
