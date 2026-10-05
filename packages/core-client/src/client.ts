import type { ErrorKind } from "./generated/ErrorKind";
import type { Methods } from "./generated/Methods";
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

/** Shared by every platform: types the calls from `Methods` and normalises errors. */
export class CoreClient {
  readonly #transport: Transport;

  constructor(transport: Transport) {
    this.#transport = transport;
  }

  async call<M extends keyof Methods>(
    method: M,
    input: Methods[M]["input"],
  ): Promise<Methods[M]["output"]> {
    try {
      const { output } = await this.#transport.call({ method, input });
      return output as Methods[M]["output"];
    } catch (error) {
      throw toCoreError(error);
    }
  }
}

function toCoreError(error: unknown): CoreError {
  if (error instanceof CoreError) return error;
  if (isApiError(error)) return new CoreError(error.kind, error.message);
  return new CoreError(
    "internal",
    "Something went wrong inside the app. Restart it and try again.",
  );
}
