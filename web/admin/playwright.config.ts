// Playwright config — Phase 7.10.
//
// Runs the e2e suite against `pnpm preview` (Vite's
// production-build server). The harness is wired here +
// committed; CI installs the browser binaries on demand
// (`npx playwright install` runs in nightly.yml only since
// the install adds ~250 MB and isn't worth bloating per-PR
// runs).
//
// Local: `pnpm exec playwright install chromium` once, then
// `pnpm test:e2e`.

import { defineConfig, devices } from '@playwright/test';

const BASE_URL = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:4173';

export default defineConfig({
  testDir: './tests/e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: BASE_URL,
    trace: 'on-first-retry',
  },
  webServer: {
    // Build is assumed to have run already (CI does
    // `pnpm build` before invoking this config); preview is
    // Vite's static server on :4173.
    //
    // --host 127.0.0.1 is load-bearing: without an explicit
    // host, Vite resolves the string "localhost" via DNS to
    // bind its listener. On GitHub Actions' ubuntu-latest
    // runners /etc/hosts maps "localhost" to both 127.0.0.1
    // and ::1, and Node's resolver can hand back the IPv6
    // entry first — so the preview server ends up listening
    // only on ::1 while this config's readiness probe (and
    // BASE_URL above) hits the literal 127.0.0.1. The probe
    // then gets ECONNREFUSED for the full timeout window with
    // no diagnostic output, since the server never fails to
    // start — it's just unreachable on the address being
    // polled. Pinning both sides to the literal IPv4 address
    // removes the DNS step entirely.
    command: 'pnpm preview --host 127.0.0.1 --port 4173 --strictPort',
    url: BASE_URL,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});
