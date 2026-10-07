import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test } from "vitest";
import {
  Link,
  optionsPath,
  PageHeading,
  parseRoute,
  RouterProvider,
  studyPath,
  useRouter,
} from "./router";

function Screens() {
  const { route, navigate, back } = useRouter();
  return (
    <div>
      <PageHeading>{route.title}</PageHeading>
      <p data-testid="name">{route.name === "study" ? `study ${route.deckId}` : route.name}</p>
      <button type="button" onClick={() => navigate("/add")}>
        go add
      </button>
      <button type="button" onClick={() => navigate(studyPath("a/b"))}>
        go study
      </button>
      <button type="button" onClick={() => navigate("/nowhere")}>
        go nowhere
      </button>
      <button type="button" onClick={back}>
        back
      </button>
      <Link path="/browse">to browse</Link>
    </div>
  );
}

function renderRouter() {
  return render(
    <RouterProvider>
      <Screens />
    </RouterProvider>,
  );
}

beforeEach(() => {
  window.history.replaceState(null, "", "/#/decks");
});
afterEach(cleanup);

describe("parseRoute", () => {
  test("knows the six routes", () => {
    expect(parseRoute("/decks")?.name).toBe("decks");
    expect(parseRoute("/add")?.name).toBe("add");
    expect(parseRoute("/browse")?.name).toBe("browse");
    expect(parseRoute("/settings")?.name).toBe("settings");
    expect(parseRoute("/settings/developer")?.name).toBe("developer");
    expect(parseRoute("/study/42")).toMatchObject({ name: "study", deckId: "42" });
  });

  test("rejects anything else", () => {
    for (const path of ["", "/", "/nope", "/study", "/decks/1", "/settings/x", "/study/%E0%A4%A"]) {
      expect(parseRoute(path)).toBeNull();
    }
  });

  test("options routes carry the deck id", () => {
    expect(parseRoute("/options/42")).toMatchObject({ name: "options", deckId: "42" });
    expect(parseRoute("/options")).toBeNull();
    expect(parseRoute(optionsPath("a/b c"))).toMatchObject({ deckId: "a/b c" });
  });

  test("study ids round-trip through encoding", () => {
    expect(parseRoute(studyPath("a/b c"))).toMatchObject({ deckId: "a/b c" });
  });
});

describe("RouterProvider", () => {
  test("starts on the route in the hash and sets the title", () => {
    window.history.replaceState(null, "", "/#/settings");
    renderRouter();
    expect(screen.getByTestId("name").textContent).toBe("settings");
    expect(document.title).toBe("Settings · Flash cards");
  });

  test("an unknown hash goes to decks and replaces the entry", () => {
    window.history.replaceState(null, "", "/#/nope");
    const length = window.history.length;
    renderRouter();
    expect(screen.getByTestId("name").textContent).toBe("decks");
    expect(window.location.hash).toBe("#/decks");
    expect(window.history.length).toBe(length);
  });

  test("navigate pushes a history entry and updates the title", () => {
    renderRouter();
    const length = window.history.length;
    fireEvent.click(screen.getByText("go add"));
    expect(screen.getByTestId("name").textContent).toBe("add");
    expect(window.location.hash).toBe("#/add");
    expect(window.history.length).toBe(length + 1);
    expect(document.title).toBe("Add · Flash cards");
  });

  test("navigating to an unknown path or the current one does nothing", () => {
    renderRouter();
    const length = window.history.length;
    fireEvent.click(screen.getByText("go nowhere"));
    fireEvent.click(screen.getByText("to browse"));
    fireEvent.click(screen.getByText("to browse"));
    expect(window.history.length).toBe(length + 1);
  });

  test("back via popstate returns to the previous route", () => {
    renderRouter();
    fireEvent.click(screen.getByText("go add"));
    act(() => {
      window.history.replaceState(null, "", "/#/decks");
      window.dispatchEvent(new PopStateEvent("popstate"));
    });
    expect(screen.getByTestId("name").textContent).toBe("decks");
    expect(document.title).toBe("Decks · Flash cards");
  });

  test("a hashchange from outside updates the route", () => {
    renderRouter();
    act(() => {
      window.history.replaceState(null, "", "/#/browse");
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    });
    expect(screen.getByTestId("name").textContent).toBe("browse");
  });

  test("study routes carry the deck id", () => {
    renderRouter();
    fireEvent.click(screen.getByText("go study"));
    expect(screen.getByTestId("name").textContent).toBe("study a/b");
  });

  test("focus stays put on first load and moves to the h1 after a navigation", () => {
    renderRouter();
    expect(document.activeElement).not.toBe(screen.getByRole("heading", { level: 1 }));
    fireEvent.click(screen.getByText("go add"));
    const heading = screen.getByRole("heading", { level: 1 });
    expect(heading.textContent).toBe("Add");
    expect(document.activeElement).toBe(heading);
  });

  test("a plain click on a Link navigates, a ctrl-click does not", () => {
    renderRouter();
    // A browser opens a new tab here. happy-dom would follow the href, so stop that after React.
    document.addEventListener("click", (e) => e.preventDefault(), { once: true });
    fireEvent.click(screen.getByText("to browse"), { ctrlKey: true });
    expect(screen.getByTestId("name").textContent).toBe("decks");
    fireEvent.click(screen.getByText("to browse"));
    expect(screen.getByTestId("name").textContent).toBe("browse");
  });

  test("useRouter needs a provider", () => {
    expect(() => render(<Screens />)).toThrow();
  });
});
