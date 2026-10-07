import { EmptyState } from "../components/EmptyState";
import { PageHeading } from "../router";

/** Decks, Add and Browse say what is coming. 2.2, 2.4 and Phase 3 replace them. */

export function DecksScreen() {
  return (
    <>
      <PageHeading>Decks</PageHeading>
      <EmptyState>Your decks will appear here.</EmptyState>
    </>
  );
}

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

/** The full-screen study route (2.3 builds the real one). */
export function StudyScreen({ deckId }: { deckId: string }) {
  return (
    <>
      <PageHeading>Study</PageHeading>
      <EmptyState>Studying deck {deckId} will be here soon.</EmptyState>
    </>
  );
}
