import { test } from "playwright/test";
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
  request,
}) => {
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
  // Also outlasts the mapping's 1 s cooldown, so the next replay is judged on safety alone.
  await expectNoMockHaCall(request, baseline, 1_250);

  await enginePatch(request, "/api/v1/settings", { "safety.allow_sensitive": true });
  await enginePatch(request, `/api/v1/mappings/${mapping.id}`, { sensitive_ack: true });
  await replay(request, "landmarks/sensitive_motion");
  await expectNoMockHaCall(request, baseline);

  await replay(request, "landmarks/thumb_up");
  await waitForMockHaCall(request, baseline, hasServiceCall("cover", "open_cover", "cover.garage"));
});
