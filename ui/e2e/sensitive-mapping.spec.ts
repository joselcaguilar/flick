import { expect, test } from "playwright/test";
import {
  enginePatch,
  enginePost,
  expectNoMockHaCall,
  hasServiceCall,
  mockHaCalls,
  replay,
  waitForMockHaCall,
} from "./helpers";

test("sensitive mapping remains blocked until safety, ack, and confirm gesture are set", async ({
  page,
  request,
}) => {
  await page.goto("/mappings/new");
  await page.getByRole("switch", { name: "Mark mapping as sensitive" }).click();
  await expect(page.getByRole("button", { name: "Resolve checks" })).toBeDisabled();

  const mapping = await enginePost<{ id: string }>(request, "/api/v1/mappings", {
    name: "Garage open requires safety",
    gesture_id: "motion.01J00000000000000000000004",
    hand: "any",
    camera_ids: [],
    target_mode: "global",
    mode: "tap",
    action: {
      kind: "call_service",
      domain: "cover",
      service: "open_cover",
      target: { entity_id: ["cover.garage"] },
      data: {},
      preset: "cover.open_cover",
    },
    sensitive_ack: false,
    confirm_gesture_id: "builtin.thumb_up",
  });

  const baseline = (await mockHaCalls(request)).length;
  await replay(request, "landmarks/sensitive_motion");
  await expectNoMockHaCall(request, baseline);

  const enableSafety = page.getByRole("button", { name: "Enable Safety setting" });
  await page.goto("/mappings/new");
  await page.getByRole("switch", { name: "Mark mapping as sensitive" }).click();
  if (await enableSafety.isVisible()) {
    await enableSafety.click();
  }
  await expect(page.getByText("Safety enabled")).toBeVisible();
  await page.getByLabel("I understand this sends a sensitive Home Assistant action.").check();
  await expect(page.getByRole("button", { name: "Enable mapping" })).toBeEnabled();

  await enginePatch(request, `/api/v1/mappings/${mapping.id}`, { sensitive_ack: true });
  await replay(request, "landmarks/sensitive_motion");
  await expectNoMockHaCall(request, baseline);

  await replay(request, "landmarks/thumb_up");
  await waitForMockHaCall(request, baseline, hasServiceCall("cover", "open_cover", "cover.garage"));
});
