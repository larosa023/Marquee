import { defineConfig, devices } from '@playwright/test';

// Smoke tests run the real UI in a browser against the fake engine in tests/mock (a build made
// with VITE_MOCK=1). They cover the flows a release must not break; the engine itself is tested in Rust.
export default defineConfig({
  testDir: './tests/e2e',
  timeout: 30_000,
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: 'http://localhost:4173',
    viewport: { width: 1440, height: 900 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'chromium',
      // PW_CHANNEL=chrome uses an installed Chrome instead of Playwright's own download.
      use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 }, channel: process.env.PW_CHANNEL || undefined },
    },
  ],
  webServer: {
    command: 'npm run build:mock && npm run preview:mock',
    url: 'http://localhost:4173',
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
