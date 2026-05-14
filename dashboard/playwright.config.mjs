import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./test",
  timeout: 30_000,
  use: {
    baseURL: "http://127.0.0.1:8765",
  },
});
