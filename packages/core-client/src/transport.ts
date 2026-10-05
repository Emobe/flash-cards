import type { ApiError } from "./generated/ApiError";
import type { Notice } from "./generated/Notice";

/**
 * The only thing a platform implements (Tauri now, a web worker in step 0.4).
 * See `docs/adr/0002-ui-core-bridge.md`.
 */
export interface Transport {
  /**
   * Rejects with an `ApiError`-shaped object when the core returns an error. `op` is the client's
   * operation ID, which `cancel` and progress notices refer to. An empty `bytes` means no attachment.
   */
  call(req: {
    method: string;
    input: unknown;
    bytes?: Uint8Array;
    op?: number;
  }): Promise<{ output: unknown; bytes?: Uint8Array }>;
  /** Asks the core to stop operation `op` at its next checkpoint. May be called before it starts. */
  cancel(op: number): void;
  /** Delivers every notice (events and progress) until the returned function is called. */
  subscribe(onNotice: (notice: Notice) => void): () => void;
}

/** Whether `value` has the shape of an `ApiError`. */
export function isApiError(value: unknown): value is ApiError {
  return (
    typeof value === "object" &&
    value !== null &&
    "kind" in value &&
    typeof value.kind === "string" &&
    "message" in value &&
    typeof value.message === "string"
  );
}
