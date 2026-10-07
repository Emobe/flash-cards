import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { CoreClient, createFakeTransport } from "core-client";
import { afterEach, expect, test } from "vitest";
import { App } from "./App";
import { CoreProvider } from "./core";
import { PlatformProvider } from "./platform";

afterEach(cleanup);

function renderApp() {
  const client = new CoreClient(
    createFakeTransport({
      getCoreInfo: () => ({ coreVersion: "9.9.9" }),
      getCollectionInfo: () => ({
        schemaVersion: 1,
        supportedSchemaVersion: 1,
        createdBy: "9.9.9",
        deviceId: "01234567-89ab-7cde-8f01-23456789abcd",
        unsupportedFeatures: [],
      }),
      spikeSchedule: ({ ratings }) => ({
        reviews: ratings.map((_, i) => ({ intervalDays: 2 + i, stability: 1, difficulty: 1 })),
      }),
      exampleDivide: ({ dividend, divisor }) => {
        if (divisor === 0) {
          throw {
            kind: "invalidInput",
            message: "Can't divide by zero. Enter a divisor other than 0.",
          };
        }
        return { quotient: dividend / divisor };
      },
    }),
  );
  render(
    <CoreProvider client={client}>
      <PlatformProvider platform={{ cardFrameUrl: "about:blank", setSystemTheme: () => {} }}>
        <App />
      </PlatformProvider>
    </CoreProvider>,
  );
}

test("renders the placeholder screen and the core version", async () => {
  renderApp();
  expect(screen.getByRole("heading", { name: "Flash cards" })).toBeDefined();
  expect(await screen.findByText("Core version 9.9.9")).toBeDefined();
});

test("shows the quotient computed by the core", async () => {
  renderApp();
  fireEvent.click(screen.getByRole("button", { name: "Divide" }));
  expect((await screen.findByRole("status")).textContent).toBe("10 ÷ 4 = 2.5");
});

test("shows a readable message when the core returns an error", async () => {
  renderApp();
  fireEvent.change(screen.getByLabelText("Divisor"), { target: { value: "0" } });
  fireEvent.click(screen.getByRole("button", { name: "Divide" }));
  expect((await screen.findByRole("alert")).textContent).toBe(
    "Can't divide by zero. Enter a divisor other than 0.",
  );
});

test("shows the intervals the core scheduled", async () => {
  renderApp();
  fireEvent.click(screen.getByRole("button", { name: "Schedule 5 Good reviews" }));
  expect((await screen.findByTestId("intervals")).textContent).toBe(
    "Intervals in days: 2, 3, 4, 5, 6",
  );
});

test("shows the collection's storage version", async () => {
  renderApp();
  expect(
    await screen.findByText("Collection storage version 1 (this build supports up to 1)."),
  ).toBeDefined();
});
