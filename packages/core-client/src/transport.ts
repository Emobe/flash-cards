import type { ApiError } from "./generated/ApiError";

/**
 * The only thing a platform implements (Tauri now, a web worker in step 0.4). Step 0.3b adds
 * cancellation, notices and attachments.
 */
export interface Transport {
  /** Rejects with an `ApiError`-shaped object when the core returns an error. */
  call(req: { method: string; input: unknown }): Promise<{ output: unknown }>;
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
