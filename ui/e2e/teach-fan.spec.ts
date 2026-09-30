import { expect, test } from "playwright/test";
import { expectHudDone, hasServiceCall, mockHaCalls, replay, waitForMockHaCall } from "./helpers";

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
  await expect(page.getByText(/Point at Ventilador Dormitorio/i)).toBeVisible();

  await replay(request, "targeting/two_anchors_25deg");
  await page.getByRole("button", { name: "Capture spot 1" }).click();
  await expect(page.getByRole("button", { name: "Capture spot 2" })).toBeVisible();
  await replay(request, "targeting/two_anchors_25deg");
  await page.getByRole("button", { name: "Capture spot 2" }).click();
  await expect(page.getByRole("button", { name: "Use current speed" })).toBeVisible();
  await page.getByRole("button", { name: "Use current speed" }).click();
  await expect(page.getByText(/Speed 1/).first()).toBeVisible();
  await page.getByRole("button", { name: "Continue to verbs" }).click();
  await page.getByRole("button", { name: "Commit taught device" }).click();
  await expect(page.getByRole("heading", { name: /is ready/ })).toBeVisible();
  await page.getByRole("link", { name: "View mappings" }).click();
  await expect(page.getByText(/Point at Ventilador Dormitorio/i).first()).toBeVisible();

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
