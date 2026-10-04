import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { App } from "./App";

afterEach(cleanup);

test("renders the placeholder screen", () => {
  render(<App />);
  expect(screen.getByRole("heading", { name: "Flash cards" })).toBeDefined();
});
