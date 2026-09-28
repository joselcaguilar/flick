import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";

const baseUrl = process.env.UI_SCREENSHOT_URL ?? "http://127.0.0.1:5173/?mock=1";
const outputDir = resolve(process.cwd(), "../docs/design/screenshots/ui");
await mkdir(outputDir, { recursive: true });

const cases = [
  { name: "desktop-light", width: 1440, height: 900, theme: "light" },
  { name: "desktop-dark", width: 1440, height: 900, theme: "dark" },
  { name: "mobile-light", width: 390, height: 844, theme: "light" },
  { name: "mobile-dark", width: 390, height: 844, theme: "dark" },
];

const browser = await chromium.launch();
for (const item of cases) {
  const context = await browser.newContext({
    viewport: { width: item.width, height: item.height },
    deviceScaleFactor: 1,
  });
  await context.addInitScript((theme) => {
    window.localStorage.setItem("flick.theme", theme);
    window.localStorage.setItem("flick.highContrast", "0");
  }, item.theme);
  const page = await context.newPage();
  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.screenshot({ path: resolve(outputDir, `${item.name}.png`), fullPage: true });
  await context.close();
}
await browser.close();
