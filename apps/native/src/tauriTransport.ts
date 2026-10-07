import { Channel, invoke } from "@tauri-apps/api/core";
import { isApiError, type Notice, type Transport } from "core-client";

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

/**
 * Base64 of `bytes`, the fast way to send bytes to Rust on every platform (ADR 0002, findings 7
 * and 9). Uses `Uint8Array.prototype.toBase64` where the WebView has it.
 */
export function encodeBase64(bytes: Uint8Array): string {
  const native = (bytes as Uint8Array & { toBase64?: () => string }).toBase64;
  if (typeof native === "function") return native.call(bytes);
  const chunk = 0x8000;
  let binary = "";
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

/** The core transport plus the one platform call that needs the session token. */
export type TauriTransport = Transport & {
  /**
   * Makes the system bars (Android) and the title bar (desktop) match the page's theme, through the
   * token-checked `set_system_theme` command (ADR 0010). Failure is logged, never thrown: the
   * page still works with the wrong bar colour.
   */
  setSystemTheme(theme: "light" | "dark", followSystem: boolean): void;
};

/**
 * Talks to the Rust core through Tauri's `call`, `subscribe` and `cancel` commands. Each needs the
 * session token that `handshake` returns once per page load, so the transport claims it first
 * (ADR 0005). The token lives only in this closure.
 */
export function createTauriTransport(): TauriTransport {
  const handshake = invoke<string>("handshake").catch((error: unknown) => {
    throw isApiError(error) ? error : new Error(String(error));
  });
  // A failed handshake is reported to whoever calls next, not as an unhandled rejection.
  handshake.catch(() => {});
  const listeners = new Set<(notice: Notice) => void>();
  // The Rust side keeps one channel per webview, so register one for the whole transport. `call`
  // waits for it, so a progress notice cannot be sent before the channel is registered.
  let registration: Promise<void> | undefined;

  function ensureRegistered(): Promise<void> {
    registration ??= (async () => {
      const token = await handshake;
      const channel = new Channel<Notice>((notice) => {
        for (const listener of listeners) listener(notice);
      });
      await invoke("subscribe", { token, onNotice: channel });
    })().catch((error: unknown) => {
      registration = undefined;
      console.error("Could not subscribe to core notices", error);
    });
    return registration;
  }

  return {
    async call({ method, input, bytes, op }) {
      const token = await handshake;
      if (listeners.size > 0) await ensureRegistered();
      const attachment = bytes && bytes.length > 0 ? encodeBase64(bytes) : undefined;
      let reply: unknown;
      try {
        reply = await invoke<ArrayBuffer | number[]>("call", {
          token,
          method,
          input,
          attachment,
          op,
        });
      } catch (error) {
        // Core errors arrive as `{ kind, message }` and pass through. Anything else (for example a
        // command the webview may not call) is turned into a generic error by `CoreClient`.
        throw isApiError(error) ? error : new Error(String(error));
      }
      const frame = decodeFrame(reply as ArrayBuffer | number[]);
      return { output: frame.output, bytes: frame.bytes.length > 0 ? frame.bytes : undefined };
    },
    cancel(op) {
      handshake.then((token) => invoke("cancel", { token, op })).catch(() => {});
    },
    setSystemTheme(theme, followSystem) {
      handshake
        .then((token) =>
          invoke("set_system_theme", { token, dark: theme === "dark", followSystem }),
        )
        .catch((error: unknown) => console.error("Could not set the system theme", error));
    },
    subscribe(onNotice) {
      listeners.add(onNotice);
      void ensureRegistered();
      return () => {
        listeners.delete(onNotice);
      };
    },
  };
}
