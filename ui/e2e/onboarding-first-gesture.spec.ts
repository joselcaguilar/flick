import { expect, test } from "playwright/test";
import { hasServiceCall, mockHaCalls, replay, waitForMockHaCall } from "./helpers";

test("onboarding connects mock HA, fires first gesture, and supports demo mode", async ({
  page,
  request,
}) => {
  await page.goto("/onboarding");
  await page.getByRole("button", { name: "Get started" }).click();
  await page.getByRole("button", { name: "Continue" }).click();
  await page.getByRole("button", { name: /Mock Home/ }).click();
  await page.getByRole("button", { name: "Connect" }).click();
  await expect(page.getByText(/Connected to Home/)).toBeVisible();
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByRole("heading", { name: "Try your first flick." })).toBeVisible();

  const baseline = (await mockHaCalls(request)).length;
  await replay(request, "landmarks/thumb_up");
  await waitForMockHaCall(request, baseline, hasServiceCall("light", "toggle", "light.bed_light"));

  await page.goto("/onboarding");
  await page.getByRole("button", { name: "3 Home Assistant" }).click();
  const beforeDemo = (await mockHaCalls(request)).length;
  await page.getByRole("button", { name: "Skip HA for demo mode" }).click();
  await page.getByRole("button", { name: "Try it" }).click();
  await expect(page.getByText(/Demo HUD · no action sent/)).toBeVisible();
  await expect.poll(async () => (await mockHaCalls(request)).length).toBe(beforeDemo);
});
