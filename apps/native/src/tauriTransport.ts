import { invoke } from "@tauri-apps/api/core";
import { isApiError, type Transport } from "core-client";

/**
 * Splits a reply from the `call` command: `u32` little-endian JSON length, the JSON output, then
 * attachment bytes. Accepts an `ArrayBuffer` (the fast path) or a plain number array (Tauri's
 * fallback path, and macOS), see ADR 0002 finding 8.
 */
export function decodeFrame(reply: ArrayBuffer | number[]): { output: unknown; bytes: Uint8Array } {
  const data = reply instanceof ArrayBuffer ? new Uint8Array(reply) : Uint8Array.from(reply);
  const length = new DataView(data.buffer, data.byteOffset, data.byteLength).getUint32(0, true);
  const json = new TextDecoder().decode(data.subarray(4, 4 + length));
  return { output: JSON.parse(json), bytes: data.subarray(4 + length) };
}

/** Talks to the Rust core through Tauri's `call` command. */
export function createTauriTransport(): Transport {
  return {
    async call({ method, input }) {
      let reply: unknown;
      try {
        reply = await invoke<ArrayBuffer | number[]>("call", { method, input });
      } catch (error) {
        // Core errors arrive as `{ kind, message }` and pass through. Anything else (for example a
        // command the webview may not call) is turned into a generic error by `CoreClient`.
        throw isApiError(error) ? error : new Error(String(error));
      }
      const { output } = decodeFrame(reply as ArrayBuffer | number[]);
      return { output };
    },
  };
}
