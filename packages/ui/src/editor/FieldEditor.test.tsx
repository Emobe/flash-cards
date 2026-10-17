import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { FieldEditor } from "./FieldEditor";

afterEach(cleanup);

test("Escape is left to the page, so it can close the window around the editor", () => {
  render(
    <FieldEditor
      value="kot"
      onChange={() => {}}
      labelId="l"
      cloze={false}
      highestCloze={() => 0}
      loadMedia={async () => new Blob()}
      onFocus={() => {}}
    />,
  );
  const content = document.querySelector(".field-editor-content") as HTMLElement;
  const seen = vi.fn((event: KeyboardEvent) => event.defaultPrevented);
  document.addEventListener("keydown", seen);
  try {
    content.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", keyCode: 27, bubbles: true, cancelable: true }),
    );
    content.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", keyCode: 13, bubbles: true, cancelable: true }),
    );
  } finally {
    document.removeEventListener("keydown", seen);
  }
  // ProseMirror still takes Enter, so the check is real.
  expect(seen.mock.results.map((r) => r.value)).toEqual([false, true]);
});
