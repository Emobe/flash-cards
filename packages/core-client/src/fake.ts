import type { Methods } from "./generated/Methods";
import type { Transport } from "./transport";

type Handlers = {
  [M in keyof Methods]?: (input: Methods[M]["input"]) => Methods[M]["output"];
};

/**
 * A transport backed by plain functions, for UI tests. A handler may throw an `ApiError`-shaped
 * object (`{ kind, message }`), as the real transports reject.
 */
export function createFakeTransport(handlers: Handlers): Transport {
  return {
    async call({ method, input }) {
      const handler = handlers[method as keyof Methods] as
        | ((input: unknown) => unknown)
        | undefined;
      if (!handler) {
        throw { kind: "unknownMethod", message: `No fake handler for "${method}".` };
      }
      return { output: handler(input) };
    },
  };
}
