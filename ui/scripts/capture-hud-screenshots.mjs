import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright";

const baseUrl = process.env.UI_SCREENSHOT_URL ?? "http://127.0.0.1:5173/hud.html?mock=1";
const outputDir = resolve(process.cwd(), "../docs/design/screenshots/ui");
await mkdir(outputDir, { recursive: true });

const states = [
  "aiming",
  "selected",
  "ambiguous",
  "candidate",
  "armed",
  "sent",
  "done",
  "failed",
  "confirm",
  "dial",
  "camera-moved",
  "paused",
];

const browser = await chromium.launch();
for (const state of states) {
  const context = await browser.newContext({ viewport: { width: 480, height: 180 }, deviceScaleFactor: 1 });
  await context.addInitScript(() => {
    window.localStorage.setItem("flick.theme", "dark");
    window.localStorage.setItem("flick.highContrast", "0");
  });
  const page = await context.newPage();
  await page.goto(`${baseUrl}&state=${state}`, { waitUntil: "networkidle" });
  await page.screenshot({ path: resolve(outputDir, `hud-${state}.png`), omitBackground: true });
  await context.close();
}
await browser.close();
