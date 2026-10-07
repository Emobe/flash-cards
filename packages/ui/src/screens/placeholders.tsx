import { EmptyState } from "../components/EmptyState";
import { PageHeading } from "../router";

/** Browse says what is coming. Phase 3 replaces it. */

export function BrowseScreen() {
  return (
    <>
      <PageHeading>Browse</PageHeading>
      <EmptyState>Search and edit all your cards here, later on.</EmptyState>
    </>
  );
}
