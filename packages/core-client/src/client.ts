import type { CoreEvent } from "./generated/CoreEvent";
import type { ErrorKind } from "./generated/ErrorKind";
import { bytesOutMethods, type Methods } from "./generated/Methods";
import type { Progress } from "./generated/Progress";
import { isApiError, type Transport } from "./transport";

/** What every failed call rejects with. `message` is a readable sentence to show the user. */
export class CoreError extends Error {
  readonly kind: ErrorKind;

  constructor(kind: ErrorKind, message: string) {
    super(message);
    this.name = "CoreError";
    this.kind = kind;
  }
}

type BaseOptions = {
  /** Aborting asks the core to stop. The call then rejects with kind `cancelled`. */
  signal?: AbortSignal;
  onProgress?: (progress: Progress) => void;
};

/** Methods that take an attachment require `bytes`. The others do not accept it. */
type CallOptions<M extends keyof Methods> = BaseOptions &
  (Methods[M]["bytesIn"] extends true ? { bytes: Uint8Array } : { bytes?: never });

type CallArgs<M extends keyof Methods> = Methods[M]["bytesIn"] extends true
  ? [options: CallOptions<M>]
  : [options?: CallOptions<M>];

/** Methods that return an attachment resolve to `{ output, bytes }`, the others to the output. */
type CallResult<M extends keyof Methods> = Methods[M]["bytesOut"] extends true
  ? { output: Methods[M]["output"]; bytes: Uint8Array }
  : Methods[M]["output"];

/** Shared by every platform: types the calls from `Methods` and normalises errors. */
export class CoreClient {
  readonly #transport: Transport;
  readonly #progress = new Map<number, (progress: Progress) => void>();
  readonly #eventListeners = new Set<(event: CoreEvent) => void>();
  #nextOp = 1;
  #unsubscribe: (() => void) | undefined;

  constructor(transport: Transport) {
    this.#transport = transport;
  }

  async call<M extends keyof Methods>(
    method: M,
    input: Methods[M]["input"],
    ...args: CallArgs<M>
  ): Promise<CallResult<M>> {
    const options: BaseOptions & { bytes?: Uint8Array } = args[0] ?? {};
    const { signal, onProgress, bytes } = options;
    if (signal?.aborted) throw cancelled();
    this.#ensureSubscribed();

    const op = this.#nextOp++;
    if (onProgress) this.#progress.set(op, onProgress);
    const onAbort = () => this.#transport.cancel(op);
    signal?.addEventListener("abort", onAbort, { once: true });
    try {
      const reply = await this.#transport.call({ method, input, bytes, op });
      if (bytesOutMethods.has(method)) {
        // An empty attachment looks the same as none on the wire.
        const bytes = reply.bytes ?? new Uint8Array(0);
        return { output: reply.output, bytes } as CallResult<M>;
      }
      return reply.output as CallResult<M>;
    } catch (error) {
      throw toCoreError(error);
    } finally {
      signal?.removeEventListener("abort", onAbort);
      this.#progress.delete(op);
    }
  }

  /** Calls `listener` for every event the core emits. Returns a function that stops listening. */
  onEvent(listener: (event: CoreEvent) => void): () => void {
    this.#ensureSubscribed();
    this.#eventListeners.add(listener);
    return () => this.#eventListeners.delete(listener);
  }

  #ensureSubscribed(): void {
    this.#unsubscribe ??= this.#transport.subscribe((notice) => {
      if (notice.type === "progress") {
        this.#progress.get(notice.op)?.(notice.progress);
      } else {
        for (const listener of this.#eventListeners) listener(notice.event);
      }
    });
  }
}

function cancelled(): CoreError {
  return new CoreError("cancelled", "The operation was cancelled.");
}

function toCoreError(error: unknown): CoreError {
  if (error instanceof CoreError) return error;
  if (isApiError(error)) return new CoreError(error.kind, error.message);
  return new CoreError(
    "internal",
    "Something went wrong inside the app. Restart it and try again.",
  );
}
