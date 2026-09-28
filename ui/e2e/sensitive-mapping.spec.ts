import { expect, test } from "playwright/test";
import { authHeaders, engineUrl } from "./helpers";

test("sensitive mapping remains blocked until safety, ack, and confirm gesture are set", async ({
  page,
  request,
}) => {
  await page.goto("/mappings/new");
  await page.getByRole("switch", { name: "Mark mapping as sensitive" }).click();
  await expect(page.getByRole("button", { name: "Resolve checks" })).toBeDisabled();

  const blocked = await request.post(`${engineUrl}/api/v1/ha/call`, {
    headers: authHeaders,
    data: {
      kind: "call_service",
      domain: "lock",
      service: "unlock",
      target: { entity_id: ["lock.front_door"] },
      data: {},
      preset: "lock.unlock",
    },
  });
  expect(blocked.status()).toBe(422);
  const problem = await blocked.json();
  expect(problem.code).toBe("safety_blocked");

  const enableSafety = page.getByRole("button", { name: "Enable Safety setting" });
  if (await enableSafety.isVisible()) {
    await enableSafety.click();
  }
  await expect(page.getByText("Safety enabled")).toBeVisible();
  await page.getByLabel("I understand this sends a sensitive Home Assistant action.").check();
  await expect(page.getByRole("button", { name: "Enable mapping" })).toBeEnabled();
});
