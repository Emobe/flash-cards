import { defineProject } from "vitest/config";

export default defineProject({
  test: {
    name: "native",
    environment: "happy-dom",
  },
});
