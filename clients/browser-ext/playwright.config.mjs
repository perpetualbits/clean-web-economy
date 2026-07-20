// Playwright configuration for the extension end-to-end test.
//
// The test loads the built `dist/` extension into Chromium, so it must be run
// after `npm run build`. A single worker keeps the shared browser context and
// static server predictable.
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./test/e2e",
  // MV3 service-worker startup plus extension loading needs a little headroom.
  timeout: 30000,
  workers: 1,
  reporter: "list",
});
