import { expect, test } from "playwright/test";
import { enginePost, expectHudDone, hasServiceCall, mockHaCalls, replay, waitForMockHaCall } from "./helpers";

test("teaches the fan and verifies circle and stop through HA, HUD, and activity", async ({
  page,
  request,
}) => {
  const context = page.context();
  const hud = await context.newPage();
  const activity = await context.newPage();
  await hud.goto("/hud.html");
  await activity.goto("/activity");

  await page.goto("/devices/teach");
  await expect(page.getByRole("heading", { name: "Teach a device" })).toBeVisible();
  await expect(page.getByText(/Ventilador Dormitorio/i).first()).toBeVisible();
  await page.getByRole("button", { name: "Start capture" }).click();
  await expect(page.getByText(/Point at Ventilador dormitorio/)).toBeVisible();

  await replay(request, "targeting/two_anchors_25deg");
  const session = await enginePost<{ id: string }>(request, "/api/v1/teach", {
    camera_id: "01J00000000000000000000001",
    target: { entity_id: "fan.ventilador_dormitorio" },
  });
  await enginePost(request, `/api/v1/teach/${session.id}/spot`, undefined);
  await enginePost(request, `/api/v1/teach/${session.id}/spot`, undefined);
  await enginePost(request, `/api/v1/teach/${session.id}/levels/use-current`, { level: 1 });
  await enginePost(request, `/api/v1/teach/${session.id}/commit`, {
    name: "Ventilador dormitorio",
    verbs: [
      { gesture_id: "builtin.circle_cw", action: { kind: "verb", verb: "level_set", level: 1 } },
      { gesture_id: "builtin.two_hand_separate", action: { kind: "verb", verb: "off" } },
    ],
  });

  const beforeCircle = (await mockHaCalls(request)).length;
  await replay(request, "targeting/point_fan_circle");
  await waitForMockHaCall(
    request,
    beforeCircle,
    hasServiceCall("fan", "turn_on", "fan.ventilador_dormitorio", { percentage: 1 }),
  );
  await expectHudDone(hud, /Fan → speed 1|Fan circle sets speed 1|Ventilador dormitorio/i);
  await expect(
    activity.getByText(/Fan → speed 1|Fan circle sets speed 1|Ventilador dormitorio/i).first(),
  ).toBeVisible();

  const beforeStop = (await mockHaCalls(request)).length;
  await replay(request, "targeting/point_fan_stop");
  await waitForMockHaCall(
    request,
    beforeStop,
    hasServiceCall("fan", "turn_off", "fan.ventilador_dormitorio"),
  );
  await expectHudDone(hud, /Fan → off|Fan → stop|Fan separate stops|Ventilador dormitorio/i);
  await expect(
    activity.getByText(/Fan → off|Fan → stop|Fan separate stops|Ventilador dormitorio/i).first(),
  ).toBeVisible();

  await hud.close();
  await activity.close();
});
