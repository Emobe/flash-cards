import type { CoreError } from "core-client";
import { PageHeading } from "../router";
import { WarningIcon } from "../shell/icons";

/**
 * Shown instead of the app when the collection could not be opened (ADR 0010 decision 6). The
 * collection is never modified on these paths.
 */
export function CollectionProblem({ error }: { error: CoreError }) {
  return (
    <div className="problem" role="alert">
      <WarningIcon />
      <PageHeading>The collection could not be opened</PageHeading>
      {error.kind === "updateRequired" ? (
        <p>
          This collection was made by a newer version of the app. Update the app to open it. Your
          cards are untouched.
        </p>
      ) : error.kind === "unavailable" ? (
        <>
          <p>{error.message}</p>
          <p>
            Another window or tab may have the collection open. Close it, then restart the app. Your
            cards are untouched.
          </p>
        </>
      ) : (
        <>
          <p>{error.message}</p>
          <p>Restart the app. If this keeps happening, your cards are still safe on this device.</p>
        </>
      )}
    </div>
  );
}
