import { EmptyState } from "../components/EmptyState";
import { PageHeading } from "../router";

/** Add and Browse say what is coming. 2.4 and Phase 3 replace them. */

export function AddScreen() {
  return (
    <>
      <PageHeading>Add</PageHeading>
      <EmptyState>Adding cards will be here soon.</EmptyState>
    </>
  );
}

export function BrowseScreen() {
  return (
    <>
      <PageHeading>Browse</PageHeading>
      <EmptyState>Search and edit all your cards here, later on.</EmptyState>
    </>
  );
}
