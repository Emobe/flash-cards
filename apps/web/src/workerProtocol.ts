/** Messages between the web transport (main thread) and the core worker. */

export type ToWorker = {
  type: "call";
  id: number;
  method: string;
  /** JSON text. */
  input: string;
  bytes?: Uint8Array;
  op?: number;
};

export type FromWorker =
  /** The collection is open. */
  | { type: "ready" }
  /**
   * The collection could not be opened. `error` is the `ApiError` as JSON text when the core
   * said why (a newer collection, a file that is not one). Without it, another tab holds it.
   */
  | { type: "openFailed"; error?: string }
  | { type: "reply"; id: number; output: string; bytes: Uint8Array }
  /** `error` is the `ApiError` as JSON text. */
  | { type: "error"; id: number; error: string }
  /** The wasm instance trapped (a panic or `unreachable`). It must not be used again. */
  | { type: "trap"; id: number }
  /** A core notice as JSON text. */
  | { type: "notice"; notice: string };

/** The part of a `Worker` the transport uses, so tests can supply a fake. */
export interface WorkerLike {
  postMessage(message: ToWorker, transfer?: Transferable[]): void;
  terminate(): void;
  onmessage: ((event: { data: FromWorker }) => void) | null;
  onerror: ((event: unknown) => void) | null;
}
