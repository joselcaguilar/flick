import { defineConfig, devices } from "playwright/test";

const engineUrl = "http://127.0.0.1:7871";
const appUrl = "http://localhost:5173";

export default defineConfig({
  testDir: "./e2e",
  testMatch: "*.spec.ts",
  fullyParallel: false,
  workers: 1,
  timeout: 45_000,
  expect: { timeout: 10_000 },
  use: {
    ...devices["Desktop Chrome"],
    baseURL: appUrl,
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { browserName: "chromium" } }],
  webServer: [
    {
      command:
        'bash -lc \'cd .. && rm -rf .e2e-data/playwright && mkdir -p .e2e-data/playwright && exec env FLICK_FAKE_LANDMARKS="$PWD/tools/fixtures" target/debug/flick-engine --dev --mock-ha --data-dir "$PWD/.e2e-data/playwright"\'',
      url: `${engineUrl}/health`,
      reuseExistingServer: !process.env.CI,
      timeout: 20_000,
    },
    {
      command: `exec env VITE_ENGINE_URL=${engineUrl} VITE_ENGINE_TOKEN=dev-token ./node_modules/.bin/vite --host 127.0.0.1`,
      url: appUrl,
      reuseExistingServer: !process.env.CI,
      timeout: 20_000,
    },
  ],
});
