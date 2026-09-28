import { expect, test } from "playwright/test";
import { enginePost, hasServiceCall, mockHaCalls, replay, waitForMockHaCall } from "./helpers";

test("custom gesture can be recorded, mapped, replayed, and sent to HA", async ({ page, request }) => {
  await page.goto("/gestures/new");
  await page.getByLabel("Gesture name").fill("Zorro light toggle");
  await page.getByRole("radio", { name: "Motion" }).click();

  for (let take = 1; take <= 3; take += 1) {
    await page.getByRole("button", { name: take === 1 ? "Start countdown" : "Record another take" }).click();
    await expect(page.getByText(`${take}/3 takes captured`)).toBeVisible({ timeout: 8_000 });
  }

  await page.getByRole("button", { name: "Train classifier" }).click();
  await expect(page.getByText(/Learned in/)).toBeVisible();
  await page.getByRole("button", { name: "Save gesture" }).click();
  await expect(page.getByText(/Saved Zorro light toggle/)).toBeVisible();

  await enginePost(request, "/api/v1/mappings", {
    name: "Zorro light toggle → Bed light",
    gesture_id: "motion.01J00000000000000000000003",
    hand: "any",
    camera_ids: [],
    target_mode: "global",
    mode: "tap",
    action: {
      kind: "call_service",
      domain: "light",
      service: "toggle",
      target: { entity_id: ["light.bed_light"] },
      data: {},
      preset: "light.toggle",
    },
    sensitive_ack: false,
  });

  await page.goto("/mappings");
  await page.getByRole("tab", { name: "Global" }).click();
  await expect(page.getByText(/Bed light/).first()).toBeVisible();

  const baseline = (await mockHaCalls(request)).length;
  await replay(request, "landmarks/custom_motion");
  await waitForMockHaCall(request, baseline, hasServiceCall("light", "toggle", "light.bed_light"));
});
