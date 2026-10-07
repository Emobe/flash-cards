import { CoreError } from "core-client";
import { useEffect, useState } from "react";
import { useCore } from "./core";

export type CollectionState =
  | { status: "opening" }
  | { status: "ready" }
  | { status: "problem"; error: CoreError };

/** Asks the core once at start whether the collection is open (ADR 0010 decision 6). */
export function useCollectionState(): CollectionState {
  const core = useCore();
  const [state, setState] = useState<CollectionState>({ status: "opening" });

  useEffect(() => {
    let current = true;
    core
      .call("getCollectionInfo", null)
      .then(() => current && setState({ status: "ready" }))
      .catch((error: unknown) => {
        if (!current) return;
        const known =
          error instanceof CoreError
            ? error
            : new CoreError("internal", "Something went wrong while opening the collection.");
        setState({ status: "problem", error: known });
      });
    return () => {
      current = false;
    };
  }, [core]);

  return state;
}
