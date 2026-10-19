import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // A fixed zone, with a daylight saving change in late October, for the date tests.
    env: { TZ: "Europe/Zurich" },
  },
});
